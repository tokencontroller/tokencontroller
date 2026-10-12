//! `tc export --csv`: every token entry on this machine as CSV, one row per
//! split, with a column per tag name. Enough to report by hand when the
//! organization has no service.

use std::collections::BTreeSet;

use crate::amounts::decimal;
use crate::app::App;
use crate::entries::{CURRENCY, Entry};
use crate::error::Result;
use crate::output::utc;
use crate::store::Status;

const FIXED: [&str; 17] = [
    "number",
    "status",
    "agent_harness",
    "session_id",
    "model",
    "started_at",
    "ended_at",
    "input_tokens",
    "output_tokens",
    "cache_write_tokens",
    "cache_read_tokens",
    "list_value",
    "currency",
    "share_percent",
    "ticket",
    "project",
    "summary",
];

pub fn run(app: &mut App<'_>) -> Result<()> {
    app.prepare()?;
    let entries = app.entries(&[Status::Draft, Status::Rejected, Status::Submitted])?;
    write_csv(app.out, &entries)
}

pub fn write_csv(out: &mut dyn std::io::Write, entries: &[Entry]) -> Result<()> {
    let tag_names: BTreeSet<&str> = entries
        .iter()
        .flat_map(|entry| entry.splits.iter())
        .flat_map(|split| split.tags.keys())
        .map(String::as_str)
        .filter(|name| !matches!(*name, "ticket" | "project"))
        .collect();
    let mut header: Vec<String> = FIXED.iter().map(|name| (*name).to_owned()).collect();
    header.extend(tag_names.iter().map(|name| (*name).to_owned()));
    header.push("placed_by".to_owned());
    header.push("unknown_models".to_owned());
    writeln!(out, "{}", line(&header))?;
    for entry in entries {
        for split in &entry.splits {
            let tokens = split.tokens.kinds();
            let mut row = vec![
                entry.row.number.to_string(),
                entry.row.status.name().to_owned(),
                entry.row.harness.name().to_owned(),
                entry.row.session_id.clone(),
                entry.model.clone(),
                utc(entry.started_ms),
                utc(entry.ended_ms),
                tokens.input.to_string(),
                tokens.output.to_string(),
                tokens.cache_write.to_string(),
                tokens.cache_read.to_string(),
                decimal(split.value_nanos, CURRENCY),
                CURRENCY.code(),
                split.share_percent.to_string(),
                split.tags.get("ticket").cloned().unwrap_or_default(),
                split.tags.get("project").cloned().unwrap_or_default(),
                entry.summary.clone().unwrap_or_default(),
            ];
            row.extend(
                tag_names
                    .iter()
                    .map(|name| split.tags.get(*name).cloned().unwrap_or_default()),
            );
            row.push(entry.placed_by.clone());
            row.push(entry.value.unknown_models.join(" "));
            writeln!(out, "{}", line(&row))?;
        }
    }
    Ok(())
}

/// One CSV line: fields with a comma, quote or line break are quoted, and a
/// field a spreadsheet would run as a formula is prefixed with a quote mark.
fn line(fields: &[String]) -> String {
    fields
        .iter()
        .map(|field| field_text(field))
        .collect::<Vec<_>>()
        .join(",")
}

fn field_text(field: &str) -> String {
    let safe = if field.starts_with(['=', '+', '-', '@'])
        && crate::amounts::parse_nanos(field).is_none()
    {
        format!("'{field}")
    } else {
        field.to_owned()
    };
    if safe.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", safe.replace('"', "\"\""))
    } else {
        safe
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_are_quoted_when_they_must_be() {
        assert_eq!(field_text("plain"), "plain");
        assert_eq!(field_text("a,b"), "\"a,b\"");
        assert_eq!(field_text("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(field_text("two\nlines"), "\"two\nlines\"");
        assert_eq!(field_text("=SUM(A1)"), "'=SUM(A1)");
        assert_eq!(field_text("-1.5"), "-1.5");
        assert_eq!(line(&["a".into(), "b,c".into()]), "a,\"b,c\"");
    }
}
