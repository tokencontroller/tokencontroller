//! Placing with the core: the organization's settings, people, projects and
//! the tickets a token entry names, loaded into the core's
//! [`Organization`], so the service places a token entry the way `tc` does
//! on the machine (ADR 0010).

use std::collections::{BTreeMap, BTreeSet};

use sqlx::PgConnection;
use sqlx::types::Json as JsonColumn;
use tokencontroller_core::directory::{Person, Project, Ticket};
use tokencontroller_core::pattern::TicketKeyPattern;
use tokencontroller_core::rules::Rule;
use tokencontroller_core::{
    CostSource, Currency, Organization, Signals, TagMap, TagName, TagValue, Tags,
};
use uuid::Uuid;

use super::StoreError;
use crate::ingest::entry;

/// An organization as the core places with it, without tickets: those are
/// loaded for each token entry with [`with_tickets`].
#[derive(Debug, Clone)]
pub(super) struct OrgContext {
    pub organization: Organization,
    /// Each member's `person` tag value, their email address, by ID.
    pub people: BTreeMap<Uuid, TagValue>,
}

/// Loads what placing needs of an organization.
pub(super) async fn org_context(
    conn: &mut PgConnection,
    organization_id: Uuid,
) -> Result<OrgContext, StoreError> {
    let row = sqlx::query!(
        r#"SELECT reporting_currency, cost_source, approval,
                  rules AS "rules: JsonColumn<Vec<Rule>>", several_keys
           FROM organizations WHERE id = $1"#,
        organization_id
    )
    .fetch_one(&mut *conn)
    .await?;
    let currency = Currency::new(&row.reporting_currency).unwrap_or(Currency::USD);
    // An organization that has not picked how it is charged yet places like
    // any other; the cost source only labels figures.
    let cost_source = row
        .cost_source
        .and_then(|source| serde_json::from_value(serde_json::Value::String(source)).ok())
        .unwrap_or(CostSource::ApiKey);
    let mut organization = Organization::new(currency, cost_source);
    organization.approval = row.approval;
    // No stored rules means the core's defaults (docs: Attribute > Rules).
    if let Some(rules) = row.rules {
        organization.rules = rules.0;
    }
    organization.several_keys =
        serde_json::from_value(serde_json::Value::String(row.several_keys)).unwrap_or_default();
    // No ticket matcher runs while data comes in.
    organization.summary_match = false;
    organization.tag_names = sqlx::query_scalar!(
        "SELECT name FROM tag_names WHERE organization_id = $1",
        organization_id
    )
    .fetch_all(&mut *conn)
    .await?
    .iter()
    .filter_map(|name| TagName::new(name).ok())
    .filter(|name| !name.is_built_in())
    .collect();
    organization.tag_map = tag_map(conn, organization_id).await?;

    let mut people = BTreeMap::new();
    for person in sqlx::query!(
        r#"SELECT id, email, tags, rules AS "rules: JsonColumn<Vec<Rule>>"
           FROM people WHERE organization_id = $1"#,
        organization_id
    )
    .fetch_all(&mut *conn)
    .await?
    {
        let Ok(email) = TagValue::new(&person.email) else {
            continue;
        };
        organization.directory.people.insert(
            email.clone(),
            Person {
                tags: tags_of(&person.tags),
                rules: person.rules.0,
            },
        );
        people.insert(person.id, email);
    }
    load_projects(conn, organization_id, &mut organization).await?;
    Ok(OrgContext {
        organization,
        people,
    })
}

async fn tag_map(conn: &mut PgConnection, organization_id: Uuid) -> Result<TagMap, StoreError> {
    let map = sqlx::query!(
        "SELECT cost_center, ladder FROM tag_map_versions
         WHERE organization_id = $1 ORDER BY version DESC LIMIT 1",
        organization_id
    )
    .fetch_optional(&mut *conn)
    .await?;
    Ok(map
        .and_then(|row| {
            let cost_center = TagName::new(&row.cost_center).ok()?;
            let ladder = row
                .ladder
                .iter()
                .map(|rung| TagName::new(rung))
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            TagMap::new(cost_center, ladder).ok()
        })
        .unwrap_or_default())
}

async fn load_projects(
    conn: &mut PgConnection,
    organization_id: Uuid,
    organization: &mut Organization,
) -> Result<(), StoreError> {
    let repositories = sqlx::query!(
        "SELECT project_id, repository FROM project_repositories WHERE organization_id = $1",
        organization_id
    )
    .fetch_all(&mut *conn)
    .await?;
    for project in sqlx::query!(
        "SELECT id, name, ticket_key_pattern, tags FROM projects WHERE organization_id = $1
         ORDER BY created_at, name",
        organization_id
    )
    .fetch_all(&mut *conn)
    .await?
    {
        let Ok(name) = TagValue::new(&project.name) else {
            continue;
        };
        organization.directory.projects.insert(
            name,
            Project {
                tags: tags_of(&project.tags),
                ticket_key_pattern: project
                    .ticket_key_pattern
                    .as_deref()
                    .and_then(|pattern| TicketKeyPattern::new(pattern).ok()),
                repositories: repositories
                    .iter()
                    .filter(|mapped| mapped.project_id == project.id)
                    .filter_map(|mapped| repository_key(&mapped.repository))
                    .collect(),
            },
        );
    }
    Ok(())
}

/// The organization with the tickets these keys name, and their parent
/// tickets, so placing knows their projects and tags.
pub(super) async fn with_tickets(
    conn: &mut PgConnection,
    organization_id: Uuid,
    context: &OrgContext,
    keys: &BTreeSet<String>,
) -> Result<Organization, StoreError> {
    let mut organization = context.organization.clone();
    if keys.is_empty() {
        return Ok(organization);
    }
    let keys: Vec<String> = keys.iter().cloned().collect();
    let tickets = sqlx::query!(
        r#"WITH RECURSIVE chain AS (
               SELECT id, parent_id FROM tickets WHERE organization_id = $1 AND key = ANY($2)
               UNION
               SELECT t.id, t.parent_id FROM tickets t JOIN chain c ON t.id = c.parent_id
               WHERE t.organization_id = $1
           )
           SELECT t.key, t.title, t.tags, p.name AS "project?", parent.key AS "parent?"
           FROM tickets t
           JOIN chain c ON c.id = t.id
           LEFT JOIN projects p ON p.id = t.project_id
           LEFT JOIN tickets parent ON parent.id = t.parent_id"#,
        organization_id,
        &keys
    )
    .fetch_all(&mut *conn)
    .await?;
    for ticket in tickets {
        let Ok(key) = TagValue::new(&ticket.key) else {
            continue;
        };
        organization.directory.tickets.insert(
            key,
            Ticket {
                project: ticket
                    .project
                    .as_deref()
                    .and_then(|name| TagValue::new(name).ok()),
                parent: ticket
                    .parent
                    .as_deref()
                    .and_then(|key| TagValue::new(key).ok()),
                title: ticket.title,
                tags: tags_of(&ticket.tags),
            },
        );
    }
    Ok(organization)
}

/// Tags stored as a JSON object of strings. Entries that are not valid tags
/// are left out.
pub(super) fn tags_of(value: &serde_json::Value) -> Tags {
    value
        .as_object()
        .map(|object| {
            object
                .iter()
                .filter_map(|(name, value)| {
                    Some((
                        TagName::new(name).ok()?,
                        TagValue::new(value.as_str()?).ok()?,
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Tags as a sender writes them, by name. Entries that are not valid tags
/// are left out; the entry checks already refused them where it matters.
pub(super) fn tags_from(map: &BTreeMap<String, String>) -> Tags {
    map.iter()
        .filter_map(|(name, value)| Some((TagName::new(name).ok()?, TagValue::new(value).ok()?)))
        .collect()
}

/// The core's signals of a token entry, with a split's own signals laid
/// over the token entry's.
pub(super) fn signals(entry: &entry::Signals, split: Option<&entry::Signals>) -> Signals {
    let pick = |field: fn(&entry::Signals) -> &Option<String>| {
        split
            .and_then(|split| field(split).clone())
            .or_else(|| field(entry).clone())
    };
    let ticket_keys = split
        .map(|split| &split.ticket_keys)
        .filter(|keys| !keys.is_empty())
        .unwrap_or(&entry.ticket_keys);
    Signals {
        branch: pick(|signals| &signals.branch),
        working_directory: pick(|signals| &signals.working_directory),
        repository: pick(|signals| &signals.repository)
            .map(|repository| repository_key(&repository).unwrap_or(repository)),
        pull_request: split
            .and_then(|split| split.pull_request)
            .or(entry.pull_request),
        subagent_type: pick(|signals| &signals.subagent_type),
        // One list on the wire: keys from prompts, the session title and
        // the pull request title, in the order they were named.
        prompt_ticket_keys: ticket_keys.clone(),
        session_title_ticket_keys: Vec::new(),
        pull_request_title_ticket_keys: Vec::new(),
    }
}

/// Every ticket key placing might look up: tickets named in tags, keys the
/// sender found, and keys the projects' patterns find in the branch.
pub(super) fn candidate_keys(
    organization: &Organization,
    named: &Tags,
    signals: &Signals,
) -> BTreeSet<String> {
    let mut keys: BTreeSet<String> = signals.prompt_ticket_keys.iter().cloned().collect();
    if let Some(ticket) = named.get(&TagName::ticket()) {
        keys.insert(ticket.as_str().to_owned());
    }
    if let Some(branch) = &signals.branch {
        for found in organization.directory.find_ticket_keys(branch) {
            keys.insert(found.key.as_str().to_owned());
        }
    }
    keys
}

/// The `owner/name` a project's repository mapping uses, from a repository
/// as agents report it, such as `github.com/kestrel/storefront` or
/// `https://github.com/kestrel/storefront.git`.
#[must_use]
pub(crate) fn repository_key(repository: &str) -> Option<String> {
    let trimmed = repository
        .trim()
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .to_ascii_lowercase();
    let without_scheme = trimmed
        .split_once("://")
        .map_or(trimmed.as_str(), |(_, rest)| rest);
    let path = without_scheme
        .rsplit_once(':')
        .filter(|(host, _)| host.contains('@'))
        .map_or(without_scheme, |(_, path)| path);
    let mut parts = path.rsplit('/').filter(|part| !part.is_empty());
    let name = parts.next()?;
    let owner = parts.next()?;
    Some(format!("{owner}/{name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repositories_become_owner_and_name() {
        for repository in [
            "github.com/kestrel/storefront",
            "https://github.com/Kestrel/storefront.git",
            "git@github.com:kestrel/storefront.git",
            "kestrel/storefront",
            "dev.azure.com/acme/kestrel/storefront/",
        ] {
            assert_eq!(
                repository_key(repository).as_deref(),
                Some("kestrel/storefront"),
                "{repository}"
            );
        }
        assert_eq!(repository_key("storefront"), None);
    }

    #[test]
    fn a_splits_signals_win_over_the_entrys() {
        let entry = entry::Signals {
            branch: Some("main".to_owned()),
            repository: Some("github.com/kestrel/storefront".to_owned()),
            ticket_keys: vec!["KD-1".to_owned()],
            ..entry::Signals::default()
        };
        let split = entry::Signals {
            branch: Some("KD-214-cart".to_owned()),
            ..entry::Signals::default()
        };
        let both = signals(&entry, Some(&split));
        assert_eq!(both.branch.as_deref(), Some("KD-214-cart"));
        assert_eq!(both.repository.as_deref(), Some("kestrel/storefront"));
        assert_eq!(both.prompt_ticket_keys, vec!["KD-1".to_owned()]);
    }

    #[test]
    fn stored_tags_skip_what_is_not_a_tag() {
        let tags = tags_of(&serde_json::json!({ "team": "web", "Bad Name": "x", "client": 3 }));
        assert_eq!(tags, Tags::parse(["team=web"]).unwrap());
    }
}
