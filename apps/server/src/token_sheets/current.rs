//! The review list (docs: Attribute > Review and submit): the person's
//! drafts, the token entries a people manager sent back with a reason, and
//! those whose ticket the ticket sync could not find. The person fixes the
//! exceptions here, by setting tags or splitting a token entry, then
//! submits.

use std::collections::{BTreeMap, BTreeSet};

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse as _, Response};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::{Date, OffsetDateTime};
use tokencontroller_core::{Currency, ExchangeRate, Money};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::cadence::CadenceKind;
use super::cadences::cadence_for;
use super::late::context_for;
use crate::AppState;
use crate::bills::amount::format_amount;
use crate::bills::rates;
use crate::close::period::{Period, PeriodDates};
use crate::error::ApiError;
use crate::ingest::entry::{self, NewSplit};
use crate::ingest::store::{self, Fix};
use crate::reports::entries::{self, EntryView};
use crate::reports::filter::Filter;
use crate::reports::lines::{self, CostLine, EntryStatus, LineQuery};
use crate::reports::scope::Scope;
use crate::sign_in::Caller;

/// The most splits a token entry can be split into by hand.
pub const MAX_SPLITS: usize = 20;

pub(super) fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(current))
        .routes(routes!(set_tags))
        .routes(routes!(set_splits))
}

/// One of the person's token sheets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct SheetSummary {
    #[serde(with = "crate::close::period::day")]
    #[schema(value_type = String, format = Date)]
    pub period_start: Date,
    #[serde(with = "crate::close::period::day")]
    #[schema(value_type = String, format = Date)]
    pub period_end: Date,
    pub token_entries: i64,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub last_submitted_at: OffsetDateTime,
}

/// A controller asked everyone to submit before the close.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct AskedToSubmit {
    /// The month, such as `2026-09`.
    pub period: String,
    pub asked_by: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub asked_at: OffsetDateTime,
}

/// The person's token sheet as it stands: what waits for them, the cadence
/// period it is for, and the token sheets they submitted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TokenSheet {
    /// The person's name, or their email until they gave one.
    pub person: String,
    /// `null` when no cadence applies.
    pub cadence: Option<CadenceKind>,
    /// The cadence period running now.
    #[serde(with = "crate::close::period::day::option")]
    #[schema(value_type = Option<String>, format = Date)]
    pub period_start: Option<Date>,
    #[serde(with = "crate::close::period::day::option")]
    #[schema(value_type = Option<String>, format = Date)]
    pub period_end: Option<Date>,
    /// When the running cadence period's token sheet is due: the end of its
    /// last day, in UTC.
    #[serde(with = "time::serde::rfc3339::option")]
    #[schema(value_type = Option<String>, format = DateTime)]
    pub due_at: Option<OffsetDateTime>,
    /// Whether an earlier cadence period still waits for its token sheet.
    pub late: bool,
    pub asked_to_submit: Option<AskedToSubmit>,
    /// Drafts, token entries sent back with a reason, and token entries
    /// with an unknown ticket, oldest first.
    pub token_entries: Vec<EntryView>,
    /// The list value of the drafts in US dollars, the currency providers
    /// price in, known models only. A decimal string.
    pub list_value: String,
    pub currency: String,
    /// The share of that list value on tickets, in basis points; `null`
    /// when nothing has a known value.
    pub share_on_tickets: Option<u64>,
    /// The person's token sheets, newest first.
    pub token_sheets: Vec<SheetSummary>,
}

/// The end of a cadence period's last day: the moment its token sheet is
/// late.
#[must_use]
pub fn due_at(period_end: Date) -> Option<OffsetDateTime> {
    period_end.next_day().map(|day| day.midnight().assume_utc())
}

/// Whether a cost line belongs in the review list.
fn waits_for_person(line: &CostLine) -> bool {
    matches!(line.status, EntryStatus::Draft | EntryStatus::Rejected)
        || line.unknown_ticket.is_some()
}

/// List values in US dollars, the currency providers price in, for the
/// review list and a submit. A value in another currency converts at
/// today's rate; without a rate it is left out, as an unknown model is.
#[derive(Debug, Clone, Default)]
pub struct Dollars {
    rates: BTreeMap<Currency, ExchangeRate>,
}

impl Dollars {
    /// Today's rate into dollars for each of `currencies`.
    ///
    /// # Errors
    ///
    /// A database error.
    pub async fn load(
        db: &PgPool,
        currencies: impl IntoIterator<Item = Currency>,
        today: Date,
    ) -> Result<Self, sqlx::Error> {
        let mut found = BTreeMap::new();
        for currency in currencies.into_iter().collect::<BTreeSet<_>>() {
            if let Ok(rate) = rates::between(db, currency, Currency::USD, today).await? {
                found.insert(currency, rate);
            }
        }
        Ok(Self { rates: found })
    }

    /// `value` in dollar nanos; `None` without a rate.
    #[must_use]
    pub fn nanos(&self, value: Money) -> Option<i128> {
        let rate = self.rates.get(&value.currency())?;
        rate.convert(value)
            .ok()
            .map(|dollars| i128::from(dollars.nanos()))
    }
}

/// The list value of the lines in US dollars and how much of it is on
/// tickets, in nanos. Lines on an unknown model, or in a currency without
/// a rate today, are left out.
fn on_tickets<'a>(
    lines: impl IntoIterator<Item = &'a CostLine>,
    dollars: &Dollars,
) -> (i128, i128) {
    let mut total = 0_i128;
    let mut ticketed = 0_i128;
    for line in lines {
        let Some(nanos) = line.list_value.and_then(|value| dollars.nanos(value)) else {
            continue;
        };
        total = total.saturating_add(nanos);
        if line.tags.contains_key("ticket") {
            ticketed = ticketed.saturating_add(nanos);
        }
    }
    (total, ticketed)
}

/// `part` of `whole` in basis points, rounded down.
#[must_use]
pub fn basis_points(part: i128, whole: i128) -> Option<u64> {
    if whole <= 0 {
        return None;
    }
    let points = part.max(0).checked_mul(10_000)?.checked_div(whole)?;
    u64::try_from(points.min(10_000)).ok()
}

/// The latest request to submit for a month that has not closed yet.
///
/// # Errors
///
/// A database error.
pub async fn asked_to_submit(
    db: &PgPool,
    organization_id: Uuid,
) -> Result<Option<AskedToSubmit>, sqlx::Error> {
    let row = sqlx::query!(
        r#"SELECT r.period, r.requested_at, p.name AS "asked_by?"
           FROM submit_requests r LEFT JOIN people p ON p.id = r.requested_by
           WHERE r.organization_id = $1 AND NOT EXISTS (
               SELECT 1 FROM closed_periods c
               WHERE c.organization_id = r.organization_id AND c.period = r.period)
           ORDER BY r.requested_at DESC LIMIT 1"#,
        organization_id,
    )
    .fetch_optional(db)
    .await?;
    Ok(row.map(|row| AskedToSubmit {
        period: Period::of_date(row.period).to_string(),
        asked_by: row.asked_by.filter(|name| !name.trim().is_empty()),
        asked_at: row.requested_at,
    }))
}

/// The person's token sheets, newest first.
async fn sheets(
    db: &PgPool,
    organization_id: Uuid,
    person_id: Uuid,
) -> Result<Vec<SheetSummary>, sqlx::Error> {
    Ok(sqlx::query!(
        r#"SELECT ts.starts_on, ts.ends_on, ts.last_submitted_at,
                  (SELECT count(*) FROM token_sheet_entries tse WHERE tse.token_sheet_id = ts.id) AS "entries!"
           FROM token_sheets ts
           WHERE ts.organization_id = $1 AND ts.person_id = $2
           ORDER BY ts.starts_on DESC
           LIMIT 52"#,
        organization_id,
        person_id,
    )
    .fetch_all(db)
    .await?
    .into_iter()
    .map(|row| SheetSummary {
        period_start: row.starts_on,
        period_end: row.ends_on,
        token_entries: row.entries,
        last_submitted_at: row.last_submitted_at,
    })
    .collect())
}

/// The caller's own cost lines that could wait for them.
async fn own_lines(db: &PgPool, caller: &Caller) -> Result<Vec<CostLine>, ApiError> {
    let query = LineQuery {
        organization_id: caller.organization_id,
        from: OffsetDateTime::UNIX_EPOCH,
        until: OffsetDateTime::now_utc()
            .checked_add(time::Duration::days(366))
            .unwrap_or(OffsetDateTime::UNIX_EPOCH),
        statuses: [
            EntryStatus::Draft,
            EntryStatus::Rejected,
            EntryStatus::Submitted,
            EntryStatus::Approved,
        ]
        .iter()
        .map(|status| status.as_str().to_owned())
        .collect(),
        filter: Filter::default(),
        with_late: true,
    };
    Ok(lines::fetch(db, &query, &Scope::of_person(caller.person_id, Vec::new())).await?)
}

/// Your token sheet: drafts, token entries sent back with a reason, and
/// token entries whose ticket the ticket sync could not find, with the
/// cadence period and whether a token sheet is late.
#[utoipa::path(
    get,
    path = "/token-sheets/current",
    tag = "token_sheets",
    params(crate::reports::csv::FormatQuery),
    responses((status = OK, description = "Your token sheet, or its token entries as CSV", content((TokenSheet = "application/json"), (String = "text/csv")))),
)]
pub(crate) async fn current(
    State(state): State<AppState>,
    caller: Caller,
    Query(query): Query<crate::reports::csv::FormatQuery>,
) -> Result<Response, ApiError> {
    let db = &state.db;
    let today = OffsetDateTime::now_utc().date();
    let person = sqlx::query!(
        "SELECT name, email FROM people WHERE id = $1",
        caller.person_id
    )
    .fetch_one(db)
    .await?;
    let cadence = cadence_for(db, caller.organization_id, caller.person_id, today).await?;
    let context = context_for(db, caller.organization_id, caller.person_id).await?;
    let waiting: Vec<CostLine> = own_lines(db, &caller)
        .await?
        .into_iter()
        .filter(waits_for_person)
        .collect();
    let drafts: Vec<&CostLine> = waiting
        .iter()
        .filter(|line| line.status == EntryStatus::Draft)
        .collect();
    let dollars = Dollars::load(
        db,
        drafts
            .iter()
            .filter_map(|line| line.list_value.map(Money::currency)),
        today,
    )
    .await?;
    let (total, ticketed) = on_tickets(drafts, &dollars);
    let total_money = Money::from_nanos(i64::try_from(total).unwrap_or(i64::MAX), Currency::USD);
    let token_entries = entries::entries(db, waiting).await?;
    if query.wants_csv() {
        return Ok(entries::to_csv(&token_entries).into_download("token-sheet.csv"));
    }
    Ok(Json(TokenSheet {
        person: if person.name.trim().is_empty() {
            person.email
        } else {
            person.name
        },
        cadence: cadence.map(super::cadence::Cadence::kind),
        period_start: context.cadence.as_ref().map(|sheet| sheet.period_start),
        period_end: context.cadence.as_ref().map(|sheet| sheet.period_end),
        due_at: context
            .cadence
            .as_ref()
            .and_then(|sheet| due_at(sheet.period_end)),
        late: context.cadence.as_ref().is_some_and(|sheet| sheet.late),
        asked_to_submit: asked_to_submit(db, caller.organization_id).await?,
        token_entries,
        list_value: format_amount(total_money),
        currency: Currency::USD.code(),
        share_on_tickets: basis_points(ticketed, total),
        token_sheets: sheets(db, caller.organization_id, caller.person_id).await?,
    })
    .into_response())
}

/// Tags to set on a token entry by hand.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct TagsInput {
    /// Every tag the token entry should name, such as `{"ticket":
    /// "KD-231"}`. The ticket brings its project and the tags they pass on.
    #[schema(value_type = BTreeMap<String, String>)]
    pub tags: BTreeMap<String, String>,
}

/// One share of a token entry split by hand.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ShareInput {
    /// Whole percent above 0; the shares add up to 100.
    pub share_percent: u32,
    #[serde(default)]
    #[schema(value_type = BTreeMap<String, String>)]
    pub tags: BTreeMap<String, String>,
}

/// A token entry's splits by hand.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct SplitsInput {
    pub splits: Vec<ShareInput>,
}

/// Checks splits by hand: two to [`MAX_SPLITS`], each above 0 percent,
/// adding up to 100, with tags the wire allows.
fn check_shares(splits: &[ShareInput]) -> Result<(), ApiError> {
    if !(2..=MAX_SPLITS).contains(&splits.len()) {
        return Err(ApiError::BadRequest(format!(
            "split a token entry into 2 to {MAX_SPLITS} shares"
        )));
    }
    if splits.iter().any(|split| split.share_percent == 0) {
        return Err(ApiError::BadRequest(
            "every share must be above 0 percent".to_owned(),
        ));
    }
    let total = splits
        .iter()
        .try_fold(0_u32, |sum, split| sum.checked_add(split.share_percent));
    if total != Some(100) {
        return Err(ApiError::BadRequest(
            "the shares must add up to 100 percent".to_owned(),
        ));
    }
    for split in splits {
        entry::check_tags(&split.tags).map_err(|error| ApiError::BadRequest(error.to_string()))?;
    }
    Ok(())
}

/// Applies a fix to one of the caller's own token entries. A rejected one
/// becomes a draft again, to go with the next submit. An approved one waits
/// for a people manager to send it back first.
async fn apply_fix(
    state: &AppState,
    caller: &Caller,
    id: Uuid,
    fix: Fix,
) -> Result<Json<EntryView>, ApiError> {
    caller.require_session()?;
    let row = sqlx::query!(
        "SELECT status, agent_manager_id FROM token_entries WHERE id = $1 AND organization_id = $2",
        id,
        caller.organization_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;
    if row.agent_manager_id != Some(caller.person_id) {
        return Err(ApiError::NotFound);
    }
    match row.status.as_str() {
        "locked" => {
            return Err(ApiError::Conflict(
                "its period is closed; a controller can post an adjustment instead".to_owned(),
            ));
        }
        "approved" => {
            return Err(ApiError::Conflict(
                "it is approved; a people manager can send it back to you first".to_owned(),
            ));
        }
        _ => {}
    }
    let mut tx = state.db.begin().await?;
    store::fix(&mut tx, caller.organization_id, id, fix).await?;
    sqlx::query!(
        "UPDATE token_entries SET status = 'draft' WHERE id = $1 AND status = 'rejected'",
        id
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    let lines: Vec<CostLine> = own_lines(&state.db, caller)
        .await?
        .into_iter()
        .filter(|line| line.entry_id == id)
        .collect();
    entries::entries(&state.db, lines)
        .await?
        .into_iter()
        .next()
        .map(Json)
        .ok_or(ApiError::NotFound)
}

/// Set the tags of one of your token entries by hand, such as its ticket.
/// It becomes one split again, placed by hand.
#[utoipa::path(
    patch,
    path = "/token-entries/{id}/tags",
    tag = "token_sheets",
    params(("id" = Uuid, Path, description = "The token entry")),
    request_body = TagsInput,
    responses(
        (status = OK, description = "The token entry, placed again", body = EntryView),
        (status = BAD_REQUEST, description = "A tag breaks the rules", body = crate::error::ErrorBody),
        (status = CONFLICT, description = "Closed or approved", body = crate::error::ErrorBody),
        (status = NOT_FOUND, description = "No such token entry of yours", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn set_tags(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<Uuid>,
    Json(input): Json<TagsInput>,
) -> Result<Json<EntryView>, ApiError> {
    entry::check_tags(&input.tags).map_err(|error| ApiError::BadRequest(error.to_string()))?;
    apply_fix(&state, &caller, id, Fix::Tags(input.tags)).await
}

/// Split one of your token entries by hand, such as one session for two
/// tickets at 60 and 40 percent.
#[utoipa::path(
    put,
    path = "/token-entries/{id}/splits",
    tag = "token_sheets",
    params(("id" = Uuid, Path, description = "The token entry")),
    request_body = SplitsInput,
    responses(
        (status = OK, description = "The token entry, placed again", body = EntryView),
        (status = BAD_REQUEST, description = "The shares do not add up to 100 percent", body = crate::error::ErrorBody),
        (status = CONFLICT, description = "Closed or approved", body = crate::error::ErrorBody),
        (status = NOT_FOUND, description = "No such token entry of yours", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn set_splits(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<Uuid>,
    Json(input): Json<SplitsInput>,
) -> Result<Json<EntryView>, ApiError> {
    check_shares(&input.splits)?;
    let splits = input
        .splits
        .into_iter()
        .map(|split| NewSplit {
            message_ids: Vec::new(),
            share_percent: Some(split.share_percent),
            tags: split.tags,
            signals: entry::Signals::default(),
        })
        .collect();
    apply_fix(&state, &caller, id, Fix::Splits(splits)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::{date, datetime};

    fn share(percent: u32) -> ShareInput {
        ShareInput {
            share_percent: percent,
            tags: BTreeMap::new(),
        }
    }

    #[test]
    fn shares_add_up_to_a_hundred() {
        assert!(check_shares(&[share(60), share(40)]).is_ok());
        assert!(check_shares(&[share(60), share(39)]).is_err());
        assert!(check_shares(&[share(100)]).is_err());
        assert!(check_shares(&[share(100), share(0)]).is_err());
        let person = ShareInput {
            share_percent: 50,
            tags: [("person".to_owned(), "me".to_owned())].into(),
        };
        assert!(check_shares(&[person, share(50)]).is_err());
    }

    #[test]
    fn basis_points_round_down_and_stay_in_range() {
        assert_eq!(basis_points(95, 100), Some(9_500));
        assert_eq!(basis_points(1, 3), Some(3_333));
        assert_eq!(basis_points(5, 0), None);
        assert_eq!(basis_points(150, 100), Some(10_000));
        assert_eq!(basis_points(-5, 100), Some(0));
    }

    #[test]
    fn a_sheet_is_due_at_the_end_of_its_last_day() {
        assert_eq!(
            due_at(date!(2026 - 10 - 11)),
            Some(datetime!(2026-10-12 00:00 UTC))
        );
    }
}
