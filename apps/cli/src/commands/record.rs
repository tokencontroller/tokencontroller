//! `tc record`: what the Claude Code mod or another tool knows about a
//! session that its files do not say: the summary the mod writes, tags and a
//! ticket picked in the session, the pull request. Reads JSON on standard
//! input: one object, or one object per line as the mod sends them. A pull
//! request title is searched for ticket keys here and only the keys are kept.
//!
//! A session named here counts even outside the tracked folders: the mod
//! knows its own session, wherever it runs.
//!
//! The mod also sends a `turn` line after every turn. tc counts tokens from
//! the session files, which hold the same turns, so those lines are taken
//! and not counted twice.

use std::collections::BTreeMap;
use std::io::Read;

use serde::{Deserialize, Serialize};

use crate::app::App;
use crate::error::{Error, Result};
use crate::output::{self, EntryView};
use crate::sources::Harness;
use crate::store::{Store, local_id};

/// Records for sessions that have no token entry yet wait here.
const PENDING: &str = "pending_record:";

#[derive(Debug, Deserialize)]
pub struct Record {
    /// `summary`, `tags` or `turn` from the mod; absent from other tools.
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub agent_harness: Option<String>,
    pub session_id: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub ticket: Option<String>,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub tags: BTreeMap<String, String>,
    #[serde(default)]
    pub pull_request: Option<serde_json::Value>,
    #[serde(default)]
    pub pull_request_title: Option<String>,
}

/// A record reduced to what is kept.
#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
struct Kept {
    summary: Option<String>,
    named: BTreeMap<String, String>,
    signals: BTreeMap<String, String>,
}

impl Kept {
    /// Adds a later record: its values win.
    fn merge(&mut self, later: Self) {
        if later.summary.is_some() {
            self.summary = later.summary;
        }
        self.named.extend(later.named);
        self.signals.extend(later.signals);
    }

    fn is_empty(&self) -> bool {
        self.summary.is_none() && self.named.is_empty() && self.signals.is_empty()
    }
}

/// The records in the input: one JSON object, an array of them, or one
/// object per line. Lines that are not records are skipped and named.
fn records(text: &str) -> Result<(Vec<Record>, Vec<String>)> {
    if let Ok(record) = serde_json::from_str::<Record>(text) {
        return Ok((vec![record], Vec::new()));
    }
    if let Ok(list) = serde_json::from_str::<Vec<Record>>(text) {
        return Ok((list, Vec::new()));
    }
    let mut found = Vec::new();
    let mut skipped = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Record>(line) {
            Ok(record) => found.push(record),
            Err(error) => skipped.push(format!("line {}: {error}", index.saturating_add(1))),
        }
    }
    if found.is_empty() && !skipped.is_empty() {
        return Err(Error::usage(format!(
            "tc record reads JSON objects with a session_id; {}",
            skipped.join("; ")
        )));
    }
    Ok((found, skipped))
}

pub fn run(app: &mut App<'_>, input: &mut dyn Read) -> Result<()> {
    let mut text = String::new();
    input
        .read_to_string(&mut text)
        .map_err(|source| Error::file("standard input", source))?;
    let (records, skipped) = records(&text)?;
    for problem in &skipped {
        eprintln!("tc: skipped {problem}");
    }
    app.refresh_context(false)?;
    let patterns = app.patterns();

    let mut by_session: BTreeMap<(String, String), Kept> = BTreeMap::new();
    let mut taken = 0_usize;
    for record in records {
        let name = record.agent_harness.as_deref().unwrap_or("claude-code");
        let harness = Harness::from_name(name).ok_or_else(|| {
            Error::usage(format!(
                "{name} is not an agent harness tc reads; use claude-code or codex"
            ))
        })?;
        taken = taken.saturating_add(1);
        if record.session_id.trim().is_empty() {
            continue;
        }
        // A session the mod or another tool reports counts in any folder.
        app.store.mark_recorded(harness, record.session_id.trim())?;
        if record.kind.as_deref() == Some("turn") {
            continue;
        }
        let session = record.session_id.trim().to_owned();
        let kept = keep(app, record, &patterns);
        by_session
            .entry((harness.name().to_owned(), session))
            .or_default()
            .merge(kept);
    }

    let mut sessions: Vec<(Harness, String)> = Vec::new();
    for ((harness, session_id), kept) in by_session {
        let Some(harness) = Harness::from_name(&harness) else {
            continue;
        };
        sessions.push((harness, session_id.clone()));
        if kept.is_empty() {
            continue;
        }
        let key = format!("{PENDING}{}", local_id(harness, &session_id));
        let mut waiting: Kept = app
            .store
            .meta(&key)?
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        waiting.merge(kept);
        app.store
            .set_meta(&key, &serde_json::to_string(&waiting)?)?;
    }
    app.prepare_without_asking()?;

    let mut entries = Vec::new();
    for (harness, session_id) in &sessions {
        if let Some(row) = app.store.entry_by_session(*harness, session_id)? {
            entries.push(app.entry(row.number)?);
        }
    }
    if app.json {
        let views: Vec<EntryView> = entries.iter().map(EntryView::of).collect();
        return output::json(
            app.out,
            &serde_json::json!({ "recorded": taken, "token_entries": views }),
        );
    }
    let numbers: Vec<String> = entries
        .iter()
        .map(|entry| entry.row.number.to_string())
        .collect();
    if numbers.is_empty() {
        writeln!(
            app.out,
            "recorded {taken}; it goes on the session's token entry once the session's file is read"
        )?;
    } else {
        writeln!(
            app.out,
            "recorded {taken} on token entry {}",
            numbers.join(", ")
        )?;
    }
    Ok(())
}

fn keep(app: &App<'_>, record: Record, patterns: &[crate::keys::KeyPattern]) -> Kept {
    let mut named = BTreeMap::new();
    for (name, value) in record.tags {
        if app.context.allows_tag(&name) && !value.trim().is_empty() {
            named.insert(name, value.trim().to_owned());
        }
    }
    if let Some(ticket) = record.ticket.filter(|t| !t.trim().is_empty()) {
        named.insert("ticket".to_owned(), ticket.trim().to_owned());
    }
    if let Some(project) = record.project.filter(|p| !p.trim().is_empty()) {
        named.insert("project".to_owned(), project.trim().to_owned());
    }
    let mut signals = BTreeMap::new();
    match record.pull_request {
        Some(serde_json::Value::Number(number)) => {
            signals.insert("pull_request".to_owned(), number.to_string());
        }
        Some(serde_json::Value::String(text)) if !text.trim().is_empty() => {
            signals.insert("pull_request".to_owned(), text.trim().to_owned());
        }
        _ => {}
    }
    if let Some(title) = record.pull_request_title {
        let keys = crate::keys::find_all(patterns, &title);
        if !keys.is_empty() {
            signals.insert("pull_request_title_keys".to_owned(), keys.join(","));
        }
    }
    Kept {
        summary: record
            .summary
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty()),
        named,
        signals,
    }
}

/// Puts waiting records on their token entries once those exist. Called
/// after every read of the session files.
pub fn apply_pending(store: &Store) -> Result<()> {
    for (key, text) in store.meta_with_prefix(PENDING)? {
        let Some(local) = key.strip_prefix(PENDING) else {
            continue;
        };
        let Some((harness, session_id)) = local.split_once(':') else {
            continue;
        };
        let Some(harness) = Harness::from_name(harness) else {
            continue;
        };
        let Some(entry) = store.entry_by_session(harness, session_id)? else {
            continue;
        };
        let kept: Kept = serde_json::from_str(&text).unwrap_or_default();
        if kept.summary.is_some() {
            store.set_summary(&entry.local_id, kept.summary.as_deref())?;
        }
        if !kept.named.is_empty() {
            let mut named = entry.named_tags.clone();
            named.extend(kept.named);
            store.set_named_tags(&entry.local_id, &named)?;
        }
        if !kept.signals.is_empty() {
            let mut signals = entry.signals.clone();
            signals.extend(kept.signals);
            store.set_signals(&entry.local_id, &signals)?;
        }
        store.delete_meta(&key)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_object_an_array_or_one_object_per_line_is_read() {
        let one = r#"{"agent_harness":"codex","session_id":"s1","summary":"x"}"#;
        assert_eq!(records(one).unwrap().0.len(), 1);
        let pretty = "{\n  \"session_id\": \"s1\",\n  \"ticket\": \"KD-1\"\n}\n";
        assert_eq!(records(pretty).unwrap().0.len(), 1);
        let list = r#"[{"session_id":"s1"},{"session_id":"s2"}]"#;
        assert_eq!(records(list).unwrap().0.len(), 2);
        let lines = [
            r#"{"kind":"turn","session_id":"s1","turn_id":"t1","tokens":{"input":1}}"#,
            r#"{"kind":"summary","session_id":"s1","summary":"Fixed the loop"}"#,
            "not json",
            r#"{"kind":"tags","session_id":"s1","tags":{"ticket":"KD-214"}}"#,
        ]
        .join("\n");
        let (found, skipped) = records(&lines).unwrap();
        assert_eq!(found.len(), 3);
        assert_eq!(skipped.len(), 1);
        assert!(skipped.first().unwrap().starts_with("line 3:"));
        assert!(records("nope\nnope").is_err());
    }

    #[test]
    fn later_records_win() {
        let mut kept = Kept {
            summary: Some("first".into()),
            named: BTreeMap::from([("ticket".to_owned(), "KD-1".to_owned())]),
            signals: BTreeMap::new(),
        };
        kept.merge(Kept {
            summary: None,
            named: BTreeMap::from([("ticket".to_owned(), "KD-2".to_owned())]),
            signals: BTreeMap::new(),
        });
        assert_eq!(kept.summary.as_deref(), Some("first"));
        assert_eq!(kept.named.get("ticket").map(String::as_str), Some("KD-2"));
        assert!(!kept.is_empty());
        assert!(Kept::default().is_empty());
    }
}
