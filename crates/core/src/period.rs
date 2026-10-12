//! Periods: one calendar month each, open or closed.

use std::fmt;

use serde::{Deserialize, Serialize};
use time::{Month, OffsetDateTime, UtcOffset};

/// A period is not a calendar month in the form `2026-09`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is not a period: use the year and month, such as 2026-09")]
pub struct PeriodError(pub String);

/// One calendar month, in UTC, such as `2026-09`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Period {
    year: i32,
    month: Month,
}

impl Period {
    /// The period of a year and a month.
    ///
    /// # Errors
    ///
    /// [`PeriodError`] if the year is outside 1 to 9999.
    pub fn new(year: i32, month: Month) -> Result<Self, PeriodError> {
        if (1..=9999).contains(&year) {
            Ok(Self { year, month })
        } else {
            Err(PeriodError(format!("{year}-{:02}", u8::from(month))))
        }
    }

    /// Reads a period written as `YYYY-MM`.
    ///
    /// # Errors
    ///
    /// [`PeriodError`] for anything else.
    pub fn parse(text: &str) -> Result<Self, PeriodError> {
        let invalid = || PeriodError(text.to_owned());
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

    /// The period a moment falls in, counted in UTC.
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

    /// The period after this one, or `None` after December 9999.
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

    /// Whether a moment falls in this period.
    #[must_use]
    pub fn contains(self, moment: OffsetDateTime) -> bool {
        Self::containing(moment) == self
    }
}

impl fmt::Display for Period {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}", self.year, u8::from(self.month))
    }
}

impl TryFrom<String> for Period {
    type Error = PeriodError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::parse(&text)
    }
}

impl From<Period> for String {
    fn from(period: Period) -> Self {
        period.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn periods_read_and_write_as_year_and_month() {
        let period = Period::parse("2026-09").unwrap();
        assert_eq!(period.year(), 2026);
        assert_eq!(period.month(), Month::September);
        assert_eq!(period.to_string(), "2026-09");
        assert_eq!(Period::parse("0001-01").unwrap().to_string(), "0001-01");
        for bad in [
            "2026-9",
            "26-09",
            "2026-13",
            "2026-00",
            "2026/09",
            "2026-09-01",
            "",
            "-2026-09",
            "+026-09",
            "0000-01",
            "２０２６-09",
        ] {
            assert!(Period::parse(bad).is_err(), "{bad}");
        }
        assert!(Period::new(0, Month::May).is_err());
        assert!(Period::new(10_000, Month::May).is_err());
        assert!(Period::new(9999, Month::May).is_ok());
        assert!(Period::new(1, Month::May).is_ok());
    }

    #[test]
    fn periods_read_and_write_as_json_strings() {
        let period = Period::parse("2026-10").unwrap();
        let json = serde_json::to_string(&period).unwrap();
        assert_eq!(json, "\"2026-10\"");
        assert_eq!(serde_json::from_str::<Period>(&json).unwrap(), period);
        assert!(serde_json::from_str::<Period>("\"October\"").is_err());
    }

    #[test]
    fn a_moment_belongs_to_its_month_in_utc() {
        let period = Period::parse("2026-09").unwrap();
        assert_eq!(
            Period::containing(datetime!(2026-09-30 23:59:59 UTC)),
            period
        );
        // 1 October 00:30 in Vienna is still 30 September in UTC.
        assert_eq!(
            Period::containing(datetime!(2026-10-01 00:30 +02:00)),
            period
        );
        assert!(!period.contains(datetime!(2026-10-01 00:00 UTC)));
        assert!(period.contains(datetime!(2026-09-01 00:00 UTC)));
    }

    #[test]
    fn periods_follow_each_other_across_years() {
        let november = Period::parse("2026-11").unwrap();
        let december = november.next().unwrap();
        assert_eq!(december.to_string(), "2026-12");
        assert_eq!(december.next().unwrap().to_string(), "2027-01");
        assert!(november < december);
        assert_eq!(Period::parse("9999-12").unwrap().next(), None);
    }
}
