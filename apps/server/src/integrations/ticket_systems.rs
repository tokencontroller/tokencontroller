//! Ticket systems an organization connects, and the sync that keeps their
//! features current: every 15 minutes per ticket system, or when someone
//! asks (docs: Account > Tags > Jira).
//!
//! An org manager connects Jira Cloud with an API token, then maps each of
//! its projects to a project tag value: the sync map. The `ticket-sync`
//! job fetches the issues that changed since the last sync of each mapped
//! project and writes each as a feature tag value event with that project
//! as default, copying the fields the sync map lists. A feature a message
//! named before it synced fills in: the fold reads the value on record.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use anyhow::Context as _;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use tokencontroller_core::{Event, Record, TagName, TagValueRecord, Tags};
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
use crate::events::{self, Log, access, prefix};
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

/// The fields the sync map can copy onto a feature.
pub const FIELDS: [&str; 6] = [
    "title",
    "description",
    "type",
    "status",
    "parent",
    "assignee",
];

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
    /// The Jira Cloud site.
    pub site: String,
    /// The token's last characters, to tell tokens apart.
    pub key_hint: String,
    /// The sync map: which of its projects feed which project tag value.
    pub mappings: Vec<Mapping>,
    /// When the last sync succeeded.
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_sync_at: Option<OffsetDateTime>,
    pub status: ConnectionStatus,
    /// Why the last sync failed.
    pub message: Option<String>,
}

/// One row of the sync map: a Jira project and the project tag value its
/// issues get as default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Mapping {
    /// The Jira project key, as `GET /ticket-systems/{id}/projects` lists
    /// them.
    pub external: String,
    /// Its name. Optional when mapping; the key stands in.
    #[serde(default)]
    pub external_name: Option<String>,
    /// The key of the project tag value.
    pub project: String,
    /// The fields copied onto each feature, from `title`, `description`,
    /// `type`, `status`, `parent` and `assignee`. Left out, all of them.
    #[serde(default)]
    pub fields: Option<Vec<String>>,
}

/// A ticket system to connect.
#[derive(Deserialize, ToSchema)]
pub struct NewTicketSystem {
    pub kind: TicketSystem,
    /// The Jira Cloud site, such as `https://acme.atlassian.net`.
    pub site: String,
    /// An API token from the ticket system.
    pub token: String,
    /// The email of the Atlassian account the token belongs to. Jira Cloud
    /// signs in with both.
    #[serde(default)]
    pub email: Option<String>,
}

/// What is stored, sealed, for a ticket system.
#[derive(Serialize, Deserialize)]
struct StoredCredentials {
    email: String,
    token: String,
}

impl From<StoredCredentials> for Credentials {
    fn from(stored: StoredCredentials) -> Self {
        Self::EmailAndToken {
            email: stored.email,
            token: stored.token,
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
        "SELECT ticket_system_id, external_project, external_name, project, fields
         FROM ticket_system_projects
         WHERE organization_id = $1
         ORDER BY external_name, external_project",
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
                        fields: Some(mapping.fields.clone()),
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

/// Refuses callers who are not org managers as of now.
async fn require_org_manager(state: &AppState, caller: &Caller) -> Result<(), ApiError> {
    let person = caller.person()?;
    let log = Log::load(state, caller.organization_id).await?;
    let fold = log.fold(OffsetDateTime::now_utc())?;
    access::require_org_manager(&fold, &person)
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

/// Connect a ticket system with an API token. Org managers only. The first
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
    require_org_manager(&state, &caller).await?;
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
        .filter(|email| !email.is_empty())
        .ok_or_else(|| {
            ApiError::BadRequest(
                "Jira Cloud signs in with the token and the email of its Atlassian account; add the email."
                    .to_owned(),
            )
        })?;
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

/// Disconnect a ticket system. Org managers only. Its features stay, and
/// stop syncing.
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
    require_org_manager(&state, &caller).await?;
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

/// The projects the ticket system shows the token, to map. Org managers
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
    require_org_manager(&state, &member).await?;
    let connection = connection(&state, member.organization_id, id).await?;
    let client = super::http::client().context("could not build the HTTP client")?;
    let projects = tickets::list_projects(&client, &connection)
        .await
        .map_err(sync_error)?;
    Ok(Json(projects))
}

/// The fields a mapping copies, checked against [`FIELDS`].
fn checked_fields(fields: Option<&[String]>) -> Result<Vec<String>, ApiError> {
    let Some(fields) = fields else {
        return Ok(FIELDS.iter().map(|field| (*field).to_owned()).collect());
    };
    let mut checked = Vec::new();
    for field in fields {
        let field = field.trim().to_ascii_lowercase();
        if !FIELDS.contains(&field.as_str()) {
            return Err(ApiError::BadRequest(format!(
                "{field:?} is not a field the sync copies; use {}",
                FIELDS.join(", ")
            )));
        }
        if !checked.contains(&field) {
            checked.push(field);
        }
    }
    Ok(checked)
}

/// Set the sync map: which Jira projects feed which project tag values,
/// replacing the earlier map. Org managers only. A sync starts right away.
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
    let person = caller.person()?;
    let organization_id = caller.organization_id;
    let log = Log::load(&state, organization_id).await?;
    let fold = log.fold(OffsetDateTime::now_utc())?;
    access::require_org_manager(&fold, &person)?;
    let known = sqlx::query_scalar!(
        "SELECT id FROM ticket_systems WHERE organization_id = $1 AND id = $2",
        organization_id,
        id
    )
    .fetch_optional(&state.db)
    .await?;
    if known.is_none() {
        return Err(ApiError::NotFound);
    }
    let mut externals = BTreeSet::new();
    let project_name = TagName::project();
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
        let project = events::json::tag_value("project", mapping.project.trim())?;
        if fold.values().tag_value(&project_name, &project).is_none() {
            return Err(ApiError::BadRequest(format!(
                "There is no project {:?}; add the project tag value first.",
                mapping.project.trim()
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
        let external = mapping.external.trim();
        let name = mapping
            .external_name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or(external);
        let fields = checked_fields(mapping.fields.as_deref())?;
        // A project that changes starts its issues over.
        sqlx::query!(
            "INSERT INTO ticket_system_projects
                 (organization_id, ticket_system_id, external_project, external_name, project, fields)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (ticket_system_id, external_project) DO UPDATE SET
                 external_name = EXCLUDED.external_name,
                 project = EXCLUDED.project,
                 fields = EXCLUDED.fields,
                 synced_until = CASE WHEN ticket_system_projects.project = EXCLUDED.project
                                      AND ticket_system_projects.fields = EXCLUDED.fields
                                     THEN ticket_system_projects.synced_until END",
            organization_id,
            id,
            external,
            name,
            mapping.project.trim(),
            &fields
        )
        .execute(&mut *tx)
        .await?;
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

/// Sync a ticket system now, without waiting for its 15 minutes. Org
/// managers only.
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
    require_org_manager(&state, &caller).await?;
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
    Api(#[from] ApiError),
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
            Self::Database(_) | Self::Api(_) => true,
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
    let mut features = 0_usize;
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
                features = features.saturating_add(count);
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
        "{synced} of {} ticket systems synced, {features} features written",
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

/// Syncs every mapped project of one ticket system. A project that fails
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
        "SELECT external_project, project, fields, synced_until FROM ticket_system_projects
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
        let count = store_features(
            state,
            system.organization_id,
            &SyncMap {
                project: &mapping.project,
                fields: &mapping.fields,
            },
            &fetched,
        )
        .await?;
        written = written.saturating_add(count);
        // Issues that changed while this sync ran come again next time.
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
    match first_failure {
        Some(failure) => Err(failure),
        None => Ok(written),
    }
}

/// One row of the sync map as the sync reads it.
#[derive(Debug, Clone, Copy)]
pub struct SyncMap<'a> {
    /// The key of the project tag value the features get as default.
    pub project: &'a str,
    /// The fields copied.
    pub fields: &'a [String],
}

/// The feature tag value record a synced issue becomes.
///
/// # Errors
///
/// [`ApiError::BadRequest`] for an issue key or project the core refuses.
pub fn feature_record(
    map: &SyncMap<'_>,
    ticket: &SyncedTicket,
) -> Result<TagValueRecord, ApiError> {
    let copies = |field: &str| map.fields.iter().any(|copied| copied == field);
    let mut defaults = Tags::new();
    defaults.insert(
        TagName::project(),
        events::json::tag_value("project", map.project)?,
    );
    let mut description = copies("description")
        .then(|| ticket.description.clone())
        .flatten();
    let mut notes = Vec::new();
    if copies("parent")
        && let Some(parent) = &ticket.parent_key
    {
        notes.push(format!("Parent: {parent}"));
    }
    if copies("assignee")
        && let Some(assignee) = ticket
            .assignee_email
            .as_ref()
            .or(ticket.assignee_name.as_ref())
    {
        notes.push(format!("Assignee: {assignee}"));
    }
    if !notes.is_empty() {
        let notes = notes.join("\n");
        description = Some(match description {
            Some(text) if !text.is_empty() => format!("{text}\n\n{notes}"),
            _ => notes,
        });
    }
    Ok(TagValueRecord {
        name: TagName::feature(),
        key: events::json::tag_value("feature", &ticket.key)?,
        title: copies("title").then(|| ticket.title.clone()).flatten(),
        description,
        source_id: Some(
            ticket
                .url
                .clone()
                .unwrap_or_else(|| format!("{}:{}", ticket.system.as_str(), ticket.key)),
        ),
        source_status: copies("status").then(|| {
            if ticket.closed {
                "done".to_owned()
            } else {
                "open".to_owned()
            }
        }),
        defaults,
    })
}

/// Writes the issues of one mapped project as feature tag values: a new key
/// becomes a feature, a known key takes the issue's fields, and a feature
/// whose record is already the same is left alone. Returns how many
/// features were written.
///
/// # Errors
///
/// Fails if a query fails or the log refuses the events.
pub async fn store_features(
    state: &AppState,
    organization_id: Uuid,
    map: &SyncMap<'_>,
    fetched: &[SyncedTicket],
) -> Result<usize, ApiError> {
    // An issue that changed during the sync comes twice; the newer copy
    // comes last.
    let newest: BTreeMap<&str, &SyncedTicket> = fetched
        .iter()
        .map(|ticket| (ticket.key.as_str(), ticket))
        .collect();
    if newest.is_empty() {
        return Ok(0);
    }
    let log = Log::load(state, organization_id).await?;
    let now = OffsetDateTime::now_utc();
    let mut new = Vec::new();
    for (index, ticket) in newest.into_values().enumerate() {
        let record = feature_record(map, ticket)?;
        let older = crate::tags::live_value_event(&log, &record.name, &record.key);
        if let Some(older) = older
            && let Record::TagValue(current) = &older.record
            && *current == record
        {
            continue;
        }
        let offset = time::Duration::microseconds(i64::try_from(index).unwrap_or(i64::MAX));
        let at = older.map_or(now.saturating_add(offset), |older| {
            Log::after(older, now.saturating_add(offset))
        });
        let mut event = Event::new(
            &events::new_id(prefix::TAG_VALUE),
            at,
            Record::TagValue(record),
        )
        .map_err(|error| ApiError::Internal(error.into()))?;
        if let Some(older) = older {
            event = event.superseding(older.id.clone());
        }
        new.push(event);
    }
    if new.is_empty() {
        return Ok(0);
    }
    events::write(state, &log, &new).await?;
    Ok(new.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokencontroller_core::TagValue;

    fn issue(key: &str, closed: bool) -> SyncedTicket {
        SyncedTicket {
            system: TicketSystem::Jira,
            external_project: "KD".to_owned(),
            key: key.to_owned(),
            title: Some("Checkout redesign".to_owned()),
            description: Some("Make it faster.".to_owned()),
            parent_key: Some("KD-1".to_owned()),
            assignee_email: Some("mara@example.com".to_owned()),
            assignee_name: Some("Mara".to_owned()),
            url: Some("https://acme.atlassian.net/browse/KD-2".to_owned()),
            updated_at: OffsetDateTime::UNIX_EPOCH,
            closed,
        }
    }

    #[test]
    fn a_synced_issue_becomes_a_feature_with_its_project_as_default() {
        let all: Vec<String> = FIELDS.iter().map(|field| (*field).to_owned()).collect();
        let map = SyncMap {
            project: "shop",
            fields: &all,
        };
        let record = feature_record(&map, &issue("KD-2", true)).unwrap();
        assert_eq!(record.name, TagName::feature());
        assert_eq!(record.key.as_str(), "KD-2");
        assert_eq!(record.title.as_deref(), Some("Checkout redesign"));
        assert_eq!(
            record.description.as_deref(),
            Some("Make it faster.\n\nParent: KD-1\nAssignee: mara@example.com")
        );
        assert_eq!(record.source_status.as_deref(), Some("done"));
        assert_eq!(
            record.source_id.as_deref(),
            Some("https://acme.atlassian.net/browse/KD-2")
        );
        assert_eq!(
            record
                .defaults
                .get(&TagName::project())
                .map(TagValue::as_str),
            Some("shop")
        );
    }

    #[test]
    fn the_sync_map_copies_only_the_fields_it_lists() {
        let few = vec!["title".to_owned()];
        let map = SyncMap {
            project: "shop",
            fields: &few,
        };
        let record = feature_record(&map, &issue("KD-2", false)).unwrap();
        assert_eq!(record.title.as_deref(), Some("Checkout redesign"));
        assert_eq!(record.description, None);
        assert_eq!(record.source_status, None);
        assert_eq!(
            checked_fields(Some(&["Title".to_owned(), "status".to_owned()])).unwrap(),
            vec!["title".to_owned(), "status".to_owned()]
        );
        assert_eq!(checked_fields(None).unwrap().len(), FIELDS.len());
        assert!(checked_fields(Some(&["priority".to_owned()])).is_err());
    }

    #[test]
    fn stored_credentials_keep_the_jira_email() {
        let jira: Credentials = StoredCredentials {
            email: "ana@acme.example".to_owned(),
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
    }

    #[test]
    fn an_outage_is_not_one_organizations_bad_token() {
        assert!(SyncFailure::Ticket(SyncError::Network("down".to_owned())).is_outage());
        assert!(!SyncFailure::Ticket(SyncError::Unauthorized("no".to_owned())).is_outage());
        assert!(!SyncFailure::Ticket(SyncError::NotFound("gone".to_owned())).is_outage());
        assert!(!SyncFailure::Secret(secrets::SecretError::WrongKey).is_outage());
    }
}
