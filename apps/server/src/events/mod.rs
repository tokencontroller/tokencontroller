//! The event log (ADR 0022): the one door to an organization's state. Every
//! write is an event appended here; every figure is the core crate's fold
//! of the log as of a moment. Nothing is updated or deleted.
//!
//! [`Log::load`] reads an organization's events, with the managed price
//! table rows the instance brings (ADR 0024), and [`Log::fold`] folds them.
//! [`append`] writes one event inside a transaction. [`access`] answers who
//! may do what from the grants in the fold. The HTTP side is `GET /events`,
//! `POST /reversals` and `GET /exports/raw`.
//!
//! Owner: the service platform work. Keep this area's code, routes and
//! queries in this folder, and its migrations in `migrations/` named
//! `<timestamp>_events_<what>.sql`.

pub mod access;
mod http;
pub mod json;

use std::collections::BTreeSet;

use sha2::{Digest as _, Sha256};
use sqlx::PgConnection;
use time::OffsetDateTime;
use tokencontroller_core::state::State;
use tokencontroller_core::{
    Currency, Event, EventId, Money, PriceRow, PriceSource, Record, TagValue,
};
use utoipa_axum::router::OpenApiRouter;
use uuid::Uuid;

use crate::AppState;
use crate::error::ApiError;

pub use http::{RawExportQuery, ReversalRequest, Reversed};

/// This area's routes, relative to `/api/v1`.
#[must_use]
pub fn routes() -> OpenApiRouter<AppState> {
    http::routes()
}

/// The prefix of a managed price table row's event id.
pub const MANAGED_ROW_PREFIX: &str = "prm_";

/// An organization's log, loaded once per request.
#[derive(Debug, Clone)]
pub struct Log {
    pub organization_id: Uuid,
    /// Every event in the order recorded, the managed price table rows
    /// among them.
    pub events: Vec<Event>,
}

impl Log {
    /// Reads an organization's whole log and the managed price table rows.
    ///
    /// # Errors
    ///
    /// Fails if a query fails or a stored event cannot be read back.
    pub async fn load(state: &AppState, organization_id: Uuid) -> Result<Self, ApiError> {
        let mut events = read_all(&state.db, organization_id).await?;
        let managed = managed_rows(state).await?;
        events.extend(managed);
        events.sort_by(|a, b| {
            a.recorded_at
                .cmp(&b.recorded_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(Self {
            organization_id,
            events,
        })
    }

    /// The log without the managed price table rows, as the raw export
    /// writes it: the organization's own events alone.
    pub fn own(&self) -> impl Iterator<Item = &Event> {
        self.events
            .iter()
            .filter(|event| !event.id.as_str().starts_with(MANAGED_ROW_PREFIX))
    }

    /// Folds the log as of `as_of`, in US dollars: the price table's
    /// currency. Reports convert at the end.
    ///
    /// # Errors
    ///
    /// [`ApiError::Internal`] when the log cannot be folded, which the
    /// checks on the way in prevent.
    pub fn fold(&self, as_of: OffsetDateTime) -> Result<State, ApiError> {
        State::fold(&self.events, as_of, Currency::USD).map_err(|error| {
            ApiError::Internal(anyhow::anyhow!("the log cannot be folded: {error}"))
        })
    }

    /// The event with this id, if it is in the log.
    #[must_use]
    pub fn find(&self, id: &str) -> Option<&Event> {
        self.events.iter().find(|event| event.id.as_str() == id)
    }

    /// The ids of the events a later event recorded up to `as_of`
    /// supersedes.
    #[must_use]
    pub fn superseded(&self, as_of: OffsetDateTime) -> BTreeSet<&EventId> {
        self.events
            .iter()
            .filter(|event| event.recorded_at <= as_of)
            .filter_map(|event| event.supersedes.as_ref())
            .collect()
    }

    /// The ids every reversal in the log names.
    #[must_use]
    pub fn reversed(&self) -> BTreeSet<&EventId> {
        self.events
            .iter()
            .filter_map(|event| match &event.record {
                Record::Reversal(id) => Some(id),
                _ => None,
            })
            .collect()
    }

    /// Whether an event is in the log and not reversed.
    #[must_use]
    pub fn is_live(&self, id: &EventId) -> bool {
        self.find(id.as_str()).is_some() && !self.reversed().contains(id)
    }

    /// The events whose id starts with `prefix` and that are not reversed,
    /// such as the grants a role gave a person.
    pub fn live_with_prefix<'a>(&'a self, prefix: &'a str) -> impl Iterator<Item = &'a Event> {
        let reversed = self.reversed();
        self.events
            .iter()
            .filter(move |event| event.id.as_str().starts_with(prefix))
            .filter(move |event| !reversed.contains(&event.id))
    }
}

impl Log {
    /// Checks that the log still folds with `new` appended: a split without
    /// weights, an estimate whose bands do not add up or an event that
    /// supersedes a later one is refused here, before anything is written.
    ///
    /// # Errors
    ///
    /// [`ApiError::BadRequest`] naming what is wrong.
    pub fn check_with(&self, new: &[Event]) -> Result<(), ApiError> {
        let mut events = self.events.clone();
        events.extend(new.iter().cloned());
        State::fold(
            &events,
            OffsetDateTime::now_utc().saturating_add(time::Duration::days(1)),
            Currency::USD,
        )
        .map(|_| ())
        .map_err(|error| ApiError::BadRequest(error.to_string()))
    }

    /// The moment a new event that supersedes `older` is recorded: `at`,
    /// or just after the older event when `at` is not later than it, since
    /// a superseding event is recorded after the one it replaces.
    #[must_use]
    pub fn after(older: &Event, at: OffsetDateTime) -> OffsetDateTime {
        if at > older.recorded_at {
            at
        } else {
            older
                .recorded_at
                .saturating_add(time::Duration::microseconds(1))
        }
    }
}

/// Checks `new` against the log and appends it in one transaction.
///
/// # Errors
///
/// [`ApiError::BadRequest`] when the log would not fold any more, or
/// whatever [`append_all`] fails with.
pub async fn write(state: &AppState, log: &Log, new: &[Event]) -> Result<(), ApiError> {
    log.check_with(new)?;
    append_all(&state.db, log.organization_id, new).await
}

async fn read_all(db: &sqlx::PgPool, organization_id: Uuid) -> Result<Vec<Event>, ApiError> {
    let rows = sqlx::query!(
        "SELECT id, recorded_at, recorded_by, supersedes, payload
         FROM events WHERE organization_id = $1 ORDER BY recorded_at, id",
        organization_id
    )
    .fetch_all(db)
    .await?;
    rows.into_iter()
        .map(|row| {
            let broken = |what: &str| {
                ApiError::Internal(anyhow::anyhow!("event {} cannot be read: {what}", row.id))
            };
            let record: Record =
                serde_json::from_value(row.payload).map_err(|error| broken(&error.to_string()))?;
            Ok(Event {
                id: EventId::new(&row.id).map_err(|error| broken(&error.to_string()))?,
                recorded_at: row.recorded_at,
                supersedes: row
                    .supersedes
                    .as_deref()
                    .map(EventId::new)
                    .transpose()
                    .map_err(|error| broken(&error.to_string()))?,
                recorded_by: row
                    .recorded_by
                    .as_deref()
                    .map(TagValue::new)
                    .transpose()
                    .map_err(|error| broken(&error.to_string()))?,
                record,
            })
        })
        .collect()
}

/// The managed price table rows as events: the rows of the file
/// `TC_PRICE_TABLE` names, loaded into `managed_price_rows`, and the rows
/// the build's hooks bring. Their ids are the same on every load.
async fn managed_rows(state: &AppState) -> Result<Vec<Event>, ApiError> {
    let rows = sqlx::query!(
        "SELECT provider, model, kind, per_million_nanos, valid_from, loaded_at FROM managed_price_rows"
    )
    .fetch_all(&state.db)
    .await?;
    let mut events = Vec::with_capacity(rows.len());
    for row in rows {
        let kind: tokencontroller_core::TokenKind =
            serde_json::from_value(serde_json::Value::String(row.kind.clone())).map_err(|_| {
                ApiError::Internal(anyhow::anyhow!("{} is not a token kind", row.kind))
            })?;
        let price = PriceRow {
            provider: row.provider,
            model: row.model,
            kind,
            per_million: Money::from_nanos(row.per_million_nanos, Currency::USD),
            valid_from: row.valid_from,
            source: PriceSource::Managed,
        };
        events.push(managed_row_event(&price, row.loaded_at)?);
    }
    for mut price in state.hooks.managed_price_rows(state).await? {
        price.source = PriceSource::Managed;
        let recorded_at = price.valid_from.midnight().assume_utc();
        events.push(managed_row_event(&price, recorded_at)?);
    }
    Ok(events)
}

/// The event of one managed row: its id is a digest of the row's key.
///
/// # Errors
///
/// Never in practice; the id is short.
pub fn managed_row_event(price: &PriceRow, recorded_at: OffsetDateTime) -> Result<Event, ApiError> {
    let digest = Sha256::digest(
        format!(
            "{}\n{}\n{:?}\n{}",
            price.provider, price.model, price.kind, price.valid_from
        )
        .as_bytes(),
    );
    let id = format!(
        "{MANAGED_ROW_PREFIX}{}",
        hex::encode(digest.iter().take(16).copied().collect::<Vec<u8>>())
    );
    Event::new(&id, recorded_at, Record::PriceRow(price.clone()))
        .map_err(|error| ApiError::Internal(error.into()))
}

/// A new event id: a short prefix naming the kind, such as `spl_`, and a
/// random part. Doors that bring their own ids keep them.
#[must_use]
pub fn new_id(prefix: &str) -> String {
    format!("{prefix}{}", Uuid::new_v4().simple())
}

/// The id prefixes of each kind of event, as the ids people see start.
pub mod prefix {
    pub const MESSAGE: &str = "msg_";
    pub const BILL: &str = "bil_";
    pub const ESTIMATE: &str = "est_";
    pub const TAG_VALUE: &str = "tag_";
    pub const SHARE: &str = "shr_";
    pub const PRICE_ROW: &str = "prc_";
    pub const ATTRIBUTE_VALUE: &str = "att_";
    pub const PERSON_ATTRIBUTE: &str = "pat_";
    pub const SPLIT: &str = "spl_";
    pub const ACCEPT: &str = "acc_";
    pub const GRANT: &str = "grt_";
    pub const CUTOFF: &str = "cut_";
    pub const SUBMIT: &str = "sub_";
    pub const APPROVAL: &str = "apr_";
    pub const RULE: &str = "rul_";
    pub const REVERSAL: &str = "rev_";
    pub const SAVED_REPORT: &str = "rpt_";
}

/// The prefix for a record.
#[must_use]
pub const fn prefix_of(record: &Record) -> &'static str {
    match record {
        Record::Message(_) => prefix::MESSAGE,
        Record::Bill(_) => prefix::BILL,
        Record::Estimate(_) => prefix::ESTIMATE,
        Record::TagValue(_) => prefix::TAG_VALUE,
        Record::Share(_) => prefix::SHARE,
        Record::PriceRow(_) => prefix::PRICE_ROW,
        Record::AttributeValue(_) => prefix::ATTRIBUTE_VALUE,
        Record::PersonAttribute(_) => prefix::PERSON_ATTRIBUTE,
        Record::Split(_) => prefix::SPLIT,
        Record::Accept(_) => prefix::ACCEPT,
        Record::Grant(_) => prefix::GRANT,
        Record::Cutoff(_) => prefix::CUTOFF,
        Record::Submit(_) => prefix::SUBMIT,
        Record::Approval(_) => prefix::APPROVAL,
        Record::Rule(_) => prefix::RULE,
        Record::Reversal(_) => prefix::REVERSAL,
    }
}

/// A new event for a record, with a fresh id, recorded now.
///
/// # Errors
///
/// Never in practice; the id fits.
pub fn event(record: Record) -> Result<Event, ApiError> {
    let id = new_id(prefix_of(&record));
    Event::new(&id, OffsetDateTime::now_utc(), record)
        .map_err(|error| ApiError::Internal(error.into()))
}

/// A person as the log names them: their email address.
///
/// # Errors
///
/// [`ApiError::Internal`] for an address the core refuses, which the
/// checks on sign-up prevent.
pub fn person(email: &str) -> Result<TagValue, ApiError> {
    TagValue::new(email).map_err(|error| {
        ApiError::Internal(anyhow::anyhow!("{email:?} cannot name a person: {error}"))
    })
}

/// Appends one event to an organization's log.
///
/// # Errors
///
/// [`ApiError::Conflict`] when an event with the same id is already in the
/// log, or a database error.
pub async fn append(
    conn: &mut PgConnection,
    organization_id: Uuid,
    event: &Event,
) -> Result<(), ApiError> {
    let payload = serde_json::to_value(&event.record)
        .map_err(|error| ApiError::Internal(anyhow::anyhow!("event cannot be written: {error}")))?;
    let inserted = sqlx::query!(
        "INSERT INTO events (organization_id, id, recorded_at, recorded_by, supersedes, kind, payload)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         ON CONFLICT DO NOTHING",
        organization_id,
        event.id.as_str(),
        event.recorded_at,
        event.recorded_by.as_ref().map(TagValue::as_str),
        event.supersedes.as_ref().map(EventId::as_str),
        event.record.kind(),
        payload
    )
    .execute(&mut *conn)
    .await?;
    if inserted.rows_affected() == 0 {
        return Err(ApiError::Conflict(format!(
            "event {} is already in the log",
            event.id
        )));
    }
    Ok(())
}

/// Appends several events in one transaction.
///
/// # Errors
///
/// As [`append`]; nothing is written when one fails.
pub async fn append_all(
    db: &sqlx::PgPool,
    organization_id: Uuid,
    events: &[Event],
) -> Result<(), ApiError> {
    let mut tx = db.begin().await?;
    for event in events {
        append(&mut tx, organization_id, event).await?;
    }
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::{date, datetime};
    use tokencontroller_core::TokenKind;

    fn row() -> PriceRow {
        PriceRow {
            provider: "anthropic".to_owned(),
            model: "claude-sonnet-5".to_owned(),
            kind: TokenKind::Input,
            per_million: Money::from_nanos(10_000_000_000, Currency::USD),
            valid_from: date!(2026 - 01 - 01),
            source: PriceSource::Managed,
        }
    }

    #[test]
    fn a_managed_row_keeps_its_id_from_one_load_to_the_next() {
        let at = datetime!(2026-10-01 00:00 UTC);
        let first = managed_row_event(&row(), at).unwrap();
        let second = managed_row_event(&row(), at).unwrap();
        assert_eq!(first.id, second.id);
        assert!(first.id.as_str().starts_with(MANAGED_ROW_PREFIX));
        let mut other = row();
        other.kind = TokenKind::Output;
        assert_ne!(managed_row_event(&other, at).unwrap().id, first.id);
    }

    #[test]
    fn new_ids_carry_their_kinds_prefix() {
        let event = event(Record::Cutoff(date!(2026 - 03 - 31))).unwrap();
        assert!(event.id.as_str().starts_with("cut_"));
        assert_eq!(prefix_of(&Record::Reversal(event.id.clone())), "rev_");
        assert_ne!(new_id("x_"), new_id("x_"));
    }

    #[test]
    fn a_log_knows_what_is_reversed() {
        let split = event(Record::Cutoff(date!(2026 - 03 - 31))).unwrap();
        let reversal = event(Record::Reversal(split.id.clone())).unwrap();
        let log = Log {
            organization_id: Uuid::nil(),
            events: vec![split.clone(), reversal.clone()],
        };
        assert!(!log.is_live(&split.id));
        assert!(log.is_live(&reversal.id));
        assert!(log.find(split.id.as_str()).is_some());
        assert_eq!(log.live_with_prefix("cut_").count(), 0);
        assert_eq!(log.live_with_prefix("rev_").count(), 1);
        assert_eq!(log.own().count(), 2);
    }

    #[test]
    fn a_log_knows_what_is_superseded_as_of_a_moment() {
        let first = event(Record::Cutoff(date!(2026 - 03 - 31))).unwrap();
        let mut second = event(Record::Cutoff(date!(2026 - 04 - 30))).unwrap();
        second.supersedes = Some(first.id.clone());
        let log = Log {
            organization_id: Uuid::nil(),
            events: vec![first.clone(), second.clone()],
        };
        let now = OffsetDateTime::now_utc();
        assert!(log.superseded(now).contains(&first.id));
        assert!(!log.superseded(now).contains(&second.id));
        let before = second.recorded_at - time::Duration::seconds(1);
        assert!(log.superseded(before).is_empty());
    }

    #[test]
    fn people_are_named_by_email() {
        assert_eq!(
            person("mara@kestrel.example").unwrap().as_str(),
            "mara@kestrel.example"
        );
        assert!(person("").is_err());
    }
}
