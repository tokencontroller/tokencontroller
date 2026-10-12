//! The organization's people, projects and tickets as the core crate's
//! [`Directory`], which places token entries and fills inherited tags.

use std::collections::BTreeMap;

use sqlx::PgPool;
use sqlx::types::Json as JsonColumn;
use tokencontroller_core::directory::{Directory, Person, Project, Ticket};
use tokencontroller_core::pattern::TicketKeyPattern;
use tokencontroller_core::tags::TagValue;
use uuid::Uuid;

use crate::error::ApiError;
use crate::tags::{self, Tags};

/// Everyone, every project that is not archived and every ticket of an
/// organization. A person's `person` tag value is their email address.
/// Removed people stay in, because their service tokens may still report.
/// Personal rules are left empty for the rules area to fill.
///
/// # Errors
///
/// Fails if a query fails, or a stored value breaks the core rules, which
/// the checks on the way in prevent.
pub async fn directory(db: &PgPool, organization_id: Uuid) -> Result<Directory, ApiError> {
    let people = sqlx::query!(
        r#"SELECT email, tags AS "tags: JsonColumn<Tags>" FROM people WHERE organization_id = $1"#,
        organization_id
    )
    .fetch_all(db)
    .await?;
    let projects = sqlx::query!(
        r#"SELECT p.name, p.ticket_key_pattern, p.tags AS "tags: JsonColumn<Tags>",
                  coalesce(array_agg(r.repository ORDER BY r.repository)
                           FILTER (WHERE r.repository IS NOT NULL), '{}') AS "repositories!"
           FROM projects p
           LEFT JOIN project_repositories r ON r.project_id = p.id
           WHERE p.organization_id = $1 AND p.archived_at IS NULL
           GROUP BY p.id"#,
        organization_id
    )
    .fetch_all(db)
    .await?;
    let tickets = sqlx::query!(
        r#"SELECT t.key, t.title, t.tags AS "tags: JsonColumn<Tags>",
                  p.name AS "project?", parent.key AS "parent?"
           FROM tickets t
           LEFT JOIN projects p ON p.id = t.project_id
           LEFT JOIN tickets parent ON parent.id = t.parent_id
           WHERE t.organization_id = $1"#,
        organization_id
    )
    .fetch_all(db)
    .await?;

    let mut directory = Directory::default();
    for person in people {
        directory.people.insert(
            value(&person.email)?,
            Person {
                tags: core_tags(&person.tags.0)?,
                rules: Vec::new(),
            },
        );
    }
    for project in projects {
        let ticket_key_pattern = project
            .ticket_key_pattern
            .as_deref()
            .map(TicketKeyPattern::new)
            .transpose()
            .map_err(|error| broken(&error))?;
        directory.projects.insert(
            value(&project.name)?,
            Project {
                tags: core_tags(&project.tags.0)?,
                ticket_key_pattern,
                repositories: project.repositories,
            },
        );
    }
    let mut by_key = BTreeMap::new();
    for ticket in tickets {
        by_key.insert(
            value(&ticket.key)?,
            Ticket {
                project: ticket.project.as_deref().map(value).transpose()?,
                parent: ticket.parent.as_deref().map(value).transpose()?,
                title: ticket.title,
                tags: core_tags(&ticket.tags.0)?,
            },
        );
    }
    directory.tickets = by_key;
    Ok(directory)
}

fn value(text: &str) -> Result<TagValue, ApiError> {
    TagValue::new(text).map_err(|error| broken(&error))
}

fn core_tags(stored: &Tags) -> Result<tokencontroller_core::Tags, ApiError> {
    tags::to_core(stored).map_err(|error| broken(&error))
}

fn broken(error: &dyn std::fmt::Display) -> ApiError {
    ApiError::Internal(anyhow::anyhow!("stored directory value: {error}"))
}
