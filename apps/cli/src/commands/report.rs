//! `tc report --by NAME|label|day`: cost in a table over this machine's
//! sessions, open and submitted alike, by any tag or attribute name, by
//! label or by day.

use tokencontroller_core::{DateRange, Filter, Label, Money, TagName, Tags, date_of};

use crate::app::App;
use crate::error::{Error, Result};
use crate::output::{self, decimal, display};

type Rows = Vec<(Option<String>, Money)>;

/// The rows of the table: by day, by label, or by a tag or attribute name,
/// the largest first.
fn rows_of(
    state: &tokencontroller_core::State,
    by: &str,
    filter: &Filter,
    window: DateRange,
) -> Result<Rows> {
    let currency = state.currency();
    let mut rows: Rows = match by {
        "day" => {
            let mut by_day: std::collections::BTreeMap<String, Money> =
                std::collections::BTreeMap::new();
            for line in state.select(filter, window) {
                let day = by_day
                    .entry(date_of(line.at).to_string())
                    .or_insert(Money::zero(currency));
                *day = day.checked_add(line.amount)?;
            }
            by_day
                .into_iter()
                .map(|(day, amount)| (Some(day), amount))
                .collect()
        }
        "label" => {
            let by_label = state.by_label(filter, window)?;
            Label::ALL
                .iter()
                .filter_map(|label| {
                    by_label
                        .iter()
                        .find(|(found, _)| found == label)
                        .map(|(_, amount)| (Some(label.as_str().to_owned()), *amount))
                })
                .collect()
        }
        name => {
            let name = TagName::new(name)?;
            state
                .by_tag(filter, window, &name)?
                .into_iter()
                .map(|(value, amount)| (value.map(|value| value.to_string()), amount))
                .collect()
        }
    };
    if by != "day" && by != "label" {
        rows.sort_by(|a, b| b.1.nanos().cmp(&a.1.nanos()).then(a.0.cmp(&b.0)));
    }
    Ok(rows)
}

pub fn run(
    app: &mut App<'_>,
    by: Option<&str>,
    filters: &[String],
    since: Option<&str>,
) -> Result<()> {
    app.prepare()?;
    let by = by.unwrap_or("feature").trim();
    if by.is_empty() {
        return Err(Error::usage(
            "say what to group by: a tag name, such as team, or label, or day",
        ));
    }
    let mut filter = Tags::new();
    for pair in filters {
        let (name, value) = super::name_value(pair)
            .filter(|(_, value)| !value.is_empty())
            .ok_or_else(|| Error::usage(format!("{pair} is not NAME=VALUE, such as team=web")))?;
        filter.insert(
            TagName::new(&name)?,
            tokencontroller_core::TagValue::new(&value)?,
        );
    }
    let filter = Filter(filter);
    let today = app.today();
    let since_day = match since {
        Some(day) => output::parse_date(day)
            .ok_or_else(|| Error::usage(format!("{day} is not a day; write it as 2026-10-01")))?,
        None => today.replace_day(1).unwrap_or(today),
    };
    let window = DateRange::new(since_day, today.max(since_day))
        .map_err(|error| Error::usage(error.to_string()))?;

    let state = app.fold()?;
    let currency = state.currency();
    let rows = rows_of(&state, by, &filter, window)?;
    let cost = state.cost(&filter, window)?;
    let total = cost.amount;

    if app.json {
        let rows: Vec<serde_json::Value> = rows
            .iter()
            .map(|(value, amount)| {
                serde_json::json!({
                    "value": value,
                    "cost": decimal(*amount),
                })
            })
            .collect();
        return output::json(
            app.out,
            &serde_json::json!({
                "by": by,
                "since": since_day.to_string(),
                "as_of": output::utc(state.as_of()),
                "filter": filter.0,
                "currency": currency.code(),
                "rows": rows,
                "total": decimal(total),
                "count_errors": cost.count_errors.len(),
                "unknown_models": cost.unknown_models.iter().map(ToString::to_string).collect::<Vec<_>>(),
            }),
        );
    }
    if rows.is_empty() {
        writeln!(app.out, "No cost since {since_day}.")?;
        return Ok(());
    }
    let mut table = vec![vec![by.to_owned(), "cost".to_owned(), "share".to_owned()]];
    for (value, amount) in &rows {
        table.push(vec![
            value.clone().unwrap_or_else(|| format!("(no {by})")),
            display(*amount),
            format!("{}%", output::percent(amount.nanos(), total.nanos())),
        ]);
    }
    table.push(vec!["total".to_owned(), display(total), String::new()]);
    output::table(app.out, &table, &[1, 2])?;
    if !cost.unknown_models.is_empty() {
        let models: Vec<String> = cost
            .unknown_models
            .iter()
            .map(ToString::to_string)
            .collect();
        writeln!(
            app.out,
            "No price for {}: that cost is missing above.",
            models.join(", ")
        )?;
    }
    if !cost.count_errors.is_empty() {
        writeln!(
            app.out,
            "{} messages have no token counts: count errors, with no cost.",
            cost.count_errors.len()
        )?;
    }
    Ok(())
}
