//! Cost reports: a tag filter with a grouping (docs: Report > Reading
//! reports, Agent harness reports, Reporting API). Any tag works as the
//! grouping, and so do the agent harness, agent name, model, effort and
//! speed of the token entries, the cost kind and the day. A report can be
//! shown in another currency, or as shares of the period. Lines within one
//! report add up; reports filtered by different tags overlap.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use tokencontroller_core::{Currency, ExchangeRate, Money, MoneyError, TagName, group_by, shares};
use utoipa::{IntoParams, ToSchema};

use super::READERS;
use super::csv::Csv;
use super::filter::{Filter, Tags};
use super::lines::StatusView;
use super::scope::Scope;
use crate::AppState;
use crate::bills::amount::format_amount;
use crate::bills::rates;
use crate::close::account::{self, Account};
use crate::close::period::{Period, PeriodDates};
use crate::error::ApiError;
use crate::sign_in::Caller;

/// What a report groups by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Grouping {
    /// The value of one tag name, such as `team`.
    Tag(String),
    AgentHarness,
    AgentName,
    Model,
    Effort,
    Speed,
    /// The day the token entry started, in UTC.
    Day,
    /// Direct, project overhead or organization overhead.
    CostKind,
}

impl Grouping {
    /// Reads a grouping: one of the built-in cuts, or else a tag name.
    #[must_use]
    pub fn parse(by: &str) -> Self {
        match by.trim() {
            "agent_harness" => Self::AgentHarness,
            "agent_name" => Self::AgentName,
            "model" => Self::Model,
            "effort" => Self::Effort,
            "speed" => Self::Speed,
            "day" => Self::Day,
            "cost_kind" => Self::CostKind,
            tag => Self::Tag(tag.to_owned()),
        }
    }

    /// The grouping's name, as `by=` takes it.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Tag(tag) => tag,
            Self::AgentHarness => "agent_harness",
            Self::AgentName => "agent_name",
            Self::Model => "model",
            Self::Effort => "effort",
            Self::Speed => "speed",
            Self::Day => "day",
            Self::CostKind => "cost_kind",
        }
    }

    /// Whether this is one of the agent harness report's cuts.
    #[must_use]
    pub const fn is_agent_cut(&self) -> bool {
        matches!(
            self,
            Self::AgentHarness | Self::AgentName | Self::Model | Self::Effort | Self::Speed
        )
    }

    fn key(&self, line: &account::Line, account: &Account) -> Option<String> {
        let entry = &line.line;
        match self {
            Self::Tag(tag) => entry.tags.get(tag).cloned(),
            Self::AgentHarness => Some(entry.agent_harness.clone()),
            Self::AgentName => entry.agent_name.clone(),
            Self::Model => Some(entry.model.clone()),
            Self::Effort => entry.effort.clone(),
            Self::Speed => entry.speed.clone(),
            Self::Day => Some(entry.started_at.date().to_string()),
            Self::CostKind => Some(
                crate::close::cost_sheet::kind_name(account.tag_map.cost_kind(&entry.core_tags()))
                    .to_owned(),
            ),
        }
    }

    fn adjustment_key(&self, tags: &Tags) -> Option<String> {
        match self {
            Self::Tag(tag) => tags.get(tag).cloned(),
            _ => None,
        }
    }
}

/// Money or shares of the period.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum View {
    #[default]
    Money,
    /// Each line's share of the report's total, for seat plans where usage
    /// is not charged in money.
    Share,
}

/// One line of a report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ReportRow {
    /// The group, such as `web`; `null` for token entries without it.
    pub value: Option<String>,
    #[schema(example = "41.87")]
    pub amount: String,
    /// The line's share of the period's usage in basis points, `4120` for
    /// 41.20 percent, in the share view. Adjustments have none.
    pub share: Option<u64>,
    pub token_entries: usize,
    /// Token entries on a model missing from the price table.
    pub unknown_model: usize,
    /// An adjustment posted to the closed period, shown as its own line.
    pub adjustment: bool,
}

/// A report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct Report {
    #[schema(value_type = String, example = "2026-09")]
    pub period: Period,
    pub closed: bool,
    pub by: String,
    pub filter: Filter,
    pub status: StatusView,
    pub view: View,
    #[schema(value_type = String, example = "EUR")]
    pub currency: Currency,
    /// The rate from the reporting currency, when shown in another one.
    pub exchange_rate: Option<String>,
    /// `list-price estimate`, or `share of the period`.
    pub label: &'static str,
    pub rows: Vec<ReportRow>,
    pub total: String,
}

/// What a report shows.
#[derive(Debug, Clone, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ReportQuery {
    /// The period, such as `2026-09`.
    #[param(value_type = String, example = "2026-09")]
    pub period: Period,
    /// A tag name, or `agent_harness`, `agent_name`, `model`, `effort`,
    /// `speed`, `day` or `cost_kind`.
    #[param(example = "team")]
    pub by: Option<String>,
    /// A tag filter, such as `client:acme,team:web`.
    #[param(example = "client:acme")]
    pub filter: Option<String>,
    /// `all` (the default) or `approved`.
    pub status: Option<StatusView>,
    /// Show the report in another currency, such as `USD`.
    pub currency: Option<String>,
    /// `money` (the default) or `share`.
    pub view: Option<View>,
    /// `csv` for a CSV file.
    pub format: Option<String>,
}

/// One computed line of a report, in the reporting currency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Computed {
    pub value: Option<String>,
    pub amount: Money,
    pub token_entries: usize,
    pub unknown_model: usize,
    pub adjustment: bool,
}

#[derive(Default)]
struct Bucket {
    nanos: i64,
    entries: usize,
    unknown: usize,
}

/// Computes a report of an account already limited to the filter and the
/// caller's scope.
///
/// # Errors
///
/// [`MoneyError`] on an overflow.
pub fn compute(
    account: &Account,
    grouping: &Grouping,
    filter: &Filter,
    scope: &Scope,
) -> Result<(Vec<Computed>, Money), MoneyError> {
    let currency = account.currency;
    let mut groups: BTreeMap<Option<String>, Bucket> = BTreeMap::new();
    for line in &account.lines {
        let bucket = groups.entry(grouping.key(line, account)).or_default();
        bucket.entries = bucket.entries.saturating_add(1);
        match line.value {
            Some(value) => {
                bucket.nanos = bucket
                    .nanos
                    .checked_add(value.nanos())
                    .ok_or(MoneyError::Overflow)?;
            }
            None => bucket.unknown = bucket.unknown.saturating_add(1),
        }
    }
    // A tag grouping is the core's group_by.
    if let Grouping::Tag(name) = grouping
        && let Ok(tag) = TagName::new(name)
    {
        let items: Vec<(tokencontroller_core::Tags, Money)> = account
            .lines
            .iter()
            .filter_map(|line| Some((line.line.core_tags(), line.value?)))
            .collect();
        let everything = tokencontroller_core::Filter::default();
        for (value, amount) in group_by(
            items.iter().map(|(tags, amount)| (tags, *amount)),
            &everything,
            &tag,
            currency,
        )? {
            groups
                .entry(value.map(|value| value.to_string()))
                .or_default()
                .nanos = amount.nanos();
        }
    }
    let mut adjustments: BTreeMap<Option<String>, i64> = BTreeMap::new();
    for adjustment in &account.adjustments {
        let visible = scope.is_everything()
            || scope
                .filters()
                .iter()
                .any(|mine| mine.matches(&adjustment.tags));
        if visible && filter.matches(&adjustment.tags) {
            let slot = adjustments
                .entry(grouping.adjustment_key(&adjustment.tags))
                .or_insert(0);
            *slot = slot
                .checked_add(adjustment.amount.nanos())
                .ok_or(MoneyError::Overflow)?;
        }
    }
    let mut rows: Vec<Computed> = groups
        .into_iter()
        .map(|(value, bucket)| Computed {
            value,
            amount: Money::from_nanos(bucket.nanos, currency),
            token_entries: bucket.entries,
            unknown_model: bucket.unknown,
            adjustment: false,
        })
        .collect();
    rows.sort_by(|a, b| {
        a.value
            .is_none()
            .cmp(&b.value.is_none())
            .then_with(|| b.amount.nanos().cmp(&a.amount.nanos()))
            .then_with(|| a.value.cmp(&b.value))
    });
    rows.extend(adjustments.into_iter().map(|(value, nanos)| Computed {
        value,
        amount: Money::from_nanos(nanos, currency),
        token_entries: 0,
        unknown_model: 0,
        adjustment: true,
    }));
    let total = Money::sum(rows.iter().map(|row| row.amount), currency)?;
    Ok((rows, total))
}

/// A report to compute: what [`ReportQuery`] says, read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    pub period: Period,
    pub grouping: Grouping,
    pub filter: Filter,
    pub status: StatusView,
    /// Another currency to show the report in.
    pub currency: Option<String>,
    pub view: View,
}

impl Spec {
    /// Reads a report query, grouping by `default_by` when it names none.
    ///
    /// # Errors
    ///
    /// [`ApiError::BadRequest`] for a filter that cannot be read.
    pub fn read(query: &ReportQuery, default_by: &str) -> Result<Self, ApiError> {
        Ok(Self {
            period: query.period,
            grouping: Grouping::parse(query.by.as_deref().unwrap_or(default_by)),
            filter: Filter::parse(query.filter.as_deref().unwrap_or(""))
                .map_err(|error| ApiError::BadRequest(error.to_string()))?,
            status: query.status.unwrap_or_default(),
            currency: query.currency.clone(),
            view: query.view.unwrap_or_default(),
        })
    }
}

/// Runs a report for a caller.
///
/// # Errors
///
/// [`ApiError::BadRequest`] for a currency that cannot be read,
/// [`ApiError::Conflict`] when an exchange rate is missing, or a database
/// error.
pub async fn run(db: &PgPool, caller: &Caller, spec: Spec) -> Result<Report, ApiError> {
    let Spec {
        period,
        grouping,
        filter,
        status,
        currency,
        view,
    } = spec;
    let scope = Scope::of(db, caller).await?;
    let account =
        account::load(db, caller.organization_id, period, status, &scope, &filter).await?;
    let (rows, total) = compute(&account, &grouping, &filter, &scope)
        .map_err(|error| ApiError::Internal(error.into()))?;

    let rate = shown_in(db, account.currency, currency.as_deref(), period).await?;
    let target = rate.to();
    let convert = |amount: Money| {
        rate.convert(amount)
            .map_err(|error| ApiError::Internal(error.into()))
    };
    let amounts = rows
        .iter()
        .map(|row| convert(row.amount))
        .collect::<Result<Vec<_>, _>>()?;
    let shares = if view == View::Share {
        share_points(&rows, &amounts)
    } else {
        vec![None; amounts.len()]
    };
    let total = if target == account.currency {
        total
    } else {
        Money::sum(amounts.iter().copied(), target)
            .map_err(|error| ApiError::Internal(error.into()))?
    };
    Ok(Report {
        period,
        closed: account.closed.is_some(),
        by: grouping.name().to_owned(),
        filter,
        status,
        view,
        currency: target,
        exchange_rate: rate_shown(rate),
        label: if view == View::Share {
            "share of the period"
        } else {
            "list-price estimate"
        },
        rows: rows
            .into_iter()
            .zip(amounts)
            .zip(shares)
            .map(|((row, amount), share)| ReportRow {
                value: row.value,
                amount: format_amount(amount),
                share,
                token_entries: row.token_entries,
                unknown_model: row.unknown_model,
                adjustment: row.adjustment,
            })
            .collect(),
        total: format_amount(total),
    })
}

/// The rate a report of `period` is shown at: from the reporting currency
/// into `currency`, at the rate of the period's last day (today for a
/// running period). No currency asked for: the reporting currency itself.
///
/// # Errors
///
/// [`ApiError::BadRequest`] for a currency that cannot be read,
/// [`ApiError::Conflict`] when the rate is missing, or a database error.
pub async fn shown_in(
    db: &PgPool,
    reporting: Currency,
    currency: Option<&str>,
    period: Period,
) -> Result<ExchangeRate, ApiError> {
    let target = match currency.map(str::trim).filter(|code| !code.is_empty()) {
        Some(code) => {
            Currency::new(code).map_err(|error| ApiError::BadRequest(error.to_string()))?
        }
        None => reporting,
    };
    let day = period.last_day().min(OffsetDateTime::now_utc().date());
    rates::between(db, reporting, target, day)
        .await?
        .map_err(|error| ApiError::Conflict(error.to_string()))
}

/// The exchange rate a report shows, when it is in another currency than
/// the reporting currency.
#[must_use]
pub fn rate_shown(rate: ExchangeRate) -> Option<String> {
    (rate.from() != rate.to()).then(|| crate::bills::amount::rate_number(rate).to_string())
}

/// Each usage line's share of the period in basis points (the core's
/// [`shares`]); adjustments get none. No usage, or a negative line, gives
/// no shares.
fn share_points(rows: &[Computed], amounts: &[Money]) -> Vec<Option<u64>> {
    let usage: Vec<Money> = rows
        .iter()
        .zip(amounts)
        .filter(|(row, _)| !row.adjustment)
        .map(|(_, amount)| *amount)
        .collect();
    let mut points = shares(&usage)
        .map_or_else(
            |_| vec![None; usage.len()],
            |points| points.into_iter().map(Some).collect(),
        )
        .into_iter();
    rows.iter()
        .map(|row| {
            if row.adjustment {
                None
            } else {
                points.next().flatten()
            }
        })
        .collect()
}

/// Basis points as a percentage with two decimals, such as `41.20`.
#[must_use]
pub fn percent_text(points: u64) -> String {
    format!("{}.{:02}", points.div_euclid(100), points.rem_euclid(100))
}

/// A report as CSV.
#[must_use]
pub fn to_csv(report: &Report) -> Csv {
    let mut csv = Csv::new([
        "period",
        report.by.as_str(),
        "amount",
        "currency",
        "share_percent",
        "token_entries",
        "unknown_model",
        "adjustment",
    ]);
    let period = report.period.to_string();
    for row in &report.rows {
        csv.row([
            period.clone(),
            row.value.clone().unwrap_or_default(),
            row.amount.clone(),
            report.currency.code(),
            row.share.map(percent_text).unwrap_or_default(),
            row.token_entries.to_string(),
            row.unknown_model.to_string(),
            if row.adjustment { "yes" } else { "no" }.to_owned(),
        ]);
    }
    csv.row([
        period,
        "total".to_owned(),
        report.total.clone(),
        report.currency.code(),
        String::new(),
        String::new(),
        String::new(),
        String::new(),
    ]);
    csv
}

/// Answers with a report as JSON or as CSV.
#[must_use]
pub fn respond(report: Report, csv: bool, name: &str) -> Response {
    if csv {
        let filename = format!("{name}-{}-by-{}.csv", report.period, report.by);
        to_csv(&report).into_download(&filename)
    } else {
        Json(report).into_response()
    }
}

/// Cost grouped by any tag, within a filter: the reporting API's main
/// report.
#[utoipa::path(
    get,
    path = "/reports/cost",
    tag = "reports",
    params(ReportQuery),
    responses(
        (status = OK, description = "The report, as JSON or CSV", content((Report = "application/json"), (String = "text/csv"))),
        (status = BAD_REQUEST, description = "The filter, grouping or currency cannot be read", body = crate::error::ErrorBody),
        (status = CONFLICT, description = "An exchange rate is missing", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn cost_report(
    State(state): State<AppState>,
    caller: Caller,
    Query(query): Query<ReportQuery>,
) -> Result<Response, ApiError> {
    caller.require_credential(READERS)?;
    let report = run(&state.db, &caller, Spec::read(&query, "project")?).await?;
    Ok(respond(
        report,
        query.format.as_deref() == Some("csv"),
        "cost",
    ))
}

/// Cost by agent harness, agent name, model, effort or speed.
#[utoipa::path(
    get,
    path = "/reports/agent-harness",
    tag = "reports",
    params(ReportQuery),
    responses(
        (status = OK, description = "The report, as JSON or CSV", content((Report = "application/json"), (String = "text/csv"))),
        (status = BAD_REQUEST, description = "`by` is not one of the agent harness cuts", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn agent_harness_report(
    State(state): State<AppState>,
    caller: Caller,
    Query(query): Query<ReportQuery>,
) -> Result<Response, ApiError> {
    caller.require_credential(READERS)?;
    let spec = Spec::read(&query, "agent_harness")?;
    if !spec.grouping.is_agent_cut() {
        return Err(ApiError::BadRequest(
            "group by agent_harness, agent_name, model, effort or speed".to_owned(),
        ));
    }
    let report = run(&state.db, &caller, spec).await?;
    Ok(respond(
        report,
        query.format.as_deref() == Some("csv"),
        "agent-harness",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groupings_round_trip_and_tags_are_the_rest() {
        for by in [
            "agent_harness",
            "agent_name",
            "model",
            "effort",
            "speed",
            "day",
            "cost_kind",
        ] {
            assert_eq!(Grouping::parse(by).name(), by);
            assert!(!matches!(Grouping::parse(by), Grouping::Tag(_)));
        }
        assert_eq!(Grouping::parse(" team "), Grouping::Tag("team".to_owned()));
        assert!(Grouping::parse("model").is_agent_cut());
        assert!(!Grouping::parse("day").is_agent_cut());
        assert!(!Grouping::parse("team").is_agent_cut());
    }
}
