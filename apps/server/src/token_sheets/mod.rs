//! Token sheets: review and submit, cadence, approval and reject with a
//! reason (docs: Attribute > Review and submit; Account > Cadence and token
//! sheets, Approval).
//!
//! Owner: the close and reports work. Keep this area's code, routes and
//! queries in this folder, and its migrations in `migrations/` named
//! `<timestamp>_token_sheets_<what>.sql`.

pub mod approval;
pub mod cadence;
pub mod cadences;
pub mod current;
pub mod late;
pub mod submit;

use utoipa_axum::router::OpenApiRouter;

use crate::AppState;

pub use late::{LateSheet, context_for, late_sheets};

/// This area's routes, relative to `/api/v1`.
#[must_use]
pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .merge(cadences::routes())
        .merge(approval::routes())
        .merge(current::routes())
        .merge(submit::routes())
        .merge(late::routes())
}
