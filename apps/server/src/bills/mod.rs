//! Bills and reconciliation (docs: Account > Bills, Reference > Bill schema).
//!
//! A bill is a provider's figure for one period's metered usage: gross,
//! discount, credit and net. Every change a provider makes is kept as a new
//! revision. Bill pulls and CSV uploads in the integrations area store bills
//! through [`record_bill`]; the bills endpoint does the same.
//!
//! Owner: the close and reports work. Keep this area's code, routes and
//! queries in this folder, and its migrations in `migrations/` named
//! `<timestamp>_bills_<what>.sql`.

pub mod amount;
pub mod ecb;
pub mod rates;
pub mod reconciliation;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use tokencontroller_core::{Currency, Money};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use self::amount::{AmountError, RateNumber, format_amount, parse_amount, rate_number};
use crate::AppState;
use crate::close::period::{Period, PeriodDates};
use crate::error::ApiError;
use crate::organizations::Role;
use crate::sign_in::{Caller, CredentialKind};

/// This area's routes, relative to `/api/v1`.
#[must_use]
pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(post_bill, list_bills))
        .routes(routes!(get_bill))
        .routes(routes!(reconciliation::get_reconciliation))
}

/// How a provider charges for what a bill covers. Seats and subscriptions
/// are flat fees and have no bill. When one provider has bills from several
/// cost sources, its token entries are charged on the first in this order.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, ToSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum CostSource {
    UsageAtApiRates,
    ApiKey,
    CloudMarketplace,
    Reseller,
}

impl CostSource {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UsageAtApiRates => "usage_at_api_rates",
            Self::ApiKey => "api_key",
            Self::CloudMarketplace => "cloud_marketplace",
            Self::Reseller => "reseller",
        }
    }

    /// The core's cost source.
    #[must_use]
    pub const fn core(self) -> tokencontroller_core::CostSource {
        match self {
            Self::UsageAtApiRates => tokencontroller_core::CostSource::UsageAtApiRates,
            Self::ApiKey => tokencontroller_core::CostSource::ApiKey,
            Self::CloudMarketplace => tokencontroller_core::CostSource::CloudMarketplace,
            Self::Reseller => tokencontroller_core::CostSource::Reseller,
        }
    }

    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "usage_at_api_rates" => Some(Self::UsageAtApiRates),
            "api_key" => Some(Self::ApiKey),
            "cloud_marketplace" => Some(Self::CloudMarketplace),
            "reseller" => Some(Self::Reseller),
            _ => None,
        }
    }
}

/// How a bill reached Token Controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum BillSource {
    /// From a provider's admin API.
    Pulled,
    /// From a CSV upload.
    Csv,
    /// Typed in by hand in the web app.
    Manual,
    /// Sent to the bills endpoint.
    Api,
}

impl BillSource {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pulled => "pulled",
            Self::Csv => "csv",
            Self::Manual => "manual",
            Self::Api => "api",
        }
    }

    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "pulled" => Some(Self::Pulled),
            "csv" => Some(Self::Csv),
            "manual" => Some(Self::Manual),
            "api" => Some(Self::Api),
            _ => None,
        }
    }
}

/// A bill to store, from any source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewBill {
    pub period: Period,
    /// Who charged, such as `anthropic`, `openai` or `aws_bedrock`: the same
    /// names token entries carry.
    pub provider: String,
    pub cost_source: CostSource,
    pub gross: Money,
    pub discount: Money,
    pub credit: Money,
    pub net: Money,
    /// To the organization's reporting currency, at the bill's date. Left
    /// out, it is filled from the reference rate of the period's last day.
    pub exchange_rate: Option<RateNumber>,
    pub source: BillSource,
    /// The provider's own detail, such as cost per workspace and day.
    pub lines: Vec<serde_json::Value>,
    /// The person who typed or sent it; `None` for a pull.
    pub recorded_by: Option<Uuid>,
}

/// Why a bill was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BillError {
    #[error("the amounts must all be in the bill's currency, {0}")]
    Currency(Currency),
    #[error(
        "gross minus discount minus credit must equal net: {gross} - {discount} - {credit} is not {net}"
    )]
    DoesNotAddUp {
        gross: Money,
        discount: Money,
        credit: Money,
        net: Money,
    },
    #[error("discount and credit are written as positive amounts that reduce the gross")]
    NegativeReduction,
    #[error("{0:?} is not a provider: use lower-case letters, digits and _, such as anthropic")]
    Provider(String),
    #[error(transparent)]
    Amount(#[from] AmountError),
    #[error("the organization does not exist")]
    NoOrganization,
}

impl From<BillError> for ApiError {
    fn from(error: BillError) -> Self {
        Self::BadRequest(error.to_string())
    }
}

/// A stored bill revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct BillView {
    pub id: Uuid,
    #[schema(value_type = String, example = "2026-09")]
    pub period: Period,
    pub provider: String,
    pub cost_source: CostSource,
    /// 1 for the first figure, higher after each restatement.
    pub revision: i32,
    #[schema(value_type = String, example = "USD")]
    pub currency: Currency,
    #[schema(example = "1234.56")]
    pub gross: String,
    #[schema(example = "0.00")]
    pub discount: String,
    #[schema(example = "0.00")]
    pub credit: String,
    #[schema(example = "1234.56")]
    pub net: String,
    /// To the reporting currency; `null` until a rate for the bill's date is
    /// known.
    #[schema(example = "0.9214")]
    pub exchange_rate: Option<String>,
    /// Whether the rate came with the bill, rather than from the reference
    /// rates.
    pub exchange_rate_given: bool,
    pub source: BillSource,
    #[schema(value_type = Vec<Object>)]
    pub lines: Vec<serde_json::Value>,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub recorded_at: OffsetDateTime,
}

/// The result of [`record_bill`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedBill {
    pub bill: BillView,
    /// False when the figures were the same as the latest revision's, so
    /// nothing was stored.
    pub changed: bool,
}

/// Stores a bill. A new revision is written only when the figures differ
/// from the latest one, so pulling the same bill twice changes nothing.
///
/// # Errors
///
/// [`BillError`] for figures that do not add up, or a database error.
pub async fn record_bill(
    db: &PgPool,
    organization_id: Uuid,
    bill: NewBill,
) -> Result<Result<RecordedBill, BillError>, sqlx::Error> {
    if let Err(error) = check(&bill) {
        return Ok(Err(error));
    }
    let Some(reporting) = reporting_currency(db, organization_id).await? else {
        return Ok(Err(BillError::NoOrganization));
    };
    let currency = bill.net.currency();
    let (exchange_rate, given) = if let Some(rate) = bill.exchange_rate {
        (Some(rate), true)
    } else {
        let rate = rates::between(db, currency, reporting, bill.period.last_day())
            .await?
            .ok()
            .filter(|_| currency != reporting)
            .map(rate_number);
        (rate, false)
    };

    let mut tx = db.begin().await?;
    let bill_id = sqlx::query_scalar!(
        r#"INSERT INTO bills (organization_id, period, provider, cost_source)
           VALUES ($1, $2, $3, $4)
           ON CONFLICT (organization_id, period, provider, cost_source)
               DO UPDATE SET provider = EXCLUDED.provider
           RETURNING id"#,
        organization_id,
        bill.period.first_day(),
        bill.provider,
        bill.cost_source.as_str(),
    )
    .fetch_one(&mut *tx)
    .await?;

    let latest = sqlx::query!(
        r#"SELECT revision, currency, gross_nanos, discount_nanos, credit_nanos, net_nanos,
                  exchange_rate_nanos, exchange_rate_given, lines
           FROM bill_revisions WHERE bill_id = $1 ORDER BY revision DESC LIMIT 1
           FOR UPDATE"#,
        bill_id,
    )
    .fetch_optional(&mut *tx)
    .await?;

    let lines = serde_json::Value::Array(bill.lines.clone());
    if let Some(latest) = &latest {
        let same = latest.currency == currency.code()
            && latest.gross_nanos == bill.gross.nanos()
            && latest.discount_nanos == bill.discount.nanos()
            && latest.credit_nanos == bill.credit.nanos()
            && latest.net_nanos == bill.net.nanos()
            && latest.lines == lines
            && (!given || latest.exchange_rate_nanos == exchange_rate.map(RateNumber::nanos));
        if same {
            tx.commit().await?;
            let view = latest_view(db, bill_id)
                .await?
                .ok_or(sqlx::Error::RowNotFound)?;
            return Ok(Ok(RecordedBill {
                bill: view,
                changed: false,
            }));
        }
    }
    let revision = latest
        .as_ref()
        .map_or(Some(1), |latest| latest.revision.checked_add(1))
        .ok_or(sqlx::Error::Protocol("too many bill revisions".to_owned()))?;
    sqlx::query!(
        r#"INSERT INTO bill_revisions
               (bill_id, revision, currency, gross_nanos, discount_nanos, credit_nanos, net_nanos,
                exchange_rate_nanos, exchange_rate_given, source, lines, recorded_by)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)"#,
        bill_id,
        revision,
        currency.code(),
        bill.gross.nanos(),
        bill.discount.nanos(),
        bill.credit.nanos(),
        bill.net.nanos(),
        exchange_rate.map(RateNumber::nanos),
        given,
        bill.source.as_str(),
        lines,
        bill.recorded_by,
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    let view = latest_view(db, bill_id)
        .await?
        .ok_or(sqlx::Error::RowNotFound)?;
    Ok(Ok(RecordedBill {
        bill: view,
        changed: true,
    }))
}

fn check(bill: &NewBill) -> Result<(), BillError> {
    let currency = bill.net.currency();
    if [bill.gross, bill.discount, bill.credit]
        .iter()
        .any(|amount| amount.currency() != currency)
    {
        return Err(BillError::Currency(currency));
    }
    if bill.discount.nanos().is_negative() || bill.credit.nanos().is_negative() {
        return Err(BillError::NegativeReduction);
    }
    let valid_provider = !bill.provider.is_empty()
        && bill.provider.len() <= 40
        && bill
            .provider
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if !valid_provider {
        return Err(BillError::Provider(bill.provider.clone()));
    }
    let core = tokencontroller_core::Bill {
        id: format!(
            "{}-{}-{}",
            bill.provider,
            bill.cost_source.as_str(),
            bill.period
        ),
        provider: bill.provider.clone(),
        cost_source: bill.cost_source.core(),
        period: bill.period,
        gross: bill.gross,
        discount: bill.discount,
        credit: bill.credit,
        net: bill.net,
        exchange_rate: None,
        revision: 1,
    };
    core.check().map_err(|_| BillError::DoesNotAddUp {
        gross: bill.gross,
        discount: bill.discount,
        credit: bill.credit,
        net: bill.net,
    })
}

/// The organization's reporting currency.
///
/// # Errors
///
/// A database error.
pub async fn reporting_currency(
    db: &PgPool,
    organization_id: Uuid,
) -> Result<Option<Currency>, sqlx::Error> {
    let code = sqlx::query_scalar!(
        "SELECT reporting_currency FROM organizations WHERE id = $1",
        organization_id
    )
    .fetch_optional(db)
    .await?;
    Ok(code.and_then(|code| Currency::new(&code).ok()))
}

/// The latest revision of every bill of a period, or of every period.
///
/// # Errors
///
/// A database error.
pub async fn latest_bills(
    db: &PgPool,
    organization_id: Uuid,
    period: Option<Period>,
) -> Result<Vec<BillView>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"SELECT DISTINCT ON (b.id)
                  b.id, b.period, b.provider, b.cost_source, r.revision, r.currency,
                  r.gross_nanos, r.discount_nanos, r.credit_nanos, r.net_nanos,
                  r.exchange_rate_nanos, r.exchange_rate_given, r.source, r.lines, r.recorded_at
           FROM bills b JOIN bill_revisions r ON r.bill_id = b.id
           WHERE b.organization_id = $1 AND ($2::date IS NULL OR b.period = $2)
           ORDER BY b.id, r.revision DESC"#,
        organization_id,
        period.map(Period::first_day),
    )
    .fetch_all(db)
    .await?;
    let mut bills: Vec<BillView> = rows
        .into_iter()
        .filter_map(|row| {
            view(RevisionRow {
                id: row.id,
                period: row.period,
                provider: row.provider,
                cost_source: row.cost_source,
                revision: row.revision,
                currency: row.currency,
                gross_nanos: row.gross_nanos,
                discount_nanos: row.discount_nanos,
                credit_nanos: row.credit_nanos,
                net_nanos: row.net_nanos,
                exchange_rate_nanos: row.exchange_rate_nanos,
                exchange_rate_given: row.exchange_rate_given,
                source: row.source,
                lines: row.lines,
                recorded_at: row.recorded_at,
            })
        })
        .collect();
    bills.sort_by(|a, b| {
        b.period
            .cmp(&a.period)
            .then_with(|| a.provider.cmp(&b.provider))
            .then_with(|| a.cost_source.as_str().cmp(b.cost_source.as_str()))
    });
    Ok(bills)
}

/// Every revision of one bill, oldest first.
///
/// # Errors
///
/// A database error.
pub async fn revisions(
    db: &PgPool,
    organization_id: Uuid,
    bill_id: Uuid,
) -> Result<Vec<BillView>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"SELECT b.id, b.period, b.provider, b.cost_source, r.revision, r.currency,
                  r.gross_nanos, r.discount_nanos, r.credit_nanos, r.net_nanos,
                  r.exchange_rate_nanos, r.exchange_rate_given, r.source, r.lines, r.recorded_at
           FROM bills b JOIN bill_revisions r ON r.bill_id = b.id
           WHERE b.organization_id = $1 AND b.id = $2
           ORDER BY r.revision"#,
        organization_id,
        bill_id,
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            view(RevisionRow {
                id: row.id,
                period: row.period,
                provider: row.provider,
                cost_source: row.cost_source,
                revision: row.revision,
                currency: row.currency,
                gross_nanos: row.gross_nanos,
                discount_nanos: row.discount_nanos,
                credit_nanos: row.credit_nanos,
                net_nanos: row.net_nanos,
                exchange_rate_nanos: row.exchange_rate_nanos,
                exchange_rate_given: row.exchange_rate_given,
                source: row.source,
                lines: row.lines,
                recorded_at: row.recorded_at,
            })
        })
        .collect())
}

async fn latest_view(db: &PgPool, bill_id: Uuid) -> Result<Option<BillView>, sqlx::Error> {
    let organization_id =
        sqlx::query_scalar!("SELECT organization_id FROM bills WHERE id = $1", bill_id)
            .fetch_one(db)
            .await?;
    Ok(revisions(db, organization_id, bill_id).await?.pop())
}

struct RevisionRow {
    id: Uuid,
    period: time::Date,
    provider: String,
    cost_source: String,
    revision: i32,
    currency: String,
    gross_nanos: i64,
    discount_nanos: i64,
    credit_nanos: i64,
    net_nanos: i64,
    exchange_rate_nanos: Option<i64>,
    exchange_rate_given: bool,
    source: String,
    lines: serde_json::Value,
    recorded_at: OffsetDateTime,
}

fn view(row: RevisionRow) -> Option<BillView> {
    let currency = Currency::new(&row.currency).ok()?;
    let money = |nanos| format_amount(Money::from_nanos(nanos, currency));
    Some(BillView {
        id: row.id,
        period: Period::of_date(row.period),
        provider: row.provider,
        cost_source: CostSource::parse(&row.cost_source)?,
        revision: row.revision,
        currency,
        gross: money(row.gross_nanos),
        discount: money(row.discount_nanos),
        credit: money(row.credit_nanos),
        net: money(row.net_nanos),
        exchange_rate: row
            .exchange_rate_nanos
            .and_then(|nanos| RateNumber::from_nanos(nanos).ok())
            .map(|rate| rate.to_string()),
        exchange_rate_given: row.exchange_rate_given,
        source: BillSource::parse(&row.source)?,
        lines: row.lines.as_array().cloned().unwrap_or_default(),
        recorded_at: row.recorded_at,
    })
}

/// A bill as the bills endpoint and the web app send it (docs: Reference >
/// Bill schema). Amounts are decimal strings in `currency`.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct BillInput {
    /// The calendar month, such as `2026-09`.
    #[schema(value_type = String, example = "2026-09")]
    pub period: Period,
    /// Who charged, such as `anthropic` or `openai`.
    #[schema(example = "anthropic")]
    pub provider: String,
    pub cost_source: CostSource,
    /// Before discount and credit. Left out, it is net plus discount plus
    /// credit.
    #[schema(example = "1234.56")]
    pub gross: Option<String>,
    #[schema(example = "0")]
    pub discount: Option<String>,
    #[schema(example = "0")]
    pub credit: Option<String>,
    #[schema(example = "1234.56")]
    pub net: String,
    #[schema(example = "USD")]
    pub currency: String,
    /// To the organization's reporting currency, at the bill's date. Left
    /// out, Token Controller fills it in.
    #[schema(example = "0.9214")]
    pub exchange_rate: Option<String>,
    /// Typed in by hand in the web app, rather than sent by a program.
    #[serde(default)]
    pub typed_in: bool,
    #[serde(default)]
    #[schema(value_type = Vec<Object>)]
    pub lines: Vec<serde_json::Value>,
}

impl BillInput {
    /// The bill as [`record_bill`] takes it.
    ///
    /// # Errors
    ///
    /// [`BillError`] for an amount, currency or rate that cannot be read.
    pub fn into_new_bill(self, recorded_by: Uuid) -> Result<NewBill, BillError> {
        let currency = Currency::new(&self.currency)
            .map_err(|_| BillError::Amount(AmountError::Invalid(self.currency.clone())))?;
        let read = |text: Option<&str>| -> Result<Money, BillError> {
            text.map_or(Ok(Money::zero(currency)), |text| {
                Ok(parse_amount(text, currency)?)
            })
        };
        let net = parse_amount(&self.net, currency)?;
        let discount = read(self.discount.as_deref())?;
        let credit = read(self.credit.as_deref())?;
        let gross = match self.gross.as_deref() {
            Some(text) => parse_amount(text, currency)?,
            None => net
                .checked_add(discount)
                .and_then(|sum| sum.checked_add(credit))
                .map_err(|_| BillError::Amount(AmountError::Overflow))?,
        };
        let exchange_rate = self
            .exchange_rate
            .as_deref()
            .map(RateNumber::parse)
            .transpose()?;
        Ok(NewBill {
            period: self.period,
            provider: self.provider,
            cost_source: self.cost_source,
            gross,
            discount,
            credit,
            net,
            exchange_rate,
            source: if self.typed_in {
                BillSource::Manual
            } else {
                BillSource::Api
            },
            lines: self.lines,
            recorded_by: Some(recorded_by),
        })
    }
}

/// Send a bill for a period.
///
/// Stores a new revision when the figures differ from the latest one, and
/// answers 200 with the unchanged bill when they do not.
#[utoipa::path(
    post,
    path = "/bills",
    tag = "bills",
    request_body = BillInput,
    responses(
        (status = CREATED, description = "Stored as a new revision", body = BillView),
        (status = OK, description = "The same figures were already stored", body = BillView),
        (status = BAD_REQUEST, description = "The figures cannot be read or do not add up", body = crate::error::ErrorBody),
        (status = FORBIDDEN, description = "Only a controller can send bills", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn post_bill(
    State(state): State<AppState>,
    caller: Caller,
    Json(input): Json<BillInput>,
) -> Result<(StatusCode, Json<BillView>), ApiError> {
    caller.require_role(Role::Controller)?;
    caller.require_credential(&[CredentialKind::Session, CredentialKind::Service])?;
    let bill = input.into_new_bill(caller.person_id)?;
    let recorded = record_bill(&state.db, caller.organization_id, bill).await??;
    let status = if recorded.changed {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(recorded.bill)))
}

/// Which bills to list.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct BillsQuery {
    /// Only this period, such as `2026-09`.
    #[param(value_type = Option<String>, example = "2026-09")]
    pub period: Option<Period>,
    /// `csv` for the table as a CSV file instead of JSON.
    #[param(example = "csv")]
    pub format: Option<String>,
}

/// The columns of the bills table and its CSV file.
const BILL_COLUMNS: &[crate::reports::csv::Column] = &[
    ("period", "/period"),
    ("provider", "/provider"),
    ("cost_source", "/cost_source"),
    ("revision", "/revision"),
    ("currency", "/currency"),
    ("gross", "/gross"),
    ("discount", "/discount"),
    ("credit", "/credit"),
    ("net", "/net"),
    ("exchange_rate", "/exchange_rate"),
    ("source", "/source"),
    ("recorded_at", "/recorded_at"),
];

/// The latest revision of every bill, newest period first.
#[utoipa::path(
    get,
    path = "/bills",
    tag = "bills",
    params(BillsQuery),
    responses(
        (status = OK, description = "The bills, as JSON or CSV", content((Vec<BillView> = "application/json"), (String = "text/csv"))),
        (status = FORBIDDEN, description = "Only a controller can read bills", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn list_bills(
    State(state): State<AppState>,
    caller: Caller,
    Query(query): Query<BillsQuery>,
) -> Result<Response, ApiError> {
    caller.require_role(Role::Controller)?;
    let bills = latest_bills(&state.db, caller.organization_id, query.period).await?;
    let filename = query.period.map_or_else(
        || "bills.csv".to_owned(),
        |period| format!("bills-{period}.csv"),
    );
    crate::reports::csv::table(
        &bills,
        BILL_COLUMNS,
        crate::reports::csv::wants_csv(query.format.as_deref()),
        &filename,
    )
}

/// Every revision of one bill, oldest first.
#[utoipa::path(
    get,
    path = "/bills/{id}",
    tag = "bills",
    params(("id" = Uuid, Path, description = "The bill"), crate::reports::csv::FormatQuery),
    responses(
        (status = OK, description = "The bill's revisions, as JSON or CSV", content((Vec<BillView> = "application/json"), (String = "text/csv"))),
        (status = NOT_FOUND, description = "No such bill", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn get_bill(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<Uuid>,
    Query(query): Query<crate::reports::csv::FormatQuery>,
) -> Result<Response, ApiError> {
    caller.require_role(Role::Controller)?;
    let revisions = revisions(&state.db, caller.organization_id, id).await?;
    let Some(first) = revisions.first() else {
        return Err(ApiError::NotFound);
    };
    let filename = format!(
        "bill-{}-{}-{}.csv",
        first.period,
        first.provider,
        first.cost_source.as_str()
    );
    crate::reports::csv::table(&revisions, BILL_COLUMNS, query.wants_csv(), &filename)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usd(text: &str) -> Money {
        parse_amount(text, Currency::USD).unwrap()
    }

    fn bill(gross: &str, discount: &str, credit: &str, net: &str) -> NewBill {
        NewBill {
            period: Period::parse("2026-09").unwrap(),
            provider: "anthropic".to_owned(),
            cost_source: CostSource::UsageAtApiRates,
            gross: usd(gross),
            discount: usd(discount),
            credit: usd(credit),
            net: usd(net),
            exchange_rate: None,
            source: BillSource::Api,
            lines: Vec::new(),
            recorded_by: None,
        }
    }

    #[test]
    fn a_bill_must_add_up() {
        assert_eq!(check(&bill("100", "10", "5", "85")), Ok(()));
        assert!(matches!(
            check(&bill("100", "10", "5", "86")),
            Err(BillError::DoesNotAddUp { .. })
        ));
        assert_eq!(
            check(&bill("100", "-10", "0", "110")),
            Err(BillError::NegativeReduction)
        );
    }

    #[test]
    fn a_bill_has_one_currency_and_a_plain_provider() {
        let mut mixed = bill("100", "0", "0", "100");
        mixed.gross = parse_amount("100", Currency::EUR).unwrap();
        assert_eq!(check(&mixed), Err(BillError::Currency(Currency::USD)));
        let mut odd = bill("100", "0", "0", "100");
        odd.provider = "Anthropic Inc".to_owned();
        assert!(matches!(check(&odd), Err(BillError::Provider(_))));
    }

    #[test]
    fn gross_defaults_to_net_plus_reductions() {
        let input = BillInput {
            period: Period::parse("2026-09").unwrap(),
            provider: "openai".to_owned(),
            cost_source: CostSource::ApiKey,
            gross: None,
            discount: Some("10".to_owned()),
            credit: None,
            net: "90".to_owned(),
            currency: "USD".to_owned(),
            exchange_rate: Some("0.9".to_owned()),
            typed_in: true,
            lines: Vec::new(),
        };
        let new = input.into_new_bill(Uuid::nil()).unwrap();
        assert_eq!(new.gross, usd("100"));
        assert_eq!(new.credit, usd("0"));
        assert_eq!(new.source, BillSource::Manual);
        assert_eq!(new.exchange_rate, Some(RateNumber::parse("0.9").unwrap()));
        assert_eq!(check(&new), Ok(()));
    }

    #[test]
    fn names_round_trip() {
        for source in [
            CostSource::UsageAtApiRates,
            CostSource::ApiKey,
            CostSource::CloudMarketplace,
            CostSource::Reseller,
        ] {
            assert_eq!(CostSource::parse(source.as_str()), Some(source));
        }
        assert_eq!(CostSource::parse("seat"), None);
        for source in [
            BillSource::Pulled,
            BillSource::Csv,
            BillSource::Manual,
            BillSource::Api,
        ] {
            assert_eq!(BillSource::parse(source.as_str()), Some(source));
        }
        assert_eq!(BillSource::parse("email"), None);
    }
}
