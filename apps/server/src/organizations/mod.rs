//! Organizations (one account is one organization), members and invites,
//! roles (agent manager, people manager, org manager), organization
//! settings, rules and the context local tools read.
//!
//! Docs: Account > Organization settings, Account > Grants and roles.
//!
//! Owner: the service platform work. Keep this area's code, routes and queries in
//! this folder, and its migrations in `migrations/` named
//! `<timestamp>_organizations_<what>.sql`.

mod audit;
mod context;
mod members;
mod provisioning;
mod role;
pub mod rules;
mod settings;

use serde::Serialize;
use sqlx::{PgConnection, PgPool};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::AppState;
use crate::error::ApiError;
use crate::tags;

pub use audit::record_audit_event;
pub use context::Context;
pub use members::{MemberLimitView, MemberStatus, MemberView, clean_name, person_of};
pub use provisioning::{add_member, member_by_email, only_organization, remove_member};
pub use role::{Role, UnknownRole};
pub use rules::{RuleView, parse_rule};
pub(crate) use settings::validate_currency;
pub use settings::{Organization, reporting_currency};

/// The longest organization name.
const MAX_NAME_CHARS: usize = 100;

/// This area's routes, relative to `/api/v1`.
#[must_use]
pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(
            settings::get_organization,
            settings::update_organization
        ))
        .routes(routes!(members::list_members, members::invite_member))
        .routes(routes!(members::get_member_limit))
        .routes(routes!(members::update_member, members::remove_member))
        .routes(routes!(members::set_filters))
        .routes(routes!(members::resend_invite))
        .routes(routes!(members::show_invite))
        .routes(routes!(rules::list_rules, rules::add_rule))
        .routes(routes!(rules::get_my_rules, rules::set_my_rules))
        .routes(routes!(context::context))
}

/// An organization's name and short name.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct OrganizationSummary {
    pub id: Uuid,
    pub name: String,
    /// Used in invite links, such as `kestrel` in `/i/kestrel-4f2a9c1e7b03`.
    pub slug: String,
}

/// A new organization with its first member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Created {
    pub organization_id: Uuid,
    /// The first member: an org manager, who sets the organization up.
    pub person_id: Uuid,
}

/// Creates an organization with `email` as its first member, an org
/// manager with the role's grants, and the built-in tag and attribute
/// names.
///
/// # Errors
///
/// Fails if a query fails.
pub async fn create(conn: &mut PgConnection, name: &str, email: &str) -> Result<Created, ApiError> {
    let slug = free_slug(&mut *conn, name).await?;
    let organization_id = sqlx::query_scalar!(
        "INSERT INTO organizations (name, slug) VALUES ($1, $2) RETURNING id",
        name,
        slug
    )
    .fetch_one(&mut *conn)
    .await?;
    let person_id = sqlx::query_scalar!(
        r#"INSERT INTO people (organization_id, email, role, joined_at)
           VALUES ($1, $2, 'org_manager', now()) RETURNING id"#,
        organization_id,
        email
    )
    .fetch_one(&mut *conn)
    .await?;
    tags::create_defaults(&mut *conn, organization_id).await?;
    let log = crate::events::Log {
        organization_id,
        events: Vec::new(),
    };
    crate::grants::set_role_grants(
        &mut *conn,
        &log,
        person_id,
        &crate::events::person(email)?,
        Role::OrgManager,
        &[],
        None,
    )
    .await?;
    tracing::info!(%organization_id, "organization created");
    Ok(Created {
        organization_id,
        person_id,
    })
}

/// Checks an organization name.
///
/// # Errors
///
/// [`ApiError::BadRequest`] when it is empty or too long.
pub fn validate_name(name: &str) -> Result<String, ApiError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
        return Err(ApiError::BadRequest(format!(
            "The organization's name must have 1 to {MAX_NAME_CHARS} characters."
        )));
    }
    Ok(name.to_owned())
}

/// The short name for invite links: lower-case letters and digits from the
/// name, such as `kestrel-digital`.
#[must_use]
pub fn slug_of(name: &str) -> String {
    let mut slug = String::new();
    for character in name.to_lowercase().chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
        if slug.len() >= 30 {
            break;
        }
    }
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() {
        "org".to_owned()
    } else {
        slug.to_owned()
    }
}

async fn free_slug(conn: &mut PgConnection, name: &str) -> Result<String, ApiError> {
    let base = slug_of(name);
    let mut candidate = base.clone();
    for _attempt in 0..5 {
        let taken = sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM organizations WHERE slug = $1) AS "taken!""#,
            candidate
        )
        .fetch_one(&mut *conn)
        .await?;
        if !taken {
            return Ok(candidate);
        }
        let suffix = crate::sign_in::secret::short_code()
            .map_err(|error| ApiError::Internal(anyhow::anyhow!("no randomness: {error}")))?;
        candidate = format!("{base}-{}", suffix.chars().take(4).collect::<String>());
    }
    Err(ApiError::Conflict(
        "Could not find a free short name; try another name.".to_owned(),
    ))
}

/// An organization's name and short name.
///
/// # Errors
///
/// Fails if the organization does not exist or a query fails.
pub async fn summary(db: &PgPool, organization_id: Uuid) -> Result<OrganizationSummary, ApiError> {
    sqlx::query_as!(
        OrganizationSummary,
        "SELECT id, name, slug FROM organizations WHERE id = $1",
        organization_id
    )
    .fetch_optional(db)
    .await?
    .ok_or(ApiError::NotFound)
}

/// One member as the API shows them.
///
/// # Errors
///
/// [`ApiError::NotFound`] if there is no such person in the organization.
pub async fn member(
    state: &AppState,
    organization_id: Uuid,
    person_id: Uuid,
) -> Result<MemberView, ApiError> {
    members::list(state, organization_id, Some(person_id), true)
        .await?
        .into_iter()
        .next()
        .ok_or(ApiError::NotFound)
}

/// Everyone in the organization who has not left.
///
/// # Errors
///
/// Fails if a query fails.
pub async fn members(state: &AppState, organization_id: Uuid) -> Result<Vec<MemberView>, ApiError> {
    members::list(state, organization_id, None, false).await
}

/// An organization's settings.
///
/// # Errors
///
/// Fails if the organization does not exist or a query fails.
pub async fn organization(db: &PgPool, organization_id: Uuid) -> Result<Organization, ApiError> {
    settings::load(db, organization_id).await
}

/// The number of members: everyone invited or joined who has not left.
///
/// # Errors
///
/// Fails if a query fails.
pub async fn member_count(db: &PgPool, organization_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        r#"SELECT count(*) AS "count!" FROM people WHERE organization_id = $1 AND removed_at IS NULL"#,
        organization_id
    )
    .fetch_one(db)
    .await
}

/// The member limit of an organization: what the build's hooks say, such as
/// the hosted service's plan. The open build and the Enterprise plan have no
/// member limit (docs: Plans and limits).
///
/// # Errors
///
/// Whatever the hook fails with.
pub async fn member_limit(
    state: &AppState,
    organization_id: Uuid,
) -> Result<Option<crate::hooks::MemberLimit>, ApiError> {
    state.hooks.member_limit(state, organization_id).await
}

/// Refuses one more member when the organization's limit is reached.
///
/// # Errors
///
/// [`ApiError::Conflict`] with the limit's message, or whatever a query or
/// hook fails with.
pub async fn check_room_for_one_more(
    state: &AppState,
    organization_id: Uuid,
) -> Result<(), ApiError> {
    if let Some(limit) = member_limit(state, organization_id).await?
        && member_count(&state.db, organization_id).await? >= limit.max
    {
        return Err(ApiError::Conflict(limit.message));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_keep_letters_and_digits() {
        assert_eq!(slug_of("Kestrel Digital"), "kestrel-digital");
        assert_eq!(slug_of("  ACME & Co. GmbH "), "acme-co-gmbh");
        assert_eq!(slug_of("Ünïcode"), "n-code");
        assert_eq!(slug_of("!!!"), "org");
        assert_eq!(slug_of(&"a".repeat(50)).len(), 30);
    }

    #[test]
    fn names_need_one_to_a_hundred_characters() {
        assert_eq!(
            validate_name("  Kestrel Digital ").unwrap(),
            "Kestrel Digital"
        );
        assert!(validate_name("   ").is_err());
        assert!(validate_name(&"x".repeat(101)).is_err());
        assert!(validate_name(&"x".repeat(100)).is_ok());
    }
}
