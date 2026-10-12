//! Submitting a token sheet (docs: Attribute > Review and submit; Account >
//! Cadence and token sheets). The CLI and the Claude Code mod send the
//! token entries reviewed on the machine; the person's drafts in the
//! service go with them. A submit says the person has reported everything
//! up to now, so it covers every cadence period since their last token
//! sheet, and an empty one says there was nothing to report. Token entries
//! of a closed month are posted to it as late adjustments.

use std::collections::BTreeSet;

use axum::Json;
use axum::extract::{DefaultBodyLimit, State};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::{Date, OffsetDateTime};
use tokencontroller_core::{Currency, Money};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::cadence::{Cadence, CadenceKind, CadencePeriod};
use super::cadences::cadence_for;
use super::current::{Dollars, basis_points};
use super::late::late_sheets;
use crate::AppState;
use crate::bills::amount::format_amount;
use crate::close::period::{Period, PeriodDates};
use crate::error::ApiError;
use crate::ingest::entry::NewTokenEntry;
use crate::ingest::http::{MAX_ENTRIES_PER_CALL, MAX_REQUEST_BYTES};
use crate::ingest::store::{self, Door};
use crate::integrations::prices;
use crate::organizations::Role;
use crate::sign_in::{Caller, CredentialKind};

/// At most this many cadence periods are covered by one submit, the most
/// recent ones.
const MAX_PERIODS: usize = 400;

pub(super) fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(submit))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES))
        .routes(routes!(remind))
}

/// What a submit sends.
#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct SubmitInput {
    /// The token entries reviewed on the machine, each with its `local_id`.
    /// Sending one again replaces it. Empty means nothing to report.
    #[serde(default)]
    pub token_entries: Vec<NewTokenEntry>,
}

/// One token entry sent, as stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct SubmittedEntry {
    pub local_id: Option<String>,
    pub id: Uuid,
    /// `submitted`, or `locked` when its month had closed.
    pub status: String,
}

/// What a submit did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct Submitted {
    /// One per token entry sent, in the order sent.
    pub token_entries: Vec<SubmittedEntry>,
    /// Token entries on the token sheet from this submit, the drafts in the
    /// service included.
    pub submitted: usize,
    /// Their list value in US dollars, the currency providers price in,
    /// known models only. A decimal string.
    pub list_value: String,
    pub currency: String,
    /// The share of that list value on tickets, in whole percent; `null`
    /// when nothing has a known value.
    pub share_on_tickets_percent: Option<u8>,
    /// Token entries of this submit whose month had closed, posted to it as
    /// late adjustments.
    pub late: usize,
    /// The cadence periods the submit covered, oldest first.
    pub token_sheets: Vec<CadencePeriod>,
}

/// The cadence periods a submit covers: every period from the day after
/// the person's last token sheet (or the running period, for a first one)
/// up to the running period, and the period of every token entry
/// submitted. At most [`MAX_PERIODS`], the most recent ones.
fn covered_periods(
    cadence: Cadence,
    last_covered: Option<Date>,
    days: &[Date],
    today: Date,
) -> Vec<CadencePeriod> {
    let mut periods: BTreeSet<CadencePeriod> = days
        .iter()
        .filter_map(|day| cadence.period_containing(*day).ok())
        .collect();
    let Ok(running) = cadence.period_containing(today) else {
        return periods.into_iter().collect();
    };
    let from = last_covered
        .and_then(Date::next_day)
        .unwrap_or(running.starts_on)
        .min(running.starts_on);
    if let Ok(ended) = cadence.ended_between(from, running.starts_on) {
        periods.extend(ended);
    }
    periods.insert(running);
    let skip = periods.len().saturating_sub(MAX_PERIODS);
    periods.into_iter().skip(skip).collect()
}

/// The list value of token entries in US dollars, known models only, and
/// how much of it is on tickets.
async fn value_on_tickets(
    db: &PgPool,
    ids: &[Uuid],
    today: Date,
) -> Result<(Money, Option<u8>), sqlx::Error> {
    let splits = sqlx::query!(
        r#"SELECT s.list_value_nanos, e.currency,
                  EXISTS (SELECT 1 FROM split_tags t WHERE t.split_id = s.id AND t.name = 'ticket') AS "on_ticket!"
           FROM splits s JOIN token_entries e ON e.id = s.token_entry_id
           WHERE e.id = ANY($1)"#,
        ids,
    )
    .fetch_all(db)
    .await?;
    let values: Vec<(Money, bool)> = splits
        .into_iter()
        .filter_map(|split| {
            let currency = Currency::new(&split.currency).ok()?;
            Some((
                Money::from_nanos(split.list_value_nanos?, currency),
                split.on_ticket,
            ))
        })
        .collect();
    let dollars =
        Dollars::load(db, values.iter().map(|(value, _)| value.currency()), today).await?;
    let mut total = 0_i128;
    let mut ticketed = 0_i128;
    for (value, on_ticket) in values {
        let Some(nanos) = dollars.nanos(value) else {
            continue;
        };
        total = total.saturating_add(nanos);
        if on_ticket {
            ticketed = ticketed.saturating_add(nanos);
        }
    }
    let percent = basis_points(ticketed, total)
        .and_then(|points| points.checked_div(100))
        .and_then(|percent| u8::try_from(percent).ok());
    let value = Money::from_nanos(i64::try_from(total).unwrap_or(i64::MAX), Currency::USD);
    Ok((value, percent))
}

/// Each token entry's status, by id.
async fn statuses(db: &PgPool, ids: &[Uuid]) -> Result<Vec<(Uuid, String)>, sqlx::Error> {
    Ok(sqlx::query!(
        "SELECT id, status FROM token_entries WHERE id = ANY($1)",
        ids
    )
    .fetch_all(db)
    .await?
    .into_iter()
    .map(|row| (row.id, row.status))
    .collect())
}

/// Writes one token sheet per covered period and puts each token entry on
/// the sheet of its period.
async fn write_sheets(
    tx: &mut sqlx::PgConnection,
    caller: &Caller,
    periods: &[CadencePeriod],
    entries: &[(Uuid, Date)],
) -> Result<(), sqlx::Error> {
    let mut sheets: Vec<(CadencePeriod, Uuid)> = Vec::new();
    for period in periods {
        let id = sqlx::query_scalar!(
            r#"INSERT INTO token_sheets (organization_id, person_id, starts_on, ends_on)
               VALUES ($1, $2, $3, $4)
               ON CONFLICT (organization_id, person_id, starts_on) DO UPDATE
               SET ends_on = greatest(token_sheets.ends_on, EXCLUDED.ends_on),
                   last_submitted_at = now()
               RETURNING id"#,
            caller.organization_id,
            caller.person_id,
            period.starts_on,
            period.ends_on,
        )
        .fetch_one(&mut *tx)
        .await?;
        sheets.push((*period, id));
    }
    for (entry_id, day) in entries {
        let Some((_, sheet_id)) = sheets.iter().find(|(period, _)| period.contains(*day)) else {
            continue;
        };
        sqlx::query!(
            r#"INSERT INTO token_sheet_entries (token_entry_id, token_sheet_id) VALUES ($1, $2)
               ON CONFLICT (token_entry_id) DO UPDATE SET token_sheet_id = EXCLUDED.token_sheet_id"#,
            entry_id,
            sheet_id,
        )
        .execute(&mut *tx)
        .await?;
    }
    Ok(())
}

/// Submit your token sheet: the token entries reviewed on your machine,
/// and your drafts here. Submit whenever you like; an empty list says there
/// was nothing to report.
#[utoipa::path(
    post,
    path = "/token-sheets/submit",
    tag = "token_sheets",
    request_body(content = SubmitInput, description = "Optional: the web app sends none"),
    responses(
        (status = OK, description = "Submitted", body = Submitted),
        (status = BAD_REQUEST, description = "A token entry breaks the schema; nothing was submitted", body = crate::error::ErrorBody),
        (status = CONFLICT, description = "A token entry sent again is locked in a closed period", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn submit(
    State(state): State<AppState>,
    caller: Caller,
    body: Option<Json<SubmitInput>>,
) -> Result<Json<Submitted>, ApiError> {
    // Signed in, in the browser or with the CLI token from `tc connect`.
    caller.require_credential(&[CredentialKind::Session])?;
    if caller.person_removed {
        return Err(ApiError::Unauthorized);
    }
    let input = body.map(|Json(input)| input).unwrap_or_default();
    if input.token_entries.len() > MAX_ENTRIES_PER_CALL {
        return Err(ApiError::BadRequest(format!(
            "send at most {MAX_ENTRIES_PER_CALL} token entries per submit"
        )));
    }
    let db = &state.db;
    let today = OffsetDateTime::now_utc().date();
    let cadence = match cadence_for(db, caller.organization_id, caller.person_id, today).await? {
        Some(cadence) => cadence,
        // Without a cadence, token sheets are kept per calendar month.
        None => Cadence::new(CadenceKind::Monthly, None, today)
            .map_err(|error| ApiError::Internal(error.into()))?,
    };
    let pricing = prices::current_pricing(db).await?;

    let mut tx = db.begin().await?;
    let stored = store::store_token_entries(
        &mut tx,
        caller.organization_id,
        caller.person_id,
        input.token_entries,
        Door::TokenSheet,
        &pricing,
    )
    .await?;
    let drafts = sqlx::query_scalar!(
        r#"UPDATE token_entries SET status = 'submitted', on_token_sheet = true, updated_at = now()
           WHERE organization_id = $1 AND agent_manager_id = $2 AND status = 'draft'
           RETURNING id"#,
        caller.organization_id,
        caller.person_id,
    )
    .fetch_all(&mut *tx)
    .await?;
    let ids: Vec<Uuid> = stored
        .iter()
        .map(|entry| entry.id)
        .chain(drafts)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let entries: Vec<(Uuid, Date)> = sqlx::query!(
        "SELECT id, started_at FROM token_entries WHERE id = ANY($1)",
        &ids
    )
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .map(|row| (row.id, row.started_at.date()))
    .collect();
    let last_covered = sqlx::query_scalar!(
        "SELECT max(ends_on) FROM token_sheets WHERE organization_id = $1 AND person_id = $2",
        caller.organization_id,
        caller.person_id,
    )
    .fetch_one(&mut *tx)
    .await?;
    let days: Vec<Date> = entries.iter().map(|(_, day)| *day).collect();
    let periods = covered_periods(cadence, last_covered, &days, today);
    write_sheets(&mut tx, &caller, &periods, &entries).await?;
    tx.commit().await?;

    crate::close::late::post_late_entries(db, caller.organization_id).await?;
    let statuses = statuses(db, &ids).await?;
    let status_of = |id: Uuid| {
        statuses
            .iter()
            .find(|(entry, _)| *entry == id)
            .map_or_else(|| "submitted".to_owned(), |(_, status)| status.clone())
    };
    let (value, share) = value_on_tickets(db, &ids, today).await?;
    // Token entries of this submit that a closed month locked.
    let late = statuses
        .iter()
        .filter(|(_, status)| status == "locked")
        .count();
    Ok(Json(Submitted {
        token_entries: stored
            .iter()
            .map(|entry| SubmittedEntry {
                local_id: entry.local_id.clone(),
                id: entry.id,
                status: status_of(entry.id),
            })
            .collect(),
        submitted: ids.len(),
        list_value: format_amount(value),
        currency: Currency::USD.code(),
        share_on_tickets_percent: share,
        late,
        token_sheets: periods,
    }))
}

/// Which month to ask for.
#[derive(Debug, Deserialize, ToSchema)]
pub struct RemindInput {
    /// The month, such as `2026-09`.
    #[schema(value_type = String, example = "2026-09")]
    pub period: Period,
}

/// How many people the request reaches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct Reminded {
    /// People with a late token sheet or drafts in that month.
    pub reminded: usize,
}

/// Ask everyone to submit their token sheets for a month before the close,
/// the way finance asks for timesheets. The request shows in the web app
/// and to local tools until the month closes.
#[utoipa::path(
    post,
    path = "/token-sheets/remind",
    tag = "token_sheets",
    request_body = RemindInput,
    responses(
        (status = OK, description = "Asked", body = Reminded),
        (status = FORBIDDEN, description = "Only a controller can ask everyone", body = crate::error::ErrorBody),
        (status = CONFLICT, description = "The month is closed", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn remind(
    State(state): State<AppState>,
    caller: Caller,
    Json(input): Json<RemindInput>,
) -> Result<Json<Reminded>, ApiError> {
    caller.require_role(Role::Controller)?;
    caller.require_session()?;
    let db = &state.db;
    let first_day = input.period.first_day();
    let closed = sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM closed_periods WHERE organization_id = $1 AND period = $2) AS "closed!""#,
        caller.organization_id,
        first_day,
    )
    .fetch_one(db)
    .await?;
    if closed {
        return Err(ApiError::Conflict(format!(
            "{} is closed; a late token sheet is posted to it as an adjustment",
            input.period
        )));
    }
    sqlx::query!(
        r#"INSERT INTO submit_requests (organization_id, period, requested_by) VALUES ($1, $2, $3)
           ON CONFLICT (organization_id, period) DO UPDATE
           SET requested_by = EXCLUDED.requested_by, requested_at = now()"#,
        caller.organization_id,
        first_day,
        caller.person_id,
    )
    .execute(db)
    .await?;
    let today = OffsetDateTime::now_utc().date();
    let mut people: BTreeSet<Uuid> =
        late_sheets(db, caller.organization_id, Some(input.period), today)
            .await?
            .into_iter()
            .map(|sheet| sheet.person_id)
            .collect();
    people.extend(
        sqlx::query_scalar!(
            r#"SELECT DISTINCT agent_manager_id AS "person!" FROM token_entries
               WHERE organization_id = $1 AND status = 'draft' AND agent_manager_id IS NOT NULL
                 AND started_at >= $2 AND started_at < $3"#,
            caller.organization_id,
            input.period.starts_at(),
            input
                .period
                .ends_at()
                .map_err(|error| ApiError::BadRequest(error.to_string()))?,
        )
        .fetch_all(db)
        .await?,
    );
    Ok(Json(Reminded {
        reminded: people.len(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    fn weekly() -> Cadence {
        Cadence::new(CadenceKind::Weekly, None, date!(2026 - 09 - 07)).unwrap()
    }

    fn starts(periods: &[CadencePeriod]) -> Vec<String> {
        periods
            .iter()
            .map(|period| period.starts_on.to_string())
            .collect()
    }

    #[test]
    fn a_first_submit_covers_the_running_period_and_its_token_entries() {
        let today = date!(2026 - 10 - 14);
        let periods = covered_periods(weekly(), None, &[date!(2026 - 09 - 22)], today);
        assert_eq!(starts(&periods), ["2026-09-21", "2026-10-12"]);
        assert_eq!(
            starts(&covered_periods(weekly(), None, &[], today)),
            ["2026-10-12"]
        );
    }

    #[test]
    fn a_submit_covers_every_period_since_the_last_token_sheet() {
        let today = date!(2026 - 10 - 14);
        let periods = covered_periods(weekly(), Some(date!(2026 - 09 - 27)), &[], today);
        assert_eq!(starts(&periods), ["2026-09-28", "2026-10-05", "2026-10-12"]);
        // Submitting again in the same period covers it again.
        let again = covered_periods(weekly(), Some(date!(2026 - 10 - 18)), &[], today);
        assert_eq!(starts(&again), ["2026-10-12"]);
    }

    #[test]
    fn without_a_cadence_token_sheets_are_months() {
        let monthly = Cadence::new(CadenceKind::Monthly, None, date!(2026 - 10 - 14)).unwrap();
        let periods = covered_periods(
            monthly,
            Some(date!(2026 - 08 - 31)),
            &[],
            date!(2026 - 10 - 14),
        );
        assert_eq!(starts(&periods), ["2026-09-01", "2026-10-01"]);
    }
}
