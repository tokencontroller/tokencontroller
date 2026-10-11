//! Magic-link sign-in over SMTP, sessions, and API tokens (`tc_svc_`,
//! `tc_read_`, `tc_org_`). OIDC and SCIM come from the extension.
//!
//! Owner: the service platform work. Keep this area's code, routes and queries in
//! this folder, and its migrations in `migrations/` named
//! `<timestamp>_sign_in_<what>.sql`.

use utoipa_axum::router::OpenApiRouter;

use crate::AppState;

/// This area's routes, relative to `/api/v1`.
#[must_use]
pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
}
