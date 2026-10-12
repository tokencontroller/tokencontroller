//! Magic-link sign-in over SMTP, sessions, and API tokens (`tc_svc_`,
//! `tc_read_`, `tc_org_`). OIDC and SCIM come from the extension.
//!
//! Every other area finds out who is calling through [`Member`] or
//! [`Caller`] (see [`caller`]).
//!
//! Owner: the service platform work. Keep this area's code, routes and queries in
//! this folder, and its migrations in `migrations/` named
//! `<timestamp>_sign_in_<what>.sql`.

pub mod caller;
pub mod links;
pub mod secret;
pub mod tokens;

use utoipa::openapi::security::{ApiKey, ApiKeyValue, HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::AppState;

pub use caller::{Caller, Credential, CredentialKind, Member};
pub use links::{domain_allowed, link_for_member, normalize_email};

/// This area's routes, relative to `/api/v1`.
#[must_use]
pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(links::sign_in))
        .routes(routes!(links::sign_up))
        .routes(routes!(links::link_page, links::use_link))
        .routes(routes!(links::poll))
        .routes(routes!(links::sign_out))
        .routes(routes!(links::me))
        .routes(routes!(tokens::list, tokens::create))
        .routes(routes!(tokens::revoke))
        .routes(routes!(tokens::hand_over))
}

/// Adds the two ways to sign in to the OpenAPI document: the web app's
/// session cookie, and a bearer token for everything else.
pub struct SecuritySchemes;

impl utoipa::Modify for SecuritySchemes {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "session",
            SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::with_description(
                caller::SESSION_COOKIE,
                "The web app's session, set when a magic link is used in the browser.",
            ))),
        );
        components.add_security_scheme(
            "bearer",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .description(Some(
                        "A local tool's session (`tc_ses_`) or an API token: `tc_svc_` for services, `tc_read_` to read, `tc_org_` for the organization.",
                    ))
                    .build(),
            ),
        );
    }
}
