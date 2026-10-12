//! Reading agents' session files on this machine.
//!
//! The readers follow the record shapes and the counting rules of the open
//! source ccusage adapters (MIT, see `THIRD_PARTY_NOTICES.md`): which lines
//! carry usage, how copies of one response are counted once, and how Codex's
//! running totals turn into per-turn counts. They keep what ccusage drops and
//! a token entry needs: the message and request IDs, the branch, the working
//! directory and the ticket keys in prompts (ADR 0014).
//!
//! Prompt text is only held in memory while a file is read, to find ticket
//! keys. It is never stored and never leaves the machine.

pub mod claude_code;
pub mod codex;

use std::fmt;
use std::path::{Path, PathBuf};

use crate::tokens::Tokens;

/// The agent harnesses whose session files `tc` reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Harness {
    ClaudeCode,
    Codex,
}

impl Harness {
    pub const ALL: [Self; 2] = [Self::ClaudeCode, Self::Codex];

    /// The name used in token entries, as the docs write it.
    pub fn name(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|harness| harness.name() == name)
    }
}

impl fmt::Display for Harness {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// One model call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub session_id: String,
    /// The key that makes copies of one response count once.
    pub key: String,
    /// The ID the agent harness gave the model call.
    pub id: String,
    pub request_id: Option<String>,
    /// Milliseconds since the Unix epoch.
    pub at_ms: i64,
    pub model: String,
    pub tokens: Tokens,
    /// The agent's own cost figure, in US dollar nano-units.
    pub reported_cost: Option<i64>,
    pub branch: Option<String>,
    pub directory: Option<String>,
    pub repository: Option<String>,
    /// The ID of the subagent that made the call, if one did.
    pub agent_id: Option<String>,
    /// A copy written into a side conversation; the main copy wins.
    pub sidechain: bool,
}

/// Ticket-key-bearing text a person wrote. Held only while reading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub session_id: String,
    pub at_ms: i64,
    pub text: String,
}

/// Everything read from one session file.
#[derive(Debug, Default)]
pub struct FileContents {
    pub messages: Vec<Message>,
    pub prompts: Vec<Prompt>,
    /// Session titles the agent wrote, by session ID; the last one wins.
    pub titles: Vec<(String, String)>,
    /// Subagent types, by session ID and subagent ID.
    pub subagents: Vec<(String, String, String)>,
}

/// The session files of every agent harness on this machine.
pub fn session_files() -> Vec<(Harness, PathBuf)> {
    let mut files = Vec::new();
    for root in claude_code::roots() {
        files.extend(
            jsonl_files(&root)
                .into_iter()
                .map(|path| (Harness::ClaudeCode, path)),
        );
    }
    for root in codex::roots() {
        files.extend(
            jsonl_files(&root)
                .into_iter()
                .map(|path| (Harness::Codex, path)),
        );
    }
    files.sort();
    files.dedup();
    files
}

/// Reads one session file. Lines that are not valid JSON are skipped, as the
/// agents sometimes leave a half-written last line.
///
/// # Errors
///
/// Only when the file cannot be read at all.
pub fn read(harness: Harness, path: &Path) -> std::io::Result<FileContents> {
    let text = std::fs::read_to_string(path)?;
    Ok(match harness {
        Harness::ClaudeCode => claude_code::parse(&text),
        Harness::Codex => codex::parse(&text),
    })
}

/// Every `.jsonl` file under a folder, at any depth.
fn jsonl_files(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                folders.push(path);
            } else if kind.is_file() && path.extension().is_some_and(|ext| ext == "jsonl") {
                found.push(path);
            }
        }
    }
    found
}

/// The user's home folder.
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Reads an RFC 3339 timestamp as milliseconds since the epoch.
pub fn timestamp_ms(text: &str) -> Option<i64> {
    text.parse::<jiff::Timestamp>()
        .ok()
        .map(jiff::Timestamp::as_millisecond)
}

/// A JSON number as nano-units. `serde_json` gives back the shortest decimal
/// form of the number, which is the one the agent wrote, and that text is
/// read digit by digit.
pub fn cost_nanos(value: &serde_json::Value) -> Option<i64> {
    match value {
        serde_json::Value::Number(number) => crate::amounts::parse_nanos(&number.to_string()),
        serde_json::Value::String(text) => crate::amounts::parse_nanos(text),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harness_names_round_trip() {
        for harness in Harness::ALL {
            assert_eq!(Harness::from_name(harness.name()), Some(harness));
            assert_eq!(harness.to_string(), harness.name());
        }
        assert_eq!(Harness::from_name("cursor"), None);
    }

    #[test]
    fn timestamps_read_with_and_without_fractions() {
        assert_eq!(timestamp_ms("1970-01-01T00:00:01Z"), Some(1000));
        assert_eq!(timestamp_ms("1970-01-01T00:00:01.5Z"), Some(1500));
        assert_eq!(timestamp_ms("1970-01-01T01:00:01+01:00"), Some(1000));
        assert_eq!(timestamp_ms("yesterday"), None);
    }

    #[test]
    fn costs_are_read_exactly() {
        let value: serde_json::Value = serde_json::from_str("0.0312").unwrap();
        assert_eq!(cost_nanos(&value), Some(31_200_000));
        assert_eq!(cost_nanos(&serde_json::json!("1.5")), Some(1_500_000_000));
        assert_eq!(cost_nanos(&serde_json::json!(null)), None);
    }

    #[test]
    fn jsonl_files_are_found_at_any_depth() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a/b");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(dir.path().join("one.jsonl"), "").unwrap();
        std::fs::write(nested.join("two.jsonl"), "").unwrap();
        std::fs::write(nested.join("notes.txt"), "").unwrap();
        let mut found = jsonl_files(dir.path());
        found.sort();
        assert_eq!(found.len(), 2);
        assert!(jsonl_files(&dir.path().join("missing")).is_empty());
    }
}
