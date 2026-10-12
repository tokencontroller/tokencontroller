//! Bills uploaded as CSV (docs: Integrations > CSV bills): our own format,
//! one bill per row, or the cost export of Bedrock, Vertex or Foundry as
//! one period's bill. The preview shows what the file holds before anything
//! is saved; the upload hands each bill to the bills area.

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::bill_connections::{bill_cost_source, new_bill};
use super::bills::csv::{self, Cloud, CsvBillError};
use super::bills::{Period, PulledBill};
use crate::AppState;
use crate::bills::amount::format_amount;
use crate::bills::{BillSource, BillView, CostSource, NewBill, record_bill};
use crate::error::{ApiError, ErrorBody};
use crate::organizations::Role;
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
    /// The period a cloud's export is the bill for, such as `2026-09`.
    #[param(value_type = Option<String>, example = "2026-09")]
    pub period: Option<Period>,
}

/// One bill as the file holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct BillCsvBill {
    #[schema(value_type = String, example = "2026-09")]
    pub period: Period,
    pub provider: String,
    pub cost_source: CostSource,
    #[schema(example = "USD")]
    pub currency: String,
    #[schema(example = "1234.56")]
    pub gross: String,
    #[schema(example = "0.00")]
    pub discount: String,
    #[schema(example = "0.00")]
    pub credit: String,
    #[schema(example = "1234.56")]
    pub net: String,
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
    /// The latest revision of each bill in the file.
    pub bills: Vec<BillView>,
    /// How many of them got a new revision; the rest were already stored
    /// with the same figures.
    pub changed: usize,
}

/// Reads the file, or names the first problem in it.
fn read(query: &BillCsvQuery, body: &Bytes) -> Result<Vec<PulledBill>, CsvBillError> {
    let text = std::str::from_utf8(body).map_err(|_| CsvBillError {
        row: None,
        column: None,
        message: "The file is not UTF-8 text. Save it as CSV in UTF-8 and upload it again."
            .to_owned(),
    })?;
    match (query.cloud, query.period) {
        (None, _) => csv::parse_bills(text),
        (Some(cloud), Some(period)) => {
            csv::parse_cloud_export(cloud, text, period).map(|bill| vec![bill])
        }
        (Some(_), None) => Err(CsvBillError {
            row: None,
            column: None,
            message: "Give the period the export is the bill for, such as 2026-09.".to_owned(),
        }),
    }
}

/// Every bill of the file as the bills area takes it, each checked as
/// `record_bill` will check it, so a file is stored whole or not at all.
fn checked(pulled: &[PulledBill], person_id: Uuid) -> Result<Vec<NewBill>, String> {
    pulled
        .iter()
        .map(|bill| {
            let new = new_bill(bill, BillSource::Csv, Some(person_id))?;
            crate::bills::check(&new).map_err(|error| {
                format!(
                    "The {} bill for {}: {error}.",
                    bill.provider_name(),
                    bill.period()
                )
            })?;
            Ok(new)
        })
        .collect()
}

fn shown(bill: &PulledBill) -> Option<BillCsvBill> {
    Some(BillCsvBill {
        period: bill.period(),
        provider: bill.provider_name().to_owned(),
        cost_source: bill_cost_source(bill.cost_source())?,
        currency: bill.currency().code(),
        gross: format_amount(bill.gross()),
        discount: format_amount(bill.discount()),
        credit: format_amount(bill.credit()),
        net: format_amount(bill.net()),
        exchange_rate: bill
            .exchange_rate()
            .map(|rate| rate.normalize().to_string()),
        lines: bill.lines().len(),
    })
}

fn allowed(caller: &Caller) -> Result<(), ApiError> {
    caller.require_role(Role::Controller)?;
    caller.require_credential(&[CredentialKind::Session, CredentialKind::Service])
}

/// Read a CSV bill file and show the bills it holds, or the first problem
/// in it with its row and column. Saves nothing. Controllers only.
#[utoipa::path(
    post,
    path = "/bills/csv/preview",
    tag = "bills",
    params(BillCsvQuery),
    request_body(content = String, content_type = "text/csv"),
    responses(
        (status = OK, body = BillCsvPreview),
        (status = FORBIDDEN, description = "Only a controller can upload bills", body = ErrorBody),
    ),
)]
async fn preview_bill_csv(
    caller: Caller,
    Query(query): Query<BillCsvQuery>,
    body: Bytes,
) -> Result<Json<BillCsvPreview>, ApiError> {
    allowed(&caller)?;
    let read = read(&query, &body).and_then(|bills| match checked(&bills, caller.person_id) {
        Ok(_) => Ok(bills),
        Err(message) => Err(CsvBillError {
            row: None,
            column: None,
            message,
        }),
    });
    Ok(Json(match read {
        Ok(bills) => BillCsvPreview {
            bills: bills.iter().filter_map(shown).collect(),
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
/// stored, so a file with a problem saves nothing. Each bill is stored as a
/// new revision when its figures differ from the latest one. Controllers
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
        (status = FORBIDDEN, description = "Only a controller can upload bills", body = ErrorBody),
    ),
)]
async fn upload_bill_csv(
    State(state): State<AppState>,
    caller: Caller,
    Query(query): Query<BillCsvQuery>,
    body: Bytes,
) -> Result<(StatusCode, Json<BillCsvUploaded>), ApiError> {
    allowed(&caller)?;
    let pulled = read(&query, &body)
        .map_err(|error| ApiError::BadRequest(format!("{error} Nothing was saved.")))?;
    let bills = checked(&pulled, caller.person_id)
        .map_err(|error| ApiError::BadRequest(format!("{error} Nothing was saved.")))?;
    let mut stored = Vec::with_capacity(bills.len());
    let mut changed = 0_usize;
    for bill in bills {
        let recorded = record_bill(&state.db, caller.organization_id, bill).await??;
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
