//! Your own reports (docs: Report > Reading reports): a saved tag filter
//! with a grouping, such as cost per team for `client: acme`. Everyone in
//! the organization sees the saved reports; each person sees the cost in
//! them that their role covers.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::READERS;
use super::cost::{self, Grouping, Report, Spec, View};
use super::filter::Filter;
use super::lines::StatusView;
use crate::AppState;
use crate::close::period::Period;
use crate::error::ApiError;
use crate::organizations::Role;
use crate::sign_in::{Caller, Credential};

pub(super) fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_saved, save_report))
        .routes(routes!(run_saved, remove_saved))
}

/// A saved report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct SavedReport {
    pub id: Uuid,
    #[schema(example = "ACME by team")]
    pub name: String,
    pub filter: Filter,
    /// A tag name, or `agent_harness`, `agent_name`, `model`, `effort`,
    /// `speed`, `day` or `cost_kind`.
    #[schema(example = "team")]
    pub by: String,
    pub created_by: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub created_at: OffsetDateTime,
}

/// A report to save.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct SaveInput {
    #[schema(example = "ACME by team")]
    pub name: String,
    /// A tag filter in the URL form, such as `client:acme`.
    #[serde(default)]
    #[schema(example = "client:acme")]
    pub filter: String,
    #[schema(example = "team")]
    pub by: String,
}

/// The saved reports of the organization, by name.
#[utoipa::path(
    get,
    path = "/reports/saved",
    tag = "reports",
    responses((status = OK, description = "Saved reports", body = Vec<SavedReport>)),
)]
pub(crate) async fn list_saved(State(state): State<AppState>, caller: Caller) -> Result<Json<Vec<SavedReport>>, ApiError> {
    caller.require_credential(READERS)?;
    let saved = sqlx::query!(
        r#"SELECT r.id, r.name, r.filter, r.group_by, p.name AS "created_by?", r.created_at
           FROM saved_reports r LEFT JOIN people p ON p.id = r.created_by
           WHERE r.organization_id = $1
           ORDER BY lower(r.name), r.created_at"#,
        caller.organization_id,
    )
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .map(|row| SavedReport {
        id: row.id,
        name: row.name,
        filter: Filter::from_json(&row.filter),
        by: row.group_by,
        created_by: row.created_by,
        created_at: row.created_at,
    })
    .collect();
    Ok(Json(saved))
}

/// Save a tag filter with a grouping as a report.
#[utoipa::path(
    post,
    path = "/reports/saved",
    tag = "reports",
    request_body = SaveInput,
    responses(
        (status = CREATED, description = "Saved", body = SavedReport),
        (status = BAD_REQUEST, description = "No name, no grouping, or a filter that cannot be read", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn save_report(
    State(state): State<AppState>,
    caller: Caller,
    Json(input): Json<SaveInput>,
) -> Result<(StatusCode, Json<SavedReport>), ApiError> {
    caller.require_credential(&[Credential::Session])?;
    let name = input.name.trim();
    if name.is_empty() {
        return Err(ApiError::BadRequest("give the report a name".to_owned()));
    }
    let by = Grouping::parse(&input.by);
    if by.name().is_empty() {
        return Err(ApiError::BadRequest("say what the report groups by, such as team".to_owned()));
    }
    let filter = Filter::parse(&input.filter).map_err(|error| ApiError::BadRequest(error.to_string()))?;
    let row = sqlx::query!(
        r#"INSERT INTO saved_reports (organization_id, name, filter, group_by, created_by)
           VALUES ($1, $2, $3, $4, $5)
           RETURNING id, created_at"#,
        caller.organization_id,
        name,
        filter.to_json(),
        by.name(),
        caller.person_id,
    )
    .fetch_one(&state.db)
    .await?;
    let created_by = sqlx::query_scalar!("SELECT name FROM people WHERE id = $1", caller.person_id)
        .fetch_optional(&state.db)
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(SavedReport {
            id: row.id,
            name: name.to_owned(),
            filter,
            by: by.name().to_owned(),
            created_by,
            created_at: row.created_at,
        }),
    ))
}

/// Which period of a saved report to show, and how.
#[derive(Debug, Clone, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct RunQuery {
    #[param(value_type = String, example = "2026-09")]
    pub period: Period,
    /// `all` (the default) or `approved`.
    pub status: Option<StatusView>,
    /// Show the report in another currency, such as `USD`.
    pub currency: Option<String>,
    /// `money` (the default) or `share`.
    pub view: Option<View>,
    /// `csv` for a CSV file.
    pub format: Option<String>,
}

/// A saved report for one period.
#[utoipa::path(
    get,
    path = "/reports/saved/{id}",
    tag = "reports",
    params(("id" = Uuid, Path, description = "The saved report"), RunQuery),
    responses(
        (status = OK, description = "The report, as JSON or CSV", content((Report = "application/json"), (String = "text/csv"))),
        (status = NOT_FOUND, description = "No such saved report", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn run_saved(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<Uuid>,
    Query(query): Query<RunQuery>,
) -> Result<Response, ApiError> {
    caller.require_credential(READERS)?;
    let saved = sqlx::query!(
        "SELECT filter, group_by FROM saved_reports WHERE id = $1 AND organization_id = $2",
        id,
        caller.organization_id,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;
    let spec = Spec {
        period: query.period,
        grouping: Grouping::parse(&saved.group_by),
        filter: Filter::from_json(&saved.filter),
        status: query.status.unwrap_or_default(),
        currency: query.currency,
        view: query.view.unwrap_or_default(),
    };
    let csv = query.format.as_deref() == Some("csv");
    let report = cost::run(&state.db, &caller, spec).await?;
    Ok(cost::respond(report, csv, "report"))
}

/// Remove a saved report. Its creator or a controller can.
#[utoipa::path(
    delete,
    path = "/reports/saved/{id}",
    tag = "reports",
    params(("id" = Uuid, Path, description = "The saved report")),
    responses(
        (status = NO_CONTENT, description = "Removed"),
        (status = NOT_FOUND, description = "No such saved report of yours", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn remove_saved(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    caller.require_credential(&[Credential::Session])?;
    let removed = sqlx::query!(
        r#"DELETE FROM saved_reports
           WHERE id = $1 AND organization_id = $2 AND (created_by = $3 OR $4)"#,
        id,
        caller.organization_id,
        caller.person_id,
        caller.role == Role::Controller,
    )
    .execute(&state.db)
    .await?
    .rows_affected();
    if removed == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}
