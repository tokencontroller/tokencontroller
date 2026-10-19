//! Shares (source record 5) and a person's plan `P(person, w)`
//! (formula 8).
//!
//! `P(q, w) = Σ over shares of q: fraction × P(project, w ∩ [from, to])`.
//! The sum of fractions over people may exceed 1; it is shown, never
//! corrected. A handover is a new share for the new person.

use serde::{Deserialize, Serialize};
use time::Date;

use crate::estimate::BASIS_POINTS;
use crate::event::DateRange;
use crate::money::{Money, MoneyError};
use crate::tags::TagValue;

/// A person's share of a project's plan from a date on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Share {
    pub person: TagValue,
    /// The key of the project tag value.
    pub project: TagValue,
    /// The fraction in basis points: 6,000 is 0.6.
    pub fraction_basis_points: u32,
    pub from: Date,
    /// Open-ended when `None`.
    #[serde(default)]
    pub to: Option<Date>,
}

impl Share {
    /// The days of a window the share is in force on.
    #[must_use]
    pub fn days_in(&self, window: DateRange) -> Option<DateRange> {
        let to = self.to.unwrap_or(window.to).min(window.to);
        DateRange::new(self.from.max(window.from), to).ok()
    }

    /// This share's part of a project plan: `fraction × P(project, ·)`.
    ///
    /// # Errors
    ///
    /// [`MoneyError::Overflow`] if the amount does not fit.
    pub fn part_of(&self, project_plan: Money) -> Result<Money, MoneyError> {
        project_plan.checked_mul_ratio(
            u64::from(self.fraction_basis_points),
            u64::from(BASIS_POINTS),
        )
    }
}

/// The fractions of one project's shares on a day, added up over people.
/// Above 10,000 basis points the project is planned more than once over.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareSum {
    pub project: TagValue,
    pub basis_points: u64,
}

impl ShareSum {
    /// Whether the fractions add up to more than 1.
    #[must_use]
    pub fn is_over(&self) -> bool {
        self.basis_points > u64::from(BASIS_POINTS)
    }
}

/// The fractions of every project's shares in force on a date, by project.
#[must_use]
pub fn share_sums<'a>(shares: impl IntoIterator<Item = &'a Share>, on: Date) -> Vec<ShareSum> {
    let mut sums: std::collections::BTreeMap<TagValue, u64> = std::collections::BTreeMap::new();
    for share in shares {
        if share.from <= on && share.to.is_none_or(|to| on <= to) {
            let sum = sums.entry(share.project.clone()).or_default();
            *sum = sum.saturating_add(u64::from(share.fraction_basis_points));
        }
    }
    sums.into_iter()
        .map(|(project, basis_points)| ShareSum {
            project,
            basis_points,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::money::Currency;
    use time::macros::date;

    fn usd(text: &str) -> Money {
        Money::parse(text, Currency::USD).unwrap()
    }

    fn share(person: &str, basis_points: u32, from: Date, to: Option<Date>) -> Share {
        Share {
            person: TagValue::new(person).unwrap(),
            project: TagValue::new("shop").unwrap(),
            fraction_basis_points: basis_points,
            from,
            to,
        }
    }

    fn march() -> DateRange {
        DateRange::new(date!(2026 - 03 - 01), date!(2026 - 03 - 31)).unwrap()
    }

    #[test]
    fn a_share_takes_its_fraction_of_the_project_plan() {
        let mara = share("mara", 6_000, date!(2026 - 03 - 01), None);
        assert_eq!(mara.part_of(usd("232.50")).unwrap(), usd("139.50"));
        assert_eq!(mara.days_in(march()), Some(march()));
        let nothing = share("tom", 0, date!(2026 - 03 - 01), None);
        assert_eq!(nothing.part_of(usd("232.50")).unwrap(), usd("0"));
    }

    #[test]
    fn a_share_is_in_force_between_its_dates() {
        let mid = share(
            "mara",
            5_000,
            date!(2026 - 03 - 10),
            Some(date!(2026 - 03 - 20)),
        );
        assert_eq!(
            mid.days_in(march()),
            Some(DateRange::new(date!(2026 - 03 - 10), date!(2026 - 03 - 20)).unwrap())
        );
        let april = DateRange::new(date!(2026 - 04 - 01), date!(2026 - 04 - 30)).unwrap();
        assert_eq!(mid.days_in(april), None);
        let ended = share(
            "mara",
            5_000,
            date!(2026 - 01 - 01),
            Some(date!(2026 - 02 - 28)),
        );
        assert_eq!(ended.days_in(march()), None);
        let last_day = share(
            "mara",
            5_000,
            date!(2026 - 01 - 01),
            Some(date!(2026 - 03 - 01)),
        );
        assert_eq!(
            last_day.days_in(march()),
            Some(DateRange::day(date!(2026 - 03 - 01)))
        );
        let starts_last_day = share("mara", 5_000, date!(2026 - 03 - 31), None);
        assert_eq!(
            starts_last_day.days_in(march()),
            Some(DateRange::day(date!(2026 - 03 - 31)))
        );
    }

    #[test]
    fn share_sums_over_one_are_shown_never_corrected() {
        let shares = vec![
            share("mara", 6_000, date!(2026 - 03 - 01), None),
            share("tom", 6_000, date!(2026 - 03 - 15), None),
            share(
                "eva",
                2_000,
                date!(2026 - 01 - 01),
                Some(date!(2026 - 02 - 28)),
            ),
        ];
        let early = share_sums(&shares, date!(2026 - 03 - 10));
        assert_eq!(early.len(), 1);
        assert_eq!(early.first().unwrap().basis_points, 6_000);
        assert!(!early.first().unwrap().is_over());
        let late = share_sums(&shares, date!(2026 - 03 - 20));
        assert_eq!(late.first().unwrap().basis_points, 12_000);
        assert!(late.first().unwrap().is_over());
        let exact = share_sums(
            &[
                share("mara", 5_000, date!(2026 - 03 - 01), None),
                share("tom", 5_000, date!(2026 - 03 - 01), None),
            ],
            date!(2026 - 03 - 01),
        );
        assert!(!exact.first().unwrap().is_over());
        let february = share_sums(&shares, date!(2026 - 02 - 28));
        assert_eq!(february.first().unwrap().basis_points, 2_000);
        assert!(share_sums(&shares, date!(2025 - 12 - 31)).is_empty());
    }

    #[test]
    fn shares_read_from_json_open_ended_by_default() {
        let json = r#"{"person": "mara", "project": "shop", "fraction_basis_points": 6000,
            "from": "2026-03-01"}"#;
        let share: Share = serde_json::from_str(json).unwrap();
        assert_eq!(share.to, None);
        assert_eq!(share.person.as_str(), "mara");
    }
}
