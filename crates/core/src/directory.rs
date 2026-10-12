//! The records behind the built-in tags: people, projects and tickets.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::pattern::TicketKeyPattern;
use crate::rules::Rule;
use crate::tags::{TagValue, Tags};

/// A person: the default cost center. Their tags, such as `team: web`, are
/// copied onto their new token entries.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Person {
    pub tags: Tags,
    /// Personal rules. They run after the organization's.
    pub rules: Vec<Rule>,
}

/// A project: a body of work made of tickets.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Project {
    /// Tags such as `client: acme`, copied onto the project's token entries.
    pub tags: Tags,
    pub ticket_key_pattern: Option<TicketKeyPattern>,
    /// Repositories mapped to the project, such as `kestrel/storefront`.
    pub repositories: Vec<String>,
}

/// A ticket, synced from a ticket system or created by hand. Only the key
/// is required; it is the ticket's key in [`Directory::tickets`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Ticket {
    pub project: Option<TagValue>,
    pub parent: Option<TagValue>,
    pub title: Option<String>,
    pub tags: Tags,
}

/// The people, projects and tickets of one organization, by tag value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Directory {
    pub people: BTreeMap<TagValue, Person>,
    pub projects: BTreeMap<TagValue, Project>,
    pub tickets: BTreeMap<TagValue, Ticket>,
}

/// A ticket key found in text, and the project whose pattern found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundKey {
    pub key: TagValue,
    pub project: TagValue,
}

impl Directory {
    /// The project a repository is mapped to. One repository belongs to one
    /// project; if two claim it, the first in order wins.
    #[must_use]
    pub fn project_of_repository(&self, repository: &str) -> Option<&TagValue> {
        self.projects
            .iter()
            .find(|(_, project)| {
                project
                    .repositories
                    .iter()
                    .any(|mapped| mapped == repository)
            })
            .map(|(name, _)| name)
    }

    /// Every ticket key in `text`, in the order they appear, found with the
    /// projects' ticket key patterns. Where two projects' patterns find a key
    /// at the same place, the first project in order wins.
    #[must_use]
    pub fn find_ticket_keys(&self, text: &str) -> Vec<FoundKey> {
        let mut found: Vec<(usize, FoundKey)> = Vec::new();
        for (name, project) in &self.projects {
            let Some(pattern) = &project.ticket_key_pattern else {
                continue;
            };
            for (position, key) in pattern.find_all(text) {
                if found.iter().any(|(at, _)| *at == position) {
                    continue;
                }
                if let Ok(key) = TagValue::new(key) {
                    found.push((
                        position,
                        FoundKey {
                            key,
                            project: name.clone(),
                        },
                    ));
                }
            }
        }
        found.sort_by_key(|(position, _)| *position);
        found.into_iter().map(|(_, key)| key).collect()
    }

    /// Whether `key` is a ticket key: a known ticket, or a key that one of
    /// the projects' patterns matches as a whole.
    #[must_use]
    pub fn is_ticket_key(&self, key: &TagValue) -> bool {
        self.tickets.contains_key(key) || self.pattern_project(key).is_some()
    }

    /// The project a ticket belongs to: the ticket's own project if it is
    /// known, else the project whose ticket key pattern matches the key.
    #[must_use]
    pub fn project_of_ticket(&self, key: &TagValue) -> Option<&TagValue> {
        self.tickets
            .get(key)
            .and_then(|ticket| ticket.project.as_ref())
            .or_else(|| self.pattern_project(key))
    }

    fn pattern_project(&self, key: &TagValue) -> Option<&TagValue> {
        self.projects
            .iter()
            .find(|(_, project)| {
                project
                    .ticket_key_pattern
                    .as_ref()
                    .is_some_and(|pattern| pattern.is_key(key.as_str()))
            })
            .map(|(name, _)| name)
    }

    /// The parent tickets of a ticket, nearest first. Stops at a ticket that
    /// is not known and never loops on a cycle.
    #[must_use]
    pub fn ancestors(&self, key: &TagValue) -> Vec<&TagValue> {
        let mut chain: Vec<&TagValue> = Vec::new();
        let mut parent = self
            .tickets
            .get(key)
            .and_then(|ticket| ticket.parent.as_ref());
        // A chain never has more links than there are tickets, plus one
        // parent that is not known; the bound keeps a mistake from looping.
        for _ in 0..=self.tickets.len() {
            let Some(next) = parent else {
                break;
            };
            if next == key || chain.contains(&next) {
                break;
            }
            chain.push(next);
            parent = self
                .tickets
                .get(next)
                .and_then(|ticket| ticket.parent.as_ref());
        }
        chain
    }

    /// The tags a token entry inherits from its person, project and ticket.
    /// Where they disagree, the ticket wins over its parents, parents over
    /// the project, and the project over the person.
    #[must_use]
    pub fn inherited_tags(
        &self,
        person: Option<&TagValue>,
        project: Option<&TagValue>,
        ticket: Option<&TagValue>,
    ) -> Tags {
        let mut tags = person
            .and_then(|person| self.people.get(person))
            .map(|person| person.tags.clone())
            .unwrap_or_default();
        if let Some(project) = project.and_then(|project| self.projects.get(project)) {
            tags = project.tags.over(&tags);
        }
        if let Some(ticket) = ticket {
            for ancestor in self.ancestors(ticket).into_iter().rev() {
                if let Some(parent) = self.tickets.get(ancestor) {
                    tags = parent.tags.over(&tags);
                }
            }
            if let Some(ticket) = self.tickets.get(ticket) {
                tags = ticket.tags.over(&tags);
            }
        }
        tags
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(text: &str) -> TagValue {
        TagValue::new(text).unwrap()
    }

    fn tags(pairs: &[&str]) -> Tags {
        Tags::parse(pairs.iter().copied()).unwrap()
    }

    fn kestrel() -> Directory {
        let mut directory = Directory::default();
        directory.people.insert(
            value("mara"),
            Person {
                tags: tags(&["team=web", "client=internal"]),
                rules: Vec::new(),
            },
        );
        directory.projects.insert(
            value("storefront"),
            Project {
                tags: tags(&["client=acme"]),
                ticket_key_pattern: Some(TicketKeyPattern::new(r"^(KD|OPS)-\d+").unwrap()),
                repositories: vec!["kestrel/storefront".to_owned()],
            },
        );
        directory.projects.insert(
            value("support"),
            Project {
                tags: tags(&["client=globex"]),
                ticket_key_pattern: Some(TicketKeyPattern::new(r"^(SUP|KD)-\d+").unwrap()),
                repositories: vec![
                    "kestrel/storefront".to_owned(),
                    "kestrel/helpdesk".to_owned(),
                ],
            },
        );
        directory.tickets.insert(
            value("KD-200"),
            Ticket {
                project: Some(value("storefront")),
                parent: None,
                title: Some("Checkout epic".to_owned()),
                tags: tags(&["feature=checkout", "purpose=product"]),
            },
        );
        directory.tickets.insert(
            value("KD-214"),
            Ticket {
                project: Some(value("storefront")),
                parent: Some(value("KD-200")),
                title: None,
                tags: tags(&["purpose=bugfix"]),
            },
        );
        directory
    }

    #[test]
    fn a_repository_belongs_to_the_first_project_that_maps_it() {
        let directory = kestrel();
        assert_eq!(
            directory.project_of_repository("kestrel/storefront"),
            Some(&value("storefront"))
        );
        assert_eq!(
            directory.project_of_repository("kestrel/helpdesk"),
            Some(&value("support"))
        );
        assert_eq!(directory.project_of_repository("kestrel/other"), None);
    }

    #[test]
    fn ticket_keys_are_found_in_order_with_their_project() {
        let directory = kestrel();
        let found = directory.find_ticket_keys("SUP-3 then KD-214 and OPS-9");
        let keys: Vec<(&str, &str)> = found
            .iter()
            .map(|found| (found.key.as_str(), found.project.as_str()))
            .collect();
        assert_eq!(
            keys,
            vec![
                ("SUP-3", "support"),
                ("KD-214", "storefront"),
                ("OPS-9", "storefront")
            ]
        );
        assert!(directory.find_ticket_keys("nothing here").is_empty());
    }

    #[test]
    fn a_key_belongs_to_its_tickets_project_or_its_pattern() {
        let mut directory = kestrel();
        directory.tickets.insert(
            value("SUP-1"),
            Ticket {
                project: Some(value("storefront")),
                ..Ticket::default()
            },
        );
        directory
            .tickets
            .insert(value("LOOSE-1"), Ticket::default());
        assert_eq!(
            directory.project_of_ticket(&value("SUP-1")),
            Some(&value("storefront"))
        );
        assert_eq!(
            directory.project_of_ticket(&value("SUP-2")),
            Some(&value("support"))
        );
        assert_eq!(directory.project_of_ticket(&value("LOOSE-1")), None);
        assert_eq!(directory.project_of_ticket(&value("XYZ-1")), None);
        assert!(directory.is_ticket_key(&value("LOOSE-1")));
        assert!(directory.is_ticket_key(&value("OPS-1")));
        assert!(!directory.is_ticket_key(&value("XYZ-1")));
    }

    #[test]
    fn parents_are_followed_without_looping() {
        let mut directory = kestrel();
        assert_eq!(
            directory.ancestors(&value("KD-214")),
            vec![&value("KD-200")]
        );
        assert!(directory.ancestors(&value("KD-200")).is_empty());
        assert!(directory.ancestors(&value("KD-999")).is_empty());
        directory.tickets.insert(
            value("A"),
            Ticket {
                parent: Some(value("B")),
                ..Ticket::default()
            },
        );
        directory.tickets.insert(
            value("B"),
            Ticket {
                parent: Some(value("A")),
                ..Ticket::default()
            },
        );
        assert_eq!(directory.ancestors(&value("A")), vec![&value("B")]);
        directory.tickets.insert(
            value("C"),
            Ticket {
                parent: Some(value("GONE")),
                ..Ticket::default()
            },
        );
        assert_eq!(directory.ancestors(&value("C")), vec![&value("GONE")]);
        directory.tickets.insert(
            value("D"),
            Ticket {
                parent: Some(value("E")),
                ..Ticket::default()
            },
        );
        directory.tickets.insert(
            value("E"),
            Ticket {
                parent: Some(value("F")),
                ..Ticket::default()
            },
        );
        directory.tickets.insert(
            value("F"),
            Ticket {
                parent: Some(value("E")),
                ..Ticket::default()
            },
        );
        assert_eq!(
            directory.ancestors(&value("D")),
            vec![&value("E"), &value("F")]
        );
        directory.tickets.insert(
            value("SELF"),
            Ticket {
                parent: Some(value("SELF")),
                ..Ticket::default()
            },
        );
        assert!(directory.ancestors(&value("SELF")).is_empty());
    }

    #[test]
    fn the_most_specific_record_wins_inherited_tags() {
        let directory = kestrel();
        let mara = value("mara");
        let storefront = value("storefront");
        let ticket = value("KD-214");
        assert_eq!(
            directory.inherited_tags(Some(&mara), None, None),
            tags(&["team=web", "client=internal"])
        );
        assert_eq!(
            directory.inherited_tags(Some(&mara), Some(&storefront), None),
            tags(&["team=web", "client=acme"])
        );
        assert_eq!(
            directory.inherited_tags(Some(&mara), Some(&storefront), Some(&ticket)),
            tags(&[
                "team=web",
                "client=acme",
                "feature=checkout",
                "purpose=bugfix"
            ])
        );
        assert_eq!(
            directory.inherited_tags(None, None, Some(&value("KD-999"))),
            Tags::new()
        );
        assert_eq!(
            directory.inherited_tags(Some(&value("nobody")), Some(&value("nothing")), None),
            Tags::new()
        );
    }
}
