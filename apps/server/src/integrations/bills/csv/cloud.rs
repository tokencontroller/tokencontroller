//! The cost exports of the clouds that resell models: the AWS Cost and
//! Usage Report (legacy or Data Exports) for Bedrock, the Google Cloud
//! billing export (the cost table download or a `BigQuery` export saved as
//! CSV) for Vertex AI, and the Azure Cost Management export for Foundry.
//! Each cloud's columns and words for charges live here and nowhere else.

use std::collections::BTreeMap;

use ::csv::StringRecord;
use serde::{Deserialize, Serialize};
use time::Date;
use tokencontroller_core::{Currency, Money};

use super::CsvBillError;
use super::sheet::{Column, Header, Sheet, cell, preamble_entry, read_amount, read_day};
use crate::integrations::bills::bill::{BillError, BillLine, BillOrigin, BillProvider, PulledBill};
use crate::integrations::bills::month::{BillMonth, Charging};
use crate::integrations::decimal::money_from_json_number;

/// Rows looked at for the header, after any preamble.
const HEADER_SEARCH_ROWS: usize = 50;

/// The scope of a bill whose export names no billing account.
const UNNAMED_ACCOUNT: &str = "default";

/// A cloud whose cost export can be uploaded as a bill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cloud {
    /// Amazon Bedrock, from the AWS Cost and Usage Report.
    Bedrock,
    /// Vertex AI, from the Google Cloud billing export.
    Vertex,
    /// Foundry and Azure `OpenAI`, from the Azure Cost Management export.
    Foundry,
}

impl Cloud {
    /// The bill's source API, as [`PulledBill::source_api`] gives it.
    #[must_use]
    pub const fn source_api(self) -> &'static str {
        match self {
            Self::Bedrock => "aws_cost_export",
            Self::Vertex => "gcp_billing_export",
            Self::Foundry => "azure_cost_export",
        }
    }

    const fn provider(self) -> BillProvider {
        match self {
            Self::Bedrock => BillProvider::AwsBedrock,
            Self::Vertex => BillProvider::GcpVertex,
            Self::Foundry => BillProvider::AzureFoundry,
        }
    }

    const fn service(self) -> &'static str {
        match self {
            Self::Bedrock => "Amazon Bedrock",
            Self::Vertex => "Vertex AI",
            Self::Foundry => "Foundry",
        }
    }

    const fn export(self) -> &'static str {
        match self {
            Self::Bedrock => "an AWS Cost and Usage Report",
            Self::Vertex => "a Google Cloud billing export",
            Self::Foundry => "an Azure cost export",
        }
    }

    /// The column names each role goes by in this cloud's exports, first
    /// match wins. Names are keys as [`super::sheet::column_key`] makes them.
    const fn names(self, role: Role) -> &'static [&'static str] {
        match (self, role) {
            (Self::Bedrock, Role::Date) => &["line_item_usage_start_date"],
            (Self::Bedrock, Role::Cost) => &["line_item_unblended_cost"],
            (Self::Bedrock, Role::Currency) => &["line_item_currency_code"],
            (Self::Bedrock, Role::Kind) => &["line_item_line_item_type"],
            (Self::Bedrock, Role::ProductCode) => &["line_item_product_code"],
            (Self::Bedrock, Role::ServiceCode) => &["product_servicecode"],
            (Self::Bedrock, Role::ProductName) => &["product_product_name"],
            (Self::Bedrock, Role::ItemDescription) => &["line_item_line_item_description"],
            (Self::Bedrock, Role::BillingEntity) => &["bill_billing_entity"],
            (Self::Bedrock, Role::Description) => {
                &["line_item_usage_type", "line_item_line_item_description"]
            }
            (Self::Bedrock, Role::Workspace) => &["line_item_usage_account_id"],
            (Self::Bedrock, Role::Scope) => &["bill_payer_account_id"],

            (Self::Vertex, Role::Date) => &["usage_start_date", "usage_start_time", "usage_date"],
            (Self::Vertex, Role::InvoiceMonth) => &["invoice_month"],
            (Self::Vertex, Role::Cost) => &["unrounded_cost", "cost"],
            (Self::Vertex, Role::Currency) => &["currency"],
            (Self::Vertex, Role::Kind) => &["cost_type"],
            (Self::Vertex, Role::CreditType) => &["credit_type"],
            (Self::Vertex, Role::Credits) => &["credits"],
            (Self::Vertex, Role::ServiceDescription) => &["service_description"],
            (Self::Vertex, Role::Seller) => &["seller_name"],
            (Self::Vertex, Role::Description) => &["sku_description", "service_description"],
            (Self::Vertex, Role::Workspace) => &["project_id"],
            (Self::Vertex, Role::Scope) => &["billing_account_id"],

            (Self::Foundry, Role::Date) => &["date", "usage_date", "charge_period_start"],
            (Self::Foundry, Role::Cost) => &[
                "cost_in_billing_currency",
                "pre_tax_cost",
                "billed_cost",
                "cost",
            ],
            (Self::Foundry, Role::Currency) => {
                &["billing_currency", "billing_currency_code", "currency"]
            }
            (Self::Foundry, Role::Kind) => &["charge_type", "charge_category"],
            (Self::Foundry, Role::MeterCategory) => &["meter_category", "x_sku_meter_category"],
            (Self::Foundry, Role::ServiceName) => &["service_name"],
            (Self::Foundry, Role::ConsumedService) => &["consumed_service"],
            (Self::Foundry, Role::Publisher) => &["publisher_name"],
            (Self::Foundry, Role::ProductName) => &["product_name"],
            (Self::Foundry, Role::Description) => {
                &["meter_name", "x_sku_meter_name", "product_name"]
            }
            (Self::Foundry, Role::Workspace) => &["resource_name", "resource_id", "instance_name"],
            (Self::Foundry, Role::Scope) => &["billing_account_id", "subscription_id"],

            _ => &[],
        }
    }

    /// Roles the header must have.
    const fn required(self) -> &'static [Role] {
        match self {
            Self::Bedrock | Self::Foundry => &[Role::Date, Role::Cost],
            Self::Vertex => &[Role::Cost, Role::ServiceDescription],
        }
    }

    /// Roles of which the header must have at least one.
    const fn one_of(self) -> &'static [Role] {
        match self {
            Self::Bedrock => &[Role::ProductCode, Role::ServiceCode, Role::ProductName],
            Self::Vertex => &[Role::Date, Role::InvoiceMonth],
            Self::Foundry => &[
                Role::MeterCategory,
                Role::ServiceName,
                Role::ConsumedService,
                Role::Publisher,
                Role::ProductName,
            ],
        }
    }
}

/// What a column means, whatever the cloud calls it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Role {
    Date,
    InvoiceMonth,
    Cost,
    Currency,
    /// Usage, credit, discount, tax: the line item, cost or charge type.
    Kind,
    CreditType,
    Credits,
    ProductCode,
    ServiceCode,
    ProductName,
    ItemDescription,
    BillingEntity,
    ServiceDescription,
    Seller,
    MeterCategory,
    ServiceName,
    ConsumedService,
    Publisher,
    Description,
    Workspace,
    Scope,
}

const ROLES: [Role; 21] = [
    Role::Date,
    Role::InvoiceMonth,
    Role::Cost,
    Role::Currency,
    Role::Kind,
    Role::CreditType,
    Role::Credits,
    Role::ProductCode,
    Role::ServiceCode,
    Role::ProductName,
    Role::ItemDescription,
    Role::BillingEntity,
    Role::ServiceDescription,
    Role::Seller,
    Role::MeterCategory,
    Role::ServiceName,
    Role::ConsumedService,
    Role::Publisher,
    Role::Description,
    Role::Workspace,
    Role::Scope,
];

/// What a row adds to the bill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Charge {
    Usage,
    Discount,
    Credit,
    /// Tax, flat fees, totals: not metered usage.
    Skip,
}

/// The columns of one export, by role.
struct Columns {
    cloud: Cloud,
    found: BTreeMap<Role, Column>,
}

impl Columns {
    /// The header's columns, or `None` if it lacks one the cloud needs.
    fn find(cloud: Cloud, header: &Header) -> Option<Self> {
        let found: BTreeMap<Role, Column> = ROLES
            .iter()
            .filter_map(|role| Some((*role, header.find(cloud.names(*role))?)))
            .collect();
        let has_required = cloud.required().iter().all(|role| found.contains_key(role));
        let has_one = cloud.one_of().iter().any(|role| found.contains_key(role));
        (has_required && has_one).then_some(Self { cloud, found })
    }

    fn get(&self, role: Role) -> Option<&Column> {
        self.found.get(&role)
    }

    fn value<'r>(&self, record: &'r StringRecord, role: Role) -> &'r str {
        cell(record, self.get(role))
    }

    fn lower(&self, record: &StringRecord, role: Role) -> String {
        self.value(record, role).to_lowercase()
    }
}

/// What the rows counted so far add up to.
struct Totals {
    currency: Option<Currency>,
    scope: Option<String>,
    usage: BTreeMap<(Date, Option<String>, String), Money>,
    discounts: Vec<Money>,
    credits: Vec<Money>,
}

/// Finds the header, which may follow a few lines of preamble, then adds up
/// the AI service's rows in the month.
pub(super) fn parse(
    cloud: Cloud,
    text: &str,
    period: BillMonth,
) -> Result<PulledBill, CsvBillError> {
    let mut sheet = Sheet::new(text);
    let decimal_comma = sheet.decimal_comma();
    let mut preamble = BTreeMap::new();
    let mut found = None;
    for row in sheet.by_ref().take(HEADER_SEARCH_ROWS) {
        let (_, record) = row?;
        let header = Header::new(&record);
        if let Some(columns) = Columns::find(cloud, &header) {
            found = Some((columns, header.currency));
            break;
        }
        preamble_entry(&record, &mut preamble);
    }
    let Some((columns, header_currency)) = found else {
        return Err(not_an_export(cloud));
    };
    let fallback_currency = header_currency.or_else(|| {
        preamble
            .get("currency")
            .and_then(|code| Currency::new(&code.trim().to_ascii_uppercase()).ok())
    });

    let mut totals = Totals {
        currency: None,
        scope: None,
        usage: BTreeMap::new(),
        discounts: Vec::new(),
        credits: Vec::new(),
    };
    for row in sheet {
        let (number, record) = row?;
        let read = Row {
            columns: &columns,
            number,
            record: &record,
            decimal_comma,
        };
        read.add_to(&mut totals, period, fallback_currency)?;
    }

    let Some(currency) = totals.currency else {
        return Err(CsvBillError::file(format!(
            "No row of this export is {} cost in {period}. Check that the export covers \
             {period} and includes {}.",
            cloud.service(),
            cloud.service()
        )));
    };
    let too_large = |_| CsvBillError::file("The amounts in this export are too large to add up.");
    let discount = Money::sum(totals.discounts, currency)
        .and_then(Money::checked_neg)
        .map_err(too_large)?;
    let credit = Money::sum(totals.credits, currency)
        .and_then(Money::checked_neg)
        .map_err(too_large)?;
    let lines = totals
        .usage
        .into_iter()
        .map(|((date, workspace, description), amount)| BillLine {
            date,
            workspace,
            api_key_id: None,
            model: None,
            description,
            amount,
        })
        .collect();
    let scope = totals
        .scope
        .or_else(|| preamble.get("billing_account_id").cloned())
        .unwrap_or_else(|| UNNAMED_ACCOUNT.to_owned());
    let origin = BillOrigin {
        provider: cloud.provider(),
        cost_source: Charging::CloudMarketplace,
        source_api: cloud.source_api(),
        scope,
    };
    PulledBill::from_lines(origin, period, lines, discount, credit, true).map_err(|error| {
        CsvBillError::file(match error {
            BillError::Money(_) => "The amounts in this export are too large to add up.".to_owned(),
            other => format!("The export does not add up: {other}."),
        })
    })
}

fn not_an_export(cloud: Cloud) -> CsvBillError {
    let mut names: Vec<&str> = cloud
        .required()
        .iter()
        .filter_map(|role| cloud.names(*role).first().copied())
        .collect();
    let one_of: Vec<&str> = cloud
        .one_of()
        .iter()
        .filter_map(|role| cloud.names(*role).first().copied())
        .collect();
    names.push("and one of");
    let wanted = format!("{} {}", names.join(", "), one_of.join(", "));
    CsvBillError::file(format!(
        "This is not {} for {}: no row names the columns it needs ({wanted}).",
        cloud.export(),
        cloud.service()
    ))
}

/// One data row of an export.
struct Row<'a> {
    columns: &'a Columns,
    number: u64,
    record: &'a StringRecord,
    decimal_comma: bool,
}

impl Row<'_> {
    fn problem(&self, role: Role, message: String) -> CsvBillError {
        CsvBillError::at(self.number, self.columns.get(role), message)
    }

    fn add_to(
        &self,
        totals: &mut Totals,
        period: BillMonth,
        fallback_currency: Option<Currency>,
    ) -> Result<(), CsvBillError> {
        if !self.is_ai_service() || !self.in_period(period)? {
            return Ok(());
        }
        let charge = self.charge()?;
        if charge == Charge::Skip {
            return Ok(());
        }
        let currency = self.currency(totals.currency, fallback_currency)?;
        totals.currency = Some(currency);
        if totals.scope.is_none() {
            let scope = self.columns.value(self.record, Role::Scope);
            totals.scope = (!scope.is_empty()).then(|| scope.to_owned());
        }

        let cost_text = self.columns.value(self.record, Role::Cost);
        let cost = if cost_text.is_empty() {
            Money::zero(currency)
        } else {
            read_amount(cost_text, currency, self.decimal_comma).map_err(|_| {
                self.problem(
                    Role::Cost,
                    format!("{cost_text:?} is not an amount. Write a plain number such as 12.34."),
                )
            })?
        };
        match charge {
            Charge::Usage => {
                let key = self.line_key(period)?;
                let sum = match totals.usage.get(&key) {
                    Some(earlier) => earlier.checked_add(cost).map_err(|_| self.too_large())?,
                    None => cost,
                };
                totals.usage.insert(key, sum);
            }
            Charge::Discount => totals.discounts.push(cost),
            Charge::Credit => totals.credits.push(cost),
            Charge::Skip => {}
        }
        self.add_credits(totals, currency)
    }

    fn too_large(&self) -> CsvBillError {
        CsvBillError::at(self.number, None, "The amounts are too large to add up.")
    }

    /// Whether the row is the cloud's AI service.
    fn is_ai_service(&self) -> bool {
        let lower = |role| self.columns.lower(self.record, role);
        match self.columns.cloud {
            Cloud::Bedrock => {
                let bedrock_codes = ["amazonbedrock", "amazonbedrockservice"];
                let coded = [Role::ProductCode, Role::ServiceCode]
                    .into_iter()
                    .any(|role| bedrock_codes.contains(&lower(role).as_str()));
                let names = [Role::ProductName, Role::ItemDescription].map(lower);
                // Claude billed through AWS Marketplace has its own product
                // code; its product name or description says what it is.
                let marketplace = lower(Role::BillingEntity).contains("marketplace")
                    && [Role::ProductName, Role::ItemDescription, Role::Description]
                        .map(lower)
                        .iter()
                        .any(|text| text.contains("claude") || text.contains("anthropic"));
                coded || names.iter().any(|name| name.contains("bedrock")) || marketplace
            }
            Cloud::Vertex => {
                let service = lower(Role::ServiceDescription);
                service == "vertex ai"
                    || service.contains("claude")
                    || service.contains("anthropic")
                    || lower(Role::Seller).contains("anthropic")
            }
            Cloud::Foundry => {
                let categories = [Role::MeterCategory, Role::ServiceName].map(lower);
                let ai_category = categories.iter().any(|category| {
                    ["foundry", "openai", "cognitive services", "ai services"]
                        .iter()
                        .any(|word| category.contains(word))
                });
                let product = lower(Role::ProductName);
                ai_category
                    || lower(Role::ConsumedService) == "microsoft.cognitiveservices"
                    || lower(Role::Publisher).contains("anthropic")
                    || ["claude", "openai", "foundry"]
                        .iter()
                        .any(|word| product.contains(word))
            }
        }
    }

    /// Whether the row belongs to the month: by its invoice month where the
    /// export has one, otherwise by the day its usage started.
    fn in_period(&self, period: BillMonth) -> Result<bool, CsvBillError> {
        let month = self.columns.value(self.record, Role::InvoiceMonth);
        if !month.is_empty() {
            let spelled = match (month.get(..4), month.get(4..)) {
                (Some(year), Some(number)) if month.len() == 6 => format!("{year}-{number}"),
                _ => month.to_owned(),
            };
            let invoice = BillMonth::parse(&spelled).map_err(|_| {
                self.problem(
                    Role::InvoiceMonth,
                    format!("{month:?} is not an invoice month such as 202609."),
                )
            })?;
            return Ok(invoice == period);
        }
        let text = self.columns.value(self.record, Role::Date);
        let day = read_day(text).ok_or_else(|| {
            self.problem(
                Role::Date,
                format!("{text:?} is not a date. Exports write dates such as 2026-09-01."),
            )
        })?;
        Ok(period.contains(day.midnight().assume_utc()))
    }

    fn charge(&self) -> Result<Charge, CsvBillError> {
        let text = self.columns.value(self.record, Role::Kind);
        let key: String = text
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .map(|c| c.to_ascii_lowercase())
            .collect();
        let credit_type = self.columns.value(self.record, Role::CreditType);
        let charge = match self.columns.cloud {
            Cloud::Bedrock => match key.as_str() {
                "" | "usage" | "discountedusage" | "savingsplancoveredusage" => Some(Charge::Usage),
                "credit" | "refund" => Some(Charge::Credit),
                "savingsplannegation" => Some(Charge::Discount),
                discount if discount.ends_with("discount") => Some(Charge::Discount),
                "tax" | "fee" | "rifee" | "savingsplanrecurringfee" | "savingsplanupfrontfee" => {
                    Some(Charge::Skip)
                }
                _ => None,
            },
            Cloud::Vertex if !credit_type.is_empty() => Some(credit_charge(credit_type)),
            Cloud::Vertex => match key.as_str() {
                "" | "usage" | "regular" => Some(Charge::Usage),
                "adjustment" => Some(Charge::Credit),
                "tax" | "total" | "roundingerror" => Some(Charge::Skip),
                _ => None,
            },
            Cloud::Foundry => match key.as_str() {
                "" | "usage" => Some(Charge::Usage),
                "refund" | "credit" | "adjustment" => Some(Charge::Credit),
                "tax" | "purchase" | "unusedreservation" | "unusedsavingsplan"
                | "roundingadjustment" => Some(Charge::Skip),
                _ => None,
            },
        };
        charge.ok_or_else(|| {
            self.problem(
                Role::Kind,
                format!(
                    "{text:?} is a kind of charge Token Controller does not know, so it cannot \
                     tell whether it is usage, a discount or a credit."
                ),
            )
        })
    }

    fn currency(
        &self,
        so_far: Option<Currency>,
        fallback: Option<Currency>,
    ) -> Result<Currency, CsvBillError> {
        let code = self.columns.value(self.record, Role::Currency);
        let currency = if code.is_empty() {
            fallback.ok_or_else(|| {
                CsvBillError::at(
                    self.number,
                    None,
                    "The export does not say its currency. Include the currency column.",
                )
            })?
        } else {
            Currency::new(&code.to_ascii_uppercase()).map_err(|_| {
                self.problem(
                    Role::Currency,
                    format!("{code:?} is not a currency. Exports use codes such as USD."),
                )
            })?
        };
        match so_far {
            Some(earlier) if earlier != currency => Err(self.problem(
                Role::Currency,
                format!(
                    "This row is in {currency}, but the rows before are in {earlier}. A bill \
                     has one currency, so export one billing account at a time."
                ),
            )),
            _ => Ok(currency),
        }
    }

    /// The line a usage row adds to: its day, account or project, and kind
    /// of usage. A row counted by its invoice month whose usage started in
    /// another month goes on the month's first day.
    fn line_key(&self, period: BillMonth) -> Result<(Date, Option<String>, String), CsvBillError> {
        let day = read_day(self.columns.value(self.record, Role::Date))
            .filter(|day| period.contains(day.midnight().assume_utc()));
        let day = match day {
            Some(day) => day,
            None => Date::from_calendar_date(period.year(), period.month(), 1)
                .map_err(|_| self.too_large())?,
        };
        let workspace = self.columns.value(self.record, Role::Workspace);
        let description = self.columns.value(self.record, Role::Description);
        Ok((
            day,
            (!workspace.is_empty()).then(|| workspace.to_owned()),
            if description.is_empty() {
                self.columns.cloud.service().to_owned()
            } else {
                description.to_owned()
            },
        ))
    }

    /// The credits a `BigQuery` export carries on a usage row: one number,
    /// or the JSON list of credits with their amounts and types.
    fn add_credits(&self, totals: &mut Totals, currency: Currency) -> Result<(), CsvBillError> {
        let text = self.columns.value(self.record, Role::Credits);
        if text.is_empty() {
            return Ok(());
        }
        let unreadable = || {
            self.problem(
                Role::Credits,
                format!("{text:?} is not a credit amount or a list of credits with amounts."),
            )
        };
        if text.starts_with('[') {
            let credits: Vec<ExportedCredit> =
                serde_json::from_str(text).map_err(|_| unreadable())?;
            for credit in credits {
                let amount =
                    money_from_json_number(&credit.amount, currency).map_err(|_| unreadable())?;
                match credit_charge(credit.kind.as_deref().unwrap_or_default()) {
                    Charge::Discount => totals.discounts.push(amount),
                    _ => totals.credits.push(amount),
                }
            }
        } else {
            let amount =
                read_amount(text, currency, self.decimal_comma).map_err(|_| unreadable())?;
            totals.credits.push(amount);
        }
        Ok(())
    }
}

/// One credit in a `BigQuery` export's `credits` list.
#[derive(Deserialize)]
struct ExportedCredit {
    amount: serde_json::Number,
    #[serde(default, rename = "type")]
    kind: Option<String>,
}

/// Whether a Google Cloud credit type is a discount, such as a committed use
/// discount or reseller margin, or a credit, such as a promotion or the free
/// tier.
fn credit_charge(credit_type: &str) -> Charge {
    let key: String = credit_type
        .trim()
        .chars()
        .map(|c| match c {
            ' ' | '-' => '_',
            other => other.to_ascii_uppercase(),
        })
        .collect();
    let discount = key.contains("DISCOUNT")
        || matches!(
            key.as_str(),
            "RESELLER_MARGIN" | "SUBSCRIPTION_BENEFIT" | "FEE_UTILIZATION_OFFSET"
        );
    if discount {
        Charge::Discount
    } else {
        Charge::Credit
    }
}

// Recorded exports follow each cloud's documented columns:
// AWS: https://docs.aws.amazon.com/cur/latest/userguide/table-dictionary-cur2.html
//   (Data Exports, CUR 2.0) and the legacy CUR's `lineItem/...` names;
// Google Cloud: https://cloud.google.com/billing/docs/how-to/cost-table (cost
//   table download) and https://cloud.google.com/billing/docs/how-to/export-data-bigquery-tables/standard-usage
//   (BigQuery export);
// Azure: https://learn.microsoft.com/azure/cost-management-billing/automate/understand-usage-details-fields
#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    const BEDROCK: &str = include_str!("../../../../tests/fixtures/bills/bedrock_cur2.csv");
    const BEDROCK_LEGACY: &str =
        include_str!("../../../../tests/fixtures/bills/bedrock_legacy_cur.csv");
    const VERTEX_COST_TABLE: &str =
        include_str!("../../../../tests/fixtures/bills/vertex_cost_table.csv");
    const VERTEX_BIGQUERY: &str =
        include_str!("../../../../tests/fixtures/bills/vertex_bigquery_export.csv");
    const FOUNDRY: &str = include_str!("../../../../tests/fixtures/bills/foundry_cost_details.csv");

    fn september() -> BillMonth {
        BillMonth::parse("2026-09").unwrap()
    }

    fn error(cloud: Cloud, text: &str) -> String {
        parse(cloud, text, september()).unwrap_err().to_string()
    }

    #[test]
    fn bedrock_counts_its_own_rows_in_the_period() {
        let bill = parse(Cloud::Bedrock, BEDROCK, september()).unwrap();
        assert_eq!(bill.provider(), BillProvider::AwsBedrock);
        assert_eq!(bill.cost_source(), Charging::CloudMarketplace);
        assert_eq!(bill.source_id(), "aws_cost_export:999988887777:2026-09");
        assert_eq!(bill.currency(), Currency::USD);
        assert_eq!(bill.gross().nanos(), 54_500_000_000);
        assert_eq!(bill.discount().nanos(), 4_675_000_000);
        assert_eq!(bill.credit().nanos(), 10_000_000_000);
        assert_eq!(bill.net().nanos(), 39_825_000_000);
        assert!(bill.is_final());

        let lines = bill.lines();
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0].date, date!(2026 - 09 - 01));
        assert_eq!(lines[0].workspace.as_deref(), Some("111122223333"));
        assert_eq!(lines[0].description, "USE1-Claude4.6Sonnet-input-tokens");
        // Two hours of the same usage on the same day are one line.
        assert_eq!(lines[0].amount.nanos(), 14_000_000_000);
        assert_eq!(lines[2].workspace.as_deref(), Some("444455556666"));
        // Claude billed through AWS Marketplace counts too.
        assert_eq!(lines[3].date, date!(2026 - 09 - 03));
        assert_eq!(lines[3].amount.nanos(), 7_750_000_000);
    }

    #[test]
    fn the_legacy_cost_and_usage_report_is_read_too() {
        let bill = parse(Cloud::Bedrock, BEDROCK_LEGACY, september()).unwrap();
        assert_eq!(bill.currency(), Currency::EUR);
        assert_eq!(bill.gross().nanos(), 2_800_000_000);
        assert_eq!(bill.net(), bill.gross());
        assert_eq!(bill.lines().len(), 1);
        assert_eq!(bill.source_id(), "aws_cost_export:999988887777:2026-09");
    }

    #[test]
    fn vertex_reads_the_cost_table_with_its_preamble() {
        let bill = parse(Cloud::Vertex, VERTEX_COST_TABLE, september()).unwrap();
        assert_eq!(bill.provider(), BillProvider::GcpVertex);
        assert_eq!(
            bill.source_id(),
            "gcp_billing_export:01A2B3-C4D5E6-F7A8B9:2026-09"
        );
        // The currency comes from the lines above the header.
        assert_eq!(bill.currency(), Currency::EUR);
        assert_eq!(bill.gross().nanos(), 76_000_000_000);
        assert_eq!(bill.discount().nanos(), 1_500_000_000);
        assert_eq!(bill.credit().nanos(), 5_000_000_000);
        assert_eq!(bill.net().nanos(), 69_500_000_000);
        let lines = bill.lines();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].workspace.as_deref(), Some("storefront-prod"));
        assert_eq!(lines[0].description, "Claude Sonnet 4.5 Output Tokens");
        assert_eq!(lines[2].date, date!(2026 - 09 - 02));
    }

    #[test]
    fn vertex_reads_a_bigquery_export_by_invoice_month() {
        let bill = parse(Cloud::Vertex, VERTEX_BIGQUERY, september()).unwrap();
        assert_eq!(bill.currency(), Currency::USD);
        assert_eq!(bill.gross().nanos(), 10_000_000_000);
        assert_eq!(bill.discount().nanos(), 850_000_000);
        assert_eq!(bill.credit().nanos(), 2_000_000_000);
        assert_eq!(bill.net().nanos(), 7_150_000_000);
        let lines = bill.lines();
        assert_eq!(lines.len(), 2);
        // Usage that started in October UTC but is on the September invoice.
        assert_eq!(lines[0].date, date!(2026 - 09 - 01));
        assert_eq!(lines[0].amount.nanos(), 1_500_000_000);
        assert_eq!(lines[1].date, date!(2026 - 09 - 30));
    }

    #[test]
    fn vertex_credits_may_be_one_number() {
        let text = "service.description,usage_start_time,cost,currency,credits\n\
                    Vertex AI,2026-09-05 10:00:00 UTC,4.00,USD,-0.25\n\
                    Vertex AI,2026-09-05 11:00:00 UTC,1.00,USD,\n";
        let bill = parse(Cloud::Vertex, text, september()).unwrap();
        assert_eq!(bill.gross().nanos(), 5_000_000_000);
        assert_eq!(bill.credit().nanos(), 250_000_000);
        assert_eq!(bill.source_id(), "gcp_billing_export:default:2026-09");
        assert_eq!(bill.lines().len(), 1);
        assert_eq!(bill.lines()[0].description, "Vertex AI");
    }

    #[test]
    fn foundry_counts_models_cognitive_services_and_claude() {
        let bill = parse(Cloud::Foundry, FOUNDRY, september()).unwrap();
        assert_eq!(bill.provider(), BillProvider::AzureFoundry);
        assert_eq!(
            bill.source_id(),
            "azure_cost_export:8b2d7c3e-1f4a-4e6b-9c0d-2a5f8e1b3c7d:\
             5e9a1c2b-3d4f-4a6b-8c7d-9e0f1a2b3c4d_2019-05-31:2026-09"
        );
        assert_eq!(bill.currency(), Currency::EUR);
        assert_eq!(bill.gross().nanos(), 59_800_000_000);
        assert!(bill.discount().is_zero());
        assert_eq!(bill.credit().nanos(), 5_000_000_000);
        assert_eq!(bill.net().nanos(), 54_800_000_000);
        let lines = bill.lines();
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0].description, "gpt-5 Inp glbl Tokens");
        assert!(
            lines[0]
                .workspace
                .as_deref()
                .is_some_and(|resource| resource.ends_with("/accounts/acme-openai"))
        );
        assert_eq!(lines[3].description, "Claude Consumption Units");
        assert_eq!(lines[3].date, date!(2026 - 09 - 03));
    }

    #[test]
    fn a_semicolon_export_with_a_bom_and_decimal_commas_is_read() {
        let text = "\u{feff}Date;MeterCategory;MeterName;CostInBillingCurrency;BillingCurrencyCode\n\
                    09/01/2026;Foundry Models;gpt-5 Inp glbl Tokens;1,50;EUR\n\
                    \"09/02/2026\";\"Azure OpenAI\";\"gpt-5; batch\";\"2,25\";EUR\n";
        let bill = parse(Cloud::Foundry, text, september()).unwrap();
        assert_eq!(bill.gross().nanos(), 3_750_000_000);
        assert_eq!(bill.lines()[1].description, "gpt-5; batch");
    }

    #[test]
    fn a_file_that_is_not_the_export_says_so() {
        assert_eq!(
            error(
                Cloud::Bedrock,
                "period,cost_source,gross,discount,credit,net,currency\n"
            ),
            "This is not an AWS Cost and Usage Report for Amazon Bedrock: no row names the \
             columns it needs (line_item_usage_start_date, line_item_unblended_cost, and \
             one of line_item_product_code, product_servicecode, product_product_name)."
        );
        assert_eq!(
            error(Cloud::Vertex, BEDROCK),
            "This is not a Google Cloud billing export for Vertex AI: no row names the \
             columns it needs (unrounded_cost, service_description, and one of \
             usage_start_date, invoice_month)."
        );
        assert!(error(Cloud::Foundry, "").starts_with("This is not an Azure cost export"));
    }

    #[test]
    fn no_rows_in_the_period_is_an_error() {
        let november = BillMonth::parse("2026-11").unwrap();
        assert_eq!(
            parse(Cloud::Bedrock, BEDROCK, november)
                .unwrap_err()
                .to_string(),
            "No row of this export is Amazon Bedrock cost in 2026-11. Check that the export \
             covers 2026-11 and includes Amazon Bedrock."
        );
    }

    #[test]
    fn rows_that_cannot_be_read_name_their_row_and_column() {
        let header = "line_item_usage_start_date,line_item_product_code,line_item_line_item_type,\
                      line_item_unblended_cost,line_item_currency_code\n";
        let row = |values: &str| error(Cloud::Bedrock, &format!("{header}{values}\n"));
        assert_eq!(
            row("2026-09-01T00:00:00Z,AmazonBedrock,Usage,1.5x,USD"),
            "Row 2, column line_item_unblended_cost: \"1.5x\" is not an amount. Write a plain \
             number such as 12.34."
        );
        assert_eq!(
            row("first of September,AmazonBedrock,Usage,1.5,USD"),
            "Row 2, column line_item_usage_start_date: \"first of September\" is not a date. \
             Exports write dates such as 2026-09-01."
        );
        assert_eq!(
            row("2026-09-01T00:00:00Z,AmazonBedrock,Bonus,1.5,USD"),
            "Row 2, column line_item_line_item_type: \"Bonus\" is a kind of charge Token \
             Controller does not know, so it cannot tell whether it is usage, a discount or \
             a credit."
        );
        assert_eq!(
            row("2026-09-01T00:00:00Z,AmazonBedrock,Usage,1.5,usd\n\
                 2026-09-02T00:00:00Z,AmazonBedrock,Usage,1.5,EUR"),
            "Row 3, column line_item_currency_code: This row is in EUR, but the rows before \
             are in USD. A bill has one currency, so export one billing account at a time."
        );
        assert_eq!(
            error(
                Cloud::Vertex,
                "service.description,usage_start_time,cost\nVertex AI,2026-09-05,1\n"
            ),
            "Row 2: The export does not say its currency. Include the currency column."
        );
        assert_eq!(
            error(
                Cloud::Vertex,
                "service.description,invoice.month,cost,currency\nVertex AI,Sept,1,USD\n"
            ),
            "Row 2, column invoice.month: \"Sept\" is not an invoice month such as 202609."
        );
        assert_eq!(
            error(
                Cloud::Vertex,
                "service.description,invoice.month,cost,currency,credits\n\
                 Vertex AI,202609,1,USD,\"[{\"\"amount\"\": \"\"x\"\"}]\"\n"
            ),
            "Row 2, column credits: \"[{\\\"amount\\\": \\\"x\\\"}]\" is not a credit amount \
             or a list of credits with amounts."
        );
    }

    #[test]
    fn credit_types_split_into_discounts_and_credits() {
        for discount in [
            "COMMITTED_USAGE_DISCOUNT",
            "COMMITTED_USAGE_DISCOUNT_DOLLAR_BASE",
            "SUSTAINED_USAGE_DISCOUNT",
            "DISCOUNT",
            "Reseller margin",
            "SUBSCRIPTION_BENEFIT",
            "FEE_UTILIZATION_OFFSET",
            "Committed use discount",
        ] {
            assert_eq!(credit_charge(discount), Charge::Discount, "{discount}");
        }
        for credit in ["PROMOTION", "FREE_TIER", "Promotion", ""] {
            assert_eq!(credit_charge(credit), Charge::Credit, "{credit}");
        }
    }
}
