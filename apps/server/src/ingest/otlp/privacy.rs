//! The prompt text rule: prompt text never enters. An event or span that
//! carries prompt, completion or message text, tool input or output, or
//! system instructions is dropped whole, before anything is read from it,
//! unless it is a model call: a model call loses every attribute and the body
//! that carry text, and keeps its token counts (ADR 0021).

use opentelemetry_proto::tonic::common::v1::any_value::Value;
use opentelemetry_proto::tonic::common::v1::{AnyValue, KeyValue};

/// Attribute names that hold prompt, completion or message text, tool input
/// or output, or system instructions, in the telemetry of the agent harnesses
/// and in the OpenTelemetry `GenAI` conventions. An event or span with one of
/// them, at any depth, is dropped whole unless the value is empty or
/// redacted.
pub const PROMPT_TEXT_ATTRIBUTES: &[&str] = &[
    // OpenTelemetry GenAI conventions, current and deprecated.
    "gen_ai.input.messages",
    "gen_ai.output.messages",
    "gen_ai.system_instructions",
    "gen_ai.tool.definitions",
    "gen_ai.tool.call.arguments",
    "gen_ai.tool.call.result",
    "gen_ai.retrieval.query.text",
    "gen_ai.retrieval.documents",
    "gen_ai.memory.query.text",
    "gen_ai.memory.records",
    "gen_ai.prompt",
    "gen_ai.completion",
    // `Claude Code` events and spans.
    "prompt",
    "prompt_text",
    "user_prompt",
    "response",
    "system_prompt",
    "system_prompt_preview",
    "user_system_prompt",
    "system_reminders",
    "new_context",
    "response.model_output",
    "tool_parameters",
    "tool_input",
    "content",
    "output",
    "diff",
    "bash_command",
    "body",
    // Codex CLI events.
    "arguments",
    "rationale",
    // Gemini CLI events.
    "request_text",
    "response_text",
    "function_args",
    // Copilot CLI spans and span events.
    "copilot_chat.hook_input",
    "copilot_chat.hook_output",
    "github.copilot.tool.parameters.command",
    "github.copilot.message",
];

/// Attribute name prefixes of the same kind: prompt template variables and
/// the numbered `gen_ai.completion.0.content` form some libraries write.
/// The numbered `gen_ai.prompt.0.content` form is matched by its digit, as
/// `gen_ai.prompt.name` names a template and holds no text.
const PROMPT_TEXT_ATTRIBUTE_PREFIXES: &[&str] = &["gen_ai.prompt.variable.", "gen_ai.completion."];

/// Log event names whose body is the message text itself.
pub const PROMPT_TEXT_EVENTS: &[&str] = &[
    "cursor.conversation.user_message",
    "cursor.conversation.assistant_message",
    "cursor.conversation.tool_io",
    "gen_ai.system.message",
    "gen_ai.user.message",
    "gen_ai.assistant.message",
    "gen_ai.tool.message",
    "gen_ai.choice",
    "gen_ai.content.prompt",
    "gen_ai.content.completion",
];

/// What agent harnesses send in place of text they were told to keep back.
const REDACTION_MARKERS: &[&str] = &["<REDACTED>", "[REDACTED]"];

/// Whether a log record or span carries prompt text: in any of its attribute
/// lists (a span's events included), or in its log body. A string body is
/// the event's own label in every agent harness (`claude_code.api_request`,
/// `api_request`, `API response from …`), so it counts only on the events in
/// [`PROMPT_TEXT_EVENTS`]; a map, list or bytes body counts when it holds any
/// text.
pub(super) fn carries_prompt_text<'a>(
    attribute_lists: impl IntoIterator<Item = &'a [KeyValue]>,
    body: Option<&AnyValue>,
    event_names: &[&str],
) -> bool {
    attribute_lists.into_iter().any(attributes_carry_text)
        || body_carries_prompt_text(body, event_names)
}

/// Whether a log body carries prompt text, as [`carries_prompt_text`] reads it.
pub(super) fn body_carries_prompt_text(body: Option<&AnyValue>, event_names: &[&str]) -> bool {
    body.and_then(|body| body.value.as_ref())
        .is_some_and(|body| body_carries_text(body, event_names))
}

/// The attributes without each one that carries prompt text: what a model
/// call keeps of an event or span that carried some.
pub(super) fn without_prompt_text(attributes: &[KeyValue]) -> Vec<KeyValue> {
    attributes
        .iter()
        .filter(|attribute| !attribute_carries_text(attribute))
        .cloned()
        .collect()
}

fn body_carries_text(body: &Value, event_names: &[&str]) -> bool {
    match body {
        Value::StringValue(_) => {
            event_names
                .iter()
                .any(|name| PROMPT_TEXT_EVENTS.contains(name))
                && holds_text(body)
        }
        Value::ArrayValue(_) | Value::KvlistValue(_) | Value::BytesValue(_) => holds_text(body),
        Value::BoolValue(_)
        | Value::IntValue(_)
        | Value::DoubleValue(_)
        | Value::StringValueStrindex(_) => false,
    }
}

fn attributes_carry_text(attributes: &[KeyValue]) -> bool {
    attributes.iter().any(attribute_carries_text)
}

fn attribute_carries_text(attribute: &KeyValue) -> bool {
    let Some(value) = attribute
        .value
        .as_ref()
        .and_then(|value| value.value.as_ref())
    else {
        return false;
    };
    (is_prompt_text_attribute(&attribute.key) && holds_text(value)) || nested_carry_text(value)
}

/// Maps inside attribute values, such as a `metadata` map, are checked by
/// their keys too.
fn nested_carry_text(value: &Value) -> bool {
    match value {
        Value::KvlistValue(list) => attributes_carry_text(&list.values),
        Value::ArrayValue(array) => array
            .values
            .iter()
            .filter_map(|item| item.value.as_ref())
            .any(nested_carry_text),
        Value::StringValue(_)
        | Value::BoolValue(_)
        | Value::IntValue(_)
        | Value::DoubleValue(_)
        | Value::BytesValue(_)
        | Value::StringValueStrindex(_) => false,
    }
}

fn is_prompt_text_attribute(key: &str) -> bool {
    PROMPT_TEXT_ATTRIBUTES.contains(&key)
        || PROMPT_TEXT_ATTRIBUTE_PREFIXES
            .iter()
            .any(|prefix| key.starts_with(prefix))
        || key
            .strip_prefix("gen_ai.prompt.")
            .and_then(|rest| rest.chars().next())
            .is_some_and(|first| first.is_ascii_digit())
}

/// Whether a value holds any text that is neither empty nor redacted.
fn holds_text(value: &Value) -> bool {
    match value {
        Value::StringValue(text) => {
            let text = text.trim();
            !text.is_empty() && !REDACTION_MARKERS.contains(&text)
        }
        Value::BytesValue(bytes) => !bytes.is_empty(),
        Value::ArrayValue(array) => array
            .values
            .iter()
            .filter_map(|item| item.value.as_ref())
            .any(holds_text),
        Value::KvlistValue(list) => list
            .values
            .iter()
            .filter_map(|entry| entry.value.as_ref().and_then(|value| value.value.as_ref()))
            .any(holds_text),
        Value::BoolValue(_)
        | Value::IntValue(_)
        | Value::DoubleValue(_)
        | Value::StringValueStrindex(_) => false,
    }
}
