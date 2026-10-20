//! Codex CLI's rollout files: one JSONL file per session under `sessions/`
//! and `archived_sessions/` in `CODEX_HOME` (default `~/.codex`).
//!
//! Counting rules, as in ccusage's Codex adapter:
//!
//! - Usage comes from `event_msg` records of type `token_count`. Their
//!   `total_token_usage` is a running total; `last_token_usage` is the turn.
//!   A record whose running total did not move is a repeat and counts nothing.
//!   Without `last_token_usage`, the turn is the difference of the totals.
//! - `input_tokens` includes the cached input and, since Codex 0.163, the
//!   cache writes (`cache_write_input_tokens`), so the uncached input is what
//!   is left after both; reasoning tokens are part of `output_tokens` already.
//! - A subagent's rollout first replays its parent's history. When the file
//!   has an `inter_agent_communication` record with `trigger_turn: true`,
//!   usage before it belongs to the parent and is skipped.
//! - The model is the one of the latest `turn_context`, or the one the usage
//!   record names.

use std::path::PathBuf;

use serde::Deserialize;
use serde_json::Value;

use super::{FileContents, Message, Prompt, home_dir, timestamp_ms};
use crate::repo::name_from_url;
use crate::tokens::Tokens;

/// `sessions/` and `archived_sessions/` in `CODEX_HOME` or `~/.codex`.
pub fn roots() -> Vec<PathBuf> {
    let home = std::env::var_os("CODEX_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|home| home.join(".codex")));
    home.map(|home| vec![home.join("sessions"), home.join("archived_sessions")])
        .unwrap_or_default()
}

#[derive(Deserialize)]
struct Line {
    #[serde(rename = "type")]
    kind: Option<String>,
    timestamp: Option<String>,
    payload: Option<Value>,
}

#[derive(Deserialize, Default, Clone, Copy, PartialEq, Eq)]
struct RawUsage {
    #[serde(default, rename = "input_tokens")]
    input: u64,
    #[serde(default, rename = "cached_input_tokens")]
    cached_input: u64,
    #[serde(default, rename = "cache_write_input_tokens")]
    cache_write: u64,
    #[serde(default, rename = "output_tokens")]
    output: u64,
}

impl RawUsage {
    fn minus(self, earlier: Option<Self>) -> Self {
        let earlier = earlier.unwrap_or_default();
        Self {
            input: self.input.saturating_sub(earlier.input),
            cached_input: self.cached_input.saturating_sub(earlier.cached_input),
            cache_write: self.cache_write.saturating_sub(earlier.cache_write),
            output: self.output.saturating_sub(earlier.output),
        }
    }

    fn tokens(self) -> Tokens {
        Tokens {
            input: self
                .input
                .saturating_sub(self.cached_input)
                .saturating_sub(self.cache_write),
            output: self.output,
            cache_write_5m: self.cache_write,
            cache_write_1h: 0,
            cache_read: self.cached_input,
        }
    }
}

fn is_child_turn_marker(line: &Line) -> bool {
    matches!(
        line.kind.as_deref(),
        Some("inter_agent_communication_metadata" | "inter_agent_communication")
    ) && line
        .payload
        .as_ref()
        .and_then(|payload| payload.get("trigger_turn"))
        .and_then(Value::as_bool)
        == Some(true)
}

fn usage_at(info: &Value, field: &str) -> Option<RawUsage> {
    info.get(field)
        .and_then(|value| RawUsage::deserialize(value).ok())
}

/// Facts about the session that apply to the messages that follow.
#[derive(Default)]
struct State {
    session_id: Option<String>,
    model: Option<String>,
    directory: Option<String>,
    branch: Option<String>,
    repository: Option<String>,
    previous_total: Option<RawUsage>,
    counted: u64,
}

/// Reads one rollout file. A file without a `session_meta` record names no
/// session and gives nothing.
pub fn parse(text: &str) -> FileContents {
    let lines: Vec<Line> = text
        .lines()
        .filter_map(|raw| serde_json::from_str::<Line>(raw).ok())
        .collect();
    let mut skipping = lines.iter().any(is_child_turn_marker);
    let mut state = State::default();
    let mut contents = FileContents::default();
    for line in &lines {
        if is_child_turn_marker(line) {
            skipping = false;
            continue;
        }
        let Some(payload) = &line.payload else {
            continue;
        };
        match line.kind.as_deref() {
            Some("session_meta") => read_meta(payload, &mut state),
            Some("turn_context") => {
                if let Some(model) = payload.get("model").and_then(Value::as_str) {
                    state.model = Some(model.to_owned());
                }
                if let Some(cwd) = payload.get("cwd").and_then(Value::as_str) {
                    state.directory = Some(cwd.to_owned());
                }
            }
            Some("event_msg") => {
                let at_ms = line.timestamp.as_deref().and_then(timestamp_ms);
                match payload.get("type").and_then(Value::as_str) {
                    Some("token_count") => {
                        if let Some(at_ms) = at_ms
                            && let Some(message) = read_usage(payload, at_ms, &mut state)
                            && !skipping
                        {
                            contents.messages.push(message);
                        }
                    }
                    Some("user_message") if !skipping => {
                        if let (Some(session_id), Some(text)) = (
                            state.session_id.clone(),
                            payload.get("message").and_then(Value::as_str),
                        ) {
                            contents.prompts.push(Prompt {
                                session_id,
                                at_ms: at_ms.unwrap_or(0),
                                text: text.to_owned(),
                            });
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    contents
}

fn read_meta(payload: &Value, state: &mut State) {
    // A subagent's rollout repeats its parent's meta first; the first one
    // names this file's session.
    if state.session_id.is_none() {
        state.session_id = payload.get("id").and_then(Value::as_str).map(str::to_owned);
    }
    if let Some(cwd) = payload.get("cwd").and_then(Value::as_str) {
        state.directory = Some(cwd.to_owned());
    }
    if let Some(git) = payload.get("git") {
        state.branch = git
            .get("branch")
            .and_then(Value::as_str)
            .filter(|branch| !branch.is_empty() && *branch != "HEAD")
            .map(str::to_owned);
        state.repository = git
            .get("repository_url")
            .and_then(Value::as_str)
            .and_then(name_from_url);
    }
}

fn read_usage(payload: &Value, at_ms: i64, state: &mut State) -> Option<Message> {
    let info = payload.get("info")?;
    let total = usage_at(info, "total_token_usage");
    let advanced = total.is_none() || total != state.previous_total;
    let turn = usage_at(info, "last_token_usage")
        .filter(|_| advanced)
        .or_else(|| total.map(|total| total.minus(state.previous_total)));
    if total.is_some() {
        state.previous_total = total;
    }
    let tokens = turn?.tokens();
    if tokens.is_zero() {
        return None;
    }
    let session_id = state.session_id.clone()?;
    let model = info
        .get("model")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| state.model.clone())
        .unwrap_or_default();
    let index = state.counted;
    state.counted = state.counted.saturating_add(1);
    Some(Message {
        key: format!("{session_id}:{index}"),
        id: format!("turn-{index}"),
        session_id,
        request_id: None,
        at_ms,
        model,
        tokens,
        reported_cost: None,
        branch: state.branch.clone(),
        directory: state.directory.clone(),
        repository: state.repository.clone(),
        agent_id: None,
        sidechain: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const META: &str = r#"{"timestamp":"2026-10-11T09:00:00Z","type":"session_meta","payload":{"id":"c1","cwd":"/work/storefront","git":{"branch":"KD-231-cart","repository_url":"git@github.com:kestrel/storefront.git"}}}"#;
    const CONTEXT: &str = r#"{"timestamp":"2026-10-11T09:00:01Z","type":"turn_context","payload":{"model":"gpt-5.2-codex","cwd":"/work/storefront"}}"#;

    fn count(at: &str, last: Option<(u64, u64, u64)>, total: Option<(u64, u64, u64)>) -> String {
        let usage = |(input, cached, output): (u64, u64, u64)| {
            format!(
                r#"{{"input_tokens":{input},"cached_input_tokens":{cached},"output_tokens":{output},"reasoning_output_tokens":1,"total_tokens":0}}"#
            )
        };
        let mut info = Vec::new();
        if let Some(last) = last {
            info.push(format!(r#""last_token_usage":{}"#, usage(last)));
        }
        if let Some(total) = total {
            info.push(format!(r#""total_token_usage":{}"#, usage(total)));
        }
        format!(
            r#"{{"timestamp":"{at}","type":"event_msg","payload":{{"type":"token_count","info":{{{}}}}}}}"#,
            info.join(",")
        )
    }

    #[test]
    fn turns_come_from_token_counts_and_repeats_count_nothing() {
        let text = [
            META.to_owned(),
            CONTEXT.to_owned(),
            count(
                "2026-10-11T09:01:00Z",
                Some((1000, 250, 125)),
                Some((1000, 250, 125)),
            ),
            count(
                "2026-10-11T09:01:01Z",
                Some((1000, 250, 125)),
                Some((1000, 250, 125)),
            ),
            count(
                "2026-10-11T09:02:00Z",
                Some((2500, 500, 300)),
                Some((3500, 750, 425)),
            ),
        ]
        .join("\n");
        let contents = parse(&text);
        assert_eq!(contents.messages.len(), 2);
        let first = contents.messages.first().unwrap();
        assert_eq!(first.session_id, "c1");
        assert_eq!(first.key, "c1:0");
        assert_eq!(first.model, "gpt-5.2-codex");
        assert_eq!(first.tokens.input, 750);
        assert_eq!(first.tokens.cache_read, 250);
        assert_eq!(first.tokens.output, 125);
        assert_eq!(first.branch.as_deref(), Some("KD-231-cart"));
        assert_eq!(first.repository.as_deref(), Some("kestrel/storefront"));
        let second = contents.messages.get(1).unwrap();
        assert_eq!(second.key, "c1:1");
        assert_eq!(second.tokens.input, 2000);
    }

    #[test]
    fn cache_writes_leave_the_input_and_count_as_cache_writes() {
        // As Codex 0.163 writes it: input_tokens holds the cache writes too.
        let text = [
            META.to_owned(),
            r#"{"timestamp":"2026-10-11T09:01:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":13290,"cached_input_tokens":0,"cache_write_input_tokens":13287,"output_tokens":5,"reasoning_output_tokens":0,"total_tokens":13295},"last_token_usage":{"input_tokens":13290,"cached_input_tokens":0,"cache_write_input_tokens":13287,"output_tokens":5,"reasoning_output_tokens":0,"total_tokens":13295}}}}"#.to_owned(),
        ]
        .join("\n");
        let contents = parse(&text);
        let first = contents.messages.first().unwrap();
        assert_eq!(first.tokens.input, 3);
        assert_eq!(first.tokens.cache_write_5m, 13287);
        assert_eq!(first.tokens.cache_read, 0);
        assert_eq!(first.tokens.output, 5);
    }

    #[test]
    fn without_the_turn_the_totals_are_subtracted() {
        let text = [
            META.to_owned(),
            count("2026-10-11T09:01:00Z", None, Some((1000, 0, 100))),
            count("2026-10-11T09:02:00Z", None, Some((1500, 200, 150))),
        ]
        .join("\n");
        let contents = parse(&text);
        let second = contents.messages.get(1).unwrap();
        assert_eq!(second.tokens.input, 300);
        assert_eq!(second.tokens.cache_read, 200);
        assert_eq!(second.tokens.output, 50);
        // No turn context: the model is unknown, never guessed.
        assert_eq!(second.model, "");
    }

    #[test]
    fn a_usage_record_without_totals_still_counts() {
        let text = [
            META.to_owned(),
            count("2026-10-11T09:01:00Z", Some((10, 0, 5)), None),
        ]
        .join("\n");
        assert_eq!(parse(&text).messages.len(), 1);
    }

    #[test]
    fn a_subagent_skips_its_parents_replayed_history() {
        let marker = r#"{"timestamp":"2026-10-11T09:03:00Z","type":"inter_agent_communication_metadata","payload":{"trigger_turn":true}}"#;
        let not_marker = r#"{"timestamp":"2026-10-11T09:02:30Z","type":"inter_agent_communication_metadata","payload":{"trigger_turn":false}}"#;
        let prompt = r#"{"timestamp":"2026-10-11T09:00:30Z","type":"event_msg","payload":{"type":"user_message","message":"parent asks about KD-1"}}"#;
        let text = [
            META.to_owned(),
            CONTEXT.to_owned(),
            prompt.to_owned(),
            count(
                "2026-10-11T09:01:00Z",
                Some((1000, 0, 100)),
                Some((1000, 0, 100)),
            ),
            not_marker.to_owned(),
            marker.to_owned(),
            count(
                "2026-10-11T09:04:00Z",
                Some((50, 0, 5)),
                Some((1050, 0, 105)),
            ),
        ]
        .join("\n");
        let contents = parse(&text);
        assert_eq!(contents.messages.len(), 1);
        assert_eq!(contents.messages.first().unwrap().tokens.input, 50);
        assert!(contents.prompts.is_empty());
    }

    #[test]
    fn prompts_are_read_from_user_messages() {
        let prompt = r#"{"timestamp":"2026-10-11T09:00:30Z","type":"event_msg","payload":{"type":"user_message","message":"work on KD-231"}}"#;
        let text = [META, prompt].join("\n");
        let contents = parse(&text);
        assert_eq!(contents.prompts.len(), 1);
        assert_eq!(contents.prompts.first().unwrap().text, "work on KD-231");
    }

    #[test]
    fn a_file_without_a_session_id_gives_nothing() {
        let text = [
            CONTEXT.to_owned(),
            count("2026-10-11T09:01:00Z", Some((1, 0, 1)), None),
        ]
        .join("\n");
        assert!(parse(&text).messages.is_empty());
    }
}
