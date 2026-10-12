//! CSV files: every table in the web app downloads as one, holding exactly
//! what the table shows (docs: Report > Exports). The same GET answers CSV
//! with `?format=csv`. Money is written as the API writes it: a decimal
//! string in the currency's minor unit, rounded by the core (ADR 0004).

use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokencontroller_core::Money;
use utoipa::IntoParams;

/// `?format=csv` on a table's GET, for tables that take no other query.
#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct FormatQuery {
    /// `csv` for the table as a CSV file instead of JSON.
    #[param(example = "csv")]
    pub format: Option<String>,
}

impl FormatQuery {
    /// Whether the caller asked for CSV.
    #[must_use]
    pub fn wants_csv(&self) -> bool {
        wants_csv(self.format.as_deref())
    }
}

/// Whether a `format` query asks for CSV.
#[must_use]
pub fn wants_csv(format: Option<&str>) -> bool {
    format == Some("csv")
}

/// One column of a table: its header and where its cell sits in a row's
/// JSON, as a JSON pointer such as `/net` or `/bill/provider`.
pub type Column = (&'static str, &'static str);

/// A file with one line per row, each row as the API sends it in JSON. Text
/// and numbers are written as they are, `true` and `false` as `yes` and
/// `no`, a missing value as an empty cell, tags as `team: web; client:
/// acme`, and a list as its items joined by `; `.
///
/// # Errors
///
/// A row that cannot be turned into JSON.
pub fn from_rows<T: Serialize>(columns: &[Column], rows: &[T]) -> Result<Csv, serde_json::Error> {
    let mut csv = Csv::new(columns.iter().map(|(name, _)| *name));
    for row in rows {
        let row = serde_json::to_value(row)?;
        csv.row(
            columns
                .iter()
                .map(|(_, pointer)| row.pointer(pointer).map_or_else(String::new, cell)),
        );
    }
    Ok(csv)
}

/// A table's rows as JSON, or as a CSV file named `filename` when the
/// caller asked for CSV.
///
/// # Errors
///
/// [`crate::error::ApiError::Internal`] for a row that cannot be turned
/// into JSON.
pub fn table<T: Serialize>(
    rows: &[T],
    columns: &[Column],
    csv: bool,
    filename: &str,
) -> Result<Response, crate::error::ApiError> {
    if csv {
        Ok(from_rows(columns, rows)
            .map_err(|error| crate::error::ApiError::Internal(error.into()))?
            .into_download(filename))
    } else {
        Ok(axum::Json(rows).into_response())
    }
}

/// One JSON value as a cell.
fn cell(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(true) => "yes".to_owned(),
        Value::Bool(false) => "no".to_owned(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => text.clone(),
        Value::Array(items) => items.iter().map(cell).collect::<Vec<_>>().join("; "),
        Value::Object(fields) => fields
            .iter()
            .map(|(name, value)| format!("{name}: {}", cell(value)))
            .collect::<Vec<_>>()
            .join("; "),
    }
}

/// A CSV file built row by row, RFC 4180 style, with a header row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Csv {
    text: String,
    columns: usize,
}

impl Csv {
    /// A file with these column names.
    #[must_use]
    pub fn new<I, S>(header: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut csv = Self {
            text: String::new(),
            columns: 0,
        };
        let names: Vec<String> = header
            .into_iter()
            .map(|name| name.as_ref().to_owned())
            .collect();
        csv.columns = names.len();
        csv.push_line(&names);
        csv
    }

    /// Adds a row. Missing cells are left empty and extra cells are dropped,
    /// so every row has as many cells as the header.
    pub fn row<I, S>(&mut self, cells: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut cells: Vec<String> = cells
            .into_iter()
            .map(Into::into)
            .take(self.columns)
            .collect();
        cells.resize(self.columns, String::new());
        self.push_line(&cells);
    }

    /// The file's text.
    #[must_use]
    pub fn into_text(self) -> String {
        self.text
    }

    /// The file as an HTTP download named `filename`.
    #[must_use]
    pub fn into_download(self, filename: &str) -> Response {
        let disposition = HeaderValue::from_str(&format!(
            "attachment; filename=\"{}\"",
            filename.replace(['"', '\\', '\r', '\n'], "")
        ))
        .unwrap_or_else(|_| HeaderValue::from_static("attachment"));
        (
            [
                (
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("text/csv; charset=utf-8"),
                ),
                (header::CONTENT_DISPOSITION, disposition),
            ],
            self.text,
        )
            .into_response()
    }

    fn push_line(&mut self, cells: &[String]) {
        let line: Vec<String> = cells.iter().map(|cell| escape(cell)).collect();
        self.text.push_str(&line.join(","));
        self.text.push_str("\r\n");
    }
}

/// Quotes a cell when it needs quoting. A text cell that a spreadsheet would
/// read as a formula gets a leading apostrophe, so a summary can never run
/// as one.
fn escape(cell: &str) -> String {
    let defused = if cell.starts_with(['=', '+', '@', '\t', '\r']) || starts_like_formula(cell) {
        format!("'{cell}")
    } else {
        cell.to_owned()
    };
    if defused.contains([',', '"', '\r', '\n']) {
        format!("\"{}\"", defused.replace('"', "\"\""))
    } else {
        defused
    }
}

/// Re-writes a CSV text made elsewhere, such as the core's cost sheet, with
/// the same quoting and formula defusing as every other download.
///
/// # Errors
///
/// A text that is not CSV.
pub fn from_text(text: &str) -> Result<Csv, ::csv::Error> {
    let mut reader = ::csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(text.as_bytes());
    let mut records = reader.records();
    let Some(header) = records.next() else {
        return Ok(Csv::new(Vec::<String>::new()));
    };
    let mut csv = Csv::new(header?.iter());
    for record in records {
        csv.row(record?.iter().map(str::to_owned));
    }
    Ok(csv)
}

/// [`from_text`] as an HTTP download named `filename`.
///
/// # Errors
///
/// [`crate::error::ApiError::Internal`] for a text that is not CSV.
pub fn download(text: &str, filename: &str) -> Result<Response, crate::error::ApiError> {
    Ok(from_text(text)
        .map_err(|error| crate::error::ApiError::Internal(error.into()))?
        .into_download(filename))
}

/// A leading minus is a formula unless the cell is a plain number.
fn starts_like_formula(cell: &str) -> bool {
    cell.strip_prefix('-').is_some_and(|rest| {
        rest.is_empty() || !rest.chars().all(|c| c.is_ascii_digit() || c == '.')
    })
}

/// An amount as a CSV cell: the currency's minor unit, such as `1234.50`,
/// without the currency code.
#[must_use]
pub fn money_cell(amount: Money) -> String {
    let shown = amount.to_string();
    shown
        .rsplit_once(' ')
        .map_or(shown.clone(), |(number, _code)| number.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokencontroller_core::Currency;

    #[test]
    fn rows_from_json_follow_the_columns() {
        let rows = [
            serde_json::json!({"bill": {"provider": "anthropic"}, "net": "12.50", "closed": true,
                "tags": {"client": "acme", "team": "web"}, "rate": null, "count": 3}),
            serde_json::json!({"bill": {"provider": "openai"}, "net": "-1.00", "closed": false,
                "tags": {}, "rate": "0.9214", "count": 0, "lines": ["a", "b"]}),
        ];
        let csv = from_rows(
            &[
                ("provider", "/bill/provider"),
                ("net", "/net"),
                ("closed", "/closed"),
                ("tags", "/tags"),
                ("rate", "/rate"),
                ("count", "/count"),
                ("lines", "/lines"),
            ],
            &rows,
        )
        .unwrap();
        assert_eq!(
            csv.into_text(),
            "provider,net,closed,tags,rate,count,lines\r\n\
             anthropic,12.50,yes,client: acme; team: web,,3,\r\n\
             openai,-1.00,no,,0.9214,0,a; b\r\n"
        );
    }

    #[test]
    fn only_csv_asks_for_csv() {
        assert!(wants_csv(Some("csv")));
        assert!(!wants_csv(Some("json")));
        assert!(!wants_csv(None));
        assert!(!FormatQuery::default().wants_csv());
    }

    #[test]
    fn writes_a_header_and_rows() {
        let mut csv = Csv::new(["ticket", "cost"]);
        csv.row(["KD-214", "41.87"]);
        assert_eq!(csv.into_text(), "ticket,cost\r\nKD-214,41.87\r\n");
    }

    #[test]
    fn quotes_commas_quotes_and_line_breaks() {
        let mut csv = Csv::new(["summary"]);
        csv.row(["Fixed the cart, then \"tested\" it\nagain"]);
        assert_eq!(
            csv.into_text(),
            "summary\r\n\"Fixed the cart, then \"\"tested\"\" it\nagain\"\r\n"
        );
    }

    #[test]
    fn rows_have_as_many_cells_as_the_header() {
        let mut csv = Csv::new(["a", "b"]);
        csv.row(["1"]);
        csv.row(["1", "2", "3"]);
        assert_eq!(csv.into_text(), "a,b\r\n1,\r\n1,2\r\n");
    }

    #[test]
    fn formulas_never_run() {
        let mut csv = Csv::new(["summary"]);
        csv.row(["=HYPERLINK(\"x\")"]);
        csv.row(["+1"]);
        csv.row(["@SUM(A1)"]);
        csv.row(["-A1"]);
        csv.row(["-"]);
        csv.row(["-12.30"]);
        assert_eq!(
            csv.into_text(),
            "summary\r\n\"'=HYPERLINK(\"\"x\"\")\"\r\n'+1\r\n'@SUM(A1)\r\n'-A1\r\n'-\r\n-12.30\r\n"
        );
    }

    #[test]
    fn text_from_elsewhere_is_defused_too() {
        let csv = from_text("bill,line,note\nb,adjustment,=cmd()\n,total,\n").unwrap();
        assert_eq!(
            csv.into_text(),
            "bill,line,note\r\nb,adjustment,'=cmd()\r\n,total,\r\n"
        );
        assert_eq!(from_text("").unwrap().into_text(), "\r\n");
    }

    #[test]
    fn money_shows_in_the_minor_unit() {
        let amount = Money::from_nanos(41_874_999_999, Currency::USD);
        assert_eq!(money_cell(amount), "41.87");
        let negative = Money::from_nanos(-5_000_000, Currency::EUR);
        assert_eq!(money_cell(negative), "-0.01");
        assert_eq!(money_cell(Money::zero(Currency::EUR)), "0.00");
    }

    #[test]
    fn a_download_names_its_file() {
        let response = Csv::new(["a"]).into_download("cost-sheet-2026-09.csv");
        assert_eq!(
            response.headers()[header::CONTENT_DISPOSITION],
            "attachment; filename=\"cost-sheet-2026-09.csv\""
        );
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "text/csv; charset=utf-8"
        );
    }
}
