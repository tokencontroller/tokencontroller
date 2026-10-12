//! Organization settings (docs: Account > Organization settings). Ticket
//! systems, organization rules and the tag map have their own areas.

use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use super::Role;
use crate::AppState;
use crate::change::Change;
use crate::error::ApiError;
use crate::sign_in::Member;

/// How a provider charges the organization. It decides what the close
/// reconciles against (docs: Account > Bills).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CostSource {
    Subscription,
    Seat,
    UsageAtApiRates,
    ApiKey,
    CloudMarketplace,
    Reseller,
}

impl CostSource {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Subscription => "subscription",
            Self::Seat => "seat",
            Self::UsageAtApiRates => "usage_at_api_rates",
            Self::ApiKey => "api_key",
            Self::CloudMarketplace => "cloud_marketplace",
            Self::Reseller => "reseller",
        }
    }

    fn from_db(value: &str) -> Option<Self> {
        [
            Self::Subscription,
            Self::Seat,
            Self::UsageAtApiRates,
            Self::ApiKey,
            Self::CloudMarketplace,
            Self::Reseller,
        ]
        .into_iter()
        .find(|source| source.as_str() == value)
    }
}

impl From<CostSource> for tokencontroller_core::CostSource {
    fn from(source: CostSource) -> Self {
        match source {
            CostSource::Subscription => Self::Subscription,
            CostSource::Seat => Self::Seat,
            CostSource::UsageAtApiRates => Self::UsageAtApiRates,
            CostSource::ApiKey => Self::ApiKey,
            CostSource::CloudMarketplace => Self::CloudMarketplace,
            CostSource::Reseller => Self::Reseller,
        }
    }
}

/// An organization and its settings.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Organization {
    pub id: Uuid,
    pub name: String,
    /// Used in invite links.
    pub slug: String,
    /// How the provider charges; `null` until a controller picks one.
    pub cost_source: Option<CostSource>,
    /// The one currency the organization computes in, such as `EUR`.
    pub reporting_currency: String,
    /// Whether people managers approve token entries before the close.
    pub approval: bool,
    /// Email domains magic-link sign-in is limited to; empty means any.
    pub sign_in_domains: Vec<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

/// A change to the settings. Fields left out stay as they are.
#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct OrganizationChange {
    pub name: Option<String>,
    /// `null` sets it back to "none yet".
    #[serde(default)]
    #[schema(value_type = Option<CostSource>)]
    pub cost_source: Change<CostSource>,
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
    Ok(Json(load(&state.db, member.organization_id).await?))
}

/// Change the organization's settings. Controllers only.
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
    member.require_role(Role::Controller)?;
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
    let set_cost_source = change.cost_source.is_given();
    let cost_source = change.cost_source.into_value().map(CostSource::as_str);
    sqlx::query!(
        r#"UPDATE organizations SET
             name = coalesce($2, name),
             cost_source = CASE WHEN $3 THEN $4 ELSE cost_source END,
             reporting_currency = coalesce($5, reporting_currency),
             approval = coalesce($6, approval),
             sign_in_domains = coalesce($7, sign_in_domains)
           WHERE id = $1"#,
        member.organization_id,
        name,
        set_cost_source,
        cost_source,
        currency,
        change.approval,
        domains.as_deref()
    )
    .execute(&state.db)
    .await?;
    Ok(Json(load(&state.db, member.organization_id).await?))
}

pub(super) async fn load(
    db: &sqlx::PgPool,
    organization_id: Uuid,
) -> Result<Organization, ApiError> {
    let row = sqlx::query!(
        r#"SELECT id, name, slug, cost_source, reporting_currency, approval, sign_in_domains, created_at
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
        cost_source: row.cost_source.as_deref().and_then(CostSource::from_db),
        reporting_currency: row.reporting_currency,
        approval: row.approval,
        sign_in_domains: row.sign_in_domains,
        created_at: row.created_at,
    })
}

fn validate_currency(code: &str) -> Result<String, ApiError> {
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
    fn cost_sources_are_the_core_ones() {
        for source in [
            CostSource::Subscription,
            CostSource::Seat,
            CostSource::UsageAtApiRates,
            CostSource::ApiKey,
            CostSource::CloudMarketplace,
            CostSource::Reseller,
        ] {
            let core = tokencontroller_core::CostSource::from(source);
            assert_eq!(
                serde_json::to_value(source).unwrap(),
                serde_json::to_value(core).unwrap()
            );
            assert_eq!(CostSource::from_db(source.as_str()), Some(source));
        }
    }

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
    fn cost_sources_round_trip() {
        for source in [
            CostSource::Seat,
            CostSource::UsageAtApiRates,
            CostSource::Reseller,
        ] {
            assert_eq!(CostSource::from_db(source.as_str()), Some(source));
        }
        assert_eq!(CostSource::from_db("barter"), None);
    }

    #[test]
    fn null_and_missing_are_different_changes() {
        let missing: OrganizationChange = serde_json::from_str("{}").unwrap();
        assert_eq!(missing.cost_source, Change::Keep);
        let null: OrganizationChange = serde_json::from_str(r#"{"cost_source": null}"#).unwrap();
        assert_eq!(null.cost_source, Change::Clear);
        let seat: OrganizationChange = serde_json::from_str(r#"{"cost_source": "seat"}"#).unwrap();
        assert_eq!(seat.cost_source, Change::Set(CostSource::Seat));
    }
}
