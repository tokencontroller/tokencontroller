//! Access (§5): grants of a capability on a filter, and the roles that are
//! named bundles of them.
//!
//! `can(person, capability, F)` holds when the person has a grant of that
//! capability on a filter that covers `F`: `view F` reads `C` and `P` for
//! every `F' ⊇ F`. Everyone can record. A manager is never a tag or
//! attribute.

use serde::{Deserialize, Serialize};

use crate::tags::{Filter, TagName, TagValue, Tags};

/// What a grant allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// Read `C` and `P` for every filter inside the grant's.
    View,
    /// Send messages and tag them. Everyone, no filter.
    Record,
    /// Tag, split and correct other people's messages inside the filter.
    Assign,
    /// Approve or reject submissions inside the filter.
    Approve,
    /// Create estimates, accept plans and set shares inside the filter.
    Plan,
    /// Hand any of the above to someone else for a filter inside this one.
    Grant,
    /// Add events dated before the cutoff.
    Adjust,
    /// Move the cutoff.
    Cutoff,
}

impl Capability {
    /// Every capability.
    pub const ALL: [Self; 8] = [
        Self::View,
        Self::Record,
        Self::Assign,
        Self::Approve,
        Self::Plan,
        Self::Grant,
        Self::Adjust,
        Self::Cutoff,
    ];

    /// The capabilities a grant can hand on to someone else.
    pub const GRANTABLE: [Self; 5] = [
        Self::View,
        Self::Assign,
        Self::Approve,
        Self::Plan,
        Self::Grant,
    ];
}

/// A capability on a filter for a person (assignment 10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    pub person: TagValue,
    pub capability: Capability,
    /// Empty means everything.
    #[serde(default)]
    pub filter: Filter,
}

impl Grant {
    /// Whether this grant answers `can(person, capability, filter)`.
    #[must_use]
    pub fn allows(&self, person: &TagValue, capability: Capability, filter: &Filter) -> bool {
        self.person == *person && self.capability == capability && self.filter.covers(filter)
    }
}

/// `can(person, capability, F)` over a set of grants. Everyone can record.
#[must_use]
pub fn can<'a>(
    grants: impl IntoIterator<Item = &'a Grant>,
    person: &TagValue,
    capability: Capability,
    filter: &Filter,
) -> bool {
    capability == Capability::Record
        || grants
            .into_iter()
            .any(|grant| grant.allows(person, capability, filter))
}

/// A named bundle of grants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Record, and view their own messages. The default for a new person.
    AgentManager,
    /// View, assign, approve, plan and grant on the filters assigned.
    PeopleManager,
    /// Everything, with adjust and cutoff.
    OrgManager,
}

impl Role {
    /// The grants of the role for a person. A people manager gets one set
    /// per filter; an agent manager and an org manager ignore the filters.
    #[must_use]
    pub fn grants(self, person: &TagValue, filters: &[Filter]) -> Vec<Grant> {
        let grant = |capability, filter| Grant {
            person: person.clone(),
            capability,
            filter,
        };
        match self {
            Self::AgentManager => {
                let mut own = Tags::new();
                own.insert(TagName::person(), person.clone());
                vec![grant(Capability::View, Filter(own))]
            }
            Self::PeopleManager => filters
                .iter()
                .flat_map(|filter| {
                    Capability::GRANTABLE
                        .iter()
                        .map(|capability| grant(*capability, filter.clone()))
                })
                .collect(),
            Self::OrgManager => Capability::ALL
                .iter()
                .filter(|capability| **capability != Capability::Record)
                .map(|capability| grant(*capability, Filter::default()))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(text: &str) -> TagValue {
        TagValue::new(text).unwrap()
    }

    fn filter(pairs: &[&str]) -> Filter {
        Filter(Tags::parse(pairs.iter().copied()).unwrap())
    }

    #[test]
    fn a_view_grant_reads_every_filter_inside_its_own() {
        let grants = vec![Grant {
            person: person("lea"),
            capability: Capability::View,
            filter: filter(&["client=acme"]),
        }];
        let lea = person("lea");
        assert!(can(
            &grants,
            &lea,
            Capability::View,
            &filter(&["client=acme"])
        ));
        assert!(can(
            &grants,
            &lea,
            Capability::View,
            &filter(&["client=acme", "project=shop"])
        ));
        assert!(!can(&grants, &lea, Capability::View, &Filter::default()));
        assert!(!can(
            &grants,
            &lea,
            Capability::View,
            &filter(&["client=globex"])
        ));
        assert!(!can(
            &grants,
            &lea,
            Capability::Assign,
            &filter(&["client=acme"])
        ));
        assert!(!can(
            &grants,
            &person("tom"),
            Capability::View,
            &filter(&["client=acme"])
        ));
    }

    #[test]
    fn a_grant_answers_only_for_its_own_person_and_capability() {
        let grant = Grant {
            person: person("lea"),
            capability: Capability::Assign,
            filter: filter(&["team=web"]),
        };
        assert!(grant.allows(&person("lea"), Capability::Assign, &filter(&["team=web"])));
        assert!(grant.allows(
            &person("lea"),
            Capability::Assign,
            &filter(&["team=web", "feature=KD-1"])
        ));
        assert!(!grant.allows(&person("lea"), Capability::View, &filter(&["team=web"])));
        assert!(!grant.allows(&person("tom"), Capability::Assign, &filter(&["team=web"])));
        assert!(!grant.allows(&person("lea"), Capability::Assign, &filter(&["team=data"])));
        assert!(!grant.allows(&person("lea"), Capability::Assign, &Filter::default()));
    }

    #[test]
    fn everyone_can_record_without_a_grant() {
        let nobody: Vec<Grant> = Vec::new();
        assert!(can(
            &nobody,
            &person("tom"),
            Capability::Record,
            &Filter::default()
        ));
        assert!(can(
            &nobody,
            &person("tom"),
            Capability::Record,
            &filter(&["client=acme"])
        ));
        for capability in Capability::ALL {
            if capability != Capability::Record {
                assert!(
                    !can(&nobody, &person("tom"), capability, &Filter::default()),
                    "{capability:?}"
                );
            }
        }
    }

    #[test]
    fn a_new_person_views_only_their_own_messages() {
        let grants = Role::AgentManager.grants(&person("tom"), &[filter(&["client=acme"])]);
        assert_eq!(grants.len(), 1);
        let tom = person("tom");
        assert!(can(
            &grants,
            &tom,
            Capability::View,
            &filter(&["person=tom"])
        ));
        assert!(can(
            &grants,
            &tom,
            Capability::View,
            &filter(&["person=tom", "project=shop"])
        ));
        assert!(!can(
            &grants,
            &tom,
            Capability::View,
            &filter(&["person=mara"])
        ));
        assert!(!can(
            &grants,
            &tom,
            Capability::View,
            &filter(&["project=shop"])
        ));
    }

    #[test]
    fn a_people_manager_holds_the_grantable_capabilities_on_each_filter() {
        let lea = person("lea");
        let grants =
            Role::PeopleManager.grants(&lea, &[filter(&["team=web"]), filter(&["client=acme"])]);
        assert_eq!(grants.len(), 10);
        for capability in Capability::GRANTABLE {
            assert!(
                can(&grants, &lea, capability, &filter(&["team=web"])),
                "{capability:?}"
            );
            assert!(
                can(&grants, &lea, capability, &filter(&["client=acme"])),
                "{capability:?}"
            );
            assert!(
                !can(&grants, &lea, capability, &Filter::default()),
                "{capability:?}"
            );
        }
        assert!(!can(
            &grants,
            &lea,
            Capability::Adjust,
            &filter(&["team=web"])
        ));
        assert!(!can(
            &grants,
            &lea,
            Capability::Cutoff,
            &filter(&["team=web"])
        ));
        assert!(Role::PeopleManager.grants(&lea, &[]).is_empty());
    }

    #[test]
    fn an_org_manager_holds_everything_on_the_empty_filter() {
        let jan = person("jan");
        let grants = Role::OrgManager.grants(&jan, &[filter(&["team=web"])]);
        assert_eq!(grants.len(), 7);
        for capability in Capability::ALL {
            assert!(
                can(&grants, &jan, capability, &Filter::default()),
                "{capability:?}"
            );
            assert!(
                can(&grants, &jan, capability, &filter(&["client=acme"])),
                "{capability:?}"
            );
        }
        assert!(!can(
            &grants,
            &person("tom"),
            Capability::View,
            &Filter::default()
        ));
    }

    #[test]
    fn grants_read_from_json_with_everything_as_the_default_filter() {
        let grant: Grant =
            serde_json::from_str(r#"{"person": "jan", "capability": "cutoff"}"#).unwrap();
        assert!(grant.filter.is_everything());
        assert_eq!(grant.capability, Capability::Cutoff);
        assert_eq!(
            serde_json::to_string(&Role::OrgManager).unwrap(),
            r#""org_manager""#
        );
        assert_eq!(Capability::ALL.len(), 8);
        assert_eq!(Capability::GRANTABLE.len(), 5);
    }
}
