//! Messages: one model call each, the unit cost is measured in (ADR 0006).
//!
//! A token entry is the sum of a session's messages. Telemetry events and
//! session-file entries for the same message are the same message, matched
//! by session ID and message ID, so a session seen through both doors is
//! counted once.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::money::Money;
use crate::tokens::TokenCounts;

/// An identifier that cannot be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is not an identifier: use 1 to 200 characters without spaces at either end")]
pub struct IdError(pub String);

macro_rules! text_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            /// Reads the identifier as the agent harness wrote it.
            ///
            /// # Errors
            ///
            /// [`IdError`] if it is empty, longer than 200 characters, or
            /// starts or ends with a space.
            pub fn new(id: &str) -> Result<Self, IdError> {
                if id.is_empty() || id.trim() != id || id.chars().count() > 200 {
                    Err(IdError(id.to_owned()))
                } else {
                    Ok(Self(id.to_owned()))
                }
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = IdError;

            fn try_from(id: String) -> Result<Self, Self::Error> {
                Self::new(&id)
            }
        }

        impl From<$name> for String {
            fn from(id: $name) -> Self {
                id.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

text_id!(
    /// The session ID an agent harness gives a session.
    SessionId
);
text_id!(
    /// The message ID an agent harness gives one model call.
    MessageId
);

/// A model and the provider that charges for it, such as `anthropic` and
/// `claude-sonnet-4-5`. Together they decide the price.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ModelRef {
    pub provider: String,
    pub model: String,
}

impl ModelRef {
    #[must_use]
    pub fn new(provider: &str, model: &str) -> Self {
        Self {
            provider: provider.to_owned(),
            model: model.to_owned(),
        }
    }
}

impl fmt::Display for ModelRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.provider, self.model)
    }
}

/// What one or more model calls used, and what they cost where that is
/// known.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub model: ModelRef,
    pub tokens: TokenCounts,
    /// The agent's own cost figure at list prices, where it gives one.
    #[serde(default)]
    pub reported_cost: Option<Money>,
    /// What the provider actually charged, where billed telemetry says so.
    #[serde(default)]
    pub billed_cost: Option<Money>,
}

/// Facts about a session that hint at its tags. Prompts and titles are read
/// on the machine; only the ticket keys found in them leave it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Signals {
    pub branch: Option<String>,
    pub working_directory: Option<String>,
    /// The repository, such as `kestrel/storefront`.
    pub repository: Option<String>,
    pub pull_request: Option<u64>,
    pub subagent_type: Option<String>,
    /// Ticket keys in the prompts, in the order they were named, once per
    /// mention.
    pub prompt_ticket_keys: Vec<String>,
    /// Ticket keys in the session title.
    pub session_title_ticket_keys: Vec<String>,
    /// Ticket keys in the pull request title.
    pub pull_request_title_ticket_keys: Vec<String>,
}

/// The signals that can change from one message to the next.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MessageSignals {
    pub branch: Option<String>,
    pub working_directory: Option<String>,
    pub repository: Option<String>,
    pub pull_request: Option<u64>,
    /// Set when a subagent made the call, such as `code-reviewer`.
    pub subagent_type: Option<String>,
    /// Ticket keys in the prompt that led to this call, once per mention.
    pub prompt_ticket_keys: Vec<String>,
}

/// One model call, as telemetry or a session file reports it. No text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub session_id: SessionId,
    pub message_id: MessageId,
    /// The provider's request ID, where the agent harness gives it. Bills
    /// join on it.
    #[serde(default)]
    pub request_id: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
    #[serde(flatten)]
    pub usage: Usage,
    #[serde(default)]
    pub signals: MessageSignals,
}

impl Message {
    /// Joins two reports of the same message, such as its telemetry event
    /// and its session-file entry. Whatever one of them lacks comes from the
    /// other; where both have a value, `self` wins.
    #[must_use]
    pub fn merge(self, other: Self) -> Self {
        let tokens = if self.usage.tokens.is_zero() {
            other.usage.tokens
        } else {
            self.usage.tokens
        };
        let signals = MessageSignals {
            branch: self.signals.branch.or(other.signals.branch),
            working_directory: self
                .signals
                .working_directory
                .or(other.signals.working_directory),
            repository: self.signals.repository.or(other.signals.repository),
            pull_request: self.signals.pull_request.or(other.signals.pull_request),
            subagent_type: self.signals.subagent_type.or(other.signals.subagent_type),
            prompt_ticket_keys: if self.signals.prompt_ticket_keys.is_empty() {
                other.signals.prompt_ticket_keys
            } else {
                self.signals.prompt_ticket_keys
            },
        };
        Self {
            session_id: self.session_id,
            message_id: self.message_id,
            request_id: self.request_id.or(other.request_id),
            at: self.at,
            usage: Usage {
                model: self.usage.model,
                tokens,
                reported_cost: self.usage.reported_cost.or(other.usage.reported_cost),
                billed_cost: self.usage.billed_cost.or(other.usage.billed_cost),
            },
            signals,
        }
    }
}

/// Counts every message once: reports with the same session ID and message
/// ID are joined with [`Message::merge`], earlier reports winning. The
/// result is in the order each message was first seen.
#[must_use]
pub fn dedupe(messages: Vec<Message>) -> Vec<Message> {
    let mut position: BTreeMap<(SessionId, MessageId), usize> = BTreeMap::new();
    let mut unique: Vec<Option<Message>> = Vec::with_capacity(messages.len());
    for message in messages {
        let key = (message.session_id.clone(), message.message_id.clone());
        if let Some(slot) = position.get(&key).and_then(|index| unique.get_mut(*index)) {
            *slot = slot.take().map(|kept| kept.merge(message));
        } else {
            position.insert(key, unique.len());
            unique.push(Some(message));
        }
    }
    unique.into_iter().flatten().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::money::Currency;
    use time::macros::datetime;

    fn message(id: &str) -> Message {
        Message {
            session_id: SessionId::new("s1").unwrap(),
            message_id: MessageId::new(id).unwrap(),
            request_id: None,
            at: datetime!(2026-10-05 09:12 UTC),
            usage: Usage {
                model: ModelRef::new("anthropic", "claude-sonnet-4-5"),
                tokens: TokenCounts {
                    input: 10,
                    ..TokenCounts::default()
                },
                reported_cost: None,
                billed_cost: None,
            },
            signals: MessageSignals::default(),
        }
    }

    #[test]
    fn identifiers_are_short_trimmed_text() {
        assert_eq!(SessionId::new("abc-1").unwrap().as_str(), "abc-1");
        assert_eq!(MessageId::new("m").unwrap().to_string(), "m");
        assert!(SessionId::new(&"x".repeat(200)).is_ok());
        for bad in ["", " a", "a ", &"x".repeat(201)] {
            assert_eq!(SessionId::new(bad), Err(IdError(bad.to_owned())));
            assert!(MessageId::new(bad).is_err());
        }
        assert!(serde_json::from_str::<SessionId>(r#""""#).is_err());
        assert_eq!(
            serde_json::to_string(&MessageId::new("m1").unwrap()).unwrap(),
            r#""m1""#
        );
        assert_eq!(
            ModelRef::new("anthropic", "claude-opus-4-1").to_string(),
            "anthropic/claude-opus-4-1"
        );
    }

    #[test]
    fn two_reports_of_one_message_are_counted_once() {
        let mut telemetry = message("m1");
        telemetry.usage.billed_cost = Some(Money::from_nanos(5, Currency::USD));
        telemetry.request_id = Some("req_1".to_owned());
        telemetry.usage.tokens = TokenCounts::default();
        let mut session_file = message("m1");
        session_file.signals.branch = Some("KD-214-fix".to_owned());
        session_file.usage.reported_cost = Some(Money::from_nanos(4, Currency::USD));
        let other = message("m2");

        let unique = dedupe(vec![telemetry, other.clone(), session_file]);
        assert_eq!(unique.len(), 2);
        let joined = unique.first().unwrap();
        assert_eq!(joined.message_id.as_str(), "m1");
        assert_eq!(joined.request_id.as_deref(), Some("req_1"));
        assert_eq!(joined.signals.branch.as_deref(), Some("KD-214-fix"));
        assert_eq!(joined.usage.tokens.input, 10);
        assert_eq!(
            joined.usage.billed_cost,
            Some(Money::from_nanos(5, Currency::USD))
        );
        assert_eq!(
            joined.usage.reported_cost,
            Some(Money::from_nanos(4, Currency::USD))
        );
        assert_eq!(unique.get(1), Some(&other));
    }

    #[test]
    fn the_first_report_wins_where_both_have_a_value() {
        let mut first = message("m1");
        first.signals = MessageSignals {
            branch: Some("a".to_owned()),
            working_directory: Some("/a".to_owned()),
            repository: Some("r/a".to_owned()),
            pull_request: Some(1),
            subagent_type: Some("x".to_owned()),
            prompt_ticket_keys: vec!["KD-1".to_owned()],
        };
        first.usage.reported_cost = Some(Money::from_nanos(1, Currency::USD));
        first.usage.billed_cost = Some(Money::from_nanos(1, Currency::USD));
        first.request_id = Some("req_a".to_owned());
        let mut second = message("m1");
        second.signals = MessageSignals {
            branch: Some("b".to_owned()),
            working_directory: Some("/b".to_owned()),
            repository: Some("r/b".to_owned()),
            pull_request: Some(2),
            subagent_type: Some("y".to_owned()),
            prompt_ticket_keys: vec!["KD-2".to_owned()],
        };
        second.usage.tokens.input = 99;
        second.usage.reported_cost = Some(Money::from_nanos(2, Currency::USD));
        second.usage.billed_cost = Some(Money::from_nanos(2, Currency::USD));
        second.request_id = Some("req_b".to_owned());
        second.at = datetime!(2026-10-06 00:00 UTC);

        let joined = first.clone().merge(second.clone());
        assert_eq!(joined, first);
        let reversed = second.clone().merge(first);
        assert_eq!(reversed, second);
    }

    #[test]
    fn a_report_without_values_takes_them_from_the_other() {
        let mut empty = message("m1");
        empty.usage.tokens = TokenCounts::default();
        let mut full = message("m1");
        full.signals = MessageSignals {
            branch: Some("b".to_owned()),
            working_directory: Some("/b".to_owned()),
            repository: Some("r/b".to_owned()),
            pull_request: Some(2),
            subagent_type: Some("y".to_owned()),
            prompt_ticket_keys: vec!["KD-2".to_owned()],
        };
        full.usage.reported_cost = Some(Money::from_nanos(2, Currency::USD));
        full.usage.billed_cost = Some(Money::from_nanos(3, Currency::USD));
        full.request_id = Some("req_b".to_owned());
        let joined = empty.merge(full.clone());
        assert_eq!(joined.signals, full.signals);
        assert_eq!(joined.usage, full.usage);
        assert_eq!(joined.request_id, full.request_id);
    }

    #[test]
    fn messages_read_from_json() {
        let json = r#"{
            "session_id": "s1", "message_id": "m1",
            "at": "2026-10-05T09:12:00Z",
            "model": {"provider": "anthropic", "model": "claude-sonnet-4-5"},
            "tokens": {"input": 10, "output": 2},
            "signals": {"branch": "KD-214-fix", "prompt_ticket_keys": ["KD-214"]}
        }"#;
        let read: Message = serde_json::from_str(json).unwrap();
        assert_eq!(read.at, datetime!(2026-10-05 09:12 UTC));
        assert_eq!(read.usage.tokens.output, 2);
        assert_eq!(read.signals.prompt_ticket_keys, vec!["KD-214"]);
        let again: Message = serde_json::from_str(&serde_json::to_string(&read).unwrap()).unwrap();
        assert_eq!(again, read);
    }
}
