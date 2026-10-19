//! `tc submit`: sends every open session as token entries, one per session
//! with its messages, then submits the window they fall in. With a service
//! token (`tc_svc_…`, as in CI) the messages are reported as the token's
//! creator and nothing is submitted.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::value::RawValue;
use tokencontroller_core::{
    DateRange, Money, Record, Submit, TagName, TagValue, Tags, TokenCounts,
};

use crate::app::App;
use crate::context::PERSON;
use crate::error::{Error, Result};
use crate::log::{SessionView, Status};
use crate::output::{self, decimal, percent, utc};

/// A token entry as the service takes it: one session record with its
/// messages (Reference: Token entry schema).
#[derive(Debug, Serialize)]
pub struct TokenEntry {
    pub local_id: String,
    pub session_id: String,
    pub agent_harness: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    pub provider: String,
    pub model: String,
    pub started_at: String,
    pub ended_at: String,
    pub tokens: TokenCounts,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reported_cost: Option<Box<RawValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub facts: Facts,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    pub tags: BTreeMap<String, String>,
    pub signals: EntrySignals,
    pub messages: Vec<EntryMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub splits: Vec<EntrySplit>,
}

#[derive(Debug, Serialize, Default)]
pub struct Facts {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turns: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retries: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagents: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files_touched: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
}

#[derive(Debug, Serialize, Default)]
pub struct EntrySignals {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_directory: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pull_request: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pull_request_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagent_type: Option<String>,
    /// Keys found in prompts, the session title and the pull request
    /// title. Only the keys, never the text.
    pub keys: Vec<String>,
    pub config: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
pub struct EntryMessage {
    pub message_id: String,
    pub at: String,
    pub model: String,
    pub tokens: TokenCounts,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reported_cost: Option<Box<RawValue>>,
}

#[derive(Debug, Serialize)]
pub struct EntrySplit {
    pub fraction: String,
    pub tags: BTreeMap<String, String>,
}

/// A decimal amount as a JSON number, written exactly as the digits.
fn number(amount: Money) -> Option<Box<RawValue>> {
    RawValue::from_string(nano_decimal(amount.nanos())).ok()
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

fn plain(tags: &Tags) -> BTreeMap<String, String> {
    tags.iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect()
}

/// The token entry of a session as it is sent. The tags given with the run
/// and the tags set by hand go in `feature`, `project` and `tags`, where
/// they win over every rule; a split by hand goes in `splits`.
pub fn token_entry(session: &SessionView) -> TokenEntry {
    let signals = &session.signals;
    let hand: Tags = session
        .hand_split
        .as_ref()
        .filter(|split| split.parts.len() == 1)
        .and_then(|split| split.parts.first().map(|part| part.tags.clone()))
        .unwrap_or_default();
    let mut tags = plain(&session.tags);
    tags.extend(plain(&hand));
    let feature = tags.remove(TagName::feature().as_str());
    let project = tags.remove(TagName::project().as_str());
    let splits: Vec<EntrySplit> = session
        .hand_split
        .as_ref()
        .filter(|split| split.parts.len() > 1)
        .map(|split| {
            let sum = split
                .parts
                .iter()
                .fold(0_u64, |sum, part| sum.saturating_add(part.weight))
                .max(1);
            split
                .parts
                .iter()
                .map(|part| EntrySplit {
                    fraction: crate::log::fraction_text(
                        part.weight
                            .saturating_mul(10_000)
                            .checked_div(sum)
                            .unwrap_or(0),
                    ),
                    tags: plain(&part.tags),
                })
                .collect()
        })
        .unwrap_or_default();
    TokenEntry {
        local_id: session.local_id.clone(),
        session_id: session.session_id.clone(),
        agent_harness: session.harness.name().to_owned(),
        agent_name: None,
        provider: session.model.provider.clone(),
        model: session.model.model.clone(),
        started_at: utc(session.started_at),
        ended_at: utc(session.ended_at),
        tokens: session.tokens,
        reported_cost: session.reported_cost.and_then(number),
        summary: session.summary.clone(),
        facts: Facts {
            turns: session.facts.turns,
            tool_calls: session.facts.tool_calls,
            retries: session.facts.retries,
            duration_seconds: session.facts.duration_seconds,
            subagents: session.facts.subagents,
            files_touched: session.facts.files_touched,
            result: session.facts.result.clone(),
        },
        feature,
        project,
        tags,
        signals: EntrySignals {
            branch: signals.branch.clone(),
            working_directory: signals.resource.get("working_directory").cloned(),
            repository: signals.repository.clone(),
            pull_request: signals
                .resource
                .get("pull_request")
                .and_then(|number| number.parse().ok()),
            pull_request_title: signals.pull_request_title.clone(),
            subagent_type: session.subagent_types.first().cloned(),
            keys: signals.prompt_keys.clone(),
            config: signals.config.clone(),
        },
        messages: session
            .messages
            .iter()
            .map(|message| EntryMessage {
                message_id: message.message_id.clone(),
                at: utc(message.at),
                model: message.model.model.clone(),
                tokens: message.tokens,
                reported_cost: message.reported_cost.and_then(number),
            })
            .collect(),
        splits,
    }
}

/// Adds `--tag` and `--signal` values to this run.
fn name_run(app: &mut App<'_>, tags: &[String], signals: &[String]) -> Result<()> {
    for pair in tags {
        let (name, value) = super::name_value(pair)
            .filter(|(_, value)| !value.is_empty())
            .ok_or_else(|| {
                Error::usage(format!("{pair} is not NAME=VALUE, such as feature=KD-214"))
            })?;
        if !app.context.allows_tag(&name) {
            return Err(Error::usage(format!(
                "{name} is not on your organization's list of tag names"
            )));
        }
        app.run
            .tags
            .insert(TagName::new(&name)?, TagValue::new(&value)?);
    }
    for pair in signals {
        let (name, value) = super::name_value(pair)
            .filter(|(_, value)| !value.is_empty())
            .ok_or_else(|| {
                Error::usage(format!("{pair} is not NAME=VALUE, such as pull_request=12"))
            })?;
        app.run.set_signal(&name, &value)?;
    }
    Ok(())
}

pub fn run(app: &mut App<'_>, tags: &[String], signals: &[String]) -> Result<()> {
    app.refresh_context(false)?;
    name_run(app, tags, signals)?;
    app.prepare()?;
    let state = app.fold()?;
    let currency = state.currency();
    let open: Vec<SessionView> = crate::log::sessions(&state, &app.store.sessions()?)?
        .into_iter()
        .filter(|session| session.status != Status::Submitted)
        .collect();
    let connection = app.connection();
    if open.is_empty() {
        return nothing_to_submit(app, connection.as_ref(), currency);
    }
    let Some(connection) = connection else {
        return Err(Error::usage(
            "tc is not connected to a service. Connect with tc connect URL, or report by hand with tc export --csv.",
        ));
    };
    let client = crate::api::Client::new(&connection.service, Some(connection.token.clone()));
    let entries: Vec<TokenEntry> = open.iter().map(token_entry).collect();
    let saved = client.post_token_entries(&entries)?;
    for (entry, answer) in entries.iter().zip(&saved) {
        if !answer.unknown_tags.is_empty() {
            eprintln!(
                "tc: {}: no tag value yet for {}; someone with assign can add it",
                entry.local_id,
                answer.unknown_tags.join(", ")
            );
        }
    }
    let window = window_of(&open, app.today())?;
    let submission_id = if connection.is_service_token() {
        None
    } else {
        let answer = client.post_submission(&window.from.to_string(), &window.to.to_string())?;
        app.context_is_stale()?;
        answer.id
    };
    let mut recorded = app.record(
        Record::Submit(Submit {
            person: TagValue::new(PERSON)?,
            window,
        }),
        None,
    )?;
    if let Some(id) = submission_id {
        // The service's id, so later context reads name the same submit.
        recorded.id =
            tokencontroller_core::EventId::new(&format!("submission-{id}")).unwrap_or(recorded.id);
    }

    let mut total = Money::zero(currency);
    let mut on_features = Money::zero(currency);
    for session in &open {
        if let Some(cost) = session.cost {
            total = total.checked_add(cost)?;
        }
        on_features = on_features.checked_add(session.on_features())?;
    }
    let share = percent(on_features.nanos(), total.nanos());
    let what = if connection.is_service_token() {
        "reported"
    } else {
        "submitted"
    };
    if app.json {
        return output::json(
            app.out,
            &serde_json::json!({
                "submitted": open.len(),
                "cost": decimal(total),
                "currency": currency.code(),
                "share_on_features_percent": share,
                "from": window.from.to_string(),
                "to": window.to.to_string(),
            }),
        );
    }
    let count = open.len();
    let noun = if count == 1 { "session" } else { "sessions" };
    writeln!(
        app.out,
        "{what} · {count} {noun} · {} · {share}% on features",
        output::display(total)
    )?;
    Ok(())
}

/// The window a submit covers: from the first day of the open sessions to
/// today.
fn window_of(open: &[SessionView], today: time::Date) -> Result<DateRange> {
    let from = open
        .iter()
        .map(|session| session.started_at.date())
        .min()
        .unwrap_or(today)
        .min(today);
    DateRange::new(from, today).map_err(|error| Error::Data(error.to_string()))
}

/// No open sessions. Signed in as a person, `tc` still submits today, so
/// the service knows there was nothing to add.
fn nothing_to_submit(
    app: &mut App<'_>,
    connection: Option<&crate::app::Connection>,
    currency: tokencontroller_core::Currency,
) -> Result<()> {
    let connection = connection.filter(|connection| !connection.is_service_token());
    if let Some(connection) = connection {
        let client = crate::api::Client::new(&connection.service, Some(connection.token.clone()));
        let today = app.today().to_string();
        client.post_submission(&today, &today)?;
        app.context_is_stale()?;
    }
    if app.json {
        return output::json(
            app.out,
            &serde_json::json!({"submitted": 0, "cost": "0.00", "currency": currency.code(), "share_on_features_percent": 0}),
        );
    }
    if connection.is_some() {
        writeln!(app.out, "submitted · nothing to add")?;
    } else {
        writeln!(app.out, "Nothing to submit: there are no open sessions.")?;
    }
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
        let usd = Money::from_nanos(31_000_000, tokencontroller_core::Currency::USD);
        assert_eq!(number(usd).unwrap().get(), "0.031");
    }

    #[test]
    fn the_window_runs_from_the_first_open_session_to_today() {
        let today = time::macros::date!(2026 - 10 - 19);
        assert_eq!(window_of(&[], today).unwrap(), DateRange::day(today));
    }

    use crate::log::Run;

    fn folded(run: &Run) -> (tempfile::TempDir, Vec<SessionView>) {
        let (dir, _state, views) =
            crate::log::fixtures::folded(run, &crate::context::Context::default(), false);
        (dir, views)
    }

    #[test]
    fn entries_carry_the_run_tags_the_rule_free_feature_and_the_hand_split() {
        let run = Run {
            tags: Tags::parse(["purpose=code-review", "project=shop"]).unwrap(),
            ..Run::default()
        };
        let (_dir, views) = folded(&run);
        let s1 = token_entry(&views[0]);
        assert_eq!(s1.agent_harness, "claude-code");
        assert_eq!(s1.provider, "anthropic");
        assert_eq!(
            s1.tags.get("purpose").map(String::as_str),
            Some("code-review")
        );
        assert_eq!(s1.project.as_deref(), Some("shop"));
        // The rule's feature is the service's to find again.
        assert_eq!(s1.feature, None);
        assert!(s1.splits.is_empty());
        assert_eq!(s1.tokens.input, 2_000_000);
        assert_eq!(s1.messages.len(), 2);
        assert_eq!(s1.reported_cost.unwrap().get(), "1");
        assert_eq!(s1.summary.as_deref(), Some("Fix checkout"));
        let s3 = token_entry(&views[2]);
        assert_eq!(s3.splits.len(), 2);
        assert_eq!(s3.splits[0].fraction, "0.6");
        assert_eq!(
            s3.splits[0].tags.get("feature").map(String::as_str),
            Some("KD-9")
        );
        assert_eq!(s3.splits[1].fraction, "0.4");
        assert_eq!(s3.feature, None);
        assert_eq!(s3.facts.turns, Some(3));
        let json = serde_json::to_value(&s3).unwrap();
        assert_eq!(json["reported_cost"], serde_json::json!(0.5));
        assert_eq!(json["splits"][0]["fraction"], "0.6");
        assert!(json.get("feature").is_none());
    }

    #[test]
    fn signals_go_out_under_the_server_s_names() {
        let mut run = Run::default();
        run.set_signal("repository", "github.com/kestrel/ops")
            .unwrap();
        run.set_signal("pull_request", "12").unwrap();
        run.set_signal("pull_request_title", "KD-300: tidy the cart")
            .unwrap();
        let (_dir, views) = folded(&run);
        let signals = token_entry(&views[0]).signals;
        assert_eq!(signals.repository.as_deref(), Some("kestrel/ops"));
        assert_eq!(signals.pull_request, Some(12));
        assert_eq!(
            signals.pull_request_title.as_deref(),
            Some("KD-300: tidy the cart")
        );
        assert_eq!(signals.branch.as_deref(), Some("KD-214-fix"));
        assert_eq!(signals.working_directory.as_deref(), Some("/w"));
        assert!(signals.keys.contains(&"KD-300".to_owned()));
        let json = serde_json::to_value(&signals).unwrap();
        assert!(json.get("subagent_type").is_none());
        assert_eq!(json["pull_request"], 12);
    }

    #[test]
    fn the_window_starts_with_the_first_open_session() {
        let (_dir, views) = folded(&Run::default());
        let today = time::macros::date!(2026 - 10 - 19);
        let window = window_of(&views, today).unwrap();
        assert_eq!(
            window,
            DateRange::new(time::macros::date!(2026 - 04 - 12), today).unwrap()
        );
        let earlier = time::macros::date!(2026 - 01 - 01);
        assert_eq!(window_of(&views, earlier).unwrap(), DateRange::day(earlier));
    }
}
