//! The three roles (docs: Account > Grants and roles, ADR 0023): named
//! bundles of grants. An agent manager records and views their own
//! messages; a people manager holds view, assign, approve, plan and grant on
//! the filters they were granted; an org manager holds every capability on
//! the empty filter. Roles are ordered so a check for "at least" works.

use std::fmt;
use std::str::FromStr;

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
    /// The value stored in the database.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AgentManager => "agent_manager",
            Self::PeopleManager => "people_manager",
            Self::OrgManager => "org_manager",
        }
    }

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

/// A role name that is not one of the three.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("unknown role {0:?}")]
pub struct UnknownRole(pub String);

impl FromStr for Role {
    type Err = UnknownRole;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "agent_manager" => Ok(Self::AgentManager),
            "people_manager" => Ok(Self::PeopleManager),
            "org_manager" => Ok(Self::OrgManager),
            other => Err(UnknownRole(other.to_owned())),
        }
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
    fn roles_round_trip_through_the_database_value() {
        for role in [Role::AgentManager, Role::PeopleManager, Role::OrgManager] {
            assert_eq!(role.as_str().parse::<Role>(), Ok(role));
        }
        assert_eq!(
            "controller".parse::<Role>(),
            Err(UnknownRole("controller".to_owned()))
        );
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
