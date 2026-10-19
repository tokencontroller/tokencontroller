//! The MCP server: `POST /api/v1/mcp` speaks the Model Context Protocol
//! (JSON-RPC over streamable HTTP, plain JSON answers) so an agent can read
//! the organization: tag values, attributes, people, plans and reports. It
//! only reads.
//!
//! Sign in with a read token (`tc_read_`) or a session. The tools see what
//! that person's grants cover.
//!
//! To add a tool, add it to [`tools`] and to [`call`].
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
use time::OffsetDateTime;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::AppState;
use crate::error::ApiError;
use crate::events::{Log, access};
use crate::sign_in::{Caller, CredentialKind};
use crate::{organizations, tags};

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
        "tools/list" => Ok(json!({"tools": tools()})),
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
        "instructions": "Reads one Token Controller organization: settings, tag values, attributes, people, plans and the cost report. Terms follow the glossary at docs.tokencontroller.com/glossary."
    })
}

/// Every tool, as `tools/list` shows it.
fn tools() -> Value {
    let no_arguments = json!({"type": "object", "properties": {}, "additionalProperties": false});
    json!([
        {
            "name": "organization",
            "title": "Organization",
            "description": "The organization's settings, cutoff, tag and attribute names and the price table in force, with the caller's own rules.",
            "inputSchema": no_arguments,
            "annotations": {"readOnlyHint": true}
        },
        {
            "name": "tag_values",
            "title": "Tag values",
            "description": "Tag values the caller may see, with title, description, source id and defaults. Filter by tag name such as project, feature or client, or by text in the key or title.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "A tag name, such as feature"},
                    "query": {"type": "string", "description": "Text in the key or title, such as KD-214"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 500, "default": 100}
                },
                "additionalProperties": false
            },
            "annotations": {"readOnlyHint": true}
        },
        {
            "name": "attributes",
            "title": "Attributes",
            "description": "Attribute values such as teams and departments with their defaults.",
            "inputSchema": no_arguments,
            "annotations": {"readOnlyHint": true}
        },
        {
            "name": "people",
            "title": "People",
            "description": "Everyone in the organization with their role, attributes such as team, and a people manager's filters.",
            "inputSchema": no_arguments,
            "annotations": {"readOnlyHint": true}
        },
        {
            "name": "plans",
            "title": "Plans",
            "description": "The accepted estimates the caller may see, each with its cost and deviation, and the shares of people in projects.",
            "inputSchema": no_arguments,
            "annotations": {"readOnlyHint": true}
        },
        {
            "name": "report",
            "title": "Cost report",
            "description": "C(F, w): the cost of a filter over a date range, grouped by a tag or attribute name, person, label, agent_harness, agent_name, model or day, inside the caller's view grants.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "from": {"type": "string", "description": "The first day, YYYY-MM-DD"},
                    "to": {"type": "string", "description": "The last day, YYYY-MM-DD"},
                    "by": {"type": "string", "description": "A tag or attribute name, or person, label, agent_harness, agent_name, model or day"},
                    "filter": {"type": "string", "description": "A filter, such as client:acme,team:web"},
                    "as_of": {"type": "string", "description": "The moment to read the log up to, RFC 3339"},
                    "currency": {"type": "string", "description": "Show money in this currency, such as EUR"},
                    "approved": {"type": "boolean", "description": "Approved messages only"}
                },
                "required": ["from", "to"],
                "additionalProperties": false
            },
            "annotations": {"readOnlyHint": true}
        }
    ])
}

fn text_argument(arguments: &Value, name: &str) -> Option<String> {
    arguments
        .get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

async fn call(
    state: &AppState,
    caller: &Caller,
    name: &str,
    arguments: &Value,
) -> Result<Value, RpcError> {
    let organization_id = caller.organization_id;
    let person = caller.person()?;
    let log = Log::load(state, organization_id).await?;
    let fold = log.fold(OffsetDateTime::now_utc())?;
    let value = match name {
        "organization" => {
            let organization = organizations::organization(&state.db, organization_id).await?;
            json!({
                "organization": organization,
                "cutoff": crate::cutoff::view(&log, fold.as_of()),
                "tag_names": tags::names(&state.db, organization_id, tags::Kind::Tag).await?
                    .into_iter().map(|(name, built_in)| json!({"name": name, "built_in": built_in})).collect::<Vec<_>>(),
                "attribute_names": tags::names(&state.db, organization_id, tags::Kind::Attribute).await?
                    .into_iter().map(|(name, built_in)| json!({"name": name, "built_in": built_in})).collect::<Vec<_>>(),
                "rules": organizations::rules::views(&fold, Some(&person)),
                "price_table": crate::price_table::rows_view(&log, &fold),
            })
        }
        "tag_values" => {
            let name = text_argument(arguments, "name");
            let query = text_argument(arguments, "query").map(|query| query.to_lowercase());
            let limit = arguments
                .get("limit")
                .and_then(Value::as_u64)
                .and_then(|limit| usize::try_from(limit).ok())
                .unwrap_or(100)
                .clamp(1, 500);
            let values: Vec<_> = tags::values_view(&log, &fold, &person)
                .into_iter()
                .filter(|value| name.as_deref().is_none_or(|name| value.name == name))
                .filter(|value| {
                    query.as_deref().is_none_or(|query| {
                        value.key.to_lowercase().contains(query)
                            || value
                                .title
                                .as_deref()
                                .is_some_and(|title| title.to_lowercase().contains(query))
                    })
                })
                .take(limit)
                .collect();
            json!(values)
        }
        "attributes" => json!(tags::attributes::values_view(&log, &fold)),
        "people" => json!(organizations::members(state, organization_id).await?),
        "plans" => {
            let mut estimates = Vec::new();
            for recorded in fold.estimates() {
                let accepted = fold
                    .accepted(&recorded.value.tags)
                    .is_some_and(|accepted| accepted.id == recorded.id);
                if !accepted || !access::can_view(&fold, &person, &recorded.value.tags) {
                    continue;
                }
                estimates.push(crate::plans::view(
                    &fold,
                    &recorded.id,
                    recorded.recorded_at,
                    &recorded.value,
                    None,
                )?);
            }
            json!({
                "plans": estimates,
                "shares": crate::plans::share_views(&log, &fold, &person),
            })
        }
        "report" => {
            let query = crate::reports::ReportQuery {
                from: text_argument(arguments, "from"),
                to: text_argument(arguments, "to"),
                as_of: text_argument(arguments, "as_of"),
                by: text_argument(arguments, "by"),
                filter: text_argument(arguments, "filter"),
                approved: arguments.get("approved").and_then(Value::as_bool),
                currency: text_argument(arguments, "currency"),
                view: None,
                format: None,
            };
            json!(crate::reports::cost(state, caller, &query).await?)
        }
        other => return Err(RpcError::invalid_params(format!("unknown tool {other}"))),
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
        assert_eq!(tools.len(), 6);
        for tool in tools {
            assert_eq!(tool["annotations"]["readOnlyHint"], true);
            assert_eq!(tool["inputSchema"]["type"], "object");
        }
    }
}
