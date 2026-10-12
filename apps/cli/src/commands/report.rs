//! `tc report --by TAG|day`: cost in a table, by any tag name or by day, over
//! this machine's token entries, drafts and submitted alike.

use std::collections::BTreeMap;

use crate::amounts::{decimal, display};
use crate::app::{App, day_of, start_of_day_ms};
use crate::entries::CURRENCY;
use crate::error::{Error, Result};
use crate::output;
use crate::store::Status;

#[derive(Debug, Default, Clone)]
struct Row {
    nanos: i64,
    entries: u64,
}

pub fn run(app: &mut App<'_>, by: &str, filters: &[String], since: Option<&str>) -> Result<()> {
    app.prepare()?;
    let by = by.trim();
    if by.is_empty() {
        return Err(Error::usage(
            "say what to group by: a tag name, such as team, or day",
        ));
    }
    let mut filter: BTreeMap<String, String> = BTreeMap::new();
    for pair in filters {
        let (name, value) = super::name_value(pair)
            .filter(|(_, value)| !value.is_empty())
            .ok_or_else(|| Error::usage(format!("{pair} is not NAME=VALUE, such as team=web")))?;
        filter.insert(name, value);
    }
    let since_day = match since {
        Some(day) => day
            .parse::<jiff::civil::Date>()
            .map_err(|_| Error::usage(format!("{day} is not a day; write it as 2026-10-01")))?,
        None => app.today().first_of_month(),
    };
    let since_ms = start_of_day_ms(since_day);

    let entries = app.entries(&[Status::Draft, Status::Rejected, Status::Submitted])?;
    let mut rows: BTreeMap<Option<String>, Row> = BTreeMap::new();
    let mut unknown = false;
    for entry in entries.iter().filter(|entry| entry.started_ms >= since_ms) {
        unknown |= !entry.value.is_known();
        for split in &entry.splits {
            if !filter
                .iter()
                .all(|(name, value)| split.tags.get(name) == Some(value))
            {
                continue;
            }
            let key = if by == "day" {
                Some(day_of(entry.started_ms).to_string())
            } else {
                split.tags.get(by).cloned()
            };
            let row = rows.entry(key).or_default();
            row.nanos = row.nanos.saturating_add(split.value_nanos);
            row.entries = row.entries.saturating_add(1);
        }
    }
    let mut ordered: Vec<(Option<String>, Row)> = rows.into_iter().collect();
    if by != "day" {
        ordered.sort_by(|a, b| b.1.nanos.cmp(&a.1.nanos).then(a.0.cmp(&b.0)));
    }
    let total = ordered
        .iter()
        .fold(0_i64, |sum, (_, row)| sum.saturating_add(row.nanos));

    if app.json {
        let rows: Vec<serde_json::Value> = ordered
            .iter()
            .map(|(value, row)| {
                serde_json::json!({
                    "value": value,
                    "list_value": decimal(row.nanos, CURRENCY),
                    "token_entries": row.entries,
                })
            })
            .collect();
        return output::json(
            app.out,
            &serde_json::json!({
                "by": by,
                "since": since_day.to_string(),
                "filter": filter,
                "currency": CURRENCY.code(),
                "rows": rows,
                "total": decimal(total, CURRENCY),
                "includes_unknown_models": unknown,
            }),
        );
    }
    if ordered.is_empty() {
        writeln!(app.out, "No token entries since {since_day}.")?;
        return Ok(());
    }
    let mut table = vec![vec![by.to_owned(), "cost".to_owned(), "share".to_owned()]];
    for (value, row) in &ordered {
        table.push(vec![
            value.clone().unwrap_or_else(|| format!("(no {by})")),
            display(row.nanos, CURRENCY),
            format!("{}%", output::percent(row.nanos, total)),
        ]);
    }
    table.push(vec![
        "total".to_owned(),
        display(total, CURRENCY),
        String::new(),
    ]);
    output::table(app.out, &table, &[1, 2])?;
    if unknown {
        writeln!(
            app.out,
            "Some sessions used a model that is not on the price list; their cost is missing above."
        )?;
    }
    Ok(())
}
