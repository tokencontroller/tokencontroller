//! Which agent harness an event or span comes from, and how each one names
//! the parts of a model call. Each agent harness has one door for its model
//! calls, so a call is never counted twice: `Claude Code`, Codex CLI, Gemini
//! CLI and `Cursor` log one event per call, Copilot CLI writes one `chat`
//! span per call. Their other events and spans are not model calls.

use tokencontroller_core::Money;

use super::fields::Fields;
use crate::ingest::model::{TelemetryHarness, TokenCounts};

/// The model name stored when an agent harness reports none.
pub(super) const UNKNOWN_MODEL: &str = "unknown";

/// Name prefixes of events, spans and instrumentation scopes that tell the
/// agent harness apart, before the `service.name` resource attribute does.
const NAME_PREFIXES: &[(&str, TelemetryHarness)] = &[
    ("claude_code.", TelemetryHarness::ClaudeCode),
    ("com.anthropic.claude_code", TelemetryHarness::ClaudeCode),
    ("codex.", TelemetryHarness::CodexCli),
    ("gemini_cli.", TelemetryHarness::GeminiCli),
    ("github.copilot", TelemetryHarness::CopilotCli),
    ("cursor.", TelemetryHarness::Cursor),
];

/// The signal an agent harness sends its model calls on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Door {
    Logs,
    Spans,
}

/// The parts of a model call that each agent harness names its own way.
pub(super) struct Call {
    pub(super) session_id: Option<String>,
    pub(super) message_id: Option<String>,
    pub(super) model: String,
    pub(super) tokens: TokenCounts,
    pub(super) reported_cost: Option<Money>,
}

/// The agent harness of an event or span, from its names (event name, span
/// name, log body) and instrumentation scope, else from `service.name`.
pub(super) fn detect(
    names: &[&str],
    scope: &str,
    service_name: Option<&str>,
) -> Option<TelemetryHarness> {
    names
        .iter()
        .copied()
        .chain([scope])
        .find_map(|name| {
            NAME_PREFIXES
                .iter()
                .find(|(prefix, _)| name.starts_with(prefix))
                .map(|(_, harness)| *harness)
        })
        .or_else(|| service_name.and_then(by_service_name))
}

/// The `service.name` each agent harness sends by default.
fn by_service_name(service_name: &str) -> Option<TelemetryHarness> {
    let name = service_name.trim().to_ascii_lowercase();
    // `claude-code`, and `claude-code-desktop` from the desktop app.
    if name.starts_with("claude-code") {
        Some(TelemetryHarness::ClaudeCode)
    // Codex CLI names itself after its client: `codex_cli_rs`, `codex-tui`,
    // `codex_exec`, `codex_vscode` and so on.
    } else if name.starts_with("codex") {
        Some(TelemetryHarness::CodexCli)
    } else if name == "gemini-cli" {
        Some(TelemetryHarness::GeminiCli)
    } else if name == "github-copilot" {
        Some(TelemetryHarness::CopilotCli)
    } else if name == "cursor" {
        Some(TelemetryHarness::Cursor)
    } else {
        None
    }
}

/// Where an agent harness's model calls arrive.
pub(super) const fn door(harness: TelemetryHarness) -> Door {
    match harness {
        TelemetryHarness::CopilotCli => Door::Spans,
        TelemetryHarness::ClaudeCode
        | TelemetryHarness::CodexCli
        | TelemetryHarness::GeminiCli
        | TelemetryHarness::Cursor => Door::Logs,
    }
}

/// Reads a model call, or `None` when the event or span is not one or one
/// of its token counts cannot be read.
pub(super) fn read_call(
    harness: TelemetryHarness,
    names: &[&str],
    fields: &mut Fields<'_>,
) -> Option<Call> {
    match harness {
        TelemetryHarness::ClaudeCode => claude_code(names, fields),
        TelemetryHarness::CodexCli => codex_cli(names, fields),
        TelemetryHarness::GeminiCli => gemini_cli(names, fields),
        TelemetryHarness::CopilotCli => copilot_cli(fields),
        TelemetryHarness::Cursor => cursor(names, fields),
    }
}

/// `claude_code.api_request` (its `event.name` is `api_request`): input
/// without cache tokens, cache tokens apart, cost in dollars, and the API
/// request ID from version 2.1.214 on.
fn claude_code(names: &[&str], fields: &mut Fields<'_>) -> Option<Call> {
    if !names
        .iter()
        .any(|name| matches!(*name, "claude_code.api_request" | "api_request"))
    {
        return None;
    }
    let tokens = TokenCounts {
        input: required(fields, "input_tokens")?,
        output: required(fields, "output_tokens")?,
        cache_write: optional(fields, &["cache_creation_tokens"])?,
        cache_read: optional(fields, &["cache_read_tokens"])?,
    };
    let reported_cost = fields
        .take_dollars("cost_usd")
        .or_else(|| fields.take_micro_dollars("cost_usd_micros"));
    fields.discard(&["cost_usd_micros"]);
    Some(Call {
        session_id: fields.take_text(&["session.id"]),
        message_id: fields.take_text(&["request_id", "client_request_id"]),
        model: model(fields, &["model"]),
        tokens,
        reported_cost,
    })
}

/// `codex.sse_event` of kind `response.completed`. As in the OpenAI
/// Responses API, the input count includes cached and cache-write tokens and
/// the output count includes reasoning. It carries no cost and no response
/// ID.
fn codex_cli(names: &[&str], fields: &mut Fields<'_>) -> Option<Call> {
    if !names.contains(&"codex.sse_event")
        || fields.peek_text("event.kind") != Some("response.completed")
    {
        return None;
    }
    let input = required(fields, "input_token_count")?;
    let output = required(fields, "output_token_count")?;
    let cache_read = optional(fields, &["cached_token_count"])?;
    let cache_write = optional(fields, &["cache_write_token_count"])?;
    fields.discard(&["event.kind", "reasoning_token_count", "tool_token_count"]);
    Some(Call {
        session_id: fields.take_text(&["conversation.id"]),
        message_id: None,
        model: model(fields, &["model"]),
        tokens: TokenCounts {
            input: uncached(input, cache_read.checked_add(cache_write)?),
            output,
            cache_write,
            cache_read,
        },
        reported_cost: None,
    })
}

/// `gemini_cli.api_response`. As in the Gemini API's usage metadata, the
/// prompt count includes cached content, while thoughts and tool-use prompts
/// are counted apart. The `gen_ai.client.inference.operation.details` event
/// logged beside it repeats the same call and is not read.
fn gemini_cli(names: &[&str], fields: &mut Fields<'_>) -> Option<Call> {
    if !names.contains(&"gemini_cli.api_response") {
        return None;
    }
    let prompt = required(fields, "input_token_count")?;
    let candidates = required(fields, "output_token_count")?;
    let cached = optional(fields, &["cached_content_token_count"])?;
    let thoughts = optional(fields, &["thoughts_token_count"])?;
    let tool_use = optional(fields, &["tool_token_count"])?;
    fields.discard(&["total_token_count"]);
    Some(Call {
        session_id: fields.take_text(&["session.id"]),
        message_id: None,
        model: model(fields, &["model"]),
        tokens: TokenCounts {
            input: uncached(prompt, cached).checked_add(tool_use)?,
            output: candidates.checked_add(thoughts)?,
            cache_write: 0,
            cache_read: cached,
        },
        reported_cost: None,
    })
}

/// A `chat` span as the OpenTelemetry `GenAI` conventions describe it: the
/// input count includes cache reads and writes, and the output count
/// includes reasoning. The `invoke_agent` span above it sums its calls and is
/// not read; `github.copilot.cost` is a multiplier, not money.
fn copilot_cli(fields: &mut Fields<'_>) -> Option<Call> {
    if !matches!(
        fields.peek_text("gen_ai.operation.name"),
        Some("chat" | "text_completion" | "generate_content")
    ) {
        return None;
    }
    let input = required(fields, "gen_ai.usage.input_tokens")?;
    let output = required(fields, "gen_ai.usage.output_tokens")?;
    let cache_read = optional(fields, &["gen_ai.usage.cache_read.input_tokens"])?;
    let cache_write = optional(
        fields,
        &[
            "gen_ai.usage.cache_creation.input_tokens",
            "gen_ai.usage.cache_write.input_tokens",
        ],
    )?;
    let model = model(fields, &["gen_ai.response.model", "gen_ai.request.model"]);
    fields.discard(&[
        "gen_ai.operation.name",
        "gen_ai.request.model",
        "gen_ai.usage.reasoning.output_tokens",
        "gen_ai.usage.reasoning_tokens",
    ]);
    Some(Call {
        session_id: fields.take_text(&["gen_ai.conversation.id", "session.id"]),
        message_id: fields.take_text(&["gen_ai.response.id"]),
        model,
        tokens: TokenCounts {
            input: uncached(input, cache_read.checked_add(cache_write)?),
            output,
            cache_write,
            cache_read,
        },
        reported_cost: None,
    })
}

/// `cursor.api.request`: tokens by kind with cache tokens apart, and
/// `cursor.event.id`, the dedupe key `Cursor` keeps across retries. The cost
/// is only in the `cursor.cost.usage` metric.
fn cursor(names: &[&str], fields: &mut Fields<'_>) -> Option<Call> {
    if !names.contains(&"cursor.api.request") {
        return None;
    }
    let tokens = TokenCounts {
        input: required(fields, "cursor.api.request.input_tokens")?,
        output: required(fields, "cursor.api.request.output_tokens")?,
        cache_write: optional(fields, &["cursor.api.request.cache_creation_tokens"])?,
        cache_read: optional(fields, &["cursor.api.request.cache_read_tokens"])?,
    };
    Some(Call {
        session_id: fields.take_text(&["cursor.conversation.id"]),
        message_id: fields.take_text(&["cursor.event.id"]),
        model: model(fields, &["cursor.model.name"]),
        tokens,
        reported_cost: None,
    })
}

/// A token count the model call must have.
fn required(fields: &mut Fields<'_>, key: &str) -> Option<u64> {
    fields.take_count(key).ok().flatten()
}

/// A token count that may be absent (zero), but not unreadable.
fn optional(fields: &mut Fields<'_>, keys: &[&str]) -> Option<u64> {
    fields.take_count_or_zero(keys).ok()
}

fn model(fields: &mut Fields<'_>, keys: &[&str]) -> String {
    fields
        .take_text(keys)
        .unwrap_or_else(|| UNKNOWN_MODEL.to_owned())
}

/// The input tokens not read from or written to a cache, from a total that
/// includes them. Should the cache tokens exceed the total, the harness
/// counted them apart after all, and the total is kept.
fn uncached(total: u64, cache: u64) -> u64 {
    total.checked_sub(cache).unwrap_or(total)
}
