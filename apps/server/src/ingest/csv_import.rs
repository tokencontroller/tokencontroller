//! CSV import of token entries: one token entry per row, the columns named
//! after the fields of the token entry schema. Nested fields use a dot:
//! `tokens.input`, `tags.team`, `signals.branch`. `tc export --csv` writes
//! the same format.
//!
//! The import reads the whole file first and shows every row with its
//! problem, so nothing is saved until the person has seen what was read.

use std::collections::BTreeMap;

use serde::Serialize;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use utoipa::ToSchema;

use super::entry::{CheckedTokenEntry, NewTokenEntry, Signals};
use super::model::TokenCounts;

/// The most rows one import may hold.
pub const MAX_ROWS: usize = 10_000;

/// Columns that are not tags, in the order `tc export --csv` writes them.
pub const COLUMNS: [&str; 22] = [
    "agent_harness",
    "agent_name",
    "model",
    "provider",
    "effort",
    "speed",
    "started_at",
    "ended_at",
    "tokens.input",
    "tokens.output",
    "tokens.cache_write",
    "tokens.cache_read",
    "reported_cost",
    "summary",
    "ticket",
    "project",
    "session_id",
    "signals.branch",
    "signals.working_directory",
    "signals.repository",
    "signals.pull_request",
    "signals.subagent_type",
];

/// Why the file as a whole cannot be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CsvError {
    #[error("the file is empty")]
    Empty,
    #[error("the file has no {0} column")]
    MissingColumn(&'static str),
    #[error(
        "column {0:?} is not a field of the token entry schema; tags go in columns named tags.<name>"
    )]
    UnknownColumn(String),
    #[error("column {0:?} appears twice")]
    DuplicateColumn(String),
    #[error("the file has more than {MAX_ROWS} rows; split it into several imports")]
    TooManyRows,
    #[error("the file cannot be read as CSV: {0}")]
    Unreadable(String),
}

/// One row as read, for the preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The line in the file, counting the header as line 1.
    pub line: u64,
    pub result: Result<CheckedTokenEntry, String>,
}

/// What a preview shows per row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct RowProblem {
    pub line: u64,
    pub problem: String,
}

const REQUIRED: [&str; 4] = ["agent_harness", "model", "started_at", "ended_at"];

/// Reads a CSV file of token entries. Comma or semicolon separated; a
/// leading byte order mark is skipped.
///
/// # Errors
///
/// A [`CsvError`] when the file as a whole cannot be read. Problems in
/// single rows are in each [`Row`] instead.
pub fn parse(text: &str) -> Result<Vec<Row>, CsvError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let first_line = text.lines().next().ok_or(CsvError::Empty)?;
    let separator = if first_line.matches(';').count() > first_line.matches(',').count() {
        b';'
    } else {
        b','
    };
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(separator)
        .flexible(false)
        .trim(csv::Trim::Headers)
        .from_reader(text.as_bytes());
    let headers: Vec<String> = reader
        .headers()
        .map_err(|error| CsvError::Unreadable(error.to_string()))?
        .iter()
        .map(str::to_owned)
        .collect();
    check_headers(&headers)?;

    let mut rows = Vec::new();
    for record in reader.records() {
        if rows.len() >= MAX_ROWS {
            return Err(CsvError::TooManyRows);
        }
        let record = record.map_err(|error| CsvError::Unreadable(error.to_string()))?;
        let line = record.position().map_or(0, csv::Position::line);
        let cells: BTreeMap<&str, &str> = headers
            .iter()
            .map(String::as_str)
            .zip(record.iter().map(str::trim))
            .filter(|(_, value)| !value.is_empty())
            .collect();
        if cells.is_empty() {
            continue;
        }
        let result =
            entry_from_cells(&cells).and_then(|entry| entry.check().map_err(|error| error.0));
        rows.push(Row { line, result });
    }
    Ok(rows)
}

fn check_headers(headers: &[String]) -> Result<(), CsvError> {
    let mut seen = std::collections::BTreeSet::new();
    for header in headers {
        if !seen.insert(header.as_str()) {
            return Err(CsvError::DuplicateColumn(header.clone()));
        }
        let known = COLUMNS.contains(&header.as_str())
            || header
                .strip_prefix("tags.")
                .is_some_and(|name| !name.is_empty());
        if !known {
            return Err(CsvError::UnknownColumn(header.clone()));
        }
    }
    for required in REQUIRED {
        if !seen.contains(required) {
            return Err(CsvError::MissingColumn(required));
        }
    }
    Ok(())
}

fn text(cells: &BTreeMap<&str, &str>, column: &str) -> Option<String> {
    cells.get(column).map(|value| (*value).to_owned())
}

fn required(cells: &BTreeMap<&str, &str>, column: &str) -> Result<String, String> {
    text(cells, column).ok_or_else(|| format!("{column} is empty"))
}

fn time(cells: &BTreeMap<&str, &str>, column: &str) -> Result<OffsetDateTime, String> {
    let value = required(cells, column)?;
    OffsetDateTime::parse(&value, &Rfc3339).map_err(|_| {
        format!("{column} {value:?} is not a date and time such as 2026-09-18T09:12:00Z")
    })
}

fn count(cells: &BTreeMap<&str, &str>, column: &str) -> Result<u64, String> {
    cells.get(column).map_or(Ok(0), |value| {
        value
            .replace(['_', ' '], "")
            .parse()
            .map_err(|_| format!("{column} {value:?} is not a whole number of tokens"))
    })
}

fn entry_from_cells(cells: &BTreeMap<&str, &str>) -> Result<NewTokenEntry, String> {
    let reported_cost = cells
        .get("reported_cost")
        .map(|value| super::entry::Amount::Text(value.replace(',', ".")));
    let pull_request = match cells.get("signals.pull_request") {
        None => None,
        Some(value) => Some(
            value
                .trim_start_matches('#')
                .parse()
                .map_err(|_| format!("signals.pull_request {value:?} is not a number"))?,
        ),
    };
    let tags = cells
        .iter()
        .filter_map(|(column, value)| {
            column
                .strip_prefix("tags.")
                .map(|name| (name.to_owned(), (*value).to_owned()))
        })
        .collect();
    Ok(NewTokenEntry {
        agent_harness: required(cells, "agent_harness")?,
        agent_name: text(cells, "agent_name"),
        model: required(cells, "model")?,
        provider: text(cells, "provider"),
        effort: text(cells, "effort"),
        speed: text(cells, "speed"),
        started_at: time(cells, "started_at")?,
        ended_at: time(cells, "ended_at")?,
        tokens: TokenCounts {
            input: count(cells, "tokens.input")?,
            output: count(cells, "tokens.output")?,
            cache_write: count(cells, "tokens.cache_write")?,
            cache_read: count(cells, "tokens.cache_read")?,
        },
        reported_cost,
        summary: text(cells, "summary"),
        ticket: text(cells, "ticket"),
        project: text(cells, "project"),
        tags,
        signals: Signals {
            branch: text(cells, "signals.branch"),
            working_directory: text(cells, "signals.working_directory"),
            repository: text(cells, "signals.repository"),
            pull_request,
            subagent_type: text(cells, "signals.subagent_type"),
            ticket_keys: Vec::new(),
        },
        session_id: text(cells, "session_id"),
        messages: Vec::new(),
        splits: Vec::new(),
        placed_by: None,
        local_id: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "\u{feff}agent_harness,model,started_at,ended_at,tokens.input,tokens.output,reported_cost,summary,ticket,tags.team\n\
        claude-code,claude-sonnet-5,2026-09-18T09:12:00Z,2026-09-18T09:40:00Z,1200,300,0.42,\"Fixed the cart, and tests\",KD-214,web\n\
        ,,,,,,,,,\n\
        codex-cli,gpt-5,2026-09-18T10:00:00Z,2026-09-18T09:00:00Z,10,1,,,,\n\
        codex-cli,gpt-5,yesterday,2026-09-18T11:00:00Z,10,1,,,,\n";

    #[test]
    fn rows_are_read_with_their_lines_and_problems() {
        let rows = parse(FILE).unwrap();
        assert_eq!(rows.len(), 3);

        let first = rows[0].result.as_ref().unwrap();
        assert_eq!(rows[0].line, 2);
        assert_eq!(
            first.entry.summary.as_deref(),
            Some("Fixed the cart, and tests")
        );
        assert_eq!(first.entry.tokens.input, 1200);
        assert_eq!(first.entry.tags["team"], "web");
        assert_eq!(first.reported_cost.unwrap().nanos(), 420_000_000);

        assert_eq!(rows[1].line, 4);
        assert_eq!(
            rows[1].result,
            Err("ended_at is before started_at".to_owned())
        );
        assert!(
            rows[2]
                .result
                .as_ref()
                .unwrap_err()
                .contains("started_at \"yesterday\"")
        );
    }

    #[test]
    fn semicolons_and_decimal_commas_work() {
        let file = "agent_harness;model;started_at;ended_at;tokens.input;reported_cost\n\
            claude-code;claude-sonnet-5;2026-09-18T09:12:00Z;2026-09-18T09:40:00Z;1 200;0,5\n";
        let rows = parse(file).unwrap();
        let entry = rows[0].result.as_ref().unwrap();
        assert_eq!(entry.entry.tokens.input, 1200);
        assert_eq!(entry.reported_cost.unwrap().nanos(), 500_000_000);
    }

    #[test]
    fn the_header_must_name_schema_fields() {
        assert_eq!(parse(""), Err(CsvError::Empty));
        assert_eq!(
            parse("agent_harness,model,started_at\n"),
            Err(CsvError::MissingColumn("ended_at"))
        );
        assert_eq!(
            parse("agent_harness,model,started_at,ended_at,team\n"),
            Err(CsvError::UnknownColumn("team".to_owned()))
        );
        assert_eq!(
            parse("agent_harness,model,model,started_at,ended_at\n"),
            Err(CsvError::DuplicateColumn("model".to_owned()))
        );
    }

    #[test]
    fn every_column_tc_writes_is_read() {
        let header = COLUMNS.join(",");
        let row = "claude-code,review bot,claude-sonnet-5,anthropic,high,,2026-09-18T09:12:00Z,2026-09-18T09:40:00Z,1,2,3,4,0.1,did it,KD-1,storefront,s-9,KD-1-fix,/src/shop,github.com/kestrel/storefront,#42,review";
        let rows = parse(&format!("{header}\n{row}\n")).unwrap();
        let entry = &rows[0].result.as_ref().unwrap().entry;
        assert_eq!(entry.agent_name.as_deref(), Some("review bot"));
        assert_eq!(
            entry.tokens,
            TokenCounts {
                input: 1,
                output: 2,
                cache_write: 3,
                cache_read: 4
            }
        );
        assert_eq!(entry.project.as_deref(), Some("storefront"));
        assert_eq!(entry.session_id.as_deref(), Some("s-9"));
        assert_eq!(entry.signals.pull_request, Some(42));
        assert_eq!(entry.signals.subagent_type.as_deref(), Some("review"));
        assert_eq!(
            entry.signals.repository.as_deref(),
            Some("github.com/kestrel/storefront")
        );
    }

    #[test]
    fn a_file_with_too_many_rows_is_refused() {
        let mut file = String::from("agent_harness,model,started_at,ended_at\n");
        for _ in 0..=MAX_ROWS {
            file.push_str("a,m,2026-09-18T09:12:00Z,2026-09-18T09:40:00Z\n");
        }
        assert_eq!(parse(&file), Err(CsvError::TooManyRows));
    }
}
