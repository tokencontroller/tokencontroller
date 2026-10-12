//! Setting a cadence for the whole organization or one project (docs:
//! Account > Cadence and token sheets). A person's cadence is the shortest
//! of the organization's and those of the projects they reported on lately.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::{Date, Duration, OffsetDateTime};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::cadence::{Cadence, CadenceKind, CadencePeriod};
use crate::AppState;
use crate::error::ApiError;
use crate::organizations::Role;
use crate::reports::filter::Filter;
use crate::reports::scope::Scope;
use crate::sign_in::{Caller, Credential};

/// How far back a person's token sheets decide which project cadences
/// apply to them.
const RECENT_DAYS: i64 = 90;

pub(super) fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_cadences, set_cadence))
        .routes(routes!(remove_cadence))
}

/// A cadence as the API shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct CadenceView {
    pub id: Uuid,
    /// The project it is set for; `null` for the whole organization.
    pub project: Option<String>,
    pub kind: CadenceKind,
    /// For a custom cadence: the length of each cadence period in days.
    pub every_days: Option<u16>,
    /// The day the first cadence period starts.
    #[schema(value_type = String, format = Date)]
    pub starts_on: Date,
    /// The cadence period running today.
    pub current: CadencePeriod,
    pub set_by: String,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub set_at: OffsetDateTime,
}

/// A cadence to set.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct CadenceInput {
    /// The project's name; left out for the whole organization.
    pub project: Option<String>,
    pub kind: CadenceKind,
    /// For a custom cadence: the length of each cadence period in days,
    /// from 1 to 366.
    pub every_days: Option<u16>,
    /// The day the first cadence period starts. Left out: the Monday of
    /// this week.
    #[schema(value_type = Option<String>, format = Date)]
    pub starts_on: Option<Date>,
}

struct CadenceRow {
    id: Uuid,
    project: Option<String>,
    kind: String,
    every_days: Option<i32>,
    starts_on: Date,
    set_by: String,
    set_at: OffsetDateTime,
}

fn cadence_of_row(kind: &str, every_days: Option<i32>, starts_on: Date) -> Option<Cadence> {
    let kind = CadenceKind::parse(kind)?;
    let days = every_days.and_then(|days| u16::try_from(days).ok());
    Cadence::new(kind, days, starts_on).ok()
}

fn view(row: CadenceRow, today: Date) -> Option<CadenceView> {
    let cadence = cadence_of_row(&row.kind, row.every_days, row.starts_on)?;
    Some(CadenceView {
        id: row.id,
        project: row.project,
        kind: cadence.kind(),
        every_days: if cadence.kind() == CadenceKind::Custom {
            cadence.days()
        } else {
            None
        },
        starts_on: row.starts_on,
        current: cadence.period_containing(today).ok()?,
        set_by: row.set_by,
        set_at: row.set_at,
    })
}

async fn all(db: &PgPool, organization_id: Uuid) -> Result<Vec<CadenceView>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"SELECT c.id, pr.name AS "project?", c.kind, c.every_days, c.starts_on, p.name AS set_by, c.set_at
           FROM cadences c
           JOIN people p ON p.id = c.set_by
           LEFT JOIN projects pr ON pr.id = c.project_id
           WHERE c.organization_id = $1
           ORDER BY pr.name NULLS FIRST"#,
        organization_id,
    )
    .fetch_all(db)
    .await?;
    let today = OffsetDateTime::now_utc().date();
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            view(
                CadenceRow {
                    id: row.id,
                    project: row.project,
                    kind: row.kind,
                    every_days: row.every_days,
                    starts_on: row.starts_on,
                    set_by: row.set_by,
                    set_at: row.set_at,
                },
                today,
            )
        })
        .collect())
}

/// The cadence that applies to a person: the shortest of the
/// organization's and the cadences of projects they put on a token sheet in
/// the last 90 days. `None` when no cadence is set.
///
/// # Errors
///
/// A database error.
pub async fn cadence_for(
    db: &PgPool,
    organization_id: Uuid,
    person_id: Uuid,
    today: Date,
) -> Result<Option<Cadence>, sqlx::Error> {
    let since = today.checked_sub(Duration::days(RECENT_DAYS)).unwrap_or(today);
    let rows = sqlx::query!(
        r#"SELECT c.kind, c.every_days, c.starts_on FROM cadences c
           WHERE c.organization_id = $1 AND (
               c.project_id IS NULL OR EXISTS (
                   SELECT 1
                   FROM token_sheets ts
                   JOIN token_sheet_entries tse ON tse.token_sheet_id = ts.id
                   JOIN splits s ON s.token_entry_id = tse.token_entry_id
                   JOIN split_tags t ON t.split_id = s.id AND t.name = 'project'
                   JOIN projects pr ON pr.id = c.project_id AND pr.name = t.value
                   WHERE ts.person_id = $2 AND ts.ends_on >= $3
               )
           )"#,
        organization_id,
        person_id,
        since,
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|row| cadence_of_row(&row.kind, row.every_days, row.starts_on))
        .min_by_key(|cadence| cadence.days().unwrap_or(31)))
}

/// The cadences of the organization and its projects.
#[utoipa::path(
    get,
    path = "/cadences",
    tag = "token_sheets",
    responses((status = OK, description = "The cadences", body = Vec<CadenceView>)),
)]
pub(crate) async fn list_cadences(
    State(state): State<AppState>,
    caller: Caller,
) -> Result<Json<Vec<CadenceView>>, ApiError> {
    Ok(Json(all(&state.db, caller.organization_id).await?))
}

fn monday_of(day: Date) -> Date {
    let back = i64::from(day.weekday().number_days_from_monday());
    day.checked_sub(Duration::days(back)).unwrap_or(day)
}

/// Set the cadence of the organization or of one project. A people manager
/// sets a project's cadence within the filters they are assigned.
#[utoipa::path(
    put,
    path = "/cadences",
    tag = "token_sheets",
    request_body = CadenceInput,
    responses(
        (status = OK, description = "The cadence in force", body = CadenceView),
        (status = BAD_REQUEST, description = "The cadence is not valid", body = crate::error::ErrorBody),
        (status = FORBIDDEN, description = "Outside the filters you are assigned", body = crate::error::ErrorBody),
        (status = NOT_FOUND, description = "No such project", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn set_cadence(
    State(state): State<AppState>,
    caller: Caller,
    Json(input): Json<CadenceInput>,
) -> Result<Json<CadenceView>, ApiError> {
    caller.require_role(Role::PeopleManager)?;
    caller.require_credential(&[Credential::Session])?;
    let today = OffsetDateTime::now_utc().date();
    let starts_on = input.starts_on.unwrap_or_else(|| monday_of(today));
    let cadence = Cadence::new(input.kind, input.every_days, starts_on)
        .map_err(|error| ApiError::BadRequest(error.to_string()))?;
    let project_id = match input.project.as_deref() {
        Some(name) => {
            let scope = Scope::of(&state.db, &caller).await?;
            let filter = Filter(std::iter::once(("project".to_owned(), name.to_owned())).collect());
            if !scope.covers(&filter) {
                return Err(ApiError::Forbidden);
            }
            Some(
                sqlx::query_scalar!(
                    "SELECT id FROM projects WHERE organization_id = $1 AND name = $2",
                    caller.organization_id,
                    name,
                )
                .fetch_optional(&state.db)
                .await?
                .ok_or(ApiError::NotFound)?,
            )
        }
        None => None,
    };
    let every_days = match cadence.kind() {
        CadenceKind::Custom => cadence.days().map(i32::from),
        _ => None,
    };
    let id = sqlx::query_scalar!(
        r#"INSERT INTO cadences (organization_id, project_id, kind, every_days, starts_on, set_by)
           VALUES ($1, $2, $3, $4, $5, $6)
           ON CONFLICT (organization_id, project_id) DO UPDATE
               SET kind = EXCLUDED.kind, every_days = EXCLUDED.every_days,
                   starts_on = EXCLUDED.starts_on, set_by = EXCLUDED.set_by, set_at = now()
           RETURNING id"#,
        caller.organization_id,
        project_id,
        cadence.kind().as_str(),
        every_days,
        starts_on,
        caller.person_id,
    )
    .fetch_one(&state.db)
    .await?;
    all(&state.db, caller.organization_id)
        .await?
        .into_iter()
        .find(|view| view.id == id)
        .map(Json)
        .ok_or(ApiError::NotFound)
}

/// Remove a cadence: its token sheets are no longer due.
#[utoipa::path(
    delete,
    path = "/cadences/{id}",
    tag = "token_sheets",
    params(("id" = Uuid, Path, description = "The cadence")),
    responses(
        (status = NO_CONTENT, description = "Removed"),
        (status = NOT_FOUND, description = "No such cadence", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn remove_cadence(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    caller.require_role(Role::PeopleManager)?;
    caller.require_credential(&[Credential::Session])?;
    let project = sqlx::query!(
        r#"SELECT pr.name AS "project?" FROM cadences c LEFT JOIN projects pr ON pr.id = c.project_id
           WHERE c.id = $1 AND c.organization_id = $2"#,
        id,
        caller.organization_id,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;
    if let Some(name) = project.project {
        let scope = Scope::of(&state.db, &caller).await?;
        if !scope.covers(&Filter(std::iter::once(("project".to_owned(), name)).collect())) {
            return Err(ApiError::Forbidden);
        }
    }
    sqlx::query!("DELETE FROM cadences WHERE id = $1", id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn a_new_cadence_starts_on_monday() {
        assert_eq!(monday_of(date!(2026 - 10 - 12)), date!(2026 - 10 - 12));
        assert_eq!(monday_of(date!(2026 - 10 - 18)), date!(2026 - 10 - 12));
        assert_eq!(monday_of(date!(2026 - 10 - 14)), date!(2026 - 10 - 12));
    }

    #[test]
    fn rows_with_unknown_kinds_are_skipped() {
        assert!(cadence_of_row("daily", None, date!(2026 - 10 - 12)).is_none());
        assert!(cadence_of_row("custom", None, date!(2026 - 10 - 12)).is_none());
        assert!(cadence_of_row("custom", Some(10), date!(2026 - 10 - 12)).is_some());
        assert!(cadence_of_row("weekly", None, date!(2026 - 10 - 12)).is_some());
    }
}
