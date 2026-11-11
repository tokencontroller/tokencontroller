//! `tc record`: what the Claude Code mod or another tool knows about a
//! session that its files do not say: the summary the mod writes, the
//! session facts, a feature or tags picked in the session, the pull request.
//! Reads JSON on standard input: one object, an array, or one object per
//! line. A pull request title is kept for the rules; only the keys found in
//! it leave the machine.
//!
//! A session named here counts even outside the tracked folders: the mod
//! knows its own session, wherever it runs. Token counts never come from
//! here: tc reads every message from the session files.

use std::collections::BTreeMap;
use std::io::Read;

use serde::Deserialize;

use crate::app::{App, RECORD};
use crate::error::{Error, Result};
use crate::log::{Facts, Recorded};
use crate::output;
use crate::sources::Harness;
use crate::store::local_id;

#[derive(Debug, Deserialize)]
pub struct Record {
    #[serde(default)]
    pub agent_harness: Option<String>,
    pub session_id: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub facts: Facts,
    #[serde(default)]
    pub feature: Option<String>,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub tags: BTreeMap<String, String>,
    #[serde(default)]
    pub pull_request: Option<serde_json::Value>,
    #[serde(default)]
    pub pull_request_title: Option<String>,
    /// Signals the tool knows about the session, such as the sign-in at
    /// session start (`sign_in`, `organization.id`). The first value of a
    /// name stays. Short values only: never prompt text.
    #[serde(default)]
    pub signals: BTreeMap<String, serde_json::Value>,
}

/// The longest signal value `tc record` keeps.
const MAX_SIGNAL_LEN: usize = 200;

/// A signal name as the service takes it: letters, digits, `.`, `_`, `-`.
fn is_signal_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
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

    let mut sessions: Vec<(Harness, String)> = Vec::new();
    let mut taken = 0_usize;
    for record in records {
        let name = record.agent_harness.as_deref().unwrap_or("claude-code");
        let harness = Harness::from_name(name).ok_or_else(|| {
            Error::usage(format!(
                "{name} is not an agent harness tc reads; use claude-code or codex"
            ))
        })?;
        taken = taken.saturating_add(1);
        let session_id = record.session_id.trim().to_owned();
        if session_id.is_empty() {
            continue;
        }
        // A session the mod or another tool reports counts in any folder.
        app.store.mark_recorded(harness, &session_id)?;
        if !sessions
            .iter()
            .any(|(h, s)| *h == harness && *s == session_id)
        {
            sessions.push((harness, session_id.clone()));
        }
        let kept = keep(app, record);
        if kept.is_empty() {
            continue;
        }
        let key = format!("{RECORD}{}", local_id(harness, &session_id));
        let mut waiting: Recorded = app
            .store
            .meta(&key)?
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        waiting.merge(kept);
        app.store
            .set_meta(&key, &serde_json::to_string(&waiting)?)?;
    }
    app.read_sessions()?;

    let state = app.fold()?;
    let all = crate::log::sessions(&state, &app.store.sessions()?)?;
    let views: Vec<&crate::log::SessionView> = sessions
        .iter()
        .filter_map(|(harness, session_id)| {
            all.iter()
                .find(|view| view.harness == *harness && view.session_id == *session_id)
        })
        .collect();
    if app.json {
        let views: Vec<super::review::SessionJson> = views
            .iter()
            .map(|view| super::review::SessionJson::of(view, state.currency()))
            .collect();
        return output::json(
            app.out,
            &serde_json::json!({ "recorded": taken, "sessions": views }),
        );
    }
    let numbers: Vec<String> = views.iter().map(|view| view.number.to_string()).collect();
    if numbers.is_empty() {
        writeln!(
            app.out,
            "recorded {taken}; it goes on the session once the session's file is read"
        )?;
    } else {
        writeln!(
            app.out,
            "recorded {taken} on session {}",
            numbers.join(", ")
        )?;
    }
    Ok(())
}

fn keep(app: &App<'_>, record: Record) -> Recorded {
    let mut tags = BTreeMap::new();
    for (name, value) in record.tags {
        if app.context.allows_tag(&name) && !value.trim().is_empty() {
            tags.insert(name, value.trim().to_owned());
        }
    }
    if let Some(feature) = record.feature.filter(|f| !f.trim().is_empty()) {
        tags.insert("feature".to_owned(), feature.trim().to_owned());
    }
    if let Some(project) = record.project.filter(|p| !p.trim().is_empty()) {
        tags.insert("project".to_owned(), project.trim().to_owned());
    }
    let pull_request = match record.pull_request {
        Some(serde_json::Value::Number(number)) => Some(number.to_string()),
        Some(serde_json::Value::String(text)) if !text.trim().is_empty() => {
            Some(text.trim().to_owned())
        }
        _ => None,
    };
    Recorded {
        summary: record
            .summary
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty()),
        tags,
        pull_request,
        pull_request_title: record
            .pull_request_title
            .filter(|title| !title.trim().is_empty()),
        facts: record.facts,
        signals: signals(record.signals),
    }
}

/// The recorded signals that are kept: named as the service takes them,
/// and short. Text and numbers are kept as text.
fn signals(given: BTreeMap<String, serde_json::Value>) -> BTreeMap<String, String> {
    given
        .into_iter()
        .filter(|(name, _)| is_signal_name(name))
        .filter_map(|(name, value)| {
            let text = match value {
                serde_json::Value::String(text) => text.trim().to_owned(),
                serde_json::Value::Number(number) => number.to_string(),
                serde_json::Value::Bool(flag) => flag.to_string(),
                _ => return None,
            };
            (!text.is_empty() && text.chars().count() <= MAX_SIGNAL_LEN).then_some((name, text))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_short_named_signals_are_kept() {
        let (found, _) = records(
            r#"{"session_id":"s1","signals":{"sign_in":"claude.ai","organization.id":" org-1 ","seats":3,"bad name":"x","prompt":{"text":"no"},"long":"LONG"}}"#
                .replace("LONG", &"x".repeat(201))
                .as_str(),
        )
        .unwrap();
        let kept = signals(found.into_iter().next().unwrap().signals);
        assert_eq!(
            kept,
            BTreeMap::from([
                ("organization.id".to_owned(), "org-1".to_owned()),
                ("seats".to_owned(), "3".to_owned()),
                ("sign_in".to_owned(), "claude.ai".to_owned()),
            ])
        );
    }

    #[test]
    fn one_object_an_array_or_one_object_per_line_is_read() {
        let one =
            r#"{"agent_harness":"codex","session_id":"s1","summary":"x","facts":{"turns":3}}"#;
        let (found, _) = records(one).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found.first().unwrap().facts.turns, Some(3));
        let pretty = "{\n  \"session_id\": \"s1\",\n  \"feature\": \"KD-1\"\n}\n";
        assert_eq!(records(pretty).unwrap().0.len(), 1);
        let list = r#"[{"session_id":"s1"},{"session_id":"s2"}]"#;
        assert_eq!(records(list).unwrap().0.len(), 2);
        let lines = [
            r#"{"session_id":"s1","summary":"Fixed the loop"}"#,
            "not json",
            r#"{"session_id":"s1","tags":{"purpose":"spike"}}"#,
        ]
        .join("\n");
        let (found, skipped) = records(&lines).unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(skipped.len(), 1);
        assert!(skipped.first().unwrap().starts_with("line 2:"));
        assert!(records("nope\nnope").is_err());
    }
}
