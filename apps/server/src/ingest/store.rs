//! From a checked token entry or a telemetry batch to events in the log:
//! one message event per model call, a split event for an explicit split,
//! and the answer the sender gets back, with the cost and label the fold
//! gives the session.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use time::OffsetDateTime;
use tokencontroller_core::state::State as Fold;
use tokencontroller_core::{
    Event, EventId, Label, Message, MessageId, ModelRef, Record, SessionFacts, SessionId, Split,
    SplitPart, SplitTarget, TagValue, Tags,
};
use utoipa::ToSchema;
use uuid::Uuid;

use super::entry::CheckedTokenEntry;
use super::model::TelemetryMessage;
use crate::AppState;
use crate::error::ApiError;
use crate::events::json;
use crate::events::{self, Log, prefix};
use crate::tags;

/// What the sender gets back for one token entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct StoredEntry {
    /// The session the messages were counted under.
    pub session_id: String,
    /// How many messages the entry stands for.
    pub messages: usize,
    /// `C(e)` summed over the entry's messages, as of now, in US dollars.
    pub cost: String,
    pub currency: &'static str,
    /// The label of the entry's largest part: `direct`, `project_overhead`
    /// or `organization_overhead`.
    pub label: &'static str,
    /// Tags the entry named that are no value on record, as `name: key`.
    /// They count as sent; add the values to see them in autocomplete.
    pub unknown_tags: Vec<String>,
}

/// The id a session record without a session id is counted under: the
/// entry's local id, or a fresh one.
fn session_id(checked: &CheckedTokenEntry) -> Result<SessionId, ApiError> {
    let text = checked
        .entry
        .session_id
        .clone()
        .or_else(|| checked.entry.local_id.clone())
        .unwrap_or_else(|| format!("entry-{}", Uuid::new_v4().simple()));
    SessionId::new(&text).map_err(|error| ApiError::BadRequest(error.to_string()))
}

/// The live message event ids by session and message id, from the
/// `message_events` table.
async fn known_messages(
    db: &sqlx::PgPool,
    organization_id: Uuid,
    session_id: &str,
) -> Result<BTreeMap<String, String>, ApiError> {
    let rows = sqlx::query!(
        "SELECT message_id, event_id FROM message_events WHERE organization_id = $1 AND session_id = $2",
        organization_id,
        session_id
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| (row.message_id, row.event_id))
        .collect())
}

/// Builds the message events of a checked entry. Each supersedes the
/// earlier record of the same session and message id, if there is one.
fn message_events(
    checked: &CheckedTokenEntry,
    session: &SessionId,
    person: &TagValue,
    tags: &Tags,
    known: &BTreeMap<String, String>,
    log: &Log,
    at: OffsetDateTime,
) -> Result<Vec<Event>, ApiError> {
    let facts = SessionFacts {
        turns: checked.entry.facts.turns,
        tool_calls: checked.entry.facts.tool_calls,
        retries: checked.entry.facts.retries,
        reruns: None,
        duration_seconds: checked.entry.facts.duration_seconds,
        subagents: checked.entry.facts.subagents,
        files_touched: checked.entry.facts.files_touched,
        result: checked.entry.facts.result.clone(),
        summary: checked.entry.summary.clone(),
    };
    let signals = checked.entry.signals.core();
    let mut events = Vec::with_capacity(checked.messages.len());
    for message in &checked.messages {
        let message_id = MessageId::new(&message.message_id)
            .map_err(|error| ApiError::BadRequest(error.to_string()))?;
        let record = Message {
            session_id: session.clone(),
            message_id,
            person: person.clone(),
            model: ModelRef::new(checked.provider, &message.model),
            tokens: message.tokens,
            at: message.at,
            agent_harness: checked.entry.agent_harness.clone(),
            agent_name: checked.entry.agent_name.clone(),
            reported_cost: message.reported_cost,
            tags: tags.clone(),
            signals: signals.clone(),
            session: facts.clone(),
        };
        let older = known
            .get(&message.message_id)
            .and_then(|id| log.find(id))
            .filter(|older| log.is_live(&older.id));
        let recorded_at = older.map_or(at, |older| Log::after(older, at));
        let mut event = Event::new(
            &events::new_id(prefix::MESSAGE),
            recorded_at,
            Record::Message(Box::new(record)),
        )
        .map_err(|error| ApiError::Internal(error.into()))?
        .by(person.clone());
        if let Some(older) = older {
            event = event.superseding(older.id.clone());
        }
        events.push(event);
    }
    Ok(events)
}

/// The split events of a checked entry's explicit splits: one on the
/// session for fractions, one per message for parts by message id.
fn split_events(
    checked: &CheckedTokenEntry,
    session: &SessionId,
    person: &TagValue,
    log: &Log,
    at: OffsetDateTime,
) -> Result<Vec<Event>, ApiError> {
    if checked.splits.is_empty() {
        return Ok(Vec::new());
    }
    let mut splits: Vec<Split> = Vec::new();
    if checked
        .splits
        .iter()
        .all(|split| split.message_ids.is_empty())
    {
        let mut parts = Vec::with_capacity(checked.splits.len());
        for split in &checked.splits {
            parts.push(SplitPart {
                weight: split.weight,
                tags: json::tags(&split.tags)?,
            });
        }
        splits.push(Split {
            target: SplitTarget::Session(session.clone()),
            parts,
        });
    } else {
        for split in &checked.splits {
            let tags = json::tags(&split.tags)?;
            for id in &split.message_ids {
                splits.push(Split::whole(
                    SplitTarget::Message {
                        session_id: session.clone(),
                        message_id: MessageId::new(id)
                            .map_err(|error| ApiError::BadRequest(error.to_string()))?,
                    },
                    tags.clone(),
                ));
            }
        }
    }
    let mut events = Vec::with_capacity(splits.len());
    for split in splits {
        events.push(crate::splits::event_for(log, split, person, at)?);
    }
    Ok(events)
}

/// The answer for a session as the fold sees it.
fn answer(fold: &Fold, session: &SessionId, messages: usize, tags: &Tags) -> StoredEntry {
    let mut cost = tokencontroller_core::Money::zero(fold.currency());
    let mut by_label: BTreeMap<Label, tokencontroller_core::Money> = BTreeMap::new();
    for line in fold
        .lines()
        .iter()
        .filter(|line| line.session_id == *session)
    {
        cost = cost.checked_add(line.amount).unwrap_or(cost);
        let entry = by_label
            .entry(line.label)
            .or_insert(tokencontroller_core::Money::zero(fold.currency()));
        *entry = entry.checked_add(line.amount).unwrap_or(*entry);
    }
    let label = by_label
        .iter()
        .max_by_key(|(_, amount)| amount.nanos())
        .map_or(Label::of(tags), |(label, _)| *label);
    StoredEntry {
        session_id: session.to_string(),
        messages,
        cost: json::money(cost),
        currency: "USD",
        label: label.as_str(),
        unknown_tags: tags::unknown_values(fold, tags),
    }
}

/// Records the local ids of entries, so sending one again adds nothing.
async fn claim_local_ids(
    db: &sqlx::PgPool,
    organization_id: Uuid,
    local_ids: &[String],
) -> Result<BTreeSet<String>, ApiError> {
    let mut seen = BTreeSet::new();
    for local_id in local_ids {
        let inserted = sqlx::query!(
            "INSERT INTO entry_ids (organization_id, local_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
            organization_id,
            local_id
        )
        .execute(db)
        .await?;
        if inserted.rows_affected() == 0 {
            seen.insert(local_id.clone());
        }
    }
    Ok(seen)
}

/// Stores checked token entries as events for `person`, and answers each
/// with its cost and label as of now. An entry whose local id was seen
/// before adds nothing and is answered from the log as it stands.
///
/// # Errors
///
/// [`ApiError::BadRequest`] for an id the core refuses or a tag name the
/// organization has not, or a database error.
pub async fn store_entries(
    state: &AppState,
    organization_id: Uuid,
    person: &TagValue,
    entries: Vec<CheckedTokenEntry>,
) -> Result<Vec<StoredEntry>, ApiError> {
    let mut log = Log::load(state, organization_id).await?;
    let local_ids: Vec<String> = entries
        .iter()
        .filter_map(|entry| entry.entry.local_id.clone())
        .collect();
    let seen = claim_local_ids(&state.db, organization_id, &local_ids).await?;
    let now = OffsetDateTime::now_utc();
    let mut new_events: Vec<Event> = Vec::new();
    let mut sessions: Vec<(SessionId, usize, Tags)> = Vec::with_capacity(entries.len());
    let mut rows: Vec<(String, String, EventId)> = Vec::new();
    for (index, checked) in entries.iter().enumerate() {
        let session = session_id(checked)?;
        let tags = json::tags(&checked.tags)?;
        tags::check_names(&state.db, organization_id, &tags).await?;
        for split in &checked.splits {
            tags::check_names(&state.db, organization_id, &json::tags(&split.tags)?).await?;
        }
        let repeated = checked
            .entry
            .local_id
            .as_ref()
            .is_some_and(|id| seen.contains(id));
        if !repeated {
            let known = known_messages(&state.db, organization_id, session.as_str()).await?;
            let at = now.saturating_add(time::Duration::microseconds(
                i64::try_from(index).unwrap_or(0),
            ));
            let messages = message_events(checked, &session, person, &tags, &known, &log, at)?;
            for event in &messages {
                if let Record::Message(message) = &event.record {
                    rows.push((
                        session.to_string(),
                        message.message_id.to_string(),
                        event.id.clone(),
                    ));
                }
            }
            let splits = split_events(
                checked,
                &session,
                person,
                &log,
                at.saturating_add(time::Duration::microseconds(1)),
            )?;
            log.events.extend(messages.iter().cloned());
            log.events.extend(splits.iter().cloned());
            new_events.extend(messages);
            new_events.extend(splits);
        }
        sessions.push((session, checked.messages.len(), tags));
    }
    let fold = Log {
        organization_id,
        events: log.events.clone(),
    }
    .fold(now.saturating_add(time::Duration::days(1)))?;
    let mut tx = state.db.begin().await?;
    for event in &new_events {
        events::append(&mut tx, organization_id, event).await?;
    }
    for (session, message, event_id) in &rows {
        sqlx::query!(
            r#"INSERT INTO message_events (organization_id, session_id, message_id, event_id)
               VALUES ($1, $2, $3, $4)
               ON CONFLICT (organization_id, session_id, message_id) DO UPDATE SET event_id = EXCLUDED.event_id"#,
            organization_id,
            session,
            message,
            event_id.as_str()
        )
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(sessions
        .iter()
        .map(|(session, messages, tags)| answer(&fold, session, *messages, tags))
        .collect())
}

/// The attributes of a telemetry message that are names on the
/// organization's lists, as tags.
async fn telemetry_tags(
    db: &sqlx::PgPool,
    organization_id: Uuid,
    message: &TelemetryMessage,
) -> Result<Tags, ApiError> {
    let mut tags = Tags::new();
    for (name, value) in &message.attributes {
        if tags::kind_of(db, organization_id, name).await?.is_some()
            && let (Ok(name), Ok(value)) = (
                tokencontroller_core::TagName::new(name),
                TagValue::new(value),
            )
        {
            tags.insert(name, value);
        }
    }
    Ok(tags)
}

/// The message record of one telemetry model call. Every attribute stays
/// on the record as a resource signal.
fn telemetry_record(
    message: &TelemetryMessage,
    person: TagValue,
    tags: Tags,
) -> Result<Message, ApiError> {
    let provider = super::provider::infer(None, message.harness.name(), &message.model);
    Ok(Message {
        session_id: SessionId::new(&message.session_id)
            .map_err(|error| ApiError::BadRequest(error.to_string()))?,
        message_id: MessageId::new(&message.message_id)
            .map_err(|error| ApiError::BadRequest(error.to_string()))?,
        person,
        model: ModelRef::new(provider, &message.model),
        tokens: Some(message.tokens),
        at: message.occurred_at,
        agent_harness: message.harness.name().to_owned(),
        agent_name: None,
        reported_cost: message.reported_cost,
        tags,
        signals: tokencontroller_core::Signals {
            repository: message.repository.clone(),
            resource: message.attributes.clone(),
            ..Default::default()
        },
        session: SessionFacts::default(),
    })
}

/// What a telemetry batch did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TelemetryOutcome {
    pub new_messages: usize,
    /// Messages already on record, from a token entry or an earlier batch.
    pub known_messages: usize,
}

/// Stores the model calls of a telemetry batch as message events. A call
/// already on record, under the same session and message id, is counted
/// once and left as it is. The person is the member the harness names by
/// email, else the organization token's creator.
///
/// # Errors
///
/// A database error, or an id the core refuses.
pub async fn store_telemetry(
    state: &AppState,
    organization_id: Uuid,
    fallback: &TagValue,
    messages: Vec<TelemetryMessage>,
) -> Result<TelemetryOutcome, ApiError> {
    let log = Log::load(state, organization_id).await?;
    let members: BTreeSet<String> = sqlx::query_scalar!(
        "SELECT email FROM people WHERE organization_id = $1",
        organization_id
    )
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .collect();
    let now = OffsetDateTime::now_utc();
    let mut outcome = TelemetryOutcome::default();
    let mut new_events = Vec::new();
    let mut rows = Vec::new();
    let mut known_by_session: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let empty = BTreeMap::new();
    for message in messages {
        if !known_by_session.contains_key(&message.session_id) {
            let known = known_messages(&state.db, organization_id, &message.session_id).await?;
            known_by_session.insert(message.session_id.clone(), known);
        }
        let known = known_by_session.get(&message.session_id).unwrap_or(&empty);
        if known.contains_key(&message.message_id)
            || rows
                .iter()
                .any(|(session, id, _)| *session == message.session_id && *id == message.message_id)
        {
            outcome.known_messages = outcome.known_messages.saturating_add(1);
            continue;
        }
        let person = match message
            .person_email
            .as_deref()
            .map(str::to_lowercase)
            .filter(|email| members.contains(email))
        {
            Some(email) => events::person(&email)?,
            None => fallback.clone(),
        };
        let tags = telemetry_tags(&state.db, organization_id, &message).await?;
        let record = telemetry_record(&message, person, tags)?;
        let event = Event::new(
            &events::new_id(prefix::MESSAGE),
            now.saturating_add(time::Duration::microseconds(
                i64::try_from(new_events.len()).unwrap_or(0),
            )),
            Record::Message(Box::new(record)),
        )
        .map_err(|error| ApiError::Internal(error.into()))?;
        rows.push((
            message.session_id.clone(),
            message.message_id.clone(),
            event.id.clone(),
        ));
        new_events.push(event);
        outcome.new_messages = outcome.new_messages.saturating_add(1);
    }
    if new_events.is_empty() {
        return Ok(outcome);
    }
    log.check_with(&new_events)?;
    let mut tx = state.db.begin().await?;
    for event in &new_events {
        events::append(&mut tx, organization_id, event).await?;
    }
    for (session, message, event_id) in &rows {
        sqlx::query!(
            r#"INSERT INTO message_events (organization_id, session_id, message_id, event_id)
               VALUES ($1, $2, $3, $4)
               ON CONFLICT (organization_id, session_id, message_id) DO UPDATE SET event_id = EXCLUDED.event_id"#,
            organization_id,
            session,
            message,
            event_id.as_str()
        )
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;
    use tokencontroller_core::Currency;

    #[test]
    fn the_answer_names_the_largest_parts_label() {
        let mara = TagValue::new("mara@kestrel.example").unwrap();
        let session = SessionId::new("S2").unwrap();
        let message = Message {
            session_id: session.clone(),
            message_id: MessageId::new("m1").unwrap(),
            person: mara.clone(),
            model: ModelRef::new("anthropic", "claude-sonnet-5"),
            tokens: Some(tokencontroller_core::TokenCounts {
                input: 1_000_000,
                ..Default::default()
            }),
            at: datetime!(2026-03-02 10:00 UTC),
            agent_harness: "claude-code".to_owned(),
            agent_name: None,
            reported_cost: None,
            tags: Tags::new(),
            signals: tokencontroller_core::Signals::default(),
            session: SessionFacts::default(),
        };
        let price = tokencontroller_core::PriceRow {
            provider: "anthropic".to_owned(),
            model: "claude-sonnet-5".to_owned(),
            kind: tokencontroller_core::TokenKind::Input,
            per_million: tokencontroller_core::Money::parse("3", Currency::USD).unwrap(),
            valid_from: time::macros::date!(2026 - 01 - 01),
            source: tokencontroller_core::PriceSource::Managed,
        };
        let split = Split {
            target: SplitTarget::Session(session.clone()),
            parts: vec![
                SplitPart {
                    weight: 60,
                    tags: Tags::parse(["feature=KD-1"]).unwrap(),
                },
                SplitPart {
                    weight: 40,
                    tags: Tags::new(),
                },
            ],
        };
        let at = datetime!(2026-03-02 10:00 UTC);
        let events = vec![
            Event::new("prc_1", at, Record::PriceRow(price)).unwrap(),
            Event::new("msg_1", at, Record::Message(Box::new(message))).unwrap(),
            Event::new("spl_1", at, Record::Split(split)).unwrap(),
        ];
        let fold = Fold::fold(&events, datetime!(2026-04-01 00:00 UTC), Currency::USD).unwrap();
        let answer = answer(&fold, &session, 1, &Tags::parse(["feature=KD-1"]).unwrap());
        assert_eq!(answer.cost, "3.00");
        assert_eq!(answer.label, "direct");
        assert_eq!(answer.unknown_tags, vec!["feature: KD-1"]);
    }
}
