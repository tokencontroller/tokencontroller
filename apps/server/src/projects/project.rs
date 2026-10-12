//! Projects with their ticket key pattern, repositories and tags, and the
//! clients named on them.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use sqlx::types::Json as JsonColumn;
use time::OffsetDateTime;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::tag_value;
use crate::AppState;
use crate::change::Change;
use crate::error::ApiError;
use crate::organizations::Role;
use crate::sign_in::Member;
use crate::tags::{self, Tags};

/// The longest ticket key pattern.
const MAX_PATTERN_CHARS: usize = 200;

/// A project: a body of work made of tickets.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Project {
    pub id: Uuid,
    /// The value of the `project` tag, such as `storefront`.
    pub name: String,
    /// A regular expression for its ticket keys, such as `^KD-\d+`.
    pub ticket_key_pattern: Option<String>,
    /// Tags its token entries inherit, such as `{"client": "acme"}`.
    pub tags: Tags,
    /// Repositories whose sessions land on it, such as `kestrel/storefront`.
    pub repositories: Vec<String>,
    pub archived: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

/// A new project.
#[derive(Debug, Deserialize, ToSchema)]
pub struct NewProject {
    pub name: String,
    pub ticket_key_pattern: Option<String>,
    #[serde(default)]
    pub tags: Tags,
    #[serde(default)]
    pub repositories: Vec<String>,
}

/// A change to a project. Fields left out stay as they are. Changes reach
/// only future token entries.
#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct ProjectChange {
    pub name: Option<String>,
    /// `null` removes the pattern.
    #[serde(default)]
    #[schema(value_type = Option<String>)]
    pub ticket_key_pattern: Change<String>,
    /// Replaces the project's tags.
    pub tags: Option<Tags>,
    /// Replaces the project's repositories.
    pub repositories: Option<Vec<String>>,
}

/// Which projects to list.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ProjectFilter {
    /// Include archived projects.
    #[serde(default)]
    pub archived: bool,
}

/// A client: a value of the `client` tag on projects.
#[derive(Debug, Serialize, ToSchema)]
pub struct Client {
    /// Such as `acme`.
    pub name: String,
    /// Its projects.
    pub projects: Vec<String>,
}

/// The project a repository belongs to, if any.
///
/// # Errors
///
/// Fails if a query fails.
pub async fn project_for_repository(
    db: &PgPool,
    organization_id: Uuid,
    repository: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar!(
        r#"SELECT r.project_id FROM project_repositories r JOIN projects p ON p.id = r.project_id
           WHERE r.organization_id = $1 AND r.repository = $2 AND p.archived_at IS NULL"#,
        organization_id,
        repository.trim().to_lowercase()
    )
    .fetch_optional(db)
    .await
}

pub(super) async fn load(
    db: &PgPool,
    organization_id: Uuid,
    only: Option<Uuid>,
    archived: bool,
) -> Result<Vec<Project>, ApiError> {
    let rows = sqlx::query!(
        r#"SELECT p.id, p.name, p.ticket_key_pattern, p.tags AS "tags: JsonColumn<Tags>",
                  p.archived_at IS NOT NULL AS "archived!", p.created_at,
                  coalesce(array_agg(r.repository ORDER BY r.repository)
                           FILTER (WHERE r.repository IS NOT NULL), '{}') AS "repositories!"
           FROM projects p
           LEFT JOIN project_repositories r ON r.project_id = p.id
           WHERE p.organization_id = $1
             AND ($2::uuid IS NULL OR p.id = $2)
             AND ($3 OR p.archived_at IS NULL)
           GROUP BY p.id
           ORDER BY lower(p.name)"#,
        organization_id,
        only,
        archived
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| Project {
            id: row.id,
            name: row.name,
            ticket_key_pattern: row.ticket_key_pattern,
            tags: row.tags.0,
            repositories: row.repositories,
            archived: row.archived,
            created_at: row.created_at,
        })
        .collect())
}

async fn one(db: &PgPool, organization_id: Uuid, id: Uuid) -> Result<Project, ApiError> {
    load(db, organization_id, Some(id), true)
        .await?
        .into_iter()
        .next()
        .ok_or(ApiError::NotFound)
}

/// The organization's projects.
#[utoipa::path(
    get,
    path = "/projects",
    tag = "projects",
    params(ProjectFilter),
    responses((status = OK, body = Vec<Project>)),
)]
pub(super) async fn list_projects(
    State(state): State<AppState>,
    member: Member,
    Query(filter): Query<ProjectFilter>,
) -> Result<Json<Vec<Project>>, ApiError> {
    Ok(Json(
        load(&state.db, member.organization_id, None, filter.archived).await?,
    ))
}

/// Create a project. People managers and controllers.
#[utoipa::path(
    post,
    path = "/projects",
    tag = "projects",
    request_body = NewProject,
    responses(
        (status = CREATED, body = Project),
        (status = BAD_REQUEST, body = crate::error::ErrorBody),
        (status = CONFLICT, description = "The name or a repository is taken", body = crate::error::ErrorBody),
    ),
)]
pub(super) async fn create_project(
    State(state): State<AppState>,
    member: Member,
    Json(request): Json<NewProject>,
) -> Result<(StatusCode, Json<Project>), ApiError> {
    member.require_session()?;
    member.require_role(Role::PeopleManager)?;
    let name = tag_value("project name", &request.name)?;
    let pattern = request
        .ticket_key_pattern
        .as_deref()
        .map(check_pattern)
        .transpose()?;
    let tags = tags::validate(&state.db, member.organization_id, &request.tags, false).await?;
    let repositories = check_repositories(&request.repositories)?;

    let mut tx = state.db.begin().await?;
    let id = sqlx::query_scalar!(
        r#"INSERT INTO projects (organization_id, name, ticket_key_pattern, tags)
           VALUES ($1, $2, $3, $4)
           ON CONFLICT (organization_id, name) DO NOTHING
           RETURNING id"#,
        member.organization_id,
        name,
        pattern,
        JsonColumn(&tags) as _
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| ApiError::Conflict(format!("There is already a project named {name:?}.")))?;
    set_repositories(&mut tx, member.organization_id, id, &repositories).await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(one(&state.db, member.organization_id, id).await?),
    ))
}

/// Change a project. People managers and controllers.
#[utoipa::path(
    patch,
    path = "/projects/{id}",
    tag = "projects",
    params(("id" = Uuid, Path)),
    request_body = ProjectChange,
    responses(
        (status = OK, body = Project),
        (status = BAD_REQUEST, body = crate::error::ErrorBody),
        (status = CONFLICT, body = crate::error::ErrorBody),
        (status = NOT_FOUND, body = crate::error::ErrorBody),
    ),
)]
pub(super) async fn update_project(
    State(state): State<AppState>,
    member: Member,
    Path(id): Path<Uuid>,
    Json(change): Json<ProjectChange>,
) -> Result<Json<Project>, ApiError> {
    member.require_session()?;
    member.require_role(Role::PeopleManager)?;
    one(&state.db, member.organization_id, id).await?;
    let name = change
        .name
        .as_deref()
        .map(|name| tag_value("project name", name))
        .transpose()?;
    let set_pattern = change.ticket_key_pattern.is_given();
    let pattern = change
        .ticket_key_pattern
        .into_value()
        .as_deref()
        .map(check_pattern)
        .transpose()?;
    let tags = match &change.tags {
        Some(tags) => Some(tags::validate(&state.db, member.organization_id, tags, false).await?),
        None => None,
    };
    let repositories = change
        .repositories
        .as_deref()
        .map(check_repositories)
        .transpose()?;

    let mut tx = state.db.begin().await?;
    let updated = sqlx::query!(
        r#"UPDATE projects SET
             name = coalesce($3, name),
             ticket_key_pattern = CASE WHEN $4 THEN $5 ELSE ticket_key_pattern END,
             tags = coalesce($6, tags)
           WHERE id = $1 AND organization_id = $2
             AND NOT EXISTS (SELECT 1 FROM projects o WHERE o.organization_id = $2 AND o.name = $3 AND o.id <> $1)"#,
        id,
        member.organization_id,
        name,
        set_pattern,
        pattern,
        tags.map(JsonColumn) as _
    )
    .execute(&mut *tx)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(ApiError::Conflict(
            "There is already a project with that name.".to_owned(),
        ));
    }
    if let Some(repositories) = repositories {
        set_repositories(&mut tx, member.organization_id, id, &repositories).await?;
    }
    tx.commit().await?;
    Ok(Json(one(&state.db, member.organization_id, id).await?))
}

/// Archive a project. Its tickets and past token entries stay; its
/// repositories are freed for other projects. People managers and
/// controllers.
#[utoipa::path(
    delete,
    path = "/projects/{id}",
    tag = "projects",
    params(("id" = Uuid, Path)),
    responses(
        (status = NO_CONTENT, description = "Archived"),
        (status = NOT_FOUND, body = crate::error::ErrorBody),
    ),
)]
pub(super) async fn archive_project(
    State(state): State<AppState>,
    member: Member,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    member.require_session()?;
    member.require_role(Role::PeopleManager)?;
    let mut tx = state.db.begin().await?;
    let archived = sqlx::query!(
        r#"UPDATE projects SET archived_at = coalesce(archived_at, now())
           WHERE id = $1 AND organization_id = $2"#,
        id,
        member.organization_id
    )
    .execute(&mut *tx)
    .await?;
    if archived.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    sqlx::query!("DELETE FROM project_repositories WHERE project_id = $1", id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The clients: values of the `client` tag on projects, with their
/// projects. Cost per client is a filter on this tag.
#[utoipa::path(
    get,
    path = "/clients",
    tag = "projects",
    responses((status = OK, body = Vec<Client>)),
)]
pub(super) async fn list_clients(
    State(state): State<AppState>,
    member: Member,
) -> Result<Json<Vec<Client>>, ApiError> {
    let projects = load(&state.db, member.organization_id, None, false).await?;
    let mut clients: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for project in projects {
        if let Some(client) = project.tags.get("client") {
            clients
                .entry(client.clone())
                .or_default()
                .push(project.name);
        }
    }
    Ok(Json(
        clients
            .into_iter()
            .map(|(name, projects)| Client { name, projects })
            .collect(),
    ))
}

async fn set_repositories(
    tx: &mut sqlx::PgConnection,
    organization_id: Uuid,
    project_id: Uuid,
    repositories: &[String],
) -> Result<(), ApiError> {
    sqlx::query!(
        "DELETE FROM project_repositories WHERE project_id = $1",
        project_id
    )
    .execute(&mut *tx)
    .await?;
    for repository in repositories {
        let taken_by = sqlx::query_scalar!(
            r#"SELECT p.name FROM project_repositories r JOIN projects p ON p.id = r.project_id
               WHERE r.organization_id = $1 AND r.repository = $2"#,
            organization_id,
            repository
        )
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(project) = taken_by {
            return Err(ApiError::Conflict(format!(
                "{repository} belongs to the project {project}. One repository belongs to one project."
            )));
        }
        sqlx::query!(
            "INSERT INTO project_repositories (organization_id, repository, project_id) VALUES ($1, $2, $3)",
            organization_id,
            repository,
            project_id
        )
        .execute(&mut *tx)
        .await?;
    }
    Ok(())
}

/// Checks a ticket key pattern: a regular expression of reasonable size.
fn check_pattern(pattern: &str) -> Result<String, ApiError> {
    let pattern = pattern.trim();
    if pattern.is_empty() || pattern.chars().count() > MAX_PATTERN_CHARS {
        return Err(ApiError::BadRequest(format!(
            "The ticket key pattern must have 1 to {MAX_PATTERN_CHARS} characters."
        )));
    }
    regex::RegexBuilder::new(pattern)
        .size_limit(1 << 16)
        .build()
        .map_err(|error| {
            ApiError::BadRequest(format!(
                "The ticket key pattern is not a regular expression: {error}"
            ))
        })?;
    Ok(pattern.to_owned())
}

/// Checks and normalizes repositories, such as `Kestrel/Storefront` to
/// `kestrel/storefront`.
fn check_repositories(repositories: &[String]) -> Result<Vec<String>, ApiError> {
    let mut checked: Vec<String> = Vec::new();
    for repository in repositories {
        let repository = repository
            .trim()
            .trim_end_matches(".git")
            .trim_end_matches('/')
            .to_lowercase();
        let parts: Vec<&str> = repository.split('/').collect();
        let valid = parts.len() >= 2
            && parts.len() <= 4
            && parts
                .iter()
                .all(|part| !part.is_empty() && !part.chars().any(char::is_whitespace));
        if !valid {
            return Err(ApiError::BadRequest(format!(
                "{repository:?} is not a repository; use owner/name, such as kestrel/storefront."
            )));
        }
        if !checked.contains(&repository) {
            checked.push(repository);
        }
    }
    Ok(checked)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_must_be_regular_expressions() {
        assert_eq!(check_pattern(r" ^KD-\d+ ").unwrap(), r"^KD-\d+");
        assert!(check_pattern(r"storefront#\d+").is_ok());
        assert!(check_pattern("KD-(").is_err());
        assert!(check_pattern("").is_err());
    }

    #[test]
    fn repositories_are_owner_and_name() {
        let given = vec![
            "Kestrel/Storefront.git".to_owned(),
            "kestrel/storefront".to_owned(),
            "kestrel/claims/portal".to_owned(),
        ];
        assert_eq!(
            check_repositories(&given).unwrap(),
            vec!["kestrel/storefront", "kestrel/claims/portal"]
        );
        for bad in [
            "storefront",
            "kestrel/",
            "/storefront",
            "kes trel/x",
            "a/b/c/d/e",
        ] {
            assert!(check_repositories(&[bad.to_owned()]).is_err(), "{bad}");
        }
    }
}
