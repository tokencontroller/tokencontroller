//! Contracts: one thing a provider charges you under, metered or flat
//! (ADR 0028).
//!
//! A contract is a value of the built-in tag name `contract`. It goes on
//! both tracks: a bill has exactly one contract, and a message gets at most
//! one, set on the whole message before it is priced. The check compares a
//! bill with the messages that carry the same contract.

use serde::{Deserialize, Serialize};

use crate::money::Money;
use crate::split::SplitTarget;
use crate::tags::TagValue;

/// How a provider charges under a contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractKind {
    /// Paid per token: the bill should equal the cost of its messages.
    Metered,
    /// A fixed fee, such as a seat plan, plus any extra usage. Cost under
    /// it is marked "on a plan".
    Flat,
}

impl ContractKind {
    /// The kind as reports write it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Metered => "metered",
            Self::Flat => "flat",
        }
    }
}

/// What a contract holds besides its key: its provider, its kind and, when
/// flat, the fee per month. Prices of its own are price rows that name the
/// contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractTerms {
    /// The provider that charges under the contract, spelled as bills
    /// spell it, such as `anthropic`. A message under the contract has this
    /// provider, whatever the sender reported.
    pub provider: String,
    pub kind: ContractKind,
    /// The fee per calendar month of a flat contract, when known.
    #[serde(default)]
    pub fee: Option<Money>,
}

impl ContractTerms {
    /// Metered terms for a provider.
    #[must_use]
    pub fn metered(provider: &str) -> Self {
        Self {
            provider: provider.to_owned(),
            kind: ContractKind::Metered,
            fee: None,
        }
    }
}

/// Sets the contract of every message of a session, or of one message, by
/// hand. One on a message wins over one on its session. A later one on the
/// same target supersedes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractAssignment {
    pub target: SplitTarget,
    pub contract: TagValue,
}

/// How a message got its contract, in the order the steps are tried.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractBy {
    /// Someone set it on the message or its session.
    Hand,
    /// The sender sent a `contract` signal with the message.
    Sender,
    /// A rule matched a signal, such as how the agent was signed in.
    Rule,
    /// Every split part's project brings the same contract.
    ProjectDefault,
    /// The message's provider has exactly one contract.
    Fallback,
}

impl ContractBy {
    /// The step as reports write it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hand => "hand",
            Self::Sender => "sender",
            Self::Rule => "rule",
            Self::ProjectDefault => "project default",
            Self::Fallback => "fallback",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::SessionId;
    use crate::money::Currency;

    #[test]
    fn kinds_and_steps_are_written_in_plain_words() {
        assert_eq!(ContractKind::Metered.as_str(), "metered");
        assert_eq!(ContractKind::Flat.as_str(), "flat");
        let steps: Vec<&str> = [
            ContractBy::Hand,
            ContractBy::Sender,
            ContractBy::Rule,
            ContractBy::ProjectDefault,
            ContractBy::Fallback,
        ]
        .iter()
        .map(|by| by.as_str())
        .collect();
        assert_eq!(
            steps,
            vec!["hand", "sender", "rule", "project default", "fallback"]
        );
        assert_eq!(
            serde_json::to_string(&ContractBy::ProjectDefault).unwrap(),
            r#""project_default""#
        );
    }

    #[test]
    fn terms_read_and_write_as_json() {
        let metered = ContractTerms::metered("anthropic");
        assert_eq!(metered.kind, ContractKind::Metered);
        assert_eq!(metered.fee, None);
        let read: ContractTerms =
            serde_json::from_str(r#"{"provider": "anthropic", "kind": "metered"}"#).unwrap();
        assert_eq!(read, metered);
        let flat = ContractTerms {
            provider: "anthropic".to_owned(),
            kind: ContractKind::Flat,
            fee: Some(Money::parse("150", Currency::USD).unwrap()),
        };
        let again: ContractTerms =
            serde_json::from_str(&serde_json::to_string(&flat).unwrap()).unwrap();
        assert_eq!(again, flat);
        let assignment = ContractAssignment {
            target: SplitTarget::Session(SessionId::new("s1").unwrap()),
            contract: TagValue::new("claude-team").unwrap(),
        };
        assert_eq!(
            serde_json::to_string(&assignment).unwrap(),
            r#"{"target":{"session":"s1"},"contract":"claude-team"}"#
        );
    }
}
