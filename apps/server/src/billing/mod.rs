//! Team plan subscriptions through Stripe for the hosted service. Off unless
//! the Stripe settings are set, so self-hosted instances never see it.
//!
//! Owner: the Stripe billing work. Keep this area's code, routes and queries in
//! this folder, and its migrations in `migrations/` named
//! `<timestamp>_billing_<what>.sql`.

use utoipa_axum::router::OpenApiRouter;

use crate::AppState;

/// This area's routes, relative to `/api/v1`.
#[must_use]
pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
}
