//! Golden cases: one folder per scenario in `tests/golden`. Each case is an
//! event log written as CSV files, one per kind of event, and a list of
//! queries; it holds what the fold must answer. `tests/golden/README.md`
//! describes the files.
//!
//! Every answer this test computes is also written to
//! `target/tmp/golden/<case>/answers.csv`, so a new case's expected file can
//! be checked by hand against the spec and then copied in.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use time::format_description::well_known::Rfc3339;
use time::{Date, OffsetDateTime};
use tokencontroller_core::{
    Accept, Approval, AttributeValue, Bill, Capability, DateRange, Decision, Distribution,
    Estimate, Event, EventId, Filter, Grant, Message, MessageId, ModelRef, Money, PersonAttribute,
    PriceRow, Record, Rule, Scope, SessionId, Share, Signal, Split, SplitPart, SplitTarget, State,
    Submit, TagName, TagValue, TagValueRecord, Tags, TextPattern, Then, TokenCounts,
};

type Row = BTreeMap<String, String>;
type Outcome<T> = Result<T, String>;

#[test]
fn golden_cases_match_their_expected_answers() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden");
    let mut cases: Vec<PathBuf> = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir())
        .collect();
    cases.sort();
    assert!(!cases.is_empty(), "no golden cases in {}", root.display());
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("golden");
    let mut failures = Vec::new();
    for case in &cases {
        let name = case.file_name().unwrap().to_string_lossy().into_owned();
        match run(case, &out.join(&name)) {
            Ok(Some(difference)) => failures.push(format!("{name}: {difference}")),
            Ok(None) => {}
            Err(error) => failures.push(format!("{name}: {error}")),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

/// Runs one case and returns how its answers differ from what it expects.
fn run(case: &Path, out: &Path) -> Outcome<Option<String>> {
    let events = events(case)?;
    let currency = tokencontroller_core::Currency::USD;
    let mut text = String::from("as_of,kind,filter,from,to,argument,key,amount,note\n");
    let mut folds: BTreeMap<OffsetDateTime, State> = BTreeMap::new();
    for row in rows(case, "queries.csv")?.ok_or("queries.csv is missing")? {
        let as_of = moment(&cell(&row, "as_of")?)?;
        if let std::collections::btree_map::Entry::Vacant(slot) = folds.entry(as_of) {
            slot.insert(State::fold(&events, as_of, currency).map_err(|error| error.to_string())?);
        }
        let state = folds.get(&as_of).ok_or("fold missing")?;
        let answers = answer(state, &row)?;
        let prefix = [
            cell(&row, "as_of")?,
            cell(&row, "kind")?,
            cell(&row, "filter")?,
            cell(&row, "from")?,
            cell(&row, "to")?,
            cell(&row, "argument")?,
        ]
        .join(",");
        for (key, amount, note) in answers {
            let _ignored = writeln!(text, "{prefix},{key},{amount},{note}");
        }
    }
    fs::create_dir_all(out).map_err(|error| error.to_string())?;
    fs::write(out.join("answers.csv"), &text).map_err(|error| error.to_string())?;
    let expected = fs::read_to_string(case.join("answers.csv"))
        .map_err(|error| format!("answers.csv: {error}"))?
        .replace("\r\n", "\n");
    if expected.trim_end() == text.trim_end() {
        Ok(None)
    } else {
        Ok(Some(format!(
            "answers.csv differs\n--- expected\n{expected}\n--- actual\n{text}"
        )))
    }
}

/// One query's answers as (key, amount, note) rows.
fn answer(state: &State, row: &Row) -> Outcome<Vec<(String, String, String)>> {
    let filter = Filter(tags(&cell(row, "filter")?)?);
    let from = cell(row, "from")?;
    let to = cell(row, "to")?;
    let argument = cell(row, "argument")?;
    let window = || -> Outcome<DateRange> {
        DateRange::new(date(&from)?, date(&to)?).map_err(|error| error.to_string())
    };
    let money = |amount: Money| exact(amount);
    let fail = |error: tokencontroller_core::MoneyError| error.to_string();
    let mut answers = Vec::new();
    match cell(row, "kind")?.as_str() {
        "messages" => {
            for line in state.message_costs() {
                answers.push((
                    format!("{}/{}", line.session_id, line.message_id),
                    line.cost.amount.map(money).unwrap_or_default(),
                    cost_label(line.cost.label).to_owned(),
                ));
            }
        }
        "lines" => {
            for line in state.lines() {
                let assigned_by = serde_json::to_value(line.assigned_by)
                    .map_err(|error| error.to_string())?
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                answers.push((
                    format!("{}/{}#{}", line.session_id, line.message_id, line.part),
                    money(line.amount),
                    format!(
                        "{assigned_by}; {}; {}",
                        line.label.as_str(),
                        tag_list(&line.tags)
                    ),
                ));
            }
        }
        "cost" => {
            let cost = state.cost(&filter, window()?).map_err(fail)?;
            answers.push((String::new(), money(cost.amount), cost_notes(&cost)));
        }
        "by" => {
            let by = name(&argument)?;
            for (value, amount) in state.by_tag(&filter, window()?, &by).map_err(fail)? {
                answers.push((
                    value.map(|value| value.to_string()).unwrap_or_default(),
                    money(amount),
                    String::new(),
                ));
            }
        }
        "by_label" => {
            for (label, amount) in state.by_label(&filter, window()?).map_err(fail)? {
                answers.push((label.as_str().to_owned(), money(amount), String::new()));
            }
        }
        "reconcile" => {
            let bill = state
                .bills()
                .find(|bill| bill.id.as_str() == argument)
                .ok_or_else(|| format!("no bill {argument}"))?;
            let reconciliation = state.reconcile(&bill.value).map_err(fail)?;
            answers.push(("bill".to_owned(), money(reconciliation.bill), String::new()));
            answers.push((
                "messages".to_owned(),
                money(reconciliation.messages),
                String::new(),
            ));
            let note = if reconciliation.a_message_is_wrong() {
                "a message is wrong"
            } else {
                ""
            };
            answers.push((
                "unexplained".to_owned(),
                money(reconciliation.unexplained),
                note.to_owned(),
            ));
        }
        other => return answer_more(state, row, other, &filter, window().ok()),
    }
    Ok(answers)
}

/// The answers of the tier 2 and other kinds of query.
fn answer_more(
    state: &State,
    row: &Row,
    kind: &str,
    filter: &Filter,
    window: Option<DateRange>,
) -> Outcome<Vec<(String, String, String)>> {
    let window = || window.ok_or_else(|| "the query needs from and to".to_owned());
    let from = cell(row, "from")?;
    let argument = cell(row, "argument")?;
    let money = |amount: Money| exact(amount);
    let fail = |error: tokencontroller_core::MoneyError| error.to_string();
    let mut answers = Vec::new();
    match kind {
        "plan" => match state.plan(filter, window()?).map_err(fail)? {
            Some(plan) => answers.push((String::new(), money(plan), String::new())),
            None => answers.push(("none".to_owned(), String::new(), String::new())),
        },
        "deviation" => match state.deviation(filter, window()?).map_err(fail)? {
            Some(deviation) => {
                answers.push(("plan".to_owned(), money(deviation.plan), String::new()));
                answers.push((
                    "actual".to_owned(),
                    money(deviation.actual.amount),
                    cost_notes(&deviation.actual),
                ));
                let note = if deviation.is_favourable() {
                    "favourable"
                } else {
                    "unfavourable"
                };
                answers.push(("delta".to_owned(), money(deviation.delta), note.to_owned()));
            }
            None => answers.push(("none".to_owned(), String::new(), String::new())),
        },
        "forecast" => {
            let past = list(&argument)
                .map(|range| {
                    let (from, to) = range
                        .split_once("..")
                        .ok_or_else(|| format!("{range} is not from..to"))?;
                    DateRange::new(date(from)?, date(to)?).map_err(|error| error.to_string())
                })
                .collect::<Outcome<Vec<_>>>()?;
            let forecast = state.forecast(filter, &past, window()?).map_err(fail)?;
            answers.push((String::new(), money(forecast), String::new()));
        }
        "can" => {
            let (person, capability) = argument
                .split_once(' ')
                .ok_or_else(|| format!("{argument} is not person capability"))?;
            let capability: Capability = word(capability)?;
            let allowed = state.can(&value(person)?, capability, filter);
            answers.push((allowed.to_string(), String::new(), String::new()));
        }
        "share_sums" => {
            for sum in state.share_sums(date(&from)?) {
                let note = if sum.is_over() { "over 1" } else { "" };
                answers.push((
                    sum.project.to_string(),
                    sum.basis_points.to_string(),
                    note.to_owned(),
                ));
            }
        }
        "submissions" => {
            for submission in state.submissions() {
                let status = match &submission.status {
                    tokencontroller_core::SubmissionStatus::Rejected { reason } => {
                        format!("rejected: {reason}")
                    }
                    other => other.as_str().to_owned(),
                };
                answers.push((
                    submission.id.to_string(),
                    String::new(),
                    format!("{} {}; {}", submission.person, submission.window, status),
                ));
            }
        }
        "cutoff" => {
            let cutoff = state.cutoff().map(|date| date.to_string());
            answers.push((
                cutoff.unwrap_or_else(|| "none".to_owned()),
                String::new(),
                state
                    .frozen_as_of()
                    .map(|moment| moment.format(&Rfc3339).unwrap_or_default())
                    .unwrap_or_default(),
            ));
        }
        "history" => {
            for entry in state.history().map_err(fail)? {
                answers.push((
                    format!("{}/{}", entry.name, entry.key),
                    money(entry.cost_so_far),
                    entry
                        .last_activity
                        .map(|date| date.to_string())
                        .unwrap_or_default(),
                ));
            }
        }
        other => return Err(format!("unknown query kind {other}")),
    }
    Ok(answers)
}

fn cost_label(label: tokencontroller_core::CostLabel) -> &'static str {
    match label {
        tokencontroller_core::CostLabel::Priced => "priced",
        tokencontroller_core::CostLabel::CountError => "count error",
        tokencontroller_core::CostLabel::UnknownModel => "unknown model",
    }
}

/// What a cost leaves out, as a note.
fn cost_notes(cost: &tokencontroller_core::Cost) -> String {
    let mut notes: Vec<String> = cost
        .count_errors
        .iter()
        .map(|(session, message)| format!("count error {session}/{message}"))
        .collect();
    notes.extend(
        cost.unknown_models
            .iter()
            .map(|model| format!("unknown model {model}")),
    );
    notes.join("; ")
}

/// The event log of a case, from every event file it has.
fn events(case: &Path) -> Outcome<Vec<Event>> {
    let mut events = Vec::new();
    let mut push = |row: &Row, record: Record| -> Outcome<()> {
        let mut event = Event::new(
            &cell(row, "id")?,
            moment(&cell(row, "recorded_at")?)?,
            record,
        )
        .map_err(|error| error.to_string())?;
        if let Some(older) = row.get("supersedes").and_then(|text| optional(text)) {
            event = event.superseding(EventId::new(older).map_err(|error| error.to_string())?);
        }
        events.push(event);
        Ok(())
    };
    for row in rows(case, "tag_values.csv")?.unwrap_or_default() {
        push(
            &row,
            Record::TagValue(TagValueRecord {
                name: name(&cell(&row, "name")?)?,
                key: value(&cell(&row, "key")?)?,
                title: optional(&cell(&row, "title")?).map(str::to_owned),
                description: None,
                source_id: None,
                source_status: optional(&cell(&row, "source_status")?).map(str::to_owned),
                defaults: tags(&cell(&row, "defaults")?)?,
            }),
        )?;
    }
    for row in rows(case, "attribute_values.csv")?.unwrap_or_default() {
        push(
            &row,
            Record::AttributeValue(AttributeValue {
                name: name(&cell(&row, "name")?)?,
                key: value(&cell(&row, "key")?)?,
                title: None,
                defaults: tags(&cell(&row, "defaults")?)?,
            }),
        )?;
    }
    for row in rows(case, "person_attributes.csv")?.unwrap_or_default() {
        push(
            &row,
            Record::PersonAttribute(PersonAttribute {
                person: value(&cell(&row, "person")?)?,
                name: name(&cell(&row, "name")?)?,
                key: value(&cell(&row, "key")?)?,
            }),
        )?;
    }
    for row in rows(case, "prices.csv")?.unwrap_or_default() {
        push(
            &row,
            Record::PriceRow(PriceRow {
                provider: cell(&row, "provider")?,
                model: cell(&row, "model")?,
                kind: word(&cell(&row, "kind")?)?,
                per_million: usd(&cell(&row, "per_million")?)?,
                valid_from: date(&cell(&row, "valid_from")?)?,
                source: word(&cell(&row, "source")?)?,
            }),
        )?;
    }
    for row in rows(case, "messages.csv")?.unwrap_or_default() {
        push(&row, Record::Message(Box::new(message(&row)?)))?;
    }
    for row in rows(case, "splits.csv")?.unwrap_or_default() {
        let session_id = session(&cell(&row, "session")?)?;
        let target = match optional(&cell(&row, "message")?) {
            Some(message) => SplitTarget::Message {
                session_id,
                message_id: MessageId::new(message).map_err(|error| error.to_string())?,
            },
            None => SplitTarget::Session(session_id),
        };
        push(
            &row,
            Record::Split(Split {
                target,
                parts: parts(&cell(&row, "parts")?)?,
            }),
        )?;
    }
    more_events(case, &mut push)?;
    Ok(events)
}

/// The rest of the event files: bills, plans, access, rules and the other
/// events.
fn more_events(case: &Path, push: &mut dyn FnMut(&Row, Record) -> Outcome<()>) -> Outcome<()> {
    for row in rows(case, "bills.csv")?.unwrap_or_default() {
        push(
            &row,
            Record::Bill(Bill {
                provider: cell(&row, "provider")?,
                range: range(&row)?,
                amount: usd(&cell(&row, "amount")?)?,
            }),
        )?;
    }
    for row in rows(case, "estimates.csv")?.unwrap_or_default() {
        push(
            &row,
            Record::Estimate(Estimate {
                description: cell(&row, "description")?,
                tags: tags(&cell(&row, "tags")?)?,
                distribution: Distribution::point(usd(&cell(&row, "median")?)?),
                markup_basis_points: match cell(&row, "markup_basis_points")?.as_str() {
                    "" => 0,
                    text => number(text)?,
                },
                estimator: cell(&row, "estimator")?,
                range: range(&row)?,
            }),
        )?;
    }
    for row in rows(case, "accepts.csv")?.unwrap_or_default() {
        push(
            &row,
            Record::Accept(Accept {
                estimate: EventId::new(&cell(&row, "estimate")?)
                    .map_err(|error| error.to_string())?,
            }),
        )?;
    }
    for row in rows(case, "shares.csv")?.unwrap_or_default() {
        push(
            &row,
            Record::Share(Share {
                person: value(&cell(&row, "person")?)?,
                project: value(&cell(&row, "project")?)?,
                fraction_basis_points: number(&cell(&row, "fraction_basis_points")?)?,
                from: date(&cell(&row, "from")?)?,
                to: optional(&cell(&row, "to")?).map(date).transpose()?,
            }),
        )?;
    }
    for row in rows(case, "grants.csv")?.unwrap_or_default() {
        push(
            &row,
            Record::Grant(Grant {
                person: value(&cell(&row, "person")?)?,
                capability: word(&cell(&row, "capability")?)?,
                filter: Filter(tags(&cell(&row, "filter")?)?),
            }),
        )?;
    }
    for row in rows(case, "rules.csv")?.unwrap_or_default() {
        push(&row, Record::Rule(rule(&row)?))?;
    }
    for row in rows(case, "cutoffs.csv")?.unwrap_or_default() {
        push(&row, Record::Cutoff(date(&cell(&row, "date")?)?))?;
    }
    for row in rows(case, "submits.csv")?.unwrap_or_default() {
        push(
            &row,
            Record::Submit(Submit {
                person: value(&cell(&row, "person")?)?,
                window: range(&row)?,
            }),
        )?;
    }
    for row in rows(case, "approvals.csv")?.unwrap_or_default() {
        let decision = match cell(&row, "decision")?.as_str() {
            "approved" => Decision::Approved,
            "rejected" => Decision::Rejected {
                reason: cell(&row, "reason")?,
            },
            other => return Err(format!("unknown decision {other}")),
        };
        push(
            &row,
            Record::Approval(Approval {
                submission: EventId::new(&cell(&row, "submission")?)
                    .map_err(|error| error.to_string())?,
                decision,
            }),
        )?;
    }
    for row in rows(case, "reversals.csv")?.unwrap_or_default() {
        push(
            &row,
            Record::Reversal(EventId::new(&cell(&row, "of")?).map_err(|error| error.to_string())?),
        )?;
    }
    Ok(())
}

/// A message from a row of `messages.csv`. Counts all empty means no
/// counts.
fn message(row: &Row) -> Outcome<Message> {
    let count = |kind: &str| -> Outcome<Option<u64>> {
        optional(&cell(row, kind)?).map(number).transpose()
    };
    let counts = [
        count("input")?,
        count("output")?,
        count("cache_write")?,
        count("cache_read")?,
    ];
    let tokens = if counts.iter().all(Option::is_none) {
        None
    } else {
        let at = |index: usize| counts.get(index).copied().flatten().unwrap_or(0);
        Some(TokenCounts {
            input: at(0),
            output: at(1),
            cache_write: at(2),
            cache_read: at(3),
        })
    };
    let signals = tokencontroller_core::Signals {
        branch: optional(&cell(row, "branch")?).map(str::to_owned),
        prompt_keys: cell(row, "prompt_keys")?
            .split(' ')
            .filter(|key| !key.is_empty())
            .map(str::to_owned)
            .collect(),
        ..tokencontroller_core::Signals::default()
    };
    Ok(Message {
        session_id: session(&cell(row, "session")?)?,
        message_id: MessageId::new(&cell(row, "message")?).map_err(|error| error.to_string())?,
        person: value(&cell(row, "person")?)?,
        model: ModelRef::new(&cell(row, "provider")?, &cell(row, "model")?),
        tokens,
        at: moment(&cell(row, "at")?)?,
        agent_harness: cell(row, "agent_harness")?,
        agent_name: optional(&cell(row, "agent_name")?).map(str::to_owned),
        reported_cost: optional(&cell(row, "reported_cost")?)
            .map(usd)
            .transpose()?,
        tags: tags(&cell(row, "tags")?)?,
        signals,
        session: tokencontroller_core::SessionFacts::default(),
    })
}

/// A rule from `name,scope,order,signal,pattern,then`. `scope` is
/// `organization` or a person. `signal` is a word, or `config NAME` or
/// `resource NAME`. `then` is `tag name=key` or `key_of name`.
fn rule(row: &Row) -> Outcome<Rule> {
    let scope = match cell(row, "scope")?.as_str() {
        "organization" => Scope::Organization,
        person => Scope::Person(value(person)?),
    };
    let signal_text = cell(row, "signal")?;
    let signal = match signal_text.split_once(' ') {
        Some(("config", name)) => Signal::Config(name.to_owned()),
        Some(("resource", name)) => Signal::Resource(name.to_owned()),
        _ => word(&signal_text)?,
    };
    let then_text = cell(row, "then")?;
    let then = match then_text.split_once(' ') {
        Some(("tag", pair)) => {
            let (tag_name, key) = pair
                .split_once('=')
                .ok_or_else(|| format!("{pair} is not name=key"))?;
            Then::Tag {
                name: name(tag_name)?,
                key: value(key)?,
            }
        }
        Some(("key_of", tag_name)) => Then::KeyOf {
            name: name(tag_name)?,
        },
        _ => return Err(format!("{then_text} is not tag name=key or key_of name")),
    };
    Ok(Rule {
        name: cell(row, "name")?,
        scope,
        order: number(&cell(row, "order")?)?,
        signal,
        pattern: TextPattern::new(&cell(row, "pattern")?).map_err(|error| error.to_string())?,
        then,
    })
}

/// Split parts written as `60 feature=KD-1|40 feature=KD-2`.
fn parts(text: &str) -> Outcome<Vec<SplitPart>> {
    text.split('|')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| {
            let (weight, part_tags) = part.split_once(' ').unwrap_or((part, ""));
            Ok(SplitPart {
                weight: number(weight)?,
                tags: tags(part_tags)?,
            })
        })
        .collect()
}

/// The rows of a CSV file in the case, or `None` if it has no such file.
fn rows(case: &Path, file: &str) -> Outcome<Option<Vec<Row>>> {
    let path = case.join(file);
    if !path.exists() {
        return Ok(None);
    }
    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_path(&path)
        .map_err(|error| format!("{file}: {error}"))?;
    reader
        .deserialize()
        .map(|row| row.map_err(|error| format!("{file}: {error}")))
        .collect::<Outcome<Vec<Row>>>()
        .map(Some)
}

fn cell(row: &Row, column: &str) -> Outcome<String> {
    row.get(column)
        .cloned()
        .ok_or_else(|| format!("no column {column} in {row:?}"))
}

fn optional(text: &str) -> Option<&str> {
    Some(text).filter(|text| !text.is_empty())
}

/// The items of a `;`-separated list.
fn list(text: &str) -> impl Iterator<Item = &str> {
    text.split(';')
        .map(str::trim)
        .filter(|item| !item.is_empty())
}

fn name(text: &str) -> Outcome<TagName> {
    TagName::new(text).map_err(|error| error.to_string())
}

fn value(text: &str) -> Outcome<TagValue> {
    TagValue::new(text).map_err(|error| error.to_string())
}

fn session(text: &str) -> Outcome<SessionId> {
    SessionId::new(text).map_err(|error| error.to_string())
}

/// Tags written as `name=value` pairs separated by `;`.
fn tags(text: &str) -> Outcome<Tags> {
    Tags::parse(list(text)).map_err(|error| error.to_string())
}

/// Tags written the way `tags` reads them, in tag name order.
fn tag_list(tags: &Tags) -> String {
    tags.iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join(";")
}

fn usd(text: &str) -> Outcome<Money> {
    Money::parse(text, tokencontroller_core::Currency::USD).map_err(|error| error.to_string())
}

fn moment(text: &str) -> Outcome<OffsetDateTime> {
    OffsetDateTime::parse(text, &Rfc3339).map_err(|error| format!("{text}: {error}"))
}

fn date(text: &str) -> Outcome<Date> {
    let format = time::macros::format_description!("[year]-[month]-[day]");
    Date::parse(text, &format).map_err(|error| format!("{text}: {error}"))
}

/// The `from` and `to` columns as a range.
fn range(row: &Row) -> Outcome<DateRange> {
    DateRange::new(date(&cell(row, "from")?)?, date(&cell(row, "to")?)?)
        .map_err(|error| error.to_string())
}

fn number<T: std::str::FromStr>(text: &str) -> Outcome<T>
where
    T::Err: std::fmt::Display,
{
    text.parse().map_err(|error| format!("{text}: {error}"))
}

/// An enum value written the way its JSON spells it, such as `input`.
fn word<T: DeserializeOwned>(text: &str) -> Outcome<T> {
    serde_json::from_value(serde_json::Value::String(text.to_owned()))
        .map_err(|error| format!("{text}: {error}"))
}

/// An amount with every digit it has, at least to the cent, such as
/// `52.98` or `0.0015`.
fn exact(money: Money) -> String {
    let nanos = money.nanos();
    let sign = if nanos.is_negative() { "-" } else { "" };
    let magnitude = nanos.unsigned_abs();
    let units = magnitude.checked_div(1_000_000_000).unwrap_or_default();
    let fraction = format!(
        "{:09}",
        magnitude.checked_rem(1_000_000_000).unwrap_or_default()
    );
    let digits = fraction.trim_end_matches('0');
    let digits = if digits.len() < 2 {
        fraction.get(..2).unwrap_or_default()
    } else {
        digits
    };
    format!("{sign}{units}.{digits}")
}

#[cfg(test)]
mod helper_tests {
    use super::*;

    #[test]
    fn amounts_are_written_with_every_digit_they_have() {
        assert_eq!(exact(usd("52.98").unwrap()), "52.98");
        assert_eq!(exact(usd("0.0015").unwrap()), "0.0015");
        assert_eq!(exact(usd("-567.5").unwrap()), "-567.50");
        assert_eq!(exact(usd("0").unwrap()), "0.00");
        assert_eq!(exact(usd("1000").unwrap()), "1000.00");
    }

    #[test]
    fn split_parts_read_weights_and_tags() {
        let two = parts("60 feature=KD-1|40 feature=KD-2").unwrap();
        assert_eq!(two.len(), 2);
        assert_eq!(two.first().unwrap().weight, 60);
        assert_eq!(
            two.get(1)
                .unwrap()
                .tags
                .get_str("feature")
                .unwrap()
                .as_str(),
            "KD-2"
        );
        let whole = parts("1").unwrap();
        assert!(whole.first().unwrap().tags.is_empty());
        assert!(parts("x feature=KD-1").is_err());
    }

    #[test]
    fn rules_read_their_scope_signal_and_action() {
        let row: Row = [
            ("name", "feature key in branch"),
            ("scope", "organization"),
            ("order", "10"),
            ("signal", "branch"),
            ("pattern", r"KD-\d+"),
            ("then", "key_of feature"),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();
        let read = rule(&row).unwrap();
        assert_eq!(read.scope, Scope::Organization);
        assert_eq!(read.signal, Signal::Branch);
        assert_eq!(
            read.then,
            Then::KeyOf {
                name: TagName::feature()
            }
        );
        let mut personal = row.clone();
        personal.insert("scope".to_owned(), "tom".to_owned());
        personal.insert("signal".to_owned(), "config project".to_owned());
        personal.insert("then".to_owned(), "tag project=shop".to_owned());
        let read = rule(&personal).unwrap();
        assert_eq!(read.scope, Scope::Person(TagValue::new("tom").unwrap()));
        assert_eq!(read.signal, Signal::Config("project".to_owned()));
        assert!(matches!(read.then, Then::Tag { .. }));
        let mut broken = row;
        broken.insert("then".to_owned(), "nothing".to_owned());
        assert!(rule(&broken).is_err());
    }

    #[test]
    fn a_message_without_any_count_has_no_counts() {
        let row: Row = [
            ("session", "S1"),
            ("message", "1"),
            ("person", "mara"),
            ("provider", "anthropic"),
            ("model", "claude-sonnet-4-5"),
            ("at", "2026-03-02T10:00:00Z"),
            ("input", ""),
            ("output", ""),
            ("cache_write", ""),
            ("cache_read", ""),
            ("agent_harness", "claude-code"),
            ("agent_name", ""),
            ("reported_cost", ""),
            ("tags", ""),
            ("branch", ""),
            ("prompt_keys", "KD-1 KD-2"),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();
        let none = message(&row).unwrap();
        assert_eq!(none.tokens, None);
        assert_eq!(none.signals.prompt_keys, vec!["KD-1", "KD-2"]);
        let mut some = row;
        some.insert("output".to_owned(), "5".to_owned());
        let counts = message(&some).unwrap().tokens.unwrap();
        assert_eq!(counts.input, 0);
        assert_eq!(counts.output, 5);
    }
}
