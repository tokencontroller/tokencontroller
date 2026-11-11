//! Bills and reconciliation (docs: Account > Bills, Reference > Bill
//! schema).
//!
//! A bill is a provider's figure for metered usage over a date range:
//! provider, first and last day, amount, currency. It is an event (ADR
//! 0022). A restated bill is a new bill event for the same provider and
//! range; the earlier one is reversed and stays visible. Bill pulls and CSV
//! uploads in the integrations area store bills through [`record_bill`];
//! the bills endpoint does the same.
//!
//! Owner: the bills work. Keep this area's code, routes and queries in this
//! folder.

pub mod amount;
pub mod ecb;
pub mod rates;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde::{Deserialize, Serialize};
use time::{Date, OffsetDateTime};
use tokencontroller_core::{
    Bill, ContractKind, Currency, DateRange, Event, Money, Record, TagValue,
};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use self::amount::{AmountError, RateNumber, format_amount, parse_amount, rate_number};
use crate::AppState;
use crate::error::ApiError;
use crate::events::{self, Log, access, prefix};
use crate::sign_in::{Caller, CredentialKind};

/// This area's routes, relative to `/api/v1`.
#[must_use]
pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(post_bill, list_bills))
        .routes(routes!(get_bill))
        .routes(routes!(get_reconciliation))
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
    /// Who charged, such as `anthropic`, `openai` or `cursor`: the same
    /// names messages carry.
    pub provider: String,
    /// The contract the bill is for, such as `anthropic-api`. Left out, it
    /// goes to the provider's only contract, else to one named after the
    /// provider. A new key creates the contract.
    pub contract: Option<TagValue>,
    /// The kind of contract a new key creates; metered when left out.
    pub contract_kind: Option<ContractKind>,
    /// The fee part of a flat contract's bill, when the bill says it.
    pub fee: Option<Money>,
    /// The extra usage part of a flat contract's bill, when the bill says
    /// it.
    pub extra_usage: Option<Money>,
    /// The first and last day the bill covers.
    pub range: DateRange,
    /// What the provider charged for metered usage in the range, in the
    /// bill's own currency.
    pub amount: Money,
    /// To the organization's reporting currency, at the bill's last day.
    /// Left out, it is filled from the reference rate of that day.
    pub exchange_rate: Option<RateNumber>,
    pub source: BillSource,
    /// The provider's own detail, such as cost per workspace and day.
    pub lines: Vec<serde_json::Value>,
    /// The person who typed or sent it; `None` for a pull.
    pub recorded_by: Option<TagValue>,
    /// Whether the provider will no longer change these figures. A pull can
    /// bring a bill before it is final; every other source is final.
    pub is_final: bool,
}

/// Why a bill was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BillError {
    #[error("{0:?} is not a provider: use lower-case letters, digits and _, such as anthropic")]
    Provider(String),
    #[error("{0:?} is not a currency code: use three capital letters, such as EUR")]
    NotACurrency(String),
    #[error("the bill's amount is below zero")]
    Negative,
    #[error(transparent)]
    Amount(#[from] AmountError),
    #[error("{0}")]
    Range(String),
    #[error(
        "{0:?} is not a contract key: use small letters, digits, - and _, such as anthropic-api"
    )]
    Contract(String),
    #[error("{0:?} is not a contract kind: use metered or flat")]
    ContractKind(String),
    #[error("the fee and the extra usage are in the bill's currency and not above its amount")]
    Parts,
}

impl From<BillError> for ApiError {
    fn from(error: BillError) -> Self {
        Self::BadRequest(error.to_string())
    }
}

/// One bill next to the cost of the messages with its contract in its
/// range, in the core's currency (USD), with what a check could otherwise
/// hide beside it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ReconciliationView {
    #[schema(example = "USD")]
    pub currency: String,
    /// The contract the bill is checked under.
    #[schema(example = "anthropic-api")]
    pub contract: String,
    /// `metered`: the bill should equal its messages. `flat`: the bill is a
    /// fee plus extra usage.
    #[schema(example = "metered")]
    pub contract_kind: String,
    /// The bill, converted at its exchange rate when it is in another
    /// currency.
    #[schema(example = "1000.00")]
    pub bill: String,
    /// The cost of the messages with the bill's contract in the range.
    #[schema(example = "950.00")]
    pub messages: String,
    /// Metered: what the bill has that no message explains; negative means
    /// a message is wrong. Flat: what the bill has besides its fee and
    /// extra usage, zero when the parts add up.
    #[schema(example = "50.00")]
    pub unexplained: String,
    /// Flat: the fee, from the bill, or from the contract for a whole
    /// month.
    pub fee: Option<String>,
    /// Flat: the extra usage, from the bill or as the bill less the fee.
    pub extra_usage: Option<String>,
    /// Flat: the cost of its messages less the extra usage, what the plan
    /// was worth. `null` while the fee is not known.
    pub covered: Option<String>,
    /// Messages of the bill's provider in its range that have no contract.
    #[schema(example = "12.40")]
    pub same_provider_no_contract: String,
    /// The part of `messages` whose contract came only from the provider
    /// having one contract.
    #[schema(example = "0.00")]
    pub contract_from_fallback: String,
    pub a_message_is_wrong: bool,
    /// Messages in the range with no counts; their cost is not in
    /// `messages`.
    pub count_errors: usize,
    /// Models in the range with no price row, as `provider/model`.
    pub unknown_models: Vec<String>,
}

/// A bill as the API shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct BillView {
    /// The bill event's id.
    pub id: String,
    pub provider: String,
    /// The contract the bill is for: the one it names, else the one it
    /// goes to.
    #[schema(example = "anthropic-api")]
    pub contract: String,
    /// The kind of contract the bill named, when it named one.
    pub contract_kind: Option<String>,
    /// The fee part of a flat contract's bill, in `currency`.
    pub fee: Option<String>,
    /// The extra usage part of a flat contract's bill, in `currency`.
    pub extra_usage: Option<String>,
    #[serde(with = "crate::dates::day")]
    #[schema(value_type = String, format = Date)]
    pub from: Date,
    #[serde(with = "crate::dates::day")]
    #[schema(value_type = String, format = Date)]
    pub to: Date,
    #[schema(example = "1234.56")]
    pub amount: String,
    #[schema(example = "USD")]
    pub currency: String,
    /// To the reporting currency; `null` until a rate for the bill's last
    /// day is known.
    #[schema(example = "0.9214")]
    pub exchange_rate: Option<String>,
    /// Whether the rate came with the bill, rather than from the reference
    /// rates.
    pub exchange_rate_given: bool,
    pub source: BillSource,
    /// Whether the provider will no longer change these figures.
    #[serde(rename = "final")]
    pub is_final: bool,
    #[schema(value_type = Vec<Object>)]
    pub lines: Vec<serde_json::Value>,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub recorded_at: OffsetDateTime,
    pub recorded_by: Option<String>,
    /// The reversal that replaced this bill, when a later bill restated
    /// it.
    pub reversed_by: Option<String>,
    /// The bill against its messages; `null` for a reversed bill, or when
    /// no rate to USD is known.
    pub reconciliation: Option<ReconciliationView>,
}

/// The result of [`record_bill`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedBill {
    pub bill: BillView,
    /// False when the figures were the same as the bill in force, so
    /// nothing was stored.
    pub changed: bool,
}

/// What `bill_details` holds for one bill event.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Details {
    exchange_rate: Option<RateNumber>,
    exchange_rate_given: bool,
    source: BillSource,
    is_final: bool,
    lines: Vec<serde_json::Value>,
}

/// Checks a bill as [`record_bill`] does, without the database, so a file
/// of bills can be checked whole before any is stored.
///
/// # Errors
///
/// [`BillError`] for a provider that is not a plain name or an amount
/// below zero.
pub fn check(bill: &NewBill) -> Result<(), BillError> {
    if !is_provider(&bill.provider) {
        return Err(BillError::Provider(bill.provider.clone()));
    }
    if bill.amount.nanos().is_negative() {
        return Err(BillError::Negative);
    }
    for part in [bill.fee, bill.extra_usage].into_iter().flatten() {
        if part.currency() != bill.amount.currency()
            || part.nanos().is_negative()
            || part.nanos() > bill.amount.nanos()
        {
            return Err(BillError::Parts);
        }
    }
    Ok(())
}

/// The bill as the log holds it.
fn core_bill(bill: &NewBill) -> Bill {
    Bill {
        provider: bill.provider.clone(),
        contract: bill.contract.clone(),
        contract_kind: bill.contract_kind,
        range: bill.range,
        amount: bill.amount,
        fee: bill.fee,
        extra_usage: bill.extra_usage,
    }
}

/// A contract key as a bill names it.
///
/// # Errors
///
/// [`BillError::Contract`] for a key that is not a tag value.
pub fn contract_key(text: &str) -> Result<TagValue, BillError> {
    let text = text.trim();
    TagValue::new(text).map_err(|_| BillError::Contract(text.to_owned()))
}

/// A contract kind as a bill names it: `metered` or `flat`.
///
/// # Errors
///
/// [`BillError::ContractKind`] for any other word.
pub fn contract_kind(text: &str) -> Result<ContractKind, BillError> {
    match text.trim() {
        "metered" => Ok(ContractKind::Metered),
        "flat" => Ok(ContractKind::Flat),
        other => Err(BillError::ContractKind(other.to_owned())),
    }
}

/// Whether a text is a provider name: up to 40 lower-case letters, digits
/// and `_`.
#[must_use]
pub fn is_provider(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 40
        && text
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Stores a bill. A new bill event is written only when the figures differ
/// from the bill in force for the same provider and range, which is then
/// reversed; pulling the same bill twice changes nothing.
///
/// # Errors
///
/// [`BillError`] for a bill that does not pass [`check`], or whatever
/// writing the log fails with.
pub async fn record_bill(
    state: &AppState,
    organization_id: Uuid,
    bill: NewBill,
) -> Result<Result<RecordedBill, BillError>, ApiError> {
    if let Err(error) = check(&bill) {
        return Ok(Err(error));
    }
    let log = Log::load(state, organization_id).await?;
    let now = OffsetDateTime::now_utc();
    let fold = log.fold(now)?;
    let standing = standing_of(state, organization_id, &fold, &bill).await?;
    let (exchange_rate, given) = (standing.exchange_rate, standing.given);
    let new = core_bill(&bill);
    let in_force = standing.in_force;
    if let Some(current) = &in_force {
        let details = standing.details;
        let same = standing.same;
        if same {
            let now_final =
                bill.is_final && details.as_ref().is_some_and(|details| !details.is_final);
            if now_final {
                sqlx::query!(
                    "UPDATE bill_details SET is_final = true WHERE organization_id = $1 AND event_id = $2",
                    organization_id,
                    current.id.as_str(),
                )
                .execute(&state.db)
                .await?;
            }
            let view = view_of(state, organization_id, current.id.as_str(), now)
                .await?
                .ok_or(ApiError::NotFound)?;
            return Ok(Ok(RecordedBill {
                bill: view,
                changed: now_final,
            }));
        }
    }

    let mut event = Event::new(&events::new_id(prefix::BILL), now, Record::Bill(new))
        .map_err(|error| ApiError::Internal(error.into()))?;
    if let Some(person) = &bill.recorded_by {
        event = event.by(person.clone());
    }
    let mut new = vec![event.clone()];
    if let Some(current) = &in_force {
        let mut reversal = Event::new(
            &events::new_id(prefix::REVERSAL),
            now.saturating_add(time::Duration::microseconds(1)),
            Record::Reversal(current.id.clone()),
        )
        .map_err(|error| ApiError::Internal(error.into()))?;
        if let Some(person) = &bill.recorded_by {
            reversal = reversal.by(person.clone());
        }
        new.push(reversal);
    }
    events::write(state, &log, &new).await?;
    sqlx::query!(
        r#"INSERT INTO bill_details
               (organization_id, event_id, exchange_rate_nanos, exchange_rate_given, source, is_final, lines)
           VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
        organization_id,
        event.id.as_str(),
        exchange_rate.map(RateNumber::nanos),
        given,
        bill.source.as_str(),
        bill.is_final,
        serde_json::Value::Array(bill.lines),
    )
    .execute(&state.db)
    .await?;
    let view = view_of(
        state,
        organization_id,
        event.id.as_str(),
        now.saturating_add(time::Duration::seconds(1)),
    )
    .await?
    .ok_or(ApiError::NotFound)?;
    Ok(Ok(RecordedBill {
        bill: view,
        changed: true,
    }))
}

/// How a new bill stands against the bill in force for the contract it
/// goes to.
pub struct Standing {
    /// The contract the bill goes to.
    pub contract: TagValue,
    /// The bill in force for the same provider, contract and range.
    pub in_force: Option<tokencontroller_core::Recorded<Bill>>,
    /// Whether the new bill has the same figures, so storing it changes
    /// nothing.
    pub same: bool,
    details: Option<Details>,
    exchange_rate: Option<RateNumber>,
    given: bool,
}

/// The bill in force for the contract `bill` goes to, and whether `bill`
/// has its figures. [`record_bill`] and the CSV preview both read it, so
/// the preview says what the upload will do.
///
/// # Errors
///
/// Whatever reading the database fails with.
pub async fn standing_of(
    state: &AppState,
    organization_id: Uuid,
    fold: &tokencontroller_core::State,
    bill: &NewBill,
) -> Result<Standing, ApiError> {
    let reporting = crate::organizations::reporting_currency(&state.db, organization_id).await?;
    let (exchange_rate, given) = rate_for(state, bill, reporting).await?;
    // The fold names each bill's contract, so the bill in force is the one
    // for the contract this bill goes to.
    let new = core_bill(bill);
    let contract = fold.contract_of_bill(&new);
    let in_force = fold
        .bills()
        .find(|recorded| {
            recorded.value.provider == bill.provider
                && recorded.value.range == bill.range
                && fold.contract_of_bill(&recorded.value) == contract
        })
        .cloned();
    let details = match &in_force {
        Some(current) => details_of(state, organization_id, current.id.as_str()).await?,
        None => None,
    };
    let same = in_force.as_ref().is_some_and(|current| {
        current.value.amount == new.amount
            && current.value.fee == new.fee
            && current.value.extra_usage == new.extra_usage
            && (new.contract_kind.is_none() || current.value.contract_kind == new.contract_kind)
            && details
                .as_ref()
                .is_some_and(|details| details.lines == bill.lines)
            && (!given
                || details
                    .as_ref()
                    .is_some_and(|details| details.exchange_rate == exchange_rate))
    });
    Ok(Standing {
        contract,
        in_force,
        same,
        details,
        exchange_rate,
        given,
    })
}

/// The bill's rate to the reporting currency, and whether it came with the
/// bill. Without one it is the reference rate of the range's last day.
async fn rate_for(
    state: &AppState,
    bill: &NewBill,
    reporting: Currency,
) -> Result<(Option<RateNumber>, bool), sqlx::Error> {
    if let Some(rate) = bill.exchange_rate {
        return Ok((Some(rate), true));
    }
    let currency = bill.amount.currency();
    let rate = rates::between(&state.db, currency, reporting, bill.range.to)
        .await?
        .ok()
        .filter(|_| currency != reporting)
        .map(rate_number);
    Ok((rate, false))
}

async fn details_of(
    state: &AppState,
    organization_id: Uuid,
    event_id: &str,
) -> Result<Option<Details>, sqlx::Error> {
    let row = sqlx::query!(
        "SELECT exchange_rate_nanos, exchange_rate_given, source, is_final, lines
         FROM bill_details WHERE organization_id = $1 AND event_id = $2",
        organization_id,
        event_id,
    )
    .fetch_optional(&state.db)
    .await?;
    Ok(row.map(|row| Details {
        exchange_rate: row
            .exchange_rate_nanos
            .and_then(|nanos| RateNumber::from_nanos(nanos).ok()),
        exchange_rate_given: row.exchange_rate_given,
        source: BillSource::parse(&row.source).unwrap_or(BillSource::Api),
        is_final: row.is_final,
        lines: row.lines.as_array().cloned().unwrap_or_default(),
    }))
}

async fn all_details(
    state: &AppState,
    organization_id: Uuid,
) -> Result<std::collections::BTreeMap<String, Details>, sqlx::Error> {
    let rows = sqlx::query!(
        "SELECT event_id, exchange_rate_nanos, exchange_rate_given, source, is_final, lines
         FROM bill_details WHERE organization_id = $1",
        organization_id,
    )
    .fetch_all(&state.db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            (
                row.event_id,
                Details {
                    exchange_rate: row
                        .exchange_rate_nanos
                        .and_then(|nanos| RateNumber::from_nanos(nanos).ok()),
                    exchange_rate_given: row.exchange_rate_given,
                    source: BillSource::parse(&row.source).unwrap_or(BillSource::Api),
                    is_final: row.is_final,
                    lines: row.lines.as_array().cloned().unwrap_or_default(),
                },
            )
        })
        .collect())
}

/// The bill's reconciliation in the core's currency, or `None` when the
/// bill's currency cannot be converted.
async fn reconcile(
    state: &AppState,
    fold: &tokencontroller_core::State,
    bill: &Bill,
) -> Result<Option<ReconciliationView>, ApiError> {
    let core = fold.currency();
    let rate = rates::between(&state.db, bill.amount.currency(), core, bill.range.to).await?;
    let Ok(rate) = rate else {
        return Ok(None);
    };
    let convert = |money: Money| {
        rate.convert(money)
            .map_err(|error| ApiError::Internal(error.into()))
    };
    let in_core = Bill {
        provider: bill.provider.clone(),
        range: bill.range,
        amount: convert(bill.amount)?,
        contract: bill.contract.clone(),
        contract_kind: bill.contract_kind,
        fee: bill.fee.map(convert).transpose()?,
        extra_usage: bill.extra_usage.map(convert).transpose()?,
    };
    let messages = fold
        .provider_cost(&bill.provider, bill.range)
        .map_err(|error| ApiError::Internal(error.into()))?;
    let reconciliation = fold
        .reconcile(&in_core)
        .map_err(|error| ApiError::Internal(error.into()))?;
    Ok(Some(ReconciliationView {
        currency: core.code(),
        contract: reconciliation.contract.as_str().to_owned(),
        contract_kind: reconciliation.kind.as_str().to_owned(),
        bill: events::json::money(reconciliation.bill),
        messages: events::json::money(reconciliation.messages),
        unexplained: events::json::money(reconciliation.unexplained),
        fee: reconciliation.fee.map(events::json::money),
        extra_usage: reconciliation.extra_usage.map(events::json::money),
        covered: reconciliation.covered.map(events::json::money),
        same_provider_no_contract: events::json::money(reconciliation.same_provider_no_contract),
        contract_from_fallback: events::json::money(reconciliation.contract_from_fallback),
        a_message_is_wrong: reconciliation.a_message_is_wrong(),
        count_errors: messages.count_errors.len(),
        unknown_models: messages
            .not_priced_models
            .iter()
            .map(|model| match &model.provider {
                Some(provider) => format!("{provider}/{}", model.model),
                None => model.model.clone(),
            })
            .collect(),
    }))
}

/// The id of the reversal that reversed `id`, if any was recorded by
/// `as_of`.
fn reversed_by(log: &Log, id: &str, as_of: OffsetDateTime) -> Option<String> {
    log.events.iter().find_map(|event| match &event.record {
        Record::Reversal(target) if target.as_str() == id && event.recorded_at <= as_of => {
            Some(event.id.as_str().to_owned())
        }
        _ => None,
    })
}

fn view(
    event: &Event,
    bill: &Bill,
    contract: String,
    details: Option<&Details>,
    reversed_by: Option<String>,
    reconciliation: Option<ReconciliationView>,
) -> BillView {
    BillView {
        id: event.id.as_str().to_owned(),
        provider: bill.provider.clone(),
        contract,
        contract_kind: bill.contract_kind.map(|kind| kind.as_str().to_owned()),
        fee: bill.fee.map(format_amount),
        extra_usage: bill.extra_usage.map(format_amount),
        from: bill.range.from,
        to: bill.range.to,
        amount: format_amount(bill.amount),
        currency: bill.amount.currency().code(),
        exchange_rate: details
            .and_then(|details| details.exchange_rate)
            .map(|rate| rate.to_string()),
        exchange_rate_given: details.is_some_and(|details| details.exchange_rate_given),
        source: details.map_or(BillSource::Api, |details| details.source),
        is_final: details.is_none_or(|details| details.is_final),
        lines: details
            .map(|details| details.lines.clone())
            .unwrap_or_default(),
        recorded_at: event.recorded_at,
        recorded_by: event
            .recorded_by
            .as_ref()
            .map(|person| person.as_str().to_owned()),
        reversed_by,
        reconciliation,
    }
}

/// Every bill in the log as of a moment, newest range first: the bills in
/// force with their reconciliation, and the reversed ones without.
///
/// # Errors
///
/// Whatever loading the log fails with.
pub async fn bills(
    state: &AppState,
    organization_id: Uuid,
    as_of: OffsetDateTime,
    include_reversed: bool,
) -> Result<Vec<BillView>, ApiError> {
    let log = Log::load(state, organization_id).await?;
    let fold = log.fold(as_of)?;
    let details = all_details(state, organization_id).await?;
    let mut views = Vec::new();
    for event in log.events.iter().filter(|event| event.recorded_at <= as_of) {
        let Record::Bill(bill) = &event.record else {
            continue;
        };
        let reversed = reversed_by(&log, event.id.as_str(), as_of);
        if reversed.is_some() && !include_reversed {
            continue;
        }
        let reconciliation = if reversed.is_some() {
            None
        } else {
            reconcile(state, &fold, bill).await?
        };
        views.push(view(
            event,
            bill,
            fold.contract_of_bill(bill).as_str().to_owned(),
            details.get(event.id.as_str()),
            reversed,
            reconciliation,
        ));
    }
    views.sort_by(|a, b| {
        b.from
            .cmp(&a.from)
            .then_with(|| a.provider.cmp(&b.provider))
            .then_with(|| a.recorded_at.cmp(&b.recorded_at))
    });
    Ok(views)
}

/// One bill by its event id, as of a moment.
///
/// # Errors
///
/// Whatever loading the log fails with.
pub async fn view_of(
    state: &AppState,
    organization_id: Uuid,
    id: &str,
    as_of: OffsetDateTime,
) -> Result<Option<BillView>, ApiError> {
    Ok(bills(state, organization_id, as_of, true)
        .await?
        .into_iter()
        .find(|bill| bill.id == id))
}

/// A bill as the bills endpoint and the web app send it (docs: Reference >
/// Bill schema). Amounts are decimal strings in `currency`.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct BillInput {
    /// Who charged, such as `anthropic` or `openai`.
    #[schema(example = "anthropic")]
    pub provider: String,
    /// The contract the bill is for, such as `anthropic-api`. Left out, it
    /// goes to the provider's only contract.
    #[schema(example = "anthropic-api")]
    pub contract: Option<String>,
    /// `metered` or `flat`: the kind of contract a new key creates.
    pub contract_kind: Option<String>,
    /// The fee part of a flat contract's bill, in `currency`.
    pub fee: Option<String>,
    /// The extra usage part of a flat contract's bill, in `currency`.
    pub extra_usage: Option<String>,
    /// The first day the bill covers, `YYYY-MM-DD`.
    #[schema(example = "2026-03-01")]
    pub from: String,
    /// The last day the bill covers, `YYYY-MM-DD`.
    #[schema(example = "2026-03-31")]
    pub to: String,
    #[schema(example = "1234.56")]
    pub amount: String,
    #[schema(example = "USD")]
    pub currency: String,
    /// To the organization's reporting currency, at the bill's last day.
    /// Left out, Token Controller fills it in.
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
    /// [`BillError`] for an amount, currency, date or rate that cannot be
    /// read.
    pub fn into_new_bill(self, recorded_by: TagValue) -> Result<NewBill, BillError> {
        let currency = Currency::new(&self.currency)
            .map_err(|_| BillError::NotACurrency(self.currency.clone()))?;
        let amount = parse_amount(&self.amount, currency)?;
        let range = crate::dates::range(Some(&self.from), Some(&self.to))
            .map_err(|error| BillError::Range(error.to_string()))?;
        let exchange_rate = self
            .exchange_rate
            .as_deref()
            .map(RateNumber::parse)
            .transpose()?;
        let part = |text: Option<&str>| {
            text.map(|text| parse_amount(text, currency))
                .transpose()
                .map_err(BillError::from)
        };
        Ok(NewBill {
            provider: self.provider.trim().to_owned(),
            contract: self.contract.as_deref().map(contract_key).transpose()?,
            contract_kind: self
                .contract_kind
                .as_deref()
                .map(contract_kind)
                .transpose()?,
            fee: part(self.fee.as_deref())?,
            extra_usage: part(self.extra_usage.as_deref())?,
            range,
            amount,
            exchange_rate,
            source: if self.typed_in {
                BillSource::Manual
            } else {
                BillSource::Api
            },
            lines: self.lines,
            recorded_by: Some(recorded_by),
            is_final: true,
        })
    }
}

/// Send a bill for a date range.
///
/// Writes a bill event when the figures differ from the bill in force for
/// the provider and range, reversing that one, and answers 200 with the
/// unchanged bill when they do not. Org managers only.
#[utoipa::path(
    post,
    path = "/bills",
    tag = "bills",
    request_body = BillInput,
    responses(
        (status = CREATED, description = "Stored as a bill event", body = BillView),
        (status = OK, description = "The same figures were already stored", body = BillView),
        (status = BAD_REQUEST, description = "The figures cannot be read", body = crate::error::ErrorBody),
        (status = FORBIDDEN, description = "Only an org manager can send bills", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn post_bill(
    State(state): State<AppState>,
    caller: Caller,
    Json(input): Json<BillInput>,
) -> Result<(StatusCode, Json<BillView>), ApiError> {
    caller.require_credential(&[CredentialKind::Session, CredentialKind::Service])?;
    let person = caller.person()?;
    require_org_manager(&state, &caller).await?;
    let bill = input.into_new_bill(person)?;
    let recorded = record_bill(&state, caller.organization_id, bill).await??;
    let status = if recorded.changed {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(recorded.bill)))
}

/// Refuses callers who are not org managers as of now.
async fn require_org_manager(state: &AppState, caller: &Caller) -> Result<(), ApiError> {
    let person = caller.person()?;
    let log = Log::load(state, caller.organization_id).await?;
    let fold = log.fold(OffsetDateTime::now_utc())?;
    access::require_org_manager(&fold, &person)
}

/// Which bills to list.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct BillsQuery {
    /// The moment to look back to; default now.
    #[param(example = "2026-04-01T00:00:00Z")]
    pub as_of: Option<String>,
    /// Only bills whose range starts on or after this day.
    #[param(example = "2026-03-01")]
    pub from: Option<String>,
    /// Only bills whose range ends on or before this day.
    #[param(example = "2026-03-31")]
    pub to: Option<String>,
    /// Only this provider.
    #[param(example = "anthropic")]
    pub provider: Option<String>,
    /// `true` to include reversed bills.
    pub reversed: Option<bool>,
    /// `csv` for the table as a CSV file instead of JSON.
    #[param(example = "csv")]
    pub format: Option<String>,
}

/// The columns of the bills table and its CSV file.
const BILL_COLUMNS: &[crate::reports::csv::Column] = &[
    ("id", "/id"),
    ("provider", "/provider"),
    ("from", "/from"),
    ("to", "/to"),
    ("amount", "/amount"),
    ("currency", "/currency"),
    ("exchange_rate", "/exchange_rate"),
    ("source", "/source"),
    ("final", "/final"),
    ("messages", "/reconciliation/messages"),
    ("unexplained", "/reconciliation/unexplained"),
    ("recorded_at", "/recorded_at"),
    ("reversed_by", "/reversed_by"),
];

/// The bills in force, newest range first, each with its reconciliation.
#[utoipa::path(
    get,
    path = "/bills",
    tag = "bills",
    params(BillsQuery),
    responses(
        (status = OK, description = "The bills, as JSON or CSV", content((Vec<BillView> = "application/json"), (String = "text/csv"))),
        (status = FORBIDDEN, description = "Only an org manager can read bills", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn list_bills(
    State(state): State<AppState>,
    caller: Caller,
    Query(query): Query<BillsQuery>,
) -> Result<Response, ApiError> {
    require_org_manager(&state, &caller).await?;
    let as_of = crate::dates::as_of(query.as_of.as_deref())?;
    let from = crate::dates::parse_optional(query.from.as_deref())?;
    let to = crate::dates::parse_optional(query.to.as_deref())?;
    let bills: Vec<BillView> = bills(
        &state,
        caller.organization_id,
        as_of,
        query.reversed.unwrap_or(false),
    )
    .await?
    .into_iter()
    .filter(|bill| from.is_none_or(|from| bill.from >= from))
    .filter(|bill| to.is_none_or(|to| bill.to <= to))
    .filter(|bill| {
        query
            .provider
            .as_deref()
            .is_none_or(|provider| bill.provider == provider)
    })
    .collect();
    crate::reports::csv::table(
        &bills,
        BILL_COLUMNS,
        crate::reports::csv::wants_csv(query.format.as_deref()),
        "bills.csv",
    )
}

/// One bill and the bills it restated, oldest first.
#[utoipa::path(
    get,
    path = "/bills/{id}",
    tag = "bills",
    params(("id" = String, Path, description = "The bill event's id"), crate::reports::csv::FormatQuery),
    responses(
        (status = OK, description = "The bill and the ones it restated, as JSON or CSV", content((Vec<BillView> = "application/json"), (String = "text/csv"))),
        (status = NOT_FOUND, description = "No such bill", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn get_bill(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
    Query(query): Query<crate::reports::csv::FormatQuery>,
) -> Result<Response, ApiError> {
    require_org_manager(&state, &caller).await?;
    let all = bills(
        &state,
        caller.organization_id,
        OffsetDateTime::now_utc(),
        true,
    )
    .await?;
    let Some(bill) = all.iter().find(|bill| bill.id == id) else {
        return Err(ApiError::NotFound);
    };
    let mut history: Vec<BillView> = all
        .iter()
        .filter(|other| {
            other.provider == bill.provider
                && other.contract == bill.contract
                && other.from == bill.from
                && other.to == bill.to
                && other.recorded_at <= bill.recorded_at
        })
        .cloned()
        .collect();
    history.sort_by_key(|view| view.recorded_at);
    let filename = format!(
        "bill-{}-{}.csv",
        bill.provider,
        crate::dates::text(bill.from)
    );
    crate::reports::csv::table(&history, BILL_COLUMNS, query.wants_csv(), &filename)
}

/// One row of the reconciliation table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ReconciliationRow {
    pub bill_id: String,
    pub provider: String,
    #[serde(with = "crate::dates::day")]
    #[schema(value_type = String, format = Date)]
    pub from: Date,
    #[serde(with = "crate::dates::day")]
    #[schema(value_type = String, format = Date)]
    pub to: Date,
    #[serde(rename = "final")]
    pub is_final: bool,
    #[serde(flatten)]
    pub figures: ReconciliationView,
}

/// Which reconciliation to show.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ReconciliationQuery {
    /// The moment to look back to; default now.
    pub as_of: Option<String>,
    /// Only this provider.
    pub provider: Option<String>,
    /// `csv` for the table as a CSV file instead of JSON.
    pub format: Option<String>,
}

const RECONCILIATION_COLUMNS: &[crate::reports::csv::Column] = &[
    ("provider", "/provider"),
    ("contract", "/contract"),
    ("contract_kind", "/contract_kind"),
    ("from", "/from"),
    ("to", "/to"),
    ("currency", "/currency"),
    ("bill", "/bill"),
    ("messages", "/messages"),
    ("unexplained", "/unexplained"),
    ("fee", "/fee"),
    ("extra_usage", "/extra_usage"),
    ("covered", "/covered"),
    ("same_provider_no_contract", "/same_provider_no_contract"),
    ("contract_from_fallback", "/contract_from_fallback"),
    ("a_message_is_wrong", "/a_message_is_wrong"),
    ("final", "/final"),
    ("bill_id", "/bill_id"),
];

/// Each bill against the cost of the messages with its contract in its
/// range, with the line no message explains, and beside it the messages of
/// the same provider with no contract and those whose contract came from
/// the fallback. Org managers only.
#[utoipa::path(
    get,
    path = "/reconciliation",
    tag = "bills",
    params(ReconciliationQuery),
    responses(
        (status = OK, description = "One row per bill in force", content((Vec<ReconciliationRow> = "application/json"), (String = "text/csv"))),
        (status = FORBIDDEN, description = "Only an org manager can read the reconciliation", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn get_reconciliation(
    State(state): State<AppState>,
    caller: Caller,
    Query(query): Query<ReconciliationQuery>,
) -> Result<Response, ApiError> {
    require_org_manager(&state, &caller).await?;
    let as_of = crate::dates::as_of(query.as_of.as_deref())?;
    let rows: Vec<ReconciliationRow> = bills(&state, caller.organization_id, as_of, false)
        .await?
        .into_iter()
        .filter(|bill| {
            query
                .provider
                .as_deref()
                .is_none_or(|provider| bill.provider == provider)
        })
        .filter_map(|bill| {
            Some(ReconciliationRow {
                bill_id: bill.id,
                provider: bill.provider,
                from: bill.from,
                to: bill.to,
                is_final: bill.is_final,
                figures: bill.reconciliation?,
            })
        })
        .collect();
    crate::reports::csv::table(
        &rows,
        RECONCILIATION_COLUMNS,
        crate::reports::csv::wants_csv(query.format.as_deref()),
        "reconciliation.csv",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    fn usd(text: &str) -> Money {
        parse_amount(text, Currency::USD).unwrap()
    }

    fn bill(amount: &str) -> NewBill {
        NewBill {
            contract: None,
            contract_kind: None,
            fee: None,
            extra_usage: None,
            provider: "anthropic".to_owned(),
            range: DateRange::new(date!(2026 - 03 - 01), date!(2026 - 03 - 31)).unwrap(),
            amount: usd(amount),
            exchange_rate: None,
            source: BillSource::Api,
            lines: Vec::new(),
            recorded_by: None,
            is_final: true,
        }
    }

    #[test]
    fn a_bill_has_a_plain_provider_and_an_amount_at_or_above_zero() {
        assert_eq!(check(&bill("1000")), Ok(()));
        assert_eq!(check(&bill("0")), Ok(()));
        assert_eq!(check(&bill("-1")), Err(BillError::Negative));
        let mut odd = bill("100");
        odd.provider = "Anthropic Inc".to_owned();
        assert!(matches!(check(&odd), Err(BillError::Provider(_))));
        assert!(is_provider("aws_bedrock"));
        assert!(!is_provider(""));
    }

    #[test]
    fn the_input_reads_dates_amount_and_rate() {
        let input = BillInput {
            provider: "openai".to_owned(),
            from: "2026-03-01".to_owned(),
            to: "2026-03-31".to_owned(),
            amount: "90".to_owned(),
            currency: "USD".to_owned(),
            exchange_rate: Some("0.9".to_owned()),
            typed_in: true,
            lines: Vec::new(),
            contract: None,
            contract_kind: None,
            fee: None,
            extra_usage: None,
        };
        let new = input
            .into_new_bill(TagValue::new("mara@example.com").unwrap())
            .unwrap();
        assert_eq!(new.amount, usd("90"));
        assert_eq!(new.range.days(), 31);
        assert_eq!(new.source, BillSource::Manual);
        assert_eq!(new.exchange_rate, Some(RateNumber::parse("0.9").unwrap()));
        assert_eq!(check(&new), Ok(()));
        let backwards = BillInput {
            from: "2026-03-31".to_owned(),
            to: "2026-03-01".to_owned(),
            provider: "openai".to_owned(),
            amount: "1".to_owned(),
            currency: "USD".to_owned(),
            exchange_rate: None,
            typed_in: false,
            lines: Vec::new(),
            contract: None,
            contract_kind: None,
            fee: None,
            extra_usage: None,
        };
        assert!(matches!(
            backwards.into_new_bill(TagValue::new("mara@example.com").unwrap()),
            Err(BillError::Range(_))
        ));
    }

    #[test]
    fn a_bill_names_its_contract_and_the_parts_of_a_flat_one() {
        let input = BillInput {
            provider: "anthropic".to_owned(),
            from: "2026-03-01".to_owned(),
            to: "2026-03-31".to_owned(),
            amount: "150".to_owned(),
            currency: "USD".to_owned(),
            exchange_rate: None,
            typed_in: false,
            lines: Vec::new(),
            contract: Some(" anthropic-max ".to_owned()),
            contract_kind: Some("flat".to_owned()),
            fee: Some("100".to_owned()),
            extra_usage: Some("50".to_owned()),
        };
        let mara = TagValue::new("mara@example.com").unwrap();
        let new = input.clone().into_new_bill(mara.clone()).unwrap();
        assert_eq!(
            new.contract.as_ref().map(TagValue::as_str),
            Some("anthropic-max")
        );
        assert_eq!(new.contract_kind, Some(ContractKind::Flat));
        assert_eq!(new.fee, Some(usd("100")));
        assert_eq!(new.extra_usage, Some(usd("50")));
        assert_eq!(check(&new), Ok(()));
        let mut too_big = new.clone();
        too_big.fee = Some(usd("151"));
        assert_eq!(check(&too_big), Err(BillError::Parts));
        let mut negative = new;
        negative.extra_usage = Some(usd("-1"));
        assert_eq!(check(&negative), Err(BillError::Parts));
        let odd_kind = BillInput {
            contract_kind: Some("monthly".to_owned()),
            ..input.clone()
        };
        assert!(matches!(
            odd_kind.into_new_bill(mara.clone()),
            Err(BillError::ContractKind(_))
        ));
        let odd_key = BillInput {
            contract: Some("   ".to_owned()),
            ..input
        };
        assert!(matches!(
            odd_key.into_new_bill(mara),
            Err(BillError::Contract(_))
        ));
    }

    #[test]
    fn source_names_round_trip() {
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
