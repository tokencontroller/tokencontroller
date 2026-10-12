//! The OpenAPI shape of the core crate's rules (docs: Attribute > Rules).
//!
//! The core crate does not derive OpenAPI schemas, so these types describe
//! the JSON its `Rule` reads and writes, for the document and the clients
//! generated from it, such as the web app's. A test keeps them in step with
//! the core crate.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::tags::Tags;

/// A rule: when every condition holds, do the action. Such as
/// `{"name": "review branches", "when": [{"signal": "branch", "test":
/// {"starts_with": "review/"}}], "then": {"set_tags": {"purpose":
/// "code-review"}}}`.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = Rule)]
pub struct RuleShape {
    /// The name the review list shows, such as `ticket key in branch`.
    pub name: String,
    /// Conditions that must all hold. No condition means always.
    #[serde(default)]
    pub when: Vec<ConditionShape>,
    pub then: ActionShape,
}

/// What a rule does.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = RuleAction)]
pub enum ActionShape {
    /// Finds a ticket key with the projects' ticket key patterns and places
    /// the token entry on that ticket.
    TicketKeyIn(KeySourceShape),
    /// Places the token entry on the project its repository is mapped to.
    RepositoryProject,
    /// Sets tags, such as `purpose: code-review`. A `ticket` or `project`
    /// here places the token entry like the rules above.
    SetTags(Tags),
}

/// Where a rule looks for ticket keys.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = RuleKeySource)]
pub enum KeySourceShape {
    Branch,
    PullRequestTitle,
    PromptsOrSessionTitle,
}

/// A test on one signal.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = RuleCondition)]
pub struct ConditionShape {
    pub signal: SignalShape,
    pub test: TestShape,
}

/// The signals a rule can test.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = RuleSignal)]
pub enum SignalShape {
    Branch,
    WorkingDirectory,
    Repository,
    /// The pull request number.
    PullRequest,
    SubagentType,
    /// The ticket the token entry was placed on. Only rules that set other
    /// tags can see it.
    Ticket,
}

/// How a signal is tested. Every test but `is_set` fails when the signal is
/// missing.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = RuleTest)]
pub enum TestShape {
    Is(String),
    StartsWith(String),
    Contains(String),
    /// A regular expression.
    Matches(String),
    IsSet,
}

/// What the "ticket key in prompts" rule does when the prompts name several
/// tickets.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = SeveralKeys)]
pub enum SeveralKeysShape {
    /// The ticket named most often wins; on a tie, the one named last.
    MostOften,
    /// The token entry is split equally between the tickets.
    EqualSplit,
    /// The ticket named first wins.
    FirstNamed,
}

#[cfg(test)]
mod tests {
    use tokencontroller_core::rules::{Rule, SeveralKeys};

    use super::*;

    #[test]
    fn the_shape_reads_and_writes_what_the_core_writes() {
        let mut rules = Rule::defaults();
        let more: Vec<Rule> = serde_json::from_value(serde_json::json!([
            {"name": "review", "when": [{"signal": "branch", "test": {"starts_with": "review/"}}],
             "then": {"set_tags": {"purpose": "code-review"}}},
            {"name": "all tests", "when": [
                {"signal": "working_directory", "test": {"contains": "infra"}},
                {"signal": "repository", "test": {"is": "kestrel/web"}},
                {"signal": "pull_request", "test": "is_set"},
                {"signal": "subagent_type", "test": {"matches": "^review"}},
                {"signal": "ticket", "test": {"is": "KD-1"}}
            ], "then": {"set_tags": {}}}
        ]))
        .unwrap();
        rules.extend(more);
        for rule in rules {
            let written = serde_json::to_value(&rule).unwrap();
            let shape: RuleShape = serde_json::from_value(written.clone()).unwrap();
            assert_eq!(serde_json::to_value(&shape).unwrap(), written);
            let back: Rule = serde_json::from_value(serde_json::to_value(&shape).unwrap()).unwrap();
            assert_eq!(back, rule);
        }
    }

    #[test]
    fn several_keys_read_and_write_what_the_core_writes() {
        for several_keys in [
            SeveralKeys::MostOften,
            SeveralKeys::EqualSplit,
            SeveralKeys::FirstNamed,
        ] {
            let written = serde_json::to_value(several_keys).unwrap();
            let shape: SeveralKeysShape = serde_json::from_value(written.clone()).unwrap();
            assert_eq!(serde_json::to_value(&shape).unwrap(), written);
        }
    }
}
