//! A token entry as the token entries endpoint, manual entry and CSV import
//! take it (the docs page Reference > Token entry schema), and the checks it
//! must pass before anything is stored.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tokencontroller_core::{Currency, Money};
use utoipa::ToSchema;

use super::model::TokenCounts;
use crate::integrations::decimal::{money_from_decimal, money_from_json_number};

/// The longest agent harness, agent name, model, ticket, project, session
/// or message ID, tag name or tag value accepted.
pub const MAX_LABEL_LEN: usize = 200;

/// The longest summary accepted: a short report, never a transcript.
pub const MAX_SUMMARY_LEN: usize = 10_000;

/// The most messages one token entry may carry.
pub const MAX_MESSAGES: usize = 20_000;

/// The tag name Token Controller sets itself, from the token.
pub const PERSON_TAG: &str = "person";

/// A decimal amount in US dollars, as a JSON number (`0.031`) or a decimal
/// string (`"0.031"`), read without floats.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Amount {
    Number(serde_json::Number),
    Text(String),
}

impl Amount {
    fn money(&self) -> Result<Money, crate::integrations::decimal::DecimalError> {
        match self {
            Self::Number(number) => money_from_json_number(number, Currency::USD),
            Self::Text(text) => money_from_decimal(text, Currency::USD),
        }
    }
}

/// One token entry as it arrives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewTokenEntry {
    /// The product the agent ran on, such as `claude-code` or `support-desk`.
    pub agent_harness: String,
    /// A label for the agent, such as "review bot".
    #[serde(default)]
    pub agent_name: Option<String>,
    /// The model that did the work.
    pub model: String,
    /// Who charges for the model calls: `anthropic`, `openai`, `google`,
    /// `aws_bedrock`, `gcp_vertex`, `azure_foundry`, `github`, `cursor` or
    /// `other`. Without it, it follows from the model's name.
    #[serde(default)]
    pub provider: Option<String>,
    /// The effort setting the agent ran with, such as `high`.
    #[serde(default)]
    pub effort: Option<String>,
    /// The speed setting the agent ran with, such as `fast`.
    #[serde(default)]
    pub speed: Option<String>,
    /// When the session started.
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub started_at: OffsetDateTime,
    /// When the session ended.
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub ended_at: OffsetDateTime,
    /// Counts by kind.
    #[schema(value_type = super::model::TokenCountsSchema)]
    pub tokens: TokenCounts,
    /// The agent's own cost figure in US dollars. Without it the list value
    /// is computed from the model and the price table.
    #[serde(default)]
    #[schema(value_type = Option<String>, example = "0.031")]
    pub reported_cost: Option<Amount>,
    /// What the session did, from one line to a short report.
    #[serde(default)]
    pub summary: Option<String>,
    /// A ticket key, such as `KD-214`. Wins over every rule.
    #[serde(default)]
    pub ticket: Option<String>,
    /// A project, by name.
    #[serde(default)]
    pub project: Option<String>,
    /// Tags from the organization's list, one value per tag name.
    #[serde(default)]
    pub tags: BTreeMap<String, String>,
    /// Facts about the session that hint at its tags.
    #[serde(default)]
    pub signals: Signals,
    /// Joins the token entry to telemetry.
    #[serde(default)]
    pub session_id: Option<String>,
    /// The session's model calls, when the agent knows them. With them, a
    /// session seen by telemetry too is counted exactly once (ADR 0006).
    #[serde(default)]
    pub messages: Vec<NewMessage>,
    /// How the token entry is split, when it is: segments by message IDs
    /// (exact) or shares by hand. Without splits it is one split.
    #[serde(default)]
    pub splits: Vec<NewSplit>,
    /// What placed it: a rule's name, `hand`, `classifier` or `none`.
    #[serde(default)]
    pub placed_by: Option<String>,
    /// The sender's own stable ID for this token entry, such as
    /// `claude-code:<session id>`. Sending it again replaces the earlier one.
    #[serde(default)]
    pub local_id: Option<String>,
}

/// One share of a token entry with its own tags.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewSplit {
    /// A segment: the messages it holds. Exact.
    #[serde(default)]
    pub message_ids: Vec<String>,
    /// A share by hand, in percent.
    #[serde(default)]
    pub share_percent: Option<u32>,
    #[serde(default)]
    pub tags: BTreeMap<String, String>,
    #[serde(default)]
    pub signals: Signals,
}

/// Facts about a session that hint at its tags.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Signals {
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub working_directory: Option<String>,
    /// Such as `github.com/kestrel/storefront`.
    #[serde(default)]
    pub repository: Option<String>,
    #[serde(default)]
    pub pull_request: Option<u64>,
    #[serde(default)]
    pub subagent_type: Option<String>,
    /// Ticket keys found in prompts, the session title and the pull request
    /// title. Only the keys, never the text.
    #[serde(default)]
    pub ticket_keys: Vec<String>,
}

/// One model call of a token entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewMessage {
    /// The agent harness's ID for this call, unique within the session.
    pub message_id: String,
    /// The provider's request ID, when the agent harness knows it.
    #[serde(default)]
    pub request_id: Option<String>,
    /// The model of this call, when it differs from the token entry's.
    #[serde(default)]
    pub model: Option<String>,
    /// When the call was made.
    #[serde(with = "time::serde::rfc3339", alias = "occurred_at")]
    #[schema(value_type = String, format = DateTime)]
    pub at: OffsetDateTime,
    #[schema(value_type = super::model::TokenCountsSchema)]
    pub tokens: TokenCounts,
    /// The agent's own cost figure for this call, in US dollars.
    #[serde(default)]
    #[schema(value_type = Option<String>, example = "0.0042")]
    pub reported_cost: Option<Amount>,
}

/// A token entry that passed every check, with its amounts read as money.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedTokenEntry {
    pub entry: NewTokenEntry,
    pub reported_cost: Option<Money>,
    /// Every message with its own reported cost read as money.
    pub messages: Vec<CheckedMessage>,
}

/// One model call that passed the checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedMessage {
    pub message_id: String,
    pub model: String,
    pub occurred_at: OffsetDateTime,
    pub request_id: Option<String>,
    pub tokens: TokenCounts,
    pub reported_cost: Option<Money>,
}

/// Why a token entry is refused, in words for the person who sent it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct EntryError(pub String);

fn label(field: &str, value: &str) -> Result<(), EntryError> {
    if value.trim().is_empty() {
        return Err(EntryError(format!("{field} must not be empty")));
    }
    if value.chars().count() > MAX_LABEL_LEN {
        return Err(EntryError(format!(
            "{field} is longer than {MAX_LABEL_LEN} characters"
        )));
    }
    Ok(())
}

fn optional_label(field: &str, value: Option<&String>) -> Result<(), EntryError> {
    value.map_or(Ok(()), |value| label(field, value))
}

fn cost(field: &str, value: Option<&Amount>) -> Result<Option<Money>, EntryError> {
    let Some(amount) = value else {
        return Ok(None);
    };
    let money = amount
        .money()
        .map_err(|error| EntryError(format!("{field}: {error}")))?;
    if money.nanos().is_negative() {
        return Err(EntryError(format!("{field} must not be negative")));
    }
    Ok(Some(money))
}

/// A tag name: lowercase letters, digits, `_` and `-`, starting with a
/// letter.
#[must_use]
pub fn is_tag_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|first| first.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
        && name.len() <= MAX_LABEL_LEN
}

impl NewTokenEntry {
    /// Checks the token entry against the schema's rules and reads its
    /// amounts as money.
    ///
    /// # Errors
    ///
    /// An [`EntryError`] naming the first field that is wrong.
    pub fn check(self) -> Result<CheckedTokenEntry, EntryError> {
        label("agent_harness", &self.agent_harness)?;
        optional_label("agent_name", self.agent_name.as_ref())?;
        label("model", &self.model)?;
        optional_label("ticket", self.ticket.as_ref())?;
        optional_label("project", self.project.as_ref())?;
        optional_label("session_id", self.session_id.as_ref())?;
        if self.ended_at < self.started_at {
            return Err(EntryError("ended_at is before started_at".to_owned()));
        }
        if let Some(summary) = &self.summary
            && summary.chars().count() > MAX_SUMMARY_LEN
        {
            return Err(EntryError(format!(
                "summary is longer than {MAX_SUMMARY_LEN} characters: send what the session did, not its transcript"
            )));
        }
        check_tags(&self.tags)?;
        for (field, value) in [("ticket", &self.ticket), ("project", &self.project)] {
            if let (Some(field_value), Some(tag_value)) = (value, self.tags.get(field))
                && field_value != tag_value
            {
                return Err(EntryError(format!(
                    "{field} is {field_value:?} but tags.{field} is {tag_value:?}: send one"
                )));
            }
        }
        optional_label("placed_by", self.placed_by.as_ref())?;
        optional_label("effort", self.effort.as_ref())?;
        optional_label("speed", self.speed.as_ref())?;
        if let Some(provider) = &self.provider
            && !super::provider::PROVIDERS.contains(&provider.as_str())
        {
            return Err(EntryError(format!(
                "provider {provider:?} is not one of {}",
                super::provider::PROVIDERS.join(", ")
            )));
        }
        optional_label("local_id", self.local_id.as_ref())?;
        let reported_cost = cost("reported_cost", self.reported_cost.as_ref())?;
        let messages = self.check_messages()?;
        self.check_splits()?;
        Ok(CheckedTokenEntry {
            entry: self,
            reported_cost,
            messages,
        })
    }

    fn check_messages(&self) -> Result<Vec<CheckedMessage>, EntryError> {
        if self.messages.is_empty() {
            return Ok(Vec::new());
        }
        if self.messages.len() > MAX_MESSAGES {
            return Err(EntryError(format!(
                "a token entry carries at most {MAX_MESSAGES} messages"
            )));
        }
        if self.session_id.is_none() {
            return Err(EntryError(
                "messages need the session_id they belong to".to_owned(),
            ));
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut sum = TokenCounts::default();
        let mut checked = Vec::with_capacity(self.messages.len());
        for message in &self.messages {
            label("message_id", &message.message_id)?;
            optional_label("message model", message.model.as_ref())?;
            if !seen.insert(message.message_id.as_str()) {
                return Err(EntryError(format!(
                    "message {:?} appears twice",
                    message.message_id
                )));
            }
            sum = sum.checked_add(message.tokens).ok_or_else(|| {
                EntryError("the messages' tokens are too many to add up".to_owned())
            })?;
            checked.push(CheckedMessage {
                message_id: message.message_id.clone(),
                model: message.model.clone().unwrap_or_else(|| self.model.clone()),
                occurred_at: message.at,
                request_id: message.request_id.clone(),
                tokens: message.tokens,
                reported_cost: cost("message reported_cost", message.reported_cost.as_ref())?,
            });
        }
        if sum != self.tokens {
            return Err(EntryError(
                "tokens must equal the sum of the messages' tokens".to_owned(),
            ));
        }
        Ok(checked)
    }

    fn check_splits(&self) -> Result<(), EntryError> {
        if self.splits.is_empty() {
            return Ok(());
        }
        for split in &self.splits {
            check_tags(&split.tags)?;
        }
        let by_message = self
            .splits
            .iter()
            .all(|split| !split.message_ids.is_empty() && split.share_percent.is_none());
        let by_share = self
            .splits
            .iter()
            .all(|split| split.message_ids.is_empty() && split.share_percent.is_some());
        if by_message {
            let mut covered = std::collections::BTreeSet::new();
            for id in self.splits.iter().flat_map(|split| &split.message_ids) {
                if !covered.insert(id.as_str()) {
                    return Err(EntryError(format!("message {id:?} is in two splits")));
                }
            }
            let messages: std::collections::BTreeSet<&str> = self
                .messages
                .iter()
                .map(|message| message.message_id.as_str())
                .collect();
            if covered != messages {
                return Err(EntryError(
                    "splits by message must hold every message of the token entry exactly once"
                        .to_owned(),
                ));
            }
            return Ok(());
        }
        if by_share {
            let total = self
                .splits
                .iter()
                .filter_map(|split| split.share_percent)
                .try_fold(0_u32, u32::checked_add);
            if total != Some(100) {
                return Err(EntryError(
                    "split shares must add up to 100 percent".to_owned(),
                ));
            }
            if self
                .splits
                .iter()
                .any(|split| split.share_percent == Some(0))
            {
                return Err(EntryError(
                    "a split share must be above 0 percent".to_owned(),
                ));
            }
            return Ok(());
        }
        Err(EntryError(
            "splits are either all by message_ids or all by share_percent".to_owned(),
        ))
    }
}

fn check_tags(tags: &BTreeMap<String, String>) -> Result<(), EntryError> {
    for (name, value) in tags {
        if !is_tag_name(name) {
            return Err(EntryError(format!(
                "tag name {name:?} must be lowercase letters, digits, _ or -, starting with a letter"
            )));
        }
        if name == PERSON_TAG {
            return Err(EntryError(
                "tag \"person\" is set from the token that sends the token entry".to_owned(),
            ));
        }
        label(&format!("tag {name}"), value)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn docs_example() -> serde_json::Value {
        // The call on the docs page Collect > Token entries endpoint.
        serde_json::json!({
            "agent_harness": "support-desk",
            "agent_name": "email answers",
            "model": "claude-sonnet-5",
            "started_at": "2026-09-18T09:12:00Z",
            "ended_at": "2026-09-18T09:14:10Z",
            "tokens": { "input": 4120, "output": 610, "cache_read": 18200 },
            "reported_cost": 0.031,
            "summary": "Answered refund question for order 1182",
            "project": "support",
            "tags": { "client": "acme", "purpose": "support" }
        })
    }

    fn checked(value: serde_json::Value) -> Result<CheckedTokenEntry, EntryError> {
        serde_json::from_value::<NewTokenEntry>(value)
            .map_err(|error| EntryError(error.to_string()))?
            .check()
    }

    #[test]
    fn the_docs_example_is_accepted() {
        let entry = checked(docs_example()).unwrap();
        assert_eq!(entry.reported_cost.unwrap().nanos(), 31_000_000);
        assert_eq!(entry.entry.tokens.cache_read, 18_200);
        assert_eq!(entry.entry.tokens.cache_write, 0);
        assert_eq!(entry.entry.tags["client"], "acme");
        assert!(entry.messages.is_empty());
    }

    #[test]
    fn unknown_fields_and_missing_required_fields_are_refused() {
        let mut value = docs_example();
        value["cost"] = serde_json::json!(1);
        assert!(checked(value).is_err());
        let mut value = docs_example();
        value.as_object_mut().unwrap().remove("model");
        assert!(checked(value).is_err());
    }

    #[test]
    fn each_rule_names_its_field() {
        let cases = [
            (
                "agent_harness",
                serde_json::json!(" "),
                "agent_harness must not be empty",
            ),
            (
                "ended_at",
                serde_json::json!("2026-09-18T09:00:00Z"),
                "ended_at is before started_at",
            ),
            (
                "reported_cost",
                serde_json::json!(-0.5),
                "reported_cost must not be negative",
            ),
            (
                "tags",
                serde_json::json!({"Team": "web"}),
                "tag name \"Team\"",
            ),
            (
                "tags",
                serde_json::json!({"person": "ana"}),
                "set from the token",
            ),
            ("tags", serde_json::json!({"project": "web"}), "send one"),
            (
                "tags",
                serde_json::json!({"team": ""}),
                "tag team must not be empty",
            ),
            (
                "summary",
                serde_json::json!("x".repeat(10_001)),
                "not its transcript",
            ),
            (
                "model",
                serde_json::json!("m".repeat(201)),
                "longer than 200",
            ),
        ];
        for (field, value, expected) in cases {
            let mut body = docs_example();
            body[field] = value;
            let error = checked(body).unwrap_err().0;
            assert!(error.contains(expected), "{field}: {error}");
        }
    }

    #[test]
    fn messages_must_add_up_to_the_entry() {
        let mut body = docs_example();
        body["session_id"] = serde_json::json!("s-1");
        body["tokens"] = serde_json::json!({"input": 10, "output": 5});
        body["messages"] = serde_json::json!([
            {"message_id": "m1", "at": "2026-09-18T09:12:30Z", "request_id": "req_1", "tokens": {"input": 4, "output": 2}, "reported_cost": 0.01},
            {"message_id": "m2", "model": "claude-haiku-5", "occurred_at": "2026-09-18T09:13:30Z", "tokens": {"input": 6, "output": 3}}
        ]);
        let entry = checked(body.clone()).unwrap();
        assert_eq!(entry.messages.len(), 2);
        assert_eq!(entry.messages[0].model, "claude-sonnet-5");
        assert_eq!(entry.messages[1].model, "claude-haiku-5");
        assert_eq!(entry.messages[0].reported_cost.unwrap().nanos(), 10_000_000);
        assert_eq!(entry.messages[0].request_id.as_deref(), Some("req_1"));

        let mut wrong_sum = body.clone();
        wrong_sum["tokens"]["input"] = serde_json::json!(11);
        assert!(
            checked(wrong_sum)
                .unwrap_err()
                .0
                .contains("sum of the messages")
        );

        let mut twice = body.clone();
        twice["messages"][1]["message_id"] = serde_json::json!("m1");
        assert!(checked(twice).unwrap_err().0.contains("appears twice"));

        let mut no_session = body;
        no_session.as_object_mut().unwrap().remove("session_id");
        assert!(checked(no_session).unwrap_err().0.contains("session_id"));
    }

    #[test]
    fn costs_may_be_numbers_or_decimal_strings() {
        let mut body = docs_example();
        body["reported_cost"] = serde_json::json!("0.031");
        assert_eq!(
            checked(body).unwrap().reported_cost.unwrap().nanos(),
            31_000_000
        );
        let mut body = docs_example();
        body["reported_cost"] = serde_json::json!("lots");
        assert!(checked(body).unwrap_err().0.contains("reported_cost"));
    }

    fn with_two_messages() -> serde_json::Value {
        let mut body = docs_example();
        body["session_id"] = serde_json::json!("s-1");
        body["tokens"] = serde_json::json!({"input": 3});
        body["messages"] = serde_json::json!([
            {"message_id": "m1", "at": "2026-09-18T09:12:30Z", "tokens": {"input": 1}},
            {"message_id": "m2", "at": "2026-09-18T09:13:30Z", "tokens": {"input": 2}}
        ]);
        body
    }

    #[test]
    fn splits_by_message_hold_every_message_once() {
        let mut body = with_two_messages();
        body["splits"] = serde_json::json!([
            {"message_ids": ["m1"], "tags": {"ticket": "KD-1"}},
            {"message_ids": ["m2"], "tags": {"ticket": "KD-2"}, "signals": {"branch": "KD-2-fix"}}
        ]);
        assert!(checked(body.clone()).is_ok());

        let mut missing = body.clone();
        missing["splits"][1]["message_ids"] = serde_json::json!(["m1"]);
        assert!(checked(missing).unwrap_err().0.contains("in two splits"));

        let mut partial = body;
        partial["splits"] = serde_json::json!([{"message_ids": ["m1"]}]);
        assert!(checked(partial).unwrap_err().0.contains("exactly once"));
    }

    #[test]
    fn splits_by_share_add_up_to_a_hundred() {
        let mut body = docs_example();
        body["splits"] = serde_json::json!([
            {"share_percent": 60, "tags": {"client": "acme"}},
            {"share_percent": 40, "tags": {"client": "globex"}}
        ]);
        assert!(checked(body.clone()).is_ok());

        let mut short = body.clone();
        short["splits"][1]["share_percent"] = serde_json::json!(39);
        assert!(checked(short).unwrap_err().0.contains("100 percent"));

        let mut zero = body.clone();
        zero["splits"] = serde_json::json!([{"share_percent": 100}, {"share_percent": 0}]);
        assert!(checked(zero).unwrap_err().0.contains("above 0"));

        let mut mixed = with_two_messages();
        mixed["splits"] = serde_json::json!([{"share_percent": 50}, {"message_ids": ["m1", "m2"]}]);
        assert!(checked(mixed).unwrap_err().0.contains("either all"));
    }

    #[test]
    fn tag_names_follow_one_pattern() {
        assert!(is_tag_name("team"));
        assert!(is_tag_name("cost-center_2"));
        assert!(!is_tag_name(""));
        assert!(!is_tag_name("2team"));
        assert!(!is_tag_name("Team"));
        assert!(!is_tag_name("team name"));
    }
}
