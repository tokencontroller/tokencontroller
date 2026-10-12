//! Reports (token budget vs actual, agent harness, outliers), CSV export from
//! every table, the read-only reporting API and the raw export.
//!
//! Owner: the close and reports work. Keep this area's code, routes and queries in
//! this folder, and its migrations in `migrations/` named
//! `<timestamp>_reports_<what>.sql`.

pub mod cost;
pub mod csv;
pub mod entries;
pub mod filter;
pub mod lines;
pub mod outliers;
pub mod raw;
pub mod saved;
pub mod scope;
pub mod table;

use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::AppState;
use crate::sign_in::CredentialKind;

/// The credentials that may read reports: a signed-in person, the CLI and
/// read tokens. Service tokens only report token entries.
pub const READERS: &[CredentialKind] = &[CredentialKind::Session, CredentialKind::Read];

/// This area's routes, relative to `/api/v1`.
#[must_use]
pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(cost::cost_report))
        .routes(routes!(cost::agent_harness_report))
        .merge(outliers::routes())
        .merge(raw::routes())
        .merge(saved::routes())
        .merge(table::routes())
}
