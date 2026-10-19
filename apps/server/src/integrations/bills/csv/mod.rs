//! Bills uploaded as CSV: our own format, one bill per row in the bill
//! schema's columns, and the cost exports of Bedrock, Vertex and Foundry,
//! one bill per file. Errors name the row and column in plain words, for
//! the preview shown before anything is saved.

mod cloud;
mod sheet;

use std::collections::BTreeMap;
use std::fmt;

use ::csv::StringRecord;
use rust_decimal::Decimal;
use time::Date;
use tokencontroller_core::{Currency, DateRange, Money};

use self::sheet::{Column, Header, Sheet, cell, read_amount, read_day, read_decimal};
use super::bill::{BillLine, BillProvider, PulledBill};
use crate::integrations::bills::month::{BillMonth, Charging};

pub use self::cloud::Cloud;

/// What is wrong with an uploaded file, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvBillError {
    /// The line in the file, counting the header as line 1, as a spreadsheet
    /// numbers its rows.
    pub row: Option<u64>,
    /// The column as the file names it.
    pub column: Option<String>,
    /// What is wrong, in plain words.
    pub message: String,
}

impl CsvBillError {
    fn file(message: impl Into<String>) -> Self {
        Self {
            row: None,
            column: None,
            message: message.into(),
        }
    }

    fn at(row: u64, column: Option<&Column>, message: impl Into<String>) -> Self {
        Self {
            row: Some(row),
            column: column.map(|column| column.name.clone()),
            message: message.into(),
        }
    }
}

impl fmt::Display for CsvBillError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.row, &self.column) {
            (Some(row), Some(column)) => write!(f, "Row {row}, column {column}: {}", self.message),
            (Some(row), None) => write!(f, "Row {row}: {}", self.message),
            (None, Some(column)) => write!(f, "Column {column}: {}", self.message),
            (None, None) => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for CsvBillError {}

/// One bill as a file holds it, before the bills area stores it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadedBill {
    /// The provider as bills store it, such as `anthropic` or `cursor`.
    pub provider: String,
    pub range: DateRange,
    pub amount: Money,
    /// The rate to the reporting currency given with the bill, if any.
    pub exchange_rate: Option<Decimal>,
    /// The provider's charging, when a cloud export names it.
    pub charging: Option<Charging>,
    /// The lines of a cloud export; empty for our own format.
    pub lines: Vec<BillLine>,
}

impl UploadedBill {
    /// A cloud export's bill as an uploaded one: the month's days as the
    /// range, the net as the amount.
    ///
    /// # Errors
    ///
    /// [`CsvBillError`] when the month has no days, which no month read
    /// from a file lacks.
    pub fn from_pulled(bill: &PulledBill) -> Result<Self, CsvBillError> {
        let range = bill.period().range().ok_or_else(|| {
            CsvBillError::file(format!("{} is not a month with days.", bill.period()))
        })?;
        Ok(Self {
            provider: bill.provider_name().to_owned(),
            range,
            amount: bill.net(),
            exchange_rate: bill.exchange_rate(),
            charging: Some(bill.cost_source()),
            lines: bill.lines().to_vec(),
        })
    }

    #[must_use]
    pub fn currency(&self) -> Currency {
        self.amount.currency()
    }
}

/// The columns of our own format, as the bill schema names them.
const REQUIRED: [&str; 5] = ["provider", "from", "to", "amount", "currency"];
const OPTIONAL: [&str; 1] = ["exchange_rate"];
const SCHEMA: &str = "provider, from, to, amount and currency, and optionally exchange_rate";

/// Where each column of our own format is.
struct BillColumns {
    provider: Column,
    from: Column,
    to: Column,
    amount: Column,
    currency: Column,
    exchange_rate: Option<Column>,
}

/// Reads bills in our own format: a header naming the fields of the bill
/// schema (`provider,from,to,amount,currency`, and optionally
/// `exchange_rate`), then one row per bill. Commas or semicolons, quoted
/// fields and a byte order mark all work.
///
/// # Errors
///
/// [`CsvBillError`] for the first problem found, naming its row and column:
/// a missing or unknown column, a value that cannot be read, a range that
/// ends before it starts, or a second bill for the same provider and
/// range.
pub fn parse_bills(text: &str) -> Result<Vec<UploadedBill>, CsvBillError> {
    let mut sheet = Sheet::new(text);
    let decimal_comma = sheet.decimal_comma();
    let Some(first) = sheet.next() else {
        return Err(CsvBillError::file(format!(
            "The file is empty. It needs a header row naming {SCHEMA}, then one row per bill."
        )));
    };
    let (_, header_record) = first?;
    let header = Header::new(&header_record);
    let columns = bill_columns(&header)?;

    let mut bills = Vec::new();
    let mut seen: BTreeMap<(String, Date, Date), u64> = BTreeMap::new();
    for row in sheet {
        let (number, record) = row?;
        let extra = record
            .iter()
            .skip(header.len())
            .any(|value| !value.trim().is_empty());
        if extra {
            return Err(CsvBillError::at(
                number,
                None,
                format!(
                    "This row has more values than the header has columns ({}).",
                    header.len()
                ),
            ));
        }
        let bill = read_bill(&columns, number, &record, decimal_comma)?;
        let key = (bill.provider.clone(), bill.range.from, bill.range.to);
        if let Some(first_row) = seen.insert(key, number) {
            return Err(CsvBillError::at(
                number,
                None,
                format!(
                    "This is a second bill for {} from {} to {}; row {first_row} already is \
                     one. Give each bill once per provider and range.",
                    bill.provider, bill.range.from, bill.range.to
                ),
            ));
        }
        bills.push(bill);
    }
    if bills.is_empty() {
        return Err(CsvBillError::file(
            "The file has a header but no bills. Add one row per bill below the header.",
        ));
    }
    Ok(bills)
}

/// Reads one cloud's own cost export as the bill for `month`. Only rows of
/// the cloud's AI service that fall inside the month count: usage makes up
/// gross, discounts and credits their own figures, and tax and flat fees
/// are left out. The bill's lines add up usage per day, account or project
/// and kind of usage.
///
/// # Errors
///
/// [`CsvBillError`] if the file is not that cloud's export, a value cannot
/// be read, rows are in different currencies, or no row is the AI
/// service's in the month.
pub fn parse_cloud_export(
    cloud: Cloud,
    text: &str,
    month: BillMonth,
) -> Result<PulledBill, CsvBillError> {
    cloud::parse(cloud, text, month)
}

fn bill_columns(header: &Header) -> Result<BillColumns, CsvBillError> {
    let mut found: BTreeMap<&str, &str> = BTreeMap::new();
    for (key, name) in header.columns() {
        if !REQUIRED.contains(&key) && !OPTIONAL.contains(&key) {
            return Err(CsvBillError::file(format!(
                "The header has a column {name:?} that is not part of the bill schema. \
                 Use {SCHEMA}."
            )));
        }
        if found.insert(key, name).is_some() {
            return Err(CsvBillError::file(format!(
                "The header names {key} twice. Give each column once."
            )));
        }
    }
    let required = |key: &str| {
        header.find(&[key]).ok_or_else(|| {
            CsvBillError::file(format!(
                "The header has no {key} column. A bill needs the columns {SCHEMA}."
            ))
        })
    };
    Ok(BillColumns {
        provider: required("provider")?,
        from: required("from")?,
        to: required("to")?,
        amount: required("amount")?,
        currency: required("currency")?,
        exchange_rate: header.find(&["exchange_rate"]),
    })
}

fn read_bill(
    columns: &BillColumns,
    row: u64,
    record: &StringRecord,
    decimal_comma: bool,
) -> Result<UploadedBill, CsvBillError> {
    let value = |column: &Column| cell(record, Some(column));
    let problem = |column: &Column, message: String| CsvBillError::at(row, Some(column), message);

    let provider = provider_cell(row, &columns.provider, value(&columns.provider))?;

    let day = |column: &Column| {
        let text = value(column);
        read_day(text).ok_or_else(|| {
            problem(
                column,
                format!("{text:?} is not a day. Write it as YYYY-MM-DD, such as 2026-03-31."),
            )
        })
    };
    let from = day(&columns.from)?;
    let to = day(&columns.to)?;
    let range = DateRange::new(from, to).map_err(|_| {
        problem(
            &columns.to,
            format!("The bill ends on {to}, before it starts on {from}."),
        )
    })?;

    let currency_text = value(&columns.currency);
    let currency = Currency::new(&currency_text.to_ascii_uppercase()).map_err(|_| {
        problem(
            &columns.currency,
            format!("{currency_text:?} is not a currency. Use a three-letter code such as USD."),
        )
    })?;

    let amount_text = value(&columns.amount);
    if amount_text.is_empty() {
        return Err(problem(
            &columns.amount,
            "The amount is empty. Write the amount, or 0.".to_owned(),
        ));
    }
    let amount = read_amount(amount_text, currency, decimal_comma).map_err(|_| {
        problem(
            &columns.amount,
            format!("{amount_text:?} is not an amount. Write a plain number such as 1234.56."),
        )
    })?;
    if amount.nanos().is_negative() {
        return Err(problem(
            &columns.amount,
            format!("{amount_text:?} is below zero. A bill's amount is zero or more."),
        ));
    }

    let exchange_rate = match columns.exchange_rate.as_ref() {
        Some(column) if !value(column).is_empty() => {
            let text = value(column);
            let rate = read_decimal(text, decimal_comma)
                .filter(|rate| rate.is_sign_positive() && !rate.is_zero())
                .ok_or_else(|| {
                    problem(
                        column,
                        format!(
                            "{text:?} is not an exchange rate. Write a number above zero, \
                             such as 0.92, or leave it empty."
                        ),
                    )
                })?;
            Some(rate)
        }
        _ => None,
    };

    Ok(UploadedBill {
        provider,
        range,
        amount,
        exchange_rate,
        charging: None,
        lines: Vec::new(),
    })
}

/// The provider in a row, as bills store it.
fn provider_cell(row: u64, column: &Column, text: &str) -> Result<String, CsvBillError> {
    read_provider(text).ok_or_else(|| {
        CsvBillError::at(
            row,
            Some(column),
            format!(
                "{text:?} is not a provider name. Use lower-case letters, digits and _, such \
                 as anthropic, openai or cursor."
            ),
        )
    })
}

/// Reads a provider: a name Token Controller knows, such as `Bedrock`, or
/// any other name of up to 40 lower-case letters, digits and `_` once case,
/// spaces and hyphens are set aside, such as `cursor`.
fn read_provider(text: &str) -> Option<String> {
    if let Some(known) = BillProvider::from_name(text) {
        return Some(known.as_str().to_owned());
    }
    let key: String = text
        .trim()
        .chars()
        .map(|c| match c {
            ' ' | '-' => '_',
            other => other.to_ascii_lowercase(),
        })
        .collect();
    let plain = !key.is_empty()
        && key.len() <= 40
        && key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    plain.then_some(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    const BILLS: &str = include_str!("../../../../tests/fixtures/bills/bills.csv");
    const BILLS_SEMICOLON: &str =
        include_str!("../../../../tests/fixtures/bills/bills_semicolon.csv");

    fn error(text: &str) -> String {
        parse_bills(text).unwrap_err().to_string()
    }

    const HEADER: &str = "provider,from,to,amount,currency\n";

    #[test]
    fn our_own_format_reads_one_bill_per_row() {
        let bills = parse_bills(BILLS).unwrap();
        assert_eq!(bills.len(), 3);

        let first = &bills[0];
        assert_eq!(first.provider, "openai");
        assert_eq!(first.range.from, date!(2026 - 03 - 01));
        assert_eq!(first.range.to, date!(2026 - 03 - 31));
        assert_eq!(first.currency(), Currency::USD);
        assert_eq!(first.amount.nanos(), 1_234_560_000_000);
        assert_eq!(first.exchange_rate, Some(Decimal::new(92, 2)));
        assert_eq!(first.charging, None);
        assert!(first.lines.is_empty());

        let second = &bills[1];
        assert_eq!(second.provider, "anthropic");
        assert_eq!(second.amount.nanos(), 1_000_000_000_000_000);
        assert_eq!(second.exchange_rate, None);

        let third = &bills[2];
        assert_eq!(third.provider, "cursor");
        assert_eq!(third.currency(), Currency::EUR);
        assert_eq!(third.range.days(), 28);
    }

    #[test]
    fn semicolons_decimal_commas_quotes_and_a_bom_are_read() {
        let bills = parse_bills(BILLS_SEMICOLON).unwrap();
        assert_eq!(bills.len(), 2);
        assert_eq!(bills[0].amount.nanos(), 1_234_560_000_000);
        assert_eq!(bills[0].currency(), Currency::EUR);
        assert_eq!(bills[0].provider, "aws_bedrock");
        assert_eq!(bills[0].exchange_rate, Some(Decimal::new(108, 2)));
        assert_eq!(bills[1].range.from, date!(2026 - 02 - 01));
    }

    #[test]
    fn the_header_must_name_the_bill_schema() {
        assert_eq!(
            error(""),
            "The file is empty. It needs a header row naming provider, from, to, amount and \
             currency, and optionally exchange_rate, then one row per bill."
        );
        assert_eq!(
            error(HEADER),
            "The file has a header but no bills. Add one row per bill below the header."
        );
        assert_eq!(
            error("provider,from,to,currency\nopenai,2026-03-01,2026-03-31,USD\n"),
            "The header has no amount column. A bill needs the columns provider, from, to, \
             amount and currency, and optionally exchange_rate."
        );
        assert_eq!(
            error("provider,from,to,amount,currency,notes\n"),
            "The header has a column \"notes\" that is not part of the bill schema. Use \
             provider, from, to, amount and currency, and optionally exchange_rate."
        );
        assert_eq!(
            error("provider,from,to,amount,currency,amount\n"),
            "The header names amount twice. Give each column once."
        );
        // Header names may use any case and spaces.
        let bills = parse_bills(
            "Provider,From,To,Amount,Currency,Exchange rate\n\
             OpenAI,2026-03-01,2026-03-31,10,usd,\n",
        )
        .unwrap();
        assert_eq!(bills[0].provider, "openai");
        assert_eq!(bills[0].currency(), Currency::USD);
        assert_eq!(bills[0].exchange_rate, None);
    }

    #[test]
    fn values_that_cannot_be_read_name_their_row_and_column() {
        let row = |values: &str| error(&format!("{HEADER}{values}\n"));
        assert_eq!(
            row("openai,March 2026,2026-03-31,10,USD"),
            "Row 2, column from: \"March 2026\" is not a day. Write it as YYYY-MM-DD, such \
             as 2026-03-31."
        );
        assert_eq!(
            row("openai,2026-03-31,2026-03-01,10,USD"),
            "Row 2, column to: The bill ends on 2026-03-01, before it starts on 2026-03-31."
        );
        assert_eq!(
            row("openai,2026-03-01,2026-03-31,10,dollars"),
            "Row 2, column currency: \"dollars\" is not a currency. Use a three-letter code \
             such as USD."
        );
        assert_eq!(
            row("openai,2026-03-01,2026-03-31,ten,USD"),
            "Row 2, column amount: \"ten\" is not an amount. Write a plain number such as \
             1234.56."
        );
        assert_eq!(
            row("openai,2026-03-01,2026-03-31,,USD"),
            "Row 2, column amount: The amount is empty. Write the amount, or 0."
        );
        assert_eq!(
            row("openai,2026-03-01,2026-03-31,-5,USD"),
            "Row 2, column amount: \"-5\" is below zero. A bill's amount is zero or more."
        );
        assert_eq!(
            row("openai,2026-03-01,2026-03-31,10,USD,extra"),
            "Row 2: This row has more values than the header has columns (5)."
        );
        let with_rate = "provider,from,to,amount,currency,exchange_rate\n\
                         openai,2026-03-01,2026-03-31,10,USD,-1\n";
        assert_eq!(
            error(with_rate),
            "Row 2, column exchange_rate: \"-1\" is not an exchange rate. Write a number \
             above zero, such as 0.92, or leave it empty."
        );
        assert_eq!(
            row("Mistral AI!,2026-03-01,2026-03-31,10,USD"),
            "Row 2, column provider: \"Mistral AI!\" is not a provider name. Use lower-case \
             letters, digits and _, such as anthropic, openai or cursor."
        );
    }

    #[test]
    fn a_range_has_one_bill_per_provider() {
        let text = "provider,from,to,amount,currency\n\
                    openai,2026-03-01,2026-03-31,10,USD\n\
                    anthropic,2026-03-01,2026-03-31,10,USD\n\
                    \n\
                    openai,2026-03-01,2026-03-31,12,USD\n";
        assert_eq!(
            error(text),
            "Row 5: This is a second bill for openai from 2026-03-01 to 2026-03-31; row 2 \
             already is one. Give each bill once per provider and range."
        );
    }

    #[test]
    fn other_providers_keep_their_name() {
        let text = "provider,from,to,amount,currency\n\
                    Cursor,2026-03-01,2026-03-31,10,USD\n\
                    Amazon Bedrock,2026-03-01,2026-03-31,10,USD\n";
        let bills = parse_bills(text).unwrap();
        assert_eq!(bills[0].provider, "cursor");
        assert_eq!(bills[1].provider, "aws_bedrock");
    }

    #[test]
    fn a_cloud_export_becomes_an_uploaded_bill_for_the_month() {
        let text = include_str!("../../../../tests/fixtures/bills/bedrock_cur2.csv");
        let month = BillMonth::parse("2026-09").unwrap();
        let pulled = parse_cloud_export(Cloud::Bedrock, text, month).unwrap();
        let uploaded = UploadedBill::from_pulled(&pulled).unwrap();
        assert_eq!(uploaded.provider, "aws_bedrock");
        assert_eq!(uploaded.range, month.range().unwrap());
        assert_eq!(uploaded.amount, pulled.net());
        assert_eq!(uploaded.charging, Some(Charging::CloudMarketplace));
        assert_eq!(uploaded.lines.len(), pulled.lines().len());
    }

    #[test]
    fn errors_without_a_row_or_column_read_as_plain_sentences() {
        let error = CsvBillError {
            row: None,
            column: Some("cost".to_owned()),
            message: "Missing.".to_owned(),
        };
        assert_eq!(error.to_string(), "Column cost: Missing.");
    }
}
