//! Outliers (docs: Report > Outliers): tickets whose cost is far from the
//! organization's own baseline for similar work. Similar past tickets are
//! the tickets of the same project in the twelve months before the period,
//! or of the whole organization when the project has too few. The report
//! lists the tickets furthest from it, high or low. Nothing is sent and
//! nothing is stopped.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use axum::Json;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tokencontroller_core::{Currency, Money, MoneyError};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::READERS;
use super::csv::Csv;
use super::filter::Filter;
use super::lines::{self, CostLine, LineQuery, StatusView};
use super::scope::Scope;
use crate::AppState;
use crate::bills::amount::format_amount;
use crate::bills::rates::Converter;
use crate::budgets::similarity::median;
use crate::close::account;
use crate::close::period::{Period, PeriodDates};
use crate::error::ApiError;
use crate::sign_in::Caller;

/// How many past tickets a baseline needs.
const MIN_SIMILAR: usize = 3;
/// How many months back past tickets are read.
const MONTHS_BACK: usize = 12;
/// Tickets listed when the query names no limit.
const DEFAULT_LIMIT: usize = 20;

pub(super) fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(outliers))
}

/// Which outliers to list.
#[derive(Debug, Clone, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct OutlierQuery {
    /// The period, such as `2026-09`.
    #[param(value_type = String, example = "2026-09")]
    pub period: Period,
    /// Only tickets matching this tag filter, such as `client:acme`.
    #[param(example = "client:acme")]
    pub filter: Option<String>,
    /// How many tickets to list, 20 when left out.
    pub limit: Option<usize>,
    /// `csv` for a CSV file.
    pub format: Option<String>,
}

/// Above or below the baseline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    High,
    Low,
}

/// What a ticket was compared with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Baseline {
    /// Past tickets of the same project.
    Project,
    /// Past tickets of the whole organization.
    Organization,
}

/// One ticket far from its baseline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct Outlier {
    #[schema(example = "KD-214")]
    pub ticket: String,
    pub project: Option<String>,
    /// The ticket's cost so far, up to the end of the period.
    #[schema(example = "212.40")]
    pub amount: String,
    /// The median cost of similar past tickets.
    #[schema(example = "48.10")]
    pub baseline: String,
    /// Cost over baseline, such as `4.4` for four times as much.
    #[schema(example = "4.4")]
    pub ratio: String,
    pub direction: Direction,
    pub compared_with: Baseline,
    /// How many past tickets the baseline comes from.
    pub similar_tickets: usize,
    pub token_entries: usize,
    /// Token entries on a model missing from the price table.
    pub unknown_model: usize,
}

/// The outliers of a period.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct Outliers {
    #[schema(value_type = String, example = "2026-09")]
    pub period: Period,
    pub closed: bool,
    #[schema(value_type = String, example = "EUR")]
    pub currency: Currency,
    /// `list-price estimate`.
    pub label: &'static str,
    /// Furthest from the baseline first.
    pub tickets: Vec<Outlier>,
    /// Tickets of the period with too few similar past tickets to compare.
    pub without_baseline: usize,
}

/// One ticket's cost.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Ticket {
    project: Option<String>,
    nanos: i64,
    entries: BTreeSet<uuid::Uuid>,
    unknown: BTreeSet<uuid::Uuid>,
    /// Has cost in the period itself.
    current: bool,
}

/// Adds cost lines to tickets, converted with `converter`.
fn add_lines<'a>(
    tickets: &mut BTreeMap<String, Ticket>,
    lines: impl IntoIterator<Item = &'a CostLine>,
    converter: &Converter,
    current: bool,
) -> Result<(), ApiError> {
    for line in lines {
        let Some(key) = line.tags.get("ticket") else {
            continue;
        };
        let ticket = tickets.entry(key.clone()).or_default();
        ticket.current = ticket.current || current;
        if ticket.project.is_none() {
            ticket.project = line.tags.get("project").cloned();
        }
        ticket.entries.insert(line.entry_id);
        match line.list_value {
            Some(value) => {
                let value = converter
                    .convert(Some(&line.provider), value)
                    .map_err(|error| ApiError::Conflict(error.to_string()))?;
                ticket.nanos = ticket
                    .nanos
                    .checked_add(value.nanos())
                    .ok_or_else(|| ApiError::Internal(MoneyError::Overflow.into()))?;
            }
            None => {
                ticket.unknown.insert(line.entry_id);
            }
        }
    }
    Ok(())
}

/// How far `amount` is from `baseline`, as a fraction of at least one:
/// twice and half are equally far.
fn distance(amount: i64, baseline: i64) -> (i128, i128) {
    if amount >= baseline {
        (i128::from(amount), i128::from(baseline))
    } else {
        (i128::from(baseline), i128::from(amount))
    }
}

/// Orders distances, furthest first. A zero denominator is infinitely far.
fn further(a: (i128, i128), b: (i128, i128)) -> Ordering {
    let left = a.0.saturating_mul(b.1);
    let right = b.0.saturating_mul(a.1);
    right.cmp(&left)
}

/// `amount / baseline` in tenths, rounded half up.
fn ratio_tenths(amount: i64, baseline: i64) -> Option<i128> {
    let amount = i128::from(amount);
    let baseline = i128::from(baseline);
    if baseline <= 0 {
        return None;
    }
    let doubled = amount.checked_mul(20)?.checked_add(baseline)?;
    doubled.checked_div(baseline.checked_mul(2)?)
}

fn format_tenths(tenths: i128) -> String {
    format!("{}.{}", tenths.div_euclid(10), tenths.rem_euclid(10))
}

/// The median cost of past tickets and how many there were.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Median {
    amount: Money,
    tickets: usize,
}

/// The baselines of each project and of the organization.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Baselines {
    projects: BTreeMap<String, Median>,
    organization: Option<Median>,
}

/// The baseline of each project, and of the organization, from past
/// tickets.
fn baselines(past: &BTreeMap<String, Ticket>, currency: Currency) -> Result<Baselines, MoneyError> {
    let mut by_project: BTreeMap<String, Vec<Money>> = BTreeMap::new();
    let mut all = Vec::new();
    for ticket in past.values().filter(|ticket| ticket.nanos > 0) {
        let amount = Money::from_nanos(ticket.nanos, currency);
        all.push(amount);
        if let Some(project) = &ticket.project {
            by_project.entry(project.clone()).or_default().push(amount);
        }
    }
    let mut projects = BTreeMap::new();
    for (project, amounts) in by_project {
        if amounts.len() >= MIN_SIMILAR
            && let Some(middle) = median(&amounts, currency)?
        {
            projects.insert(
                project,
                Median {
                    amount: middle,
                    tickets: amounts.len(),
                },
            );
        }
    }
    let organization = if all.len() >= MIN_SIMILAR {
        median(&all, currency)?.map(|middle| Median {
            amount: middle,
            tickets: all.len(),
        })
    } else {
        None
    };
    Ok(Baselines {
        projects,
        organization,
    })
}

/// An outlier with its distance from its baseline, for sorting.
type Found = ((i128, i128), Outlier);

/// Each ticket with cost next to its baseline, with its distance from it,
/// and how many tickets had no baseline to compare with.
fn against_baselines(
    tickets: BTreeMap<String, Ticket>,
    baselines: &Baselines,
    currency: Currency,
) -> (Vec<Found>, usize) {
    let Baselines {
        projects,
        organization,
    } = baselines;
    let mut without_baseline = 0_usize;
    let mut found: Vec<Found> = Vec::new();
    for (key, ticket) in tickets {
        if ticket.nanos <= 0 {
            continue;
        }
        let baseline = ticket
            .project
            .as_ref()
            .and_then(|project| projects.get(project))
            .map(|found| (*found, Baseline::Project))
            .or_else(|| organization.map(|found| (found, Baseline::Organization)));
        let Some((
            Median {
                amount: middle,
                tickets: similar,
            },
            compared_with,
        )) = baseline
        else {
            without_baseline = without_baseline.saturating_add(1);
            continue;
        };
        let Some(tenths) = ratio_tenths(ticket.nanos, middle.nanos()) else {
            without_baseline = without_baseline.saturating_add(1);
            continue;
        };
        found.push((
            distance(ticket.nanos, middle.nanos()),
            Outlier {
                ticket: key,
                project: ticket.project,
                amount: format_amount(Money::from_nanos(ticket.nanos, currency)),
                baseline: format_amount(middle),
                ratio: format_tenths(tenths),
                direction: if ticket.nanos >= middle.nanos() {
                    Direction::High
                } else {
                    Direction::Low
                },
                compared_with,
                similar_tickets: similar,
                token_entries: ticket.entries.len(),
                unknown_model: ticket.unknown.len(),
            },
        ));
    }
    (found, without_baseline)
}

/// Computes the outliers of a period for a caller.
///
/// # Errors
///
/// [`ApiError::BadRequest`] for a filter that cannot be read,
/// [`ApiError::Conflict`] when an exchange rate is missing, or a database
/// error.
pub async fn compute(
    db: &PgPool,
    caller: &Caller,
    query: &OutlierQuery,
) -> Result<Outliers, ApiError> {
    let filter = Filter::parse(query.filter.as_deref().unwrap_or(""))
        .map_err(|error| ApiError::BadRequest(error.to_string()))?;
    let scope = Scope::of(db, caller).await?;
    let account = account::load(
        db,
        caller.organization_id,
        query.period,
        StatusView::All,
        &scope,
        &filter,
    )
    .await?;
    let currency = account.currency;

    let mut from = query.period;
    for _ in 0..MONTHS_BACK {
        from = from
            .previous()
            .map_err(|error| ApiError::BadRequest(error.to_string()))?;
    }
    let past_query = LineQuery {
        organization_id: caller.organization_id,
        from: from.starts_at(),
        until: query.period.starts_at(),
        statuses: StatusView::All.statuses(),
        filter: Filter::default(),
        with_late: true,
    };
    let past_lines = lines::fetch(db, &past_query, &Scope::everything()).await?;

    // The period's tickets as the caller sees them, with their earlier cost.
    let mut tickets: BTreeMap<String, Ticket> = BTreeMap::new();
    add_lines(
        &mut tickets,
        account.lines.iter().map(|line| &line.line),
        &account.converter,
        true,
    )?;
    let earlier = past_lines
        .iter()
        .filter(|line| scope.sees(line) && filter.matches(&line.tags))
        .filter(|line| {
            line.tags
                .get("ticket")
                .is_some_and(|ticket| tickets.contains_key(ticket))
        })
        .collect::<Vec<_>>();
    add_lines(&mut tickets, earlier, &account.converter, false)?;

    // Past tickets of the whole organization, without the period's own.
    let mut past: BTreeMap<String, Ticket> = BTreeMap::new();
    add_lines(&mut past, &past_lines, &account.converter, false)?;
    past.retain(|key, _| !tickets.contains_key(key));
    let baselines = baselines(&past, currency).map_err(|error| ApiError::Internal(error.into()))?;
    let (mut found, without_baseline) = against_baselines(tickets, &baselines, currency);
    found.sort_by(|a, b| further(a.0, b.0).then_with(|| a.1.ticket.cmp(&b.1.ticket)));
    Ok(Outliers {
        period: query.period,
        closed: account.closed.is_some(),
        currency,
        label: "list-price estimate",
        tickets: found
            .into_iter()
            .take(query.limit.unwrap_or(DEFAULT_LIMIT))
            .map(|(_, outlier)| outlier)
            .collect(),
        without_baseline,
    })
}

/// Outliers as CSV.
#[must_use]
pub fn to_csv(outliers: &Outliers) -> Csv {
    let mut csv = Csv::new([
        "period",
        "ticket",
        "project",
        "amount",
        "baseline",
        "currency",
        "ratio",
        "direction",
        "compared_with",
        "similar_tickets",
        "token_entries",
        "unknown_model",
    ]);
    for ticket in &outliers.tickets {
        csv.row([
            outliers.period.to_string(),
            ticket.ticket.clone(),
            ticket.project.clone().unwrap_or_default(),
            ticket.amount.clone(),
            ticket.baseline.clone(),
            outliers.currency.code(),
            ticket.ratio.clone(),
            match ticket.direction {
                Direction::High => "high",
                Direction::Low => "low",
            }
            .to_owned(),
            match ticket.compared_with {
                Baseline::Project => "project",
                Baseline::Organization => "organization",
            }
            .to_owned(),
            ticket.similar_tickets.to_string(),
            ticket.token_entries.to_string(),
            ticket.unknown_model.to_string(),
        ]);
    }
    csv
}

/// Tickets that cost far more or less than similar past tickets.
#[utoipa::path(
    get,
    path = "/reports/outliers",
    tag = "reports",
    params(OutlierQuery),
    responses(
        (status = OK, description = "The outliers, as JSON or CSV", content((Outliers = "application/json"), (String = "text/csv"))),
        (status = BAD_REQUEST, description = "The filter cannot be read", body = crate::error::ErrorBody),
        (status = CONFLICT, description = "An exchange rate is missing", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn outliers(
    State(state): State<AppState>,
    caller: Caller,
    Query(query): Query<OutlierQuery>,
) -> Result<Response, ApiError> {
    caller.require_credential(READERS)?;
    let outliers = compute(&state.db, &caller, &query).await?;
    if query.format.as_deref() == Some("csv") {
        let filename = format!("outliers-{}.csv", outliers.period);
        return Ok(to_csv(&outliers).into_download(&filename));
    }
    Ok(Json(outliers).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twice_and_half_are_equally_far() {
        assert_eq!(
            further(distance(200, 100), distance(50, 100)),
            Ordering::Equal
        );
        assert_eq!(
            further(distance(400, 100), distance(50, 100)),
            Ordering::Less
        );
        assert_eq!(
            further(distance(110, 100), distance(80, 100)),
            Ordering::Greater
        );
        assert_eq!(
            further(distance(0, 100), distance(900, 100)),
            Ordering::Less
        );
    }

    #[test]
    fn ratios_round_to_tenths() {
        assert_eq!(
            ratio_tenths(212, 48).map(format_tenths).as_deref(),
            Some("4.4")
        );
        assert_eq!(
            ratio_tenths(25, 100).map(format_tenths).as_deref(),
            Some("0.3")
        );
        assert_eq!(
            ratio_tenths(100, 100).map(format_tenths).as_deref(),
            Some("1.0")
        );
        assert_eq!(ratio_tenths(1, 0), None);
    }

    #[test]
    fn a_baseline_needs_three_past_tickets() {
        let usd = Currency::new("USD").unwrap();
        let ticket = |project: &str, nanos: i64| Ticket {
            project: Some(project.to_owned()),
            nanos,
            ..Ticket::default()
        };
        let past: BTreeMap<String, Ticket> = [
            ("A-1", ticket("shop", 10)),
            ("A-2", ticket("shop", 30)),
            ("A-3", ticket("shop", 20)),
            ("B-1", ticket("app", 100)),
        ]
        .into_iter()
        .map(|(key, ticket)| (key.to_owned(), ticket))
        .collect();
        let Baselines {
            projects,
            organization,
        } = baselines(&past, usd).unwrap();
        let median = |nanos: i64, tickets: usize| Median {
            amount: Money::from_nanos(nanos, usd),
            tickets,
        };
        assert_eq!(projects.get("shop"), Some(&median(20, 3)));
        assert_eq!(projects.get("app"), None);
        assert_eq!(organization, Some(median(25, 4)));
    }
}
