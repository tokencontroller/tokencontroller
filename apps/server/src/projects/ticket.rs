//! Tickets: synced from a ticket system or created by hand. Only the key is
//! required. Tickets can sit inside other tickets.

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

/// The most tickets one page lists.
const MAX_PAGE: i64 = 500;

/// Where a ticket comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TicketSource {
    /// Created in the web app, for work no ticket system knows.
    Manual,
    Jira,
    AzureDevops,
    GithubIssues,
}

impl TicketSource {
    fn from_db(source: &str) -> Self {
        match source {
            "jira" => Self::Jira,
            "azure_devops" => Self::AzureDevops,
            "github_issues" => Self::GithubIssues,
            _ => Self::Manual,
        }
    }
}

/// A ticket: any unit of work.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Ticket {
    pub id: Uuid,
    /// The value of the `ticket` tag, such as `KD-214`.
    pub key: String,
    pub project_id: Option<Uuid>,
    pub title: Option<String>,
    pub description: Option<String>,
    /// The ticket it sits in, such as a story's epic.
    pub parent_id: Option<Uuid>,
    /// The person it is assigned to. Being assignee costs nothing.
    pub assignee_id: Option<Uuid>,
    pub source: TicketSource,
    /// Tags its token entries inherit, beyond its project's.
    pub tags: Tags,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

/// A ticket created by hand.
#[derive(Debug, Deserialize, ToSchema)]
pub struct NewTicket {
    pub key: String,
    /// Without it, the project whose ticket key pattern matches the key.
    pub project_id: Option<Uuid>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub parent_id: Option<Uuid>,
    pub assignee_id: Option<Uuid>,
    #[serde(default)]
    pub tags: Tags,
}

/// A change to a ticket. Fields left out stay as they are; `null` clears.
/// The title and description of a synced ticket come from its ticket system.
#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct TicketChange {
    #[serde(default)]
    #[schema(value_type = Option<Uuid>)]
    pub project_id: Change<Uuid>,
    #[serde(default)]
    #[schema(value_type = Option<String>)]
    pub title: Change<String>,
    #[serde(default)]
    #[schema(value_type = Option<String>)]
    pub description: Change<String>,
    #[serde(default)]
    #[schema(value_type = Option<Uuid>)]
    pub parent_id: Change<Uuid>,
    #[serde(default)]
    #[schema(value_type = Option<Uuid>)]
    pub assignee_id: Change<Uuid>,
    /// Replaces the ticket's tags.
    pub tags: Option<Tags>,
}

/// Which tickets to list.
#[derive(Debug, Deserialize, IntoParams)]
pub struct TicketFilter {
    pub project_id: Option<Uuid>,
    pub parent_id: Option<Uuid>,
    /// Part of the key or the title.
    pub q: Option<String>,
    /// At most 500; default 100.
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: i64,
}

/// The project whose ticket key pattern matches `key`, if exactly one does.
///
/// # Errors
///
/// Fails if a query fails.
pub async fn project_for_key(
    db: &PgPool,
    organization_id: Uuid,
    key: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    let projects = sqlx::query!(
        r#"SELECT id, ticket_key_pattern AS "pattern!" FROM projects
           WHERE organization_id = $1 AND archived_at IS NULL AND ticket_key_pattern IS NOT NULL"#,
        organization_id
    )
    .fetch_all(db)
    .await?;
    let mut matching = projects.into_iter().filter(|project| {
        regex::RegexBuilder::new(&project.pattern)
            .size_limit(1 << 16)
            .build()
            .is_ok_and(|pattern| pattern.is_match(key))
    });
    Ok(match (matching.next(), matching.next()) {
        (Some(project), None) => Some(project.id),
        _ => None,
    })
}

pub(super) async fn load(
    db: &PgPool,
    organization_id: Uuid,
    only: Option<Uuid>,
    filter: &TicketFilter,
) -> Result<Vec<Ticket>, ApiError> {
    let limit = filter.limit.unwrap_or(100).clamp(1, MAX_PAGE);
    let search = filter
        .q
        .as_deref()
        .map(str::trim)
        .filter(|q| !q.is_empty())
        .map(|q| {
            format!(
                "%{}%",
                q.replace('\\', "\\\\")
                    .replace('%', "\\%")
                    .replace('_', "\\_")
            )
        });
    let rows = sqlx::query!(
        r#"SELECT id, key, project_id, title, description, parent_id, assignee_id, source,
                  tags AS "tags: JsonColumn<Tags>", updated_at
           FROM tickets
           WHERE organization_id = $1
             AND ($2::uuid IS NULL OR id = $2)
             AND ($3::uuid IS NULL OR project_id = $3)
             AND ($4::uuid IS NULL OR parent_id = $4)
             AND ($5::text IS NULL OR key ILIKE $5 OR title ILIKE $5)
           ORDER BY key
           LIMIT $6 OFFSET $7"#,
        organization_id,
        only,
        filter.project_id,
        filter.parent_id,
        search,
        limit,
        filter.offset.max(0)
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| Ticket {
            id: row.id,
            key: row.key,
            project_id: row.project_id,
            title: row.title,
            description: row.description,
            parent_id: row.parent_id,
            assignee_id: row.assignee_id,
            source: TicketSource::from_db(&row.source),
            tags: row.tags.0,
            updated_at: row.updated_at,
        })
        .collect())
}

async fn one(db: &PgPool, organization_id: Uuid, id: Uuid) -> Result<Ticket, ApiError> {
    let everything = TicketFilter {
        project_id: None,
        parent_id: None,
        q: None,
        limit: Some(1),
        offset: 0,
    };
    load(db, organization_id, Some(id), &everything)
        .await?
        .into_iter()
        .next()
        .ok_or(ApiError::NotFound)
}

/// The organization's tickets, by key.
#[utoipa::path(
    get,
    path = "/tickets",
    tag = "tickets",
    params(TicketFilter),
    responses((status = OK, body = Vec<Ticket>)),
)]
pub(super) async fn list_tickets(
    State(state): State<AppState>,
    member: Member,
    Query(filter): Query<TicketFilter>,
) -> Result<Json<Vec<Ticket>>, ApiError> {
    Ok(Json(
        load(&state.db, member.organization_id, None, &filter).await?,
    ))
}

/// One ticket.
#[utoipa::path(
    get,
    path = "/tickets/{id}",
    tag = "tickets",
    params(("id" = Uuid, Path)),
    responses(
        (status = OK, body = Ticket),
        (status = NOT_FOUND, body = crate::error::ErrorBody),
    ),
)]
pub(super) async fn get_ticket(
    State(state): State<AppState>,
    member: Member,
    Path(id): Path<Uuid>,
) -> Result<Json<Ticket>, ApiError> {
    Ok(Json(one(&state.db, member.organization_id, id).await?))
}

/// Create a ticket by hand, for work no ticket system knows. Any member.
#[utoipa::path(
    post,
    path = "/tickets",
    tag = "tickets",
    request_body = NewTicket,
    responses(
        (status = CREATED, body = Ticket),
        (status = BAD_REQUEST, body = crate::error::ErrorBody),
        (status = CONFLICT, description = "A ticket with this key exists", body = crate::error::ErrorBody),
    ),
)]
pub(super) async fn create_ticket(
    State(state): State<AppState>,
    member: Member,
    Json(request): Json<NewTicket>,
) -> Result<(StatusCode, Json<Ticket>), ApiError> {
    member.require_session()?;
    let key = tag_value("ticket key", &request.key)?;
    let tags = tags::validate(&state.db, member.organization_id, &request.tags, false).await?;
    let project_id = match request.project_id {
        Some(id) => Some(id),
        None => super::project_for_key(&state.db, member.organization_id, &key).await?,
    };
    let references = References {
        project: project_id,
        parent: request.parent_id,
        assignee: request.assignee_id,
    };
    check_references(&state.db, member.organization_id, None, &references).await?;
    let id = sqlx::query_scalar!(
        r#"INSERT INTO tickets (organization_id, key, project_id, title, description, parent_id, assignee_id, tags)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
           ON CONFLICT (organization_id, key) DO NOTHING
           RETURNING id"#,
        member.organization_id,
        key,
        project_id,
        clean_text(request.title.as_deref()),
        clean_text(request.description.as_deref()),
        request.parent_id,
        request.assignee_id,
        JsonColumn(&tags) as _
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| ApiError::Conflict(format!("There is already a ticket {key}.")))?;
    Ok((
        StatusCode::CREATED,
        Json(one(&state.db, member.organization_id, id).await?),
    ))
}

/// Change a ticket. People managers and controllers; for tickets created by
/// hand, any member.
#[utoipa::path(
    patch,
    path = "/tickets/{id}",
    tag = "tickets",
    params(("id" = Uuid, Path)),
    request_body = TicketChange,
    responses(
        (status = OK, body = Ticket),
        (status = BAD_REQUEST, body = crate::error::ErrorBody),
        (status = NOT_FOUND, body = crate::error::ErrorBody),
    ),
)]
pub(super) async fn update_ticket(
    State(state): State<AppState>,
    member: Member,
    Path(id): Path<Uuid>,
    Json(change): Json<TicketChange>,
) -> Result<Json<Ticket>, ApiError> {
    member.require_session()?;
    let current = one(&state.db, member.organization_id, id).await?;
    if current.source != TicketSource::Manual {
        member.require_role(Role::PeopleManager)?;
        if change.title.is_given() || change.description.is_given() {
            return Err(ApiError::BadRequest(
                "The title and description of a synced ticket come from its ticket system."
                    .to_owned(),
            ));
        }
    }
    let tags = match &change.tags {
        Some(tags) => Some(tags::validate(&state.db, member.organization_id, tags, false).await?),
        None => None,
    };
    let references = References {
        project: pick(change.project_id.clone(), current.project_id),
        parent: pick(change.parent_id.clone(), current.parent_id),
        assignee: pick(change.assignee_id.clone(), current.assignee_id),
    };
    check_references(&state.db, member.organization_id, Some(id), &references).await?;
    let title = pick(
        change.title.map(|title| title.trim().to_owned()),
        current.title,
    );
    let description = pick(change.description, current.description);
    sqlx::query!(
        r#"UPDATE tickets SET project_id = $3, title = $4, description = $5, parent_id = $6,
             assignee_id = $7, tags = coalesce($8, tags), updated_at = now()
           WHERE id = $1 AND organization_id = $2"#,
        id,
        member.organization_id,
        references.project,
        clean_text(title.as_deref()),
        clean_text(description.as_deref()),
        references.parent,
        references.assignee,
        tags.map(JsonColumn) as _
    )
    .execute(&state.db)
    .await?;
    Ok(Json(one(&state.db, member.organization_id, id).await?))
}

fn pick<T>(change: Change<T>, current: Option<T>) -> Option<T> {
    match change {
        Change::Keep => current,
        Change::Clear => None,
        Change::Set(value) => Some(value),
    }
}

fn clean_text(text: Option<&str>) -> Option<String> {
    text.map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

struct References {
    project: Option<Uuid>,
    parent: Option<Uuid>,
    assignee: Option<Uuid>,
}

/// The project, parent and assignee must be in the organization, and a
/// ticket can never sit inside itself.
async fn check_references(
    db: &PgPool,
    organization_id: Uuid,
    ticket_id: Option<Uuid>,
    references: &References,
) -> Result<(), ApiError> {
    let row = sqlx::query!(
        r#"SELECT
             ($2::uuid IS NULL OR EXISTS (SELECT 1 FROM projects WHERE id = $2 AND organization_id = $1)) AS "project!",
             ($3::uuid IS NULL OR EXISTS (SELECT 1 FROM tickets WHERE id = $3 AND organization_id = $1)) AS "parent!",
             ($4::uuid IS NULL OR EXISTS (SELECT 1 FROM people WHERE id = $4 AND organization_id = $1 AND removed_at IS NULL)) AS "assignee!",
             ($5::uuid IS NOT NULL AND $3::uuid IS NOT NULL AND EXISTS (
                WITH RECURSIVE up(id, parent_id) AS (
                    SELECT id, parent_id FROM tickets WHERE id = $3
                    UNION
                    SELECT t.id, t.parent_id FROM tickets t JOIN up ON t.id = up.parent_id
                )
                SELECT 1 FROM up WHERE id = $5
             )) AS "cycle!""#,
        organization_id,
        references.project,
        references.parent,
        references.assignee,
        ticket_id
    )
    .fetch_one(db)
    .await?;
    if !row.project {
        return Err(ApiError::BadRequest("No such project.".to_owned()));
    }
    if !row.parent {
        return Err(ApiError::BadRequest("No such parent ticket.".to_owned()));
    }
    if !row.assignee {
        return Err(ApiError::BadRequest(
            "The assignee must be a member.".to_owned(),
        ));
    }
    if row.cycle {
        return Err(ApiError::BadRequest(
            "A ticket cannot sit inside itself.".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_keeps_clears_or_sets() {
        assert_eq!(pick(Change::Keep, Some(1)), Some(1));
        assert_eq!(pick(Change::Clear, Some(1)), None);
        assert_eq!(pick(Change::Set(2), Some(1)), Some(2));
    }

    #[test]
    fn empty_text_is_no_text() {
        assert_eq!(clean_text(Some("  ")), None);
        assert_eq!(clean_text(Some(" Fix it ")), Some("Fix it".to_owned()));
        assert_eq!(clean_text(None), None);
    }

    #[test]
    fn synced_sources_read_back() {
        assert_eq!(TicketSource::from_db("jira"), TicketSource::Jira);
        assert_eq!(
            TicketSource::from_db("github_issues"),
            TicketSource::GithubIssues
        );
        assert_eq!(TicketSource::from_db("manual"), TicketSource::Manual);
    }
}
