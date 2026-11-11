//! The content text rule (ADR 0028): content text (prompts, responses,
//! tool input and output, file contents, system instructions) is removed on
//! arrival. A record that carries some loses every attribute and the body
//! that carry it, and is read from the rest: a model call keeps its token
//! counts, any other record becomes an activity. The received record keeps
//! the text only when the organization switches keeping it on; the log
//! never holds it.

use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_proto::tonic::common::v1::any_value::Value;
use opentelemetry_proto::tonic::common::v1::{AnyValue, KeyValue};
use opentelemetry_proto::tonic::metrics::v1::metric::Data;

/// Attribute names that hold prompt, completion or message text, tool input
/// or output, or system instructions, in the telemetry of the agent harnesses
/// and in the OpenTelemetry `GenAI` conventions. An event or span with one of
/// them, at any depth, loses it unless the value is empty or redacted.
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

/// Whether a name is one that holds content text, as an attribute, a signal
/// or a session fact.
#[must_use]
pub fn is_content_text_name(name: &str) -> bool {
    is_prompt_text_attribute(name)
}

/// Removes content text from a logs export request, as the received record
/// keeps it when the organization keeps none: from the resource, from each
/// event's attributes and from a body that carries some. Answers how many
/// events lost something.
pub fn strip_logs(request: &mut ExportLogsServiceRequest) -> u32 {
    let mut stripped = 0_u32;
    for resource_logs in &mut request.resource_logs {
        if let Some(resource) = resource_logs.resource.as_mut() {
            resource.attributes = without_prompt_text(&resource.attributes);
        }
        for scope_logs in &mut resource_logs.scope_logs {
            for log in &mut scope_logs.log_records {
                let names = log_names(log);
                let names: Vec<&str> = names.iter().map(String::as_str).collect();
                let body_has_text = body_carries_prompt_text(log.body.as_ref(), &names);
                let attributes_have_text = attributes_carry_text(&log.attributes);
                if body_has_text {
                    log.body = None;
                }
                if attributes_have_text {
                    log.attributes = without_prompt_text(&log.attributes);
                }
                if body_has_text || attributes_have_text {
                    stripped = stripped.saturating_add(1);
                }
            }
        }
    }
    stripped
}

/// The names a log event goes by, as the extraction reads them.
fn log_names(log: &opentelemetry_proto::tonic::logs::v1::LogRecord) -> Vec<String> {
    let event_name = log
        .attributes
        .iter()
        .find(|attribute| attribute.key == "event.name")
        .and_then(|attribute| attribute.value.as_ref())
        .and_then(|value| match value.value.as_ref() {
            Some(Value::StringValue(text)) => Some(text.clone()),
            _ => None,
        });
    let body = log
        .body
        .as_ref()
        .and_then(|body| match body.value.as_ref() {
            Some(Value::StringValue(text)) => Some(text.clone()),
            _ => None,
        });
    [event_name, Some(log.event_name.clone()), body]
        .into_iter()
        .flatten()
        .filter(|name| !name.is_empty())
        .collect()
}

/// Removes content text from a traces export request: from the resource,
/// each span's attributes and its events' attributes. Answers how many
/// spans lost something.
pub fn strip_traces(request: &mut ExportTraceServiceRequest) -> u32 {
    let mut stripped = 0_u32;
    for resource_spans in &mut request.resource_spans {
        if let Some(resource) = resource_spans.resource.as_mut() {
            resource.attributes = without_prompt_text(&resource.attributes);
        }
        for scope_spans in &mut resource_spans.scope_spans {
            for span in &mut scope_spans.spans {
                let mut changed = attributes_carry_text(&span.attributes);
                span.attributes = without_prompt_text(&span.attributes);
                for event in &mut span.events {
                    changed = changed || attributes_carry_text(&event.attributes);
                    event.attributes = without_prompt_text(&event.attributes);
                }
                if changed {
                    stripped = stripped.saturating_add(1);
                }
            }
        }
    }
    stripped
}

/// Removes content text from a metrics export request: from the resource
/// and from each data point's attributes. Answers how many data points
/// lost something.
pub fn strip_metrics(request: &mut ExportMetricsServiceRequest) -> u32 {
    let mut stripped = 0_u32;
    let mut strip = |attributes: &mut Vec<KeyValue>| {
        if attributes_carry_text(attributes) {
            *attributes = without_prompt_text(attributes);
            stripped = stripped.saturating_add(1);
        }
    };
    for resource_metrics in &mut request.resource_metrics {
        if let Some(resource) = resource_metrics.resource.as_mut() {
            resource.attributes = without_prompt_text(&resource.attributes);
        }
        for scope_metrics in &mut resource_metrics.scope_metrics {
            for metric in &mut scope_metrics.metrics {
                match metric.data.as_mut() {
                    Some(Data::Sum(sum)) => {
                        for point in &mut sum.data_points {
                            strip(&mut point.attributes);
                        }
                    }
                    Some(Data::Gauge(gauge)) => {
                        for point in &mut gauge.data_points {
                            strip(&mut point.attributes);
                        }
                    }
                    Some(Data::Histogram(histogram)) => {
                        for point in &mut histogram.data_points {
                            strip(&mut point.attributes);
                        }
                    }
                    Some(Data::ExponentialHistogram(histogram)) => {
                        for point in &mut histogram.data_points {
                            strip(&mut point.attributes);
                        }
                    }
                    Some(Data::Summary(summary)) => {
                        for point in &mut summary.data_points {
                            strip(&mut point.attributes);
                        }
                    }
                    None => {}
                }
            }
        }
    }
    stripped
}
