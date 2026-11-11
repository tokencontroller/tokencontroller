//! Dates on the wire: `YYYY-MM-DD` in UTC, date ranges and the `as_of`
//! moment every list and report takes.

use time::format_description::well_known::{Iso8601, Rfc3339};
use time::{Date, OffsetDateTime};
use tokencontroller_core::DateRange;

use crate::error::ApiError;

time::serde::format_description!(pub day, Date, "[year]-[month]-[day]");

/// `Option<Date>` as `YYYY-MM-DD` or `null`.
pub mod option_day {
    use serde::{Deserialize as _, Deserializer, Serialize as _, Serializer};
    use time::Date;

    /// # Errors
    ///
    /// Never; the date formats.
    pub fn serialize<S: Serializer>(date: &Option<Date>, serializer: S) -> Result<S::Ok, S::Error> {
        match date {
            Some(date) => super::text(*date).serialize(serializer),
            None => serializer.serialize_none(),
        }
    }

    /// # Errors
    ///
    /// When the text is not a date.
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Date>, D::Error> {
        let text: Option<String> = Option::deserialize(deserializer)?;
        text.map(|text| super::parse(&text).map_err(serde::de::Error::custom))
            .transpose()
    }
}

/// A date as `YYYY-MM-DD`.
#[must_use]
pub fn text(date: Date) -> String {
    date.format(&Iso8601::DATE).unwrap_or_default()
}

/// Reads `YYYY-MM-DD`.
///
/// # Errors
///
/// [`ApiError::BadRequest`] for anything else.
pub fn parse(text: &str) -> Result<Date, ApiError> {
    let day = text.trim();
    let wrong = || ApiError::BadRequest(format!("{text:?} is not a date; write it as YYYY-MM-DD"));
    if day.len() != "YYYY-MM-DD".len() {
        return Err(wrong());
    }
    Date::parse(day, &Iso8601::DATE).map_err(|_| wrong())
}

/// Reads an optional `YYYY-MM-DD`.
///
/// # Errors
///
/// [`ApiError::BadRequest`] for text that is not a date.
pub fn parse_optional(text: Option<&str>) -> Result<Option<Date>, ApiError> {
    text.map(str::trim)
        .filter(|text| !text.is_empty())
        .map(parse)
        .transpose()
}

/// A date range from two `YYYY-MM-DD` texts, both ends included.
///
/// # Errors
///
/// [`ApiError::BadRequest`] when a date is missing or wrong, or `to` is
/// before `from`.
pub fn range(from: Option<&str>, to: Option<&str>) -> Result<DateRange, ApiError> {
    let from = parse(from.ok_or_else(|| ApiError::BadRequest("from is required".to_owned()))?)?;
    let to = parse(to.ok_or_else(|| ApiError::BadRequest("to is required".to_owned()))?)?;
    DateRange::new(from, to).map_err(|error| ApiError::BadRequest(error.to_string()))
}

/// A date range for a list: a missing `from` is the beginning of time and a
/// missing `to` its end, so a list without either shows everything.
///
/// # Errors
///
/// [`ApiError::BadRequest`] when a date is wrong, or `to` is before `from`.
pub fn range_or_everything(from: Option<&str>, to: Option<&str>) -> Result<DateRange, ApiError> {
    let from = parse_optional(from)?.unwrap_or(Date::MIN);
    let to = parse_optional(to)?.unwrap_or(Date::MAX);
    range_of(from, to)
}

/// A date range from two dates.
///
/// # Errors
///
/// [`ApiError::BadRequest`] when `to` is before `from`.
pub fn range_of(from: Date, to: Date) -> Result<DateRange, ApiError> {
    DateRange::new(from, to).map_err(|error| ApiError::BadRequest(error.to_string()))
}

/// The moment a list or report is as of: the end of the `as_of` day in UTC,
/// or now when it is left out. The day's events count, the next day's do
/// not.
///
/// # Errors
///
/// [`ApiError::BadRequest`] for text that is not a date.
pub fn as_of(text: Option<&str>) -> Result<OffsetDateTime, ApiError> {
    let Some(text) = text.map(str::trim).filter(|text| !text.is_empty()) else {
        return Ok(OffsetDateTime::now_utc());
    };
    if let Ok(moment) = OffsetDateTime::parse(text, &Rfc3339) {
        return Ok(moment);
    }
    parse(text).map(end_of).map_err(|_| {
        ApiError::BadRequest(format!(
            "{text:?} is not a moment; write a day as YYYY-MM-DD or a time as RFC 3339"
        ))
    })
}

/// The last moment of a day in UTC.
#[must_use]
pub fn end_of(date: Date) -> OffsetDateTime {
    date.next_day().map_or_else(
        || OffsetDateTime::new_utc(date, time::Time::MIDNIGHT),
        |next| {
            OffsetDateTime::new_utc(next, time::Time::MIDNIGHT)
                .saturating_sub(time::Duration::nanoseconds(1))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::{date, datetime};

    #[test]
    fn dates_read_and_write_as_iso_days() {
        assert_eq!(parse("2026-03-01").unwrap(), date!(2026 - 03 - 01));
        assert_eq!(parse(" 2026-03-01 ").unwrap(), date!(2026 - 03 - 01));
        assert_eq!(text(date!(2026 - 03 - 01)), "2026-03-01");
        assert!(parse("03/01/2026").is_err());
        assert_eq!(parse_optional(None).unwrap(), None);
        assert_eq!(parse_optional(Some("  ")).unwrap(), None);
        assert_eq!(
            parse_optional(Some("2026-03-01")).unwrap(),
            Some(date!(2026 - 03 - 01))
        );
    }

    #[test]
    fn a_range_needs_both_ends_in_order() {
        let march = range(Some("2026-03-01"), Some("2026-03-31")).unwrap();
        assert_eq!(march.days(), 31);
        assert!(range(None, Some("2026-03-31")).is_err());
        assert!(range(Some("2026-03-31"), Some("2026-03-01")).is_err());
    }

    #[test]
    fn a_list_without_ends_shows_everything() {
        let all = range_or_everything(None, None).unwrap();
        assert!(all.contains(date!(2026 - 03 - 01)));
        let from_march = range_or_everything(Some("2026-03-01"), None).unwrap();
        assert!(from_march.contains(date!(2030 - 01 - 01)));
        assert!(!from_march.contains(date!(2026 - 02 - 28)));
        let to_march = range_or_everything(None, Some("2026-03-31")).unwrap();
        assert!(to_march.contains(date!(2020 - 01 - 01)));
        assert!(!to_march.contains(date!(2026 - 04 - 01)));
        assert!(range_or_everything(Some("2026-03-31"), Some("2026-03-01")).is_err());
        assert!(range_or_everything(Some("March"), None).is_err());
    }

    #[test]
    fn as_of_is_the_end_of_the_day() {
        assert_eq!(
            as_of(Some("2026-03-31")).unwrap(),
            datetime!(2026-03-31 23:59:59.999999999 UTC)
        );
        assert!(as_of(None).unwrap() <= OffsetDateTime::now_utc());
        assert!(as_of(Some("yesterday")).is_err());
    }

    #[test]
    fn as_of_takes_a_moment_as_it_is() {
        assert_eq!(
            as_of(Some("2026-03-12T15:00:00Z")).unwrap(),
            datetime!(2026-03-12 15:00 UTC)
        );
        assert_eq!(
            as_of(Some(" 2026-03-12T15:00:00+02:00 ")).unwrap(),
            datetime!(2026-03-12 13:00 UTC)
        );
        assert!(as_of(Some("2026-03-12T15:00")).is_err());
    }

    #[test]
    fn the_end_of_a_day_is_just_before_the_next() {
        assert_eq!(
            end_of(date!(2026 - 02 - 28)),
            datetime!(2026-02-28 23:59:59.999999999 UTC)
        );
        assert_eq!(
            end_of(Date::MAX),
            OffsetDateTime::new_utc(Date::MAX, time::Time::MIDNIGHT)
        );
    }

    #[test]
    fn a_date_with_a_time_is_not_a_date() {
        assert!(parse("2026-03-12T15:00:00Z").is_err());
    }

    #[test]
    fn optional_days_serialize_as_text_or_null() {
        #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
        struct Holder {
            #[serde(with = "option_day")]
            day: Option<Date>,
        }
        let some = Holder {
            day: Some(date!(2026 - 03 - 01)),
        };
        let json = serde_json::to_string(&some).unwrap();
        assert_eq!(json, r#"{"day":"2026-03-01"}"#);
        assert_eq!(serde_json::from_str::<Holder>(&json).unwrap(), some);
        let none: Holder = serde_json::from_str(r#"{"day":null}"#).unwrap();
        assert_eq!(none.day, None);
        assert!(serde_json::from_str::<Holder>(r#"{"day":"soon"}"#).is_err());
    }
}
