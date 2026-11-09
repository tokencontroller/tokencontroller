//! Messages: one model call each, the unit cost is measured in (source
//! record 1).
//!
//! A message carries its person, provider, model, token counts by kind and
//! time, plus the signals and session facts around it. Never prompts. A
//! reported cost may be stored as a fact; it is never the figure.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::money::Money;
use crate::tags::Tags;
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
/// `claude-sonnet-4-5`. Together they decide the price. A model written with
/// its provider in front, such as `anthropic/claude-sonnet-4-5`, is the same
/// model.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(from = "StoredModelRef")]
pub struct ModelRef {
    pub provider: String,
    pub model: String,
}

/// A model as the log keeps it, read through [`ModelRef::new`] so entries
/// taken in before the provider was dropped from the name price too.
#[derive(Deserialize)]
struct StoredModelRef {
    provider: String,
    model: String,
}

impl From<StoredModelRef> for ModelRef {
    fn from(stored: StoredModelRef) -> Self {
        Self::new(&stored.provider, &stored.model)
    }
}

impl ModelRef {
    #[must_use]
    pub fn new(provider: &str, model: &str) -> Self {
        let model = model
            .strip_prefix(provider)
            .and_then(|rest| rest.strip_prefix('/'))
            .filter(|rest| !rest.is_empty())
            .unwrap_or(model);
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

/// Signals around a message. Signals are not tags: a rule turns them into a
/// split. Prompts never leave the machine; only keys found in them do.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Signals {
    pub branch: Option<String>,
    pub working_directory: Option<String>,
    /// The repository, such as `kestrel/storefront`.
    pub repository: Option<String>,
    /// The pull request number.
    pub pull_request: Option<u64>,
    pub pull_request_title: Option<String>,
    /// The type of the subagent that ran the session, if one did.
    pub subagent_type: Option<String>,
    /// Keys found in the prompts, such as `KD-1`, once per mention.
    pub prompt_keys: Vec<String>,
    /// Values from a config file in the repository, by name.
    pub config: BTreeMap<String, String>,
    /// Telemetry resource attributes, by name.
    pub resource: BTreeMap<String, String>,
}

/// Facts about the session a message belongs to. All optional.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SessionFacts {
    pub turns: Option<u64>,
    pub tool_calls: Option<u64>,
    pub retries: Option<u64>,
    pub reruns: Option<u64>,
    pub duration_seconds: Option<u64>,
    pub subagents: Option<u64>,
    pub files_touched: Option<u64>,
    /// Where the result is, such as a pull request URL.
    pub result: Option<String>,
    /// A one-line summary of the work. Free text, never a prompt.
    pub summary: Option<String>,
}

/// One model call, as telemetry or a session file reports it. No text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub session_id: SessionId,
    pub message_id: MessageId,
    /// The person whose session it is: the holder of the key or the service
    /// token the agent ran with.
    pub person: crate::tags::TagValue,
    pub model: ModelRef,
    /// Token counts by kind. `None` when the door gave no counts: the
    /// message has no cost and shows as a count error, never as zero.
    #[serde(default)]
    pub tokens: Option<TokenCounts>,
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
    /// The product the agent ran on, such as Claude Code.
    pub agent_harness: String,
    /// An optional label, such as `review bot`.
    #[serde(default)]
    pub agent_name: Option<String>,
    /// The agent's own cost figure, stored as a fact. Never the figure.
    #[serde(default)]
    pub reported_cost: Option<Money>,
    /// Tag keys sent with the message by whoever recorded it, such as
    /// `feature: KD-1`. They are the default split when nobody decides.
    #[serde(default)]
    pub tags: Tags,
    #[serde(default)]
    pub signals: Signals,
    #[serde(default)]
    pub session: SessionFacts,
}

impl Message {
    /// The key a message is counted under: its session and message ids.
    #[must_use]
    pub fn key(&self) -> (SessionId, MessageId) {
        (self.session_id.clone(), self.message_id.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::money::Currency;
    use crate::tags::TagValue;
    use time::macros::datetime;

    fn message(id: &str) -> Message {
        Message {
            session_id: SessionId::new("s1").unwrap(),
            message_id: MessageId::new(id).unwrap(),
            person: TagValue::new("mara").unwrap(),
            model: ModelRef::new("anthropic", "claude-sonnet-4-5"),
            tokens: Some(TokenCounts {
                input: 10,
                ..TokenCounts::default()
            }),
            at: datetime!(2026-10-05 09:12 UTC),
            agent_harness: "claude-code".to_owned(),
            agent_name: None,
            reported_cost: None,
            tags: Tags::new(),
            signals: Signals::default(),
            session: SessionFacts::default(),
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
    fn a_model_written_with_its_provider_is_the_same_model() {
        let plain = ModelRef::new("anthropic", "claude-sonnet-4-5");
        assert_eq!(
            ModelRef::new("anthropic", "anthropic/claude-sonnet-4-5"),
            plain
        );
        // Another provider in front is part of the model's name, as routers write it.
        assert_eq!(
            ModelRef::new("openrouter", "anthropic/claude-sonnet-4-5").model,
            "anthropic/claude-sonnet-4-5"
        );
        assert_eq!(ModelRef::new("anthropic", "anthropic/").model, "anthropic/");
        let stored: ModelRef = serde_json::from_str(
            r#"{"provider":"anthropic","model":"anthropic/claude-sonnet-4-5"}"#,
        )
        .unwrap();
        assert_eq!(stored, plain);
    }

    #[test]
    fn a_message_is_keyed_by_session_and_message_id() {
        let first = message("m1");
        let (session, id) = first.key();
        assert_eq!(session.as_str(), "s1");
        assert_eq!(id.as_str(), "m1");
        assert_ne!(message("m1").key(), message("m2").key());
    }

    #[test]
    fn messages_read_from_json_without_optional_parts() {
        let json = r#"{
            "session_id": "s1", "message_id": "m1", "person": "mara",
            "at": "2026-10-05T09:12:00Z", "agent_harness": "claude-code",
            "model": {"provider": "anthropic", "model": "claude-sonnet-4-5"},
            "tokens": {"input": 10, "output": 2}
        }"#;
        let read: Message = serde_json::from_str(json).unwrap();
        assert_eq!(read.at, datetime!(2026-10-05 09:12 UTC));
        assert_eq!(read.tokens.unwrap().output, 2);
        assert_eq!(read.reported_cost, None);
        assert!(read.tags.is_empty());
        assert_eq!(read.signals, Signals::default());
        assert_eq!(read.session, SessionFacts::default());
        let again: Message = serde_json::from_str(&serde_json::to_string(&read).unwrap()).unwrap();
        assert_eq!(again, read);
    }

    #[test]
    fn a_message_without_counts_reads_as_none_not_zero() {
        let json = r#"{
            "session_id": "s1", "message_id": "m1", "person": "mara",
            "at": "2026-10-05T09:12:00Z", "agent_harness": "claude-code",
            "model": {"provider": "anthropic", "model": "claude-sonnet-4-5"}
        }"#;
        let read: Message = serde_json::from_str(json).unwrap();
        assert_eq!(read.tokens, None);
    }

    #[test]
    fn signals_and_session_facts_keep_what_the_door_sent() {
        let json = r#"{
            "session_id": "s1", "message_id": "m1", "person": "mara",
            "at": "2026-10-05T09:12:00Z", "agent_harness": "claude-code",
            "model": {"provider": "anthropic", "model": "claude-sonnet-4-5"},
            "tokens": {"input": 10},
            "reported_cost": {"nanos": 5, "currency": "USD"},
            "tags": {"feature": "KD-1"},
            "signals": {"branch": "KD-1-fix", "prompt_keys": ["KD-1", "KD-2"],
                        "working_directory": "/w/shop", "pull_request": 7, "subagent_type": "review",
                        "config": {"project": "shop"}, "resource": {"service.name": "bot"}},
            "session": {"turns": 3, "result": "https://example.test/pr/1", "summary": "fixed it"}
        }"#;
        let read: Message = serde_json::from_str(json).unwrap();
        assert_eq!(
            read.reported_cost,
            Some(Money::from_nanos(5, Currency::USD))
        );
        assert_eq!(read.tags.get_str("feature").unwrap().as_str(), "KD-1");
        assert_eq!(read.signals.branch.as_deref(), Some("KD-1-fix"));
        assert_eq!(read.signals.working_directory.as_deref(), Some("/w/shop"));
        assert_eq!(read.signals.pull_request, Some(7));
        assert_eq!(read.signals.subagent_type.as_deref(), Some("review"));
        assert_eq!(read.signals.prompt_keys, vec!["KD-1", "KD-2"]);
        assert_eq!(read.signals.config.get("project").unwrap(), "shop");
        assert_eq!(read.signals.resource.get("service.name").unwrap(), "bot");
        assert_eq!(read.session.turns, Some(3));
        assert_eq!(read.session.summary.as_deref(), Some("fixed it"));
        assert!(serde_json::from_str::<Signals>(r#"{"prompt": "secret"}"#).is_err());
        assert!(serde_json::from_str::<SessionFacts>(r#"{"prompt": "secret"}"#).is_err());
    }
}
