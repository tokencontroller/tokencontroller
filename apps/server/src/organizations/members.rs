//! Members, invites and the filters people managers are assigned (docs:
//! Account > Organization settings > People, Account > Roles).

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use sqlx::types::Json as JsonColumn;
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use super::Role;
use crate::error::ApiError;
use crate::mail::{self, Mail, render};
use crate::sign_in::{Member, domain_allowed, normalize_email, secret};
use crate::tags::{self, Tags};
use crate::{AppState, Config};

const INVITE_HTML: &str = include_str!("../sign_in/mail/invite.html");
const INVITE_TEXT: &str = include_str!("../sign_in/mail/invite.txt");

/// Where a member stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemberStatus {
    /// Invited and not signed in yet.
    Invited,
    Joined,
    /// Left the organization. Kept so their past token entries keep their
    /// agent manager.
    Removed,
}

/// A member as the API shows them.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct MemberView {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    pub role: Role,
    /// Tags their new token entries inherit, such as `{"team": "web"}`.
    pub tags: Tags,
    /// For a people manager: the filters they read and approve, such as
    /// `{"team": "web"}` or `{"client": "acme"}`.
    pub filters: Vec<Tags>,
    pub status: MemberStatus,
    #[serde(with = "time::serde::rfc3339")]
    pub invited_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub joined_at: Option<OffsetDateTime>,
}

/// Invite someone by email.
#[derive(Debug, Deserialize, ToSchema)]
pub struct NewMember {
    pub email: String,
    #[serde(default)]
    pub name: String,
    /// Default: agent manager.
    #[serde(default = "agent_manager")]
    pub role: Role,
    #[serde(default)]
    pub tags: Tags,
}

const fn agent_manager() -> Role {
    Role::AgentManager
}

/// A member who was just invited, and the link they got by mail.
#[derive(Debug, Serialize, ToSchema)]
pub struct Invited {
    pub member: MemberView,
    /// Opens sign-in with the email filled in; `tc connect` takes it too.
    pub invite_url: String,
}

/// A change to a member. Fields left out stay as they are. Tags replace the
/// member's tags; only their future token entries follow.
#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct MemberChange {
    pub name: Option<String>,
    pub role: Option<Role>,
    pub tags: Option<Tags>,
}

/// What an invite link shows before sign-in.
#[derive(Debug, Serialize, ToSchema)]
pub struct InviteView {
    pub organization_name: String,
    /// The address the invite went to; sign in with it.
    pub email: String,
}

/// The filters assigned to a person.
///
/// # Errors
///
/// Fails if a query fails.
pub async fn filters_of(db: &PgPool, person_id: Uuid) -> Result<Vec<Tags>, sqlx::Error> {
    let rows = sqlx::query_scalar!(
        r#"SELECT filter AS "filter: JsonColumn<Tags>" FROM people_manager_filters
           WHERE person_id = $1 ORDER BY created_at, filter::text"#,
        person_id
    )
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(|JsonColumn(filter)| filter).collect())
}

pub(super) async fn list(
    db: &PgPool,
    organization_id: Uuid,
    only: Option<Uuid>,
    include_removed: bool,
) -> Result<Vec<MemberView>, ApiError> {
    let rows = sqlx::query!(
        r#"SELECT p.id, p.email, p.name, p.role, p.tags AS "tags: JsonColumn<Tags>",
                  p.invited_at, p.joined_at, p.removed_at,
                  coalesce(array_agg(f.filter ORDER BY f.created_at, f.filter::text)
                           FILTER (WHERE f.id IS NOT NULL), '{}') AS "filters!: Vec<JsonColumn<Tags>>"
           FROM people p
           LEFT JOIN people_manager_filters f ON f.person_id = p.id
           WHERE p.organization_id = $1
             AND ($2::uuid IS NULL OR p.id = $2)
             AND ($3 OR p.removed_at IS NULL)
           GROUP BY p.id
           ORDER BY p.removed_at IS NOT NULL, lower(p.name), p.email"#,
        organization_id,
        only,
        include_removed
    )
    .fetch_all(db)
    .await?;
    rows.into_iter()
        .map(|row| {
            let status = match (row.removed_at, row.joined_at) {
                (Some(_), _) => MemberStatus::Removed,
                (None, Some(_)) => MemberStatus::Joined,
                (None, None) => MemberStatus::Invited,
            };
            Ok(MemberView {
                id: row.id,
                email: row.email,
                name: row.name,
                role: row
                    .role
                    .parse()
                    .map_err(|error: super::UnknownRole| ApiError::Internal(error.into()))?,
                tags: row.tags.0,
                filters: row
                    .filters
                    .into_iter()
                    .map(|JsonColumn(filter)| filter)
                    .collect(),
                status,
                invited_at: row.invited_at,
                joined_at: row.joined_at,
            })
        })
        .collect()
}

/// Everyone in the organization.
#[utoipa::path(
    get,
    path = "/members",
    tag = "members",
    responses((status = OK, body = Vec<MemberView>)),
)]
pub(super) async fn list_members(
    State(state): State<AppState>,
    member: Member,
) -> Result<Json<Vec<MemberView>>, ApiError> {
    Ok(Json(
        list(&state.db, member.organization_id, None, false).await?,
    ))
}

/// Invite someone by email. They get a link that signs them in. Controllers
/// only.
#[utoipa::path(
    post,
    path = "/members",
    tag = "members",
    request_body = NewMember,
    responses(
        (status = CREATED, body = Invited),
        (status = BAD_REQUEST, body = crate::error::ErrorBody),
        (status = CONFLICT, description = "Already a member, or the plan's member limit is reached", body = crate::error::ErrorBody),
    ),
)]
pub(super) async fn invite_member(
    State(state): State<AppState>,
    member: Member,
    Json(request): Json<NewMember>,
) -> Result<(StatusCode, Json<Invited>), ApiError> {
    member.require_session()?;
    member.require_role(Role::Controller)?;
    let email = normalize_email(&request.email)?;
    let name = clean_name(&request.name)?;
    let tags = tags::validate(&state.db, member.organization_id, &request.tags, false).await?;

    let organization = sqlx::query!(
        "SELECT slug, sign_in_domains FROM organizations WHERE id = $1",
        member.organization_id
    )
    .fetch_one(&state.db)
    .await?;
    if !domain_allowed(&email, &organization.sign_in_domains) {
        return Err(ApiError::BadRequest(format!(
            "Sign-in is limited to {}. Change the sign-in domains first.",
            organization.sign_in_domains.join(", ")
        )));
    }
    let existing = sqlx::query_scalar!(
        "SELECT organization_id FROM people WHERE email = $1 AND removed_at IS NULL",
        email
    )
    .fetch_optional(&state.db)
    .await?;
    match existing {
        Some(organization_id) if organization_id == member.organization_id => {
            return Err(ApiError::Conflict(format!("{email} is already a member.")));
        }
        Some(_) => {
            return Err(ApiError::Conflict(format!(
                "{email} is a member of another organization. One account belongs to one organization."
            )));
        }
        None => {}
    }
    let limit = crate::billing::member_limit(&state, member.organization_id).await?;
    if super::member_count(&state.db, member.organization_id).await? >= limit.max {
        return Err(ApiError::Conflict(limit.message.to_owned()));
    }

    let code = format!(
        "{}-{}",
        organization.slug,
        secret::short_code()
            .map_err(|error| ApiError::Internal(anyhow::anyhow!("no randomness: {error}")))?
    );
    let mut tx = state.db.begin().await?;
    let person_id = sqlx::query_scalar!(
        r#"INSERT INTO people (organization_id, email, name, role, tags)
           VALUES ($1, $2, $3, $4, $5) RETURNING id"#,
        member.organization_id,
        email,
        name,
        request.role.as_str(),
        JsonColumn(&tags) as _
    )
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query!(
        "INSERT INTO invites (code, organization_id, person_id, created_by) VALUES ($1, $2, $3, $4)",
        code,
        member.organization_id,
        person_id,
        member.person_id
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    super::record_audit_event(
        &state,
        member.organization_id,
        Some(member.person_id),
        "member.invited",
        Some(person_id),
    );
    let invite_url = invite_url(&state.config, &code);
    send_invite(&state, member.person_id, person_id, &invite_url).await?;
    let invited = super::member(&state.db, member.organization_id, person_id).await?;
    Ok((
        StatusCode::CREATED,
        Json(Invited {
            member: invited,
            invite_url,
        }),
    ))
}

/// Send the invite mail again to someone who has not signed in yet.
/// Controllers only.
#[utoipa::path(
    post,
    path = "/members/{id}/invite",
    tag = "members",
    params(("id" = Uuid, Path)),
    responses(
        (status = OK, body = Invited),
        (status = CONFLICT, description = "They already signed in", body = crate::error::ErrorBody),
        (status = NOT_FOUND, body = crate::error::ErrorBody),
    ),
)]
pub(super) async fn resend_invite(
    State(state): State<AppState>,
    member: Member,
    Path(id): Path<Uuid>,
) -> Result<Json<Invited>, ApiError> {
    member.require_session()?;
    member.require_role(Role::Controller)?;
    let invited = super::member(&state.db, member.organization_id, id).await?;
    if invited.status != MemberStatus::Invited {
        return Err(ApiError::Conflict(format!(
            "{} already signed in.",
            invited.email
        )));
    }
    let code = sqlx::query_scalar!(
        "SELECT code FROM invites WHERE person_id = $1 ORDER BY created_at DESC LIMIT 1",
        id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;
    let invite_url = invite_url(&state.config, &code);
    send_invite(&state, member.person_id, id, &invite_url).await?;
    Ok(Json(Invited {
        member: invited,
        invite_url,
    }))
}

fn invite_url(config: &Config, code: &str) -> String {
    format!("{}/i/{code}", config.public_url)
}

async fn send_invite(
    state: &AppState,
    inviter_id: Uuid,
    person_id: Uuid,
    invite_url: &str,
) -> Result<(), ApiError> {
    let row = sqlx::query!(
        r#"SELECT p.email, p.role, o.name AS organization,
                  (SELECT coalesce(nullif(i.name, ''), i.email) FROM people i WHERE i.id = $2) AS "inviter!"
           FROM people p JOIN organizations o ON o.id = p.organization_id
           WHERE p.id = $1"#,
        person_id,
        inviter_id
    )
    .fetch_one(&state.db)
    .await?;
    let role = row
        .role
        .parse::<Role>()
        .map_err(|error| ApiError::Internal(error.into()))?
        .to_string();
    let values = [
        ("email", row.email.as_str()),
        ("organization", row.organization.as_str()),
        ("inviter", row.inviter.as_str()),
        ("role", role.as_str()),
        ("invite_url", invite_url),
        ("host", mail::host_of(&state.config.public_url)),
    ];
    let invite = Mail {
        kind: "invite",
        to: row.email.clone(),
        subject: format!("Join {} on Token Controller", row.organization),
        text: render(INVITE_TEXT, &values, false),
        html: render(INVITE_HTML, &values, true),
    };
    // The invite exists either way; the controller can send it again.
    if let Err(error) = mail::send(&state.db, &state.config, invite).await {
        tracing::warn!("invite mail failed: {error:#}");
    }
    Ok(())
}

/// Change a member's name, role or tags. Controllers only.
#[utoipa::path(
    patch,
    path = "/members/{id}",
    tag = "members",
    params(("id" = Uuid, Path)),
    request_body = MemberChange,
    responses(
        (status = OK, body = MemberView),
        (status = CONFLICT, description = "The organization would have no controller", body = crate::error::ErrorBody),
        (status = NOT_FOUND, body = crate::error::ErrorBody),
    ),
)]
pub(super) async fn update_member(
    State(state): State<AppState>,
    member: Member,
    Path(id): Path<Uuid>,
    Json(change): Json<MemberChange>,
) -> Result<Json<MemberView>, ApiError> {
    member.require_session()?;
    member.require_role(Role::Controller)?;
    let current = super::member(&state.db, member.organization_id, id).await?;
    if current.status == MemberStatus::Removed {
        return Err(ApiError::NotFound);
    }
    if current.role == Role::Controller && change.role.is_some_and(|role| role != Role::Controller)
    {
        keep_a_controller(&state.db, member.organization_id, id).await?;
    }
    let name = change.name.as_deref().map(clean_name).transpose()?;
    let tags = match &change.tags {
        Some(tags) => Some(tags::validate(&state.db, member.organization_id, tags, false).await?),
        None => None,
    };
    sqlx::query!(
        r#"UPDATE people SET
             name = coalesce($3, name),
             role = coalesce($4, role),
             tags = coalesce($5, tags)
           WHERE id = $1 AND organization_id = $2"#,
        id,
        member.organization_id,
        name,
        change.role.map(Role::as_str),
        tags.map(JsonColumn) as _
    )
    .execute(&state.db)
    .await?;
    super::record_audit_event(
        &state,
        member.organization_id,
        Some(member.person_id),
        "member.changed",
        Some(id),
    );
    Ok(Json(
        super::member(&state.db, member.organization_id, id).await?,
    ))
}

/// Remove a member. They can no longer sign in; their past token entries
/// keep them as agent manager, and their service tokens wait for a new
/// owner. Controllers only.
#[utoipa::path(
    delete,
    path = "/members/{id}",
    tag = "members",
    params(("id" = Uuid, Path)),
    responses(
        (status = NO_CONTENT, description = "Removed"),
        (status = CONFLICT, description = "The organization would have no controller", body = crate::error::ErrorBody),
        (status = NOT_FOUND, body = crate::error::ErrorBody),
    ),
)]
pub(super) async fn remove_member(
    State(state): State<AppState>,
    member: Member,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    member.require_session()?;
    member.require_role(Role::Controller)?;
    super::remove_member(&state, member.organization_id, id, Some(member.person_id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn keep_a_controller(
    db: &PgPool,
    organization_id: Uuid,
    leaving: Uuid,
) -> Result<(), ApiError> {
    let others = sqlx::query_scalar!(
        r#"SELECT count(*) AS "count!" FROM people
           WHERE organization_id = $1 AND role = 'controller' AND removed_at IS NULL AND id <> $2"#,
        organization_id,
        leaving
    )
    .fetch_one(db)
    .await?;
    if others == 0 {
        return Err(ApiError::Conflict(
            "The organization needs at least one controller. Make someone else controller first."
                .to_owned(),
        ));
    }
    Ok(())
}

/// Assign a people manager the filters they read and approve, such as
/// `{"team": "web"}` and `{"client": "acme"}`. Replaces their filters.
/// Controllers only.
#[utoipa::path(
    put,
    path = "/members/{id}/filters",
    tag = "members",
    params(("id" = Uuid, Path)),
    request_body = Vec<Tags>,
    responses(
        (status = OK, body = MemberView),
        (status = BAD_REQUEST, body = crate::error::ErrorBody),
        (status = NOT_FOUND, body = crate::error::ErrorBody),
    ),
)]
pub(super) async fn set_filters(
    State(state): State<AppState>,
    member: Member,
    Path(id): Path<Uuid>,
    Json(filters): Json<Vec<Tags>>,
) -> Result<Json<MemberView>, ApiError> {
    member.require_session()?;
    member.require_role(Role::Controller)?;
    let current = super::member(&state.db, member.organization_id, id).await?;
    if current.role != Role::PeopleManager && !filters.is_empty() {
        return Err(ApiError::BadRequest(
            "Only people managers are assigned filters. A controller sees everything.".to_owned(),
        ));
    }
    let mut checked = Vec::with_capacity(filters.len());
    for filter in &filters {
        if filter.is_empty() {
            return Err(ApiError::BadRequest(
                "A filter needs at least one tag.".to_owned(),
            ));
        }
        let filter = tags::validate(&state.db, member.organization_id, filter, true).await?;
        if !checked.contains(&filter) {
            checked.push(filter);
        }
    }
    let mut tx = state.db.begin().await?;
    sqlx::query!(
        "DELETE FROM people_manager_filters WHERE person_id = $1",
        id
    )
    .execute(&mut *tx)
    .await?;
    for filter in checked {
        sqlx::query!(
            "INSERT INTO people_manager_filters (organization_id, person_id, filter) VALUES ($1, $2, $3)",
            member.organization_id,
            id,
            JsonColumn(filter) as _
        )
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    super::record_audit_event(
        &state,
        member.organization_id,
        Some(member.person_id),
        "member.filters_changed",
        Some(id),
    );
    Ok(Json(
        super::member(&state.db, member.organization_id, id).await?,
    ))
}

/// What an invite link is for, before the person signs in: the page fills
/// in the email and asks for a sign-in link.
#[utoipa::path(
    get,
    path = "/invites/{code}",
    tag = "members",
    params(("code" = String, Path, description = "The part after /i/ in the invite link")),
    responses(
        (status = OK, body = InviteView),
        (status = NOT_FOUND, body = crate::error::ErrorBody),
    ),
)]
pub(super) async fn show_invite(
    State(state): State<AppState>,
    Path(code): Path<String>,
) -> Result<Json<InviteView>, ApiError> {
    sqlx::query_as!(
        InviteView,
        r#"SELECT o.name AS organization_name, p.email
           FROM invites i
           JOIN people p ON p.id = i.person_id AND p.removed_at IS NULL
           JOIN organizations o ON o.id = i.organization_id
           WHERE i.code = $1"#,
        code
    )
    .fetch_optional(&state.db)
    .await?
    .map(Json)
    .ok_or(ApiError::NotFound)
}

/// Checks a person's name and trims it. It may be empty.
///
/// # Errors
///
/// [`ApiError::BadRequest`] when it is longer than 100 characters.
pub fn clean_name(name: &str) -> Result<String, ApiError> {
    let name = name.trim();
    if name.chars().count() > 100 {
        return Err(ApiError::BadRequest(
            "The name is longer than 100 characters.".to_owned(),
        ));
    }
    Ok(name.to_owned())
}
