//! Bill pulls (Anthropic Enterprise, Anthropic Console, OpenAI), CSV bill
//! uploads and the Jira sync that copies tickets as feature tag values.
//!
//! Owner: the collection and integrations work. Keep this area's code, routes and queries in
//! this folder, and its migrations in `migrations/` named
//! `<timestamp>_integrations_<what>.sql`.

pub mod bill_connections;
pub mod bill_uploads;
pub mod bills;
pub mod decimal;
pub mod http;
pub mod secrets;
pub mod ticket_systems;
pub mod tickets;

use serde::Serialize;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;

use crate::AppState;

/// This area's routes, relative to `/api/v1`.
#[must_use]
pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .merge(ticket_systems::routes())
        .merge(bill_connections::routes())
        .merge(bill_uploads::routes())
}

/// How a connection's last pull or sync went.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionStatus {
    /// It worked, or it has not run yet.
    Ok,
    /// The last one failed; the message says why.
    Failing,
}

impl ConnectionStatus {
    /// `Failing` while there is a last error.
    #[must_use]
    pub const fn of(last_error: Option<&String>) -> Self {
        if last_error.is_some() {
            Self::Failing
        } else {
            Self::Ok
        }
    }
}
