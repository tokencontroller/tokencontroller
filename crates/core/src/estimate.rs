//! Estimates (source record 3), the accept that makes one the plan
//! (assignment 9), and the plan figure `P(F, w)` (formula 7).
//!
//! An estimate's amount is a probability distribution in dollars: bands
//! with a median. Many estimates may exist for one tag set; the accepted one
//! is the plan. Its amount, markup inside, is spread over a window by the
//! days of its date range.

use serde::{Deserialize, Serialize};

use crate::event::DateRange;
use crate::money::{Currency, Money, MoneyError};
use crate::tags::Tags;

/// Basis points in a whole: 10,000 is 100 percent.
pub const BASIS_POINTS: u32 = 10_000;

/// One band of a distribution: the chance, in basis points, that the cost
/// is above the band before and at most `up_to`. The last band has no
/// upper end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Band {
    pub up_to: Option<Money>,
    pub basis_points: u32,
}

/// A distribution that cannot be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DistributionError {
    #[error("the bands add up to {0} basis points, not 10,000")]
    BandsDoNotAddUp(u64),
    #[error("the bands must go up: {earlier} comes before {later}")]
    BandsOutOfOrder { earlier: Money, later: Money },
    #[error("the median {median} is not in {currency}")]
    CurrencyMismatch { median: Money, currency: Currency },
    #[error("the markup must be at most {0} basis points")]
    MarkupTooLarge(u32),
}

/// A probability distribution of what work will cost: bands with a median.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Distribution {
    pub median: Money,
    /// Bands in rising order of `up_to`; their basis points add up to
    /// 10,000. Empty when the estimator gave only a median.
    #[serde(default)]
    pub bands: Vec<Band>,
}

impl Distribution {
    /// A distribution of one figure: the median alone.
    #[must_use]
    pub const fn point(median: Money) -> Self {
        Self {
            median,
            bands: Vec::new(),
        }
    }

    /// The currency of the amounts.
    #[must_use]
    pub const fn currency(&self) -> Currency {
        self.median.currency()
    }

    /// Checks that the bands add up to 10,000 basis points, rise, and are in
    /// the median's currency.
    ///
    /// # Errors
    ///
    /// [`DistributionError`] naming the problem.
    pub fn check(&self) -> Result<(), DistributionError> {
        if self.bands.is_empty() {
            return Ok(());
        }
        let total = self.bands.iter().fold(0_u64, |sum, band| {
            sum.saturating_add(u64::from(band.basis_points))
        });
        if total != u64::from(BASIS_POINTS) {
            return Err(DistributionError::BandsDoNotAddUp(total));
        }
        let mut last: Option<Money> = None;
        for band in &self.bands {
            if let Some(up_to) = band.up_to {
                if up_to.currency() != self.currency() {
                    return Err(DistributionError::CurrencyMismatch {
                        median: up_to,
                        currency: self.currency(),
                    });
                }
                if let Some(earlier) = last
                    && up_to.nanos() <= earlier.nanos()
                {
                    return Err(DistributionError::BandsOutOfOrder {
                        earlier,
                        later: up_to,
                    });
                }
                last = Some(up_to);
            }
        }
        Ok(())
    }
}

/// An estimate of what a piece of work will cost, made by any estimator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Estimate {
    pub description: String,
    /// The tag set the estimate is for, such as `project: shop`.
    pub tags: Tags,
    pub distribution: Distribution,
    /// Overhead on top of the median, in basis points: 1,500 is 15 percent.
    /// The plan amount has the markup inside.
    #[serde(default)]
    pub markup_basis_points: u32,
    /// Who made it: a person, or an estimator such as the System 1
    /// classifier.
    pub estimator: String,
    /// The days the work is planned for. The plan spreads over them.
    pub range: DateRange,
}

/// The most markup an estimate may carry: 1,000 percent.
const MAX_MARKUP: u32 = 100_000;

impl Estimate {
    /// Checks the distribution and the markup.
    ///
    /// # Errors
    ///
    /// As [`Distribution::check`], or [`DistributionError::MarkupTooLarge`].
    pub fn check(&self) -> Result<(), DistributionError> {
        self.distribution.check()?;
        if self.markup_basis_points > MAX_MARKUP {
            return Err(DistributionError::MarkupTooLarge(MAX_MARKUP));
        }
        Ok(())
    }

    /// The plan amount: the median with the markup inside, rounded half
    /// away from zero to the nano-unit.
    ///
    /// # Errors
    ///
    /// [`MoneyError::Overflow`] if the amount does not fit.
    pub fn amount(&self) -> Result<Money, MoneyError> {
        let factor = u64::from(BASIS_POINTS).saturating_add(u64::from(self.markup_basis_points));
        self.distribution
            .median
            .checked_mul_ratio(factor, u64::from(BASIS_POINTS))
    }

    /// `P(F, w)` for this estimate: its amount spread over the window by
    /// days of its range. Zero when the window and the range share no day.
    ///
    /// # Errors
    ///
    /// [`MoneyError::Overflow`] if the amount does not fit.
    pub fn plan(&self, window: DateRange) -> Result<Money, MoneyError> {
        let amount = self.amount()?;
        let Some(shared) = self.range.intersection(window) else {
            return Ok(Money::zero(amount.currency()));
        };
        amount.checked_mul_ratio(shared.days(), self.range.days())
    }
}

/// Makes one estimate the plan for its tag set. Accepting another one for
/// the same tag set supersedes it; the earlier one stays as a record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Accept {
    /// The id of the estimate's event.
    pub estimate: crate::event::EventId,
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    fn usd(text: &str) -> Money {
        Money::parse(text, Currency::USD).unwrap()
    }

    fn band(up_to: Option<&str>, basis_points: u32) -> Band {
        Band {
            up_to: up_to.map(usd),
            basis_points,
        }
    }

    fn shop() -> Estimate {
        Estimate {
            description: "Shop".to_owned(),
            tags: Tags::parse(["project=shop"]).unwrap(),
            distribution: Distribution::point(usd("750")),
            markup_basis_points: 0,
            estimator: "mara".to_owned(),
            range: DateRange::new(date!(2026 - 02 - 01), date!(2026 - 05 - 11)).unwrap(),
        }
    }

    fn march() -> DateRange {
        DateRange::new(date!(2026 - 03 - 01), date!(2026 - 03 - 31)).unwrap()
    }

    #[test]
    fn the_plan_spreads_the_amount_over_the_window_by_days() {
        // 750 over 100 days, 31 of them in March.
        assert_eq!(shop().plan(march()).unwrap(), usd("232.50"));
        assert_eq!(shop().plan(shop().range).unwrap(), usd("750"));
        let april = DateRange::new(date!(2026 - 06 - 01), date!(2026 - 06 - 30)).unwrap();
        assert_eq!(shop().plan(april).unwrap(), usd("0"));
        let one_day = DateRange::day(date!(2026 - 05 - 11));
        assert_eq!(shop().plan(one_day).unwrap(), usd("7.50"));
    }

    #[test]
    fn the_markup_is_inside_the_amount() {
        let mut marked_up = shop();
        marked_up.markup_basis_points = 2_000;
        assert_eq!(marked_up.amount().unwrap(), usd("900"));
        assert_eq!(marked_up.plan(march()).unwrap(), usd("279"));
        assert_eq!(shop().amount().unwrap(), usd("750"));
        assert_eq!(marked_up.check(), Ok(()));
        marked_up.markup_basis_points = MAX_MARKUP.saturating_add(1);
        assert_eq!(
            marked_up.check(),
            Err(DistributionError::MarkupTooLarge(MAX_MARKUP))
        );
        let huge = Estimate {
            distribution: Distribution::point(Money::from_nanos(i64::MAX, Currency::USD)),
            markup_basis_points: 1,
            ..shop()
        };
        assert_eq!(huge.amount(), Err(MoneyError::Overflow));
    }

    #[test]
    fn bands_add_up_to_the_whole_and_rise() {
        let good = Distribution {
            median: usd("610"),
            bands: vec![
                band(Some("250"), 400),
                band(Some("500"), 3_100),
                band(Some("750"), 4_100),
                band(Some("1000"), 1_700),
                band(None, 700),
            ],
        };
        assert_eq!(good.check(), Ok(()));
        assert_eq!(good.currency(), Currency::USD);
        assert_eq!(Distribution::point(usd("1")).check(), Ok(()));
        let short = Distribution {
            median: usd("610"),
            bands: vec![band(Some("250"), 400), band(None, 600)],
        };
        assert_eq!(
            short.check(),
            Err(DistributionError::BandsDoNotAddUp(1_000))
        );
        let falling = Distribution {
            median: usd("610"),
            bands: vec![band(Some("500"), 5_000), band(Some("500"), 5_000)],
        };
        assert_eq!(
            falling.check(),
            Err(DistributionError::BandsOutOfOrder {
                earlier: usd("500"),
                later: usd("500"),
            })
        );
        let mixed = Distribution {
            median: usd("610"),
            bands: vec![
                Band {
                    up_to: Some(Money::zero(Currency::EUR)),
                    basis_points: 5_000,
                },
                band(None, 5_000),
            ],
        };
        assert!(matches!(
            mixed.check(),
            Err(DistributionError::CurrencyMismatch { .. })
        ));
        let estimate = Estimate {
            distribution: short,
            ..shop()
        };
        assert!(estimate.check().is_err());
    }

    #[test]
    fn estimates_read_from_json_with_a_median_alone() {
        let json = r#"{"description": "KD-1", "tags": {"feature": "KD-1"},
            "distribution": {"median": {"nanos": 500000000000, "currency": "USD"}},
            "estimator": "system-1",
            "range": {"from": "2026-03-01", "to": "2026-03-31"}}"#;
        let estimate: Estimate = serde_json::from_str(json).unwrap();
        assert_eq!(estimate.markup_basis_points, 0);
        assert!(estimate.distribution.bands.is_empty());
        assert_eq!(estimate.plan(march()).unwrap(), usd("500"));
        let accept = Accept {
            estimate: crate::event::EventId::new("est-1").unwrap(),
        };
        assert_eq!(
            serde_json::to_string(&accept).unwrap(),
            r#"{"estimate":"est-1"}"#
        );
    }
}
