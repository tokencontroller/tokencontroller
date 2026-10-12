//! Links to a closed period's cost sheet for finance, who need no login
//! (docs: Report > The cost sheet, for finance). Only a hash of each link's
//! secret is stored; a controller can revoke a link at any time.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde::Serialize;
use time::OffsetDateTime;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::period::{Period, PeriodDates};
use super::{SheetQuery, parse_period, sheet_response};
use crate::AppState;
use crate::error::ApiError;
use crate::organizations::Role;
use crate::sign_in::Caller;

pub(super) fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(create_link, list_links))
        .routes(routes!(revoke_link))
        .routes(routes!(shared_cost_sheet))
}

/// A link to a cost sheet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct CostSheetLink {
    pub id: Uuid,
    #[schema(value_type = String, example = "2026-09")]
    pub period: String,
    /// The address to hand finance. Shown only when the link is made.
    pub url: Option<String>,
    pub created_by: String,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    #[schema(value_type = Option<String>, format = DateTime)]
    pub revoked_at: Option<OffsetDateTime>,
}

/// A secret of 64 hex digits, from two random UUIDs (244 random bits).
fn new_secret() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

/// Make a link that shows the cost sheet without signing in.
#[utoipa::path(
    post,
    path = "/periods/{period}/cost-sheet/links",
    tag = "close",
    params(("period" = String, Path, description = "The closed period, such as 2026-09")),
    responses(
        (status = CREATED, description = "The link, with its address", body = CostSheetLink),
        (status = CONFLICT, description = "The period is not closed", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn create_link(
    State(state): State<AppState>,
    caller: Caller,
    Path(period): Path<String>,
) -> Result<(StatusCode, Json<CostSheetLink>), ApiError> {
    caller.require_role(Role::Controller)?;
    caller.require_session()?;
    let period = parse_period(&period)?;
    let secret = new_secret();
    let row = sqlx::query!(
        r#"INSERT INTO cost_sheet_links (organization_id, period, secret_hash, created_by)
           SELECT $1, $2, encode(sha256(convert_to($3, 'UTF8')), 'hex'), $4
           WHERE EXISTS (SELECT 1 FROM closed_periods WHERE organization_id = $1 AND period = $2)
           RETURNING id, created_at"#,
        caller.organization_id,
        period.first_day(),
        secret,
        caller.person_id,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| {
        ApiError::Conflict(format!(
            "{period} is not closed yet; only a closed period has a cost sheet to share."
        ))
    })?;
    let created_by = sqlx::query_scalar!("SELECT name FROM people WHERE id = $1", caller.person_id)
        .fetch_one(&state.db)
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(CostSheetLink {
            id: row.id,
            period: period.to_string(),
            url: Some(format!(
                "{}/shared/cost-sheet/{secret}",
                state.config.public_url
            )),
            created_by,
            created_at: row.created_at,
            revoked_at: None,
        }),
    ))
}

/// The links made for a period.
#[utoipa::path(
    get,
    path = "/periods/{period}/cost-sheet/links",
    tag = "close",
    params(("period" = String, Path, description = "The period, such as 2026-09")),
    responses((status = OK, description = "The links, newest first", body = Vec<CostSheetLink>)),
)]
pub(crate) async fn list_links(
    State(state): State<AppState>,
    caller: Caller,
    Path(period): Path<String>,
) -> Result<Json<Vec<CostSheetLink>>, ApiError> {
    caller.require_role(Role::Controller)?;
    let period = parse_period(&period)?;
    let rows = sqlx::query!(
        r#"SELECT l.id, l.created_at, l.revoked_at, p.name AS created_by
           FROM cost_sheet_links l JOIN people p ON p.id = l.created_by
           WHERE l.organization_id = $1 AND l.period = $2
           ORDER BY l.created_at DESC"#,
        caller.organization_id,
        period.first_day(),
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|row| CostSheetLink {
                id: row.id,
                period: period.to_string(),
                url: None,
                created_by: row.created_by,
                created_at: row.created_at,
                revoked_at: row.revoked_at,
            })
            .collect(),
    ))
}

/// Revoke a link: it stops working at once.
#[utoipa::path(
    delete,
    path = "/cost-sheet-links/{id}",
    tag = "close",
    params(("id" = Uuid, Path, description = "The link")),
    responses(
        (status = NO_CONTENT, description = "Revoked"),
        (status = NOT_FOUND, description = "No such link", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn revoke_link(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    caller.require_role(Role::Controller)?;
    caller.require_session()?;
    let revoked = sqlx::query!(
        r#"UPDATE cost_sheet_links SET revoked_at = coalesce(revoked_at, now())
           WHERE id = $1 AND organization_id = $2"#,
        id,
        caller.organization_id,
    )
    .execute(&state.db)
    .await?
    .rows_affected();
    if revoked == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// The cost sheet behind a link, without signing in.
#[utoipa::path(
    get,
    path = "/shared/cost-sheets/{secret}",
    tag = "close",
    params(("secret" = String, Path, description = "The secret part of the link"), SheetQuery),
    responses(
        (status = OK, description = "The cost sheet, as JSON or CSV", content((super::cost_sheet::CostSheet = "application/json"), (String = "text/csv"))),
        (status = NOT_FOUND, description = "No such link, or it was revoked", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn shared_cost_sheet(
    State(state): State<AppState>,
    Path(secret): Path<String>,
    Query(query): Query<SheetQuery>,
) -> Result<Response, ApiError> {
    if secret.len() != 64 || !secret.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ApiError::NotFound);
    }
    let link = sqlx::query!(
        r#"SELECT organization_id, period FROM cost_sheet_links
           WHERE secret_hash = encode(sha256(convert_to($1, 'UTF8')), 'hex') AND revoked_at IS NULL"#,
        secret,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;
    let period = Period::of_date(link.period);
    sheet_response(&state.db, link.organization_id, period, &query).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_long_and_fresh() {
        let one = new_secret();
        assert_eq!(one.len(), 64);
        assert!(one.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(one, new_secret());
    }
}
