//! How things look: aligned tables for people, and the JSON shapes scripts,
//! the Claude Code mod and the GitHub Action read.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write;

use serde::Serialize;

use crate::amounts::{decimal, display};
use crate::entries::{CURRENCY, Entry};
use crate::error::Result;
use crate::tokens::TokenKinds;

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

/// A time for the review list: `09:12` today, `Oct 11 09:12` before.
pub fn when(at_ms: i64, today: jiff::civil::Date) -> String {
    let Ok(timestamp) = jiff::Timestamp::from_millisecond(at_ms) else {
        return String::new();
    };
    let zoned = timestamp.to_zoned(jiff::tz::TimeZone::system());
    if zoned.date() == today {
        zoned.strftime("%H:%M").to_string()
    } else {
        zoned.strftime("%b %d %H:%M").to_string()
    }
}

/// An RFC 3339 time in UTC for JSON and CSV.
pub fn utc(at_ms: i64) -> String {
    jiff::Timestamp::from_millisecond(at_ms)
        .map(|timestamp| timestamp.strftime("%Y-%m-%dT%H:%M:%SZ").to_string())
        .unwrap_or_default()
}

/// Tags as the review list shows them: ticket and project first.
pub fn tag_line(tags: &BTreeMap<String, String>) -> String {
    let mut parts: Vec<String> = Vec::new();
    for first in ["ticket", "project"] {
        if let Some(value) = tags.get(first) {
            parts.push(format!("{first} {value}"));
        }
    }
    parts.extend(
        tags.iter()
            .filter(|(name, _)| !matches!(name.as_str(), "ticket" | "project" | "person"))
            .map(|(name, value)| format!("{name} {value}")),
    );
    parts.join(" · ")
}

/// The cost of an entry for people: `$41.87`, with a note when some of it
/// is not on the price list.
pub fn cost(entry: &Entry) -> String {
    let shown = display(entry.value.nanos, CURRENCY);
    if entry.value.is_known() {
        shown
    } else if entry.value.nanos == 0 {
        "unknown model".to_owned()
    } else {
        format!("{shown} + unknown model")
    }
}

/// Why an entry sits where it does, for the review list's last column.
pub fn reason(entry: &Entry) -> String {
    if let Some(reason) = &entry.row.rejected_reason {
        return format!("rejected: {reason}");
    }
    if entry.ticket().is_none() && !entry.candidates.is_empty() {
        let count = entry.candidates.len();
        let noun = if count == 1 {
            "candidate"
        } else {
            "candidates"
        };
        return format!("no ticket, {count} {noun}");
    }
    entry.placed_by.clone()
}

#[derive(Debug, Serialize)]
pub struct SplitView {
    pub share_percent: u64,
    pub list_value: String,
    pub tags: BTreeMap<String, String>,
}

/// A token entry in JSON, as `tc review --json` prints it. `cost`, `tags`
/// and `by` are what the review list shows: the value of the priced model
/// calls, the tags of the largest split, and why it sits there.
#[derive(Debug, Serialize)]
pub struct EntryView {
    pub number: i64,
    pub cost: String,
    pub tags: BTreeMap<String, String>,
    pub by: String,
    pub agent_harness: String,
    pub session_id: String,
    pub model: String,
    pub started_at: String,
    pub ended_at: String,
    pub tokens: TokenKinds,
    /// `None` when some model calls are not on the price list.
    pub list_value: Option<String>,
    pub unknown_models: Vec<String>,
    pub currency: String,
    pub summary: Option<String>,
    pub status: String,
    pub rejected_reason: Option<String>,
    pub placed_by: String,
    pub candidates: Vec<String>,
    pub splits: Vec<SplitView>,
}

impl EntryView {
    pub fn of(entry: &Entry) -> Self {
        let tags = entry
            .splits
            .iter()
            .max_by_key(|split| split.share_percent)
            .map(|split| split.tags.clone())
            .unwrap_or_default();
        Self {
            number: entry.row.number,
            cost: decimal(entry.value.nanos, CURRENCY),
            tags,
            by: reason(entry),
            agent_harness: entry.row.harness.name().to_owned(),
            session_id: entry.row.session_id.clone(),
            model: entry.model.clone(),
            started_at: utc(entry.started_ms),
            ended_at: utc(entry.ended_ms),
            tokens: entry.tokens.kinds(),
            list_value: entry
                .value
                .is_known()
                .then(|| decimal(entry.value.nanos, CURRENCY)),
            unknown_models: entry.value.unknown_models.clone(),
            currency: CURRENCY.code(),
            summary: entry.summary.clone(),
            status: entry.row.status.name().to_owned(),
            rejected_reason: entry.row.rejected_reason.clone(),
            placed_by: entry.placed_by.clone(),
            candidates: entry.candidates.clone(),
            splits: entry
                .splits
                .iter()
                .map(|split| SplitView {
                    share_percent: split.share_percent,
                    list_value: decimal(split.value_nanos, CURRENCY),
                    tags: split.tags.clone(),
                })
                .collect(),
        }
    }
}

/// The share of `part` in `whole` as a JSON number from 0 to 1 with four
/// decimals, such as `0.9512`, computed without floats.
pub fn fraction(part: i64, whole: i64) -> Box<serde_json::value::RawValue> {
    let basis_points = if whole <= 0 || part <= 0 {
        0
    } else {
        i128::from(part)
            .saturating_mul(20_000)
            .checked_div(i128::from(whole))
            .unwrap_or(0)
            .saturating_add(1)
            .checked_div(2)
            .unwrap_or(0)
            .min(10_000)
    };
    let whole_part = basis_points.checked_div(10_000).unwrap_or(0);
    let rest = basis_points.checked_rem(10_000).unwrap_or(0);
    let digits = format!("{rest:04}");
    let digits = digits.trim_end_matches('0');
    let text = if digits.is_empty() {
        whole_part.to_string()
    } else {
        format!("{whole_part}.{digits}")
    };
    serde_json::value::RawValue::from_string(text)
        .unwrap_or_else(|_| serde_json::value::RawValue::NULL.to_owned())
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

    #[test]
    fn tables_line_up() {
        let rows = vec![
            vec!["#".to_owned(), "cost".to_owned(), "tags".to_owned()],
            vec![
                "1".to_owned(),
                "$41.87".to_owned(),
                "ticket KD-214".to_owned(),
            ],
            vec!["12".to_owned(), "$9.75".to_owned(), "team web".to_owned()],
        ];
        let mut out = Vec::new();
        table(&mut out, &rows, &[0, 1]).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(
            text,
            "   #   cost tags\n   1 $41.87 ticket KD-214\n  12  $9.75 team web\n"
        );
    }

    #[test]
    fn tags_show_ticket_and_project_first_and_never_the_person() {
        let tags = BTreeMap::from([
            ("team".to_owned(), "web".to_owned()),
            ("ticket".to_owned(), "KD-214".to_owned()),
            ("person".to_owned(), "m@x".to_owned()),
            ("project".to_owned(), "storefront".to_owned()),
        ]);
        assert_eq!(
            tag_line(&tags),
            "ticket KD-214 · project storefront · team web"
        );
        assert_eq!(tag_line(&BTreeMap::new()), "");
    }

    #[test]
    fn fractions_have_four_decimals() {
        assert_eq!(fraction(1, 3).get(), "0.3333");
        assert_eq!(fraction(2, 3).get(), "0.6667");
        assert_eq!(fraction(95, 100).get(), "0.95");
        assert_eq!(fraction(5, 5).get(), "1");
        assert_eq!(fraction(0, 5).get(), "0");
        assert_eq!(fraction(5, 0).get(), "0");
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
    fn times_show_in_utc_for_machines() {
        assert_eq!(utc(1_760_173_920_000), "2025-10-11T09:12:00Z");
        let today = jiff::civil::date(1999, 1, 1);
        assert!(!when(1_760_173_920_000, today).is_empty());
    }

    #[test]
    fn json_is_one_line() {
        let mut out = Vec::new();
        json(&mut out, &serde_json::json!({"a": 1})).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "{\"a\":1}\n");
    }
}
