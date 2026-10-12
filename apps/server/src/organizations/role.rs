//! The three roles (docs: Account > Roles). Roles stack: each includes
//! everything the one before it can do, so they are ordered.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// What a member can see and do.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, ToSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Reviews and submits their own token sheets; sees their own cost.
    AgentManager,
    /// Reads reports, sets token budgets and cadence, approves and rejects,
    /// for the filters they are assigned.
    PeopleManager,
    /// Sees everything, sets up the organization, runs the close.
    Controller,
}

impl Role {
    /// The value stored in the database.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AgentManager => "agent_manager",
            Self::PeopleManager => "people_manager",
            Self::Controller => "controller",
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::AgentManager => "agent manager",
            Self::PeopleManager => "people manager",
            Self::Controller => "controller",
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
            "controller" => Ok(Self::Controller),
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
        assert!(Role::PeopleManager < Role::Controller);
    }

    #[test]
    fn roles_round_trip_through_the_database_value() {
        for role in [Role::AgentManager, Role::PeopleManager, Role::Controller] {
            assert_eq!(role.as_str().parse::<Role>(), Ok(role));
        }
        assert_eq!(
            "admin".parse::<Role>(),
            Err(UnknownRole("admin".to_owned()))
        );
    }

    #[test]
    fn roles_read_as_glossary_words() {
        assert_eq!(Role::PeopleManager.to_string(), "people manager");
        assert_eq!(
            serde_json::to_string(&Role::AgentManager).unwrap(),
            "\"agent_manager\""
        );
    }
}
