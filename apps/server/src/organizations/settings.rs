//! Organization settings (docs: Account > Organization settings). Tag names,
//! attribute names, rules, the price table and the cutoff have their own
//! areas; this is the name, the reporting currency, approval and sign-in.

use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};
use time::{Date, OffsetDateTime};
use utoipa::ToSchema;
use uuid::Uuid;

use super::Role;
use crate::AppState;
use crate::error::ApiError;
use crate::events::Log;
use crate::sign_in::Member;

/// An organization and its settings.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Organization {
    pub id: Uuid,
    pub name: String,
    /// Used in invite links.
    pub slug: String,
    /// The one currency figures are shown in, such as `EUR`. The price
    /// table is in US dollars.
    pub reporting_currency: String,
    /// Whether submissions need approval.
    pub approval: bool,
    /// Email domains magic-link sign-in is limited to; empty means any.
    pub sign_in_domains: Vec<String>,
    /// The cutoff in force: figures up to it are frozen. `null` until an
    /// org manager sets one.
    #[serde(with = "crate::dates::option_day")]
    #[schema(value_type = Option<String>, format = Date)]
    pub cutoff: Option<Date>,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub created_at: OffsetDateTime,
}

/// A change to the settings. Fields left out stay as they are.
#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct OrganizationChange {
    pub name: Option<String>,
    pub reporting_currency: Option<String>,
    pub approval: Option<bool>,
    pub sign_in_domains: Option<Vec<String>>,
}

/// The organization's settings.
#[utoipa::path(
    get,
    path = "/organization",
    tag = "organization",
    responses((status = OK, body = Organization)),
)]
pub(super) async fn get_organization(
    State(state): State<AppState>,
    member: Member,
) -> Result<Json<Organization>, ApiError> {
    Ok(Json(
        load_with_cutoff(&state, member.organization_id).await?,
    ))
}

/// Change the organization's settings. Org managers only.
#[utoipa::path(
    patch,
    path = "/organization",
    tag = "organization",
    request_body = OrganizationChange,
    responses(
        (status = OK, body = Organization),
        (status = BAD_REQUEST, body = crate::error::ErrorBody),
        (status = FORBIDDEN, body = crate::error::ErrorBody),
    ),
)]
pub(super) async fn update_organization(
    State(state): State<AppState>,
    member: Member,
    Json(change): Json<OrganizationChange>,
) -> Result<Json<Organization>, ApiError> {
    member.require_session()?;
    member.require_role(Role::OrgManager)?;
    let name = change
        .name
        .as_deref()
        .map(super::validate_name)
        .transpose()?;
    let currency = change
        .reporting_currency
        .as_deref()
        .map(validate_currency)
        .transpose()?;
    let domains = change
        .sign_in_domains
        .as_deref()
        .map(validate_domains)
        .transpose()?;
    sqlx::query!(
        r#"UPDATE organizations SET
             name = coalesce($2, name),
             reporting_currency = coalesce($3, reporting_currency),
             approval = coalesce($4, approval),
             sign_in_domains = coalesce($5, sign_in_domains)
           WHERE id = $1"#,
        member.organization_id,
        name,
        currency,
        change.approval,
        domains.as_deref()
    )
    .execute(&state.db)
    .await?;
    super::record_audit_event(
        &state,
        member.organization_id,
        Some(member.person_id),
        "organization.settings_changed",
        None,
    );
    Ok(Json(
        load_with_cutoff(&state, member.organization_id).await?,
    ))
}

async fn load_with_cutoff(
    state: &AppState,
    organization_id: Uuid,
) -> Result<Organization, ApiError> {
    let mut organization = load(&state.db, organization_id).await?;
    let log = Log::load(state, organization_id).await?;
    organization.cutoff = log.fold(OffsetDateTime::now_utc())?.cutoff();
    Ok(organization)
}

pub(super) async fn load(
    db: &sqlx::PgPool,
    organization_id: Uuid,
) -> Result<Organization, ApiError> {
    let row = sqlx::query!(
        r#"SELECT id, name, slug, reporting_currency, approval, sign_in_domains, created_at
           FROM organizations WHERE id = $1"#,
        organization_id
    )
    .fetch_optional(db)
    .await?
    .ok_or(ApiError::NotFound)?;
    Ok(Organization {
        id: row.id,
        name: row.name,
        slug: row.slug,
        reporting_currency: row.reporting_currency,
        approval: row.approval,
        sign_in_domains: row.sign_in_domains,
        cutoff: None,
        created_at: row.created_at,
    })
}

/// The organization's reporting currency.
///
/// # Errors
///
/// Fails if the organization does not exist or a query fails.
pub async fn reporting_currency(
    db: &sqlx::PgPool,
    organization_id: Uuid,
) -> Result<tokencontroller_core::Currency, ApiError> {
    let code = sqlx::query_scalar!(
        "SELECT reporting_currency FROM organizations WHERE id = $1",
        organization_id
    )
    .fetch_optional(db)
    .await?
    .ok_or(ApiError::NotFound)?;
    tokencontroller_core::Currency::new(&code)
        .map_err(|error| ApiError::Internal(anyhow::anyhow!("stored currency {code}: {error}")))
}

pub(crate) fn validate_currency(code: &str) -> Result<String, ApiError> {
    let code = code.trim().to_uppercase();
    tokencontroller_core::Currency::new(&code)
        .map(tokencontroller_core::Currency::code)
        .map_err(|_| {
            ApiError::BadRequest(format!(
                "{code:?} is not a currency code; use three letters such as EUR or USD."
            ))
        })
}

fn validate_domains(domains: &[String]) -> Result<Vec<String>, ApiError> {
    let mut checked: Vec<String> = Vec::new();
    for domain in domains {
        let domain = domain.trim().trim_start_matches('@').to_lowercase();
        let valid = domain.contains('.')
            && !domain.starts_with('.')
            && !domain.ends_with('.')
            && domain
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
        if !valid {
            return Err(ApiError::BadRequest(format!(
                "{domain:?} is not an email domain."
            )));
        }
        if !checked.contains(&domain) {
            checked.push(domain);
        }
    }
    Ok(checked)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn currencies_are_three_letters() {
        assert_eq!(validate_currency(" eur ").unwrap(), "EUR");
        assert!(validate_currency("EURO").is_err());
        assert!(validate_currency("E1R").is_err());
    }

    #[test]
    fn domains_are_cleaned_and_checked() {
        let domains = vec!["@Kestrel.Example".to_owned(), "kestrel.example".to_owned()];
        assert_eq!(validate_domains(&domains).unwrap(), vec!["kestrel.example"]);
        assert!(validate_domains(&["kestrel".to_owned()]).is_err());
        assert!(validate_domains(&["kes trel.example".to_owned()]).is_err());
    }

    #[test]
    fn a_change_leaves_out_what_it_does_not_name() {
        let change: OrganizationChange = serde_json::from_str(r#"{"approval": true}"#).unwrap();
        assert_eq!(change.approval, Some(true));
        assert_eq!(change.name, None);
        assert_eq!(change.reporting_currency, None);
    }
}
