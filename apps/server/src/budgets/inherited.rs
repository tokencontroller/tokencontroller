//! The tags a filter brings with it through the records behind the built-in
//! tags (docs: Account > Tags, where tags come from). A token entry on
//! `ticket: KD-214` inherits its project and the project's `client: acme`,
//! and one by a person tagged `team: web` inherits `team: web`. So a people
//! manager over `client: acme` may set a token budget on `ticket: KD-214`,
//! and one over `team: web` on a person of that team.

use std::collections::{BTreeMap, BTreeSet};

use sqlx::PgPool;
use uuid::Uuid;

use crate::reports::filter::{Filter, Tags};

/// The tags of the people, projects and tickets some filters name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inherited {
    /// Tags per person's email.
    people: BTreeMap<String, Tags>,
    /// Tags per project name.
    projects: BTreeMap<String, Tags>,
    /// Tags and project name per ticket key.
    tickets: BTreeMap<String, (Tags, Option<String>)>,
}

/// The values of one tag name across some filters.
fn values<'a>(filters: &[&'a Filter], name: &str) -> BTreeSet<&'a str> {
    filters
        .iter()
        .filter_map(|filter| filter.0.get(name).map(String::as_str))
        .collect()
}

fn owned(values: &BTreeSet<&str>) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

impl Inherited {
    /// Reads the records behind the `person`, `project` and `ticket` tags of
    /// `filters`.
    ///
    /// # Errors
    ///
    /// A database error.
    pub async fn load(
        db: &PgPool,
        organization_id: Uuid,
        filters: &[&Filter],
    ) -> Result<Self, sqlx::Error> {
        let emails = owned(&values(filters, "person"));
        let keys = owned(&values(filters, "ticket"));
        let people = if emails.is_empty() {
            BTreeMap::new()
        } else {
            sqlx::query!(
                "SELECT email, tags FROM people WHERE organization_id = $1 AND email = ANY($2)",
                organization_id,
                &emails,
            )
            .fetch_all(db)
            .await?
            .into_iter()
            .map(|row| (row.email, Filter::from_json(&row.tags).0))
            .collect()
        };
        let tickets: BTreeMap<String, (Tags, Option<String>)> = if keys.is_empty() {
            BTreeMap::new()
        } else {
            sqlx::query!(
                r#"SELECT t.key, t.tags, p.name AS "project?"
                   FROM tickets t LEFT JOIN projects p ON p.id = t.project_id
                   WHERE t.organization_id = $1 AND t.key = ANY($2)
                   ORDER BY t.key, t.id"#,
                organization_id,
                &keys,
            )
            .fetch_all(db)
            .await?
            .into_iter()
            .map(|row| (row.key, (Filter::from_json(&row.tags).0, row.project)))
            .collect()
        };
        let mut names: BTreeSet<String> = owned(&values(filters, "project")).into_iter().collect();
        names.extend(tickets.values().filter_map(|(_, project)| project.clone()));
        let names: Vec<String> = names.into_iter().collect();
        let projects = if names.is_empty() {
            BTreeMap::new()
        } else {
            sqlx::query!(
                "SELECT name, tags FROM projects WHERE organization_id = $1 AND name = ANY($2)",
                organization_id,
                &names,
            )
            .fetch_all(db)
            .await?
            .into_iter()
            .map(|row| (row.name, Filter::from_json(&row.tags).0))
            .collect()
        };
        Ok(Self {
            people,
            projects,
            tickets,
        })
    }

    /// `filter` with the tags its ticket, project and person carry added.
    /// The filter's own tags always win.
    #[must_use]
    pub fn expand(&self, filter: &Filter) -> Filter {
        let mut tags = filter.0.clone();
        let add = |tags: &mut Tags, more: &Tags| {
            for (name, value) in more {
                tags.entry(name.clone()).or_insert_with(|| value.clone());
            }
        };
        if let Some((ticket_tags, project)) =
            filter.0.get("ticket").and_then(|key| self.tickets.get(key))
        {
            add(&mut tags, ticket_tags);
            if let Some(project) = project {
                tags.entry("project".to_owned())
                    .or_insert_with(|| project.clone());
            }
        }
        if let Some(project_tags) = tags
            .get("project")
            .and_then(|name| self.projects.get(name))
            .cloned()
        {
            add(&mut tags, &project_tags);
        }
        if let Some(person_tags) = filter
            .0
            .get("person")
            .and_then(|email| self.people.get(email))
        {
            add(&mut tags, person_tags);
        }
        Filter(tags)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(pairs: &[(&str, &str)]) -> Tags {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    fn inherited() -> Inherited {
        Inherited {
            people: [("ana@example.com".to_owned(), tags(&[("team", "web")]))].into(),
            projects: [("acme-shop".to_owned(), tags(&[("client", "acme")]))].into(),
            tickets: [
                (
                    "KD-214".to_owned(),
                    (tags(&[("feature", "cart")]), Some("acme-shop".to_owned())),
                ),
                ("OPS-1".to_owned(), (tags(&[]), None)),
            ]
            .into(),
        }
    }

    #[test]
    fn a_ticket_brings_its_project_and_the_projects_tags() {
        let expanded = inherited().expand(&Filter(tags(&[("ticket", "KD-214")])));
        assert_eq!(
            expanded.0,
            tags(&[
                ("ticket", "KD-214"),
                ("feature", "cart"),
                ("project", "acme-shop"),
                ("client", "acme"),
            ])
        );
        let client = Filter(tags(&[("client", "acme")]));
        assert!(expanded.lies_within(&client));
    }

    #[test]
    fn a_person_brings_their_team() {
        let expanded = inherited().expand(&Filter(tags(&[("person", "ana@example.com")])));
        assert_eq!(
            expanded.0,
            tags(&[("person", "ana@example.com"), ("team", "web")])
        );
    }

    #[test]
    fn the_filters_own_tags_win_and_unknown_records_add_nothing() {
        let filter = Filter(tags(&[("project", "acme-shop"), ("client", "globex")]));
        assert_eq!(inherited().expand(&filter), filter);
        let unknown = Filter(tags(&[("ticket", "NOPE-1"), ("person", "bo@example.com")]));
        assert_eq!(inherited().expand(&unknown), unknown);
        let no_project = Filter(tags(&[("ticket", "OPS-1")]));
        assert_eq!(inherited().expand(&no_project), no_project);
        assert_eq!(inherited().expand(&Filter::default()), Filter::default());
    }

    #[test]
    fn values_are_collected_per_tag_name() {
        let a = Filter(tags(&[("ticket", "KD-1"), ("person", "ana@example.com")]));
        let b = Filter(tags(&[("ticket", "KD-2")]));
        let found = values(&[&a, &b], "ticket");
        assert_eq!(found.into_iter().collect::<Vec<_>>(), ["KD-1", "KD-2"]);
        assert!(values(&[&a, &b], "project").is_empty());
    }
}
