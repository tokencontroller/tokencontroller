//! `tc record`: what the Claude Code mod, the GitHub Action or another tool
//! knows about a session that its files do not say: the summary the mod
//! writes, tags and a ticket picked in the session, the pull request. Reads
//! one JSON object on standard input. A pull request title is searched for
//! ticket keys here and only the keys are kept.

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
    pub agent_harness: String,
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

pub fn run(app: &mut App<'_>, input: &mut dyn Read) -> Result<()> {
    let mut text = String::new();
    input
        .read_to_string(&mut text)
        .map_err(|source| Error::file("standard input", source))?;
    let record: Record = serde_json::from_str(&text)
        .map_err(|error| Error::usage(format!("tc record reads one JSON object: {error}")))?;
    let harness = Harness::from_name(&record.agent_harness).ok_or_else(|| {
        Error::usage(format!(
            "{} is not an agent harness tc reads; use claude-code or codex",
            record.agent_harness
        ))
    })?;
    app.refresh_context(false)?;
    let patterns = app.patterns();
    let session_id = record.session_id.clone();
    let kept = keep(app, record, &patterns);
    let key = format!("{PENDING}{}", local_id(harness, &session_id));
    app.store.set_meta(&key, &serde_json::to_string(&kept)?)?;
    app.prepare()?;
    let entry = match app.store.entry_by_session(harness, &session_id)? {
        Some(row) => Some(app.entry(row.number)?),
        None => None,
    };
    if app.json {
        return output::json(
            app.out,
            &serde_json::json!({ "token_entry": entry.as_ref().map(EntryView::of) }),
        );
    }
    match entry {
        Some(entry) => writeln!(app.out, "recorded on token entry {}", entry.row.number)?,
        None => writeln!(
            app.out,
            "recorded; it goes on the session's token entry once the session's file is read"
        )?,
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
