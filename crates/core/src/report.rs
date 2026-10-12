//! Reports and token budgets: tag filters over splits.
//!
//! Filters overlap, so two reports or two token budgets are never added
//! together. Grouping by one tag name adds up to the filter's total, with a
//! line for splits without that tag.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::money::{Currency, Money, MoneyError};
use crate::period::Period;
use crate::tags::{Filter, TagName, TagValue, Tags};

/// Cost by the values of one tag name, for the splits a filter matches.
/// The `None` line holds the splits without the tag. Lines are ordered by
/// value, the `None` line first.
///
/// # Errors
///
/// [`MoneyError::CurrencyMismatch`] if an amount is not in `currency`, or
/// [`MoneyError::Overflow`].
pub fn group_by<'a, I>(
    items: I,
    filter: &Filter,
    by: &TagName,
    currency: Currency,
) -> Result<Vec<(Option<TagValue>, Money)>, MoneyError>
where
    I: IntoIterator<Item = (&'a Tags, Money)>,
{
    let mut groups: BTreeMap<Option<TagValue>, Money> = BTreeMap::new();
    for (tags, amount) in items {
        if !filter.matches(tags) {
            continue;
        }
        let group = groups
            .entry(tags.get(by).cloned())
            .or_insert(Money::zero(currency));
        *group = group.checked_add(amount)?;
    }
    Ok(groups.into_iter().collect())
}

/// What the splits a filter matches cost together.
///
/// # Errors
///
/// As [`group_by`].
pub fn actual<'a, I>(items: I, filter: &Filter, currency: Currency) -> Result<Money, MoneyError>
where
    I: IntoIterator<Item = (&'a Tags, Money)>,
{
    Money::sum(
        items
            .into_iter()
            .filter(|(tags, _)| filter.matches(tags))
            .map(|(_, amount)| amount),
        currency,
    )
}

/// Money a human has planned for a filter and a period. It never stops
/// spending.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenBudget {
    pub filter: Filter,
    pub period: Period,
    pub amount: Money,
}

/// How actual cost compares with a token budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Overrun,
    Underrun,
    OnBudget,
}

/// A token budget next to what was spent against it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetVsActual {
    pub budget: Money,
    pub actual: Money,
    /// Actual minus budget: above zero is an overrun.
    pub difference: Money,
    pub outcome: Outcome,
}

impl TokenBudget {
    /// Compares the budget with the splits of its period that its filter
    /// matches. The caller passes the splits of the budget's period.
    ///
    /// # Errors
    ///
    /// As [`group_by`].
    pub fn compare<'a, I>(&self, items: I) -> Result<BudgetVsActual, MoneyError>
    where
        I: IntoIterator<Item = (&'a Tags, Money)>,
    {
        let actual = actual(items, &self.filter, self.amount.currency())?;
        let difference = actual.checked_sub(self.amount)?;
        let outcome = match difference.nanos().signum() {
            1 => Outcome::Overrun,
            -1 => Outcome::Underrun,
            _ => Outcome::OnBudget,
        };
        Ok(BudgetVsActual {
            budget: self.amount,
            actual,
            difference,
            outcome,
        })
    }
}

/// Basis points in a whole: 10,000 is 100 percent.
pub const BASIS_POINTS: u64 = 10_000;

/// Each amount's share of their total in basis points, for the share view.
/// The shares add up to exactly 10,000 with the largest-remainder method.
///
/// # Errors
///
/// [`MoneyError::NoWeights`] if every amount is zero or there are none, and
/// [`MoneyError::InvalidAmount`] if one is negative: shares of a period
/// are shares of usage, which is never below zero.
pub fn shares(amounts: &[Money]) -> Result<Vec<u64>, MoneyError> {
    let weights = amounts
        .iter()
        .map(|amount| {
            u64::try_from(amount.nanos()).map_err(|_| MoneyError::InvalidAmount(amount.to_string()))
        })
        .collect::<Result<Vec<u64>, _>>()?;
    let whole = i64::try_from(BASIS_POINTS).map_err(|_| MoneyError::Overflow)?;
    Money::from_nanos(whole, Currency::USD)
        .allocate(&weights)?
        .into_iter()
        .map(|share| u64::try_from(share.nanos()).map_err(|_| MoneyError::Overflow))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usd(text: &str) -> Money {
        Money::parse(text, Currency::USD).unwrap()
    }

    fn tags(pairs: &[&str]) -> Tags {
        Tags::parse(pairs.iter().copied()).unwrap()
    }

    fn filter(pairs: &[&str]) -> Filter {
        Filter(tags(pairs))
    }

    fn items() -> Vec<(Tags, Money)> {
        vec![
            (tags(&["team=web", "client=acme"]), usd("600")),
            (tags(&["team=data", "client=acme"]), usd("300")),
            (tags(&["team=web"]), usd("220")),
            (tags(&["client=globex"]), usd("80")),
        ]
    }

    fn borrowed(items: &[(Tags, Money)]) -> impl Iterator<Item = (&Tags, Money)> {
        items.iter().map(|(tags, amount)| (tags, *amount))
    }

    #[test]
    fn grouping_by_one_tag_adds_up_with_a_line_for_the_rest() {
        let items = items();
        let team = TagName::new("team").unwrap();
        let groups = group_by(borrowed(&items), &Filter::default(), &team, Currency::USD).unwrap();
        assert_eq!(
            groups,
            vec![
                (None, usd("80")),
                (Some(TagValue::new("data").unwrap()), usd("300")),
                (Some(TagValue::new("web").unwrap()), usd("820")),
            ]
        );
        let acme = group_by(
            borrowed(&items),
            &filter(&["client=acme"]),
            &team,
            Currency::USD,
        )
        .unwrap();
        assert_eq!(
            acme,
            vec![
                (Some(TagValue::new("data").unwrap()), usd("300")),
                (Some(TagValue::new("web").unwrap()), usd("600")),
            ]
        );
        assert!(
            group_by(
                [(&tags(&[]), Money::zero(Currency::EUR))],
                &Filter::default(),
                &team,
                Currency::USD
            )
            .is_err()
        );
    }

    #[test]
    fn budgets_compare_with_what_their_filter_matches() {
        let items = items();
        let web = TokenBudget {
            filter: filter(&["team=web"]),
            period: Period::parse("2026-10").unwrap(),
            amount: usd("800"),
        };
        let compared = web.compare(borrowed(&items)).unwrap();
        assert_eq!(compared.actual, usd("820"));
        assert_eq!(compared.difference, usd("20"));
        assert_eq!(compared.outcome, Outcome::Overrun);
        let acme = TokenBudget {
            filter: filter(&["client=acme"]),
            amount: usd("1000"),
            ..web.clone()
        };
        let compared = acme.compare(borrowed(&items)).unwrap();
        assert_eq!(compared.outcome, Outcome::Underrun);
        assert_eq!(compared.difference, usd("-100"));
        let exact = TokenBudget {
            amount: usd("900"),
            ..acme
        };
        assert_eq!(
            exact.compare(borrowed(&items)).unwrap().outcome,
            Outcome::OnBudget
        );
        assert_eq!(
            actual(borrowed(&items), &filter(&["client=globex"]), Currency::USD).unwrap(),
            usd("80")
        );
    }

    #[test]
    fn shares_add_up_to_the_whole() {
        assert_eq!(
            shares(&[usd("1"), usd("1"), usd("1")]).unwrap(),
            vec![3334, 3333, 3333]
        );
        assert_eq!(
            shares(&[usd("3"), usd("0"), usd("1")]).unwrap(),
            vec![7500, 0, 2500]
        );
        assert_eq!(shares(&[usd("0")]), Err(MoneyError::NoWeights));
        assert_eq!(shares(&[]), Err(MoneyError::NoWeights));
        assert!(matches!(
            shares(&[usd("1"), usd("-1")]),
            Err(MoneyError::InvalidAmount(_))
        ));
    }
}
