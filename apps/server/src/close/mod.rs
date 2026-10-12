//! The monthly close with locked rows, price table and tag map versions (ADR
//! 0007), adjustments and the cost sheet (docs: Account > Monthly close,
//! Adjustments; Report > The cost sheet).
//!
//! Owner: the close and reports work. Keep this area's code, routes and
//! queries in this folder, and its migrations in `migrations/` named
//! `<timestamp>_close_<what>.sql`.

pub mod account;
pub mod cost_sheet;
pub mod ladder;
pub mod late;
pub mod links;
pub mod period;
pub mod seed;
pub mod signals;

use std::collections::{BTreeMap, BTreeSet};

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use tokencontroller_core::Money;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use self::account::Account;
use self::cost_sheet::CostSheet;
use self::ladder::CostKind;
use self::period::Period;
use crate::AppState;
use crate::bills::amount::{format_amount, parse_amount};
use crate::error::ApiError;
use crate::organizations::Role;
use crate::reports::filter::{Filter, Tags};
use crate::reports::lines::StatusView;
use crate::reports::scope::Scope;
use crate::sign_in::{Caller, Credential};

/// This area's routes, relative to `/api/v1`.
#[must_use]
pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_periods))
        .routes(routes!(get_period))
        .routes(routes!(close_period))
        .routes(routes!(post_adjustment))
        .routes(routes!(get_cost_sheet))
        .merge(links::routes())
}

/// A period and whether it is closed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PeriodSummary {
    #[schema(value_type = String, example = "2026-09")]
    pub period: Period,
    /// Whether the calendar month is over.
    pub ended: bool,
    pub closed: bool,
    #[serde(with = "time::serde::rfc3339::option")]
    #[schema(value_type = Option<String>, format = DateTime)]
    pub closed_at: Option<OffsetDateTime>,
    /// The controller who closed it.
    pub closed_by: Option<String>,
}

/// What still needs a look before a period closes (docs: Account > Monthly
/// close, steps 1 to 3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct OpenItems {
    /// Bills in for the period.
    pub bills: usize,
    /// Organization overhead: cost on no project that may belong on one.
    pub organization_overhead: String,
    pub organization_overhead_entries: usize,
    /// Token sheets whose cadence period is over and that were not
    /// submitted.
    pub late_token_sheets: usize,
    /// With approval on: token entries not approved yet.
    pub unapproved: usize,
    /// Token entries sent back to their agent manager.
    pub rejected: usize,
    /// Token entries on a model missing from the price table.
    pub unknown_model: usize,
}

/// One period, with what is still open in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PeriodView {
    #[serde(flatten)]
    pub summary: PeriodSummary,
    pub approval: bool,
    pub open_items: OpenItems,
    /// Whether a controller can close it now.
    pub can_close: bool,
    /// Why it cannot close yet, in plain words.
    pub blockers: Vec<String>,
}

async fn names(db: &PgPool, ids: &[Uuid]) -> Result<BTreeMap<Uuid, String>, sqlx::Error> {
    let rows = sqlx::query!("SELECT id, name FROM people WHERE id = ANY($1)", ids)
        .fetch_all(db)
        .await?;
    Ok(rows.into_iter().map(|row| (row.id, row.name)).collect())
}

async fn approval_on(db: &PgPool, organization_id: Uuid) -> Result<bool, ApiError> {
    sqlx::query_scalar!("SELECT approval FROM organizations WHERE id = $1", organization_id)
        .fetch_optional(db)
        .await?
        .ok_or(ApiError::NotFound)
}

fn ended(period: Period, now: OffsetDateTime) -> bool {
    period.ends_at().is_ok_and(|end| now >= end)
}

/// Counts token entries of a period per status.
async fn status_counts(
    db: &PgPool,
    organization_id: Uuid,
    period: Period,
) -> Result<BTreeMap<String, i64>, ApiError> {
    let until = period
        .ends_at()
        .map_err(|error| ApiError::BadRequest(error.to_string()))?;
    let rows = sqlx::query!(
        r#"SELECT status, count(*) AS "count!" FROM token_entries
           WHERE organization_id = $1 AND started_at >= $2 AND started_at < $3
           GROUP BY status"#,
        organization_id,
        period.starts_at(),
        until,
    )
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(|row| (row.status, row.count)).collect())
}

/// Every period from the first with a token entry or a bill up to this
/// month, newest first.
#[utoipa::path(
    get,
    path = "/periods",
    tag = "close",
    responses((status = OK, description = "The periods", body = Vec<PeriodSummary>)),
)]
pub(crate) async fn list_periods(
    State(state): State<AppState>,
    caller: Caller,
) -> Result<Json<Vec<PeriodSummary>>, ApiError> {
    caller.require_role(Role::PeopleManager)?;
    let organization_id = caller.organization_id;
    let now = OffsetDateTime::now_utc();
    let first = sqlx::query_scalar!(
        r#"SELECT least(
               (SELECT min(started_at)::date FROM token_entries WHERE organization_id = $1),
               (SELECT min(period) FROM bills WHERE organization_id = $1)
           ) AS first"#,
        organization_id,
    )
    .fetch_one(&state.db)
    .await?;
    let closed = sqlx::query!(
        r#"SELECT c.period AS "period!", c.closed_at AS "closed_at!", p.name AS "closed_by?"
           FROM closed_periods c LEFT JOIN people p ON p.id = c.closed_by
           WHERE c.organization_id = $1"#,
        organization_id,
    )
    .fetch_all(&state.db)
    .await?;
    let current = Period::of(now);
    let mut period = first.map_or(current, Period::of_date);
    let mut periods = Vec::new();
    while period <= current {
        let row = closed.iter().find(|row| row.period == period.first_day());
        periods.push(PeriodSummary {
            period,
            ended: ended(period, now),
            closed: row.is_some(),
            closed_at: row.map(|row| row.closed_at),
            closed_by: row.and_then(|row| row.closed_by.clone()),
        });
        let Ok(next) = period.next() else { break };
        period = next;
    }
    periods.reverse();
    Ok(Json(periods))
}

/// One period and what is still open in it.
#[utoipa::path(
    get,
    path = "/periods/{period}",
    tag = "close",
    params(("period" = String, Path, description = "The period, such as 2026-09")),
    responses(
        (status = OK, description = "The period", body = PeriodView),
        (status = CONFLICT, description = "An exchange rate is missing", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn get_period(
    State(state): State<AppState>,
    caller: Caller,
    Path(period): Path<String>,
) -> Result<Json<PeriodView>, ApiError> {
    caller.require_role(Role::Controller)?;
    let period = parse_period(&period)?;
    Ok(Json(period_view(&state.db, caller.organization_id, period).await?))
}

fn parse_period(text: &str) -> Result<Period, ApiError> {
    text.parse()
        .map_err(|error: period::PeriodError| ApiError::BadRequest(error.to_string()))
}

async fn period_view(db: &PgPool, organization_id: Uuid, period: Period) -> Result<PeriodView, ApiError> {
    let now = OffsetDateTime::now_utc();
    let approval = approval_on(db, organization_id).await?;
    let account = account::load(
        db,
        organization_id,
        period,
        StatusView::All,
        &Scope::everything(),
        &Filter::default(),
    )
    .await?;
    let closed_by = match &account.closed {
        Some(closed) => names(db, &[closed.closed_by]).await?.remove(&closed.closed_by),
        None => None,
    };
    let counts = status_counts(db, organization_id, period).await?;
    let count = |status: &str| usize::try_from(counts.get(status).copied().unwrap_or(0)).unwrap_or(0);
    let late = crate::token_sheets::late_sheets(db, organization_id, Some(period), now.date())
        .await?
        .len();
    let overhead: Vec<_> = account
        .lines
        .iter()
        .filter(|line| account.ladder.cost_kind(&line.line.tags) == CostKind::OrganizationOverhead)
        .collect();
    let overhead_total = Money::sum(overhead.iter().filter_map(|line| line.value), account.currency)
        .map_err(|error| ApiError::Internal(error.into()))?;
    let open_items = OpenItems {
        bills: account.bills.len(),
        organization_overhead: format_amount(overhead_total),
        organization_overhead_entries: overhead.len(),
        late_token_sheets: late,
        unapproved: if approval { count("submitted") } else { 0 },
        rejected: count("rejected"),
        unknown_model: account.lines.iter().filter(|line| line.value.is_none()).count(),
    };
    let blockers = blockers(&account, approval, &open_items, now);
    Ok(PeriodView {
        summary: PeriodSummary {
            period,
            ended: ended(period, now),
            closed: account.closed.is_some(),
            closed_at: account.closed.as_ref().map(|closed| closed.closed_at),
            closed_by,
        },
        approval,
        can_close: blockers.is_empty(),
        blockers,
        open_items,
    })
}

fn blockers(account: &Account, approval: bool, items: &OpenItems, now: OffsetDateTime) -> Vec<String> {
    let mut blockers = Vec::new();
    if account.closed.is_some() {
        blockers.push(format!("{} is already closed. Corrections are adjustments.", account.period));
        return blockers;
    }
    if !ended(account.period, now) {
        blockers.push(format!("{} is not over yet.", account.period));
    }
    if approval && items.unapproved > 0 {
        blockers.push(format!(
            "{} token entries are not approved yet. With approval on, every token entry must be approved before the close.",
            items.unapproved
        ));
    }
    blockers
}

/// Close a period: lock its token entries and keep the tag map, price table
/// and bills it closed with. Answers with the cost sheet.
#[utoipa::path(
    post,
    path = "/periods/{period}/close",
    tag = "close",
    params(("period" = String, Path, description = "The period, such as 2026-09")),
    responses(
        (status = OK, description = "Closed; the cost sheet", body = CostSheet),
        (status = CONFLICT, description = "The period cannot close yet; the message says why", body = crate::error::ErrorBody),
        (status = FORBIDDEN, description = "Only a controller can close a period", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn close_period(
    State(state): State<AppState>,
    caller: Caller,
    Path(period): Path<String>,
) -> Result<Json<CostSheet>, ApiError> {
    caller.require_role(Role::Controller)?;
    caller.require_credential(&[Credential::Session])?;
    let period = parse_period(&period)?;
    close(&state.db, caller.organization_id, period, caller.person_id, OffsetDateTime::now_utc()).await?;
    let sheet = sheet(&state.db, caller.organization_id, period, &[]).await?;
    Ok(Json(sheet))
}

/// Closes a period as of `now`.
///
/// # Errors
///
/// [`ApiError::Conflict`] when the period is not over, already closed, has
/// unapproved token entries with approval on, or changed while closing.
pub async fn close(
    db: &PgPool,
    organization_id: Uuid,
    period: Period,
    controller: Uuid,
    now: OffsetDateTime,
) -> Result<(), ApiError> {
    let view = period_view(db, organization_id, period).await?;
    if let Some(blocker) = view.blockers.first() {
        return Err(ApiError::Conflict(blocker.clone()));
    }
    if !ended(period, now) {
        return Err(ApiError::Conflict(format!("{period} is not over yet.")));
    }
    let account = account::load(
        db,
        organization_id,
        period,
        StatusView::All,
        &Scope::everything(),
        &Filter::default(),
    )
    .await?;
    let figures = crate::bills::reconciliation::provider_figures(&account)
        .map_err(|error| ApiError::Internal(error.into()))?;
    let entry_ids: Vec<Uuid> = account
        .lines
        .iter()
        .map(|line| line.line.entry_id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let (tag_map_version, _) = account::current_ladder(db, organization_id).await?;
    let price_table_version = current_price_table_version(db).await?;

    let mut tx = db.begin().await?;
    sqlx::query!(
        r#"INSERT INTO closed_periods
               (organization_id, period, closed_at, closed_by, reporting_currency, approval,
                tag_map_version_id, price_table_version_id)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"#,
        organization_id,
        period.first_day(),
        now,
        controller,
        account.currency.code(),
        view.approval,
        tag_map_version,
        price_table_version,
    )
    .execute(&mut *tx)
    .await
    .map_err(|error| match error {
        sqlx::Error::Database(ref database) if database.is_unique_violation() => {
            ApiError::Conflict(format!("{period} is already closed."))
        }
        other => ApiError::Database(other),
    })?;
    let locked = sqlx::query!(
        r#"UPDATE token_entries SET status = 'locked'
           WHERE id = ANY($1) AND organization_id = $2 AND status IN ('submitted', 'approved')"#,
        &entry_ids,
        organization_id,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if usize::try_from(locked).ok() != Some(entry_ids.len()) {
        tx.rollback().await?;
        return Err(ApiError::Conflict(
            "token entries changed while the period was closing; close again".to_owned(),
        ));
    }
    write_snapshot(&mut tx, &account, &figures).await?;
    tx.commit().await?;
    tracing::info!(%organization_id, %period, entries = entry_ids.len(), "period closed");
    late::post_late_entries(db, organization_id).await?;
    Ok(())
}

/// Keeps each bill revision and rate the period closed with.
async fn write_snapshot(
    tx: &mut sqlx::PgConnection,
    account: &Account,
    figures: &[crate::bills::reconciliation::ProviderFigures],
) -> Result<(), sqlx::Error> {
    let organization_id = account.organization_id;
    let period = account.period;
    let mut seen_providers = BTreeSet::new();
    for bill in &account.bills {
        let figure = figures.iter().find(|figure| figure.provider == bill.view.provider);
        let first_of_provider = seen_providers.insert(bill.view.provider.clone());
        let (entries, residual) = match figure {
            Some(figure) if first_of_provider => (figure.entries.nanos(), figure.residual.nanos()),
            _ => (0, 0),
        };
        sqlx::query!(
            r#"INSERT INTO closed_period_bills
                   (organization_id, period, bill_revision_id, entries_nanos, residual_nanos, exchange_rate_nanos)
               VALUES ($1, $2, $3, $4, $5, $6)"#,
            organization_id,
            period.first_day(),
            bill.revision_id,
            entries,
            residual,
            bill.rate.nanos(),
        )
        .execute(&mut *tx)
        .await?;
    }
    for (currency, rate) in account.converter.rates() {
        sqlx::query!(
            r#"INSERT INTO closed_period_rates (organization_id, period, currency, exchange_rate_nanos)
               VALUES ($1, $2, $3, $4)"#,
            organization_id,
            period.first_day(),
            currency.code(),
            rate.nanos(),
        )
        .execute(&mut *tx)
        .await?;
    }
    Ok(())
}

/// The price table version in force, kept with a closed period.
async fn current_price_table_version(db: &PgPool) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar!("SELECT id FROM price_table_versions ORDER BY created_at DESC, id DESC LIMIT 1")
        .fetch_optional(db)
        .await
}

/// An adjustment to post (docs: Account > Adjustments).
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct AdjustmentInput {
    /// Plus or minus, in the reporting currency, such as `-120.00`.
    #[schema(example = "-120.00")]
    pub amount: String,
    /// The cost object and any other tags, such as `{"ticket": "KD-214"}`.
    /// No tags puts it on the organization.
    #[serde(default)]
    #[schema(value_type = BTreeMap<String, String>)]
    pub tags: Tags,
    #[schema(example = "Provider credit for the September outage")]
    pub reason: String,
}

/// Post a signed correction to a closed period.
#[utoipa::path(
    post,
    path = "/periods/{period}/adjustments",
    tag = "close",
    params(("period" = String, Path, description = "The closed period, such as 2026-09")),
    request_body = AdjustmentInput,
    responses(
        (status = CREATED, description = "Posted; the cost sheet with it", body = CostSheet),
        (status = CONFLICT, description = "The period is not closed", body = crate::error::ErrorBody),
        (status = BAD_REQUEST, description = "The amount or reason is missing", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn post_adjustment(
    State(state): State<AppState>,
    caller: Caller,
    Path(period): Path<String>,
    Json(input): Json<AdjustmentInput>,
) -> Result<(StatusCode, Json<CostSheet>), ApiError> {
    caller.require_role(Role::Controller)?;
    caller.require_credential(&[Credential::Session])?;
    let period = parse_period(&period)?;
    let organization_id = caller.organization_id;
    let currency = sqlx::query_scalar!(
        "SELECT reporting_currency FROM closed_periods WHERE organization_id = $1 AND period = $2",
        organization_id,
        period.first_day(),
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| {
        ApiError::Conflict(format!(
            "{period} is open: change the token entries themselves. Adjustments correct closed periods."
        ))
    })?;
    let currency = tokencontroller_core::Currency::new(&currency)
        .map_err(|error| ApiError::Internal(error.into()))?;
    let amount = parse_amount(&input.amount, currency).map_err(|error| ApiError::BadRequest(error.to_string()))?;
    if amount.is_zero() {
        return Err(ApiError::BadRequest("an adjustment needs an amount other than zero".to_owned()));
    }
    let reason = input.reason.trim();
    if reason.is_empty() {
        return Err(ApiError::BadRequest("an adjustment needs a reason".to_owned()));
    }
    if input.tags.iter().any(|(name, value)| name.trim().is_empty() || value.trim().is_empty()) {
        return Err(ApiError::BadRequest("every tag needs a name and a value".to_owned()));
    }
    sqlx::query!(
        r#"INSERT INTO adjustments (organization_id, period, amount_nanos, currency, tags, reason, posted_by)
           VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
        organization_id,
        period.first_day(),
        amount.nanos(),
        currency.code(),
        Filter(input.tags).to_json(),
        reason,
        caller.person_id,
    )
    .execute(&state.db)
    .await?;
    let sheet = sheet(&state.db, organization_id, period, &[]).await?;
    Ok((StatusCode::CREATED, Json(sheet)))
}

/// Computes the cost sheet of a period with extra tag columns.
///
/// # Errors
///
/// [`ApiError::Conflict`] when an exchange rate is missing, or a database
/// error.
pub async fn sheet(
    db: &PgPool,
    organization_id: Uuid,
    period: Period,
    columns: &[String],
) -> Result<CostSheet, ApiError> {
    let account = account::load(
        db,
        organization_id,
        period,
        StatusView::All,
        &Scope::everything(),
        &Filter::default(),
    )
    .await?;
    let posted_by: Vec<Uuid> = account.adjustments.iter().filter_map(|a| a.posted_by).collect();
    let names = names(db, &posted_by).await?;
    cost_sheet::compute(&account, columns, &names).map_err(|error| ApiError::Internal(error.into()))
}

/// How to show the cost sheet.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SheetQuery {
    /// Extra tag names as columns, comma-separated, such as `team,purpose`.
    pub columns: Option<String>,
    /// `csv` for a CSV file; JSON otherwise.
    pub format: Option<String>,
}

impl SheetQuery {
    /// The extra column names.
    #[must_use]
    pub fn column_names(&self) -> Vec<String> {
        self.columns
            .as_deref()
            .unwrap_or("")
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect()
    }

    /// Whether a CSV file was asked for.
    #[must_use]
    pub fn wants_csv(&self) -> bool {
        self.format.as_deref() == Some("csv")
    }
}

/// Answers with the cost sheet as JSON or as a CSV download.
#[must_use]
pub fn sheet_response(sheet: CostSheet, csv: bool) -> Response {
    if csv {
        let filename = format!("cost-sheet-{}.csv", sheet.period);
        cost_sheet::to_csv(&sheet).into_download(&filename)
    } else {
        Json(sheet).into_response()
    }
}

/// The cost sheet of a period: final once it is closed, a preview while it
/// is open.
#[utoipa::path(
    get,
    path = "/periods/{period}/cost-sheet",
    tag = "close",
    params(("period" = String, Path, description = "The period, such as 2026-09"), SheetQuery),
    responses(
        (status = OK, description = "The cost sheet, as JSON or CSV", content((CostSheet = "application/json"), (String = "text/csv"))),
        (status = CONFLICT, description = "An exchange rate is missing", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn get_cost_sheet(
    State(state): State<AppState>,
    caller: Caller,
    Path(period): Path<String>,
    Query(query): Query<SheetQuery>,
) -> Result<Response, ApiError> {
    caller.require_role(Role::Controller)?;
    let period = parse_period(&period)?;
    let sheet = sheet(&state.db, caller.organization_id, period, &query.column_names()).await?;
    Ok(sheet_response(sheet, query.wants_csv()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn a_period_ends_when_the_next_month_starts() {
        let september: Period = "2026-09".parse().unwrap();
        assert!(!ended(september, datetime!(2026-09-30 23:59:59 UTC)));
        assert!(ended(september, datetime!(2026-10-01 00:00 UTC)));
    }

    #[test]
    fn columns_are_read_from_a_list() {
        let query = SheetQuery {
            columns: Some(" team, purpose ,,".to_owned()),
            format: Some("csv".to_owned()),
        };
        assert_eq!(query.column_names(), ["team", "purpose"]);
        assert!(query.wants_csv());
        assert!(SheetQuery::default().column_names().is_empty());
        assert!(!SheetQuery::default().wants_csv());
    }
}
