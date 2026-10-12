//! Bills uploaded as CSV: our own format, one bill per row, and the cost
//! exports of Bedrock, Vertex and Foundry, one bill per file. Errors name
//! the row and column in plain words, for the preview shown before
//! anything is saved.

mod cloud;
mod sheet;

use std::collections::BTreeMap;
use std::fmt;

use ::csv::StringRecord;
use tokencontroller_core::{Currency, Money};

use self::sheet::{Column, Header, Sheet, cell, read_amount, read_decimal};
use super::bill::{BillError, BillFigures, BillOrigin, BillProvider, PulledBill};
use tokencontroller_core::{CostSource, Period};

pub use self::cloud::Cloud;

/// The bill's source API for our own CSV format, as
/// [`PulledBill::source_api`] gives it.
pub const SOURCE_API: &str = "csv";

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

/// The columns of our own format, as the bill schema names them.
const REQUIRED: [&str; 7] = [
    "period",
    "cost_source",
    "gross",
    "discount",
    "credit",
    "net",
    "currency",
];
const OPTIONAL: [&str; 2] = ["exchange_rate", "provider"];
const SCHEMA: &str = "period, cost_source, gross, discount, credit, net and currency, \
     and optionally exchange_rate and provider";

/// Where each column of our own format is.
struct BillColumns {
    period: Column,
    cost_source: Column,
    gross: Column,
    discount: Column,
    credit: Column,
    net: Column,
    currency: Column,
    exchange_rate: Option<Column>,
    provider: Option<Column>,
}

/// Reads bills in our own format: a header naming the fields of the bill
/// schema (`period,cost_source,gross,discount,credit,net,currency`, and
/// optionally `exchange_rate` and `provider`), then one row per bill.
/// Commas or semicolons, quoted fields and a byte order mark all work. An
/// empty discount or credit is zero; a missing provider is `other`.
///
/// # Errors
///
/// [`CsvBillError`] for the first problem found, naming its row and column:
/// a missing or unknown column, a value that cannot be read, a net that is
/// not gross minus discount minus credit, or a second bill for the same
/// provider, cost source and period.
pub fn parse_bills(text: &str) -> Result<Vec<PulledBill>, CsvBillError> {
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
    let mut seen: BTreeMap<String, u64> = BTreeMap::new();
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
        if let Some(first_row) = seen.insert(bill.source_id(), number) {
            return Err(CsvBillError::at(
                number,
                None,
                format!(
                    "This is a second bill for {}, {} in {}; row {first_row} already is one. \
                     Give each period's bill once per provider and cost source.",
                    bill.provider_name(),
                    cost_source_name(bill.cost_source()),
                    bill.period()
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

/// Reads one cloud's own cost export as the bill for `period`. Only rows of
/// the cloud's AI service that fall inside the period count: usage makes up
/// gross, discounts and credits their own figures, and tax and flat fees
/// are left out. The bill's lines add up usage per day, account or project
/// and kind of usage.
///
/// # Errors
///
/// [`CsvBillError`] if the file is not that cloud's export, a value cannot
/// be read, rows are in different currencies, or no row is the AI
/// service's in the period.
pub fn parse_cloud_export(
    cloud: Cloud,
    text: &str,
    period: Period,
) -> Result<PulledBill, CsvBillError> {
    cloud::parse(cloud, text, period)
}

fn bill_columns(header: &Header) -> Result<BillColumns, CsvBillError> {
    let mut found: BTreeMap<&str, &str> = BTreeMap::new();
    for (key, name) in header.columns() {
        if key == "revision" {
            return Err(CsvBillError::file(
                "The header has a revision column. Token Controller sets the revision when a \
                 provider restates a bill, so leave the column out.",
            ));
        }
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
        period: required("period")?,
        cost_source: required("cost_source")?,
        gross: required("gross")?,
        discount: required("discount")?,
        credit: required("credit")?,
        net: required("net")?,
        currency: required("currency")?,
        exchange_rate: header.find(&["exchange_rate"]),
        provider: header.find(&["provider"]),
    })
}

fn read_bill(
    columns: &BillColumns,
    row: u64,
    record: &StringRecord,
    decimal_comma: bool,
) -> Result<PulledBill, CsvBillError> {
    let value = |column: &Column| cell(record, Some(column));
    let problem = |column: &Column, message: String| CsvBillError::at(row, Some(column), message);

    let period_text = value(&columns.period);
    let period = Period::parse(period_text).map_err(|_| {
        problem(
            &columns.period,
            format!(
                "{period_text:?} is not a period. Write the calendar month as YYYY-MM, \
                 such as 2026-09."
            ),
        )
    })?;

    let cost_source = cost_source_cell(row, &columns.cost_source, value(&columns.cost_source))?;

    let currency_text = value(&columns.currency);
    let currency = Currency::new(&currency_text.to_ascii_uppercase()).map_err(|_| {
        problem(
            &columns.currency,
            format!("{currency_text:?} is not a currency. Use a three-letter code such as USD."),
        )
    })?;

    let amount = |column: &Column, empty_is_zero: bool| {
        let text = value(column);
        if text.is_empty() {
            return if empty_is_zero {
                Ok(Money::zero(currency))
            } else {
                Err(problem(
                    column,
                    "The amount is empty. Write the amount, or 0.".to_owned(),
                ))
            };
        }
        read_amount(text, currency, decimal_comma).map_err(|_| {
            problem(
                column,
                format!("{text:?} is not an amount. Write a plain number such as 1234.56."),
            )
        })
    };
    let figures = BillFigures {
        gross: amount(&columns.gross, false)?,
        discount: amount(&columns.discount, true)?,
        credit: amount(&columns.credit, true)?,
        net: amount(&columns.net, false)?,
    };

    let (provider, provider_name) = match columns.provider.as_ref() {
        Some(column) if !value(column).is_empty() => provider_cell(row, column, value(column))?,
        _ => (BillProvider::Other, None),
    };

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

    let origin = BillOrigin {
        provider,
        cost_source,
        source_api: SOURCE_API,
        scope: format!(
            "{}/{}",
            provider_name
                .as_deref()
                .unwrap_or_else(|| provider.as_str()),
            cost_source_name(cost_source)
        ),
    };
    let mut bill = PulledBill::new(origin, period, figures, Vec::new(), true)
        .map_err(|error| figures_problem(row, &columns.net, &error))?;
    if let Some(name) = provider_name {
        bill = bill.with_provider_name(name);
    }
    Ok(match exchange_rate {
        Some(rate) => bill.with_exchange_rate(rate),
        None => bill,
    })
}

fn figures_problem(row: u64, net: &Column, error: &BillError) -> CsvBillError {
    let message = match error {
        BillError::DoesNotAddUp {
            gross,
            discount,
            credit,
            net,
            expected,
        } => format!(
            "Gross {gross} minus discount {discount} minus credit {credit} is {expected}, \
             but net is {net}. Net must be gross minus discount minus credit."
        ),
        other => format!("The amounts do not hold together: {other}."),
    };
    CsvBillError::at(row, Some(net), message)
}

/// The cost source in a row. Seats and subscriptions have no bill.
fn cost_source_cell(row: u64, column: &Column, text: &str) -> Result<CostSource, CsvBillError> {
    let cost_source = read_cost_source(text).ok_or_else(|| {
        CsvBillError::at(
            row,
            Some(column),
            format!(
                "{text:?} is not a cost source. Use usage_at_api_rates, api_key, \
                 cloud_marketplace or reseller."
            ),
        )
    })?;
    if matches!(cost_source, CostSource::Subscription | CostSource::Seat) {
        return Err(CsvBillError::at(
            row,
            Some(column),
            "Seats and subscriptions are flat fees and have no bill. Leave this row out; \
             finance handles them as before.",
        ));
    }
    Ok(cost_source)
}

/// The provider in a row, and its name when Token Controller has no
/// [`BillProvider`] for it.
fn provider_cell(
    row: u64,
    column: &Column,
    text: &str,
) -> Result<(BillProvider, Option<String>), CsvBillError> {
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

/// Reads a cost source as the bill schema spells it, ignoring case, spaces
/// and hyphens, so `API key` and `usage at API rates` work too.
fn read_cost_source(text: &str) -> Option<CostSource> {
    let key: String = text
        .trim()
        .chars()
        .map(|c| match c {
            ' ' | '-' => '_',
            other => other.to_ascii_lowercase(),
        })
        .collect();
    match key.as_str() {
        "subscription" => Some(CostSource::Subscription),
        "seat" => Some(CostSource::Seat),
        "usage_at_api_rates" => Some(CostSource::UsageAtApiRates),
        "api_key" => Some(CostSource::ApiKey),
        "cloud_marketplace" => Some(CostSource::CloudMarketplace),
        "reseller" => Some(CostSource::Reseller),
        _ => None,
    }
}

/// Reads a provider: a name Token Controller knows, such as `Bedrock`, or
/// any other name of up to 40 lower-case letters, digits and `_` once case,
/// spaces and hyphens are set aside, such as `cursor`. The second value is
/// that other name.
fn read_provider(text: &str) -> Option<(BillProvider, Option<String>)> {
    if let Some(known) = BillProvider::from_name(text) {
        return Some((known, None));
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
    plain.then_some((BillProvider::Other, Some(key)))
}

/// A cost source as the bill schema spells it.
const fn cost_source_name(cost_source: CostSource) -> &'static str {
    match cost_source {
        CostSource::Subscription => "subscription",
        CostSource::Seat => "seat",
        CostSource::UsageAtApiRates => "usage_at_api_rates",
        CostSource::ApiKey => "api_key",
        CostSource::CloudMarketplace => "cloud_marketplace",
        CostSource::Reseller => "reseller",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    const BILLS: &str = include_str!("../../../../tests/fixtures/bills/bills.csv");
    const BILLS_SEMICOLON: &str =
        include_str!("../../../../tests/fixtures/bills/bills_semicolon.csv");

    fn error(text: &str) -> String {
        parse_bills(text).unwrap_err().to_string()
    }

    const HEADER: &str = "period,cost_source,gross,discount,credit,net,currency\n";

    #[test]
    fn our_own_format_reads_one_bill_per_row() {
        let bills = parse_bills(BILLS).unwrap();
        assert_eq!(bills.len(), 3);

        let first = &bills[0];
        assert_eq!(first.period().to_string(), "2026-09");
        assert_eq!(first.cost_source(), CostSource::ApiKey);
        assert_eq!(first.provider(), BillProvider::OpenAi);
        assert_eq!(first.currency(), Currency::USD);
        assert_eq!(first.gross().nanos(), 1_234_560_000_000);
        assert!(first.discount().is_zero());
        assert!(first.credit().is_zero());
        assert_eq!(first.net(), first.gross());
        assert_eq!(first.exchange_rate(), Some(Decimal::new(92, 2)));
        assert_eq!(first.source_api(), "csv");
        assert_eq!(first.source_id(), "csv:openai/api_key:2026-09");
        assert!(first.is_final());
        assert!(first.lines().is_empty());

        let second = &bills[1];
        assert_eq!(second.provider(), BillProvider::Anthropic);
        assert_eq!(second.cost_source(), CostSource::UsageAtApiRates);
        assert_eq!(second.gross().nanos(), 10_000_000_000_000);
        assert_eq!(second.discount().nanos(), 1_500_000_000_000);
        assert_eq!(second.credit().nanos(), 250_000_000_000);
        assert_eq!(second.net().nanos(), 8_250_000_000_000);
        assert_eq!(second.exchange_rate(), None);

        let third = &bills[2];
        assert_eq!(third.provider(), BillProvider::Other);
        assert_eq!(third.currency(), Currency::EUR);
        assert_eq!(third.cost_source(), CostSource::Reseller);
    }

    #[test]
    fn semicolons_decimal_commas_quotes_and_a_bom_are_read() {
        let bills = parse_bills(BILLS_SEMICOLON).unwrap();
        assert_eq!(bills.len(), 2);
        assert_eq!(bills[0].gross().nanos(), 1_234_560_000_000);
        assert_eq!(bills[0].discount().nanos(), 100_000_000_000);
        assert_eq!(bills[0].net().nanos(), 1_134_560_000_000);
        assert_eq!(bills[0].currency(), Currency::EUR);
        assert_eq!(bills[0].provider(), BillProvider::AwsBedrock);
        assert_eq!(bills[0].cost_source(), CostSource::CloudMarketplace);
        assert_eq!(bills[0].exchange_rate(), Some(Decimal::new(108, 2)));
        assert_eq!(bills[1].period().to_string(), "2026-08");
    }

    #[test]
    fn the_header_must_name_the_bill_schema() {
        assert_eq!(
            error(""),
            "The file is empty. It needs a header row naming period, cost_source, gross, \
             discount, credit, net and currency, and optionally exchange_rate and provider, \
             then one row per bill."
        );
        assert_eq!(
            error(HEADER),
            "The file has a header but no bills. Add one row per bill below the header."
        );
        assert_eq!(
            error("period,cost_source,gross,discount,credit,currency\n2026-09,api_key,1,0,0,USD\n"),
            "The header has no net column. A bill needs the columns period, cost_source, \
             gross, discount, credit, net and currency, and optionally exchange_rate and \
             provider."
        );
        assert_eq!(
            error("period,cost_source,gross,discount,credit,net,currency,notes\n"),
            "The header has a column \"notes\" that is not part of the bill schema. Use \
             period, cost_source, gross, discount, credit, net and currency, and optionally \
             exchange_rate and provider."
        );
        assert_eq!(
            error("period,cost_source,gross,discount,credit,net,currency,net\n"),
            "The header names net twice. Give each column once."
        );
        assert_eq!(
            error("period,cost_source,gross,discount,credit,net,currency,revision\n"),
            "The header has a revision column. Token Controller sets the revision when a \
             provider restates a bill, so leave the column out."
        );
        // Header names may use any case and spaces.
        let bills = parse_bills(
            "Period,Cost source,Gross,Discount,Credit,Net,Currency,Exchange rate\n\
             2026-09,API key,10,0,0,10,usd,\n",
        )
        .unwrap();
        assert_eq!(bills[0].cost_source(), CostSource::ApiKey);
        assert_eq!(bills[0].currency(), Currency::USD);
        assert_eq!(bills[0].exchange_rate(), None);
    }

    #[test]
    fn values_that_cannot_be_read_name_their_row_and_column() {
        let row = |values: &str| error(&format!("{HEADER}{values}\n"));
        assert_eq!(
            row("Sept 2026,api_key,10,0,0,10,USD"),
            "Row 2, column period: \"Sept 2026\" is not a period. Write the calendar month \
             as YYYY-MM, such as 2026-09."
        );
        assert_eq!(
            row("2026-09,credit card,10,0,0,10,USD"),
            "Row 2, column cost_source: \"credit card\" is not a cost source. Use \
             usage_at_api_rates, api_key, cloud_marketplace or reseller."
        );
        assert_eq!(
            row("2026-09,seat,10,0,0,10,USD"),
            "Row 2, column cost_source: Seats and subscriptions are flat fees and have no \
             bill. Leave this row out; finance handles them as before."
        );
        assert_eq!(
            row("2026-09,api_key,10,0,0,10,dollars"),
            "Row 2, column currency: \"dollars\" is not a currency. Use a three-letter code \
             such as USD."
        );
        assert_eq!(
            row("2026-09,api_key,ten,0,0,10,USD"),
            "Row 2, column gross: \"ten\" is not an amount. Write a plain number such as \
             1234.56."
        );
        assert_eq!(
            row("2026-09,api_key,10,0,0,,USD"),
            "Row 2, column net: The amount is empty. Write the amount, or 0."
        );
        assert_eq!(
            row("2026-09,api_key,10,0,0,10,USD,extra"),
            "Row 2: This row has more values than the header has columns (7)."
        );
        let with_rate = "period,cost_source,gross,discount,credit,net,currency,exchange_rate\n\
                         2026-09,api_key,10,0,0,10,USD,-1\n";
        assert_eq!(
            error(with_rate),
            "Row 2, column exchange_rate: \"-1\" is not an exchange rate. Write a number \
             above zero, such as 0.92, or leave it empty."
        );
        let with_provider = "period,cost_source,gross,discount,credit,net,currency,provider\n\
                             2026-09,api_key,10,0,0,10,USD,Mistral AI!\n";
        assert_eq!(
            error(with_provider),
            "Row 2, column provider: \"Mistral AI!\" is not a provider name. Use lower-case \
             letters, digits and _, such as anthropic, openai or cursor."
        );
    }

    #[test]
    fn net_must_be_gross_less_discount_and_credit() {
        assert_eq!(
            error(&format!("{HEADER}2026-09,api_key,1000,100,0,950,USD\n")),
            "Row 2, column net: Gross 1000.00 USD minus discount 100.00 USD minus credit \
             0.00 USD is 900.00 USD, but net is 950.00 USD. Net must be gross minus discount \
             minus credit."
        );
        let bills = parse_bills(&format!("{HEADER}2026-09,api_key,1000,,,1000,USD\n")).unwrap();
        assert_eq!(bills[0].net().nanos(), 1_000_000_000_000);
    }

    #[test]
    fn a_period_has_one_bill_per_provider_and_cost_source() {
        let text = "period,cost_source,gross,discount,credit,net,currency,provider\n\
                    2026-09,api_key,10,0,0,10,USD,openai\n\
                    2026-09,api_key,10,0,0,10,USD,anthropic\n\
                    \n\
                    2026-09,api_key,12,0,0,12,USD,openai\n";
        assert_eq!(
            error(text),
            "Row 5: This is a second bill for openai, api_key in 2026-09; row 2 already is \
             one. Give each period's bill once per provider and cost source."
        );
    }

    #[test]
    fn other_providers_keep_their_name() {
        let text = "period,cost_source,gross,discount,credit,net,currency,provider\n\
                    2026-09,reseller,10,0,0,10,USD,Cursor\n\
                    2026-09,api_key,10,0,0,10,USD,Amazon Bedrock\n";
        let bills = parse_bills(text).unwrap();
        assert_eq!(bills[0].provider(), BillProvider::Other);
        assert_eq!(bills[0].provider_name(), "cursor");
        assert_eq!(bills[0].source_id(), "csv:cursor/reseller:2026-09");
        assert_eq!(bills[1].provider_name(), "aws_bedrock");
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
