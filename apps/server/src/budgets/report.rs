//! Token budget vs actual (docs: Report > Token budget vs actual): one row
//! per token budget in force for a period, whatever tag filter it sits on,
//! with the token budget, the actual cost and the difference. Overrun is
//! actual above token budget; underrun is actual below. Each row opens down
//! to the tickets and token entries behind its actual.

use std::collections::{BTreeMap, BTreeSet};

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use tokencontroller_core::{Currency, Money, MoneyError};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::actual::{self, Actual, Actuals};
use super::token_budget::{self, TokenBudget, TokenBudgetView, Viewer};
use crate::AppState;
use crate::bills::amount::format_amount;
use crate::bills::rates::{self, RateError};
use crate::close::period::Period;
use crate::error::ApiError;
use crate::reports::csv::Csv;
use crate::reports::entries::{self, EntryView};
use crate::reports::filter::Tags;
use crate::reports::lines::StatusView;
use crate::sign_in::Caller;

pub(super) fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(token_budget_vs_actual))
        .routes(routes!(token_budget_detail))
}

/// How actual compares with the token budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Actual above token budget.
    Overrun,
    /// Actual below token budget.
    Underrun,
    /// Actual exactly at the token budget.
    AtTokenBudget,
}

impl Outcome {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Overrun => "overrun",
            Self::Underrun => "underrun",
            Self::AtTokenBudget => "at_token_budget",
        }
    }

    /// The outcome of a `difference` of actual minus token budget. When
    /// token entries on an unknown model are not in actual, an actual that
    /// is not already above the token budget could still be: `None`.
    #[must_use]
    pub const fn of(difference: Money, complete: bool) -> Option<Self> {
        if difference.nanos() > 0 {
            Some(Self::Overrun)
        } else if !complete {
            None
        } else if difference.is_zero() {
            Some(Self::AtTokenBudget)
        } else {
            Some(Self::Underrun)
        }
    }
}

/// One token budget against its actual.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TokenBudgetRow {
    pub token_budget: TokenBudgetView,
    /// The period actual is counted over; `null` for an all-time token
    /// budget, which counts every period.
    #[schema(example = "2026-09")]
    pub actual_period: Option<String>,
    /// The token budget in the reporting currency.
    #[schema(example = "1500.00")]
    pub amount: String,
    /// What was spent, in the reporting currency: list value of the counting
    /// token entries matching the filter, plus adjustments to closed
    /// periods. Token entries on an unknown model are not in it.
    #[schema(example = "1620.00")]
    pub actual: String,
    /// Actual minus token budget: above zero is an overrun, below zero an
    /// underrun.
    #[schema(example = "120.00")]
    pub difference: String,
    /// `null` when token entries on an unknown model leave it open.
    pub outcome: Option<Outcome>,
    pub token_entries: usize,
    /// Token entries on a model missing from the price table: counted, but
    /// their cost is not known, so it is not in `actual`.
    pub unknown_model_entries: usize,
}

/// Token budget vs actual for one period.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TokenBudgetReport {
    #[schema(example = "2026-09")]
    pub period: String,
    #[schema(example = "EUR")]
    pub currency: String,
    /// The view of token entries counted: `all` or `approved`.
    pub status: StatusView,
    /// Token budgets overlap, so rows are never added up.
    pub rows: Vec<TokenBudgetRow>,
}

/// Which token budget vs actual to show.
#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ReportQuery {
    /// The period, such as `2026-09`. Left out: this month.
    #[param(example = "2026-09")]
    pub period: Option<String>,
    /// `all` (the default) or `approved`.
    pub status: Option<StatusView>,
    /// `csv` for a CSV file.
    pub format: Option<String>,
}

/// Which row of token budget vs actual to open down.
#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct DetailQuery {
    /// The period, such as `2026-09`. Left out: this month.
    #[param(example = "2026-09")]
    pub period: Option<String>,
    /// `all` (the default) or `approved`.
    pub status: Option<StatusView>,
    /// `csv` for a CSV file.
    pub format: Option<String>,
    /// The table the CSV file holds: `tickets` (the default) or
    /// `token_entries`.
    pub table: Option<String>,
}

/// The period asked for, or this month.
fn period_or_now(text: Option<&str>, now: OffsetDateTime) -> Result<Period, ApiError> {
    match text.map(str::trim).filter(|text| !text.is_empty()) {
        Some(text) => text
            .parse()
            .map_err(|error: crate::close::period::PeriodError| {
                ApiError::BadRequest(error.to_string())
            }),
        None => Ok(Period::of(now)),
    }
}

fn wants_csv(format: Option<&str>) -> bool {
    format == Some("csv")
}

/// The token budget in the reporting currency, at the rate of the period's
/// last day, or today for all time and a running period.
async fn in_reporting_currency(
    db: &PgPool,
    amount: Money,
    currency: Currency,
    period: Option<Period>,
    today: time::Date,
) -> Result<Money, ApiError> {
    if amount.currency() == currency {
        return Ok(amount);
    }
    let day = period.map_or(today, |period| period.last_day().min(today));
    let rate = rates::between(db, amount.currency(), currency, day)
        .await?
        .map_err(|error: RateError| ApiError::Conflict(error.to_string()))?;
    rate.convert(amount, currency)
        .map_err(|error| ApiError::Conflict(error.to_string()))
}

fn money_error(error: MoneyError) -> ApiError {
    ApiError::Internal(error.into())
}

/// One row from a token budget and its actual.
fn row(
    budget: &TokenBudget,
    amount: Money,
    actual: &Actual<'_>,
    actual_period: Option<Period>,
) -> Result<TokenBudgetRow, MoneyError> {
    let difference = actual.value.checked_sub(amount)?;
    Ok(TokenBudgetRow {
        token_budget: TokenBudgetView::from(budget),
        actual_period: actual_period.map(|period| period.to_string()),
        amount: format_amount(amount),
        actual: format_amount(actual.value),
        difference: format_amount(difference),
        outcome: Outcome::of(difference, actual.is_complete()),
        token_entries: actual.token_entries,
        unknown_model_entries: actual.unknown_model_entries,
    })
}

/// The periods to load for these token budgets shown for `period`.
async fn periods_for(
    db: &PgPool,
    organization_id: Uuid,
    budgets: &[TokenBudget],
    period: Period,
    now: OffsetDateTime,
) -> Result<BTreeSet<Period>, sqlx::Error> {
    let mut periods = BTreeSet::from([period]);
    if budgets
        .iter()
        .any(|budget| budget.applies.actual_period(period).is_none())
    {
        periods.extend(actual::all_periods(db, organization_id, now).await?);
    }
    Ok(periods)
}

/// Computes token budget vs actual for `caller`.
///
/// # Errors
///
/// [`ApiError::Conflict`] when an exchange rate is missing, or a database
/// error.
pub async fn report(
    db: &PgPool,
    caller: &Caller,
    period: Period,
    status: StatusView,
) -> Result<TokenBudgetReport, ApiError> {
    let now = OffsetDateTime::now_utc();
    let organization_id = caller.organization_id;
    let viewer = Viewer::of(db, caller).await?;
    let budgets: Vec<TokenBudget> = token_budget::visible(db, organization_id, &viewer)
        .await?
        .into_iter()
        .filter(|budget| budget.applies.in_force(period))
        .collect();
    let periods = periods_for(db, organization_id, &budgets, period, now).await?;
    let actuals = Actuals::load(db, organization_id, &periods, status, &viewer.scope).await?;
    let mut rows = Vec::new();
    for budget in &budgets {
        let actual_period = budget.applies.actual_period(period);
        let amount = in_reporting_currency(
            db,
            budget.amount,
            actuals.currency,
            actual_period,
            now.date(),
        )
        .await?;
        let actual = actuals
            .of(&budget.filter, actual_period)
            .map_err(money_error)?;
        rows.push((
            actual
                .value
                .checked_sub(amount)
                .map_err(money_error)?
                .nanos(),
            row(budget, amount, &actual, actual_period).map_err(money_error)?,
        ));
    }
    // The largest overrun first, the largest underrun last.
    rows.sort_by(|(a, row_a), (b, row_b)| {
        b.cmp(a)
            .then_with(|| row_a.token_budget.id.cmp(&row_b.token_budget.id))
    });
    Ok(TokenBudgetReport {
        period: period.to_string(),
        currency: actuals.currency.code(),
        status,
        rows: rows.into_iter().map(|(_, row)| row).collect(),
    })
}

/// Token budget vs actual as a CSV file.
#[must_use]
pub fn to_csv(report: &TokenBudgetReport) -> Csv {
    let mut csv = Csv::new([
        "token_budget",
        "name",
        "filter",
        "applies",
        "actual_period",
        "amount",
        "actual",
        "difference",
        "outcome",
        "token_entries",
        "unknown_model_entries",
        "currency",
    ]);
    for row in &report.rows {
        csv.row([
            row.token_budget.id.to_string(),
            row.token_budget.name.clone().unwrap_or_default(),
            row.token_budget.filter.to_string(),
            row.token_budget.applies.clone(),
            row.actual_period
                .clone()
                .unwrap_or_else(|| "all time".to_owned()),
            row.amount.clone(),
            row.actual.clone(),
            row.difference.clone(),
            row.outcome
                .map_or("unknown model", Outcome::as_str)
                .to_owned(),
            row.token_entries.to_string(),
            row.unknown_model_entries.to_string(),
            report.currency.clone(),
        ]);
    }
    csv
}

/// Token budget vs actual: one row per token budget in force for the
/// period, with the token budget, the actual cost and the difference.
/// Token budgets overlap, so the rows are never added up.
#[utoipa::path(
    get,
    path = "/reports/token-budgets",
    tag = "token_budgets",
    params(ReportQuery),
    responses(
        (status = OK, description = "Token budget vs actual, as JSON or CSV", content((TokenBudgetReport = "application/json"), (String = "text/csv"))),
        (status = BAD_REQUEST, description = "The period is not valid", body = crate::error::ErrorBody),
        (status = CONFLICT, description = "An exchange rate is missing", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn token_budget_vs_actual(
    State(state): State<AppState>,
    caller: Caller,
    Query(query): Query<ReportQuery>,
) -> Result<Response, ApiError> {
    let period = period_or_now(query.period.as_deref(), OffsetDateTime::now_utc())?;
    let report = report(&state.db, &caller, period, query.status.unwrap_or_default()).await?;
    if wants_csv(query.format.as_deref()) {
        return Ok(to_csv(&report).into_download(&format!("token-budget-vs-actual-{period}.csv")));
    }
    Ok(Json(report).into_response())
}

/// The actual of one ticket behind a token budget.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TicketActual {
    /// The ticket key; `null` for cost on no ticket.
    #[schema(example = "KD-214")]
    pub ticket: Option<String>,
    pub title: Option<String>,
    #[schema(example = "41.87")]
    pub actual: String,
    pub token_entries: usize,
    pub unknown_model_entries: usize,
}

/// An adjustment to a closed period that counts in actual.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct AdjustmentLine {
    pub id: Uuid,
    #[schema(example = "-120.00")]
    pub amount: String,
    #[schema(value_type = BTreeMap<String, String>)]
    pub tags: Tags,
    pub reason: String,
    /// Posted from a token sheet submitted after its period closed.
    pub late: bool,
}

/// One token budget opened down to what makes up its actual.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TokenBudgetDetail {
    #[schema(example = "2026-09")]
    pub period: String,
    #[schema(example = "EUR")]
    pub currency: String,
    pub row: TokenBudgetRow,
    /// Actual per ticket, the largest first; they add up to actual.
    pub tickets: Vec<TicketActual>,
    /// The token entries behind actual, with their own list value.
    pub token_entries: Vec<EntryView>,
    pub adjustments: Vec<AdjustmentLine>,
}

#[derive(Default)]
struct TicketBucket {
    nanos: i64,
    entries: BTreeSet<Uuid>,
    unknown: BTreeSet<Uuid>,
}

/// The actual of one ticket, before titles are added.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TicketSum {
    ticket: Option<String>,
    value: Money,
    token_entries: usize,
    unknown_model_entries: usize,
}

/// Actual per `ticket` tag value, the largest first and cost on no ticket
/// last.
fn per_ticket(actual: &Actual<'_>, currency: Currency) -> Result<Vec<TicketSum>, MoneyError> {
    let mut buckets: BTreeMap<Option<String>, TicketBucket> = BTreeMap::new();
    for line in &actual.lines {
        let bucket = buckets
            .entry(line.line.tags.get("ticket").cloned())
            .or_default();
        bucket.entries.insert(line.line.entry_id);
        match line.value {
            Some(value) => {
                bucket.nanos = bucket
                    .nanos
                    .checked_add(value.nanos())
                    .ok_or(MoneyError::Overflow)?;
            }
            None => {
                bucket.unknown.insert(line.line.entry_id);
            }
        }
    }
    for adjustment in &actual.adjustments {
        let bucket = buckets
            .entry(adjustment.tags.get("ticket").cloned())
            .or_default();
        bucket.nanos = bucket
            .nanos
            .checked_add(adjustment.amount.nanos())
            .ok_or(MoneyError::Overflow)?;
    }
    let mut tickets: Vec<TicketSum> = buckets
        .into_iter()
        .map(|(ticket, bucket)| TicketSum {
            ticket,
            value: Money::from_nanos(bucket.nanos, currency),
            token_entries: bucket.entries.len(),
            unknown_model_entries: bucket.unknown.len(),
        })
        .collect();
    tickets.sort_by(|a, b| {
        a.ticket
            .is_none()
            .cmp(&b.ticket.is_none())
            .then_with(|| b.value.nanos().cmp(&a.value.nanos()))
            .then_with(|| a.ticket.cmp(&b.ticket))
    });
    Ok(tickets)
}

/// The titles of some tickets, by key.
async fn titles(
    db: &PgPool,
    organization_id: Uuid,
    keys: &[String],
) -> Result<BTreeMap<String, String>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"SELECT DISTINCT ON (key) key, title AS "title!"
           FROM tickets WHERE organization_id = $1 AND key = ANY($2) AND title IS NOT NULL
           ORDER BY key, id"#,
        organization_id,
        keys,
    )
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(|row| (row.key, row.title)).collect())
}

/// Opens one token budget down to its tickets, token entries and
/// adjustments.
///
/// # Errors
///
/// [`ApiError::NotFound`] for a token budget the caller does not see,
/// [`ApiError::Conflict`] when it is not in force in `period` or an exchange
/// rate is missing, or a database error.
pub async fn detail(
    db: &PgPool,
    caller: &Caller,
    id: Uuid,
    period: Period,
    status: StatusView,
) -> Result<TokenBudgetDetail, ApiError> {
    let now = OffsetDateTime::now_utc();
    let organization_id = caller.organization_id;
    let viewer = Viewer::of(db, caller).await?;
    let budget = token_budget::find_visible(db, organization_id, &viewer, id).await?;
    if !budget.applies.in_force(period) {
        return Err(ApiError::Conflict(format!(
            "this token budget applies to {}, not to {period}",
            budget.applies.describe()
        )));
    }
    let periods = periods_for(
        db,
        organization_id,
        std::slice::from_ref(&budget),
        period,
        now,
    )
    .await?;
    let actuals = Actuals::load(db, organization_id, &periods, status, &viewer.scope).await?;
    let currency = actuals.currency;
    let actual_period = budget.applies.actual_period(period);
    let amount =
        in_reporting_currency(db, budget.amount, currency, actual_period, now.date()).await?;
    let actual = actuals
        .of(&budget.filter, actual_period)
        .map_err(money_error)?;
    let row = row(&budget, amount, &actual, actual_period).map_err(money_error)?;
    let tickets = per_ticket(&actual, currency).map_err(money_error)?;
    let keys: Vec<String> = tickets
        .iter()
        .filter_map(|sum| sum.ticket.clone())
        .collect();
    let titles = titles(db, organization_id, &keys).await?;
    let token_entries = entries::entries(
        db,
        actual.lines.iter().map(|line| line.line.clone()).collect(),
    )
    .await?;
    Ok(TokenBudgetDetail {
        period: period.to_string(),
        currency: currency.code(),
        row,
        tickets: tickets
            .into_iter()
            .map(|sum| TicketActual {
                title: sum.ticket.as_ref().and_then(|key| titles.get(key).cloned()),
                ticket: sum.ticket,
                actual: format_amount(sum.value),
                token_entries: sum.token_entries,
                unknown_model_entries: sum.unknown_model_entries,
            })
            .collect(),
        token_entries,
        adjustments: actual
            .adjustments
            .iter()
            .map(|adjustment| AdjustmentLine {
                id: adjustment.id,
                amount: format_amount(adjustment.amount),
                tags: adjustment.tags.clone(),
                reason: adjustment.reason.clone(),
                late: adjustment.late,
            })
            .collect(),
    })
}

/// The tickets of one token budget as a CSV file.
#[must_use]
pub fn tickets_csv(detail: &TokenBudgetDetail) -> Csv {
    let mut csv = Csv::new([
        "ticket",
        "title",
        "actual",
        "token_entries",
        "unknown_model_entries",
        "currency",
    ]);
    for ticket in &detail.tickets {
        csv.row([
            ticket
                .ticket
                .clone()
                .unwrap_or_else(|| "no ticket".to_owned()),
            ticket.title.clone().unwrap_or_default(),
            ticket.actual.clone(),
            ticket.token_entries.to_string(),
            ticket.unknown_model_entries.to_string(),
            detail.currency.clone(),
        ]);
    }
    csv
}

/// One token budget opened down to the tickets (by their `ticket` tag) and
/// token entries behind its actual, for a period it is in force in.
#[utoipa::path(
    get,
    path = "/reports/token-budgets/{id}",
    tag = "token_budgets",
    params(("id" = Uuid, Path, description = "The token budget"), DetailQuery),
    responses(
        (status = OK, description = "The token budget's actual opened down, as JSON or CSV", content((TokenBudgetDetail = "application/json"), (String = "text/csv"))),
        (status = BAD_REQUEST, description = "The period or table is not valid", body = crate::error::ErrorBody),
        (status = CONFLICT, description = "The token budget is not in force in the period, or an exchange rate is missing", body = crate::error::ErrorBody),
        (status = NOT_FOUND, description = "No such token budget", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn token_budget_detail(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<Uuid>,
    Query(query): Query<DetailQuery>,
) -> Result<Response, ApiError> {
    let period = period_or_now(query.period.as_deref(), OffsetDateTime::now_utc())?;
    let detail = detail(
        &state.db,
        &caller,
        id,
        period,
        query.status.unwrap_or_default(),
    )
    .await?;
    if !wants_csv(query.format.as_deref()) {
        return Ok(Json(detail).into_response());
    }
    match query.table.as_deref().unwrap_or("tickets") {
        "tickets" => {
            Ok(tickets_csv(&detail)
                .into_download(&format!("token-budget-{id}-{period}-tickets.csv")))
        }
        "token_entries" => Ok(entries::to_csv(&detail.token_entries)
            .into_download(&format!("token-budget-{id}-{period}-token-entries.csv"))),
        other => Err(ApiError::BadRequest(format!(
            "{other:?} is not a table: use tickets or token_entries"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::close::account::{Adjustment, Line};
    use crate::reports::filter::Filter;
    use crate::reports::lines::{CostLine, EntryStatus};
    use time::macros::datetime;

    fn eur(cents: i64) -> Money {
        Money::from_minor_units(cents, Currency::EUR).unwrap()
    }

    #[test]
    fn overrun_is_above_and_underrun_below() {
        assert_eq!(Outcome::of(eur(1), true), Some(Outcome::Overrun));
        assert_eq!(
            Outcome::of(Money::from_nanos(1, Currency::EUR), true),
            Some(Outcome::Overrun)
        );
        assert_eq!(Outcome::of(eur(0), true), Some(Outcome::AtTokenBudget));
        assert_eq!(Outcome::of(eur(-1), true), Some(Outcome::Underrun));
        assert_eq!(
            Outcome::of(Money::from_nanos(-1, Currency::EUR), true),
            Some(Outcome::Underrun)
        );
    }

    #[test]
    fn unknown_models_leave_an_underrun_open_but_not_an_overrun() {
        assert_eq!(Outcome::of(eur(1), false), Some(Outcome::Overrun));
        assert_eq!(Outcome::of(eur(0), false), None);
        assert_eq!(Outcome::of(eur(-500), false), None);
        assert_eq!(Outcome::AtTokenBudget.as_str(), "at_token_budget");
        assert_eq!(Outcome::Overrun.as_str(), "overrun");
        assert_eq!(Outcome::Underrun.as_str(), "underrun");
    }

    fn line(entry: u128, cents: Option<i64>, ticket: Option<&str>) -> Line {
        let mut tags = Tags::new();
        if let Some(ticket) = ticket {
            tags.insert("ticket".to_owned(), ticket.to_owned());
        }
        Line {
            line: CostLine {
                split_id: Uuid::from_u128(entry),
                entry_id: Uuid::from_u128(entry),
                agent_manager_id: None,
                provider: "anthropic".to_owned(),
                agent_harness: "claude-code".to_owned(),
                agent_name: None,
                model: "claude-opus-5".to_owned(),
                effort: None,
                speed: None,
                started_at: datetime!(2026-09-01 09:00 UTC),
                status: EntryStatus::Submitted,
                automatic: false,
                summary: String::new(),
                list_value: cents.map(eur),
                tags,
            },
            value: cents.map(eur),
        }
    }

    #[test]
    fn tickets_add_up_to_actual_with_no_ticket_last() {
        let lines = [
            line(1, Some(100), Some("KD-1")),
            line(2, Some(900), None),
            line(3, Some(300), Some("KD-2")),
            line(4, None, Some("KD-2")),
            line(5, Some(50), Some("KD-1")),
        ];
        let credit = Adjustment {
            id: Uuid::nil(),
            amount: eur(-20),
            tags: [("ticket".to_owned(), "KD-1".to_owned())].into(),
            reason: "credit".to_owned(),
            late: false,
            token_entry_id: None,
            posted_by: None,
            posted_at: datetime!(2026-10-02 09:00 UTC),
        };
        let actual = Actual {
            value: eur(1_330),
            token_entries: 5,
            unknown_model_entries: 1,
            lines: lines.iter().collect(),
            adjustments: vec![&credit],
        };
        let tickets = per_ticket(&actual, Currency::EUR).unwrap();
        let sum = |ticket: Option<&str>, cents, token_entries, unknown_model_entries| TicketSum {
            ticket: ticket.map(str::to_owned),
            value: eur(cents),
            token_entries,
            unknown_model_entries,
        };
        assert_eq!(
            tickets,
            [
                sum(Some("KD-2"), 300, 2, 1),
                sum(Some("KD-1"), 130, 2, 0),
                sum(None, 900, 1, 0)
            ]
        );
        let total = Money::sum(tickets.iter().map(|sum| sum.value), Currency::EUR).unwrap();
        assert_eq!(total, actual.value);
    }

    #[test]
    fn the_csv_names_the_outcome() {
        let report = TokenBudgetReport {
            period: "2026-09".to_owned(),
            currency: "EUR".to_owned(),
            status: StatusView::All,
            rows: vec![TokenBudgetRow {
                token_budget: TokenBudgetView {
                    id: Uuid::nil(),
                    name: None,
                    filter: Filter::parse("ticket:KD-214").unwrap(),
                    period: None,
                    every_month: false,
                    applies: "all time".to_owned(),
                    amount: "100.00".to_owned(),
                    currency: "EUR".to_owned(),
                    token_forecast_id: None,
                    set_by: None,
                    set_at: datetime!(2026-09-01 09:00 UTC),
                },
                actual_period: None,
                amount: "100.00".to_owned(),
                actual: "80.00".to_owned(),
                difference: "-20.00".to_owned(),
                outcome: None,
                token_entries: 3,
                unknown_model_entries: 1,
            }],
        };
        assert_eq!(
            to_csv(&report).into_text(),
            "token_budget,name,filter,applies,actual_period,amount,actual,difference,outcome,token_entries,unknown_model_entries,currency\r\n\
             00000000-0000-0000-0000-000000000000,,ticket:KD-214,all time,all time,100.00,80.00,-20.00,unknown model,3,1,EUR\r\n"
        );
    }

    #[test]
    fn the_period_defaults_to_this_month() {
        let now = datetime!(2026-10-12 09:00 UTC);
        assert_eq!(period_or_now(None, now).unwrap().to_string(), "2026-10");
        assert_eq!(
            period_or_now(Some(" "), now).unwrap().to_string(),
            "2026-10"
        );
        assert_eq!(
            period_or_now(Some("2026-09"), now).unwrap().to_string(),
            "2026-09"
        );
        assert!(period_or_now(Some("September"), now).is_err());
        assert!(wants_csv(Some("csv")));
        assert!(!wants_csv(Some("json")));
        assert!(!wants_csv(None));
    }
}
