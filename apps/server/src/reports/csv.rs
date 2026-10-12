//! CSV files: every table in the web app downloads as one, holding exactly
//! what the table shows. Money is written in the currency's minor unit,
//! rounded only here (ADR 0004).

use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use tokencontroller_core::Money;

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
        let names: Vec<String> = header.into_iter().map(|name| name.as_ref().to_owned()).collect();
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
        let mut cells: Vec<String> = cells.into_iter().map(Into::into).take(self.columns).collect();
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

/// A leading minus is a formula unless the cell is a plain number.
fn starts_like_formula(cell: &str) -> bool {
    cell.strip_prefix('-')
        .is_some_and(|rest| rest.is_empty() || !rest.chars().all(|c| c.is_ascii_digit() || c == '.'))
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
