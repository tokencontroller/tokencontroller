//! Bill pulls (Anthropic Enterprise, Anthropic Console, OpenAI), ticket sync
//! (Jira Cloud, Azure DevOps, GitHub Issues), the price table refresh and late
//! token sheet reminders.
//!
//! Owner: the collection and integrations work. Keep this area's code, routes and queries in
//! this folder, and its migrations in `migrations/` named
//! `<timestamp>_integrations_<what>.sql`.

pub mod bills;
pub mod decimal;
pub mod http;
pub mod prices;
pub mod reminders;
pub mod secrets;
pub mod tickets;

use utoipa_axum::router::OpenApiRouter;

use crate::AppState;

/// This area's routes, relative to `/api/v1`.
#[must_use]
pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
}
