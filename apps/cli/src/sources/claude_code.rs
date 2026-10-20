//! Claude Code's session transcripts: one JSONL file per session under
//! `projects/`, with subagent transcripts in a folder beside it.
//!
//! Counting rules, as in ccusage's Claude adapter:
//!
//! - Claude Code writes one line per content block of a response, each with
//!   the full usage. A response is counted once, by its message ID.
//! - A call's id in Token Controller is its API request ID (`req_…`), the
//!   id Claude Code's telemetry sends for the same call, so the two doors
//!   join (ADR 0018). A line without one keeps the message ID.
//! - A side conversation can replay a parent response under a new request ID;
//!   the main copy wins over the `isSidechain` copy.
//! - Advisor calls inside a response (`usage.iterations` of type
//!   `advisor_message`) are separate model calls with their own model.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use super::{FileContents, Message, Prompt, cost_nanos, home_dir, timestamp_ms};
use crate::tokens::Tokens;

/// The folders Claude Code keeps its transcripts in: every folder named in
/// `CLAUDE_CONFIG_DIR` (comma-separated), or `~/.config/claude` and
/// `~/.claude`, each with `projects/` inside.
pub fn roots() -> Vec<PathBuf> {
    let config_dir =
        std::env::var_os("CLAUDE_CONFIG_DIR").map(|dirs| dirs.to_string_lossy().into_owned());
    roots_from(config_dir.as_deref(), home_dir())
}

fn roots_from(config_dir: Option<&str>, home: Option<PathBuf>) -> Vec<PathBuf> {
    let named: Vec<PathBuf> = config_dir
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|dir| !dir.is_empty())
        .map(|dir| Path::new(dir).join("projects"))
        .collect();
    if !named.is_empty() {
        return named;
    }
    home.map(|home| {
        vec![
            home.join(".config/claude/projects"),
            home.join(".claude/projects"),
        ]
    })
    .unwrap_or_default()
}

#[derive(Deserialize)]
struct Line {
    #[serde(rename = "type")]
    kind: Option<String>,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    timestamp: Option<String>,
    cwd: Option<String>,
    #[serde(rename = "gitBranch")]
    git_branch: Option<String>,
    #[serde(rename = "isSidechain", default)]
    is_sidechain: bool,
    #[serde(rename = "isMeta", default)]
    is_meta: bool,
    #[serde(rename = "requestId")]
    request_id: Option<String>,
    #[serde(rename = "agentId")]
    agent_id: Option<String>,
    #[serde(rename = "costUSD")]
    cost_usd: Option<Value>,
    message: Option<ApiMessage>,
    #[serde(rename = "toolUseResult")]
    tool_use_result: Option<Value>,
    #[serde(rename = "aiTitle")]
    ai_title: Option<String>,
    #[serde(rename = "customTitle")]
    custom_title: Option<String>,
    summary: Option<String>,
}

#[derive(Deserialize)]
struct ApiMessage {
    id: Option<String>,
    model: Option<String>,
    usage: Option<Usage>,
    content: Option<Value>,
}

#[derive(Deserialize, Default, Clone, Copy)]
struct CacheCreation {
    #[serde(default)]
    ephemeral_5m_input_tokens: u64,
    #[serde(default)]
    ephemeral_1h_input_tokens: u64,
}

#[derive(Deserialize, Default)]
struct Usage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
    cache_creation: Option<CacheCreation>,
    #[serde(default)]
    iterations: Vec<Iteration>,
}

#[derive(Deserialize)]
struct Iteration {
    #[serde(rename = "type")]
    kind: Option<String>,
    model: Option<String>,
    #[serde(flatten)]
    usage: Usage,
}

impl Usage {
    fn tokens(&self) -> Tokens {
        // Older transcripts give only the total cache write, which was the
        // five-minute cache.
        let split = self.cache_creation.unwrap_or(CacheCreation {
            ephemeral_5m_input_tokens: self.cache_creation_input_tokens,
            ephemeral_1h_input_tokens: 0,
        });
        Tokens {
            input: self.input_tokens,
            output: self.output_tokens,
            cache_write_5m: split.ephemeral_5m_input_tokens,
            cache_write_1h: split.ephemeral_1h_input_tokens,
            cache_read: self.cache_read_input_tokens,
        }
    }
}

/// Reads one transcript.
pub fn parse(text: &str) -> FileContents {
    let mut contents = FileContents::default();
    // Tool calls that started a subagent, by tool use ID: the subagent type.
    let mut subagent_calls: Vec<(String, String)> = Vec::new();
    for raw in text.lines() {
        let Ok(line) = serde_json::from_str::<Line>(raw) else {
            continue;
        };
        let Some(session_id) = line.session_id.clone() else {
            continue;
        };
        match line.kind.as_deref() {
            Some("assistant") => {
                if let Some(message) = &line.message {
                    subagent_calls.extend(subagent_types(message.content.as_ref()));
                }
                read_assistant(&line, &session_id, &mut contents.messages);
            }
            Some("user") => {
                if let Some(agent) = started_subagent(&line, &subagent_calls) {
                    contents
                        .subagents
                        .push((session_id.clone(), agent.0, agent.1));
                }
                if line.is_meta || line.is_sidechain {
                    continue;
                }
                let at_ms = line
                    .timestamp
                    .as_deref()
                    .and_then(timestamp_ms)
                    .unwrap_or(0);
                if let Some(text) = line
                    .message
                    .as_ref()
                    .and_then(|message| prompt_text(message.content.as_ref()))
                {
                    contents.prompts.push(Prompt {
                        session_id,
                        at_ms,
                        text,
                    });
                }
            }
            Some("ai-title" | "custom-title" | "summary") => {
                if let Some(title) = line
                    .custom_title
                    .or(line.ai_title)
                    .or(line.summary)
                    .filter(|title| !title.trim().is_empty())
                {
                    contents.titles.push((session_id, title.trim().to_owned()));
                }
            }
            _ => {}
        }
    }
    contents
}

fn read_assistant(line: &Line, session_id: &str, messages: &mut Vec<Message>) {
    let Some(message) = &line.message else {
        return;
    };
    let (Some(message_id), Some(usage)) = (&message.id, &message.usage) else {
        return;
    };
    let Some(at_ms) = line.timestamp.as_deref().and_then(timestamp_ms) else {
        return;
    };
    let model = message.model.clone().unwrap_or_default();
    let tokens = usage.tokens();
    // Error placeholders carry the model `<synthetic>` and no usage.
    if tokens.is_zero() || model.starts_with('<') {
        return;
    }
    let key = message_key(session_id, message_id, line.request_id.as_deref(), at_ms);
    // The id both doors share. Claude Code's telemetry names a call by its
    // API request id, so the call takes that id here too; the response id
    // stays the key that makes copies count once.
    let id = line
        .request_id
        .clone()
        .unwrap_or_else(|| message_id.clone());
    let branch = line
        .git_branch
        .clone()
        .filter(|branch| !branch.is_empty() && branch != "HEAD");
    let base = Message {
        session_id: session_id.to_owned(),
        key: key.clone(),
        id: id.clone(),
        request_id: line.request_id.clone(),
        at_ms,
        model,
        tokens,
        reported_cost: line.cost_usd.as_ref().and_then(cost_nanos),
        branch,
        directory: line.cwd.clone(),
        repository: None,
        agent_id: line.agent_id.clone(),
        sidechain: line.is_sidechain,
    };
    let advisors: Vec<Message> = usage
        .iterations
        .iter()
        .filter(|iteration| iteration.kind.as_deref() == Some("advisor_message"))
        .enumerate()
        .filter_map(|(index, iteration)| {
            let tokens = iteration.usage.tokens();
            (!tokens.is_zero()).then(|| Message {
                key: format!("{key}:advisor:{index}"),
                id: format!("{id}:advisor:{index}"),
                model: iteration
                    .model
                    .clone()
                    .unwrap_or_else(|| base.model.clone()),
                tokens,
                reported_cost: None,
                ..base.clone()
            })
        })
        .collect();
    messages.push(base);
    messages.extend(advisors);
}

/// One response counts once. Anthropic message IDs (`msg_…`) are unique per
/// response, so they are the key on their own; that also folds a side
/// conversation's replay of a parent response into the parent's copy.
/// Gateways reuse IDs, so other IDs are keyed with their session and request
/// or time.
fn message_key(session_id: &str, message_id: &str, request_id: Option<&str>, at_ms: i64) -> String {
    if message_id.starts_with("msg_") {
        return message_id.to_owned();
    }
    match request_id {
        Some(request_id) => format!("{session_id}:{message_id}:{request_id}"),
        None => format!("{session_id}:{message_id}:{at_ms}"),
    }
}

/// The text a person typed: a plain string, or the text blocks of a content
/// list. Tool results are not the person's words and are skipped.
fn prompt_text(content: Option<&Value>) -> Option<String> {
    match content? {
        Value::String(text) => Some(text.clone()),
        Value::Array(blocks) => {
            let texts: Vec<&str> = blocks
                .iter()
                .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|block| block.get("text").and_then(Value::as_str))
                .collect();
            (!texts.is_empty()).then(|| texts.join("\n"))
        }
        _ => None,
    }
}

/// Subagent calls in an assistant response: tool use ID and subagent type.
fn subagent_types(content: Option<&Value>) -> Vec<(String, String)> {
    let Some(Value::Array(blocks)) = content else {
        return Vec::new();
    };
    blocks
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("tool_use"))
        .filter(|block| {
            matches!(
                block.get("name").and_then(Value::as_str),
                Some("Task" | "Agent")
            )
        })
        .filter_map(|block| {
            let id = block.get("id")?.as_str()?;
            let kind = block.get("input")?.get("subagent_type")?.as_str()?;
            Some((id.to_owned(), kind.to_owned()))
        })
        .collect()
}

/// A tool result that names the subagent a subagent call started: its ID and
/// type.
fn started_subagent(line: &Line, calls: &[(String, String)]) -> Option<(String, String)> {
    let agent_id = line
        .tool_use_result
        .as_ref()?
        .get("agentId")?
        .as_str()?
        .to_owned();
    let Some(Value::Array(blocks)) = line.message.as_ref()?.content.as_ref() else {
        return None;
    };
    let tool_use_id = blocks
        .iter()
        .find_map(|block| block.get("tool_use_id").and_then(Value::as_str))?;
    let kind = calls
        .iter()
        .find(|(id, _)| id == tool_use_id)
        .map(|(_, kind)| kind.clone())?;
    Some((agent_id, kind))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assistant(id: &str, request: &str, at: &str, sidechain: bool, usage: &str) -> String {
        format!(
            r#"{{"type":"assistant","sessionId":"s1","timestamp":"{at}","cwd":"/work/storefront","gitBranch":"KD-214-fix","isSidechain":{sidechain},"requestId":"{request}","message":{{"id":"{id}","model":"claude-sonnet-4-5","usage":{usage}}}}}"#
        )
    }

    const USAGE: &str = r#"{"input_tokens":10,"output_tokens":20,"cache_creation_input_tokens":30,"cache_read_input_tokens":40}"#;

    #[test]
    fn each_line_of_a_response_carries_its_usage_and_counts_once_later() {
        let text = [
            assistant("msg_1", "req_1", "2026-10-11T09:12:00Z", false, USAGE),
            assistant("msg_1", "req_1", "2026-10-11T09:12:00Z", false, USAGE),
            assistant("msg_2", "req_2", "2026-10-11T09:13:00Z", false, USAGE),
        ]
        .join("\n");
        let contents = parse(&text);
        // The parser keeps every line; the store keeps one row per key.
        assert_eq!(contents.messages.len(), 3);
        let first = contents.messages.first().unwrap();
        assert_eq!(first.key, "msg_1");
        assert_eq!(first.branch.as_deref(), Some("KD-214-fix"));
        assert_eq!(first.directory.as_deref(), Some("/work/storefront"));
        assert_eq!(first.tokens.cache_write_5m, 30);
        assert_eq!(first.tokens.cache_read, 40);
        assert_eq!(first.request_id.as_deref(), Some("req_1"));
        // Telemetry names the same call req_1 (ADR 0018), so that is its id.
        assert_eq!(first.id, "req_1");
        assert_eq!(contents.messages.get(2).unwrap().id, "req_2");
    }

    #[test]
    fn a_call_without_a_request_id_keeps_the_response_id() {
        let text = r#"{"type":"assistant","sessionId":"s1","timestamp":"2026-10-11T09:12:00Z","cwd":"/work/storefront","message":{"id":"msg_7","model":"claude-sonnet-4-5","usage":{"input_tokens":10,"output_tokens":20}}}"#;
        let contents = parse(text);
        let only = contents.messages.first().unwrap();
        assert_eq!(only.id, "msg_7");
        assert_eq!(only.key, "msg_7");
        assert_eq!(only.request_id, None);
    }

    #[test]
    fn a_sidechain_replay_has_the_same_key_as_its_parent() {
        let text = [
            assistant("msg_1", "req_1", "2026-10-11T09:12:00Z", false, USAGE),
            assistant("msg_1", "req_9", "2026-10-11T09:12:00Z", true, USAGE),
        ]
        .join("\n");
        let contents = parse(&text);
        let keys: Vec<&str> = contents.messages.iter().map(|m| m.key.as_str()).collect();
        assert_eq!(keys, ["msg_1", "msg_1"]);
        assert!(contents.messages.get(1).unwrap().sidechain);
    }

    #[test]
    fn gateway_ids_are_keyed_with_session_and_request_or_time() {
        assert_eq!(message_key("s", "abc", Some("r"), 5), "s:abc:r");
        assert_eq!(message_key("s", "abc", None, 5), "s:abc:5");
        assert_eq!(message_key("s", "msg_x", Some("r"), 5), "msg_x");
    }

    #[test]
    fn one_hour_cache_writes_are_kept_apart() {
        let usage = r#"{"input_tokens":1,"output_tokens":2,"cache_creation_input_tokens":30,"cache_read_input_tokens":4,"cache_creation":{"ephemeral_5m_input_tokens":10,"ephemeral_1h_input_tokens":20}}"#;
        let contents = parse(&assistant(
            "msg_1",
            "r",
            "2026-10-11T09:12:00Z",
            false,
            usage,
        ));
        let tokens = contents.messages.first().unwrap().tokens;
        assert_eq!(tokens.cache_write_5m, 10);
        assert_eq!(tokens.cache_write_1h, 20);
    }

    #[test]
    fn advisor_calls_are_their_own_messages() {
        let usage = r#"{"input_tokens":1,"output_tokens":2,"iterations":[{"type":"message","input_tokens":1,"output_tokens":2},{"type":"advisor_message","model":"claude-opus-5","input_tokens":100,"output_tokens":50}]}"#;
        let contents = parse(&assistant(
            "msg_1",
            "r",
            "2026-10-11T09:12:00Z",
            false,
            usage,
        ));
        assert_eq!(contents.messages.len(), 2);
        let advisor = contents.messages.get(1).unwrap();
        assert_eq!(advisor.model, "claude-opus-5");
        assert_eq!(advisor.key, "msg_1:advisor:0");
        assert_eq!(advisor.id, "r:advisor:0");
        assert_eq!(advisor.tokens.input, 100);
    }

    #[test]
    fn placeholders_and_broken_lines_are_skipped() {
        let synthetic = r#"{"type":"assistant","sessionId":"s1","timestamp":"2026-10-11T09:12:00Z","message":{"id":"x","model":"<synthetic>","usage":{"input_tokens":0,"output_tokens":0}}}"#;
        let text = format!("{synthetic}\n{{not json\n\n");
        assert!(parse(&text).messages.is_empty());
    }

    #[test]
    fn head_is_not_a_branch_and_cost_is_read_exactly() {
        let line = r#"{"type":"assistant","sessionId":"s1","timestamp":"2026-10-11T09:12:00Z","gitBranch":"HEAD","costUSD":0.0312,"message":{"id":"msg_1","model":"m","usage":{"input_tokens":1}}}"#;
        let message = parse(line).messages.into_iter().next().unwrap();
        assert_eq!(message.branch, None);
        assert_eq!(message.reported_cost, Some(31_200_000));
    }

    #[test]
    fn prompts_and_titles_are_read_but_tool_results_and_meta_lines_are_not() {
        let text = [
            r#"{"type":"user","sessionId":"s1","timestamp":"2026-10-11T09:00:00Z","message":{"role":"user","content":"Fix KD-214 please"}}"#,
            r#"{"type":"user","sessionId":"s1","timestamp":"2026-10-11T09:01:00Z","message":{"role":"user","content":[{"type":"text","text":"see KD-231"},{"type":"tool_result","tool_use_id":"t","content":"KD-999"}]}}"#,
            r#"{"type":"user","sessionId":"s1","isMeta":true,"message":{"content":"KD-1"}}"#,
            r#"{"type":"user","sessionId":"s1","isSidechain":true,"message":{"content":"KD-2"}}"#,
            r#"{"type":"user","sessionId":"s1","message":{"content":[{"type":"tool_result","tool_use_id":"t","content":"x"}]}}"#,
            r#"{"type":"ai-title","sessionId":"s1","aiTitle":"Fix checkout redirect loop"}"#,
            r#"{"type":"custom-title","sessionId":"s1","customTitle":"  "}"#,
        ]
        .join("\n");
        let contents = parse(&text);
        let prompts: Vec<&str> = contents.prompts.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(prompts, ["Fix KD-214 please", "see KD-231"]);
        assert_eq!(contents.prompts.first().unwrap().at_ms, 1_791_709_200_000);
        assert_eq!(
            contents.titles,
            [("s1".to_owned(), "Fix checkout redirect loop".to_owned())]
        );
    }

    #[test]
    fn subagent_types_are_linked_to_the_subagent_they_started() {
        let text = [
            r#"{"type":"assistant","sessionId":"s1","timestamp":"2026-10-11T09:12:00Z","message":{"id":"msg_1","model":"m","usage":{"input_tokens":1},"content":[{"type":"tool_use","id":"toolu_1","name":"Task","input":{"subagent_type":"code-reviewer"}}]}}"#,
            r#"{"type":"user","sessionId":"s1","toolUseResult":{"agentId":"a42"},"message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"done"}]}}"#,
            r#"{"type":"assistant","sessionId":"s1","agentId":"a42","isSidechain":true,"timestamp":"2026-10-11T09:13:00Z","message":{"id":"msg_2","model":"m","usage":{"input_tokens":1}}}"#,
        ]
        .join("\n");
        let contents = parse(&text);
        assert_eq!(
            contents.subagents,
            [(
                "s1".to_owned(),
                "a42".to_owned(),
                "code-reviewer".to_owned()
            )]
        );
        assert_eq!(
            contents.messages.get(1).unwrap().agent_id.as_deref(),
            Some("a42")
        );
    }

    #[test]
    fn roots_follow_claude_config_dir() {
        let home = Some(PathBuf::from("/home/m"));
        assert_eq!(
            roots_from(Some("/a, /b,"), home.clone()),
            [PathBuf::from("/a/projects"), PathBuf::from("/b/projects")]
        );
        assert_eq!(
            roots_from(Some(" "), home.clone()),
            [
                PathBuf::from("/home/m/.config/claude/projects"),
                PathBuf::from("/home/m/.claude/projects")
            ]
        );
        assert_eq!(roots_from(None, home).len(), 2);
        assert!(roots_from(None, None).is_empty());
    }
}
