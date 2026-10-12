//! Draft review, token sheet submit, cadence, approval and reject with a
//! reason.
//!
//! Owner: the close and reports work. Keep this area's code, routes and queries in
//! this folder, and its migrations in `migrations/` named
//! `<timestamp>_token_sheets_<what>.sql`.

pub mod cadence;

use utoipa_axum::router::OpenApiRouter;

use crate::AppState;

/// This area's routes, relative to `/api/v1`.
#[must_use]
pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
}
