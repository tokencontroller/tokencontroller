//! The tag map: which tags fill the cost center and the cost object, and
//! the cost kind that follows from the ticket and project tags.

use serde::{Deserialize, Serialize};

use crate::tags::{TagName, TagValue, Tags};

/// A tag map that cannot be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TagMapError {
    #[error("the ladder names {0} twice: each tag name can be one rung only")]
    DuplicateRung(TagName),
}

/// The organization's setting that says which tags fill the cost center and
/// the cost object. By default the cost center is `person` and the ladder is
/// ticket, project; the organization is always the last rung.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "TagMapFields", into = "TagMapFields")]
pub struct TagMap {
    cost_center: TagName,
    ladder: Vec<TagName>,
}

/// How a tag map reads and writes as JSON.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TagMapFields {
    cost_center: TagName,
    ladder: Vec<TagName>,
}

impl TryFrom<TagMapFields> for TagMap {
    type Error = TagMapError;

    fn try_from(fields: TagMapFields) -> Result<Self, Self::Error> {
        Self::new(fields.cost_center, fields.ladder)
    }
}

impl From<TagMap> for TagMapFields {
    fn from(map: TagMap) -> Self {
        Self {
            cost_center: map.cost_center,
            ladder: map.ladder,
        }
    }
}

impl Default for TagMap {
    fn default() -> Self {
        Self {
            cost_center: TagName::person(),
            ladder: vec![TagName::ticket(), TagName::project()],
        }
    }
}

impl TagMap {
    /// A tag map with its cost center tag and its ladder, most specific rung
    /// first. The organization is the rung after the last.
    ///
    /// # Errors
    ///
    /// [`TagMapError::DuplicateRung`] if the ladder names a tag twice.
    pub fn new(cost_center: TagName, ladder: Vec<TagName>) -> Result<Self, TagMapError> {
        for (position, rung) in ladder.iter().enumerate() {
            if ladder.iter().take(position).any(|earlier| earlier == rung) {
                return Err(TagMapError::DuplicateRung(rung.clone()));
            }
        }
        Ok(Self {
            cost_center,
            ladder,
        })
    }

    /// The tag name that fills the cost center.
    #[must_use]
    pub fn cost_center_tag(&self) -> &TagName {
        &self.cost_center
    }

    /// The rungs of the ladder, most specific first, without the
    /// organization.
    #[must_use]
    pub fn ladder(&self) -> &[TagName] {
        &self.ladder
    }

    /// Who answers for a split's cost: the value of the cost center tag.
    #[must_use]
    pub fn cost_center(&self, tags: &Tags) -> CostCenter {
        CostCenter {
            tag: self.cost_center.clone(),
            value: tags.get(&self.cost_center).cloned(),
        }
    }

    /// What a split's cost paid for: the first rung of the ladder with a
    /// value, or else the organization.
    #[must_use]
    pub fn cost_object(&self, tags: &Tags) -> CostObject {
        self.ladder
            .iter()
            .find_map(|rung| {
                tags.get(rung).map(|value| CostObject::Rung {
                    tag: rung.clone(),
                    value: value.clone(),
                })
            })
            .unwrap_or(CostObject::Organization)
    }

    /// How specific a split's cost is. A ticket makes it direct. A project,
    /// or a rung between ticket and project such as an epic, makes it
    /// project overhead. Anything else is organization overhead.
    #[must_use]
    pub fn cost_kind(&self, tags: &Tags) -> CostKind {
        let project = TagName::project();
        if tags.contains(&TagName::ticket()) {
            return CostKind::Direct;
        }
        let below_project = self.ladder.iter().take_while(|rung| **rung != project);
        if tags.contains(&project)
            || (self.ladder.contains(&project)
                && below_project.into_iter().any(|rung| tags.contains(rung)))
        {
            CostKind::ProjectOverhead
        } else {
            CostKind::OrganizationOverhead
        }
    }
}

/// Who answers for a cost: the cost center tag and its value. A split
/// without that tag, possible only when the cost center is not `person`,
/// shows on the line for entries without it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CostCenter {
    pub tag: TagName,
    pub value: Option<TagValue>,
}

/// What a cost paid for.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum CostObject {
    /// The first rung of the ladder with a value, such as `ticket: KD-214`.
    Rung { tag: TagName, value: TagValue },
    /// No rung has a value.
    Organization,
}

/// What a split's cost object says about it. Never set by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostKind {
    /// Cost on a ticket.
    Direct,
    /// Cost on a project but on no ticket.
    ProjectOverhead,
    /// Cost on no project. It still has a person.
    OrganizationOverhead,
}

impl CostKind {
    /// The name the docs and CSV exports use, such as `project overhead`.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::ProjectOverhead => "project overhead",
            Self::OrganizationOverhead => "organization overhead",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(text: &str) -> TagName {
        TagName::new(text).unwrap()
    }

    fn tags(pairs: &[&str]) -> Tags {
        Tags::parse(pairs.iter().copied()).unwrap()
    }

    fn agency_map() -> TagMap {
        TagMap::new(
            name("team"),
            vec![
                name("ticket"),
                name("epic"),
                name("project"),
                name("client"),
            ],
        )
        .unwrap()
    }

    #[test]
    fn the_default_map_is_person_and_ticket_project_organization() {
        let map = TagMap::default();
        assert_eq!(map.cost_center_tag(), &TagName::person());
        assert_eq!(map.ladder(), &[TagName::ticket(), TagName::project()]);
        let entry = tags(&["person=mara", "ticket=KD-214", "project=storefront"]);
        assert_eq!(
            map.cost_center(&entry),
            CostCenter {
                tag: TagName::person(),
                value: Some(TagValue::new("mara").unwrap()),
            }
        );
        assert_eq!(
            map.cost_object(&entry),
            CostObject::Rung {
                tag: TagName::ticket(),
                value: TagValue::new("KD-214").unwrap(),
            }
        );
    }

    #[test]
    fn the_cost_object_is_the_first_rung_with_a_value() {
        let map = agency_map();
        let rung = |tag: &str, value: &str| CostObject::Rung {
            tag: name(tag),
            value: TagValue::new(value).unwrap(),
        };
        assert_eq!(
            map.cost_object(&tags(&["project=acme-shop", "client=acme"])),
            rung("project", "acme-shop")
        );
        assert_eq!(
            map.cost_object(&tags(&["client=acme"])),
            rung("client", "acme")
        );
        assert_eq!(
            map.cost_object(&tags(&["team=web", "purpose=learning"])),
            CostObject::Organization
        );
    }

    #[test]
    fn the_cost_center_can_be_a_team_and_can_be_missing() {
        let map = agency_map();
        assert_eq!(
            map.cost_center(&tags(&["team=web"])).value,
            Some(TagValue::new("web").unwrap())
        );
        assert_eq!(map.cost_center(&tags(&["person=mara"])).value, None);
    }

    #[test]
    fn the_cost_kind_follows_from_ticket_and_project() {
        let map = agency_map();
        assert_eq!(map.cost_kind(&tags(&["ticket=ACME-42"])), CostKind::Direct);
        assert_eq!(
            map.cost_kind(&tags(&["ticket=ACME-42", "project=acme-shop"])),
            CostKind::Direct
        );
        assert_eq!(
            map.cost_kind(&tags(&["project=acme-shop"])),
            CostKind::ProjectOverhead
        );
        // A rung between ticket and project counts as project overhead.
        assert_eq!(
            map.cost_kind(&tags(&["epic=ACME-1"])),
            CostKind::ProjectOverhead
        );
        // A rung above project does not.
        assert_eq!(
            map.cost_kind(&tags(&["client=acme"])),
            CostKind::OrganizationOverhead
        );
        assert_eq!(
            map.cost_kind(&tags(&["person=jonas", "purpose=learning"])),
            CostKind::OrganizationOverhead
        );
    }

    #[test]
    fn without_a_project_rung_only_the_project_tag_makes_project_overhead() {
        let map = TagMap::new(TagName::person(), vec![name("ticket"), name("epic")]).unwrap();
        assert_eq!(
            map.cost_kind(&tags(&["epic=E-1"])),
            CostKind::OrganizationOverhead
        );
        assert_eq!(
            map.cost_kind(&tags(&["project=p"])),
            CostKind::ProjectOverhead
        );
    }

    #[test]
    fn a_rung_is_named_once() {
        assert_eq!(
            TagMap::new(
                TagName::person(),
                vec![name("ticket"), name("epic"), name("ticket")]
            ),
            Err(TagMapError::DuplicateRung(name("ticket")))
        );
        assert!(TagMap::new(TagName::person(), vec![]).is_ok());
    }

    #[test]
    fn cost_kinds_have_their_docs_names() {
        assert_eq!(CostKind::Direct.label(), "direct");
        assert_eq!(CostKind::ProjectOverhead.label(), "project overhead");
        assert_eq!(
            CostKind::OrganizationOverhead.label(),
            "organization overhead"
        );
    }

    #[test]
    fn a_tag_map_reads_and_writes_as_json() {
        let map = agency_map();
        let json = serde_json::to_string(&map).unwrap();
        assert_eq!(
            json,
            r#"{"cost_center":"team","ladder":["ticket","epic","project","client"]}"#
        );
        assert_eq!(serde_json::from_str::<TagMap>(&json).unwrap(), map);
        assert!(
            serde_json::from_str::<TagMap>(r#"{"cost_center":"team","ladder":["a","a"]}"#).is_err()
        );
    }
}
