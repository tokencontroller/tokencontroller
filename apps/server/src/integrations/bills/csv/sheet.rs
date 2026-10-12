//! Reading CSV text as people and clouds write it: a byte order mark, comma
//! or semicolon separators, quoted fields, decimal commas in semicolon
//! files, and header names in any of the usual spellings.

use std::collections::BTreeMap;
use std::str::FromStr as _;

use ::csv::{ReaderBuilder, StringRecord, StringRecordsIntoIter, Trim};
use rust_decimal::Decimal;
use time::Date;
use time::macros::format_description;
use tokencontroller_core::{Currency, Money};

use super::CsvBillError;
use crate::integrations::decimal::{DecimalError, money_from_decimal};

/// Lines looked at to tell commas from semicolons.
const SNIFF_LINES: usize = 10;

/// The rows of a CSV file, each with its line number.
pub(super) struct Sheet<'a> {
    bytes: &'a [u8],
    records: StringRecordsIntoIter<&'a [u8]>,
    decimal_comma: bool,
    /// The line at byte `counted_to`, counting from 1.
    line: u64,
    counted_to: usize,
}

impl<'a> Sheet<'a> {
    /// Reads `text`, after a byte order mark if there is one.
    pub(super) fn new(text: &'a str) -> Self {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let separator = separator(text);
        let records = ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .delimiter(separator)
            .trim(Trim::All)
            .from_reader(text.as_bytes())
            .into_records();
        Self {
            bytes: text.as_bytes(),
            records,
            decimal_comma: separator == b';',
            line: 1,
            counted_to: 0,
        }
    }

    /// Whether amounts may use a decimal comma, as in semicolon files.
    pub(super) const fn decimal_comma(&self) -> bool {
        self.decimal_comma
    }

    /// The line a record starts on. The CSV reader's own count skips blank
    /// lines and its position points before them, so this steps over line
    /// breaks to the record and counts them itself, from where it last
    /// stopped.
    fn line_at(&mut self, position: Option<&::csv::Position>) -> u64 {
        let Some(after) = position.and_then(|position| usize::try_from(position.byte()).ok())
        else {
            return self.line;
        };
        let blank = self.bytes.get(after..).map_or(0, |rest| {
            rest.iter()
                .take_while(|b| matches!(**b, b'\r' | b'\n'))
                .count()
        });
        let byte = after.saturating_add(blank);
        let breaks = self
            .bytes
            .get(self.counted_to..byte)
            .map_or(0, |between| between.split(|b| *b == b'\n').skip(1).count());
        self.line = self
            .line
            .saturating_add(u64::try_from(breaks).unwrap_or(u64::MAX));
        self.counted_to = self.counted_to.max(byte);
        self.line
    }
}

impl Iterator for Sheet<'_> {
    /// A row that is not blank, with its line number.
    type Item = Result<(u64, StringRecord), CsvBillError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let record = match self.records.next()? {
                Ok(record) => record,
                Err(error) => {
                    let line = self.line_at(error.position());
                    return Some(Err(CsvBillError {
                        row: Some(line),
                        column: None,
                        message: format!("This row cannot be read as CSV: {error}."),
                    }));
                }
            };
            if record.iter().all(str::is_empty) {
                continue;
            }
            let line = self.line_at(record.position());
            return Some(Ok((line, record)));
        }
    }
}

/// Picks the separator that appears more often outside quotes in the
/// first lines: a semicolon, or else a comma.
fn separator(text: &str) -> u8 {
    let mut commas: usize = 0;
    let mut semicolons: usize = 0;
    let mut lines: usize = 0;
    let mut quoted = false;
    for c in text.chars() {
        match c {
            '"' => quoted = !quoted,
            ',' if !quoted => commas = commas.saturating_add(1),
            ';' if !quoted => semicolons = semicolons.saturating_add(1),
            '\n' if !quoted => {
                lines = lines.saturating_add(1);
                if lines >= SNIFF_LINES {
                    break;
                }
            }
            _ => {}
        }
    }
    if semicolons > commas { b';' } else { b',' }
}

/// A header row: each column under one key, such as `line_item_usage_start_date`
/// for `lineItem/UsageStartDate` and `service_description` for both
/// `Service description` and `service.description`.
pub(super) struct Header {
    keys: Vec<String>,
    names: Vec<String>,
    /// A currency some exports put in a column name, such as `Cost (EUR)`.
    pub(super) currency: Option<Currency>,
}

/// A column found in the header.
#[derive(Debug, Clone)]
pub(super) struct Column {
    pub(super) index: usize,
    /// As the file spells it, for messages.
    pub(super) name: String,
}

impl Header {
    pub(super) fn new(record: &StringRecord) -> Self {
        let mut keys = Vec::new();
        let mut names = Vec::new();
        let mut currency = None;
        for name in record {
            let (key, named_currency) = column_key(name);
            currency = currency.or(named_currency);
            keys.push(key);
            names.push(name.to_owned());
        }
        Self {
            keys,
            names,
            currency,
        }
    }

    /// The first of `candidates` the header has.
    pub(super) fn find(&self, candidates: &[&str]) -> Option<Column> {
        candidates.iter().find_map(|candidate| {
            let index = self.keys.iter().position(|key| key == candidate)?;
            Some(Column {
                index,
                name: self.names.get(index).cloned().unwrap_or_default(),
            })
        })
    }

    /// Every column, by key and as spelled.
    pub(super) fn columns(&self) -> impl Iterator<Item = (&str, &str)> {
        self.keys
            .iter()
            .map(String::as_str)
            .zip(self.names.iter().map(String::as_str))
    }

    pub(super) fn len(&self) -> usize {
        self.keys.len()
    }
}

/// The key of a column name, and the currency in it if there is one:
/// `Cost (EUR)` is `cost` in EUR.
pub(super) fn column_key(name: &str) -> (String, Option<Currency>) {
    let name = name.trim().trim_start_matches('\u{feff}');
    let (base, currency) = match name.split_once('(') {
        Some((base, rest)) => {
            let inside = rest.trim_end().trim_end_matches(')').trim();
            (base, Currency::new(&inside.to_ascii_uppercase()).ok())
        }
        None => (name, None),
    };
    let mut key = String::new();
    let mut previous: Option<char> = None;
    for c in base.chars() {
        if c.is_ascii_alphanumeric() {
            let starts_word = c.is_ascii_uppercase()
                && previous.is_some_and(|p| p.is_ascii_lowercase() || p.is_ascii_digit());
            if starts_word {
                key.push('_');
            }
            key.push(c.to_ascii_lowercase());
        } else if !key.is_empty() && !key.ends_with('_') {
            key.push('_');
        }
        previous = Some(c);
    }
    while key.ends_with('_') {
        key.pop();
    }
    (key, currency)
}

/// A cell of a row, trimmed; empty if the row is shorter.
pub(super) fn cell<'r>(record: &'r StringRecord, column: Option<&Column>) -> &'r str {
    column
        .and_then(|column| record.get(column.index))
        .map_or("", str::trim)
}

/// Lines of `key,value` before a header, such as `Currency,USD`, by key.
pub(super) fn preamble_entry(record: &StringRecord, preamble: &mut BTreeMap<String, String>) {
    let mut cells = record.iter().filter(|cell| !cell.is_empty());
    if let (Some(key), Some(value), None) = (cells.next(), cells.next(), cells.next()) {
        preamble
            .entry(column_key(key).0)
            .or_insert_with(|| value.to_owned());
    }
}

/// Reads an amount such as `1234.56`, or `1.234,56` with a decimal comma.
pub(super) fn read_amount(
    text: &str,
    currency: Currency,
    decimal_comma: bool,
) -> Result<Money, DecimalError> {
    money_from_decimal(&plain_number(text, decimal_comma), currency)
}

/// Reads a decimal number such as an exchange rate.
pub(super) fn read_decimal(text: &str, decimal_comma: bool) -> Option<Decimal> {
    let plain = plain_number(text, decimal_comma);
    Decimal::from_str(&plain)
        .or_else(|_| Decimal::from_scientific(&plain))
        .ok()
}

fn plain_number(text: &str, decimal_comma: bool) -> String {
    let text = text.trim();
    if decimal_comma && text.contains(',') {
        text.chars()
            .filter(|c| !matches!(c, '.' | ' ' | '\u{a0}'))
            .map(|c| if c == ',' { '.' } else { c })
            .collect()
    } else {
        text.to_owned()
    }
}

/// Reads the day a timestamp or date falls on, as the file writes it:
/// `2026-09-01`, `2026-09-01T00:00:00Z`, `2026-09-01 00:00:00 UTC`,
/// `09/01/2026` or `Sep 1, 2026`.
pub(super) fn read_day(text: &str) -> Option<Date> {
    let text = text.trim();
    let iso = format_description!("[year]-[month]-[day]");
    if let Some(day) = text.get(..10).and_then(|head| Date::parse(head, iso).ok()) {
        return Some(day);
    }
    let first_word = text.split([' ', 'T']).next().unwrap_or(text);
    let us = format_description!("[month padding:none]/[day padding:none]/[year]");
    if let Ok(day) = Date::parse(first_word, us) {
        return Some(day);
    }
    let written = format_description!("[month repr:short] [day padding:none], [year]");
    Date::parse(text, written).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    fn rows(text: &str) -> Vec<(u64, Vec<String>)> {
        Sheet::new(text)
            .map(|row| {
                let (line, record) = row.unwrap();
                (line, record.iter().map(str::to_owned).collect())
            })
            .collect()
    }

    #[test]
    fn commas_semicolons_quotes_and_a_bom_are_read() {
        assert_eq!(
            rows("a,b\n1,\"x, y\"\n"),
            vec![
                (1, vec!["a".to_owned(), "b".to_owned()]),
                (2, vec!["1".to_owned(), "x, y".to_owned()])
            ]
        );
        let semicolons = "\u{feff}a;b\n\n\"1,5\";\"say \"\"hi\"\"\"\n;\n";
        assert_eq!(
            rows(semicolons),
            vec![
                (1, vec!["a".to_owned(), "b".to_owned()]),
                (3, vec!["1,5".to_owned(), "say \"hi\"".to_owned()])
            ]
        );
        assert!(Sheet::new(semicolons).decimal_comma());
        assert!(!Sheet::new("a,b").decimal_comma());
        assert_eq!(separator("a;b,c;d"), b';');
        assert_eq!(separator("a,b;c,d"), b',');
        assert_eq!(separator("\"a;b;c\",d"), b',');
    }

    #[test]
    fn column_names_have_one_key_in_every_spelling() {
        for (name, key) in [
            ("lineItem/UsageStartDate", "line_item_usage_start_date"),
            ("line_item_usage_start_date", "line_item_usage_start_date"),
            ("Service description", "service_description"),
            ("service.description", "service_description"),
            ("SKU description", "sku_description"),
            ("CostInBillingCurrency", "cost_in_billing_currency"),
            ("costInBillingCurrency", "cost_in_billing_currency"),
            ("x_SkuMeterCategory", "x_sku_meter_category"),
            (" Exchange rate ", "exchange_rate"),
            ("\u{feff}period", "period"),
        ] {
            assert_eq!(column_key(name), (key.to_owned(), None), "{name}");
        }
        assert_eq!(
            column_key("Cost (EUR)"),
            ("cost".to_owned(), Some(Currency::EUR))
        );
        assert_eq!(
            column_key("Unrounded cost ($)"),
            ("unrounded_cost".to_owned(), None)
        );
    }

    #[test]
    fn amounts_may_use_a_decimal_comma_in_semicolon_files() {
        let nanos =
            |text: &str, comma: bool| read_amount(text, Currency::EUR, comma).map(Money::nanos);
        assert_eq!(nanos("1234.56", false), Ok(1_234_560_000_000));
        assert_eq!(nanos("1.234,56", true), Ok(1_234_560_000_000));
        assert_eq!(nanos("1 234,5", true), Ok(1_234_500_000_000));
        assert_eq!(nanos("0.5", true), Ok(500_000_000));
        assert_eq!(nanos("-1,5", true), Ok(-1_500_000_000));
        assert!(nanos("1,5", false).is_err());
        assert_eq!(read_decimal("0,92", true), Some(Decimal::new(92, 2)));
        assert_eq!(read_decimal("x", false), None);
    }

    #[test]
    fn days_are_read_in_the_formats_exports_use() {
        for text in [
            "2026-09-01",
            "2026-09-01T00:00:00Z",
            "2026-09-01T00:00:00.000Z",
            "2026-09-01 07:00:00 UTC",
            "2026-09-01T00:00:00-07:00",
            "09/01/2026",
            "9/1/2026",
            "9/1/2026 12:00:00 AM",
            "Sep 1, 2026",
        ] {
            assert_eq!(read_day(text), Some(date!(2026 - 09 - 01)), "{text}");
        }
        for text in ["", "yesterday", "2026-13-01", "13/01/2026"] {
            assert_eq!(read_day(text), None, "{text}");
        }
    }

    #[test]
    fn preamble_lines_are_kept_by_key() {
        let mut preamble = BTreeMap::new();
        for (_, record) in
            Sheet::new("Billing account ID,0X0X0X\nCurrency,EUR\na,b,c\n").map(Result::unwrap)
        {
            preamble_entry(&record, &mut preamble);
        }
        assert_eq!(preamble.get("currency").map(String::as_str), Some("EUR"));
        assert_eq!(
            preamble.get("billing_account_id").map(String::as_str),
            Some("0X0X0X")
        );
        assert_eq!(preamble.len(), 2);
    }
}
