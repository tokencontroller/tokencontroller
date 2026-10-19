//! Members managed by an identity provider: what the Enterprise plan's SCIM
//! and OIDC sign-in call instead of invites. People added here get no
//! invite mail; they sign in through their identity provider or by magic
//! link.

use sqlx::PgPool;
use uuid::Uuid;

use super::audit::record_audit_event;
use super::members::{clean_name, keep_an_org_manager};
use super::{MemberStatus, member};
use crate::AppState;
use crate::error::ApiError;
use crate::sign_in::normalize_email;

/// The organization the instance serves when it serves exactly one, as an
/// Enterprise plan instance does. `None` before anyone signed up, and on an
/// instance with several organizations, such as the hosted service.
///
/// # Errors
///
/// Fails if a query fails.
pub async fn only_organization(db: &PgPool) -> Result<Option<Uuid>, ApiError> {
    let ids = sqlx::query_scalar!("SELECT id FROM organizations LIMIT 2")
        .fetch_all(db)
        .await?;
    Ok(match ids.as_slice() {
        [id] => Some(*id),
        _ => None,
    })
}

/// The member of the organization with this email address, if they have
/// not been removed.
///
/// # Errors
///
/// [`ApiError::BadRequest`] for something that is not an email address, or
/// a failed query.
pub async fn member_by_email(
    db: &PgPool,
    organization_id: Uuid,
    email: &str,
) -> Result<Option<Uuid>, ApiError> {
    let email = normalize_email(email)?;
    Ok(sqlx::query_scalar!(
        "SELECT id FROM people WHERE organization_id = $1 AND email = $2 AND removed_at IS NULL",
        organization_id,
        email
    )
    .fetch_optional(db)
    .await?)
}

/// Makes the person a member and returns their ID. A member who was removed
/// comes back with their old role; a new one is an agent manager with the
/// role's grants. Someone who already is a member keeps everything and gets
/// their ID back. `display_name` replaces the name when given.
///
/// # Errors
///
/// - [`ApiError::BadRequest`] for an address that is not one, or a name over
///   100 characters.
/// - [`ApiError::Conflict`] when the person is a member of another
///   organization, or this one is at its member limit.
pub async fn add_member(
    state: &AppState,
    organization_id: Uuid,
    email: &str,
    display_name: Option<&str>,
) -> Result<Uuid, ApiError> {
    let email = normalize_email(email)?;
    let name = display_name.map(clean_name).transpose()?;
    let active = sqlx::query!(
        "SELECT id, organization_id FROM people WHERE email = $1 AND removed_at IS NULL",
        email
    )
    .fetch_optional(&state.db)
    .await?;
    if let Some(active) = active {
        if active.organization_id != organization_id {
            return Err(ApiError::Conflict(format!(
                "{email} is a member of another organization. One account belongs to one organization."
            )));
        }
        if let Some(name) = name {
            sqlx::query!("UPDATE people SET name = $2 WHERE id = $1", active.id, name)
                .execute(&state.db)
                .await?;
        }
        return Ok(active.id);
    }
    super::check_room_for_one_more(state, organization_id).await?;
    let removed = sqlx::query_scalar!(
        r#"SELECT id FROM people
           WHERE organization_id = $1 AND email = $2 AND removed_at IS NOT NULL
           ORDER BY removed_at DESC LIMIT 1"#,
        organization_id,
        email
    )
    .fetch_optional(&state.db)
    .await?;
    let (person_id, action) = if let Some(id) = removed {
        sqlx::query!(
            "UPDATE people SET removed_at = NULL, name = coalesce($2, name) WHERE id = $1",
            id,
            name
        )
        .execute(&state.db)
        .await?;
        (id, "member.restored")
    } else {
        let mut tx = state.db.begin().await?;
        let id = sqlx::query_scalar!(
            "INSERT INTO people (organization_id, email, name) VALUES ($1, $2, $3) RETURNING id",
            organization_id,
            email,
            name.unwrap_or_default()
        )
        .fetch_one(&mut *tx)
        .await?;
        let log = crate::events::Log::load(state, organization_id).await?;
        crate::grants::set_role_grants(
            &mut tx,
            &log,
            id,
            &crate::events::person(&email)?,
            super::Role::AgentManager,
            &[],
            None,
        )
        .await?;
        tx.commit().await?;
        (id, "member.added")
    };
    record_audit_event(state, organization_id, None, action, Some(person_id));
    Ok(person_id)
}

/// Removes a member: they are signed out and can no longer sign in, and
/// their past messages keep them as their person. `actor` is the member who
/// removed them, or `None` for an identity provider.
///
/// # Errors
///
/// - [`ApiError::NotFound`] if they are not a member of the organization.
/// - [`ApiError::Conflict`] if they are its last org manager.
pub async fn remove_member(
    state: &AppState,
    organization_id: Uuid,
    person_id: Uuid,
    actor: Option<Uuid>,
) -> Result<(), ApiError> {
    let current = member(state, organization_id, person_id).await?;
    if current.status == MemberStatus::Removed {
        return Err(ApiError::NotFound);
    }
    if current.role == super::Role::OrgManager {
        keep_an_org_manager(&state.db, organization_id, person_id).await?;
    }
    let mut tx = state.db.begin().await?;
    sqlx::query!(
        "UPDATE people SET removed_at = now() WHERE id = $1",
        person_id
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!("DELETE FROM sessions WHERE person_id = $1", person_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query!("DELETE FROM invites WHERE person_id = $1", person_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    record_audit_event(
        state,
        organization_id,
        actor,
        "member.removed",
        Some(person_id),
    );
    Ok(())
}
