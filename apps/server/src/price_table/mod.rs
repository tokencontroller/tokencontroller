//! The price table (docs: Account > Price table, Reference > Price tables,
//! ADR 0024): the only cost basis. Managed rows come from the file
//! `TC_PRICE_TABLE` names and from the build's hooks; an organization's own
//! rows and overrides are events in its log, and win over managed rows.
//!
//! Owner: the service platform work. Keep this area's code, routes and
//! queries in this folder, and its migrations in `migrations/` named
//! `<timestamp>_price_table_<what>.sql`.

use std::time::Duration;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use time::{Date, OffsetDateTime};
use tokencontroller_core::state::State as Fold;
use tokencontroller_core::{Currency, Money, PriceRow, PriceSource, Record, TokenKind};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::AppState;
use crate::error::ApiError;
use crate::events::json::{self, Amount};
use crate::events::{self, Log, access};
use crate::sign_in::Member;

/// The job that reads the price table file again.
pub const JOB: &str = "price-table";
/// How often the file is read again.
pub const JOB_EVERY: Duration = Duration::from_mins(15);

/// This area's routes, relative to `/api/v1`.
#[must_use]
pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(get_price_table))
        .routes(routes!(add_row))
}

/// One row as the API shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PriceRowView {
    /// The row's event id. An organization's row is reversed by it;
    /// managed rows cannot be reversed.
    pub id: String,
    pub provider: String,
    pub model: String,
    /// `input`, `output`, `cache_read` or `cache_write`.
    #[schema(value_type = String, example = "input")]
    pub kind: TokenKind,
    /// US dollars per million tokens, as a decimal string.
    pub price_per_million: String,
    #[serde(with = "crate::dates::day")]
    #[schema(value_type = String, format = Date)]
    pub valid_from: Date,
    /// `managed` or `organization`.
    #[schema(value_type = String, example = "organization")]
    pub source: PriceSource,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub recorded_at: OffsetDateTime,
}

/// The table as of a moment.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PriceTableView {
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub as_of: OffsetDateTime,
    pub rows: Vec<PriceRowView>,
}

/// A new row of the organization's own.
#[derive(Debug, Deserialize, ToSchema)]
pub struct NewPriceRow {
    pub provider: String,
    pub model: String,
    #[schema(value_type = String, example = "input")]
    pub kind: TokenKind,
    /// US dollars per million tokens, as a number or a decimal string.
    #[schema(value_type = String, example = "15.00")]
    pub price_per_million: Amount,
    /// `YYYY-MM-DD`.
    pub valid_from: String,
}

/// What a list is cut by.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PriceTableQuery {
    /// The date up to which the log is read. Default today.
    pub as_of: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
}

/// The rows in force as of the fold, each with its event.
#[must_use]
pub fn rows_view(log: &Log, fold: &Fold) -> Vec<PriceRowView> {
    let mut views: Vec<PriceRowView> = log
        .events
        .iter()
        .filter(|event| event.recorded_at <= fold.as_of())
        .filter_map(|event| match &event.record {
            Record::PriceRow(row) if fold.prices().rows().any(|live| live == row) => {
                Some(PriceRowView {
                    id: event.id.to_string(),
                    provider: row.provider.clone(),
                    model: row.model.clone(),
                    kind: row.kind,
                    price_per_million: json::money_exact(row.per_million),
                    valid_from: row.valid_from,
                    source: row.source,
                    recorded_at: event.recorded_at,
                })
            }
            _ => None,
        })
        .collect();
    views.sort_by(|a, b| {
        a.provider
            .cmp(&b.provider)
            .then(a.model.cmp(&b.model))
            .then(a.valid_from.cmp(&b.valid_from))
            .then(a.kind.cmp(&b.kind))
    });
    views
}

/// The rows in force: the managed table with your own rows and overrides.
#[utoipa::path(
    get,
    path = "/price-table",
    tag = "price table",
    params(PriceTableQuery),
    responses((status = OK, body = PriceTableView)),
)]
async fn get_price_table(
    State(state): State<AppState>,
    member: Member,
    Query(query): Query<PriceTableQuery>,
) -> Result<Json<PriceTableView>, ApiError> {
    let log = Log::load(&state, member.organization_id).await?;
    let fold = log.fold(crate::dates::as_of(query.as_of.as_deref())?)?;
    let rows = rows_view(&log, &fold)
        .into_iter()
        .filter(|row| {
            query
                .provider
                .as_deref()
                .is_none_or(|provider| provider == row.provider)
        })
        .filter(|row| {
            query
                .model
                .as_deref()
                .is_none_or(|model| model == row.model)
        })
        .collect();
    Ok(Json(PriceTableView {
        as_of: fold.as_of(),
        rows,
    }))
}

/// Reads a price per million tokens.
///
/// # Errors
///
/// [`ApiError::BadRequest`] for a negative or unreadable price.
pub fn price(amount: &Amount) -> Result<Money, ApiError> {
    let price = amount.money(Currency::USD)?;
    if price.nanos().is_negative() {
        return Err(ApiError::BadRequest("a price is not negative".to_owned()));
    }
    Ok(price)
}

/// Add a row of your own, for a model the managed table lacks or a price
/// you negotiated: the same provider, model, kind and valid-from date as a
/// managed row overrides it. Org managers only. A wrong row is reversed,
/// never edited.
#[utoipa::path(
    post,
    path = "/price-table/rows",
    tag = "price table",
    request_body = NewPriceRow,
    responses(
        (status = CREATED, body = PriceRowView),
        (status = BAD_REQUEST, body = crate::error::ErrorBody),
        (status = CONFLICT, description = "The same row is already in force", body = crate::error::ErrorBody),
        (status = FORBIDDEN, body = crate::error::ErrorBody),
    ),
)]
async fn add_row(
    State(state): State<AppState>,
    member: Member,
    Json(request): Json<NewPriceRow>,
) -> Result<(StatusCode, Json<PriceRowView>), ApiError> {
    member.require_session()?;
    let log = Log::load(&state, member.organization_id).await?;
    let fold = log.fold(OffsetDateTime::now_utc())?;
    let person = member.person()?;
    access::require_org_manager(&fold, &person)?;
    let provider = request.provider.trim().to_lowercase();
    if !crate::ingest::provider::PROVIDERS.contains(&provider.as_str()) {
        return Err(ApiError::BadRequest(format!(
            "provider {provider:?} is not one of {}",
            crate::ingest::provider::PROVIDERS.join(", ")
        )));
    }
    let model = request.model.trim();
    if model.is_empty() || model.chars().count() > 200 {
        return Err(ApiError::BadRequest(
            "model has 1 to 200 characters".to_owned(),
        ));
    }
    let row = PriceRow {
        provider,
        model: model.to_owned(),
        kind: request.kind,
        per_million: price(&request.price_per_million)?,
        valid_from: crate::dates::parse(&request.valid_from)?,
        source: PriceSource::Organization,
    };
    if fold.prices().rows().any(|live| *live == row) {
        return Err(ApiError::Conflict(
            "the same row is already in force".to_owned(),
        ));
    }
    let event = events::event(Record::PriceRow(row.clone()))?.by(person);
    events::write(&state, &log, std::slice::from_ref(&event)).await?;
    crate::organizations::record_audit_event(
        &state,
        member.organization_id,
        Some(member.person_id),
        "price_row.added",
        None,
    );
    Ok((
        StatusCode::CREATED,
        Json(PriceRowView {
            id: event.id.to_string(),
            provider: row.provider,
            model: row.model,
            kind: row.kind,
            price_per_million: json::money_exact(row.per_million),
            valid_from: row.valid_from,
            source: row.source,
            recorded_at: event.recorded_at,
        }),
    ))
}

/// Why the price table file cannot be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FileError {
    #[error("line {line}: {problem}")]
    Row { line: u64, problem: String },
    #[error("the file needs the columns provider, model, kind, price_per_million and valid_from")]
    Header,
    #[error("the file cannot be read as CSV: {0}")]
    Unreadable(String),
}

/// Reads the rows of a price table file: `provider,model,kind,
/// price_per_million,valid_from`.
///
/// # Errors
///
/// [`FileError`] naming the first row that is wrong.
pub fn parse_file(text: &str) -> Result<Vec<PriceRow>, FileError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .flexible(true)
        .from_reader(text.as_bytes());
    let headers: Vec<String> = reader
        .headers()
        .map_err(|error| FileError::Unreadable(error.to_string()))?
        .iter()
        .map(str::to_owned)
        .collect();
    let column = |name: &str| headers.iter().position(|header| header == name);
    let (Some(provider), Some(model), Some(kind), Some(price), Some(valid_from)) = (
        column("provider"),
        column("model"),
        column("kind"),
        column("price_per_million"),
        column("valid_from"),
    ) else {
        return Err(FileError::Header);
    };
    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record.map_err(|error| FileError::Unreadable(error.to_string()))?;
        let line = record.position().map_or(0, csv::Position::line);
        let cell = |index: usize| record.get(index).unwrap_or_default().trim().to_owned();
        if record.iter().all(|cell| cell.trim().is_empty()) {
            continue;
        }
        let problem = |problem: String| FileError::Row { line, problem };
        let kind_text = cell(kind);
        let kind: TokenKind = serde_json::from_value(serde_json::Value::String(kind_text.clone()))
            .map_err(|_| {
                problem(format!(
                    "{kind_text:?} is not input, output, cache_read or cache_write"
                ))
            })?;
        let price_text = cell(price);
        let per_million = Money::parse(&price_text, Currency::USD)
            .map_err(|_| problem(format!("{price_text:?} is not a price")))?;
        if per_million.nanos().is_negative() {
            return Err(problem("a price is not negative".to_owned()));
        }
        let date_text = cell(valid_from);
        let valid_from = crate::dates::parse(&date_text)
            .map_err(|_| problem(format!("{date_text:?} is not a date such as 2026-01-01")))?;
        let provider = cell(provider).to_lowercase();
        let model = cell(model);
        if provider.is_empty() || model.is_empty() {
            return Err(problem("provider and model must not be empty".to_owned()));
        }
        rows.push(PriceRow {
            provider,
            model,
            kind,
            per_million,
            valid_from,
            source: PriceSource::Managed,
        });
    }
    Ok(rows)
}

/// Reads the file `TC_PRICE_TABLE` names into the managed rows, if one is
/// set. A row seen before keeps its load time; a changed price is a new
/// row only when its valid-from date is new, as the docs say rows are
/// never edited. Returns how many rows the table holds.
///
/// # Errors
///
/// Fails when the file cannot be read, so the service refuses to start on
/// a wrong file.
pub async fn load_file(state: &AppState) -> anyhow::Result<usize> {
    let Some(path) = state.config.price_table.as_deref() else {
        return Ok(0);
    };
    let text = tokio::fs::read_to_string(path)
        .await
        .map_err(|error| anyhow::anyhow!("TC_PRICE_TABLE {path}: {error}"))?;
    let rows =
        parse_file(&text).map_err(|error| anyhow::anyhow!("TC_PRICE_TABLE {path}: {error}"))?;
    store_managed(&state.db, &rows).await?;
    Ok(rows.len())
}

/// Writes managed rows, keeping the rows there are.
///
/// # Errors
///
/// Fails if a query fails.
pub async fn store_managed(db: &sqlx::PgPool, rows: &[PriceRow]) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;
    for row in rows {
        let kind = serde_json::to_value(row.kind)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default();
        sqlx::query!(
            r#"INSERT INTO managed_price_rows (provider, model, kind, per_million_nanos, valid_from)
               VALUES ($1, $2, $3, $4, $5)
               ON CONFLICT (provider, model, kind, valid_from) DO UPDATE
               SET per_million_nanos = EXCLUDED.per_million_nanos"#,
            row.provider,
            row.model,
            kind,
            row.per_million.nanos(),
            row.valid_from
        )
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// The job: reads the file again, so a changed file is picked up without
/// a restart.
///
/// # Errors
///
/// Whatever reading the file fails with.
pub async fn run_job(state: AppState) -> anyhow::Result<String> {
    let rows = load_file(&state).await?;
    Ok(if state.config.price_table.is_some() {
        format!("{rows} rows in the price table file")
    } else {
        "no price table file".to_owned()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "provider,model,kind,price_per_million,valid_from\n\
        anthropic,claude-opus-5,input,15.00,2026-01-01\n\
        anthropic,claude-opus-5,output,75.00,2026-01-01\n\
        \n\
        anthropic,claude-opus-5,cache_read,1.50,2026-01-01\n\
        anthropic,claude-opus-5,cache_write,18.75,2026-01-01\n";

    #[test]
    fn the_docs_file_reads_into_four_rows() {
        let rows = parse_file(FILE).unwrap();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[3].kind, TokenKind::CacheWrite);
        assert_eq!(json::money_exact(rows[3].per_million), "18.75");
        assert_eq!(rows[0].source, PriceSource::Managed);
    }

    #[test]
    fn a_wrong_row_names_its_line() {
        let wrong =
            "provider,model,kind,price_per_million,valid_from\nanthropic,m,tokens,1,2026-01-01\n";
        assert!(matches!(
            parse_file(wrong),
            Err(FileError::Row { line: 2, .. })
        ));
        assert_eq!(parse_file("provider,model\n"), Err(FileError::Header));
        let negative =
            "provider,model,kind,price_per_million,valid_from\nanthropic,m,input,-1,2026-01-01\n";
        assert!(parse_file(negative).is_err());
        assert!(price(&Amount::Text("-1".to_owned())).is_err());
        assert_eq!(
            price(&Amount::Text("15".to_owned())).unwrap().nanos(),
            15_000_000_000
        );
    }
}
