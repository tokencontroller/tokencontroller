//! Cadence: how often token sheets are due. Weekly, every two weeks and
//! custom cadences are runs of whole days counted from the day the cadence
//! starts; a monthly cadence follows calendar months.

use serde::{Deserialize, Serialize};
use time::{Date, Duration};
use utoipa::ToSchema;

/// How often token sheets are due.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CadenceKind {
    Weekly,
    EveryTwoWeeks,
    Monthly,
    /// Every `every_days` days.
    Custom,
}

impl CadenceKind {
    /// The name the database and the API use.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Weekly => "weekly",
            Self::EveryTwoWeeks => "every_two_weeks",
            Self::Monthly => "monthly",
            Self::Custom => "custom",
        }
    }

    /// Reads the name the database and the API use.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "weekly" => Some(Self::Weekly),
            "every_two_weeks" => Some(Self::EveryTwoWeeks),
            "monthly" => Some(Self::Monthly),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }
}

/// Why a cadence is not valid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CadenceError {
    #[error("a custom cadence needs a number of days between 1 and 366")]
    Days,
    #[error("the date is outside the calendar Token Controller can hold")]
    OutOfRange,
}

/// The longest custom cadence, in days.
pub const MAX_CUSTOM_DAYS: u16 = 366;

/// A cadence: its kind, and for runs of days, the day the first one starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cadence {
    kind: CadenceKind,
    days: u16,
    starts_on: Date,
}

/// One stretch of a cadence: from `starts_on` up to and including `ends_on`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, ToSchema)]
pub struct CadencePeriod {
    #[schema(value_type = String, format = Date)]
    pub starts_on: Date,
    #[schema(value_type = String, format = Date)]
    pub ends_on: Date,
}

impl Cadence {
    /// A cadence of `kind` whose first stretch starts on `starts_on`.
    /// `custom_days` is read only for a custom cadence.
    ///
    /// # Errors
    ///
    /// [`CadenceError::Days`] for a custom cadence without a sensible number
    /// of days.
    pub fn new(
        kind: CadenceKind,
        custom_days: Option<u16>,
        starts_on: Date,
    ) -> Result<Self, CadenceError> {
        let days = match kind {
            CadenceKind::Weekly => 7,
            CadenceKind::EveryTwoWeeks => 14,
            CadenceKind::Monthly => 0,
            CadenceKind::Custom => match custom_days {
                Some(days) if (1..=MAX_CUSTOM_DAYS).contains(&days) => days,
                _ => return Err(CadenceError::Days),
            },
        };
        Ok(Self {
            kind,
            days,
            starts_on,
        })
    }

    #[must_use]
    pub const fn kind(self) -> CadenceKind {
        self.kind
    }

    /// The length of each stretch in days, for every kind but monthly.
    #[must_use]
    pub fn days(self) -> Option<u16> {
        (self.kind != CadenceKind::Monthly).then_some(self.days)
    }

    #[must_use]
    pub const fn starts_on(self) -> Date {
        self.starts_on
    }

    /// The cadence period `day` falls in. Days before the cadence started
    /// fall in the stretches that would have come before it.
    ///
    /// # Errors
    ///
    /// [`CadenceError::OutOfRange`] at the ends of the calendar.
    pub fn period_containing(self, day: Date) -> Result<CadencePeriod, CadenceError> {
        if self.kind == CadenceKind::Monthly {
            let starts_on = day.replace_day(1).map_err(|_| CadenceError::OutOfRange)?;
            let ends_on = day
                .replace_day(day.month().length(day.year()))
                .map_err(|_| CadenceError::OutOfRange)?;
            return Ok(CadencePeriod { starts_on, ends_on });
        }
        let length = i64::from(self.days);
        let offset = i64::from(day.to_julian_day())
            .checked_sub(i64::from(self.starts_on.to_julian_day()))
            .ok_or(CadenceError::OutOfRange)?;
        let index = offset
            .checked_div_euclid(length)
            .ok_or(CadenceError::OutOfRange)?;
        let start_offset = index.checked_mul(length).ok_or(CadenceError::OutOfRange)?;
        let starts_on = self
            .starts_on
            .checked_add(Duration::days(start_offset))
            .ok_or(CadenceError::OutOfRange)?;
        let ends_on = starts_on
            .checked_add(Duration::days(
                length.checked_sub(1).ok_or(CadenceError::OutOfRange)?,
            ))
            .ok_or(CadenceError::OutOfRange)?;
        Ok(CadencePeriod { starts_on, ends_on })
    }

    /// Every cadence period that ended on or after `from` and before `today`,
    /// oldest first: the ones a token sheet may be late for.
    ///
    /// # Errors
    ///
    /// [`CadenceError::OutOfRange`] at the ends of the calendar.
    pub fn ended_between(
        self,
        from: Date,
        today: Date,
    ) -> Result<Vec<CadencePeriod>, CadenceError> {
        let mut periods = Vec::new();
        let mut current = self.period_containing(from)?;
        while current.ends_on < today {
            periods.push(current);
            let next_day = current.ends_on.next_day().ok_or(CadenceError::OutOfRange)?;
            current = self.period_containing(next_day)?;
        }
        Ok(periods)
    }
}

impl CadencePeriod {
    /// Whether `day` falls in the cadence period.
    #[must_use]
    pub fn contains(self, day: Date) -> bool {
        self.starts_on <= day && day <= self.ends_on
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    fn period(starts_on: Date, ends_on: Date) -> CadencePeriod {
        CadencePeriod { starts_on, ends_on }
    }

    #[test]
    fn weekly_runs_seven_days_from_its_start() {
        let cadence = Cadence::new(CadenceKind::Weekly, None, date!(2026 - 09 - 07)).unwrap();
        assert_eq!(
            cadence.period_containing(date!(2026 - 09 - 07)).unwrap(),
            period(date!(2026 - 09 - 07), date!(2026 - 09 - 13))
        );
        assert_eq!(
            cadence.period_containing(date!(2026 - 09 - 13)).unwrap(),
            period(date!(2026 - 09 - 07), date!(2026 - 09 - 13))
        );
        assert_eq!(
            cadence.period_containing(date!(2026 - 09 - 14)).unwrap(),
            period(date!(2026 - 09 - 14), date!(2026 - 09 - 20))
        );
    }

    #[test]
    fn days_before_the_start_fall_in_earlier_stretches() {
        let cadence =
            Cadence::new(CadenceKind::EveryTwoWeeks, None, date!(2026 - 09 - 07)).unwrap();
        assert_eq!(
            cadence.period_containing(date!(2026 - 09 - 06)).unwrap(),
            period(date!(2026 - 08 - 24), date!(2026 - 09 - 06))
        );
        assert_eq!(
            cadence.period_containing(date!(2026 - 08 - 24)).unwrap(),
            period(date!(2026 - 08 - 24), date!(2026 - 09 - 06))
        );
    }

    #[test]
    fn monthly_follows_the_calendar() {
        let cadence = Cadence::new(CadenceKind::Monthly, Some(3), date!(2026 - 01 - 15)).unwrap();
        assert_eq!(cadence.days(), None);
        assert_eq!(
            cadence.period_containing(date!(2026 - 02 - 10)).unwrap(),
            period(date!(2026 - 02 - 01), date!(2026 - 02 - 28))
        );
    }

    #[test]
    fn a_custom_cadence_needs_its_days() {
        let start = date!(2026 - 09 - 01);
        assert_eq!(
            Cadence::new(CadenceKind::Custom, None, start),
            Err(CadenceError::Days)
        );
        assert_eq!(
            Cadence::new(CadenceKind::Custom, Some(0), start),
            Err(CadenceError::Days)
        );
        assert_eq!(
            Cadence::new(CadenceKind::Custom, Some(367), start),
            Err(CadenceError::Days)
        );
        let one = Cadence::new(CadenceKind::Custom, Some(1), start).unwrap();
        assert_eq!(
            one.period_containing(date!(2026 - 09 - 05)).unwrap(),
            period(date!(2026 - 09 - 05), date!(2026 - 09 - 05))
        );
        let longest = Cadence::new(CadenceKind::Custom, Some(366), start).unwrap();
        assert_eq!(longest.days(), Some(366));
    }

    #[test]
    fn lists_the_periods_that_are_over() {
        let cadence = Cadence::new(CadenceKind::Weekly, None, date!(2026 - 09 - 07)).unwrap();
        let ended = cadence
            .ended_between(date!(2026 - 09 - 08), date!(2026 - 09 - 21))
            .unwrap();
        assert_eq!(
            ended,
            vec![
                period(date!(2026 - 09 - 07), date!(2026 - 09 - 13)),
                period(date!(2026 - 09 - 14), date!(2026 - 09 - 20)),
            ]
        );
        // On its last day a period is not over yet.
        let none = cadence
            .ended_between(date!(2026 - 09 - 08), date!(2026 - 09 - 13))
            .unwrap();
        assert!(none.is_empty());
    }

    #[test]
    fn kinds_round_trip_through_their_names() {
        for kind in [
            CadenceKind::Weekly,
            CadenceKind::EveryTwoWeeks,
            CadenceKind::Monthly,
            CadenceKind::Custom,
        ] {
            assert_eq!(CadenceKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(CadenceKind::parse("daily"), None);
    }

    #[test]
    fn a_period_contains_both_ends() {
        let week = period(date!(2026 - 09 - 07), date!(2026 - 09 - 13));
        assert!(week.contains(date!(2026 - 09 - 07)));
        assert!(week.contains(date!(2026 - 09 - 13)));
        assert!(!week.contains(date!(2026 - 09 - 06)));
        assert!(!week.contains(date!(2026 - 09 - 14)));
    }
}
