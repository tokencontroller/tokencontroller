//! A token entry as the token entries endpoint and CSV import take it
//! (docs: Reference > Token entry schema), and the checks it must pass
//! before it becomes message events.

use std::collections::{BTreeMap, BTreeSet};

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tokencontroller_core::{Currency, Money, TokenCounts};
use utoipa::ToSchema;

use crate::events::json::{Amount, TagsSchema};
use crate::integrations::decimal::{money_from_decimal, money_from_json_number};

/// The longest agent harness, agent name, model, key, session or message
/// id, tag name or tag value accepted.
pub const MAX_LABEL_LEN: usize = 200;

/// The longest summary accepted: a short report, never a transcript.
pub const MAX_SUMMARY_LEN: usize = 10_000;

/// The most messages one token entry may carry.
pub const MAX_MESSAGES: usize = 20_000;

/// The tag name Token Controller sets itself, from the token.
pub const PERSON_TAG: &str = "person";

/// The scale a split fraction is held at: `0.6` is 600 000 of 1 000 000.
pub const FRACTION_SCALE: u32 = 6;

/// Token counts by kind as they arrive. All four kinds are required; an
/// entry missing one has no counts and is a count error, never zero.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TokensInput {
    #[serde(default)]
    pub input: Option<u64>,
    #[serde(default)]
    pub output: Option<u64>,
    #[serde(default)]
    pub cache_read: Option<u64>,
    #[serde(default)]
    pub cache_write: Option<u64>,
}

impl TokensInput {
    /// The counts, when every kind was sent.
    #[must_use]
    pub fn complete(self) -> Option<TokenCounts> {
        Some(TokenCounts {
            input: self.input?,
            output: self.output?,
            cache_read: self.cache_read?,
            cache_write: self.cache_write?,
        })
    }

    /// Counts that were all sent.
    #[must_use]
    pub const fn of(counts: TokenCounts) -> Self {
        Self {
            input: Some(counts.input),
            output: Some(counts.output),
            cache_read: Some(counts.cache_read),
            cache_write: Some(counts.cache_write),
        }
    }
}

/// Facts about the session. All optional.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Facts {
    #[serde(default)]
    pub turns: Option<u64>,
    #[serde(default)]
    pub tool_calls: Option<u64>,
    #[serde(default)]
    pub retries: Option<u64>,
    #[serde(default)]
    pub duration_seconds: Option<u64>,
    #[serde(default)]
    pub subagents: Option<u64>,
    #[serde(default)]
    pub files_touched: Option<u64>,
    /// Where the result is, such as the pull request's URL.
    #[serde(default)]
    pub result: Option<String>,
}

/// Facts about a session that rules read. Signals are not tags.
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
    /// The pull request number.
    #[serde(default)]
    pub pull_request: Option<u64>,
    #[serde(default)]
    pub pull_request_title: Option<String>,
    #[serde(default)]
    pub subagent_type: Option<String>,
    /// Keys found in prompts, the session title and the pull request
    /// title, such as `KD-214`. Only the keys, never the text.
    #[serde(default)]
    pub keys: Vec<String>,
    /// Values from the repository's `.tokencontroller.toml`, by name.
    #[serde(default)]
    pub config: BTreeMap<String, String>,
}

impl Signals {
    /// The signals as the core carries them.
    #[must_use]
    pub fn core(&self) -> tokencontroller_core::Signals {
        tokencontroller_core::Signals {
            branch: self.branch.clone(),
            working_directory: self.working_directory.clone(),
            repository: self.repository.clone(),
            pull_request: self.pull_request,
            pull_request_title: self.pull_request_title.clone(),
            subagent_type: self.subagent_type.clone(),
            prompt_keys: self.keys.clone(),
            config: self.config.clone(),
            resource: BTreeMap::new(),
        }
    }
}

/// One token entry as it arrives: one message, or one session with its
/// messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewTokenEntry {
    /// The product the agent ran on, such as `claude-code` or `support-desk`.
    pub agent_harness: String,
    /// A label for the agent, such as "review bot".
    #[serde(default)]
    pub agent_name: Option<String>,
    /// Who charges for the model calls: `anthropic`, `openai`, `google`,
    /// `aws_bedrock`, `gcp_vertex`, `azure_foundry`, `github`, `cursor` or
    /// `other`. Without it, it follows from the model's name.
    #[serde(default)]
    pub provider: Option<String>,
    /// The model that did the work.
    pub model: String,
    /// When the session started.
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub started_at: OffsetDateTime,
    /// When the session ended.
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub ended_at: OffsetDateTime,
    /// Counts by kind. All four kinds; a missing kind is a count error.
    pub tokens: TokensInput,
    /// The agent's own cost figure in US dollars, a fact about the message.
    /// The cost shown is counts × the price table.
    #[serde(default)]
    #[schema(value_type = Option<String>, example = "0.031")]
    pub reported_cost: Option<Amount>,
    /// What the session did, from one line to a short report.
    #[serde(default)]
    pub summary: Option<String>,
    /// Session facts.
    #[serde(default)]
    pub facts: Facts,
    /// A feature key, such as `KD-214`. Wins over every rule.
    #[serde(default)]
    pub feature: Option<String>,
    /// A project, by key.
    #[serde(default)]
    pub project: Option<String>,
    /// Any other tags, one value per tag name. They win over rules.
    #[serde(default)]
    #[schema(value_type = TagsSchema)]
    pub tags: BTreeMap<String, String>,
    /// Facts about the session that rules read.
    #[serde(default)]
    pub signals: Signals,
    /// The agent harness's id for the session. Required for a session
    /// record.
    #[serde(default)]
    pub session_id: Option<String>,
    /// The session's model calls. With them, a session that telemetry also
    /// reported is counted exactly once, call by call.
    #[serde(default)]
    pub messages: Vec<NewMessage>,
    /// How the session is split. Without it the session is one split, or
    /// rules split it.
    #[serde(default)]
    pub splits: Vec<NewSplit>,
    /// The sender's own id for the entry, such as `claude-code:<session
    /// id>`. Sending it again adds nothing.
    #[serde(default)]
    pub local_id: Option<String>,
}

/// One part of a split: a fraction, or exactly these messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = EntrySplit)]
pub struct NewSplit {
    /// The part's fraction, as a number or a decimal string such as `"0.6"`.
    /// The fractions add up to 1.
    #[serde(default)]
    #[schema(value_type = Option<String>, example = "0.6")]
    pub fraction: Option<Amount>,
    /// Instead of a fraction: the messages this part covers, exactly.
    #[serde(default)]
    pub message_ids: Vec<String>,
    #[serde(default)]
    #[schema(value_type = TagsSchema)]
    pub tags: BTreeMap<String, String>,
}

/// One model call of a session record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewMessage {
    /// The agent harness's id for this call, unique within the session.
    pub message_id: String,
    /// When the call was made.
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub at: OffsetDateTime,
    /// Counts by kind.
    pub tokens: TokensInput,
    /// The model of this call, when it differs from the session's.
    #[serde(default)]
    pub model: Option<String>,
    /// The agent's own cost figure for this call, in US dollars.
    #[serde(default)]
    #[schema(value_type = Option<String>, example = "0.0042")]
    pub reported_cost: Option<Amount>,
}

/// A token entry that passed every check, read into what the store needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedTokenEntry {
    pub entry: NewTokenEntry,
    pub provider: &'static str,
    /// The tags the entry names: `feature`, `project` and `tags` together.
    pub tags: BTreeMap<String, String>,
    pub reported_cost: Option<Money>,
    /// The messages: the entry's own, or the one message a record without
    /// messages stands for.
    pub messages: Vec<CheckedMessage>,
    /// The splits, read: by fraction as weights at [`FRACTION_SCALE`], or
    /// by message ids.
    pub splits: Vec<CheckedSplit>,
}

/// One model call that passed the checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedMessage {
    pub message_id: String,
    pub model: String,
    pub at: OffsetDateTime,
    /// `None` when a kind was missing: a count error.
    pub tokens: Option<TokenCounts>,
    pub reported_cost: Option<Money>,
}

/// One part of a split, read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedSplit {
    pub weight: u64,
    pub message_ids: Vec<String>,
    pub tags: BTreeMap<String, String>,
}

/// Why a token entry is refused, in words for the person who sent it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct EntryError(pub String);

fn label(field: &str, value: &str) -> Result<(), EntryError> {
    if value.trim().is_empty() {
        return Err(EntryError(format!("{field} must not be empty")));
    }
    if value.trim() != value {
        return Err(EntryError(format!(
            "{field} must not start or end with a space"
        )));
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
    let money = match amount {
        Amount::Number(number) => money_from_json_number(number, Currency::USD),
        Amount::Text(text) => money_from_decimal(text, Currency::USD),
    }
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
        && name.len() <= 40
}

/// The id of the one message a session record without messages stands
/// for.
pub const WHOLE_SESSION_MESSAGE: &str = "session";

impl NewTokenEntry {
    /// Checks the token entry against the schema's rules and reads its
    /// amounts.
    ///
    /// # Errors
    ///
    /// An [`EntryError`] naming the first field that is wrong.
    pub fn check(self) -> Result<CheckedTokenEntry, EntryError> {
        label("agent_harness", &self.agent_harness)?;
        optional_label("agent_name", self.agent_name.as_ref())?;
        label("model", &self.model)?;
        optional_label("feature", self.feature.as_ref())?;
        optional_label("project", self.project.as_ref())?;
        optional_label("session_id", self.session_id.as_ref())?;
        optional_label("local_id", self.local_id.as_ref())?;
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
        let mut tags = self.tags.clone();
        for (field, value) in [("feature", &self.feature), ("project", &self.project)] {
            if let Some(value) = value {
                if let Some(tag_value) = self.tags.get(field)
                    && tag_value != value
                {
                    return Err(EntryError(format!(
                        "{field} is {value:?} but tags.{field} is {tag_value:?}: send one"
                    )));
                }
                tags.insert(field.to_owned(), value.clone());
            }
        }
        for key in &self.signals.keys {
            label("signals.keys", key)?;
        }
        if let Some(provider) = &self.provider
            && !super::provider::PROVIDERS.contains(&provider.as_str())
        {
            return Err(EntryError(format!(
                "provider {provider:?} is not one of {}",
                super::provider::PROVIDERS.join(", ")
            )));
        }
        let provider =
            super::provider::infer(self.provider.as_deref(), &self.agent_harness, &self.model);
        let reported_cost = cost("reported_cost", self.reported_cost.as_ref())?;
        let messages = self.check_messages(reported_cost)?;
        let splits = self.check_splits(&messages)?;
        Ok(CheckedTokenEntry {
            provider,
            tags,
            reported_cost,
            messages,
            splits,
            entry: self,
        })
    }

    fn check_messages(
        &self,
        reported_cost: Option<Money>,
    ) -> Result<Vec<CheckedMessage>, EntryError> {
        if self.messages.is_empty() {
            return Ok(vec![CheckedMessage {
                message_id: WHOLE_SESSION_MESSAGE.to_owned(),
                model: self.model.clone(),
                at: self.ended_at,
                tokens: self.tokens.complete(),
                reported_cost,
            }]);
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
        let mut seen = BTreeSet::new();
        let mut sum = Some(TokenCounts::default());
        let mut checked = Vec::new();
        for message in &self.messages {
            label("message_id", &message.message_id)?;
            optional_label("message model", message.model.as_ref())?;
            if !seen.insert(message.message_id.as_str()) {
                return Err(EntryError(format!(
                    "message {:?} appears twice",
                    message.message_id
                )));
            }
            let tokens = message.tokens.complete();
            sum = match (sum, tokens) {
                (Some(sum), Some(tokens)) => Some(sum.checked_add(tokens).ok_or_else(|| {
                    EntryError("the messages' tokens are too many to add up".to_owned())
                })?),
                _ => None,
            };
            checked.push(CheckedMessage {
                message_id: message.message_id.clone(),
                model: message.model.clone().unwrap_or_else(|| self.model.clone()),
                at: message.at,
                tokens,
                reported_cost: cost("message reported_cost", message.reported_cost.as_ref())?,
            });
        }
        if let (Some(sum), Some(tokens)) = (sum, self.tokens.complete())
            && sum != tokens
        {
            return Err(EntryError(
                "tokens must equal the sum of the messages' tokens".to_owned(),
            ));
        }
        Ok(checked)
    }

    fn check_splits(&self, messages: &[CheckedMessage]) -> Result<Vec<CheckedSplit>, EntryError> {
        if self.splits.is_empty() {
            return Ok(Vec::new());
        }
        for split in &self.splits {
            check_tags(&split.tags)?;
        }
        let by_message = self
            .splits
            .iter()
            .all(|split| !split.message_ids.is_empty() && split.fraction.is_none());
        let by_fraction = self
            .splits
            .iter()
            .all(|split| split.message_ids.is_empty() && split.fraction.is_some());
        if by_message {
            if self.messages.is_empty() {
                return Err(EntryError(
                    "splits by message_ids need the entry's messages".to_owned(),
                ));
            }
            let mut covered = BTreeSet::new();
            for id in self.splits.iter().flat_map(|split| &split.message_ids) {
                if !covered.insert(id.as_str()) {
                    return Err(EntryError(format!("message {id:?} is in two splits")));
                }
            }
            let known: BTreeSet<&str> = messages
                .iter()
                .map(|message| message.message_id.as_str())
                .collect();
            if covered != known {
                return Err(EntryError(
                    "splits by message_ids must hold every message of the entry exactly once"
                        .to_owned(),
                ));
            }
            return Ok(self
                .splits
                .iter()
                .map(|split| CheckedSplit {
                    weight: 1,
                    message_ids: split.message_ids.clone(),
                    tags: split.tags.clone(),
                })
                .collect());
        }
        if by_fraction {
            let mut weights = Vec::new();
            let mut total = Decimal::ZERO;
            for split in &self.splits {
                let fraction = fraction(split.fraction.as_ref())?;
                total = total
                    .checked_add(fraction)
                    .ok_or_else(|| EntryError("the fractions do not add up".to_owned()))?;
                weights.push(weight(fraction)?);
            }
            if total != Decimal::ONE {
                return Err(EntryError(format!(
                    "the fractions add up to {total}, not 1"
                )));
            }
            return Ok(self
                .splits
                .iter()
                .zip(weights)
                .map(|(split, weight)| CheckedSplit {
                    weight,
                    message_ids: Vec::new(),
                    tags: split.tags.clone(),
                })
                .collect());
        }
        Err(EntryError(
            "splits are either all by fraction or all by message_ids".to_owned(),
        ))
    }
}

fn fraction(amount: Option<&Amount>) -> Result<Decimal, EntryError> {
    let amount = amount.ok_or_else(|| EntryError("a split needs a fraction".to_owned()))?;
    let fraction = amount
        .decimal()
        .map_err(|error| EntryError(format!("fraction: {error}")))?;
    if fraction <= Decimal::ZERO || fraction > Decimal::ONE {
        return Err(EntryError(format!(
            "a fraction is above 0 and at most 1, not {fraction}"
        )));
    }
    if fraction.scale() > FRACTION_SCALE && fraction.normalize().scale() > FRACTION_SCALE {
        return Err(EntryError(format!(
            "a fraction has at most {FRACTION_SCALE} decimals, not {fraction}"
        )));
    }
    Ok(fraction)
}

fn weight(fraction: Decimal) -> Result<u64, EntryError> {
    let scaled = fraction
        .checked_mul(Decimal::from(10_u64.pow(FRACTION_SCALE)))
        .ok_or_else(|| EntryError("the fraction does not fit".to_owned()))?
        .normalize();
    u64::try_from(scaled).map_err(|_| EntryError(format!("{fraction} is not a fraction")))
}

/// Checks tags named by hand or sent: names on the wire's rules, never
/// `person`, values not empty and not too long.
///
/// # Errors
///
/// [`EntryError`] for the first tag that breaks a rule.
pub fn check_tags(tags: &BTreeMap<String, String>) -> Result<(), EntryError> {
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
            "tokens": { "input": 4120, "output": 610, "cache_read": 18200, "cache_write": 0 },
            "reported_cost": 0.031,
            "summary": "Answered refund question for order 1182",
            "project": "support",
            "tags": { "client": "acme", "purpose": "support" },
            "facts": { "turns": 3, "result": "https://example.com/pr/1" }
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
        assert_eq!(entry.provider, "anthropic");
        assert_eq!(entry.tags["client"], "acme");
        assert_eq!(entry.tags["project"], "support");
        assert_eq!(entry.messages.len(), 1);
        assert_eq!(entry.messages[0].message_id, WHOLE_SESSION_MESSAGE);
        assert_eq!(entry.messages[0].tokens.unwrap().cache_read, 18_200);
        assert_eq!(entry.entry.facts.turns, Some(3));
    }

    #[test]
    fn a_missing_token_kind_is_a_count_error() {
        let mut value = docs_example();
        value["tokens"] = serde_json::json!({"input": 1, "output": 2, "cache_read": 3});
        let entry = checked(value).unwrap();
        assert!(entry.messages[0].tokens.is_none());
    }

    #[test]
    fn unknown_fields_and_missing_required_fields_are_refused() {
        for removed in ["placed_by", "request_id", "ticket", "share_percent"] {
            let mut value = docs_example();
            value[removed] = serde_json::json!("x");
            assert!(checked(value).is_err(), "{removed}");
        }
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
            (
                "provider",
                serde_json::json!("acme-cloud"),
                "provider \"acme-cloud\" is not one of",
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
        body["tokens"] =
            serde_json::json!({"input": 10, "output": 5, "cache_read": 0, "cache_write": 0});
        body["messages"] = serde_json::json!([
            {"message_id": "m1", "at": "2026-09-18T09:12:30Z", "tokens": {"input": 4, "output": 2, "cache_read": 0, "cache_write": 0}, "reported_cost": 0.01},
            {"message_id": "m2", "model": "claude-haiku-5", "at": "2026-09-18T09:13:30Z", "tokens": {"input": 6, "output": 3, "cache_read": 0, "cache_write": 0}}
        ]);
        let entry = checked(body.clone()).unwrap();
        assert_eq!(entry.messages.len(), 2);
        assert_eq!(entry.messages[0].model, "claude-sonnet-5");
        assert_eq!(entry.messages[1].model, "claude-haiku-5");
        assert_eq!(entry.messages[0].reported_cost.unwrap().nanos(), 10_000_000);

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
        body["tokens"] =
            serde_json::json!({"input": 3, "output": 0, "cache_read": 0, "cache_write": 0});
        body["messages"] = serde_json::json!([
            {"message_id": "m1", "at": "2026-09-18T09:12:30Z", "tokens": {"input": 1, "output": 0, "cache_read": 0, "cache_write": 0}},
            {"message_id": "m2", "at": "2026-09-18T09:13:30Z", "tokens": {"input": 2, "output": 0, "cache_read": 0, "cache_write": 0}}
        ]);
        body
    }

    #[test]
    fn splits_by_message_hold_every_message_once() {
        let mut body = with_two_messages();
        body["splits"] = serde_json::json!([
            {"message_ids": ["m1"], "tags": {"feature": "KD-1"}},
            {"message_ids": ["m2"], "tags": {"feature": "KD-2"}}
        ]);
        let entry = checked(body.clone()).unwrap();
        assert_eq!(entry.splits.len(), 2);
        assert_eq!(entry.splits[1].message_ids, vec!["m2"]);

        let mut missing = body.clone();
        missing["splits"][1]["message_ids"] = serde_json::json!(["m1"]);
        assert!(checked(missing).unwrap_err().0.contains("in two splits"));

        let mut partial = body;
        partial["splits"] = serde_json::json!([{"message_ids": ["m1"]}]);
        assert!(checked(partial).unwrap_err().0.contains("exactly once"));
    }

    #[test]
    fn fractions_add_up_to_one() {
        let mut body = docs_example();
        body["splits"] = serde_json::json!([
            {"fraction": 0.6, "tags": {"feature": "KD-1"}},
            {"fraction": "0.4", "tags": {"feature": "KD-2"}}
        ]);
        let entry = checked(body.clone()).unwrap();
        assert_eq!(entry.splits[0].weight, 600_000);
        assert_eq!(entry.splits[1].weight, 400_000);

        let mut short = body.clone();
        short["splits"][1]["fraction"] = serde_json::json!("0.39");
        assert!(checked(short).unwrap_err().0.contains("not 1"));

        let mut zero = body.clone();
        zero["splits"] = serde_json::json!([{"fraction": 1}, {"fraction": 0}]);
        assert!(checked(zero).unwrap_err().0.contains("above 0"));

        let mut fine = body;
        fine["splits"] = serde_json::json!([{"fraction": "0.3333333"}, {"fraction": "0.6666667"}]);
        assert!(checked(fine).unwrap_err().0.contains("decimals"));

        let mut mixed = with_two_messages();
        mixed["splits"] = serde_json::json!([{"fraction": 0.5}, {"message_ids": ["m1", "m2"]}]);
        assert!(checked(mixed).unwrap_err().0.contains("either all"));
    }

    #[test]
    fn signals_become_the_cores_signals() {
        let signals: Signals = serde_json::from_value(serde_json::json!({
            "branch": "feature/KD-1",
            "repository": "github.com/kestrel/shop",
            "pull_request": 12,
            "pull_request_title": "KD-1 refund flow",
            "subagent_type": "review",
            "keys": ["KD-1"],
            "config": {"project": "shop"}
        }))
        .unwrap();
        let core = signals.core();
        assert_eq!(core.prompt_keys, vec!["KD-1"]);
        assert_eq!(core.pull_request, Some(12));
        assert_eq!(core.config.get("project").map(String::as_str), Some("shop"));
        assert!(core.resource.is_empty());
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
