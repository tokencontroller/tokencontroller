//! Rules: instructions that turn signals into tags.
//!
//! The same rules run on the machine (in the CLI, which the Claude Code mod
//! and the GitHub Action call) and in the service, so both passes place a
//! token entry the same way (ADR 0010). [`crate::placement`] runs them.

use serde::{Deserialize, Serialize};

use crate::message::Signals;
use crate::pattern::TextPattern;
use crate::tags::{TagName, TagValue, Tags};

/// One rule: when every condition holds, do the action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    /// The name the review list shows, such as `ticket key in branch`.
    pub name: String,
    /// Conditions that must all hold. No condition means always.
    #[serde(default)]
    pub when: Vec<Condition>,
    pub then: Action,
}

impl Rule {
    /// The rules every organization starts with: a ticket key in the
    /// branch, then in the pull request title, then in the prompts or the
    /// session title, and the repository's project.
    #[must_use]
    pub fn defaults() -> Vec<Self> {
        let rule = |name: &str, then| Self {
            name: name.to_owned(),
            when: Vec::new(),
            then,
        };
        vec![
            rule(
                "ticket key in branch",
                Action::TicketKeyIn(KeySource::Branch),
            ),
            rule(
                "ticket key in pull request title",
                Action::TicketKeyIn(KeySource::PullRequestTitle),
            ),
            rule(
                "ticket key in prompts or session title",
                Action::TicketKeyIn(KeySource::PromptsOrSessionTitle),
            ),
            rule("repository", Action::RepositoryProject),
        ]
    }

    /// Whether every condition holds for these signals and, for conditions
    /// on the ticket, the ticket placed so far.
    #[must_use]
    pub fn applies(&self, signals: &Signals, ticket: Option<&TagValue>) -> bool {
        self.when
            .iter()
            .all(|condition| condition.holds(signals, ticket))
    }

    /// The ticket this rule sets directly, if its action sets tags with a
    /// ticket.
    #[must_use]
    pub fn sets_ticket(&self) -> Option<&TagValue> {
        match &self.then {
            Action::SetTags(tags) => tags.get(&TagName::ticket()),
            Action::TicketKeyIn(_) | Action::RepositoryProject => None,
        }
    }
}

/// What a rule does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Finds a ticket key with the projects' ticket key patterns and places
    /// the token entry on that ticket.
    TicketKeyIn(KeySource),
    /// Places the token entry on the project its repository is mapped to.
    RepositoryProject,
    /// Sets tags, such as `purpose: code-review`. A `ticket` or `project`
    /// here places the token entry like the rules above.
    SetTags(Tags),
}

/// Where a rule looks for ticket keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeySource {
    Branch,
    PullRequestTitle,
    PromptsOrSessionTitle,
}

/// A test on one signal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Condition {
    pub signal: SignalKey,
    pub test: Test,
}

impl Condition {
    /// Whether the condition holds.
    #[must_use]
    pub fn holds(&self, signals: &Signals, ticket: Option<&TagValue>) -> bool {
        let pull_request = signals.pull_request.map(|number| number.to_string());
        let value = match self.signal {
            SignalKey::Branch => signals.branch.as_deref(),
            SignalKey::WorkingDirectory => signals.working_directory.as_deref(),
            SignalKey::Repository => signals.repository.as_deref(),
            SignalKey::PullRequest => pull_request.as_deref(),
            SignalKey::SubagentType => signals.subagent_type.as_deref(),
            SignalKey::Ticket => ticket.map(TagValue::as_str),
        };
        self.test.passes(value)
    }
}

/// The signals a rule can test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalKey {
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

/// How a signal is tested. Every test but `is_set` fails when the signal
/// is missing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Test {
    Is(String),
    StartsWith(String),
    Contains(String),
    Matches(TextPattern),
    IsSet,
}

impl Test {
    /// Whether a signal's value passes the test.
    #[must_use]
    pub fn passes(&self, value: Option<&str>) -> bool {
        match (self, value) {
            (Self::IsSet, value) => value.is_some(),
            (_, None) => false,
            (Self::Is(expected), Some(value)) => value == expected,
            (Self::StartsWith(prefix), Some(value)) => value.starts_with(prefix.as_str()),
            (Self::Contains(part), Some(value)) => value.contains(part.as_str()),
            (Self::Matches(pattern), Some(value)) => pattern.is_match(value),
        }
    }
}

/// What the "ticket key in prompts" rule does when the prompts name several
/// tickets.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeveralKeys {
    /// The ticket named most often wins; on a tie, the one named last.
    #[default]
    MostOften,
    /// The token entry is split equally between the tickets.
    EqualSplit,
    /// The ticket named first wins.
    FirstNamed,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn condition(signal: SignalKey, test: Test) -> Condition {
        Condition { signal, test }
    }

    fn signals() -> Signals {
        Signals {
            branch: Some("review/KD-214".to_owned()),
            working_directory: Some("/work/storefront".to_owned()),
            repository: Some("kestrel/storefront".to_owned()),
            pull_request: Some(88),
            subagent_type: Some("code-reviewer".to_owned()),
            ..Signals::default()
        }
    }

    #[test]
    fn tests_compare_the_signal_text() {
        assert!(Test::Is("a".to_owned()).passes(Some("a")));
        assert!(!Test::Is("a".to_owned()).passes(Some("ab")));
        assert!(Test::StartsWith("re".to_owned()).passes(Some("review")));
        assert!(!Test::StartsWith("view".to_owned()).passes(Some("review")));
        assert!(Test::Contains("vie".to_owned()).passes(Some("review")));
        assert!(!Test::Contains("x".to_owned()).passes(Some("review")));
        let pattern = Test::Matches(TextPattern::new("^rev").unwrap());
        assert!(pattern.passes(Some("review")));
        assert!(!pattern.passes(Some("preview")));
        assert!(Test::IsSet.passes(Some("")));
        assert!(!Test::IsSet.passes(None));
        for test in [
            Test::Is(String::new()),
            Test::StartsWith(String::new()),
            Test::Contains(String::new()),
            Test::Matches(TextPattern::new("").unwrap()),
        ] {
            assert!(!test.passes(None), "{test:?}");
        }
    }

    #[test]
    fn conditions_read_each_signal() {
        let signals = signals();
        let ticket = TagValue::new("KD-214").unwrap();
        let holds = |signal, expected: &str| {
            condition(signal, Test::Is(expected.to_owned())).holds(&signals, Some(&ticket))
        };
        assert!(holds(SignalKey::Branch, "review/KD-214"));
        assert!(holds(SignalKey::WorkingDirectory, "/work/storefront"));
        assert!(holds(SignalKey::Repository, "kestrel/storefront"));
        assert!(holds(SignalKey::PullRequest, "88"));
        assert!(holds(SignalKey::SubagentType, "code-reviewer"));
        assert!(holds(SignalKey::Ticket, "KD-214"));
        assert!(!condition(SignalKey::Ticket, Test::IsSet).holds(&signals, None));
        assert!(!condition(SignalKey::Branch, Test::IsSet).holds(&Signals::default(), None));
    }

    #[test]
    fn a_rule_applies_when_all_its_conditions_hold() {
        let rule = Rule {
            name: "review branches".to_owned(),
            when: vec![
                condition(SignalKey::Branch, Test::StartsWith("review/".to_owned())),
                condition(
                    SignalKey::Repository,
                    Test::Is("kestrel/storefront".to_owned()),
                ),
            ],
            then: Action::SetTags(Tags::parse(["purpose=code-review"]).unwrap()),
        };
        assert!(rule.applies(&signals(), None));
        let mut other_repository = signals();
        other_repository.repository = Some("kestrel/app".to_owned());
        assert!(!rule.applies(&other_repository, None));
        let mut other_branch = signals();
        other_branch.branch = Some("main".to_owned());
        assert!(!rule.applies(&other_branch, None));
        let always = Rule {
            when: Vec::new(),
            ..rule
        };
        assert!(always.applies(&Signals::default(), None));
    }

    #[test]
    fn only_set_tags_with_a_ticket_sets_a_ticket() {
        let sets = Rule {
            name: "ops".to_owned(),
            when: Vec::new(),
            then: Action::SetTags(Tags::parse(["ticket=OPS-1", "purpose=ops"]).unwrap()),
        };
        assert_eq!(sets.sets_ticket().unwrap().as_str(), "OPS-1");
        let only_purpose = Rule {
            then: Action::SetTags(Tags::parse(["purpose=ops"]).unwrap()),
            ..sets.clone()
        };
        assert_eq!(only_purpose.sets_ticket(), None);
        for rule in Rule::defaults() {
            assert_eq!(rule.sets_ticket(), None);
        }
    }

    #[test]
    fn the_default_rules_are_the_four_from_the_docs() {
        let names: Vec<String> = Rule::defaults().into_iter().map(|rule| rule.name).collect();
        assert_eq!(
            names,
            vec![
                "ticket key in branch",
                "ticket key in pull request title",
                "ticket key in prompts or session title",
                "repository",
            ]
        );
        assert_eq!(SeveralKeys::default(), SeveralKeys::MostOften);
    }

    #[test]
    fn rules_read_and_write_as_json() {
        let json = r#"{
            "name": "review branches",
            "when": [{"signal": "branch", "test": {"starts_with": "review/"}},
                     {"signal": "pull_request", "test": "is_set"}],
            "then": {"set_tags": {"purpose": "code-review"}}
        }"#;
        let rule: Rule = serde_json::from_str(json).unwrap();
        assert_eq!(rule.when.len(), 2);
        assert!(rule.applies(&signals(), None));
        let again: Rule = serde_json::from_str(&serde_json::to_string(&rule).unwrap()).unwrap();
        assert_eq!(again, rule);
        let key_rule: Rule =
            serde_json::from_str(r#"{"name": "b", "then": {"ticket_key_in": "branch"}}"#).unwrap();
        assert_eq!(key_rule.then, Action::TicketKeyIn(KeySource::Branch));
    }
}
