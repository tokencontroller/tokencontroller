//! Bills uploaded as CSV (docs: Account > Bills > CSV): our own format,
//! one bill per row in the bill schema's columns, or the cost export of
//! Bedrock, Vertex or Foundry as one month's bill. The preview shows what
//! the file holds before anything is saved; the upload hands each bill to
//! the bills area.

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use time::{Date, OffsetDateTime};
use tokencontroller_core::TagValue;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::bill_connections::line_json;
use super::bills::BillMonth;
use super::bills::csv::{self, Cloud, CsvBillError, UploadedBill};
use crate::AppState;
use crate::bills::amount::{RateNumber, format_amount};
use crate::bills::{BillSource, BillView, NewBill, record_bill};
use crate::error::{ApiError, ErrorBody};
use crate::events::{Log, access};
use crate::sign_in::{Caller, CredentialKind};

pub(super) fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(preview_bill_csv))
        .routes(routes!(upload_bill_csv))
}

/// What kind of file is uploaded.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct BillCsvQuery {
    /// The cloud whose own cost export this is: `bedrock`, `vertex` or
    /// `foundry`. Left out, the file is in the bill schema's columns.
    #[param(value_type = Option<String>, example = "bedrock")]
    pub cloud: Option<Cloud>,
    /// The month a cloud's export is the bill for, such as `2026-03`.
    #[param(value_type = Option<String>, example = "2026-03")]
    pub month: Option<BillMonth>,
}

/// One bill as the file holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct BillCsvBill {
    pub provider: String,
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
    #[schema(example = "0.9214")]
    pub exchange_rate: Option<String>,
    /// How many lines a cloud's export was summed from.
    pub lines: usize,
}

/// What is wrong with the file, and where.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct BillCsvProblem {
    /// The line in the file, counting the header as line 1.
    pub row: Option<u64>,
    /// The column as the file names it.
    pub column: Option<String>,
    /// What is wrong, in plain words.
    pub message: String,
}

/// What a CSV file holds, before anything is saved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct BillCsvPreview {
    /// The bills the file holds; empty when there is a problem.
    pub bills: Vec<BillCsvBill>,
    /// The first problem found. Nothing can be uploaded until it is fixed.
    pub problem: Option<BillCsvProblem>,
}

/// What an upload stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct BillCsvUploaded {
    /// Each bill in the file as it now stands.
    pub bills: Vec<BillView>,
    /// How many of them became a new bill event; the rest were already
    /// stored with the same figures.
    pub changed: usize,
}

/// Reads the file, or names the first problem in it.
fn read(query: &BillCsvQuery, body: &Bytes) -> Result<Vec<UploadedBill>, CsvBillError> {
    let text = std::str::from_utf8(body).map_err(|_| CsvBillError {
        row: None,
        column: None,
        message: "The file is not UTF-8 text. Save it as CSV in UTF-8 and upload it again."
            .to_owned(),
    })?;
    match (query.cloud, query.month) {
        (None, _) => csv::parse_bills(text),
        (Some(cloud), Some(month)) => {
            let pulled = csv::parse_cloud_export(cloud, text, month)?;
            Ok(vec![UploadedBill::from_pulled(&pulled)?])
        }
        (Some(_), None) => Err(CsvBillError {
            row: None,
            column: None,
            message: "Give the month the export is the bill for, such as 2026-03.".to_owned(),
        }),
    }
}

/// An uploaded bill as the bills area takes it.
///
/// # Errors
///
/// A plain sentence for an exchange rate the bills area cannot hold.
pub fn new_bill(bill: &UploadedBill, recorded_by: TagValue) -> Result<NewBill, String> {
    let exchange_rate = bill
        .exchange_rate
        .map(|rate| RateNumber::parse(&rate.normalize().to_string()))
        .transpose()
        .map_err(|error| error.to_string())?;
    let mut lines = Vec::new();
    if let Some(charging) = bill.charging {
        lines.push(serde_json::json!({
            "description": "charging",
            "charging": charging.as_str(),
        }));
    }
    lines.extend(bill.lines.iter().map(line_json));
    Ok(NewBill {
        provider: bill.provider.clone(),
        range: bill.range,
        amount: bill.amount,
        exchange_rate,
        source: BillSource::Csv,
        lines,
        recorded_by: Some(recorded_by),
        is_final: true,
    })
}

/// Every bill of the file as the bills area takes it, each checked as
/// `record_bill` will check it, so a file is stored whole or not at all.
fn checked(uploaded: &[UploadedBill], person: &TagValue) -> Result<Vec<NewBill>, String> {
    uploaded
        .iter()
        .map(|bill| {
            let new = new_bill(bill, person.clone())?;
            crate::bills::check(&new).map_err(|error| {
                format!(
                    "The {} bill from {} to {}: {error}.",
                    bill.provider, bill.range.from, bill.range.to
                )
            })?;
            Ok(new)
        })
        .collect()
}

fn shown(bill: &UploadedBill) -> BillCsvBill {
    BillCsvBill {
        provider: bill.provider.clone(),
        from: bill.range.from,
        to: bill.range.to,
        amount: format_amount(bill.amount),
        currency: bill.currency().code(),
        exchange_rate: bill.exchange_rate.map(|rate| rate.normalize().to_string()),
        lines: bill.lines.len(),
    }
}

async fn allowed(state: &AppState, caller: &Caller) -> Result<TagValue, ApiError> {
    caller.require_credential(&[CredentialKind::Session, CredentialKind::Service])?;
    let person = caller.person()?;
    let log = Log::load(state, caller.organization_id).await?;
    let fold = log.fold(OffsetDateTime::now_utc())?;
    access::require_org_manager(&fold, &person)?;
    Ok(person)
}

/// Read a CSV bill file and show the bills it holds, or the first problem
/// in it with its row and column. Saves nothing. Org managers only.
#[utoipa::path(
    post,
    path = "/bills/csv/preview",
    tag = "bills",
    params(BillCsvQuery),
    request_body(content = String, content_type = "text/csv"),
    responses(
        (status = OK, body = BillCsvPreview),
        (status = FORBIDDEN, description = "Only an org manager can upload bills", body = ErrorBody),
    ),
)]
async fn preview_bill_csv(
    State(state): State<AppState>,
    caller: Caller,
    Query(query): Query<BillCsvQuery>,
    body: Bytes,
) -> Result<Json<BillCsvPreview>, ApiError> {
    let person = allowed(&state, &caller).await?;
    let read = read(&query, &body).and_then(|bills| match checked(&bills, &person) {
        Ok(_) => Ok(bills),
        Err(message) => Err(CsvBillError {
            row: None,
            column: None,
            message,
        }),
    });
    Ok(Json(match read {
        Ok(bills) => BillCsvPreview {
            bills: bills.iter().map(shown).collect(),
            problem: None,
        },
        Err(error) => BillCsvPreview {
            bills: Vec::new(),
            problem: Some(BillCsvProblem {
                row: error.row,
                column: error.column,
                message: error.message,
            }),
        },
    }))
}

/// Upload a CSV bill file. Every bill in it is checked before any is
/// stored, so a file with a problem saves nothing. Each bill becomes a bill
/// event when its figures differ from the bill in force. Org managers
/// only.
#[utoipa::path(
    post,
    path = "/bills/csv",
    tag = "bills",
    params(BillCsvQuery),
    request_body(content = String, content_type = "text/csv"),
    responses(
        (status = CREATED, body = BillCsvUploaded),
        (status = BAD_REQUEST, description = "The file has a problem; nothing was saved", body = ErrorBody),
        (status = FORBIDDEN, description = "Only an org manager can upload bills", body = ErrorBody),
    ),
)]
async fn upload_bill_csv(
    State(state): State<AppState>,
    caller: Caller,
    Query(query): Query<BillCsvQuery>,
    body: Bytes,
) -> Result<(StatusCode, Json<BillCsvUploaded>), ApiError> {
    let person = allowed(&state, &caller).await?;
    let uploaded = read(&query, &body)
        .map_err(|error| ApiError::BadRequest(format!("{error} Nothing was saved.")))?;
    let bills = checked(&uploaded, &person)
        .map_err(|error| ApiError::BadRequest(format!("{error} Nothing was saved.")))?;
    let mut stored = Vec::new();
    let mut changed = 0_usize;
    for bill in bills {
        let recorded = record_bill(&state, caller.organization_id, bill).await??;
        if recorded.changed {
            changed = changed.saturating_add(1);
        }
        stored.push(recorded.bill);
    }
    Ok((
        StatusCode::CREATED,
        Json(BillCsvUploaded {
            bills: stored,
            changed,
        }),
    ))
}
