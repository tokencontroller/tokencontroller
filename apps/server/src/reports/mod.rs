//! Reports (docs: Reference > API > Reports, Report > Deviation): `C(F, w)`
//! grouped by any tag or attribute name, plan against cost with its
//! deviation, the forecast for the next range, and saved reports. Every
//! report reads the log as of a moment, answers inside the caller's view
//! grants and is computed by the core; this area only cuts, groups,
//! converts and formats.
//!
//! Owner: the reports work. Keep this area's code and routes in this
//! folder.

pub mod csv;

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde::{Deserialize, Serialize};
use time::{Date, OffsetDateTime};
use tokencontroller_core::state::Line;
use tokencontroller_core::{Currency, DateRange, Filter, Money, State as Fold, TagName, TagValue};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::AppState;
use crate::error::ApiError;
use crate::events::{self, Log, access, json, prefix};
use crate::messages::{Submitted, submitted};
use crate::sign_in::{Caller, CredentialKind, Member};

/// This area's routes, relative to `/api/v1`.
#[must_use]
pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(cost_report))
        .routes(routes!(deviation_report))
        .routes(routes!(forecast_report))
        .routes(routes!(list_saved, save_report))
}

/// How many past windows a forecast averages.
const FORECAST_WINDOWS: u64 = 3;

/// What every report takes.
#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ReportQuery {
    /// The first day, `YYYY-MM-DD`, in UTC.
    #[param(example = "2026-03-01")]
    pub from: Option<String>,
    /// The last day, `YYYY-MM-DD`, in UTC.
    #[param(example = "2026-03-31")]
    pub to: Option<String>,
    /// The moment up to which the log is read; default now.
    #[param(example = "2026-04-01T00:00:00Z")]
    pub as_of: Option<String>,
    /// A tag or attribute name, or `person`, `label`, `agent_harness`,
    /// `agent_name`, `model` or `day`.
    #[param(example = "project")]
    pub by: Option<String>,
    /// A filter, such as `client:acme,team:web`.
    #[param(example = "client:acme")]
    pub filter: Option<String>,
    /// `true` shows approved messages only.
    pub approved: Option<bool>,
    /// Show money in another currency, such as `EUR`.
    #[param(example = "EUR")]
    pub currency: Option<String>,
    /// `money` (the default) or `share`.
    #[param(example = "money")]
    pub view: Option<String>,
    /// `csv` for a CSV file instead of JSON.
    #[param(example = "csv")]
    pub format: Option<String>,
}

/// One row of the cost report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct CostRow {
    /// The value of the grouping, or `null` for lines without it.
    pub key: Option<String>,
    #[schema(example = "700.00")]
    pub amount: String,
    /// The row's share of the total, a decimal fraction.
    #[schema(example = "0.736842")]
    pub share: String,
    pub messages: usize,
}

/// Rows of a cost report in the making: the amount and the messages of
/// each group.
type Groups = BTreeMap<Option<String>, (Money, std::collections::BTreeSet<(String, String)>)>;

/// The cost report: `C(F, w)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct CostReport {
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub as_of: OffsetDateTime,
    #[serde(with = "crate::dates::day")]
    #[schema(value_type = String, format = Date)]
    pub from: Date,
    #[serde(with = "crate::dates::day")]
    #[schema(value_type = String, format = Date)]
    pub to: Date,
    pub filter: BTreeMap<String, String>,
    pub by: Option<String>,
    #[schema(example = "USD")]
    pub currency: String,
    /// `money` or `share`.
    pub view: String,
    #[schema(example = "950.00")]
    pub total: String,
    pub rows: Vec<CostRow>,
    /// Messages in the filter and range with no counts; their cost is not
    /// in the total.
    pub count_errors: usize,
    /// Models in the filter and range with no price row.
    pub unknown_models: Vec<String>,
}

/// One row of the deviation report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct DeviationRow {
    pub key: Option<String>,
    pub filter: BTreeMap<String, String>,
    /// `P(F, w)`, or `null` when nothing is planned.
    pub plan: Option<String>,
    pub cost: String,
    /// Plan minus cost; positive is favourable.
    pub deviation: Option<String>,
    pub favourable: Option<bool>,
    /// `scope`, `trouble` or `estimate_itself`, from the classifier.
    pub label: Option<String>,
}

/// The deviation report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct DeviationReport {
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub as_of: OffsetDateTime,
    #[serde(with = "crate::dates::day")]
    #[schema(value_type = String, format = Date)]
    pub from: Date,
    #[serde(with = "crate::dates::day")]
    #[schema(value_type = String, format = Date)]
    pub to: Date,
    pub filter: BTreeMap<String, String>,
    pub by: Option<String>,
    pub currency: String,
    pub rows: Vec<DeviationRow>,
}

/// One past window of a forecast.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PastWindow {
    #[serde(with = "crate::dates::day")]
    #[schema(value_type = String, format = Date)]
    pub from: Date,
    #[serde(with = "crate::dates::day")]
    #[schema(value_type = String, format = Date)]
    pub to: Date,
    pub cost: String,
}

/// One row of the forecast report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ForecastRow {
    pub key: Option<String>,
    pub filter: BTreeMap<String, String>,
    pub forecast: String,
    pub past: Vec<PastWindow>,
}

/// The forecast report: the cost per day over the past windows times the
/// days of the next range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ForecastReport {
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub as_of: OffsetDateTime,
    #[serde(with = "crate::dates::day")]
    #[schema(value_type = String, format = Date)]
    pub from: Date,
    #[serde(with = "crate::dates::day")]
    #[schema(value_type = String, format = Date)]
    pub to: Date,
    pub filter: BTreeMap<String, String>,
    pub by: Option<String>,
    pub currency: String,
    pub rows: Vec<ForecastRow>,
}

/// A saved report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct SavedReport {
    pub id: String,
    pub name: String,
    pub filter: BTreeMap<String, String>,
    pub by: Option<String>,
    #[serde(with = "crate::dates::option_day")]
    #[schema(value_type = Option<String>, format = Date)]
    pub from: Option<Date>,
    #[serde(with = "crate::dates::option_day")]
    #[schema(value_type = Option<String>, format = Date)]
    pub to: Option<Date>,
    pub created_by: String,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub created_at: OffsetDateTime,
}

/// A report to save.
#[derive(Debug, Deserialize, ToSchema)]
pub struct NewSavedReport {
    pub name: String,
    #[serde(default)]
    pub filter: BTreeMap<String, String>,
    pub by: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
}

/// What a report groups by.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Grouping {
    Tag(TagName),
    Person,
    Label,
    AgentHarness,
    AgentName,
    Model,
    Day,
}

impl Grouping {
    fn parse(by: Option<&str>) -> Result<Option<Self>, ApiError> {
        let Some(by) = by.map(str::trim).filter(|by| !by.is_empty()) else {
            return Ok(None);
        };
        Ok(Some(match by {
            "person" => Self::Person,
            "label" => Self::Label,
            "agent_harness" => Self::AgentHarness,
            "agent_name" => Self::AgentName,
            "model" => Self::Model,
            "day" => Self::Day,
            name => Self::Tag(json::tag_name(name)?),
        }))
    }

    fn word(&self) -> String {
        match self {
            Self::Tag(name) => name.to_string(),
            Self::Person => "person".to_owned(),
            Self::Label => "label".to_owned(),
            Self::AgentHarness => "agent_harness".to_owned(),
            Self::AgentName => "agent_name".to_owned(),
            Self::Model => "model".to_owned(),
            Self::Day => "day".to_owned(),
        }
    }
}

/// The agent harness and name of every message, for grouping.
fn agents(fold: &Fold) -> BTreeMap<(String, String), (String, Option<String>)> {
    fold.messages()
        .map(|recorded| {
            (
                (
                    recorded.value.session_id.to_string(),
                    recorded.value.message_id.to_string(),
                ),
                (
                    recorded.value.agent_harness.clone(),
                    recorded.value.agent_name.clone(),
                ),
            )
        })
        .collect()
}

fn key_of(
    line: &Line,
    grouping: &Grouping,
    agents: &BTreeMap<(String, String), (String, Option<String>)>,
) -> Option<String> {
    match grouping {
        Grouping::Tag(name) => line.tags.get(name).map(|value| value.as_str().to_owned()),
        Grouping::Person => Some(line.person.as_str().to_owned()),
        Grouping::Label => Some(line.label.as_str().to_owned()),
        Grouping::AgentHarness => agents
            .get(&(line.session_id.to_string(), line.message_id.to_string()))
            .map(|(harness, _)| harness.clone()),
        Grouping::AgentName => agents
            .get(&(line.session_id.to_string(), line.message_id.to_string()))
            .and_then(|(_, name)| name.clone()),
        Grouping::Model => Some(line.model.to_string()),
        Grouping::Day => Some(crate::dates::text(
            line.at.to_offset(time::UtcOffset::UTC).date(),
        )),
    }
}

/// The lines of the fold the caller may view, in the filter and range.
fn visible<'a>(
    fold: &'a Fold,
    person: &TagValue,
    filter: &'a Filter,
    window: DateRange,
    approved: bool,
) -> Vec<&'a Line> {
    fold.select(filter, window)
        .filter(|line| access::can_view(fold, person, &line.tags))
        .filter(|line| !approved || submitted(fold, &line.person, line.at) == Submitted::Approved)
        .collect()
}

/// Everything a report call shares: the fold, the caller, the range and
/// how money is shown.
struct Setup {
    fold: Fold,
    person: TagValue,
    as_of: OffsetDateTime,
    window: DateRange,
    filter: Filter,
    grouping: Option<Grouping>,
    currency: Currency,
    rate: tokencontroller_core::ExchangeRate,
    share: bool,
}

impl Setup {
    async fn new(state: &AppState, caller: &Caller, query: &ReportQuery) -> Result<Self, ApiError> {
        let person = caller.person()?;
        let as_of = crate::dates::as_of(query.as_of.as_deref())?;
        let window = crate::dates::range(query.from.as_deref(), query.to.as_deref())?;
        let filter = json::filter(query.filter.as_deref())?;
        let grouping = Grouping::parse(query.by.as_deref())?;
        let log = Log::load(state, caller.organization_id).await?;
        let fold = log.fold(as_of)?;
        let currency = match query.currency.as_deref().map(str::trim) {
            Some(code) if !code.is_empty() => Currency::new(&code.to_ascii_uppercase())
                .map_err(|_| ApiError::BadRequest(format!("{code:?} is not a currency code")))?,
            _ => {
                crate::organizations::reporting_currency(&state.db, caller.organization_id).await?
            }
        };
        let rate = crate::bills::rates::between(&state.db, fold.currency(), currency, window.to)
            .await?
            .map_err(|error| ApiError::Conflict(error.to_string()))?;
        let share = match query.view.as_deref().map(str::trim) {
            None | Some("" | "money") => false,
            Some("share") => true,
            Some(other) => {
                return Err(ApiError::BadRequest(format!(
                    "{other:?} is not a view; use money or share"
                )));
            }
        };
        Ok(Self {
            fold,
            person,
            as_of,
            window,
            filter,
            grouping,
            currency,
            rate,
            share,
        })
    }

    /// An amount in the report's currency, as text.
    fn money(&self, amount: Money) -> Result<String, ApiError> {
        let converted = self
            .rate
            .convert(amount)
            .map_err(|error| ApiError::Internal(error.into()))?;
        Ok(json::money(converted))
    }

    /// The filter of one row: the report's filter plus the row's value.
    fn row_filter(&self, key: Option<&str>) -> Result<Filter, ApiError> {
        let mut tags = self.filter.0.clone();
        if let (Some(grouping), Some(key)) = (&self.grouping, key) {
            match grouping {
                Grouping::Tag(name) => {
                    tags.insert(name.clone(), json::tag_value(name.as_str(), key)?);
                }
                Grouping::Person => {
                    tags.insert(TagName::person(), json::tag_value("person", key)?);
                }
                _ => {}
            }
        }
        Ok(Filter(tags))
    }

    /// The cost of the visible lines per value of the grouping.
    fn costs_by_key(&self, approved: bool) -> Result<BTreeMap<Option<String>, Money>, ApiError> {
        let agents = agents(&self.fold);
        let mut costs: BTreeMap<Option<String>, Money> = BTreeMap::new();
        for line in visible(
            &self.fold,
            &self.person,
            &self.filter,
            self.window,
            approved,
        ) {
            let key = self
                .grouping
                .as_ref()
                .and_then(|grouping| key_of(line, grouping, &agents));
            let sum = costs
                .entry(key)
                .or_insert_with(|| Money::zero(self.fold.currency()));
            *sum = sum
                .checked_add(line.amount)
                .map_err(|error| ApiError::Internal(error.into()))?;
        }
        Ok(costs)
    }

    /// The values of the grouping among the visible lines, in order.
    fn keys(&self, approved: bool) -> Vec<Option<String>> {
        let Some(grouping) = &self.grouping else {
            return vec![None];
        };
        let agents = agents(&self.fold);
        let mut keys: Vec<Option<String>> = Vec::new();
        for line in visible(
            &self.fold,
            &self.person,
            &self.filter,
            self.window,
            approved,
        ) {
            let key = key_of(line, grouping, &agents);
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        keys.sort();
        keys
    }
}

/// A share of a total as a decimal fraction with six places.
fn share_text(part: i64, total: i64) -> String {
    if total <= 0 {
        return "0".to_owned();
    }
    let part = u64::try_from(part).unwrap_or(0);
    let total = u64::try_from(total).unwrap_or(1);
    crate::splits::fraction_text(part, total)
}

/// `C(F, w)`: the cost of a filter over a range, grouped by any tag or
/// attribute name. Answers inside the caller's view grants.
#[utoipa::path(
    get,
    path = "/reports/cost",
    tag = "reports",
    params(ReportQuery),
    responses(
        (status = OK, description = "The report, as JSON or CSV", content((CostReport = "application/json"), (String = "text/csv"))),
        (status = BAD_REQUEST, body = crate::error::ErrorBody),
    ),
)]
async fn cost_report(
    State(state): State<AppState>,
    member: Member,
    Query(query): Query<ReportQuery>,
) -> Result<Response, ApiError> {
    let report = cost(&state, &member, &query).await?;
    table(
        &report.rows,
        &[
            ("key", "/key"),
            ("amount", "/amount"),
            ("share", "/share"),
            ("messages", "/messages"),
        ],
        &report,
        query.format.as_deref(),
        "cost.csv",
    )
}

/// The cost report for a caller, as the endpoint and the MCP server
/// compute it.
///
/// # Errors
///
/// [`ApiError::BadRequest`] for a query that cannot be read, or whatever
/// loading the log fails with.
pub async fn cost(
    state: &AppState,
    caller: &Caller,
    query: &ReportQuery,
) -> Result<CostReport, ApiError> {
    let setup = Setup::new(state, caller, query).await?;
    let approved = query.approved == Some(true);
    let lines = visible(
        &setup.fold,
        &setup.person,
        &setup.filter,
        setup.window,
        approved,
    );
    let agents = agents(&setup.fold);
    let mut groups: Groups = BTreeMap::new();
    let mut total = Money::zero(setup.fold.currency());
    for line in &lines {
        let key = setup
            .grouping
            .as_ref()
            .and_then(|grouping| key_of(line, grouping, &agents));
        let entry = groups.entry(key).or_insert_with(|| {
            (
                Money::zero(setup.fold.currency()),
                std::collections::BTreeSet::new(),
            )
        });
        entry.0 = entry
            .0
            .checked_add(line.amount)
            .map_err(|error| ApiError::Internal(error.into()))?;
        entry
            .1
            .insert((line.session_id.to_string(), line.message_id.to_string()));
        total = total
            .checked_add(line.amount)
            .map_err(|error| ApiError::Internal(error.into()))?;
    }
    let cost = setup
        .fold
        .cost(&setup.filter, setup.window)
        .map_err(|error| ApiError::Internal(error.into()))?;
    let mut rows = Vec::with_capacity(groups.len());
    for (key, (amount, messages)) in &groups {
        rows.push(CostRow {
            key: key.clone(),
            amount: if setup.share {
                share_text(amount.nanos(), total.nanos())
            } else {
                setup.money(*amount)?
            },
            share: share_text(amount.nanos(), total.nanos()),
            messages: messages.len(),
        });
    }
    Ok(CostReport {
        as_of: setup.as_of,
        from: setup.window.from,
        to: setup.window.to,
        filter: json::tags_map(&setup.filter.0),
        by: setup.grouping.as_ref().map(Grouping::word),
        currency: setup.currency.code(),
        view: if setup.share { "share" } else { "money" }.to_owned(),
        total: setup.money(total)?,
        rows,
        count_errors: cost.count_errors.len(),
        unknown_models: cost
            .unknown_models
            .iter()
            .map(ToString::to_string)
            .collect(),
    })
}

/// Plan, cost and deviation per value of the grouping, with labels from
/// the classifier where one is set up. Groupings other than a tag or
/// attribute name or `person` have no plan.
#[utoipa::path(
    get,
    path = "/reports/deviation",
    tag = "reports",
    params(ReportQuery),
    responses(
        (status = OK, description = "The report, as JSON or CSV", content((DeviationReport = "application/json"), (String = "text/csv"))),
        (status = BAD_REQUEST, body = crate::error::ErrorBody),
    ),
)]
async fn deviation_report(
    State(state): State<AppState>,
    member: Member,
    Query(query): Query<ReportQuery>,
) -> Result<Response, ApiError> {
    let setup = Setup::new(&state, &member, &query).await?;
    let approved = query.approved == Some(true);
    let classifier = crate::plans::Classifier::from_config(&state.config);
    // The cost of each row is the cost of its own lines, the same
    // partition the cost report uses: the row without a value holds the
    // lines that have none, not the whole filter.
    let costs = setup.costs_by_key(approved)?;
    let mut rows = Vec::new();
    for key in setup.keys(approved) {
        let filter = setup.row_filter(key.as_deref())?;
        let cost = costs
            .get(&key)
            .copied()
            .unwrap_or_else(|| Money::zero(setup.fold.currency()));
        // Lines without a value of the grouping have no plan of their own.
        let deviation = if setup.grouping.is_some() && key.is_none() {
            None
        } else {
            setup
                .fold
                .deviation(&filter, setup.window)
                .map_err(|error| ApiError::Internal(error.into()))?
        };
        let mut row = DeviationRow {
            key: key.clone(),
            filter: json::tags_map(&filter.0),
            plan: None,
            cost: setup.money(cost)?,
            deviation: None,
            favourable: None,
            label: None,
        };
        if let Some(deviation) = deviation {
            row.plan = Some(setup.money(deviation.plan)?);
            row.deviation = Some(setup.money(deviation.delta)?);
            row.favourable = Some(deviation.is_favourable());
            if let (Some(classifier), Some(accepted)) =
                (&classifier, setup.fold.accepted(&filter.0))
                && !deviation.delta.is_zero()
            {
                match classifier
                    .label_deviation(
                        &accepted.value,
                        &json::money(cost),
                        &json::money(deviation.delta),
                    )
                    .await
                {
                    Ok(label) => row.label = Some(label.as_str().to_owned()),
                    Err(error) => tracing::warn!("deviation label: {error}"),
                }
            }
        }
        rows.push(row);
    }
    let report = DeviationReport {
        as_of: setup.as_of,
        from: setup.window.from,
        to: setup.window.to,
        filter: json::tags_map(&setup.filter.0),
        by: setup.grouping.as_ref().map(Grouping::word),
        currency: setup.currency.code(),
        rows,
    };
    table(
        &report.rows,
        &[
            ("key", "/key"),
            ("plan", "/plan"),
            ("cost", "/cost"),
            ("deviation", "/deviation"),
            ("favourable", "/favourable"),
            ("label", "/label"),
        ],
        &report,
        query.format.as_deref(),
        "deviation.csv",
    )
}

/// The past windows a forecast averages: three of the next range's length,
/// ending the day before it.
fn past_windows(next: DateRange) -> Vec<DateRange> {
    let days = i64::try_from(next.days()).unwrap_or(i64::MAX);
    let mut windows = Vec::new();
    let mut to = next.from.previous_day();
    for _ in 0..FORECAST_WINDOWS {
        let Some(end) = to else {
            break;
        };
        let Some(start) = end.checked_sub(time::Duration::days(days.saturating_sub(1))) else {
            break;
        };
        if let Ok(window) = DateRange::new(start, end) {
            windows.push(window);
        }
        to = start.previous_day();
    }
    windows.reverse();
    windows
}

/// The forecast of the filter for the next range: `from` and `to` name the
/// range ahead; the cost per day over the three ranges of that length
/// before it, times its days.
#[utoipa::path(
    get,
    path = "/reports/forecast",
    tag = "reports",
    params(ReportQuery),
    responses(
        (status = OK, description = "The report, as JSON or CSV", content((ForecastReport = "application/json"), (String = "text/csv"))),
        (status = BAD_REQUEST, body = crate::error::ErrorBody),
    ),
)]
async fn forecast_report(
    State(state): State<AppState>,
    member: Member,
    Query(query): Query<ReportQuery>,
) -> Result<Response, ApiError> {
    let setup = Setup::new(&state, &member, &query).await?;
    let approved = query.approved == Some(true);
    let past = past_windows(setup.window);
    // The keys come from the past windows: the values that had cost.
    let keys = match &setup.grouping {
        None => vec![None],
        Some(grouping) => {
            let agents = agents(&setup.fold);
            let mut keys: Vec<Option<String>> = Vec::new();
            for window in &past {
                for line in visible(&setup.fold, &setup.person, &setup.filter, *window, approved) {
                    let key = key_of(line, grouping, &agents);
                    if !keys.contains(&key) {
                        keys.push(key);
                    }
                }
            }
            keys.sort();
            keys
        }
    };
    let mut rows = Vec::new();
    for key in keys {
        let filter = setup.row_filter(key.as_deref())?;
        let forecast = setup
            .fold
            .forecast(&filter, &past, setup.window)
            .map_err(|error| ApiError::Internal(error.into()))?;
        let mut windows = Vec::with_capacity(past.len());
        for window in &past {
            let cost = setup
                .fold
                .cost(&filter, *window)
                .map_err(|error| ApiError::Internal(error.into()))?;
            windows.push(PastWindow {
                from: window.from,
                to: window.to,
                cost: setup.money(cost.amount)?,
            });
        }
        rows.push(ForecastRow {
            key,
            filter: json::tags_map(&filter.0),
            forecast: setup.money(forecast)?,
            past: windows,
        });
    }
    let report = ForecastReport {
        as_of: setup.as_of,
        from: setup.window.from,
        to: setup.window.to,
        filter: json::tags_map(&setup.filter.0),
        by: setup.grouping.as_ref().map(Grouping::word),
        currency: setup.currency.code(),
        rows,
    };
    table(
        &report.rows,
        &[("key", "/key"), ("forecast", "/forecast")],
        &report,
        query.format.as_deref(),
        "forecast.csv",
    )
}

/// The report as JSON, or its rows as a CSV file.
fn table<R: Serialize, T: Serialize>(
    rows: &[R],
    columns: &[csv::Column],
    report: &T,
    format: Option<&str>,
    filename: &str,
) -> Result<Response, ApiError> {
    if csv::wants_csv(format) {
        csv::table(rows, columns, true, filename)
    } else {
        Ok(axum::response::IntoResponse::into_response(Json(report)))
    }
}

/// The saved reports of the organization that are not reversed.
async fn saved(state: &AppState, organization_id: Uuid) -> Result<Vec<SavedReport>, ApiError> {
    let rows = sqlx::query!(
        "SELECT r.id, r.name, r.filter, r.grouped_by, r.from_day, r.to_day, p.email, r.created_at
         FROM saved_reports r JOIN people p ON p.id = r.created_by
         WHERE r.organization_id = $1 AND r.reversed_at IS NULL
         ORDER BY r.created_at, r.id",
        organization_id
    )
    .fetch_all(&state.db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| SavedReport {
            id: row.id,
            name: row.name,
            filter: serde_json::from_value(row.filter).unwrap_or_default(),
            by: row.grouped_by,
            from: row.from_day,
            to: row.to_day,
            created_by: row.email,
            created_at: row.created_at,
        })
        .collect())
}

/// The saved reports of the organization.
#[utoipa::path(
    get,
    path = "/reports/saved",
    tag = "reports",
    responses((status = OK, body = Vec<SavedReport>)),
)]
async fn list_saved(
    State(state): State<AppState>,
    member: Member,
) -> Result<Json<Vec<SavedReport>>, ApiError> {
    Ok(Json(saved(&state, member.organization_id).await?))
}

/// Save a filter, grouping and range as a report. Reverse it through
/// `POST /reversals` with its id.
#[utoipa::path(
    post,
    path = "/reports/saved",
    tag = "reports",
    request_body = NewSavedReport,
    responses(
        (status = CREATED, body = SavedReport),
        (status = BAD_REQUEST, body = crate::error::ErrorBody),
    ),
)]
async fn save_report(
    State(state): State<AppState>,
    caller: Caller,
    Json(body): Json<NewSavedReport>,
) -> Result<(StatusCode, Json<SavedReport>), ApiError> {
    caller.require_credential(&[CredentialKind::Session, CredentialKind::Service])?;
    let name = body.name.trim();
    if name.is_empty() || name.chars().count() > 100 {
        return Err(ApiError::BadRequest(
            "a saved report needs a name of up to 100 characters".to_owned(),
        ));
    }
    let filter = json::tags(&body.filter)?;
    crate::tags::check_names(&state.db, caller.organization_id, &filter).await?;
    let by = Grouping::parse(body.by.as_deref())?.map(|grouping| grouping.word());
    let from = crate::dates::parse_optional(body.from.as_deref())?;
    let to = crate::dates::parse_optional(body.to.as_deref())?;
    if let (Some(from), Some(to)) = (from, to) {
        crate::dates::range_of(from, to)?;
    }
    let id = events::new_id(prefix::SAVED_REPORT);
    sqlx::query!(
        "INSERT INTO saved_reports (id, organization_id, name, filter, grouped_by, from_day, to_day, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        id,
        caller.organization_id,
        name,
        serde_json::to_value(json::tags_map(&filter)).unwrap_or_default(),
        by,
        from,
        to,
        caller.person_id,
    )
    .execute(&state.db)
    .await?;
    let report = saved(&state, caller.organization_id)
        .await?
        .into_iter()
        .find(|report| report.id == id)
        .ok_or(ApiError::NotFound)?;
    Ok((StatusCode::CREATED, Json(report)))
}

/// Reverses a saved report: the one who saved it or an org manager.
///
/// # Errors
///
/// [`ApiError::NotFound`] for an id that is not a saved report of the
/// organization, [`ApiError::Conflict`] when it is reversed already,
/// [`ApiError::Forbidden`] for anyone else.
pub async fn reverse_saved(
    state: &AppState,
    caller: &Caller,
    fold: &Fold,
    id: &str,
) -> Result<(), ApiError> {
    let row = sqlx::query!(
        "SELECT created_by, reversed_at FROM saved_reports WHERE organization_id = $1 AND id = $2",
        caller.organization_id,
        id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;
    if row.reversed_at.is_some() {
        return Err(ApiError::Conflict(
            "the saved report is reversed already".to_owned(),
        ));
    }
    if row.created_by != caller.person_id && !access::is_org_manager(fold, &caller.person()?) {
        return Err(ApiError::Forbidden);
    }
    sqlx::query!(
        "UPDATE saved_reports SET reversed_at = now() WHERE organization_id = $1 AND id = $2",
        caller.organization_id,
        id
    )
    .execute(&state.db)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn groupings_are_read_from_the_by_parameter() {
        assert_eq!(Grouping::parse(None).unwrap(), None);
        assert_eq!(Grouping::parse(Some(" ")).unwrap(), None);
        assert_eq!(
            Grouping::parse(Some("person")).unwrap(),
            Some(Grouping::Person)
        );
        assert_eq!(Grouping::parse(Some("day")).unwrap(), Some(Grouping::Day));
        assert_eq!(
            Grouping::parse(Some("project")).unwrap(),
            Some(Grouping::Tag(TagName::project()))
        );
        assert_eq!(
            Grouping::parse(Some("project")).unwrap().unwrap().word(),
            "project"
        );
        assert!(Grouping::parse(Some("not a name!")).is_err());
    }

    #[test]
    fn the_forecast_averages_three_windows_of_the_same_length() {
        let next = DateRange::new(date!(2026 - 04 - 01), date!(2026 - 04 - 10)).unwrap();
        let past = past_windows(next);
        assert_eq!(past.len(), 3);
        assert_eq!(past[2].to, date!(2026 - 03 - 31));
        assert_eq!(past[2].from, date!(2026 - 03 - 22));
        assert_eq!(past[0].from, date!(2026 - 03 - 02));
        assert!(past.iter().all(|window| window.days() == 10));
    }

    #[test]
    fn shares_are_fractions_of_the_total() {
        assert_eq!(share_text(700, 950), "0.736842");
        assert_eq!(share_text(0, 0), "0");
        assert_eq!(share_text(1, 1), "1");
    }
}
