//! Periods as the database stores them. The period itself is the core's
//! [`Period`]: one calendar month in UTC, written `2026-09` (ADR 0015). The
//! database keeps a period as its first day, and queries need its bounds
//! as moments; [`PeriodDates`] adds those.

use time::{Date, Month, OffsetDateTime, Time};
pub use tokencontroller_core::Period;

// `day`: a day in JSON as `2026-10-12`, for `#[serde(with = ...)]` on
// `Date` fields; `day::option` does the same for an `Option<Date>`.
time::serde::format_description!(pub day, Date, "[year]-[month]-[day]");

/// A period at the end of the calendar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the period is outside the calendar Token Controller can hold")]
pub struct OutOfRange;

/// A period's days and moments.
pub trait PeriodDates: Sized {
    /// The period a day falls in.
    #[must_use]
    fn of_date(date: Date) -> Self;

    /// The period's first day, as the database stores it.
    #[must_use]
    fn first_day(self) -> Date;

    /// The period's last day.
    #[must_use]
    fn last_day(self) -> Date;

    /// The first moment of the period, midnight UTC on its first day.
    #[must_use]
    fn starts_at(self) -> OffsetDateTime;

    /// The first moment after the period: midnight UTC on the next month's
    /// first day.
    ///
    /// # Errors
    ///
    /// [`OutOfRange`] at the end of the calendar.
    fn ends_at(self) -> Result<OffsetDateTime, OutOfRange>;

    /// The month before.
    ///
    /// # Errors
    ///
    /// [`OutOfRange`] at the start of the calendar.
    fn previous(self) -> Result<Self, OutOfRange>;

    /// The month after.
    ///
    /// # Errors
    ///
    /// [`OutOfRange`] at the end of the calendar.
    fn following(self) -> Result<Self, OutOfRange>;
}

impl PeriodDates for Period {
    fn of_date(date: Date) -> Self {
        Self::containing(date.with_time(Time::MIDNIGHT).assume_utc())
    }

    fn first_day(self) -> Date {
        Date::from_calendar_date(self.year(), self.month(), 1).unwrap_or(Date::MIN)
    }

    fn last_day(self) -> Date {
        Date::from_calendar_date(self.year(), self.month(), self.month().length(self.year()))
            .unwrap_or(Date::MAX)
    }

    fn starts_at(self) -> OffsetDateTime {
        self.first_day().with_time(Time::MIDNIGHT).assume_utc()
    }

    fn ends_at(self) -> Result<OffsetDateTime, OutOfRange> {
        self.following().map(PeriodDates::starts_at)
    }

    fn previous(self) -> Result<Self, OutOfRange> {
        let (year, month) = match self.month() {
            Month::January => (
                self.year().checked_sub(1).ok_or(OutOfRange)?,
                Month::December,
            ),
            month => (self.year(), month.previous()),
        };
        Self::new(year, month).map_err(|_| OutOfRange)
    }

    fn following(self) -> Result<Self, OutOfRange> {
        self.next().ok_or(OutOfRange)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::{date, datetime};

    fn period(text: &str) -> Period {
        Period::parse(text).unwrap()
    }

    #[test]
    fn reads_and_writes_year_and_month() {
        let period = period("2026-09");
        assert_eq!(period.to_string(), "2026-09");
        assert_eq!(period.first_day(), date!(2026 - 09 - 01));
        assert_eq!(period.last_day(), date!(2026 - 09 - 30));
        assert_eq!(Period::of_date(date!(2026 - 09 - 17)), period);
    }

    #[test]
    fn refuses_anything_else() {
        for text in [
            "2026-9",
            "26-09",
            "2026-13",
            "2026-00",
            "2026/09",
            "",
            "2026-09-01",
        ] {
            assert!(Period::parse(text).is_err(), "{text} should not parse");
        }
    }

    #[test]
    fn february_ends_on_the_right_day() {
        assert_eq!(period("2028-02").last_day(), date!(2028 - 02 - 29));
        assert_eq!(period("2026-02").last_day(), date!(2026 - 02 - 28));
    }

    #[test]
    fn moves_across_the_year() {
        assert_eq!(
            period("2026-12").following().unwrap().to_string(),
            "2027-01"
        );
        assert_eq!(period("2027-01").previous().unwrap().to_string(), "2026-12");
        assert_eq!(period("2026-06").previous().unwrap().to_string(), "2026-05");
        assert!(period("0001-01").previous().is_err());
        assert!(period("9999-12").following().is_err());
    }

    #[test]
    fn a_moment_belongs_to_its_utc_month() {
        let period = period("2026-09");
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
