//! The MCP server: `POST /api/v1/mcp` speaks the Model Context Protocol
//! (JSON-RPC over streamable HTTP, plain JSON answers) so an agent can read
//! the organization: its settings and tag map, members, projects, clients
//! and tickets. It only reads.
//!
//! Sign in with a read token (`tc_read_`) or a session. The tools see what
//! that person's role sees.
//!
//! To add a tool, add it to [`tools`] and to [`call`]. The operators of the
//! hosted service also get the customer relations tools
//! ([`customer_relations::operator_tools`]).
//!
//! Owner: the service platform work. Keep this area's code, routes and queries in
//! this folder, and its migrations in `migrations/` named
//! `<timestamp>_mcp_<what>.sql`.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{Value, json};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::AppState;
use crate::error::ApiError;
use crate::sign_in::{Caller, CredentialKind};
use crate::{customer_relations, organizations, projects, tags};

/// Protocol versions this server speaks.
const PROTOCOL_VERSIONS: [&str; 3] = [LATEST_PROTOCOL_VERSION, "2025-03-26", "2024-11-05"];
/// The version offered to clients that ask for one this server does not know.
const LATEST_PROTOCOL_VERSION: &str = "2025-06-18";

/// This area's routes, relative to `/api/v1`.
#[must_use]
pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(mcp))
}

/// One JSON-RPC message.
#[derive(Debug, Deserialize)]
struct Request {
    /// Absent on notifications.
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

/// A JSON-RPC error.
struct RpcError {
    code: i64,
    message: String,
}

impl RpcError {
    fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: message.into(),
        }
    }
}

impl From<ApiError> for RpcError {
    fn from(error: ApiError) -> Self {
        if let ApiError::BadRequest(message) = error {
            return Self::invalid_params(message);
        }
        let message = error.to_string();
        if matches!(error, ApiError::Database(_) | ApiError::Internal(_)) {
            tracing::error!("MCP tool failed: {error}");
        }
        Self {
            code: -32603,
            message,
        }
    }
}

/// The MCP endpoint. Send JSON-RPC messages with a read token or a session.
#[utoipa::path(
    post,
    path = "/mcp",
    tag = "mcp",
    request_body(content = Object, description = "A JSON-RPC 2.0 message of the Model Context Protocol"),
    responses(
        (status = OK, description = "The JSON-RPC answer", body = Object),
        (status = ACCEPTED, description = "A notification was received"),
        (status = UNAUTHORIZED, body = crate::error::ErrorBody),
    ),
)]
async fn mcp(
    State(state): State<AppState>,
    caller: Caller,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    caller.require_credential(&[CredentialKind::Session, CredentialKind::Read])?;
    if caller.person_removed {
        return Err(ApiError::Unauthorized);
    }
    let Ok(request) = serde_json::from_value::<Request>(body) else {
        return Ok(Json(json!({
            "jsonrpc": "2.0",
            "id": null,
            "error": {"code": -32600, "message": "not a JSON-RPC request"}
        }))
        .into_response());
    };
    let Some(id) = request.id else {
        // Notifications, such as `notifications/initialized`, get no answer.
        return Ok(StatusCode::ACCEPTED.into_response());
    };
    let answer = match handle(&state, &caller, &request.method, request.params).await {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err(error) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": error.code, "message": error.message}
        }),
    };
    Ok(Json(answer).into_response())
}

async fn handle(
    state: &AppState,
    caller: &Caller,
    method: &str,
    params: Value,
) -> Result<Value, RpcError> {
    match method {
        "initialize" => Ok(initialize(&params)),
        "ping" => Ok(json!({})),
        "tools/list" => {
            let mut all = tools();
            if customer_relations::is_operator(state, caller.person_id).await?
                && let Some(list) = all.as_array_mut()
            {
                list.extend(customer_relations::operator_tools());
            }
            Ok(json!({"tools": all}))
        }
        "tools/call" => {
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| RpcError::invalid_params("tools/call needs a tool name"))?;
            let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
            let result = call(state, caller, name, &arguments).await?;
            let text = serde_json::to_string_pretty(&result).unwrap_or_default();
            Ok(json!({
                "content": [{"type": "text", "text": text}],
                "structuredContent": {"result": result},
                "isError": false
            }))
        }
        other => Err(RpcError {
            code: -32601,
            message: format!("unknown method {other}"),
        }),
    }
}

fn initialize(params: &Value) -> Value {
    let asked = params.get("protocolVersion").and_then(Value::as_str);
    let version = asked
        .and_then(|asked| PROTOCOL_VERSIONS.iter().find(|version| **version == asked))
        .copied()
        .unwrap_or(LATEST_PROTOCOL_VERSION);
    json!({
        "protocolVersion": version,
        "capabilities": {"tools": {"listChanged": false}},
        "serverInfo": {"name": "token-controller", "title": "Token Controller", "version": env!("CARGO_PKG_VERSION")},
        "instructions": "Reads one Token Controller organization: settings, tag names and tag map, members, projects, clients and tickets. Terms follow the glossary at docs.tokencontroller.com/glossary."
    })
}

/// Every tool, as `tools/list` shows it.
fn tools() -> Value {
    let no_arguments = json!({"type": "object", "properties": {}, "additionalProperties": false});
    json!([
        {
            "name": "organization",
            "title": "Organization settings",
            "description": "The organization's settings: cost source, reporting currency, approval, sign-in domains, tag names and the tag map in force.",
            "inputSchema": no_arguments,
            "annotations": {"readOnlyHint": true}
        },
        {
            "name": "members",
            "title": "Members",
            "description": "Everyone in the organization with their role, tags such as team, and the filters assigned to people managers.",
            "inputSchema": no_arguments,
            "annotations": {"readOnlyHint": true}
        },
        {
            "name": "projects",
            "title": "Projects",
            "description": "Projects with their ticket key pattern, repositories and tags such as client.",
            "inputSchema": no_arguments,
            "annotations": {"readOnlyHint": true}
        },
        {
            "name": "clients",
            "title": "Clients",
            "description": "Values of the client tag on projects, with their projects.",
            "inputSchema": no_arguments,
            "annotations": {"readOnlyHint": true}
        },
        {
            "name": "tickets",
            "title": "Tickets",
            "description": "Tickets by key, with title, project, parent and assignee. Filter by project name or by text in the key or title.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project": {"type": "string", "description": "A project name, such as storefront"},
                    "query": {"type": "string", "description": "Text in the ticket key or title, such as KD-214"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 500, "default": 100}
                },
                "additionalProperties": false
            },
            "annotations": {"readOnlyHint": true}
        }
    ])
}

async fn call(
    state: &AppState,
    caller: &Caller,
    name: &str,
    arguments: &Value,
) -> Result<Value, RpcError> {
    let db = &state.db;
    let organization_id = caller.organization_id;
    let value = match name {
        "organization" => json!({
            "organization": organizations::organization(db, organization_id).await?,
            "tag_names": tags::tag_names(db, organization_id).await.map_err(ApiError::from)?,
            "tag_map": tags::current_tag_map(db, organization_id).await.map_err(ApiError::from)?,
        }),
        "members" => json!(organizations::members(db, organization_id).await?),
        "projects" => json!(projects::projects(db, organization_id).await?),
        "clients" => {
            let all = projects::projects(db, organization_id).await?;
            let mut clients = std::collections::BTreeMap::<String, Vec<String>>::new();
            for project in all {
                if let Some(client) = project.tags.get("client") {
                    clients
                        .entry(client.clone())
                        .or_default()
                        .push(project.name);
                }
            }
            json!(clients)
        }
        "tickets" => {
            let project = arguments.get("project").and_then(Value::as_str);
            let project_id = match project {
                Some(project) => Some(
                    projects::projects(db, organization_id)
                        .await?
                        .into_iter()
                        .find(|candidate| candidate.name == project)
                        .map(|found| found.id)
                        .ok_or_else(|| {
                            RpcError::invalid_params(format!("no project named {project}"))
                        })?,
                ),
                None => None,
            };
            let query = arguments
                .get("query")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let limit = arguments
                .get("limit")
                .and_then(Value::as_i64)
                .unwrap_or(100);
            json!(projects::tickets(db, organization_id, project_id, query, limit).await?)
        }
        other => {
            let operator_tool = if customer_relations::is_operator(state, caller.person_id).await? {
                customer_relations::call_operator_tool(state, other, arguments).await?
            } else {
                None
            };
            operator_tool
                .ok_or_else(|| RpcError::invalid_params(format!("unknown tool {other}")))?
        }
    };
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_newest_known_protocol_version_is_offered() {
        let answer = initialize(&json!({"protocolVersion": "2025-03-26"}));
        assert_eq!(answer["protocolVersion"], "2025-03-26");
        let answer = initialize(&json!({"protocolVersion": "1999-01-01"}));
        assert_eq!(answer["protocolVersion"], "2025-06-18");
        assert_eq!(answer["serverInfo"]["name"], "token-controller");
    }

    #[test]
    fn every_tool_only_reads_and_has_a_schema() {
        let tools = tools();
        let tools = tools.as_array().unwrap();
        assert_eq!(tools.len(), 5);
        for tool in tools {
            assert_eq!(tool["annotations"]["readOnlyHint"], true);
            assert_eq!(tool["inputSchema"]["type"], "object");
        }
    }
}
