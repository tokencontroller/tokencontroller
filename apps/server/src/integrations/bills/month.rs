//! One calendar month, the range a provider's bill covers, and the way the
//! provider charges for it.

use std::fmt;

use serde::{Deserialize, Serialize};
use time::{Date, Month, OffsetDateTime, UtcOffset};
use tokencontroller_core::DateRange;

/// A month is not written as `2026-09`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is not a month: use the year and month, such as 2026-09")]
pub struct MonthError(pub String);

/// One calendar month, in UTC, such as `2026-09`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct BillMonth {
    year: i32,
    month: Month,
}

impl BillMonth {
    /// The month of a year.
    ///
    /// # Errors
    ///
    /// [`MonthError`] if the year is outside 1 to 9999.
    pub fn new(year: i32, month: Month) -> Result<Self, MonthError> {
        if (1..=9999).contains(&year) {
            Ok(Self { year, month })
        } else {
            Err(MonthError(format!("{year}-{:02}", u8::from(month))))
        }
    }

    /// Reads a month written as `YYYY-MM`.
    ///
    /// # Errors
    ///
    /// [`MonthError`] for anything else.
    pub fn parse(text: &str) -> Result<Self, MonthError> {
        let invalid = || MonthError(text.to_owned());
        let (year, month) = text.split_once('-').ok_or_else(invalid)?;
        let digits = |part: &str, length: usize| {
            part.len() == length && part.bytes().all(|byte| byte.is_ascii_digit())
        };
        if !digits(year, 4) || !digits(month, 2) {
            return Err(invalid());
        }
        let year = year.parse::<i32>().map_err(|_| invalid())?;
        let month = month
            .parse::<u8>()
            .ok()
            .and_then(|month| Month::try_from(month).ok())
            .ok_or_else(invalid)?;
        Self::new(year, month).map_err(|_| invalid())
    }

    /// The month a moment falls in, counted in UTC.
    #[must_use]
    pub fn containing(moment: OffsetDateTime) -> Self {
        let utc = moment.to_offset(UtcOffset::UTC);
        Self {
            year: utc.year(),
            month: utc.month(),
        }
    }

    #[must_use]
    pub const fn year(self) -> i32 {
        self.year
    }

    #[must_use]
    pub const fn month(self) -> Month {
        self.month
    }

    /// The month after this one, or `None` after December 9999.
    #[must_use]
    pub fn next(self) -> Option<Self> {
        match self.month {
            Month::December => Self::new(self.year.checked_add(1)?, Month::January).ok(),
            month => Some(Self {
                year: self.year,
                month: month.next(),
            }),
        }
    }

    /// The month before this one, or `None` before January 1.
    #[must_use]
    pub fn previous(self) -> Option<Self> {
        match self.month {
            Month::January => Self::new(self.year.checked_sub(1)?, Month::December).ok(),
            month => Some(Self {
                year: self.year,
                month: month.previous(),
            }),
        }
    }

    /// Whether a moment falls in this month.
    #[must_use]
    pub fn contains(self, moment: OffsetDateTime) -> bool {
        Self::containing(moment) == self
    }

    /// The first day of the month.
    #[must_use]
    pub fn first_day(self) -> Option<Date> {
        Date::from_calendar_date(self.year, self.month, 1).ok()
    }

    /// The last day of the month.
    #[must_use]
    pub fn last_day(self) -> Option<Date> {
        let days = time::util::days_in_month(self.month, self.year);
        Date::from_calendar_date(self.year, self.month, days).ok()
    }

    /// The month's days as a date range: what a bill for the month covers.
    #[must_use]
    pub fn range(self) -> Option<DateRange> {
        DateRange::new(self.first_day()?, self.last_day()?).ok()
    }
}

impl fmt::Display for BillMonth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}", self.year, u8::from(self.month))
    }
}

impl TryFrom<String> for BillMonth {
    type Error = MonthError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::parse(&text)
    }
}

impl From<BillMonth> for String {
    fn from(month: BillMonth) -> Self {
        month.to_string()
    }
}

/// How a provider charges for what a bill covers: the metered ways. Seats
/// and subscriptions are flat fees and have no bill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Charging {
    UsageAtApiRates,
    ApiKey,
    CloudMarketplace,
    Reseller,
}

impl Charging {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UsageAtApiRates => "usage_at_api_rates",
            Self::ApiKey => "api_key",
            Self::CloudMarketplace => "cloud_marketplace",
            Self::Reseller => "reseller",
        }
    }

    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        [
            Self::UsageAtApiRates,
            Self::ApiKey,
            Self::CloudMarketplace,
            Self::Reseller,
        ]
        .into_iter()
        .find(|charging| charging.as_str() == text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::{date, datetime};

    #[test]
    fn months_read_and_write_as_year_and_month() {
        let month = BillMonth::parse("2026-09").unwrap();
        assert_eq!(month.year(), 2026);
        assert_eq!(month.month(), Month::September);
        assert_eq!(month.to_string(), "2026-09");
        for bad in [
            "2026-9", "26-09", "2026-13", "2026-00", "2026/09", "", "0000-01",
        ] {
            assert!(BillMonth::parse(bad).is_err(), "{bad}");
        }
        assert!(BillMonth::new(0, Month::May).is_err());
        let json = serde_json::to_string(&month).unwrap();
        assert_eq!(json, "\"2026-09\"");
        assert_eq!(serde_json::from_str::<BillMonth>(&json).unwrap(), month);
    }

    #[test]
    fn a_month_has_its_days_and_follows_the_last() {
        let february = BillMonth::parse("2024-02").unwrap();
        assert_eq!(february.first_day(), Some(date!(2024 - 02 - 01)));
        assert_eq!(february.last_day(), Some(date!(2024 - 02 - 29)));
        assert_eq!(february.range().unwrap().days(), 29);
        assert_eq!(
            BillMonth::parse("2026-12")
                .unwrap()
                .next()
                .unwrap()
                .to_string(),
            "2027-01"
        );
        assert_eq!(BillMonth::parse("9999-12").unwrap().next(), None);
        assert_eq!(
            BillMonth::parse("2027-01")
                .unwrap()
                .previous()
                .unwrap()
                .to_string(),
            "2026-12"
        );
        assert_eq!(BillMonth::parse("0001-01").unwrap().previous(), None);
        assert_eq!(
            BillMonth::containing(datetime!(2026-10-01 00:30 +02:00)).to_string(),
            "2026-09"
        );
        assert!(february.contains(datetime!(2024-02-29 23:59 UTC)));
        assert_eq!(Charging::parse("api_key"), Some(Charging::ApiKey));
        assert_eq!(Charging::UsageAtApiRates.as_str(), "usage_at_api_rates");
    }
}
