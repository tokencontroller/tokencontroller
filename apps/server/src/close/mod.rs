//! The monthly close with locked rows, price table and tag map versions (ADR
//! 0007), adjustments and the cost sheet.
//!
//! Owner: the close and reports work. Keep this area's code, routes and queries in
//! this folder, and its migrations in `migrations/` named
//! `<timestamp>_close_<what>.sql`.

pub mod period;

use utoipa_axum::router::OpenApiRouter;

use crate::AppState;

/// This area's routes, relative to `/api/v1`.
#[must_use]
pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
}
