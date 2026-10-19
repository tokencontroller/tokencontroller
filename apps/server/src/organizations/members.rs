//! Members, invites, roles and the filters a people manager is granted
//! (docs: Account > Organization settings, Account > Grants and roles).
//! A role is written as grants in the log; a member's attributes come
//! from the fold.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use tokencontroller_core::{Filter, PersonAttribute, Record, TagValue};
use utoipa::ToSchema;
use uuid::Uuid;

use super::Role;
use crate::error::ApiError;
use crate::events::json::{self, TagsSchema};
use crate::events::{self, Log, access};
use crate::mail::{self, Mail, render};
use crate::sign_in::{Member, domain_allowed, normalize_email, secret};
use crate::{AppState, Config, grants, tags};

const INVITE_HTML: &str = include_str!("../sign_in/mail/invite.html");
const INVITE_TEXT: &str = include_str!("../sign_in/mail/invite.txt");

/// Where a member stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemberStatus {
    /// Invited and not signed in yet.
    Invited,
    Joined,
    /// Left the organization. Kept so their past messages keep their
    /// person.
    Removed,
}

/// A member as the API shows them.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct MemberView {
    pub id: Uuid,
    /// The address they sign in with: the tag value `person` on their
    /// messages.
    pub email: String,
    pub name: String,
    pub role: Role,
    /// Their attributes today, such as `{"team": "web"}`.
    #[schema(value_type = TagsSchema)]
    pub attributes: BTreeMap<String, String>,
    /// For a people manager: the filters their role was granted on, such as
    /// `{"team": "web"}` or `{"client": "acme"}`.
    #[schema(value_type = Vec<TagsSchema>)]
    pub filters: Vec<BTreeMap<String, String>>,
    pub status: MemberStatus,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub invited_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    #[schema(value_type = Option<String>, format = DateTime)]
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
    /// Their attributes, such as `{"team": "web"}`; each a value on record.
    #[serde(default)]
    #[schema(value_type = Option<TagsSchema>)]
    pub attributes: Option<BTreeMap<String, String>>,
    /// For a people manager: the filters to grant the role on.
    #[serde(default)]
    #[schema(value_type = Vec<TagsSchema>)]
    pub filters: Vec<BTreeMap<String, String>>,
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

/// A change to a member. Fields left out stay as they are.
#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct MemberChange {
    pub name: Option<String>,
    /// The address they sign in with. It must not belong to anyone else.
    pub email: Option<String>,
    /// A new role replaces the grants of the old one.
    pub role: Option<Role>,
    /// For a people manager: the filters to grant the role on. Replaces
    /// the earlier filters.
    #[serde(default)]
    #[schema(value_type = Option<Vec<TagsSchema>>)]
    pub filters: Option<Vec<BTreeMap<String, String>>>,
}

/// What an invite link shows before sign-in.
#[derive(Debug, Serialize, ToSchema)]
pub struct InviteView {
    pub organization_name: String,
    /// The address the invite went to; sign in with it.
    pub email: String,
}

pub(super) async fn list(
    state: &AppState,
    organization_id: Uuid,
    only: Option<Uuid>,
    include_removed: bool,
) -> Result<Vec<MemberView>, ApiError> {
    let rows = sqlx::query!(
        r#"SELECT id, email, name, role, invited_at, joined_at, removed_at
           FROM people
           WHERE organization_id = $1
             AND ($2::uuid IS NULL OR id = $2)
             AND ($3 OR removed_at IS NULL)
           ORDER BY removed_at IS NOT NULL, lower(name), email"#,
        organization_id,
        only,
        include_removed
    )
    .fetch_all(&state.db)
    .await?;
    let log = Log::load(state, organization_id).await?;
    let fold = log.fold(OffsetDateTime::now_utc())?;
    let today = tokencontroller_core::price::date_of(OffsetDateTime::now_utc());
    rows.into_iter()
        .map(|row| {
            let status = match (row.removed_at, row.joined_at) {
                (Some(_), _) => MemberStatus::Removed,
                (None, Some(_)) => MemberStatus::Joined,
                (None, None) => MemberStatus::Invited,
            };
            let person = events::person(&row.email)?;
            Ok(MemberView {
                id: row.id,
                attributes: tags::attributes::attributes_of(&fold, &person, today),
                filters: grants::role_filters(&log, row.id)
                    .iter()
                    .map(json::tags_map)
                    .collect(),
                email: row.email,
                name: row.name,
                role: row
                    .role
                    .parse()
                    .map_err(|error: super::UnknownRole| ApiError::Internal(error.into()))?,
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
        list(&state, member.organization_id, None, false).await?,
    ))
}

/// How many members the organization has and may have (docs: Plans and
/// limits).
#[derive(Debug, Serialize, ToSchema)]
pub struct MemberLimitView {
    /// Everyone invited or joined who has not left. Every member counts the
    /// same, whatever their role.
    pub members: i64,
    /// The most members the organization may have, such as a hosted plan's.
    /// `null` when there is no limit, as on the open build.
    #[schema(required = true)]
    pub member_limit: Option<i64>,
    /// What an invite past the limit is refused with, such as "This instance
    /// allows no more members." `null` when there is no limit.
    #[schema(required = true)]
    pub message: Option<String>,
}

/// The organization's member count and member limit.
#[utoipa::path(
    get,
    path = "/organization/member-limit",
    tag = "members",
    responses((status = OK, body = MemberLimitView)),
)]
pub(super) async fn get_member_limit(
    State(state): State<AppState>,
    member: Member,
) -> Result<Json<MemberLimitView>, ApiError> {
    let limit = super::member_limit(&state, member.organization_id).await?;
    let (member_limit, message) = limit.map(|limit| (limit.max, limit.message)).unzip();
    Ok(Json(MemberLimitView {
        members: super::member_count(&state.db, member.organization_id).await?,
        member_limit,
        message,
    }))
}

/// Checks the filters a people manager is granted: each a non-empty set
/// of known names with values on record.
async fn check_filters(
    state: &AppState,
    fold: &tokencontroller_core::state::State,
    organization_id: Uuid,
    role: Role,
    filters: &[BTreeMap<String, String>],
) -> Result<Vec<Filter>, ApiError> {
    if role != Role::PeopleManager && !filters.is_empty() {
        return Err(ApiError::BadRequest(
            "Only a people manager is granted filters. An org manager holds everything; an agent manager their own messages.".to_owned(),
        ));
    }
    let mut checked: Vec<Filter> = Vec::with_capacity(filters.len());
    for filter in filters {
        let tags = json::tags(filter)?;
        if tags.is_empty() {
            return Err(ApiError::BadRequest(
                "A filter needs at least one tag or attribute.".to_owned(),
            ));
        }
        tags::check_names(&state.db, organization_id, &tags).await?;
        let unknown = tags::unknown_values(fold, &tags);
        if let Some(first) = unknown.first() {
            return Err(ApiError::BadRequest(format!(
                "{first} is not a value on record; add it first"
            )));
        }
        let filter = Filter(tags);
        if !checked.contains(&filter) {
            checked.push(filter);
        }
    }
    Ok(checked)
}

/// Checks a member's attributes: known attribute names with values on
/// record.
async fn check_attributes(
    state: &AppState,
    fold: &tokencontroller_core::state::State,
    organization_id: Uuid,
    attributes: &BTreeMap<String, String>,
) -> Result<tokencontroller_core::Tags, ApiError> {
    let tags = json::tags(attributes)?;
    for (name, key) in tags.iter() {
        if tags::kind_of(&state.db, organization_id, name.as_str()).await?
            != Some(tags::Kind::Attribute)
        {
            return Err(ApiError::BadRequest(format!(
                "{name} is not an attribute name of the organization"
            )));
        }
        if fold.values().attribute_value(name, key).is_none() {
            return Err(ApiError::BadRequest(format!(
                "{name}: {key} is not a value on record; add it first"
            )));
        }
    }
    Ok(tags)
}

/// Checks that `email` may be invited into the organization and gives the
/// invite code: the address must be inside the sign-in domains, belong to
/// nobody yet, and the organization must have room for one more.
async fn invite_code_for(
    state: &AppState,
    organization_id: Uuid,
    email: &str,
) -> Result<String, ApiError> {
    let organization = sqlx::query!(
        "SELECT slug, sign_in_domains FROM organizations WHERE id = $1",
        organization_id
    )
    .fetch_one(&state.db)
    .await?;
    if !domain_allowed(email, &organization.sign_in_domains) {
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
        Some(found) if found == organization_id => {
            return Err(ApiError::Conflict(format!("{email} is already a member.")));
        }
        Some(_) => {
            return Err(ApiError::Conflict(format!(
                "{email} is a member of another organization. One account belongs to one organization."
            )));
        }
        None => {}
    }
    super::check_room_for_one_more(state, organization_id).await?;
    Ok(format!(
        "{}-{}",
        organization.slug,
        secret::short_code()
            .map_err(|error| ApiError::Internal(anyhow::anyhow!("no randomness: {error}")))?
    ))
}

/// Invite someone by email. They get a link that signs them in. Org
/// managers only.
#[utoipa::path(
    post,
    path = "/members",
    tag = "members",
    request_body = NewMember,
    responses(
        (status = CREATED, body = Invited),
        (status = BAD_REQUEST, body = crate::error::ErrorBody),
        (status = CONFLICT, description = "Already a member, or the member limit is reached", body = crate::error::ErrorBody),
    ),
)]
pub(super) async fn invite_member(
    State(state): State<AppState>,
    member: Member,
    Json(request): Json<NewMember>,
) -> Result<(StatusCode, Json<Invited>), ApiError> {
    member.require_session()?;
    let log = Log::load(&state, member.organization_id).await?;
    let fold = log.fold(OffsetDateTime::now_utc())?;
    let caller = member.person()?;
    access::require_org_manager(&fold, &caller)?;
    let email = normalize_email(&request.email)?;
    let name = clean_name(&request.name)?;
    let filters = check_filters(
        &state,
        &fold,
        member.organization_id,
        request.role,
        &request.filters,
    )
    .await?;
    let attributes = check_attributes(
        &state,
        &fold,
        member.organization_id,
        request.attributes.as_ref().unwrap_or(&BTreeMap::new()),
    )
    .await?;

    let code = invite_code_for(&state, member.organization_id, &email).await?;
    let person = events::person(&email)?;
    let mut tx = state.db.begin().await?;
    let person_id = sqlx::query_scalar!(
        r#"INSERT INTO people (organization_id, email, name, role)
           VALUES ($1, $2, $3, $4) RETURNING id"#,
        member.organization_id,
        email,
        name,
        request.role.as_str()
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
    grants::set_role_grants(
        &mut tx,
        &log,
        person_id,
        &person,
        request.role,
        &filters,
        Some(&caller),
    )
    .await?;
    for (attribute, key) in attributes.iter() {
        let event = events::event(Record::PersonAttribute(PersonAttribute {
            person: person.clone(),
            name: attribute.clone(),
            key: key.clone(),
            from: None,
        }))?
        .by(caller.clone());
        events::append(&mut tx, member.organization_id, &event).await?;
    }
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
    let invited = super::member(&state, member.organization_id, person_id).await?;
    Ok((
        StatusCode::CREATED,
        Json(Invited {
            member: invited,
            invite_url,
        }),
    ))
}

/// Send the invite mail again to someone who has not signed in yet. Org
/// managers only.
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
    member.require_role(Role::OrgManager)?;
    let invited = super::member(&state, member.organization_id, id).await?;
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
    let logo = mail::logo_url(&state.config.public_url);
    let values = [
        ("logo_url", logo.as_str()),
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
    // The invite exists either way; an org manager can send it again.
    if let Err(error) = mail::send(&state.db, &state.config, invite).await {
        tracing::warn!("invite mail failed: {error:#}");
    }
    Ok(())
}

/// Change a member's name, email, role or filters. A new role or new
/// filters replace the grants the old ones gave. Org managers only.
#[utoipa::path(
    patch,
    path = "/members/{id}",
    tag = "members",
    params(("id" = Uuid, Path)),
    request_body = MemberChange,
    responses(
        (status = OK, body = MemberView),
        (status = CONFLICT, description = "The organization would have no org manager", body = crate::error::ErrorBody),
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
    let log = Log::load(&state, member.organization_id).await?;
    let fold = log.fold(OffsetDateTime::now_utc())?;
    let caller = member.person()?;
    access::require_org_manager(&fold, &caller)?;
    let current = super::member(&state, member.organization_id, id).await?;
    if current.status == MemberStatus::Removed {
        return Err(ApiError::NotFound);
    }
    let role = change.role.unwrap_or(current.role);
    if current.role == Role::OrgManager && role != Role::OrgManager {
        keep_an_org_manager(&state.db, member.organization_id, id).await?;
    }
    let name = change.name.as_deref().map(clean_name).transpose()?;
    let email = match change.email.as_deref() {
        Some(email) => Some(check_new_email(&state.db, member.organization_id, id, email).await?),
        None => None,
    };
    // A new role or a new address rewrites the grants: they name the
    // person by email.
    let regrant =
        change.role.is_some() || email.as_deref().is_some_and(|new| *new != current.email);
    let filters = match &change.filters {
        Some(filters) => {
            Some(check_filters(&state, &fold, member.organization_id, role, filters).await?)
        }
        None if regrant => Some(
            check_filters(
                &state,
                &fold,
                member.organization_id,
                role,
                &current.filters,
            )
            .await
            .unwrap_or_default(),
        ),
        None => None,
    };
    let mut tx = state.db.begin().await?;
    sqlx::query!(
        r#"UPDATE people SET
             name = coalesce($3, name),
             role = coalesce($4, role),
             email = coalesce($5, email)
           WHERE id = $1 AND organization_id = $2"#,
        id,
        member.organization_id,
        name,
        change.role.map(Role::as_str),
        email
    )
    .execute(&mut *tx)
    .await?;
    if let Some(filters) = filters {
        let person = events::person(email.as_deref().unwrap_or(&current.email))?;
        grants::set_role_grants(&mut tx, &log, id, &person, role, &filters, Some(&caller)).await?;
    }
    tx.commit().await?;
    super::record_audit_event(
        &state,
        member.organization_id,
        Some(member.person_id),
        "member.changed",
        Some(id),
    );
    Ok(Json(
        super::member(&state, member.organization_id, id).await?,
    ))
}

/// A new address for a member, checked like an invite's: well formed, inside
/// the organization's sign-in domains and not used by anyone else.
async fn check_new_email(
    db: &PgPool,
    organization_id: Uuid,
    person_id: Uuid,
    email: &str,
) -> Result<String, ApiError> {
    let email = normalize_email(email)?;
    let domains = sqlx::query_scalar!(
        "SELECT sign_in_domains FROM organizations WHERE id = $1",
        organization_id
    )
    .fetch_one(db)
    .await?;
    if !domain_allowed(&email, &domains) {
        return Err(ApiError::BadRequest(format!(
            "Sign-in is limited to {}. Change the sign-in domains first.",
            domains.join(", ")
        )));
    }
    let holder = sqlx::query!(
        "SELECT id, organization_id FROM people WHERE email = $1 AND removed_at IS NULL",
        email
    )
    .fetch_optional(db)
    .await?;
    match holder {
        Some(holder) if holder.id == person_id => {}
        Some(holder) if holder.organization_id == organization_id => {
            return Err(ApiError::Conflict(format!("{email} is already a member.")));
        }
        Some(_) => {
            return Err(ApiError::Conflict(format!(
                "{email} is a member of another organization. One account belongs to one organization."
            )));
        }
        None => {}
    }
    Ok(email)
}

/// Remove a member. They can no longer sign in; their past messages keep
/// them as their person, and their service tokens wait for a new owner.
/// Org managers only.
#[utoipa::path(
    delete,
    path = "/members/{id}",
    tag = "members",
    params(("id" = Uuid, Path)),
    responses(
        (status = NO_CONTENT, description = "Removed"),
        (status = CONFLICT, description = "The organization would have no org manager", body = crate::error::ErrorBody),
        (status = NOT_FOUND, body = crate::error::ErrorBody),
    ),
)]
pub(super) async fn remove_member(
    State(state): State<AppState>,
    member: Member,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    member.require_session()?;
    member.require_role(Role::OrgManager)?;
    super::remove_member(&state, member.organization_id, id, Some(member.person_id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Refuses a change that would leave the organization without an org
/// manager.
///
/// # Errors
///
/// [`ApiError::Conflict`] when `leaving` is the last org manager.
pub(super) async fn keep_an_org_manager(
    db: &PgPool,
    organization_id: Uuid,
    leaving: Uuid,
) -> Result<(), ApiError> {
    let others = sqlx::query_scalar!(
        r#"SELECT count(*) AS "count!" FROM people
           WHERE organization_id = $1 AND role = 'org_manager' AND removed_at IS NULL AND id <> $2"#,
        organization_id,
        leaving
    )
    .fetch_one(db)
    .await?;
    if others == 0 {
        return Err(ApiError::Conflict(
            "The organization needs at least one org manager. Make someone else org manager first."
                .to_owned(),
        ));
    }
    Ok(())
}

/// Grant a people manager the filters they view, assign, approve and plan
/// on, such as `{"team": "web"}` and `{"client": "acme"}`. Replaces their
/// filters. Org managers only.
#[utoipa::path(
    put,
    path = "/members/{id}/filters",
    tag = "members",
    params(("id" = Uuid, Path)),
    request_body = Vec<TagsSchema>,
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
    Json(filters): Json<Vec<BTreeMap<String, String>>>,
) -> Result<Json<MemberView>, ApiError> {
    member.require_session()?;
    let log = Log::load(&state, member.organization_id).await?;
    let fold = log.fold(OffsetDateTime::now_utc())?;
    let caller = member.person()?;
    access::require_org_manager(&fold, &caller)?;
    let current = super::member(&state, member.organization_id, id).await?;
    if current.status == MemberStatus::Removed {
        return Err(ApiError::NotFound);
    }
    let filters = check_filters(
        &state,
        &fold,
        member.organization_id,
        current.role,
        &filters,
    )
    .await?;
    let person = events::person(&current.email)?;
    let mut tx = state.db.begin().await?;
    grants::set_role_grants(
        &mut tx,
        &log,
        id,
        &person,
        current.role,
        &filters,
        Some(&caller),
    )
    .await?;
    tx.commit().await?;
    super::record_audit_event(
        &state,
        member.organization_id,
        Some(member.person_id),
        "member.filters_changed",
        Some(id),
    );
    Ok(Json(
        super::member(&state, member.organization_id, id).await?,
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

/// The member with this email as the log names them.
///
/// # Errors
///
/// [`ApiError::NotFound`] when nobody in the organization has the address.
pub async fn person_of(
    db: &PgPool,
    organization_id: Uuid,
    person_id: Uuid,
) -> Result<TagValue, ApiError> {
    let email = sqlx::query_scalar!(
        "SELECT email FROM people WHERE organization_id = $1 AND id = $2",
        organization_id,
        person_id
    )
    .fetch_optional(db)
    .await?
    .ok_or(ApiError::NotFound)?;
    events::person(&email)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_trimmed_and_bounded() {
        assert_eq!(clean_name("  Mara Lindqvist ").unwrap(), "Mara Lindqvist");
        assert_eq!(clean_name("").unwrap(), "");
        assert!(clean_name(&"x".repeat(101)).is_err());
    }

    #[test]
    fn a_new_member_is_an_agent_manager_by_default() {
        let request: NewMember =
            serde_json::from_str(r#"{"email": "tom@kestrel.example"}"#).unwrap();
        assert_eq!(request.role, Role::AgentManager);
        assert!(request.filters.is_empty());
        assert!(request.attributes.is_none());
    }
}
