//! Creating an organization. Every way there takes one email:
//!
//! - The sign-up form posts `/sign-up`. The mail's link opens the link page,
//!   and its button creates the organization and signs the founder in.
//! - Asking to sign in with an address that has no account mails a link to
//!   the web app's sign-up form. The form reads the address from
//!   `GET /sign-up/link` and posts the organization to `POST /sign-up/link`,
//!   which signs the founder in at once.
//!
//! Either way the answers to the sign-up questions travel with the link, so
//! the organization is set up the same in whichever browser opens it.

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::links::{self, LinkSent, LinkToken};
use crate::AppState;
use crate::error::ApiError;
use crate::organizations::{self, CostSource};

/// What the founder manages. It picks the first steps the web app shows
/// after sign-up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Manages {
    Agents,
    People,
}

impl Manages {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Agents => "agents",
            Self::People => "people",
        }
    }

    pub(super) fn from_db(value: &str) -> Option<Self> {
        [Self::Agents, Self::People]
            .into_iter()
            .find(|manages| manages.as_str() == value)
    }

    /// Where the web app shows the first steps for this answer.
    pub(super) const fn start_page(self) -> &'static str {
        match self {
            Self::Agents => "/?first_steps=agents",
            Self::People => "/?first_steps=people",
        }
    }
}

/// The organization a sign-up creates, with the answers to the sign-up
/// questions.
#[derive(Debug, Deserialize, ToSchema)]
pub struct NewOrganization {
    /// Such as `Kestrel Digital`.
    pub organization_name: String,
    /// Your own name, as your colleagues will see it. Can be set later.
    #[serde(default)]
    pub name: Option<String>,
    /// How the provider charges. Left out, it stays "none yet".
    #[serde(default)]
    pub cost_source: Option<CostSource>,
    /// The one currency the organization computes in, such as `EUR`. Left
    /// out, it is `USD`.
    #[serde(default)]
    pub reporting_currency: Option<String>,
    /// Picks the first steps the web app shows after sign-up.
    #[serde(default)]
    pub manages: Option<Manages>,
}

/// Create an organization: the link signs in its first controller.
#[derive(Debug, Deserialize, ToSchema)]
pub struct SignUpRequest {
    pub email: String,
    #[serde(flatten)]
    pub organization: NewOrganization,
    #[serde(default)]
    pub local_tool: bool,
}

/// A checked sign-up, kept on the link until the link is used.
pub(super) struct SignUp {
    pub(super) organization_name: String,
    pub(super) person_name: Option<String>,
    pub(super) cost_source: Option<CostSource>,
    pub(super) reporting_currency: Option<String>,
    pub(super) manages: Option<Manages>,
}

impl SignUp {
    fn check(new: &NewOrganization) -> Result<Self, ApiError> {
        Ok(Self {
            organization_name: organizations::validate_name(&new.organization_name)?,
            person_name: new
                .name
                .as_deref()
                .map(organizations::clean_name)
                .transpose()?
                .filter(|name| !name.is_empty()),
            cost_source: new.cost_source,
            reporting_currency: new
                .reporting_currency
                .as_deref()
                .map(organizations::validate_currency)
                .transpose()?,
            manages: new.manages,
        })
    }
}

/// Create an organization and email its first controller a link.
///
/// The organization exists once the link is used, in any browser. An
/// address that is already a member gets an ordinary sign-in link instead:
/// one account belongs to one organization.
#[utoipa::path(
    post,
    path = "/sign-up",
    operation_id = "sign_up_organization",
    tag = "sign-in",
    request_body = SignUpRequest,
    responses(
        (status = ACCEPTED, description = "A mail is on its way", body = LinkSent),
        (status = BAD_REQUEST, description = "Not an email address, no name, or not a currency", body = crate::error::ErrorBody),
        (status = TOO_MANY_REQUESTS, description = "Too many links for this address", body = crate::error::ErrorBody),
    ),
)]
pub(super) async fn sign_up(
    State(state): State<AppState>,
    Json(request): Json<SignUpRequest>,
) -> Result<(StatusCode, Json<LinkSent>), ApiError> {
    let email = links::normalize_email(&request.email)?;
    let sign_up = SignUp::check(&request.organization)?;
    let sent = links::request_link(&state, &email, Some(sign_up), request.local_tool).await?;
    Ok((StatusCode::ACCEPTED, Json(sent)))
}

/// The address a sign-up link was sent to.
#[derive(Debug, Serialize, ToSchema)]
pub struct SignUpLink {
    pub email: String,
}

/// The address behind the link in the "no account" mail, for the web app's
/// sign-up form. Reading it uses nothing up.
#[utoipa::path(
    get,
    path = "/sign-up/link",
    operation_id = "sign_up_link",
    tag = "sign-in",
    params(LinkToken),
    responses(
        (status = OK, body = SignUpLink),
        (status = GONE, description = "The link was used, has expired, or cannot create an organization", body = crate::error::ErrorBody),
    ),
)]
pub(super) async fn link(
    State(state): State<AppState>,
    Query(link): Query<LinkToken>,
) -> Result<Json<SignUpLink>, ApiError> {
    let email = links::sign_up_address(&state.db, &state.config, &link.token)
        .await?
        .ok_or_else(gone)?;
    Ok(Json(SignUpLink { email }))
}

/// Create an organization with the link from the "no account" mail.
#[derive(Debug, Deserialize, ToSchema)]
pub struct SignUpWithLink {
    pub token: String,
    #[serde(flatten)]
    pub organization: NewOrganization,
}

/// Create an organization with the link from the "no account" mail. The
/// founder is signed in at once: the answer sets the session cookie, and
/// no second mail goes out.
#[utoipa::path(
    post,
    path = "/sign-up/link",
    operation_id = "use_sign_up_link",
    tag = "sign-in",
    request_body = SignUpWithLink,
    responses(
        (status = NO_CONTENT, description = "Created and signed in; the session cookie is set"),
        (status = BAD_REQUEST, description = "No name, or not a currency", body = crate::error::ErrorBody),
        (status = GONE, description = "The link was used, has expired, or cannot create an organization", body = crate::error::ErrorBody),
    ),
)]
pub(super) async fn use_link(
    State(state): State<AppState>,
    Json(request): Json<SignUpWithLink>,
) -> Result<Response, ApiError> {
    let sign_up = SignUp::check(&request.organization)?;
    links::sign_up_with_link(&state, &request.token, sign_up)
        .await?
        .ok_or_else(gone)
}

fn gone() -> ApiError {
    ApiError::Gone(format!(
        "This link no longer works: it was used, or it is older than {} minutes.",
        links::LINK_MINUTES
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manages_round_trips_and_picks_a_start_page() {
        for manages in [Manages::Agents, Manages::People] {
            assert_eq!(Manages::from_db(manages.as_str()), Some(manages));
            assert_eq!(
                serde_json::to_value(manages).unwrap(),
                serde_json::Value::from(manages.as_str())
            );
            assert!(manages.start_page().ends_with(manages.as_str()));
        }
        assert_eq!(Manages::from_db("cats"), None);
    }

    #[test]
    fn a_sign_up_is_checked() {
        let new: NewOrganization = serde_json::from_str(
            r#"{"organization_name": " Kestrel ", "name": "  ", "reporting_currency": "eur"}"#,
        )
        .unwrap();
        let checked = SignUp::check(&new).unwrap();
        assert_eq!(checked.organization_name, "Kestrel");
        assert_eq!(checked.person_name, None);
        assert_eq!(checked.reporting_currency.as_deref(), Some("EUR"));
        let bad: NewOrganization = serde_json::from_str(
            r#"{"organization_name": "Kestrel", "reporting_currency": "EURO"}"#,
        )
        .unwrap();
        assert!(SignUp::check(&bad).is_err());
    }
}
