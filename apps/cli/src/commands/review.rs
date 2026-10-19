//! `tc review`, `tc tag` and `tc split`: the sessions since the last submit
//! with their splits, and fixing the exceptions by hand (Attribute: Check
//! and submit). Every fix is a new split on the session that supersedes the
//! rule's; `--remember` makes it a personal rule.

use serde::Serialize;
use tokencontroller_core::{
    Record, Rule, Scope, SessionId, Signal, Split, SplitPart, SplitTarget, TagName, TagValue, Tags,
    TextPattern, Then,
};

use crate::app::App;
use crate::context::PERSON;
use crate::error::{Error, Result};
use crate::log::{SessionView, Status};
use crate::output::{self, decimal, utc};

/// One part of a split in JSON: its fraction as a decimal string and its
/// tags.
#[derive(Debug, Serialize)]
pub struct SplitView {
    pub fraction: String,
    pub tags: Tags,
    pub cost: String,
}

/// A session in JSON, as the Claude Code mod and scripts read it.
#[derive(Debug, Serialize)]
pub struct SessionJson {
    pub number: i64,
    pub local_id: String,
    pub agent_harness: String,
    pub session_id: String,
    pub started_at: String,
    pub ended_at: String,
    pub model: String,
    pub provider: String,
    pub tokens: tokencontroller_core::TokenCounts,
    pub cost: Option<String>,
    pub currency: String,
    pub count_errors: usize,
    pub unknown_models: Vec<String>,
    pub summary: Option<String>,
    pub status: String,
    pub rejected_reason: Option<String>,
    pub assigned_by: String,
    pub label: String,
    pub splits: Vec<SplitView>,
    pub keys: Vec<String>,
    pub branch: Option<String>,
    pub repository: Option<String>,
    pub messages: usize,
}

impl SessionJson {
    pub fn of(session: &SessionView, currency: tokencontroller_core::Currency) -> Self {
        Self {
            number: session.number,
            local_id: session.local_id.clone(),
            agent_harness: session.harness.name().to_owned(),
            session_id: session.session_id.clone(),
            started_at: utc(session.started_at),
            ended_at: utc(session.ended_at),
            model: session.model.model.clone(),
            provider: session.model.provider.clone(),
            tokens: session.tokens,
            cost: session.cost.map(decimal),
            currency: currency.code(),
            count_errors: session.count_errors,
            unknown_models: session.unknown_models.clone(),
            summary: session.summary.clone(),
            status: session.status.as_str().to_owned(),
            rejected_reason: match &session.status {
                Status::Rejected(reason) => Some(reason.clone()),
                _ => None,
            },
            assigned_by: session.assigned_by.clone(),
            label: session.label.as_str().to_owned(),
            splits: session
                .parts
                .iter()
                .map(|part| SplitView {
                    fraction: part.fraction(),
                    tags: part.tags.clone(),
                    cost: decimal(part.amount),
                })
                .collect(),
            keys: session.signals.prompt_keys.clone(),
            branch: session.signals.branch.clone(),
            repository: session.signals.repository.clone(),
            messages: session.messages.len(),
        }
    }
}

pub fn run(app: &mut App<'_>, all: bool) -> Result<()> {
    app.prepare()?;
    let state = app.fold()?;
    let sessions: Vec<SessionView> = crate::log::sessions(&state, &app.store.sessions()?)?
        .into_iter()
        .filter(|session| all || session.status != Status::Submitted)
        .collect();
    if app.json {
        let views: Vec<SessionJson> = sessions
            .iter()
            .map(|session| SessionJson::of(session, state.currency()))
            .collect();
        return output::json(
            app.out,
            &serde_json::json!({ "sessions": views, "as_of": utc(state.as_of()), "currency": state.currency().code() }),
        );
    }
    if sessions.is_empty() {
        writeln!(
            app.out,
            "No sessions to check. New sessions show up here after they start."
        )?;
        return Ok(());
    }
    print_sessions(app, &sessions)
}

/// The review list as the docs show it, with each summary under its
/// session.
pub fn print_sessions(app: &mut App<'_>, sessions: &[SessionView]) -> Result<()> {
    let today = app.today();
    let mut rows = vec![vec![
        "#".to_owned(),
        "when".to_owned(),
        "cost".to_owned(),
        "tags".to_owned(),
        "by".to_owned(),
    ]];
    let mut summaries = Vec::new();
    for session in sessions {
        rows.push(vec![
            session.number.to_string(),
            output::when(session.started_at, today),
            cost_cell(session),
            tags_cell(session),
            by_cell(session),
        ]);
        summaries.push(session.summary.clone());
    }
    // Render the table, then put each summary under its row.
    let mut table = Vec::new();
    output::table(&mut table, &rows, &[0, 2])?;
    let text = String::from_utf8_lossy(&table).into_owned();
    let mut lines = text.lines();
    if let Some(header) = lines.next() {
        writeln!(app.out, "{header}")?;
    }
    for (line, summary) in lines.zip(summaries) {
        writeln!(app.out, "{line}")?;
        if let Some(summary) = summary {
            writeln!(app.out, "        {}", first_line(&summary))?;
        }
    }
    Ok(())
}

fn cost_cell(session: &SessionView) -> String {
    match session.cost {
        Some(cost) if session.unknown_models.is_empty() && session.count_errors == 0 => {
            output::display(cost)
        }
        Some(cost) => format!("{}+", output::display(cost)),
        None if session.count_errors > 0 => "count error".to_owned(),
        None => "no price".to_owned(),
    }
}

fn by_cell(session: &SessionView) -> String {
    match &session.status {
        Status::Rejected(reason) => format!("{} · rejected: {reason}", session.assigned_by),
        Status::Submitted => format!("{} · submitted", session.assigned_by),
        Status::Open => session.assigned_by.clone(),
    }
}

fn first_line(text: &str) -> String {
    let line = text.lines().next().unwrap_or_default().trim();
    let shown: String = line.chars().take(100).collect();
    if shown.chars().count() < line.chars().count() {
        format!("{shown}…")
    } else {
        shown
    }
}

fn tags_cell(session: &SessionView) -> String {
    let one = |tags: &Tags| {
        let text = output::tag_line(tags);
        if text.is_empty() {
            "no project".to_owned()
        } else {
            text
        }
    };
    match session.parts.as_slice() {
        [] => "no project".to_owned(),
        [only] => one(&only.tags),
        parts => parts
            .iter()
            .map(|part| {
                format!(
                    "{}% {}",
                    part.basis_points.checked_div(100).unwrap_or(0),
                    one(&part.tags)
                )
            })
            .collect::<Vec<_>>()
            .join(" | "),
    }
}

/// Checks a tag name against the organization's list.
fn allowed(app: &App<'_>, name: &str) -> Result<()> {
    if app.context.allows_tag(name) {
        return Ok(());
    }
    let list = app.context.tag_names.join(", ");
    Err(Error::usage(format!(
        "{name} is not on your organization's list of tag names ({list})"
    )))
}

/// `tc tag N name=value…`: a new split on the session with the tags
/// changed. Setting a feature makes the session one part again; other tags
/// go on every part.
pub fn tag(app: &mut App<'_>, number: i64, pairs: &[String], remember: bool) -> Result<()> {
    app.prepare()?;
    let session = editable(app, number)?;
    let mut set: Vec<(TagName, Option<TagValue>)> = Vec::new();
    for pair in pairs {
        let (name, value) = super::name_value(pair).ok_or_else(|| {
            Error::usage(format!("{pair} is not NAME=VALUE, such as feature=KD-214"))
        })?;
        allowed(app, &name)?;
        let name = TagName::new(&name)?;
        if name == TagName::person() {
            return Err(Error::usage(
                "the person comes from the session and cannot be set",
            ));
        }
        let value = if value.is_empty() {
            None
        } else {
            Some(TagValue::new(&value)?)
        };
        set.push((name, value));
    }
    let sets_feature = set
        .iter()
        .any(|(name, value)| *name == TagName::feature() && value.is_some());
    let mut parts: Vec<SplitPart> = if sets_feature || session.parts.len() <= 1 {
        vec![SplitPart {
            weight: 1,
            tags: session
                .parts
                .first()
                .map(|part| part.tags.clone())
                .unwrap_or_default(),
        }]
    } else {
        session
            .parts
            .iter()
            .map(|part| SplitPart {
                weight: part.basis_points.max(1),
                tags: part.tags.clone(),
            })
            .collect()
    };
    for part in &mut parts {
        for (name, value) in &set {
            match value {
                Some(value) => {
                    part.tags.insert(name.clone(), value.clone());
                }
                None => {
                    part.tags.remove(name);
                }
            }
        }
    }
    let split = Split {
        target: SplitTarget::Session(
            SessionId::new(&session.session_id).map_err(|error| Error::Data(error.to_string()))?,
        ),
        parts,
    };
    record_split(app, &session, split)?;
    if remember {
        let set: Vec<(TagName, TagValue)> = set
            .into_iter()
            .filter_map(|(name, value)| value.map(|value| (name, value)))
            .collect();
        remember_rule(app, &session, &set)?;
    }
    show_one(app, number)
}

/// `tc split N FEATURE=WEIGHT…`
pub fn split(app: &mut App<'_>, number: i64, pairs: &[String], remember: bool) -> Result<()> {
    app.prepare()?;
    if remember {
        return Err(Error::usage(
            "a split by weights cannot become a rule; use tc tag N feature=KEY --remember for one feature",
        ));
    }
    let session = editable(app, number)?;
    let mut parts: Vec<SplitPart> = Vec::new();
    let others: Tags = session
        .parts
        .first()
        .map(|part| part.tags.retain(|name| *name != TagName::feature()))
        .unwrap_or_default();
    for pair in pairs {
        let (feature, weight) = super::name_value(pair).ok_or_else(|| {
            Error::usage(format!("{pair} is not FEATURE=WEIGHT, such as KD-231=60"))
        })?;
        let weight: u64 = weight
            .parse()
            .ok()
            .filter(|weight| *weight > 0)
            .ok_or_else(|| {
                Error::usage(format!(
                    "the weight in {pair} must be a whole number above 0"
                ))
            })?;
        let feature = TagValue::new(&feature)?;
        if parts
            .iter()
            .any(|part| part.tags.get(&TagName::feature()) == Some(&feature))
        {
            return Err(Error::usage(format!("{feature} is named twice")));
        }
        let mut tags = others.clone();
        tags.insert(TagName::feature(), feature);
        parts.push(SplitPart { weight, tags });
    }
    if parts.len() < 2 {
        return Err(Error::usage(
            "a split needs two features or more; to set one feature use tc tag N feature=KEY",
        ));
    }
    let split = Split {
        target: SplitTarget::Session(
            SessionId::new(&session.session_id).map_err(|error| Error::Data(error.to_string()))?,
        ),
        parts,
    };
    record_split(app, &session, split)?;
    show_one(app, number)
}

/// Records a split on this machine and, connected, in the organization.
fn record_split(app: &mut App<'_>, session: &SessionView, split: Split) -> Result<()> {
    split
        .check()
        .map_err(|error| Error::usage(error.to_string()))?;
    if let Some(client) = app.client() {
        client.post_split(&serde_json::json!({
            "session_id": session.session_id,
            "agent_harness": session.harness.name(),
            "local_id": session.local_id,
            "parts": split.parts.iter().map(|part| serde_json::json!({
                "fraction": crate::log::fraction_text(fraction_of(part.weight, &split)),
                "weight": part.weight,
                "tags": part.tags,
            })).collect::<Vec<_>>(),
        }))?;
    }
    app.record(Record::Split(split), Some(&session.local_id))?;
    Ok(())
}

fn fraction_of(weight: u64, split: &Split) -> u64 {
    let sum = split
        .parts
        .iter()
        .fold(0_u64, |sum, part| sum.saturating_add(part.weight))
        .max(1);
    weight.saturating_mul(10_000).checked_div(sum).unwrap_or(0)
}

/// `--remember`: a personal rule per tag set, from the session's signals:
/// the branch for a feature, the repository for anything else.
fn remember_rule(
    app: &mut App<'_>,
    session: &SessionView,
    set: &[(TagName, TagValue)],
) -> Result<()> {
    let signals = &session.signals;
    let mut rules: Vec<Rule> = Vec::new();
    for (name, value) in set {
        let (signal, text) = if *name == TagName::feature() {
            (Signal::Branch, signals.branch.as_deref())
        } else {
            (Signal::Repository, signals.repository.as_deref())
        };
        let Some(text) = text.filter(|text| !text.is_empty()) else {
            return Err(Error::usage(format!(
                "session {} has no signal to remember {name}={value} by",
                session.number
            )));
        };
        let word = match signal {
            Signal::Branch => "branch",
            _ => "repository",
        };
        let pattern = TextPattern::new(&format!("^{}$", regex::escape(text)))
            .map_err(|error| Error::Data(error.to_string()))?;
        rules.push(Rule {
            name: format!("{word} is {text}"),
            scope: Scope::Person(TagValue::new(PERSON)?),
            order: 0,
            signal,
            pattern,
            then: Then::Tag {
                name: name.clone(),
                key: value.clone(),
            },
        });
    }
    let mut personal = super::rules::personal(app)?;
    personal.extend(rules);
    super::rules::save(app, &personal)
}

fn editable(app: &App<'_>, number: i64) -> Result<SessionView> {
    let session = app.session(number)?;
    if session.status == Status::Submitted {
        return Err(Error::usage(format!(
            "session {number} is submitted; a new split on it still goes in the log, but ask your people manager to reject the submission if the figures must change"
        )));
    }
    Ok(session)
}

fn show_one(app: &mut App<'_>, number: i64) -> Result<()> {
    let session = app.session(number)?;
    if app.json {
        let currency = app.fold()?.currency();
        return output::json(
            app.out,
            &serde_json::json!({ "session": SessionJson::of(&session, currency) }),
        );
    }
    print_sessions(app, &[session])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_summaries_are_cut_to_one_line() {
        assert_eq!(first_line("Fixed it\nmore"), "Fixed it");
        let long = "x".repeat(150);
        let cut = first_line(&long);
        assert_eq!(cut.chars().count(), 101);
        assert!(cut.ends_with('…'));
        assert_eq!(first_line(&"y".repeat(100)).chars().count(), 100);
    }

    #[test]
    fn weights_become_fractions_in_basis_points() {
        let split = Split {
            target: SplitTarget::Session(SessionId::new("s").unwrap()),
            parts: vec![
                SplitPart {
                    weight: 60,
                    tags: Tags::new(),
                },
                SplitPart {
                    weight: 40,
                    tags: Tags::new(),
                },
            ],
        };
        assert_eq!(fraction_of(60, &split), 6_000);
        assert_eq!(fraction_of(40, &split), 4_000);
    }

    #[test]
    fn the_json_is_what_the_mod_reads() {
        let (_dir, _state, views) = crate::log::fixtures::folded(
            &crate::log::Run::default(),
            &crate::context::Context::default(),
            false,
        );
        let s3 = SessionJson::of(&views[2], tokencontroller_core::Currency::USD);
        assert_eq!(s3.cost.as_deref(), Some("4.50"));
        assert_eq!(s3.currency, "USD");
        assert_eq!(s3.status, "open");
        assert_eq!(s3.rejected_reason, None);
        assert_eq!(s3.assigned_by, "hand");
        assert_eq!(s3.label, "direct");
        assert_eq!(s3.splits[0].fraction, "0.6");
        assert_eq!(s3.splits[0].cost, "2.70");
        assert_eq!(s3.messages, 1);
        let json = serde_json::to_value(&s3).unwrap();
        assert_eq!(json["cost"], "4.50");
        assert_eq!(json["splits"][1]["tags"]["feature"], "KD-8");
        assert_eq!(json["agent_harness"], "claude-code");
        let s2 = SessionJson::of(&views[1], tokencontroller_core::Currency::USD);
        assert_eq!(s2.keys, ["KD-1", "KD-2"]);
        assert_eq!(s2.assigned_by, "rule: key in prompts");
    }

    #[test]
    fn a_rejected_session_says_why() {
        let (_dir, _state, mut views) = crate::log::fixtures::folded(
            &crate::log::Run::default(),
            &crate::context::Context::default(),
            false,
        );
        views[0].status = Status::Rejected("wrong client".into());
        let json = SessionJson::of(&views[0], tokencontroller_core::Currency::EUR);
        assert_eq!(json.status, "rejected");
        assert_eq!(json.rejected_reason.as_deref(), Some("wrong client"));
        assert_eq!(json.currency, "EUR");
    }
}
