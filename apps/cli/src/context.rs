//! What this machine knows about the person's organization: its tag names,
//! projects with their ticket key patterns and repositories, the rules, the
//! cadence. Connected, it comes from the service (`GET /api/v1/me/context`);
//! local, only the personal rules and a ticket key pattern from `tc config`.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::keys::KeyPattern;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Context {
    #[serde(default)]
    pub organization: Option<Organization>,
    #[serde(default)]
    pub person: Option<Person>,
    #[serde(default)]
    pub projects: Vec<Project>,
    /// Organization rules first, then personal ones, each group in order.
    #[serde(default)]
    pub rules: Vec<Rule>,
    #[serde(default)]
    pub cadence: Option<Cadence>,
    /// Token entries a people manager sent back, with the reason.
    #[serde(default)]
    pub rejected: Vec<Rejection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Organization {
    pub name: String,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub tag_names: Vec<String>,
    #[serde(default)]
    pub several_keys: SeveralKeys,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Person {
    pub email: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub tags: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Project {
    pub name: String,
    #[serde(default)]
    pub ticket_key_pattern: Option<String>,
    #[serde(default)]
    pub repositories: Vec<String>,
    #[serde(default)]
    pub tags: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Cadence {
    pub period_start: String,
    pub period_end: String,
    #[serde(default)]
    pub due_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Rejection {
    pub local_id: String,
    pub reason: String,
}

/// What to do when prompts name several tickets (Rules: "When prompts name
/// several tickets").
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SeveralKeys {
    /// The ticket named most often wins; on a tie, the latest one.
    #[default]
    MostNamed,
    /// An equal split between the tickets.
    Split,
    /// The first one named.
    First,
}

/// Whose rule it is. Organization rules run before personal ones.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Organization,
    #[default]
    Personal,
}

/// The signals a rule can test.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Signal {
    Branch,
    Directory,
    Repository,
    PullRequest,
    Subagent,
    AgentHarness,
}

impl Signal {
    pub const ALL: [Self; 6] = [
        Self::Branch,
        Self::Directory,
        Self::Repository,
        Self::PullRequest,
        Self::Subagent,
        Self::AgentHarness,
    ];

    /// The word used on the command line, such as `pull-request`.
    pub fn word(self) -> &'static str {
        match self {
            Self::Branch => "branch",
            Self::Directory => "directory",
            Self::Repository => "repository",
            Self::PullRequest => "pull-request",
            Self::Subagent => "subagent",
            Self::AgentHarness => "agent-harness",
        }
    }

    pub fn from_word(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|signal| signal.word() == word)
    }
}

/// How a rule compares a signal with its value.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Test {
    Is,
    StartsWith,
    EndsWith,
    Contains,
    Matches,
}

impl Test {
    pub const ALL: [Self; 5] = [
        Self::Is,
        Self::StartsWith,
        Self::EndsWith,
        Self::Contains,
        Self::Matches,
    ];

    pub fn word(self) -> &'static str {
        match self {
            Self::Is => "is",
            Self::StartsWith => "starts-with",
            Self::EndsWith => "ends-with",
            Self::Contains => "contains",
            Self::Matches => "matches",
        }
    }

    pub fn from_word(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|test| test.word() == word)
    }

    pub fn holds(self, signal: &str, value: &str) -> bool {
        match self {
            Self::Is => signal == value,
            Self::StartsWith => signal.starts_with(value),
            Self::EndsWith => signal.ends_with(value),
            Self::Contains => signal.contains(value),
            Self::Matches => regex::Regex::new(value).is_ok_and(|regex| regex.is_match(signal)),
        }
    }
}

/// A rule: when a signal passes a test, set tags.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Rule {
    #[serde(default)]
    pub scope: Scope,
    pub when: Condition,
    pub set: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Condition {
    pub signal: Signal,
    pub test: Test,
    pub value: String,
}

impl fmt::Display for Rule {
    /// `branch starts-with review/ → purpose=code-review`
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let set: Vec<String> = self
            .set
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect();
        write!(
            f,
            "{} {} {} → {}",
            self.when.signal.word(),
            self.when.test.word(),
            self.when.value,
            set.join(" ")
        )
    }
}

impl Context {
    /// The compiled ticket key patterns of every project, plus a personal
    /// one from `tc config ticket-key-pattern`.
    pub fn key_patterns(&self, personal: Option<&str>) -> Vec<KeyPattern> {
        let mut patterns: Vec<KeyPattern> = self
            .projects
            .iter()
            .filter_map(|project| {
                let pattern = project.ticket_key_pattern.as_deref()?;
                KeyPattern::new(Some(project.name.clone()), pattern)
            })
            .collect();
        if let Some(pattern) = personal.and_then(|pattern| KeyPattern::new(None, pattern)) {
            patterns.push(pattern);
        }
        patterns
    }

    /// The project a repository is mapped to.
    pub fn project_of_repository(&self, repository: &str) -> Option<&Project> {
        self.projects.iter().find(|project| {
            project
                .repositories
                .iter()
                .any(|mapped| mapped.eq_ignore_ascii_case(repository))
        })
    }

    pub fn project(&self, name: &str) -> Option<&Project> {
        self.projects.iter().find(|project| project.name == name)
    }

    /// Whether a tag name may be set: built-in names always, others only if
    /// the organization lists them. With no organization, any name.
    pub fn allows_tag(&self, name: &str) -> bool {
        if matches!(name, "ticket" | "project" | "person") {
            return true;
        }
        self.organization.as_ref().is_none_or(|organization| {
            organization.tag_names.is_empty()
                || organization.tag_names.iter().any(|allowed| allowed == name)
        })
    }

    pub fn several_keys(&self) -> SeveralKeys {
        self.organization
            .as_ref()
            .map(|organization| organization.several_keys)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_service_shape_reads() {
        let json = r#"{
            "organization": {"name": "Kestrel Digital", "currency": "EUR", "tag_names": ["team", "client"], "several_keys": "split"},
            "person": {"email": "m.ortner@kestrel.example", "tags": {"team": "web"}},
            "projects": [{"name": "storefront", "ticket_key_pattern": "^(KD|OPS)-\\d+", "repositories": ["kestrel/storefront"], "tags": {"client": "acme"}}],
            "rules": [{"scope": "organization", "when": {"signal": "branch", "test": "starts_with", "value": "review/"}, "set": {"purpose": "code-review"}}],
            "cadence": null,
            "rejected": [{"local_id": "claude-code:s1", "reason": "wrong client"}],
            "something_new": true
        }"#;
        let context: Context = serde_json::from_str(json).unwrap();
        assert_eq!(context.several_keys(), SeveralKeys::Split);
        assert_eq!(context.key_patterns(None).len(), 1);
        assert_eq!(context.key_patterns(Some(r"APP-\d+")).len(), 2);
        assert_eq!(
            context
                .project_of_repository("Kestrel/Storefront")
                .map(|p| p.name.as_str()),
            Some("storefront")
        );
        assert!(context.project_of_repository("kestrel/other").is_none());
        assert!(context.project("storefront").is_some());
        assert!(context.allows_tag("team"));
        assert!(context.allows_tag("ticket"));
        assert!(!context.allows_tag("purpose"));
        let rule = context.rules.first().unwrap();
        assert_eq!(
            rule.to_string(),
            "branch starts-with review/ → purpose=code-review"
        );
    }

    #[test]
    fn without_an_organization_every_tag_name_is_allowed() {
        let context = Context::default();
        assert!(context.allows_tag("purpose"));
        assert_eq!(context.several_keys(), SeveralKeys::MostNamed);
    }

    #[test]
    fn tests_compare_as_named() {
        assert!(Test::Is.holds("main", "main"));
        assert!(!Test::Is.holds("main2", "main"));
        assert!(Test::StartsWith.holds("review/x", "review/"));
        assert!(Test::EndsWith.holds("x-spike", "-spike"));
        assert!(Test::Contains.holds("a-docs-b", "docs"));
        assert!(Test::Matches.holds("release-1.2", r"^release-\d"));
        assert!(!Test::Matches.holds("x", "("));
    }

    #[test]
    fn words_round_trip() {
        for signal in Signal::ALL {
            assert_eq!(Signal::from_word(signal.word()), Some(signal));
        }
        for test in Test::ALL {
            assert_eq!(Test::from_word(test.word()), Some(test));
        }
        assert_eq!(Signal::from_word("prompt"), None);
        assert_eq!(Test::from_word("like"), None);
    }
}
