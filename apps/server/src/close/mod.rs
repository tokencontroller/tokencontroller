//! The monthly close with locked rows, price table and tag map versions (ADR
//! 0007), adjustments and the cost sheet (docs: Account > Monthly close,
//! Adjustments; Report > The cost sheet).
//!
//! Owner: the close and reports work. Keep this area's code, routes and
//! queries in this folder, and its migrations in `migrations/` named
//! `<timestamp>_close_<what>.sql`.

pub mod account;
pub mod cost_sheet;
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
use tokencontroller_core::message::Signals;
use tokencontroller_core::{CloseCheck, CostKind, Money, Status, TagValue, TokenEntry};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use self::account::Account;
use self::cost_sheet::CostSheet;
use self::period::{Period, PeriodDates};
use crate::AppState;
use crate::bills::amount::{format_amount, parse_amount};
use crate::error::ApiError;
use crate::organizations::Role;
use crate::reports::filter::{Filter, Tags};
use crate::reports::lines::{EntryStatus, StatusView};
use crate::reports::scope::Scope;
use crate::sign_in::Caller;

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
    /// Providers whose token entries are on no bill yet. On a metered plan
    /// the period cannot close until their bill is in.
    pub providers_without_bill: Vec<String>,
    /// Providers whose bill may still change, such as Anthropic Enterprise
    /// before its figures are final. The period cannot close until it is.
    pub bills_not_final: Vec<String>,
    /// Organization overhead: cost on no project that may belong on one.
    pub organization_overhead: String,
    /// The reporting currency, which `organization_overhead` is in.
    #[schema(example = "EUR")]
    pub currency: String,
    pub organization_overhead_entries: usize,
    /// Token sheets whose cadence period is over and that were not
    /// submitted.
    pub late_token_sheets: usize,
    /// With approval on: token entries submitted or rejected but not
    /// approved yet.
    pub unapproved: usize,
    /// Token entries sent back to their agent manager.
    pub rejected: usize,
    /// Token entries without an agent manager: they show as an error until
    /// someone takes them.
    pub without_agent_manager: usize,
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

/// People's names and emails.
struct People {
    names: BTreeMap<Uuid, String>,
    emails: BTreeMap<Uuid, String>,
}

async fn people(db: &PgPool, ids: &[Uuid]) -> Result<People, sqlx::Error> {
    let rows = sqlx::query!("SELECT id, name, email FROM people WHERE id = ANY($1)", ids)
        .fetch_all(db)
        .await?;
    let mut people = People {
        names: BTreeMap::new(),
        emails: BTreeMap::new(),
    };
    for row in rows {
        let name = if row.name.trim().is_empty() {
            row.email.clone()
        } else {
            row.name
        };
        people.names.insert(row.id, name);
        people.emails.insert(row.id, row.email);
    }
    Ok(people)
}

fn ended(period: Period, now: OffsetDateTime) -> bool {
    period.ends_at().is_ok_and(|end| now >= end)
}

/// The core's check of a period's token entries: what is unapproved,
/// rejected, or without an agent manager.
async fn close_check(
    db: &PgPool,
    organization_id: Uuid,
    period: Period,
    approval: bool,
) -> Result<CloseCheck, ApiError> {
    let until = period
        .ends_at()
        .map_err(|error| ApiError::BadRequest(error.to_string()))?;
    let rows = sqlx::query!(
        r#"SELECT e.status, e.started_at, e.ended_at, e.agent_harness, e.on_token_sheet,
                  p.email AS "email?"
           FROM token_entries e LEFT JOIN people p ON p.id = e.agent_manager_id
           WHERE e.organization_id = $1 AND e.started_at >= $2 AND e.started_at < $3"#,
        organization_id,
        period.starts_at(),
        until,
    )
    .fetch_all(db)
    .await?;
    let entries: Vec<TokenEntry> = rows
        .into_iter()
        .filter_map(|row| {
            Some(TokenEntry {
                agent_harness: row.agent_harness,
                agent_name: None,
                agent_manager: row.email.and_then(|email| TagValue::new(&email).ok()),
                session_id: None,
                started_at: row.started_at,
                ended_at: row.ended_at,
                summary: None,
                arrival: if row.on_token_sheet {
                    tokencontroller_core::Arrival::TokenSheet
                } else {
                    tokencontroller_core::Arrival::Automatic
                },
                status: EntryStatus::parse(&row.status)?.core(),
                tags: tokencontroller_core::Tags::new(),
                configured: tokencontroller_core::Tags::new(),
                signals: Signals::default(),
                segments: Vec::new(),
                hand_split: Vec::new(),
            })
        })
        .collect();
    Ok(CloseCheck::of(period, approval, &entries))
}

/// Every period from the first with a token entry or a bill up to this
/// month, newest first. Everyone can list them, to pick a period.
#[utoipa::path(
    get,
    path = "/periods",
    tag = "close",
    params(crate::reports::csv::FormatQuery),
    responses((status = OK, description = "The periods, as JSON or CSV", content((Vec<PeriodSummary> = "application/json"), (String = "text/csv")))),
)]
pub(crate) async fn list_periods(
    State(state): State<AppState>,
    caller: Caller,
    Query(query): Query<crate::reports::csv::FormatQuery>,
) -> Result<Response, ApiError> {
    caller.require_credential(crate::reports::READERS)?;
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
    let current = Period::containing(now);
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
        let Ok(next) = period.following() else { break };
        period = next;
    }
    periods.reverse();
    crate::reports::csv::table(&periods, PERIOD_COLUMNS, query.wants_csv(), "periods.csv")
}

/// The columns of the periods table and its CSV file.
const PERIOD_COLUMNS: &[crate::reports::csv::Column] = &[
    ("period", "/period"),
    ("ended", "/ended"),
    ("closed", "/closed"),
    ("closed_at", "/closed_at"),
    ("closed_by", "/closed_by"),
];

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
    Ok(Json(
        period_view(&state.db, caller.organization_id, period).await?,
    ))
}

fn parse_period(text: &str) -> Result<Period, ApiError> {
    Period::parse(text).map_err(|error| ApiError::BadRequest(error.to_string()))
}

async fn period_view(
    db: &PgPool,
    organization_id: Uuid,
    period: Period,
) -> Result<PeriodView, ApiError> {
    let now = OffsetDateTime::now_utc();
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
        Some(closed) => people(db, &[closed.closed_by])
            .await?
            .names
            .remove(&closed.closed_by),
        None => None,
    };
    let check = close_check(db, organization_id, period, account.approval).await?;
    let late = crate::token_sheets::late_sheets(db, organization_id, Some(period), now.date())
        .await?
        .len();
    let overhead: Vec<_> = account
        .counted()
        .filter(|line| {
            account.tag_map.cost_kind(&line.line.core_tags()) == CostKind::OrganizationOverhead
        })
        .collect();
    let overhead_total = Money::sum(
        overhead.iter().filter_map(|line| line.value),
        account.currency,
    )
    .map_err(|error| ApiError::Internal(error.into()))?;
    let without_bill = account.providers_without_bill();
    let open_items = OpenItems {
        bills: account.bills.len(),
        providers_without_bill: without_bill.keys().cloned().collect(),
        bills_not_final: account
            .bills
            .iter()
            .filter(|bill| !bill.view.is_final)
            .map(|bill| bill.view.provider.clone())
            .collect(),
        organization_overhead: format_amount(overhead_total),
        currency: account.currency.code(),
        organization_overhead_entries: overhead.len(),
        late_token_sheets: late,
        unapproved: check.unapproved,
        rejected: check.rejected,
        without_agent_manager: check.without_agent_manager,
        unknown_model: account
            .counted()
            .filter(|line| line.value.is_none())
            .count(),
    };
    let blockers = blockers(&account, &check, &without_bill, now);
    Ok(PeriodView {
        summary: PeriodSummary {
            period,
            ended: ended(period, now),
            closed: account.closed.is_some(),
            closed_at: account.closed.as_ref().map(|closed| closed.closed_at),
            closed_by,
        },
        approval: account.approval,
        can_close: blockers.is_empty(),
        blockers,
        open_items,
    })
}

fn blockers(
    account: &Account,
    check: &CloseCheck,
    without_bill: &BTreeMap<String, usize>,
    now: OffsetDateTime,
) -> Vec<String> {
    let mut blockers = Vec::new();
    if account.closed.is_some() {
        blockers.push(format!(
            "{} is already closed. Corrections are adjustments.",
            account.period
        ));
        return blockers;
    }
    if !ended(account.period, now) {
        blockers.push(format!("{} is not over yet.", account.period));
    }
    if !check.can_close() {
        blockers.push(format!(
            "{} token entries are not approved yet. With approval on, every token entry must be approved before the close.",
            check.unapproved
        ));
    }
    for bill in account.bills.iter().filter(|bill| !bill.view.is_final) {
        blockers.push(format!(
            "The bill from {} for {} is not final yet. The close waits until {} stops changing its figures.",
            bill.view.provider, account.period, bill.view.provider
        ));
    }
    if account.metered {
        for (provider, count) in without_bill {
            blockers.push(format!(
                "There is no bill from {provider} for {} yet, so {count} token entries are on no bill. Send the bill from the API, as CSV or typed in.",
                account.period
            ));
        }
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
    caller.require_session()?;
    let period = parse_period(&period)?;
    close(
        &state.db,
        caller.organization_id,
        period,
        caller.person_id,
        OffsetDateTime::now_utc(),
    )
    .await?;
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
    let sheet = account.cost_sheet(&[], &BTreeMap::new())?;
    let entry_ids: Vec<Uuid> = account
        .counted()
        .map(|line| line.line.entry_id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    // Submitted and approved token entries lock; drafts and rejected ones
    // stay as they are and reach the period only as late adjustments.
    let locking: Vec<String> = EntryStatus::ALL
        .into_iter()
        .filter(|status| {
            status.core() != Status::Locked
                && tokencontroller_core::close::status_after_close(status.core()) == Status::Locked
        })
        .map(|status| status.as_str().to_owned())
        .collect();
    let tag_map_version = i32::try_from(account.tag_map_version.number())
        .map_err(|error| ApiError::Internal(error.into()))?;
    let price_table_version = current_price_table_version(db).await?;

    let mut tx = db.begin().await?;
    sqlx::query!(
        r#"INSERT INTO closed_periods
               (organization_id, period, closed_at, closed_by, reporting_currency, approval,
                tag_map_version, price_table_version_id)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"#,
        organization_id,
        period.first_day(),
        now,
        controller,
        account.currency.code(),
        account.approval,
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
           WHERE id = ANY($1) AND organization_id = $2 AND status = ANY($3)"#,
        &entry_ids,
        organization_id,
        &locking,
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
    write_snapshot(&mut tx, &account, &sheet).await?;
    tx.commit().await?;
    tracing::info!(%organization_id, %period, entries = entry_ids.len(), "period closed");
    late::post_late_entries(db, organization_id).await?;
    Ok(())
}

/// Keeps each bill revision as it was reconciled and every rate the period
/// closed with.
async fn write_snapshot(
    tx: &mut sqlx::PgConnection,
    account: &Account,
    sheet: &tokencontroller_core::CostSheet,
) -> Result<(), sqlx::Error> {
    let organization_id = account.organization_id;
    let period = account.period;
    for (bill, block) in account.bills.iter().zip(&sheet.blocks) {
        let reconciliation = block.bill.reconciliation;
        sqlx::query!(
            r#"INSERT INTO closed_period_bills
                   (organization_id, period, bill_revision_id, entries_nanos, residual_nanos, exchange_rate_nanos)
               VALUES ($1, $2, $3, $4, $5, $6)"#,
            organization_id,
            period.first_day(),
            bill.revision_id,
            reconciliation.explained.nanos(),
            reconciliation.residual.nanos(),
            crate::bills::amount::rate_number(block.bill.rate).nanos(),
        )
        .execute(&mut *tx)
        .await?;
    }
    for rate in account.converter.rates() {
        sqlx::query!(
            r#"INSERT INTO closed_period_rates
                   (organization_id, period, from_currency, to_currency, exchange_rate_nanos)
               VALUES ($1, $2, $3, $4, $5)"#,
            organization_id,
            period.first_day(),
            rate.from().code(),
            rate.to().code(),
            crate::bills::amount::rate_number(rate).nanos(),
        )
        .execute(&mut *tx)
        .await?;
    }
    Ok(())
}

/// The price table version in force, kept with a closed period.
async fn current_price_table_version(db: &PgPool) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar!(
        "SELECT id FROM price_table_versions ORDER BY created_at DESC, id DESC LIMIT 1"
    )
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
    caller.require_session()?;
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
    let amount = parse_amount(&input.amount, currency)
        .map_err(|error| ApiError::BadRequest(error.to_string()))?;
    if amount.is_zero() {
        return Err(ApiError::BadRequest(
            "an adjustment needs an amount other than zero".to_owned(),
        ));
    }
    let reason = input.reason.trim();
    if reason.is_empty() {
        return Err(ApiError::BadRequest(
            "an adjustment needs a reason".to_owned(),
        ));
    }
    if input
        .tags
        .iter()
        .any(|(name, value)| name.trim().is_empty() || value.trim().is_empty())
    {
        return Err(ApiError::BadRequest(
            "every tag needs a name and a value".to_owned(),
        ));
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

/// A period's account for the cost sheet, with the names and emails of
/// whoever posted its adjustments.
async fn sheet_account(
    db: &PgPool,
    organization_id: Uuid,
    period: Period,
) -> Result<(Account, People), ApiError> {
    let account = account::load(
        db,
        organization_id,
        period,
        StatusView::All,
        &Scope::everything(),
        &Filter::default(),
    )
    .await?;
    let posted_by: Vec<Uuid> = account
        .adjustments
        .iter()
        .filter_map(|a| a.posted_by)
        .collect();
    let people = people(db, &posted_by).await?;
    Ok((account, people))
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
    let (account, people) = sheet_account(db, organization_id, period).await?;
    Ok(cost_sheet::compute(
        &account,
        columns,
        &people.names,
        &people.emails,
    )?)
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

/// Answers with the cost sheet as JSON or as the core's CSV download.
///
/// # Errors
///
/// As [`sheet`].
pub async fn sheet_response(
    db: &PgPool,
    organization_id: Uuid,
    period: Period,
    query: &SheetQuery,
) -> Result<Response, ApiError> {
    match sheet_or_csv(db, organization_id, period, query).await? {
        Ok(sheet) => Ok(Json(sheet).into_response()),
        Err(csv) => Ok(csv),
    }
}

/// The cost sheet as JSON, or as a CSV download (the `Err` side) when the
/// query asks for CSV.
///
/// # Errors
///
/// [`ApiError`] for a missing exchange rate or a database failure.
pub async fn sheet_or_csv(
    db: &PgPool,
    organization_id: Uuid,
    period: Period,
    query: &SheetQuery,
) -> Result<Result<CostSheet, Response>, ApiError> {
    let (account, people) = sheet_account(db, organization_id, period).await?;
    let columns = query.column_names();
    if query.wants_csv() {
        let text = cost_sheet::to_csv(&account, &columns, &people.emails)?;
        let filename = format!("cost-sheet-{period}.csv");
        return Ok(Err(crate::reports::csv::download(&text, &filename)?));
    }
    Ok(Ok(cost_sheet::compute(
        &account,
        &columns,
        &people.names,
        &people.emails,
    )?))
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
    sheet_response(&state.db, caller.organization_id, period, &query).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn a_period_ends_when_the_next_month_starts() {
        let september = Period::parse("2026-09").unwrap();
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
