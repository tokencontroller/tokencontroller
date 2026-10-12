//! Token budgets on any tag filter, token budget vs actual, and token
//! forecasts (docs: Account > Token budgets and forecasts; Report > Token
//! budget vs actual).
//!
//! Owner: the close and reports work. Keep this area's code, routes and queries in
//! this folder, and its migrations in `migrations/` named
//! `<timestamp>_budgets_<what>.sql`.

pub mod actual;
pub mod embeddings;
pub mod forecast;
pub mod inherited;
pub mod report;
pub mod similarity;
pub mod token_budget;

pub use forecast::embed_pending;

use utoipa_axum::router::OpenApiRouter;

use crate::AppState;

/// This area's routes, relative to `/api/v1`.
#[must_use]
pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .merge(token_budget::routes())
        .merge(report::routes())
        .merge(forecast::routes())
}
