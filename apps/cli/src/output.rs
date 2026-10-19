//! How things look: aligned tables for people, the JSON shapes scripts, the
//! Claude Code mod and the GitHub Action read, and dates and times in UTC.

use std::fmt::Write as _;
use std::io::Write;

use serde::Serialize;
use time::format_description::well_known::Rfc3339;
use time::macros::format_description;
use time::{Date, OffsetDateTime};
use tokencontroller_core::{Currency, Money, TagName, Tags};

use crate::error::Result;

/// Writes rows as columns padded to their widest cell. Columns listed in
/// `right` are aligned right.
pub fn table(out: &mut dyn Write, rows: &[Vec<String>], right: &[usize]) -> Result<()> {
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    let widths: Vec<usize> = (0..columns)
        .map(|column| {
            rows.iter()
                .filter_map(|row| row.get(column))
                .map(|cell| cell.chars().count())
                .max()
                .unwrap_or(0)
        })
        .collect();
    for row in rows {
        let mut line = String::from(" ");
        for (column, cell) in row.iter().enumerate() {
            let width = widths.get(column).copied().unwrap_or(0);
            let last = column.saturating_add(1) == row.len();
            // Writing to a String cannot fail.
            let _ignored = if right.contains(&column) {
                write!(line, " {cell:>width$}")
            } else if last {
                write!(line, " {cell}")
            } else {
                write!(line, " {cell:<width$}")
            };
        }
        writeln!(out, "{}", line.trim_end())?;
    }
    Ok(())
}

/// Prints a value as one line of JSON.
pub fn json<T: Serialize>(out: &mut dyn Write, value: &T) -> Result<()> {
    serde_json::to_writer(&mut *out, value)?;
    writeln!(out)?;
    Ok(())
}

/// A moment from milliseconds since the epoch, in UTC.
pub fn moment(at_ms: i64) -> Option<OffsetDateTime> {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(at_ms).saturating_mul(1_000_000)).ok()
}

/// Milliseconds since the epoch of a moment.
pub fn millis(at: OffsetDateTime) -> i64 {
    i64::try_from(at.unix_timestamp_nanos().saturating_div(1_000_000)).unwrap_or(0)
}

/// A date written as `2026-10-01`.
pub fn parse_date(text: &str) -> Option<Date> {
    Date::parse(text.trim(), &format_description!("[year]-[month]-[day]")).ok()
}

/// A time for the review list: `09:12` today, `Oct 11 09:12` before, in UTC.
pub fn when(at: OffsetDateTime, today: Date) -> String {
    let at = at.to_offset(time::UtcOffset::UTC);
    let format = if at.date() == today {
        format_description!("[hour]:[minute]")
    } else {
        format_description!("[month repr:short] [day] [hour]:[minute]")
    };
    at.format(format).unwrap_or_default()
}

/// An RFC 3339 time in UTC for JSON and CSV, to the second.
pub fn utc(at: OffsetDateTime) -> String {
    at.to_offset(time::UtcOffset::UTC)
        .replace_nanosecond(0)
        .ok()
        .and_then(|at| at.format(&Rfc3339).ok())
        .unwrap_or_default()
}

/// An amount rounded to the currency's minor unit, as a decimal string
/// without the currency, such as `41.87`. This is how money goes into JSON
/// and CSV.
pub fn decimal(amount: Money) -> String {
    let text = amount.to_string();
    // `Money` shows `41.87 USD`; keep the number.
    text.split_once(' ')
        .map_or(text.clone(), |(number, _)| number.to_owned())
}

/// An amount for people to read, such as `$41.87` or `€41.87`. Other
/// currencies show their code after the number.
pub fn display(amount: Money) -> String {
    let number = decimal(amount);
    let currency = amount.currency();
    let (sign, digits) = match number.strip_prefix('-') {
        Some(rest) => ("-", rest.to_owned()),
        None => ("", number.clone()),
    };
    if currency == Currency::USD {
        format!("{sign}${digits}")
    } else if currency == Currency::EUR {
        format!("{sign}€{digits}")
    } else {
        format!("{number} {currency}")
    }
}

/// Tags as the review list shows them: feature and project first, never the
/// person.
pub fn tag_line(tags: &Tags) -> String {
    let mut parts: Vec<String> = Vec::new();
    for first in [TagName::feature(), TagName::project()] {
        if let Some(value) = tags.get(&first) {
            parts.push(format!("{first} {value}"));
        }
    }
    parts.extend(
        tags.iter()
            .filter(|(name, _)| !name.is_built_in() && **name != TagName::person())
            .map(|(name, value)| format!("{name} {value}")),
    );
    for later in [TagName::client()] {
        if let Some(value) = tags.get(&later) {
            parts.push(format!("{later} {value}"));
        }
    }
    parts.join(" · ")
}

/// The share of `part` in `whole`, in whole percent rounded half up.
pub fn percent(part: i64, whole: i64) -> u64 {
    if whole <= 0 || part <= 0 {
        return 0;
    }
    let scaled = i128::from(part)
        .saturating_mul(200)
        .checked_div(i128::from(whole))
        .unwrap_or(0)
        .saturating_add(1)
        .checked_div(2)
        .unwrap_or(0);
    u64::try_from(scaled).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn tables_line_up() {
        let rows = vec![
            vec!["#".to_owned(), "cost".to_owned(), "tags".to_owned()],
            vec![
                "1".to_owned(),
                "$41.87".to_owned(),
                "feature KD-214".to_owned(),
            ],
            vec!["12".to_owned(), "$9.75".to_owned(), "team web".to_owned()],
        ];
        let mut out = Vec::new();
        table(&mut out, &rows, &[0, 1]).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(
            text,
            "   #   cost tags\n   1 $41.87 feature KD-214\n  12  $9.75 team web\n"
        );
    }

    #[test]
    fn tags_show_feature_and_project_first_and_never_the_person() {
        let tags = Tags::parse([
            "team=web",
            "feature=KD-214",
            "person=me",
            "project=storefront",
            "client=acme",
        ])
        .unwrap();
        assert_eq!(
            tag_line(&tags),
            "feature KD-214 · project storefront · team web · client acme"
        );
        assert_eq!(tag_line(&Tags::new()), "");
    }

    #[test]
    fn percents_round_half_up() {
        assert_eq!(percent(95, 100), 95);
        assert_eq!(percent(1, 3), 33);
        assert_eq!(percent(2, 3), 67);
        assert_eq!(percent(1, 200), 1);
        assert_eq!(percent(0, 10), 0);
        assert_eq!(percent(5, 0), 0);
        assert_eq!(percent(-5, 10), 0);
    }

    #[test]
    fn times_show_in_utc_for_machines_and_people() {
        let at = moment(1_760_173_920_000).unwrap();
        assert_eq!(utc(at), "2025-10-11T09:12:00Z");
        assert_eq!(millis(at), 1_760_173_920_000);
        assert_eq!(when(at, date!(2025 - 10 - 11)), "09:12");
        assert_eq!(when(at, date!(1999 - 01 - 01)), "Oct 11 09:12");
        assert_eq!(parse_date("2026-10-01"), Some(date!(2026 - 10 - 01)));
        assert_eq!(parse_date("yesterday"), None);
    }

    #[test]
    fn amounts_show_with_their_symbol() {
        let usd = |nanos| Money::from_nanos(nanos, Currency::USD);
        assert_eq!(decimal(usd(41_870_000_000)), "41.87");
        assert_eq!(display(usd(41_870_000_000)), "$41.87");
        assert_eq!(display(usd(-5_000_000_000)), "-$5.00");
        assert_eq!(
            display(Money::from_nanos(9_750_000_000, Currency::EUR)),
            "€9.75"
        );
        let yen = Currency::new("JPY").unwrap();
        assert_eq!(
            display(Money::from_nanos(1_000_000_000_000, yen)),
            "1000 JPY"
        );
    }

    #[test]
    fn json_is_one_line() {
        let mut out = Vec::new();
        json(&mut out, &serde_json::json!({"a": 1})).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "{\"a\":1}\n");
    }

    #[test]
    fn an_empty_table_prints_nothing_and_one_column_is_not_padded() {
        let mut out = Vec::new();
        table(&mut out, &[], &[]).unwrap();
        assert!(out.is_empty());
        let rows = vec![vec!["short".to_owned()], vec!["much longer".to_owned()]];
        table(&mut out, &rows, &[]).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "  short\n  much longer\n");
    }

    #[test]
    fn moments_outside_the_calendar_are_none() {
        assert!(moment(i64::MAX).is_none());
        assert_eq!(utc(moment(0).unwrap()), "1970-01-01T00:00:00Z");
        assert_eq!(decimal(Money::zero(Currency::USD)), "0.00");
        assert_eq!(display(Money::zero(Currency::EUR)), "€0.00");
    }
}
