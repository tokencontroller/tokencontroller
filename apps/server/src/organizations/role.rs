//! The three roles (docs: Account > Grants and roles, ADR 0023): named
//! bundles of grants. An agent manager records and views their own
//! messages; a people manager holds view, assign, approve, plan and grant on
//! the filters they were granted; an org manager holds every capability on
//! the empty filter. Roles are ordered so a check for "at least" works.

use std::fmt;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// The role a member holds.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, ToSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Anyone who uses an agent: record, and view on `person: self`.
    AgentManager,
    /// Tech leads, heads of department, project and account managers: view,
    /// assign, approve, plan and grant on their filters.
    PeopleManager,
    /// Whoever answers for the numbers as a whole: every capability on the
    /// empty filter, adjust and the cutoff.
    OrgManager,
}

impl Role {
    /// The core crate's role, whose grants this role bundles.
    #[must_use]
    pub const fn core(self) -> tokencontroller_core::Role {
        match self {
            Self::AgentManager => tokencontroller_core::Role::AgentManager,
            Self::PeopleManager => tokencontroller_core::Role::PeopleManager,
            Self::OrgManager => tokencontroller_core::Role::OrgManager,
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::AgentManager => "agent manager",
            Self::PeopleManager => "people manager",
            Self::OrgManager => "org manager",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_stack() {
        assert!(Role::AgentManager < Role::PeopleManager);
        assert!(Role::PeopleManager < Role::OrgManager);
    }

    #[test]
    fn roles_read_as_glossary_words() {
        assert_eq!(Role::OrgManager.to_string(), "org manager");
        assert_eq!(
            serde_json::to_string(&Role::OrgManager).unwrap(),
            "\"org_manager\""
        );
        assert_eq!(
            Role::OrgManager.core(),
            tokencontroller_core::Role::OrgManager
        );
    }
}
