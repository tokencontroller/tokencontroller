//! `tc submit`: sends the reviewed drafts as the token sheet. With a service
//! token (`tc_svc_…`, as in CI) each entry is reported as an automatic token
//! entry instead, which never goes on a token sheet.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::value::RawValue;

use crate::amounts::decimal;
use crate::app::App;
use crate::entries::{CURRENCY, Entry};
use crate::error::{Error, Result};
use crate::output::{self, percent, utc};
use crate::store::{Status, StoredMessage};
use crate::tokens::TokenKinds;

/// A token entry as the service takes it: the token entry schema from the
/// docs, plus the per-message rows (ADR 0006), the splits and how it was
/// placed. Tags named by hand go in `ticket`, `project` and `tags`, where
/// they win over every rule; the machine's own placement is in `splits`.
#[derive(Debug, Serialize)]
pub struct Outgoing {
    pub local_id: String,
    pub agent_harness: String,
    pub model: String,
    pub started_at: String,
    pub ended_at: String,
    pub tokens: TokenKinds,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reported_cost: Option<Box<RawValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ticket: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    pub tags: BTreeMap<String, String>,
    pub signals: OutgoingSignals,
    pub session_id: String,
    pub messages: Vec<OutgoingMessage>,
    pub splits: Vec<OutgoingSplit>,
    pub placed_by: String,
}

#[derive(Debug, Serialize, Default)]
pub struct OutgoingSignals {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_directory: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pull_request: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagent_type: Option<String>,
    /// Ticket keys found in prompts, the session title and the pull
    /// request title. Only the keys, never the text.
    pub ticket_keys: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct OutgoingMessage {
    pub message_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    pub at: String,
    pub model: String,
    pub tokens: TokenKinds,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reported_cost: Option<Box<RawValue>>,
}

#[derive(Debug, Serialize)]
pub struct OutgoingSplit {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub share_percent: Option<u64>,
    pub tags: BTreeMap<String, String>,
    pub signals: OutgoingSignals,
}

/// A decimal amount as a JSON number, written exactly as the digits.
fn number(nanos: i64) -> Option<Box<RawValue>> {
    let text = nano_decimal(nanos);
    RawValue::from_string(text).ok()
}

/// All nine decimals, trailing zeros removed: `0.031`, `12`.
fn nano_decimal(nanos: i64) -> String {
    let sign = if nanos.is_negative() { "-" } else { "" };
    let magnitude = nanos.unsigned_abs();
    let whole = magnitude.checked_div(1_000_000_000).unwrap_or(0);
    let fraction = magnitude.checked_rem(1_000_000_000).unwrap_or(0);
    if fraction == 0 {
        return format!("{sign}{whole}");
    }
    let digits = format!("{fraction:09}");
    format!("{sign}{whole}.{}", digits.trim_end_matches('0'))
}

fn signals_of(split: &crate::entries::Split) -> OutgoingSignals {
    let signals = &split.signals;
    let mut keys: Vec<String> = signals
        .prompt_keys
        .iter()
        .map(|count| count.key.clone())
        .collect();
    for key in &signals.pull_request_title_keys {
        if !keys.contains(key) {
            keys.push(key.clone());
        }
    }
    OutgoingSignals {
        branch: signals.branch.clone(),
        working_directory: signals.directory.clone(),
        repository: signals.repository.clone(),
        pull_request: signals.pull_request.clone(),
        subagent_type: signals.subagent.clone(),
        ticket_keys: keys,
    }
}

fn message_of(message: &StoredMessage) -> OutgoingMessage {
    OutgoingMessage {
        message_id: message.message_id.clone(),
        request_id: message.request_id.clone(),
        at: utc(message.at_ms),
        model: message.model.clone(),
        tokens: message.tokens.kinds(),
        reported_cost: message.reported_cost.and_then(number),
    }
}

/// The token entry as it is sent.
pub fn outgoing(entry: &Entry) -> Outgoing {
    let mut named = entry.named.clone();
    let ticket = named.remove("ticket");
    let project = named.remove("project");
    let reported: Option<i64> = entry
        .messages
        .iter()
        .map(|message| message.reported_cost)
        .try_fold(0_i64, |sum, cost| cost.map(|cost| sum.saturating_add(cost)));
    let main = entry
        .splits
        .iter()
        .max_by_key(|split| split.share_percent)
        .map(signals_of)
        .unwrap_or_default();
    Outgoing {
        local_id: entry.row.local_id.clone(),
        agent_harness: entry.row.harness.name().to_owned(),
        model: entry.model.clone(),
        started_at: utc(entry.started_ms),
        ended_at: utc(entry.ended_ms),
        tokens: entry.tokens.kinds(),
        reported_cost: reported
            .filter(|_| !entry.messages.is_empty())
            .and_then(number),
        summary: entry.summary.clone(),
        ticket,
        project,
        tags: named,
        signals: main,
        session_id: entry.row.session_id.clone(),
        messages: entry.messages.iter().map(message_of).collect(),
        splits: entry
            .splits
            .iter()
            .map(|split| OutgoingSplit {
                message_ids: split.message_ids.clone(),
                share_percent: split.message_ids.is_none().then_some(split.share_percent),
                tags: split.tags.clone(),
                signals: signals_of(split),
            })
            .collect(),
        placed_by: entry.placed_by.clone(),
    }
}

/// Adds `--tag` and `--signal` values to this run.
fn name_run(app: &mut App<'_>, tags: &[String], signals: &[String]) -> Result<()> {
    for pair in tags {
        let (name, value) = super::name_value(pair)
            .filter(|(_, value)| !value.is_empty())
            .ok_or_else(|| {
                Error::usage(format!("{pair} is not NAME=VALUE, such as ticket=KD-214"))
            })?;
        if !app.context.allows_tag(&name) {
            return Err(Error::usage(format!(
                "{name} is not on your organization's list of tag names"
            )));
        }
        app.run.tags.insert(name, value);
    }
    for pair in signals {
        let (name, value) = super::name_value(pair)
            .filter(|(_, value)| !value.is_empty())
            .ok_or_else(|| {
                Error::usage(format!("{pair} is not NAME=VALUE, such as pull_request=12"))
            })?;
        if !crate::entries::RUN_SIGNALS.contains(&name.as_str()) {
            return Err(Error::usage(format!(
                "{name} is not a signal tc submit takes; use {}",
                crate::entries::RUN_SIGNALS.join(", ")
            )));
        }
        app.run.signals.insert(name, value);
    }
    Ok(())
}

pub fn run(app: &mut App<'_>, tags: &[String], signals: &[String]) -> Result<()> {
    app.refresh_context(false)?;
    name_run(app, tags, signals)?;
    app.prepare()?;
    let entries = app.entries(&[Status::Draft, Status::Rejected])?;
    if entries.is_empty() {
        if app.json {
            return output::json(
                app.out,
                &serde_json::json!({"submitted": 0, "list_value": "0.00", "currency": CURRENCY.code(), "share_on_tickets_percent": 0}),
            );
        }
        writeln!(
            app.out,
            "Nothing to submit: there are no draft token entries."
        )?;
        return Ok(());
    }
    let connection = app.connection().ok_or_else(|| {
        Error::usage(
            "tc is not connected to a service. Connect with tc connect URL, or report by hand with tc export --csv.",
        )
    })?;
    let client = crate::api::Client::new(&connection.service, Some(connection.token.clone()));
    let payload: Vec<Outgoing> = entries.iter().map(outgoing).collect();
    let mut submitted: Vec<(String, Option<String>)> = Vec::new();
    if connection.is_service_token() {
        for entry in &payload {
            let created = client.post_token_entry(entry)?;
            submitted.push((entry.local_id.clone(), created.id));
            // Mark as it goes, so a failure halfway sends nothing twice.
            app.store.mark_submitted(
                &[(
                    entry.local_id.clone(),
                    submitted.last().and_then(|s| s.1.clone()),
                )],
                app.now_ms,
            )?;
        }
    } else {
        let answer = client.submit(&payload)?;
        for entry in &payload {
            let id = answer
                .token_entries
                .iter()
                .find(|answered| answered.local_id == entry.local_id)
                .and_then(|answered| answered.id.clone());
            submitted.push((entry.local_id.clone(), id));
        }
        app.store.mark_submitted(&submitted, app.now_ms)?;
    }

    let total = entries
        .iter()
        .fold(0_i64, |sum, entry| sum.saturating_add(entry.value.nanos));
    let on_tickets = entries.iter().fold(0_i64, |sum, entry| {
        sum.saturating_add(entry.on_tickets_nanos())
    });
    let share = percent(on_tickets, total);
    let what = if connection.is_service_token() {
        "reported"
    } else {
        "submitted token sheet"
    };
    if app.json {
        return output::json(
            app.out,
            &serde_json::json!({
                "submitted": entries.len(),
                "list_value": decimal(total, CURRENCY),
                "currency": CURRENCY.code(),
                "share_on_tickets_percent": share,
            }),
        );
    }
    let count = entries.len();
    let noun = if count == 1 {
        "token entry"
    } else {
        "token entries"
    };
    writeln!(
        app.out,
        "{what} · {count} {noun} · {} · {share}% on tickets",
        crate::amounts::display(total, CURRENCY)
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts_go_out_as_exact_numbers() {
        assert_eq!(nano_decimal(31_000_000), "0.031");
        assert_eq!(nano_decimal(12_000_000_000), "12");
        assert_eq!(nano_decimal(-1_500_000_000), "-1.5");
        assert_eq!(nano_decimal(1), "0.000000001");
        assert_eq!(number(31_000_000).unwrap().get(), "0.031");
    }
}
