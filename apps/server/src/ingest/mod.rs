//! The doors: how messages reach the log (docs: Collect). The token entries
//! endpoint and CSV import take token entries; the OpenTelemetry receiver
//! takes agent telemetry. Every door writes message events; the core's
//! fold prices them and rules split them.
//!
//! Owner: the service platform work. Keep this area's code, routes and
//! queries in this folder, and its migrations in `migrations/` named
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

pub use http::{MAX_ENTRIES_PER_CALL, MAX_REQUEST_BYTES, StoredEntries, TokenEntries};
pub use store::{StoredEntry, TelemetryOutcome};

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
