//! The background job runner: a Postgres advisory lock per job and a heartbeat
//! per job (ADR 0002).
//!
//! Owner: the collection and integrations work. Keep this area's code, routes and queries in
//! this folder, and its migrations in `migrations/` named
//! `<timestamp>_jobs_<what>.sql`.

use utoipa_axum::router::OpenApiRouter;

use crate::AppState;

/// This area's routes, relative to `/api/v1`.
#[must_use]
pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
}
