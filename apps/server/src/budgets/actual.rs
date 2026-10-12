//! Actual: what was spent against a token budget. It is the list value of
//! the counting cost lines that match the token budget's filter, converted
//! into the reporting currency as each period's account does, plus the
//! adjustments posted to closed periods on that filter. Token entries on an
//! unknown model are counted, never valued as zero.

use std::collections::{BTreeMap, BTreeSet};

use sqlx::PgPool;
use time::OffsetDateTime;
use tokencontroller_core::{Currency, Money, MoneyError};
use uuid::Uuid;

use crate::close::account::{self, AccountError, Adjustment, Line};
use crate::close::period::Period;
use crate::reports::filter::Filter;
use crate::reports::lines::StatusView;
use crate::reports::scope::Scope;

/// What was spent on one filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actual<'a> {
    /// The known part, in the reporting currency.
    pub value: Money,
    /// Token entries with at least one matching split.
    pub token_entries: usize,
    /// Of those, the ones on a model missing from the price table, whose
    /// cost is not in `value`.
    pub unknown_model_entries: usize,
    pub lines: Vec<&'a Line>,
    pub adjustments: Vec<&'a Adjustment>,
}

impl Actual<'_> {
    /// Whether every token entry behind it has a known cost.
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        self.unknown_model_entries == 0
    }
}

/// One period's cost lines and adjustments.
#[derive(Debug, Clone, Default)]
struct PeriodFigures {
    lines: Vec<Line>,
    adjustments: Vec<Adjustment>,
}

/// The cost lines and adjustments of some periods, in the reporting
/// currency, as one caller may see them.
#[derive(Debug, Clone)]
pub struct Actuals {
    pub currency: Currency,
    periods: BTreeMap<Period, PeriodFigures>,
}

impl Actuals {
    /// Loads `periods` as `scope` may see them.
    ///
    /// # Errors
    ///
    /// [`AccountError::Rate`] when a figure cannot be converted to the
    /// reporting currency, or a database error.
    pub async fn load(
        db: &PgPool,
        organization_id: Uuid,
        periods: &BTreeSet<Period>,
        view: StatusView,
        scope: &Scope,
    ) -> Result<Self, AccountError> {
        let currency = crate::bills::reporting_currency(db, organization_id)
            .await?
            .ok_or(AccountError::NoOrganization)?;
        let mut figures = BTreeMap::new();
        for period in periods {
            let account = account::load(
                db,
                organization_id,
                *period,
                view,
                scope,
                &Filter::default(),
            )
            .await?;
            let adjustments = account
                .adjustments
                .into_iter()
                .filter(|adjustment| sees_adjustment(scope, adjustment))
                .collect();
            figures.insert(
                *period,
                PeriodFigures {
                    lines: account.lines,
                    adjustments,
                },
            );
        }
        Ok(Self {
            currency,
            periods: figures,
        })
    }

    /// What was spent on `filter` in `period`, or in every loaded period
    /// for `None`.
    ///
    /// # Errors
    ///
    /// [`MoneyError::Overflow`] when the sum does not fit.
    pub fn of(&self, filter: &Filter, period: Option<Period>) -> Result<Actual<'_>, MoneyError> {
        let figures: Vec<&PeriodFigures> = match period {
            Some(period) => self.periods.get(&period).into_iter().collect(),
            None => self.periods.values().collect(),
        };
        let lines: Vec<&Line> = figures
            .iter()
            .flat_map(|figures| figures.lines.iter())
            .filter(|line| filter.matches(&line.line.tags))
            .collect();
        let adjustments: Vec<&Adjustment> = figures
            .iter()
            .flat_map(|figures| figures.adjustments.iter())
            .filter(|adjustment| filter.matches(&adjustment.tags))
            .collect();
        let value = Money::sum(
            lines
                .iter()
                .filter_map(|line| line.value)
                .chain(adjustments.iter().map(|adjustment| adjustment.amount)),
            self.currency,
        )?;
        let entries: BTreeSet<Uuid> = lines.iter().map(|line| line.line.entry_id).collect();
        let unknown: BTreeSet<Uuid> = lines
            .iter()
            .filter(|line| line.value.is_none())
            .map(|line| line.line.entry_id)
            .collect();
        Ok(Actual {
            value,
            token_entries: entries.len(),
            unknown_model_entries: unknown.len(),
            lines,
            adjustments,
        })
    }

    /// Every loaded cost line, oldest period first.
    pub fn lines(&self) -> impl Iterator<Item = &Line> {
        self.periods
            .values()
            .flat_map(|figures| figures.lines.iter())
    }
}

/// Whether `scope` sees an adjustment: a controller sees all of them, a
/// people manager those within their filters.
fn sees_adjustment(scope: &Scope, adjustment: &Adjustment) -> bool {
    scope.is_everything()
        || scope
            .filters()
            .iter()
            .any(|mine| mine.matches(&adjustment.tags))
}

/// Every period from the organization's first token entry up to this month:
/// what an all-time token budget counts.
///
/// # Errors
///
/// A database error.
pub async fn all_periods(
    db: &PgPool,
    organization_id: Uuid,
    now: OffsetDateTime,
) -> Result<BTreeSet<Period>, sqlx::Error> {
    let first = sqlx::query_scalar!(
        "SELECT min(started_at) FROM token_entries WHERE organization_id = $1",
        organization_id,
    )
    .fetch_one(db)
    .await?;
    Ok(periods_between(first.map(Period::of), Period::of(now)))
}

/// The periods from `first` up to `last`, or only `last` when there is no
/// first or it lies after `last`.
fn periods_between(first: Option<Period>, last: Period) -> BTreeSet<Period> {
    let mut periods = BTreeSet::from([last]);
    let Some(mut period) = first else {
        return periods;
    };
    while period < last {
        periods.insert(period);
        let Ok(next) = period.next() else { break };
        period = next;
    }
    periods
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reports::filter::Tags;
    use crate::reports::lines::{CostLine, EntryStatus};
    use time::macros::datetime;

    fn period(text: &str) -> Period {
        text.parse().unwrap()
    }

    fn usd(cents: i64) -> Money {
        Money::from_minor_units(cents, Currency::USD).unwrap()
    }

    fn line(entry: u128, cents: Option<i64>, tags: &[(&str, &str)]) -> Line {
        Line {
            line: CostLine {
                split_id: Uuid::from_u128(entry.checked_mul(100).unwrap()),
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
                list_value: cents.map(usd),
                tags: tags
                    .iter()
                    .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
                    .collect::<Tags>(),
            },
            value: cents.map(usd),
        }
    }

    fn adjustment(cents: i64, tags: &[(&str, &str)]) -> Adjustment {
        Adjustment {
            id: Uuid::nil(),
            amount: usd(cents),
            tags: tags
                .iter()
                .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
                .collect(),
            reason: "credit".to_owned(),
            late: false,
            token_entry_id: None,
            posted_by: None,
            posted_at: datetime!(2026-10-02 09:00 UTC),
        }
    }

    fn actuals() -> Actuals {
        let august = PeriodFigures {
            lines: vec![line(1, Some(1_000), &[("ticket", "KD-1"), ("team", "web")])],
            adjustments: Vec::new(),
        };
        let september = PeriodFigures {
            lines: vec![
                line(2, Some(500), &[("ticket", "KD-1"), ("team", "web")]),
                line(2, Some(250), &[("ticket", "KD-2"), ("team", "web")]),
                line(3, None, &[("ticket", "KD-1"), ("team", "web")]),
                line(4, Some(700), &[("team", "data")]),
            ],
            adjustments: vec![adjustment(-100, &[("ticket", "KD-1")])],
        };
        Actuals {
            currency: Currency::USD,
            periods: [(period("2026-08"), august), (period("2026-09"), september)].into(),
        }
    }

    #[test]
    fn one_period_counts_its_lines_and_adjustments() {
        let actuals = actuals();
        let kd1 = Filter::parse("ticket:KD-1").unwrap();
        let actual = actuals.of(&kd1, Some(period("2026-09"))).unwrap();
        assert_eq!(actual.value, usd(400));
        assert_eq!(actual.token_entries, 2);
        assert_eq!(actual.unknown_model_entries, 1);
        assert!(!actual.is_complete());
        assert_eq!(actual.lines.len(), 2);
        assert_eq!(actual.adjustments.len(), 1);
    }

    #[test]
    fn all_time_counts_every_period() {
        let actuals = actuals();
        let kd1 = Filter::parse("ticket:KD-1").unwrap();
        assert_eq!(actuals.of(&kd1, None).unwrap().value, usd(1_400));
        let web = Filter::parse("team:web").unwrap();
        let actual = actuals.of(&web, None).unwrap();
        assert_eq!(actual.value, usd(1_750));
        assert_eq!(actual.token_entries, 3);
        assert_eq!(actuals.lines().count(), 5);
    }

    #[test]
    fn nothing_matching_is_zero_and_complete() {
        let actuals = actuals();
        let none = Filter::parse("client:globex").unwrap();
        let actual = actuals.of(&none, None).unwrap();
        assert_eq!(actual.value, usd(0));
        assert_eq!(actual.token_entries, 0);
        assert!(actual.is_complete());
        let unloaded = actuals.of(&none, Some(period("2026-01"))).unwrap();
        assert_eq!(unloaded.value, usd(0));
    }

    #[test]
    fn people_managers_see_adjustments_within_their_filters() {
        let web = Scope::of_person(Uuid::nil(), vec![Filter::parse("team:web").unwrap()]);
        assert!(sees_adjustment(
            &web,
            &adjustment(100, &[("team", "web"), ("ticket", "KD-1")])
        ));
        assert!(!sees_adjustment(
            &web,
            &adjustment(100, &[("ticket", "KD-1")])
        ));
        assert!(sees_adjustment(&Scope::everything(), &adjustment(100, &[])));
        assert!(!sees_adjustment(
            &Scope::of_person(Uuid::nil(), Vec::new()),
            &adjustment(100, &[])
        ));
    }

    #[test]
    fn all_time_runs_from_the_first_token_entry_to_this_month() {
        let periods = periods_between(Some(period("2026-07")), period("2026-10"));
        let shown: Vec<String> = periods.iter().map(ToString::to_string).collect();
        assert_eq!(shown, ["2026-07", "2026-08", "2026-09", "2026-10"]);
        assert_eq!(periods_between(None, period("2026-10")).len(), 1);
        assert_eq!(
            periods_between(Some(period("2026-10")), period("2026-10")).len(),
            1
        );
        assert_eq!(
            periods_between(Some(period("2026-12")), period("2026-10")).len(),
            1
        );
    }
}
