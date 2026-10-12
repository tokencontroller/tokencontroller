//! Ticket systems an organization connects, and the sync that keeps their
//! tickets current: every 15 minutes per ticket system, or when someone asks
//! (docs: Integrations > Jira Cloud, Azure DevOps, GitHub Issues).
//!
//! A controller connects a system with an API token, then maps each of its
//! projects to a Token Controller project. The `ticket-sync` job fetches the
//! tickets that changed since the last sync of each mapped project and
//! writes them to `tickets`, with their parents and assignees. Token entries
//! that named a ticket before it synced are placed again afterwards, so the
//! ticket fills in.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use anyhow::Context as _;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::tickets::{
    self, Connection, Credentials, ExternalProject, SyncError, SyncedTicket, TicketSystem,
};
use super::{ConnectionStatus, secrets};
use crate::AppState;
use crate::error::{ApiError, ErrorBody};
use crate::organizations::Role;
use crate::sign_in::{Caller, Member};

/// The job that syncs tickets.
pub const JOB: &str = "ticket-sync";

/// How often the job looks for ticket systems that are due. A sync someone
/// asks for starts at the runner's next tick instead.
pub const JOB_EVERY: Duration = Duration::from_mins(5);

/// How often each ticket system syncs, in minutes.
const SYNC_EVERY_MINUTES: i32 = 15;

/// Ticket systems one run syncs at most; the rest wait for the next run.
const PER_RUN: i64 = 20;

pub(super) fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_ticket_systems, connect_ticket_system))
        .routes(routes!(disconnect_ticket_system))
        .routes(routes!(external_projects))
        .routes(routes!(set_mappings))
        .routes(routes!(sync_now))
}

/// A connected ticket system, as the web app shows it. The token is never
/// sent back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TicketSystemView {
    pub id: Uuid,
    pub kind: TicketSystem,
    /// The Jira Cloud site, the Azure DevOps organization or the GitHub API.
    pub site: String,
    /// The token's last characters, to tell tokens apart.
    pub key_hint: String,
    /// Which of its projects feed which Token Controller project.
    pub mappings: Vec<Mapping>,
    /// When the last sync succeeded.
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_sync_at: Option<OffsetDateTime>,
    pub status: ConnectionStatus,
    /// Why the last sync failed.
    pub message: Option<String>,
}

/// One project of a ticket system mapped to a Token Controller project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Mapping {
    /// The ticket system's project: the Jira project key, the Azure DevOps
    /// project ID or the GitHub repository as `owner/name`, as
    /// `GET /ticket-systems/{id}/projects` lists them.
    pub external: String,
    /// Its name. Optional when mapping; the key stands in.
    #[serde(default)]
    pub external_name: Option<String>,
    /// The Token Controller project, by name.
    pub project: String,
}

/// A ticket system to connect.
#[derive(Deserialize, ToSchema)]
pub struct NewTicketSystem {
    pub kind: TicketSystem,
    /// The Jira Cloud site, such as `https://acme.atlassian.net`, the Azure
    /// DevOps organization, such as `https://dev.azure.com/acme`, or
    /// `https://api.github.com`.
    pub site: String,
    /// An API token from the ticket system.
    pub token: String,
    /// For Jira Cloud: the email of the Atlassian account the token belongs
    /// to. Jira Cloud signs in with both.
    #[serde(default)]
    pub email: Option<String>,
}

/// What is stored, sealed, for a ticket system.
#[derive(Serialize, Deserialize)]
struct StoredCredentials {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    email: Option<String>,
    token: String,
}

impl From<StoredCredentials> for Credentials {
    fn from(stored: StoredCredentials) -> Self {
        match stored.email {
            Some(email) => Self::EmailAndToken {
                email,
                token: stored.token,
            },
            None => Self::Token(stored.token),
        }
    }
}

async fn views(
    db: &PgPool,
    organization_id: Uuid,
    only: Option<Uuid>,
) -> Result<Vec<TicketSystemView>, sqlx::Error> {
    let systems = sqlx::query!(
        "SELECT id, kind, site, key_hint, last_sync_at, last_error FROM ticket_systems
         WHERE organization_id = $1 AND ($2::uuid IS NULL OR id = $2)
         ORDER BY created_at, id",
        organization_id,
        only
    )
    .fetch_all(db)
    .await?;
    let mappings = sqlx::query!(
        "SELECT m.ticket_system_id, m.external_project, m.external_name, p.name AS project
         FROM ticket_system_projects m JOIN projects p ON p.id = m.project_id
         WHERE m.organization_id = $1
         ORDER BY m.external_name, m.external_project",
        organization_id
    )
    .fetch_all(db)
    .await?;
    Ok(systems
        .into_iter()
        .filter_map(|system| {
            Some(TicketSystemView {
                id: system.id,
                kind: TicketSystem::from_name(&system.kind)?,
                site: system.site,
                key_hint: system.key_hint,
                mappings: mappings
                    .iter()
                    .filter(|mapping| mapping.ticket_system_id == system.id)
                    .map(|mapping| Mapping {
                        external: mapping.external_project.clone(),
                        external_name: Some(mapping.external_name.clone()),
                        project: mapping.project.clone(),
                    })
                    .collect(),
                last_sync_at: system.last_sync_at,
                status: ConnectionStatus::of(system.last_error.as_ref()),
                message: system.last_error,
            })
        })
        .collect())
}

async fn view(db: &PgPool, organization_id: Uuid, id: Uuid) -> Result<TicketSystemView, ApiError> {
    views(db, organization_id, Some(id))
        .await?
        .into_iter()
        .next()
        .ok_or(ApiError::NotFound)
}

/// The ticket systems the organization connected.
#[utoipa::path(
    get,
    path = "/ticket-systems",
    tag = "integrations",
    responses((status = OK, body = Vec<TicketSystemView>)),
)]
async fn list_ticket_systems(
    State(state): State<AppState>,
    member: Member,
) -> Result<Json<Vec<TicketSystemView>>, ApiError> {
    Ok(Json(views(&state.db, member.organization_id, None).await?))
}

/// Connect a ticket system with an API token. Controllers only. The first
/// sync starts right away; whoever connects a ticket system answers for
/// that data flow.
#[utoipa::path(
    post,
    path = "/ticket-systems",
    tag = "integrations",
    request_body = NewTicketSystem,
    responses(
        (status = CREATED, body = TicketSystemView),
        (status = BAD_REQUEST, description = "The address or the token is not usable", body = ErrorBody),
    ),
)]
async fn connect_ticket_system(
    State(state): State<AppState>,
    caller: Caller,
    Json(body): Json<NewTicketSystem>,
) -> Result<(StatusCode, Json<TicketSystemView>), ApiError> {
    caller.require_session()?;
    caller.require_role(Role::Controller)?;
    let site = body.site.trim().trim_end_matches('/').to_owned();
    body.kind
        .check_base_url(&site)
        .map_err(ApiError::BadRequest)?;
    let token = body.token.trim().to_owned();
    if token.is_empty() {
        return Err(ApiError::BadRequest(format!(
            "Add an API token from {}.",
            body.kind.name()
        )));
    }
    let email = body
        .email
        .map(|email| email.trim().to_owned())
        .filter(|email| !email.is_empty());
    let email = match (body.kind, email) {
        (TicketSystem::Jira, None) => {
            return Err(ApiError::BadRequest(
                "Jira Cloud signs in with the token and the email of its Atlassian account; add the email."
                    .to_owned(),
            ));
        }
        (TicketSystem::Jira, email) => email,
        (TicketSystem::AzureDevOps | TicketSystem::GitHubIssues, _) => None,
    };
    let key_hint = secrets::hint(&token);
    let stored = serde_json::to_string(&StoredCredentials { email, token })
        .context("could not write the credentials")?;
    let sealed = secrets::seal(&state.config.secret_key, &stored)
        .context("could not seal the credentials")?;
    let id = sqlx::query_scalar!(
        "INSERT INTO ticket_systems
             (organization_id, kind, site, sealed_credentials, key_hint, created_by, sync_requested_at)
         VALUES ($1, $2, $3, $4, $5, $6, now()) RETURNING id",
        caller.organization_id,
        body.kind.as_str(),
        site,
        sealed,
        key_hint,
        caller.person_id
    )
    .fetch_one(&state.db)
    .await?;
    crate::jobs::request_now(&state.db, JOB).await?;
    Ok((
        StatusCode::CREATED,
        Json(view(&state.db, caller.organization_id, id).await?),
    ))
}

/// Disconnect a ticket system. Controllers only. Its tickets stay, and stop
/// syncing.
#[utoipa::path(
    delete,
    path = "/ticket-systems/{id}",
    tag = "integrations",
    params(("id" = Uuid, Path, description = "The ticket system")),
    responses(
        (status = NO_CONTENT, description = "Disconnected"),
        (status = NOT_FOUND, body = ErrorBody),
    ),
)]
async fn disconnect_ticket_system(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    caller.require_session()?;
    caller.require_role(Role::Controller)?;
    let deleted = sqlx::query!(
        "DELETE FROM ticket_systems WHERE organization_id = $1 AND id = $2",
        caller.organization_id,
        id
    )
    .execute(&state.db)
    .await?;
    if deleted.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// The connection to a stored ticket system.
async fn connection(
    state: &AppState,
    organization_id: Uuid,
    id: Uuid,
) -> Result<Connection, ApiError> {
    let row = sqlx::query!(
        "SELECT kind, site, sealed_credentials FROM ticket_systems
         WHERE organization_id = $1 AND id = $2",
        organization_id,
        id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;
    let system = TicketSystem::from_name(&row.kind).ok_or(ApiError::NotFound)?;
    open_connection(
        &state.config.secret_key,
        system,
        row.site,
        &row.sealed_credentials,
    )
    .map_err(|error| ApiError::Conflict(error.to_string()))
}

fn open_connection(
    secret_key: &str,
    system: TicketSystem,
    base_url: String,
    sealed: &str,
) -> Result<Connection, secrets::SecretError> {
    let stored: StoredCredentials = serde_json::from_str(&secrets::open(secret_key, sealed)?)
        .map_err(|_| secrets::SecretError::Format)?;
    Ok(Connection {
        system,
        base_url,
        credentials: stored.into(),
    })
}

fn sync_error(error: SyncError) -> ApiError {
    match error {
        SyncError::RateLimited { .. } => ApiError::TooManyRequests(error.to_string()),
        other => ApiError::BadRequest(other.to_string()),
    }
}

/// The projects the ticket system shows the token, to map. Controllers
/// only.
#[utoipa::path(
    get,
    path = "/ticket-systems/{id}/projects",
    tag = "integrations",
    params(("id" = Uuid, Path, description = "The ticket system")),
    responses(
        (status = OK, body = Vec<ExternalProject>),
        (status = BAD_REQUEST, description = "The ticket system refused the token or could not be read", body = ErrorBody),
        (status = NOT_FOUND, body = ErrorBody),
    ),
)]
async fn external_projects(
    State(state): State<AppState>,
    member: Member,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<ExternalProject>>, ApiError> {
    member.require_role(Role::Controller)?;
    let connection = connection(&state, member.organization_id, id).await?;
    let client = super::http::client().context("could not build the HTTP client")?;
    let projects = tickets::list_projects(&client, &connection)
        .await
        .map_err(sync_error)?;
    Ok(Json(projects))
}

/// The ticket key pattern a mapped project gets when it has none: Jira keys
/// start with the project key, Azure Boards keys with `AB#`, and GitHub keys
/// with the repository's name.
fn key_pattern(system: TicketSystem, external_project: &str) -> Option<String> {
    let prefix = match system {
        TicketSystem::Jira => format!("{}-", regex::escape(external_project.trim())),
        TicketSystem::AzureDevOps => "AB#".to_owned(),
        TicketSystem::GitHubIssues => {
            let name = external_project.trim().rsplit('/').next()?;
            format!("{}#", regex::escape(name))
        }
    };
    Some(format!("^{prefix}\\d+"))
}

/// Set which projects of the ticket system feed which Token Controller
/// projects, replacing the earlier mapping. Controllers only. A project
/// without a ticket key pattern gets the one its tickets use, and a sync
/// starts right away.
#[utoipa::path(
    put,
    path = "/ticket-systems/{id}/mappings",
    tag = "integrations",
    params(("id" = Uuid, Path, description = "The ticket system")),
    request_body = Vec<Mapping>,
    responses(
        (status = OK, body = TicketSystemView),
        (status = BAD_REQUEST, description = "A project is unknown or mapped twice", body = ErrorBody),
        (status = NOT_FOUND, body = ErrorBody),
    ),
)]
async fn set_mappings(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<Uuid>,
    Json(mappings): Json<Vec<Mapping>>,
) -> Result<Json<TicketSystemView>, ApiError> {
    caller.require_session()?;
    caller.require_role(Role::Controller)?;
    let organization_id = caller.organization_id;
    let kind = sqlx::query_scalar!(
        "SELECT kind FROM ticket_systems WHERE organization_id = $1 AND id = $2",
        organization_id,
        id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;
    let system = TicketSystem::from_name(&kind).ok_or(ApiError::NotFound)?;
    let mut externals = BTreeSet::new();
    for mapping in &mappings {
        if mapping.external.trim().is_empty() {
            return Err(ApiError::BadRequest(
                "Each mapping needs the ticket system's project.".to_owned(),
            ));
        }
        if !externals.insert(mapping.external.trim().to_owned()) {
            return Err(ApiError::BadRequest(format!(
                "{} is mapped twice; map each project once.",
                mapping.external.trim()
            )));
        }
    }
    let mut tx = state.db.begin().await?;
    let externals: Vec<String> = externals.into_iter().collect();
    sqlx::query!(
        "DELETE FROM ticket_system_projects
         WHERE ticket_system_id = $1 AND NOT (external_project = ANY($2))",
        id,
        &externals
    )
    .execute(&mut *tx)
    .await?;
    for mapping in &mappings {
        store_mapping(&mut tx, organization_id, id, system, mapping).await?;
    }
    sqlx::query!(
        "UPDATE ticket_systems SET sync_requested_at = now() WHERE id = $1",
        id
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    crate::jobs::request_now(&state.db, JOB).await?;
    Ok(Json(view(&state.db, organization_id, id).await?))
}

async fn store_mapping(
    conn: &mut sqlx::PgConnection,
    organization_id: Uuid,
    ticket_system_id: Uuid,
    system: TicketSystem,
    mapping: &Mapping,
) -> Result<(), ApiError> {
    let external = mapping.external.trim();
    let project = sqlx::query!(
        "SELECT id, ticket_key_pattern FROM projects
         WHERE organization_id = $1 AND name = $2 AND archived_at IS NULL",
        organization_id,
        mapping.project.trim()
    )
    .fetch_optional(&mut *conn)
    .await?
    .ok_or_else(|| {
        ApiError::BadRequest(format!(
            "There is no project {:?}; add it under Projects first.",
            mapping.project.trim()
        ))
    })?;
    let name = mapping
        .external_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(external);
    // A project that changes starts its tickets over.
    sqlx::query!(
        "INSERT INTO ticket_system_projects
             (organization_id, ticket_system_id, external_project, external_name, project_id)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (ticket_system_id, external_project) DO UPDATE SET
             external_name = EXCLUDED.external_name,
             project_id = EXCLUDED.project_id,
             synced_until = CASE WHEN ticket_system_projects.project_id = EXCLUDED.project_id
                                 THEN ticket_system_projects.synced_until END",
        organization_id,
        ticket_system_id,
        external,
        name,
        project.id
    )
    .execute(&mut *conn)
    .await?;
    if project.ticket_key_pattern.is_none()
        && let Some(pattern) = key_pattern(system, external)
    {
        sqlx::query!(
            "UPDATE projects SET ticket_key_pattern = $3
             WHERE organization_id = $1 AND id = $2 AND ticket_key_pattern IS NULL",
            organization_id,
            project.id,
            pattern
        )
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// Sync a ticket system now, without waiting for its 15 minutes. Controllers
/// only.
#[utoipa::path(
    post,
    path = "/ticket-systems/{id}/sync",
    tag = "integrations",
    params(("id" = Uuid, Path, description = "The ticket system")),
    responses(
        (status = ACCEPTED, description = "The sync starts within a minute", body = TicketSystemView),
        (status = NOT_FOUND, body = ErrorBody),
    ),
)]
async fn sync_now(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<Uuid>,
) -> Result<(StatusCode, Json<TicketSystemView>), ApiError> {
    caller.require_session()?;
    caller.require_role(Role::Controller)?;
    let asked = sqlx::query!(
        "UPDATE ticket_systems SET sync_requested_at = now()
         WHERE organization_id = $1 AND id = $2",
        caller.organization_id,
        id
    )
    .execute(&state.db)
    .await?;
    if asked.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    crate::jobs::request_now(&state.db, JOB).await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(view(&state.db, caller.organization_id, id).await?),
    ))
}

/// Why one ticket system did not sync.
#[derive(Debug, thiserror::Error)]
enum SyncFailure {
    #[error("{0}")]
    Ticket(#[from] SyncError),
    #[error("{0}")]
    Secret(#[from] secrets::SecretError),
    #[error("{0}")]
    Database(#[from] sqlx::Error),
    #[error("{0}")]
    Store(#[from] crate::ingest::store::StoreError),
}

impl SyncFailure {
    /// Whether the failure is on the side of Token Controller or the ticket
    /// system as a whole, rather than one organization's token or project.
    const fn is_outage(&self) -> bool {
        match self {
            Self::Ticket(error) => matches!(
                error,
                SyncError::Network(_) | SyncError::Provider { .. } | SyncError::Format(_)
            ),
            Self::Database(_) | Self::Store(_) => true,
            Self::Secret(_) => false,
        }
    }
}

/// The `ticket-sync` job: syncs each ticket system that is due, at most
/// [`PER_RUN`] of them.
///
/// # Errors
///
/// Fails when the database fails, or when every ticket system it tried
/// failed for a reason that is not one organization's own, so the status
/// page shows the outage.
pub async fn sync_due(state: AppState) -> anyhow::Result<String> {
    let client = super::http::client().context("could not build the HTTP client")?;
    let due = sqlx::query!(
        r#"UPDATE ticket_systems SET last_sync_started_at = now()
           WHERE id IN (
               SELECT id FROM ticket_systems
               WHERE last_sync_started_at IS NULL
                  OR last_sync_started_at <= now() - make_interval(mins => $1)
                  OR sync_requested_at >= last_sync_started_at
               ORDER BY last_sync_started_at NULLS FIRST
               LIMIT $2)
           RETURNING id, organization_id, kind, site, sealed_credentials,
                     last_sync_started_at AS "started_at!""#,
        SYNC_EVERY_MINUTES,
        PER_RUN
    )
    .fetch_all(&state.db)
    .await?;
    let mut synced = 0_usize;
    let mut tickets = 0_usize;
    let mut outages = Vec::new();
    for system in &due {
        let result = match TicketSystem::from_name(&system.kind) {
            Some(kind) => {
                sync_system(
                    &state,
                    &client,
                    SystemRow {
                        id: system.id,
                        organization_id: system.organization_id,
                        kind,
                        site: system.site.clone(),
                        sealed: &system.sealed_credentials,
                        started_at: system.started_at,
                    },
                )
                .await
            }
            None => Ok(0),
        };
        let error = result.as_ref().err().map(ToString::to_string);
        sqlx::query!(
            "UPDATE ticket_systems SET
                 last_sync_at = CASE WHEN $2::text IS NULL THEN now() ELSE last_sync_at END,
                 last_error = $2
             WHERE id = $1",
            system.id,
            error
        )
        .execute(&state.db)
        .await?;
        match result {
            Ok(count) => {
                synced = synced.saturating_add(1);
                tickets = tickets.saturating_add(count);
            }
            Err(failure) if failure.is_outage() => outages.push(failure.to_string()),
            Err(_) => {}
        }
    }
    if !due.is_empty() && outages.len() == due.len() {
        anyhow::bail!(
            "no ticket system could sync: {}",
            outages
                .into_iter()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
                .join("; ")
        );
    }
    Ok(format!(
        "{synced} of {} ticket systems synced, {tickets} tickets written",
        due.len()
    ))
}

/// One ticket system the job syncs.
struct SystemRow<'a> {
    id: Uuid,
    organization_id: Uuid,
    kind: TicketSystem,
    site: String,
    sealed: &'a str,
    started_at: OffsetDateTime,
}

/// Syncs every mapped project of one ticket system, then places again the
/// token entries that named a ticket before it synced. A project that fails
/// does not stop the others; the first failure is returned.
async fn sync_system(
    state: &AppState,
    client: &reqwest::Client,
    system: SystemRow<'_>,
) -> Result<usize, SyncFailure> {
    let connection = open_connection(
        &state.config.secret_key,
        system.kind,
        system.site,
        system.sealed,
    )?;
    let mappings = sqlx::query!(
        "SELECT external_project, project_id, synced_until FROM ticket_system_projects
         WHERE ticket_system_id = $1 ORDER BY external_project",
        system.id
    )
    .fetch_all(&state.db)
    .await?;
    let mut written = 0_usize;
    let mut first_failure = None;
    for mapping in mappings {
        let fetched = tickets::fetch_updated(
            client,
            &connection,
            &mapping.external_project,
            mapping.synced_until,
        )
        .await;
        let fetched = match fetched {
            Ok(fetched) => fetched,
            Err(error) => {
                first_failure.get_or_insert(SyncFailure::from(error));
                continue;
            }
        };
        let count = store_tickets(
            &state.db,
            system.organization_id,
            mapping.project_id,
            system.kind,
            &fetched,
        )
        .await?;
        written = written.saturating_add(count);
        // Tickets that changed while this sync ran come again next time.
        sqlx::query!(
            "UPDATE ticket_system_projects
             SET synced_until = $3::timestamptz - interval '5 minutes'
             WHERE ticket_system_id = $1 AND external_project = $2",
            system.id,
            mapping.external_project,
            system.started_at
        )
        .execute(&state.db)
        .await?;
    }
    crate::ingest::store::place_unsynced(&state.db, system.organization_id).await?;
    match first_failure {
        Some(failure) => Err(failure),
        None => Ok(written),
    }
}

/// Writes the tickets of one mapped project: new keys are added, known keys
/// take the ticket system's title, description, project and assignee, and
/// parents are linked where the parent is known. Returns how many tickets
/// were written.
///
/// # Errors
///
/// Fails if a query fails.
pub async fn store_tickets(
    db: &PgPool,
    organization_id: Uuid,
    project_id: Uuid,
    system: TicketSystem,
    fetched: &[SyncedTicket],
) -> Result<usize, sqlx::Error> {
    // A ticket that changed during the sync comes twice; the newer copy
    // comes last.
    let newest: BTreeMap<&str, &SyncedTicket> = fetched
        .iter()
        .map(|ticket| (ticket.key.as_str(), ticket))
        .collect();
    if newest.is_empty() {
        return Ok(0);
    }
    let tickets: Vec<&SyncedTicket> = newest.into_values().collect();
    let keys: Vec<String> = tickets.iter().map(|ticket| ticket.key.clone()).collect();
    // Arrays carry an empty string for a missing value; the queries read
    // it back as NULL.
    let text = |value: &Option<String>| value.clone().unwrap_or_default();
    let titles: Vec<String> = tickets.iter().map(|ticket| text(&ticket.title)).collect();
    let descriptions: Vec<String> = tickets
        .iter()
        .map(|ticket| text(&ticket.description))
        .collect();
    let parents: Vec<String> = tickets
        .iter()
        .map(|ticket| text(&ticket.parent_key))
        .collect();
    let emails: Vec<String> = tickets
        .iter()
        .map(|ticket| text(&ticket.assignee_email))
        .collect();
    // Assigned to someone whose email the system does not share: keep the
    // assignee Token Controller has.
    let keep_assignee: Vec<bool> = tickets
        .iter()
        .map(|ticket| ticket.assignee_email.is_none() && ticket.assignee_name.is_some())
        .collect();
    let mut tx = db.begin().await?;
    sqlx::query!(
        "INSERT INTO tickets (organization_id, key, project_id, title, description, source, updated_at)
         SELECT $1, x.key, $2, NULLIF(x.title, ''), NULLIF(x.description, ''), $3, now()
         FROM unnest($4::text[], $5::text[], $6::text[]) AS x(key, title, description)
         ON CONFLICT (organization_id, key) DO UPDATE SET
             project_id = EXCLUDED.project_id,
             title = EXCLUDED.title,
             description = EXCLUDED.description,
             source = EXCLUDED.source,
             updated_at = now()",
        organization_id,
        project_id,
        system.as_str(),
        &keys,
        &titles,
        &descriptions
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        "UPDATE tickets t SET assignee_id = (
             SELECT p.id FROM people p
             WHERE p.organization_id = $1 AND p.email = lower(trim(NULLIF(x.email, '')))
               AND p.removed_at IS NULL)
         FROM unnest($2::text[], $3::text[], $4::bool[]) AS x(key, email, keep)
         WHERE t.organization_id = $1 AND t.key = x.key AND NOT x.keep",
        organization_id,
        &keys,
        &emails,
        &keep_assignee
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        "UPDATE tickets t SET parent_id = (
             SELECT p.id FROM tickets p
             WHERE p.organization_id = $1 AND p.key = x.parent AND p.id <> t.id)
         FROM unnest($2::text[], $3::text[]) AS x(key, parent)
         WHERE t.organization_id = $1 AND t.key = x.key",
        organization_id,
        &keys,
        &parents
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(keys.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapped_projects_get_the_key_pattern_their_tickets_use() {
        assert_eq!(
            key_pattern(TicketSystem::Jira, "KD").as_deref(),
            Some("^KD-\\d+")
        );
        assert_eq!(
            key_pattern(
                TicketSystem::AzureDevOps,
                "6ce954b1-ce1f-45d1-b94d-e6bf2464ba2c"
            )
            .as_deref(),
            Some("^AB#\\d+")
        );
        assert_eq!(
            key_pattern(TicketSystem::GitHubIssues, "kestrel/storefront").as_deref(),
            Some("^storefront#\\d+")
        );
        assert_eq!(
            key_pattern(TicketSystem::GitHubIssues, "kestrel/web.app").as_deref(),
            Some("^web\\.app#\\d+")
        );
        for pattern in [
            key_pattern(TicketSystem::Jira, "KD"),
            key_pattern(TicketSystem::AzureDevOps, "x"),
            key_pattern(TicketSystem::GitHubIssues, "kestrel/storefront"),
        ]
        .into_iter()
        .flatten()
        {
            assert!(
                tokencontroller_core::pattern::TicketKeyPattern::new(&pattern).is_ok(),
                "{pattern}"
            );
        }
    }

    #[test]
    fn stored_credentials_keep_the_jira_email() {
        let jira: Credentials = StoredCredentials {
            email: Some("ana@acme.example".to_owned()),
            token: "atl-token".to_owned(),
        }
        .into();
        assert_eq!(
            jira,
            Credentials::EmailAndToken {
                email: "ana@acme.example".to_owned(),
                token: "atl-token".to_owned()
            }
        );
        let github: Credentials = StoredCredentials {
            email: None,
            token: "ghp_x".to_owned(),
        }
        .into();
        assert_eq!(github, Credentials::Token("ghp_x".to_owned()));
        let written = serde_json::to_string(&StoredCredentials {
            email: None,
            token: "t".to_owned(),
        })
        .unwrap();
        assert_eq!(written, r#"{"token":"t"}"#);
    }

    #[test]
    fn an_outage_is_not_one_organizations_bad_token() {
        assert!(SyncFailure::Ticket(SyncError::Network("down".to_owned())).is_outage());
        assert!(!SyncFailure::Ticket(SyncError::Unauthorized("no".to_owned())).is_outage());
        assert!(!SyncFailure::Ticket(SyncError::NotFound("gone".to_owned())).is_outage());
        assert!(!SyncFailure::Secret(secrets::SecretError::WrongKey).is_outage());
    }
}
