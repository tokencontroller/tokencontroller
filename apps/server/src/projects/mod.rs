//! Projects, clients and tickets: the built-in tags with records behind them
//! (docs: Account > Projects and tickets). A client is a tag on projects,
//! not a record of its own.
//!
//! Owner: the service platform work. Keep this area's code, routes and queries in
//! this folder, and its migrations in `migrations/` named
//! `<timestamp>_projects_<what>.sql`.

mod directory;
mod project;
mod ticket;

use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::AppState;
use crate::error::ApiError;

pub use directory::directory;
pub use project::{Client, Project, project_for_repository};
pub use ticket::{Ticket, TicketSource, project_for_key};

/// This area's routes, relative to `/api/v1`.
#[must_use]
pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(project::list_projects, project::create_project))
        .routes(routes!(project::update_project, project::archive_project))
        .routes(routes!(project::list_clients))
        .routes(routes!(ticket::list_tickets, ticket::create_ticket))
        .routes(routes!(ticket::get_ticket, ticket::update_ticket))
}

/// The organization's projects that are not archived.
///
/// # Errors
///
/// Fails if a query fails.
pub async fn projects(
    db: &sqlx::PgPool,
    organization_id: uuid::Uuid,
) -> Result<Vec<Project>, ApiError> {
    project::load(db, organization_id, None, false).await
}

/// Tickets by key, optionally of one project or matching `q` in key or
/// title, at most `limit` (up to 500).
///
/// # Errors
///
/// Fails if a query fails.
pub async fn tickets(
    db: &sqlx::PgPool,
    organization_id: uuid::Uuid,
    project_id: Option<uuid::Uuid>,
    q: Option<String>,
    limit: i64,
) -> Result<Vec<Ticket>, ApiError> {
    let filter = ticket::TicketFilter {
        project_id,
        parent_id: None,
        q,
        limit: Some(limit),
        offset: 0,
    };
    ticket::load(db, organization_id, None, &filter).await
}

/// The longest project name or ticket key.
const MAX_NAME_CHARS: usize = 100;

/// Checks a project name or ticket key: the value of the `project` or
/// `ticket` tag.
fn tag_value(what: &str, value: &str) -> Result<String, ApiError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > MAX_NAME_CHARS {
        return Err(ApiError::BadRequest(format!(
            "The {what} must have 1 to {MAX_NAME_CHARS} characters."
        )));
    }
    Ok(value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_keys_are_trimmed_and_limited() {
        assert_eq!(tag_value("key", " KD-214 ").unwrap(), "KD-214");
        assert!(tag_value("key", " ").is_err());
        assert!(tag_value("key", &"K".repeat(101)).is_err());
        assert!(tag_value("key", &"K".repeat(100)).is_ok());
    }
}
