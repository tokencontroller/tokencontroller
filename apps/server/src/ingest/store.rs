//! From a checked token entry or a telemetry batch to events in the log:
//! one message event per model call, a split event for an explicit split,
//! and the answer the sender gets back, with the cost and label the fold
//! gives the session.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use sha2::{Digest as _, Sha256};
use time::OffsetDateTime;
use tokencontroller_core::state::State as Fold;
use tokencontroller_core::{
    Activity, Event, EventId, Label, Message, MessageId, ModelRef, Record, SessionFacts, SessionId,
    Split, SplitPart, SplitTarget, TagValue, Tags,
};
use utoipa::ToSchema;
use uuid::Uuid;

use super::entry::CheckedTokenEntry;
use super::model::{TelemetryActivity, TelemetryMessage};
use super::quota;
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

/// When an entry arrived and the received record it was read from.
#[derive(Clone, Copy)]
struct Arrived<'a> {
    at: OffsetDateTime,
    received: Option<&'a str>,
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
    arrived: Arrived<'_>,
) -> Result<Vec<Event>, ApiError> {
    let Arrived { at, received } = arrived;
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
    let mut signals = checked.entry.signals.core(&checked.entry.facts);
    without_content_text(&mut signals);
    let mut events = Vec::with_capacity(checked.messages.len());
    for message in &checked.messages {
        let message_id = MessageId::new(&message.message_id)
            .map_err(|error| ApiError::BadRequest(error.to_string()))?;
        let record = Message {
            session_id: session.clone(),
            message_id,
            person: person.clone(),
            model: checked.provider.as_deref().map_or_else(
                || ModelRef::unreported(&message.model),
                |provider| ModelRef::new(provider, &message.model),
            ),
            tokens: message.tokens.clone(),
            at: message.at,
            agent_harness: checked.entry.agent_harness.clone(),
            agent_name: checked.entry.agent_name.clone(),
            reported_cost: message.reported_cost,
            tags: tags.clone(),
            signals: signals.clone(),
            session: facts.clone(),
            received: received.map(str::to_owned),
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

/// Removes the open signals whose name holds content text: the log never
/// holds content text (ADR 0028).
fn without_content_text(signals: &mut tokencontroller_core::Signals) {
    signals
        .other
        .retain(|name, _| !super::otlp::is_content_text_name(name));
    signals
        .resource
        .retain(|name, _| !super::otlp::is_content_text_name(name));
}

/// The entry's tags with the signals named like a tag added: a signal such
/// as `project` or `contract` is the sender's own assignment. A tag the
/// entry names itself wins.
async fn with_signal_tags(
    db: &sqlx::PgPool,
    organization_id: Uuid,
    checked: &CheckedTokenEntry,
) -> Result<BTreeMap<String, String>, ApiError> {
    let mut named = checked.tags.clone();
    for (name, value) in &checked.entry.signals.other {
        let Some(text) = value.as_str() else {
            continue;
        };
        if named.contains_key(name)
            || name == super::entry::PERSON_TAG
            || !super::entry::is_tag_name(name)
            || TagValue::new(text).is_err()
        {
            continue;
        }
        let is_tag = name == "contract"
            || tags::kind_of(db, organization_id, name).await? == Some(tags::Kind::Tag);
        if is_tag {
            named.insert(name.clone(), text.to_owned());
        }
    }
    Ok(named)
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
            .or_insert_with(|| tokencontroller_core::Money::zero(fold.currency()));
        *entry = entry.checked_add(line.amount).unwrap_or(*entry);
    }
    let label = by_label
        .iter()
        .max_by_key(|(_, amount)| amount.nanos())
        .map_or_else(|| Label::of(tags), |(label, _)| *label);
    StoredEntry {
        session_id: session.to_string(),
        messages,
        cost: json::money(cost),
        currency: "USD",
        label: label.as_str(),
        unknown_tags: tags::unknown_values(fold, tags),
    }
}

/// How many of `events` are messages new to the log: a message that
/// supersedes the earlier record of the same call is the same message, so
/// it does not count against the daily limit.
fn new_messages(events: &[Event]) -> usize {
    events
        .iter()
        .filter(|event| matches!(event.record, Record::Message(_)) && event.supersedes.is_none())
        .count()
}

/// The local ids among `local_ids` that were stored before, so sending an
/// entry again adds nothing.
async fn seen_local_ids(
    db: &sqlx::PgPool,
    organization_id: Uuid,
    local_ids: &[String],
) -> Result<BTreeSet<String>, ApiError> {
    let seen = sqlx::query_scalar!(
        "SELECT local_id FROM entry_ids WHERE organization_id = $1 AND local_id = ANY($2)",
        organization_id,
        local_ids
    )
    .fetch_all(db)
    .await?;
    Ok(seen.into_iter().collect())
}

/// Records the local ids of the entries a transaction stores, in that
/// transaction: an entry whose batch is refused or fails stays unknown, so
/// sending it again stores it. Two batches that carry the same new local id
/// at the same moment would count it twice, so the second is refused and
/// its sender sends again.
///
/// # Errors
///
/// [`ApiError::Conflict`] when another batch claimed one of the ids first.
async fn claim_local_ids(
    tx: &mut sqlx::PgConnection,
    organization_id: Uuid,
    local_ids: &[String],
) -> Result<(), ApiError> {
    for local_id in local_ids {
        let inserted = sqlx::query!(
            "INSERT INTO entry_ids (organization_id, local_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
            organization_id,
            local_id
        )
        .execute(&mut *tx)
        .await?;
        if inserted.rows_affected() == 0 {
            return Err(ApiError::Conflict(format!(
                "the token entry {local_id} arrived twice at once; send it again"
            )));
        }
    }
    Ok(())
}

/// Stores checked token entries as events for `person`, and answers each
/// with its cost and label as of now. An entry whose local id was seen
/// before adds nothing and is answered from the log as it stands.
///
/// # Errors
///
/// [`ApiError::BadRequest`] for an id the core refuses or a tag name the
/// organization has not, [`ApiError::TooManyRequests`] when the batch would
/// pass the organization's daily message limit, [`ApiError::Conflict`] when
/// the same new local id arrives twice at once, or a database error. Nothing
/// is stored then, and the local ids of the batch stay unknown.
pub async fn store_entries(
    state: &AppState,
    organization_id: Uuid,
    person: &TagValue,
    entries: Vec<CheckedTokenEntry>,
) -> Result<Vec<StoredEntry>, ApiError> {
    store_received_entries(state, organization_id, person, entries, None).await
}

/// [`store_entries`] for entries read from a received record: each message
/// points back to it.
///
/// # Errors
///
/// As [`store_entries`].
pub async fn store_received_entries(
    state: &AppState,
    organization_id: Uuid,
    person: &TagValue,
    entries: Vec<CheckedTokenEntry>,
    received: Option<&str>,
) -> Result<Vec<StoredEntry>, ApiError> {
    let limit = quota::limit(state, organization_id).await?;
    quota::check(state, organization_id, limit.as_ref(), 0).await?;
    let mut log = Log::load(state, organization_id).await?;
    let local_ids: Vec<String> = entries
        .iter()
        .filter_map(|entry| entry.entry.local_id.clone())
        .collect();
    // The ids stored before, and then each id as its first copy in this
    // batch is used: a second copy in the same batch adds nothing either.
    let mut seen = seen_local_ids(&state.db, organization_id, &local_ids).await?;
    let mut claimed: Vec<String> = Vec::new();
    let now = OffsetDateTime::now_utc();
    let mut new_events: Vec<Event> = Vec::new();
    let mut sessions: Vec<(SessionId, usize, Tags)> = Vec::with_capacity(entries.len());
    let mut rows: Vec<(String, String, EventId)> = Vec::new();
    for (index, checked) in entries.iter().enumerate() {
        let session = session_id(checked)?;
        let tags = json::tags(&with_signal_tags(&state.db, organization_id, checked).await?)?;
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
            let messages = message_events(
                checked,
                &session,
                person,
                &tags,
                &known,
                &log,
                Arrived { at, received },
            )?;
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
            if let Some(local_id) = &checked.entry.local_id {
                seen.insert(local_id.clone());
                claimed.push(local_id.clone());
            }
        }
        sessions.push((session, checked.messages.len(), tags));
    }
    let fold = Log {
        organization_id,
        events: log.events.clone(),
    }
    .fold(now.saturating_add(time::Duration::days(1)))?;
    quota::check(
        state,
        organization_id,
        limit.as_ref(),
        new_messages(&new_events),
    )
    .await?;
    let mut tx = state.db.begin().await?;
    claim_local_ids(&mut tx, organization_id, &claimed).await?;
    for event in &new_events {
        events::append(&mut tx, organization_id, event).await?;
    }
    remember_message_events(&mut tx, organization_id, &rows).await?;
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
    received: Option<&str>,
) -> Result<Message, ApiError> {
    let mut signals = tokencontroller_core::Signals {
        repository: message.repository.clone(),
        resource: message.attributes.clone(),
        ..Default::default()
    };
    without_content_text(&mut signals);
    Ok(Message {
        session_id: SessionId::new(&message.session_id)
            .map_err(|error| ApiError::BadRequest(error.to_string()))?,
        message_id: MessageId::new(&message.message_id)
            .map_err(|error| ApiError::BadRequest(error.to_string()))?,
        person,
        model: message.provider.as_deref().map_or_else(
            || ModelRef::unreported(&message.model),
            |provider| ModelRef::new(provider, &message.model),
        ),
        tokens: Some(message.tokens.clone()),
        at: message.occurred_at,
        agent_harness: message.harness.name().to_owned(),
        agent_name: None,
        reported_cost: message.reported_cost,
        tags,
        signals,
        session: SessionFacts::default(),
        received: received.map(str::to_owned),
    })
}

/// The activity record of one telemetry record that is no model call. Its
/// event id is built from what it holds, so an export sent again, or a
/// received record read again, adds it once.
fn telemetry_activity(
    activity: &TelemetryActivity,
    person: TagValue,
    received: Option<&str>,
) -> Result<(String, Activity), ApiError> {
    let mut signals = tokencontroller_core::Signals {
        resource: activity.attributes.clone(),
        ..Default::default()
    };
    without_content_text(&mut signals);
    let session_id = activity
        .session_id
        .as_deref()
        .map(SessionId::new)
        .transpose()
        .map_err(|error| ApiError::BadRequest(error.to_string()))?;
    let mut hasher = Sha256::new();
    for part in [
        activity.harness.name().to_owned(),
        activity.kind.clone(),
        activity.session_id.clone().unwrap_or_default(),
        activity.occurred_at.unix_timestamp_nanos().to_string(),
        person.to_string(),
        serde_json::to_string(&activity.attributes).unwrap_or_default(),
    ] {
        hasher.update(part.as_bytes());
        hasher.update([0x1f]);
    }
    let digest: Vec<u8> = hasher.finalize().iter().copied().take(16).collect();
    let id = format!("{}{}", prefix::ACTIVITY, hex::encode(digest));
    Ok((
        id,
        Activity {
            kind: activity.kind.clone(),
            session_id,
            person,
            at: activity.occurred_at,
            agent_harness: activity.harness.name().to_owned(),
            signals,
            received: received.map(str::to_owned),
        },
    ))
}

/// What a telemetry batch did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TelemetryOutcome {
    pub new_messages: usize,
    /// Messages already on record, from a token entry or an earlier batch.
    pub known_messages: usize,
    pub new_activities: usize,
}

/// What one telemetry request brings, read from its received record.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Telemetry {
    pub messages: Vec<TelemetryMessage>,
    pub activities: Vec<TelemetryActivity>,
    /// The received record it was read from.
    pub received: Option<String>,
}

/// Stores the model calls of a telemetry batch as message events. A call
/// already on record, under the same session and message id, is counted
/// once and left as it is. The person is the member the harness names by
/// email, else the organization token's creator.
///
/// # Errors
///
/// A database error, an id the core refuses, or
/// [`ApiError::TooManyRequests`] when the batch would pass the
/// organization's daily message limit.
pub async fn store_telemetry(
    state: &AppState,
    organization_id: Uuid,
    fallback: &TagValue,
    messages: Vec<TelemetryMessage>,
) -> Result<TelemetryOutcome, ApiError> {
    store_received_telemetry(
        state,
        organization_id,
        fallback,
        Telemetry {
            messages,
            ..Telemetry::default()
        },
    )
    .await
}

/// [`store_telemetry`] for the model calls and activities read from a
/// received record: each points back to it. An activity already on record
/// adds nothing.
///
/// # Errors
///
/// As [`store_telemetry`].
pub async fn store_received_telemetry(
    state: &AppState,
    organization_id: Uuid,
    fallback: &TagValue,
    telemetry: Telemetry,
) -> Result<TelemetryOutcome, ApiError> {
    let Telemetry {
        messages,
        activities,
        received,
    } = telemetry;
    let received = received.as_deref();
    let limit = quota::limit(state, organization_id).await?;
    quota::check(state, organization_id, limit.as_ref(), 0).await?;
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
    let person_of = |email: Option<&str>| match email
        .map(str::to_lowercase)
        .filter(|email| members.contains(email))
    {
        Some(email) => events::person(&email),
        None => Ok(fallback.clone()),
    };
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
        let person = person_of(message.person_email.as_deref())?;
        let tags = telemetry_tags(&state.db, organization_id, &message).await?;
        let record = telemetry_record(&message, person, tags, received)?;
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
    let mut activity_ids = BTreeSet::new();
    for activity in &activities {
        let person = person_of(activity.person_email.as_deref())?;
        let (id, record) = telemetry_activity(activity, person, received)?;
        if log.find(&id).is_some() || !activity_ids.insert(id.clone()) {
            continue;
        }
        new_events.push(
            Event::new(
                &id,
                now.saturating_add(time::Duration::microseconds(
                    i64::try_from(new_events.len()).unwrap_or(0),
                )),
                Record::Activity(Box::new(record)),
            )
            .map_err(|error| ApiError::Internal(error.into()))?,
        );
        outcome.new_activities = outcome.new_activities.saturating_add(1);
    }
    if new_events.is_empty() {
        return Ok(outcome);
    }
    log.check_with(&new_events)?;
    quota::check(
        state,
        organization_id,
        limit.as_ref(),
        new_messages(&new_events),
    )
    .await?;
    append_telemetry(&state.db, organization_id, &new_events, &rows).await?;
    Ok(outcome)
}

/// Appends telemetry's events and remembers which event holds each model
/// call, in one transaction.
async fn append_telemetry(
    db: &sqlx::PgPool,
    organization_id: Uuid,
    new_events: &[Event],
    rows: &[(String, String, EventId)],
) -> Result<(), ApiError> {
    let mut tx = db.begin().await?;
    for event in new_events {
        events::append(&mut tx, organization_id, event).await?;
    }
    remember_message_events(&mut tx, organization_id, rows).await?;
    tx.commit().await?;
    Ok(())
}

/// Remembers which event holds each model call, by session and message id.
async fn remember_message_events(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    organization_id: Uuid,
    rows: &[(String, String, EventId)],
) -> Result<(), ApiError> {
    for (session, message, event_id) in rows {
        sqlx::query!(
            r#"INSERT INTO message_events (organization_id, session_id, message_id, event_id)
               VALUES ($1, $2, $3, $4)
               ON CONFLICT (organization_id, session_id, message_id) DO UPDATE SET event_id = EXCLUDED.event_id"#,
            organization_id,
            session,
            message,
            event_id.as_str()
        )
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
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
            person: mara,
            model: ModelRef::new("anthropic", "claude-sonnet-5"),
            tokens: Some(tokencontroller_core::TokenCounts::common(
                1_000_000, 0, 0, 0,
            )),
            at: datetime!(2026-03-02 10:00 UTC),
            agent_harness: "claude-code".to_owned(),
            agent_name: None,
            reported_cost: None,
            tags: Tags::new(),
            signals: tokencontroller_core::Signals::default(),
            session: SessionFacts::default(),
            received: None,
        };
        let price = tokencontroller_core::PriceRow {
            provider: "anthropic".to_owned(),
            model: "claude-sonnet-5".to_owned(),
            kind: tokencontroller_core::TokenKind::INPUT,
            per_million: tokencontroller_core::Money::parse("3", Currency::USD).unwrap(),
            valid_from: time::macros::date!(2026 - 01 - 01),
            source: tokencontroller_core::PriceSource::Managed,
            contract: None,
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
