//! The operators' tools on the MCP server (`/api/v1/mcp`): the admin area's
//! data for routines and agents that read what the operators see. Only a
//! caller whose address is in `TC_OPERATOR_EMAILS` sees or calls them; they
//! only read.

use serde_json::{Value, json};
use uuid::Uuid;

use super::admin::{self, Scope};
use super::{customers, feedback};
use crate::error::ApiError;
use crate::hosted::Hosted;

/// The operators' tools, as `tools/list` shows them to an operator.
#[must_use]
pub fn operator_tools() -> Vec<Value> {
    let no_arguments = json!({"type": "object", "properties": {}, "additionalProperties": false});
    vec![
        json!({
            "name": "customers",
            "title": "Customers",
            "description": "Operators only. Every organization on this instance with its plan, member count, open support conversations and unanswered feedback reports.",
            "inputSchema": no_arguments,
            "annotations": {"readOnlyHint": true}
        }),
        json!({
            "name": "customer_record",
            "title": "Customer record",
            "description": "Operators only. One organization's record: plan, members with newsletter status, support conversations, feedback reports and notes.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "organization_id": {"type": "string", "description": "The organization's id, from the customers tool"}
                },
                "required": ["organization_id"],
                "additionalProperties": false
            },
            "annotations": {"readOnlyHint": true}
        }),
        json!({
            "name": "support_waiting",
            "title": "Support waiting for a person",
            "description": "Operators only. Support conversations the AI support answerer handed to the operators, without their messages.",
            "inputSchema": no_arguments,
            "annotations": {"readOnlyHint": true}
        }),
        json!({
            "name": "assistant_missed",
            "title": "Questions the docs did not answer",
            "description": "Operators only. Questions to the site assistant of the last 90 days that the docs did not answer. Each one is a docs change to make.",
            "inputSchema": no_arguments,
            "annotations": {"readOnlyHint": true}
        }),
        json!({
            "name": "feedback_open",
            "title": "Unanswered feedback reports",
            "description": "Operators only. Feedback reports nobody answered yet, with their organization and sender, without screenshots.",
            "inputSchema": no_arguments,
            "annotations": {"readOnlyHint": true}
        }),
    ]
}

/// Runs operator tool `name`; `None` when no operator tool has that name.
/// The caller must be an operator ([`admin::is_operator`]).
///
/// # Errors
///
/// [`ApiError::BadRequest`] for a bad argument, [`ApiError::NotFound`] for an
/// unknown organization, or a database error.
pub async fn call_operator_tool(
    state: &Hosted,
    name: &str,
    arguments: &Value,
) -> Result<Option<Value>, ApiError> {
    let value = match name {
        "customers" => json!(customers::summaries(state, None).await?),
        "customer_record" => {
            let organization_id = arguments
                .get("organization_id")
                .and_then(Value::as_str)
                .and_then(|id| Uuid::parse_str(id).ok())
                .ok_or_else(|| {
                    ApiError::BadRequest(
                        "organization_id is an organization's id, from the customers tool."
                            .to_owned(),
                    )
                })?;
            json!(customers::record(state, organization_id).await?)
        }
        "support_waiting" => {
            json!(admin::conversations(&state.db, Some("escalated"), None).await?)
        }
        "assistant_missed" => json!(admin::questions(&state.db, Some(false)).await?),
        "feedback_open" => json!(
            feedback::received(
                &state.db,
                Scope::All,
                Some(feedback::Status::Received),
                None
            )
            .await?
        ),
        _ => return Ok(None),
    };
    Ok(Some(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_operator_tool_only_reads_and_has_a_schema() {
        let tools = operator_tools();
        assert_eq!(tools.len(), 5);
        for tool in tools {
            assert_eq!(tool["annotations"]["readOnlyHint"], true);
            assert_eq!(tool["inputSchema"]["type"], "object");
            assert!(
                tool["description"]
                    .as_str()
                    .unwrap()
                    .starts_with("Operators only.")
            );
        }
    }
}
