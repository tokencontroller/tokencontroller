//! Every door that brings token entries and telemetry in: the OpenTelemetry
//! receiver at `/api/v1/ingest/otlp`, the token entries endpoint, manual entry
//! and CSV import, and per-message dedupe (ADR 0006).
//!
//! Owner: the collection and integrations work. Keep this area's code, routes and queries in
//! this folder, and its migrations in `migrations/` named
//! `<timestamp>_ingest_<what>.sql`.

pub mod csv_import;
pub mod entry;
pub mod http;
pub mod model;
pub mod otlp;
pub mod provider;
pub mod store;

use axum::extract::DefaultBodyLimit;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::AppState;

/// This area's routes, relative to `/api/v1`.
#[must_use]
pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(http::create_token_entries))
        .routes(routes!(http::preview_csv))
        .routes(routes!(http::import_csv))
        .routes(routes!(http::otlp_logs))
        .routes(routes!(http::otlp_traces))
        .routes(routes!(http::otlp_metrics))
        .layer(DefaultBodyLimit::max(http::MAX_REQUEST_BYTES))
}
