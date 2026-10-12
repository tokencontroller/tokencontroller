//! A period: one calendar month, written `2026-09`. Periods follow UTC, so
//! a token entry belongs to the month in which it started, in UTC.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use time::{Date, Month, OffsetDateTime, Time};

/// Why a period could not be read or moved.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PeriodError {
    #[error("{0:?} is not a period: write the month as YYYY-MM, such as 2026-09")]
    Invalid(String),
    #[error("the period is outside the calendar Token Controller can hold")]
    OutOfRange,
}

/// One calendar month.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Period {
    first_day: Date,
}

impl Period {
    /// The month of `year` and `month`.
    ///
    /// # Errors
    ///
    /// [`PeriodError::OutOfRange`] for a year `time` cannot hold.
    pub fn new(year: i32, month: Month) -> Result<Self, PeriodError> {
        Date::from_calendar_date(year, month, 1)
            .map(|first_day| Self { first_day })
            .map_err(|_| PeriodError::OutOfRange)
    }

    /// The period a day falls in.
    #[must_use]
    pub fn of_date(date: Date) -> Self {
        Self {
            first_day: date.replace_day(1).unwrap_or(date),
        }
    }

    /// The period a moment falls in, in UTC.
    #[must_use]
    pub fn of(moment: OffsetDateTime) -> Self {
        Self::of_date(moment.to_offset(time::UtcOffset::UTC).date())
    }

    /// The period's first day, as the database stores it.
    #[must_use]
    pub const fn first_day(self) -> Date {
        self.first_day
    }

    /// The period's last day.
    #[must_use]
    pub fn last_day(self) -> Date {
        self.first_day
            .replace_day(self.first_day.month().length(self.first_day.year()))
            .unwrap_or(self.first_day)
    }

    /// The first moment of the period, midnight UTC on its first day.
    #[must_use]
    pub fn starts_at(self) -> OffsetDateTime {
        self.first_day.with_time(Time::MIDNIGHT).assume_utc()
    }

    /// The first moment after the period: midnight UTC on the next month's
    /// first day.
    ///
    /// # Errors
    ///
    /// [`PeriodError::OutOfRange`] at the end of the calendar.
    pub fn ends_at(self) -> Result<OffsetDateTime, PeriodError> {
        self.next().map(Self::starts_at)
    }

    /// The month after.
    ///
    /// # Errors
    ///
    /// [`PeriodError::OutOfRange`] at the end of the calendar.
    pub fn next(self) -> Result<Self, PeriodError> {
        self.last_day()
            .next_day()
            .map(|first_day| Self { first_day })
            .ok_or(PeriodError::OutOfRange)
    }

    /// The month before.
    ///
    /// # Errors
    ///
    /// [`PeriodError::OutOfRange`] at the start of the calendar.
    pub fn previous(self) -> Result<Self, PeriodError> {
        self.first_day
            .previous_day()
            .map(Self::of_date)
            .ok_or(PeriodError::OutOfRange)
    }

    /// Whether a moment falls in the period.
    #[must_use]
    pub fn contains(self, moment: OffsetDateTime) -> bool {
        Self::of(moment) == self
    }
}

impl FromStr for Period {
    type Err = PeriodError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let invalid = || PeriodError::Invalid(text.to_owned());
        let (year, month) = text.trim().split_once('-').ok_or_else(invalid)?;
        if year.len() != 4 || month.len() != 2 {
            return Err(invalid());
        }
        let year: i32 = year.parse().map_err(|_| invalid())?;
        let month: u8 = month.parse().map_err(|_| invalid())?;
        let month = Month::try_from(month).map_err(|_| invalid())?;
        Self::new(year, month).map_err(|_| invalid())
    }
}

impl TryFrom<String> for Period {
    type Error = PeriodError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        text.parse()
    }
}

impl From<Period> for String {
    fn from(period: Period) -> Self {
        period.to_string()
    }
}

impl fmt::Display for Period {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:04}-{:02}",
            self.first_day.year(),
            u8::from(self.first_day.month())
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::{date, datetime};

    #[test]
    fn reads_and_writes_year_and_month() {
        let period: Period = "2026-09".parse().unwrap();
        assert_eq!(period.to_string(), "2026-09");
        assert_eq!(period.first_day(), date!(2026 - 09 - 01));
        assert_eq!(period.last_day(), date!(2026 - 09 - 30));
    }

    #[test]
    fn refuses_anything_else() {
        for text in ["2026-9", "26-09", "2026-13", "2026-00", "2026/09", "", "2026-09-01"] {
            assert!(text.parse::<Period>().is_err(), "{text} should not parse");
        }
    }

    #[test]
    fn february_ends_on_the_right_day() {
        let leap: Period = "2028-02".parse().unwrap();
        assert_eq!(leap.last_day(), date!(2028 - 02 - 29));
        let plain: Period = "2026-02".parse().unwrap();
        assert_eq!(plain.last_day(), date!(2026 - 02 - 28));
    }

    #[test]
    fn moves_across_the_year() {
        let december: Period = "2026-12".parse().unwrap();
        assert_eq!(december.next().unwrap().to_string(), "2027-01");
        let january: Period = "2027-01".parse().unwrap();
        assert_eq!(january.previous().unwrap().to_string(), "2026-12");
    }

    #[test]
    fn a_moment_belongs_to_its_utc_month() {
        let period: Period = "2026-09".parse().unwrap();
        assert!(period.contains(datetime!(2026-09-01 00:00 UTC)));
        assert!(period.contains(datetime!(2026-09-30 23:59:59 UTC)));
        assert!(!period.contains(datetime!(2026-10-01 00:00 UTC)));
        // 01:30 in Vienna on 1 October is still 30 September in UTC.
        assert!(period.contains(datetime!(2026-10-01 01:30 +02:00)));
        assert_eq!(period.starts_at(), datetime!(2026-09-01 00:00 UTC));
        assert_eq!(period.ends_at().unwrap(), datetime!(2026-10-01 00:00 UTC));
    }

    #[test]
    fn serializes_as_text() {
        let period: Period = serde_json::from_str("\"2026-09\"").unwrap();
        assert_eq!(serde_json::to_string(&period).unwrap(), "\"2026-09\"");
        assert!(serde_json::from_str::<Period>("\"September\"").is_err());
    }
}
