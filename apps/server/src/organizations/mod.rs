//! Organizations (one account is one organization), members and invites, roles
//! (agent manager, people manager, controller) and organization settings.
//!
//! Owner: the service platform work. Keep this area's code, routes and queries in
//! this folder, and its migrations in `migrations/` named
//! `<timestamp>_organizations_<what>.sql`.

use utoipa_axum::router::OpenApiRouter;

use crate::AppState;

/// This area's routes, relative to `/api/v1`.
#[must_use]
pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
}
