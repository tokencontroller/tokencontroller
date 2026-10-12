//! The tag map's ladder and the cost kind it gives a split (docs: Account >
//! Tags and the tag map). The cost object is the first rung with a value,
//! else the organization; the cost kind follows from the ticket and project
//! tags and is never set by hand.

use serde::Serialize;
use utoipa::ToSchema;

use crate::reports::filter::Tags;

/// How specific a split's cost object is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CostKind {
    /// On a ticket.
    Direct,
    /// On a project but on no ticket, including rungs between ticket and
    /// project such as an epic.
    ProjectOverhead,
    /// On no project.
    OrganizationOverhead,
}

impl CostKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::ProjectOverhead => "project_overhead",
            Self::OrganizationOverhead => "organization_overhead",
        }
    }
}

/// The ladder of tag names that fills the cost object, most specific first.
/// The organization is the implicit last rung.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ladder(Vec<String>);

impl Default for Ladder {
    /// Ticket, then project, then the organization.
    fn default() -> Self {
        Self(vec!["ticket".to_owned(), "project".to_owned()])
    }
}

/// A split's cost object: the rung and its value, or the organization.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CostObject {
    Rung { name: String, value: String },
    Organization,
}

impl Ladder {
    #[must_use]
    pub const fn new(rungs: Vec<String>) -> Self {
        Self(rungs)
    }

    #[must_use]
    pub fn rungs(&self) -> &[String] {
        &self.0
    }

    /// The first rung of the ladder with a value.
    #[must_use]
    pub fn cost_object(&self, tags: &Tags) -> CostObject {
        self.0
            .iter()
            .find_map(|rung| {
                tags.get(rung).map(|value| CostObject::Rung {
                    name: rung.clone(),
                    value: value.clone(),
                })
            })
            .unwrap_or(CostObject::Organization)
    }

    /// The cost kind of a split with these tags: direct with a ticket,
    /// project overhead with a project or a rung below it, otherwise
    /// organization overhead.
    #[must_use]
    pub fn cost_kind(&self, tags: &Tags) -> CostKind {
        if tags.contains_key("ticket") {
            return CostKind::Direct;
        }
        let up_to_project = self
            .0
            .iter()
            .position(|rung| rung == "project")
            .map_or(&[][..], |at| self.0.get(..=at).unwrap_or(&[]));
        if tags.contains_key("project") || up_to_project.iter().any(|rung| tags.contains_key(rung)) {
            CostKind::ProjectOverhead
        } else {
            CostKind::OrganizationOverhead
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(pairs: &[(&str, &str)]) -> Tags {
        pairs
            .iter()
            .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn the_default_ladder_is_ticket_then_project() {
        let ladder = Ladder::default();
        assert_eq!(
            ladder.cost_object(&tags(&[("ticket", "KD-214"), ("project", "storefront")])),
            CostObject::Rung {
                name: "ticket".to_owned(),
                value: "KD-214".to_owned()
            }
        );
        assert_eq!(
            ladder.cost_object(&tags(&[("project", "storefront"), ("team", "web")])),
            CostObject::Rung {
                name: "project".to_owned(),
                value: "storefront".to_owned()
            }
        );
        assert_eq!(ladder.cost_object(&tags(&[("team", "web")])), CostObject::Organization);
    }

    #[test]
    fn the_kind_follows_from_ticket_and_project() {
        let ladder = Ladder::default();
        assert_eq!(ladder.cost_kind(&tags(&[("ticket", "KD-1")])), CostKind::Direct);
        assert_eq!(
            ladder.cost_kind(&tags(&[("project", "storefront")])),
            CostKind::ProjectOverhead
        );
        assert_eq!(
            ladder.cost_kind(&tags(&[("team", "web")])),
            CostKind::OrganizationOverhead
        );
    }

    #[test]
    fn rungs_between_ticket_and_project_are_project_overhead() {
        let ladder = Ladder::new(
            ["ticket", "epic", "project", "client"]
                .map(str::to_owned)
                .to_vec(),
        );
        assert_eq!(ladder.cost_kind(&tags(&[("epic", "KD-100")])), CostKind::ProjectOverhead);
        assert_eq!(
            ladder.cost_kind(&tags(&[("client", "acme")])),
            CostKind::OrganizationOverhead
        );
        assert_eq!(
            ladder.cost_object(&tags(&[("client", "acme")])),
            CostObject::Rung {
                name: "client".to_owned(),
                value: "acme".to_owned()
            }
        );
    }

    #[test]
    fn a_ladder_without_project_has_no_project_overhead() {
        let ladder = Ladder::new(vec!["ticket".to_owned(), "client".to_owned()]);
        assert_eq!(
            ladder.cost_kind(&tags(&[("client", "acme")])),
            CostKind::OrganizationOverhead
        );
        assert_eq!(
            ladder.cost_kind(&tags(&[("project", "storefront")])),
            CostKind::ProjectOverhead
        );
        assert_eq!(CostKind::Direct.as_str(), "direct");
        assert_eq!(CostKind::ProjectOverhead.as_str(), "project_overhead");
        assert_eq!(CostKind::OrganizationOverhead.as_str(), "organization_overhead");
    }
}
