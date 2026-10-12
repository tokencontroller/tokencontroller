//! What this machine knows about the person's organization: its tag names,
//! projects with their ticket key patterns and repositories, the rules, the
//! cadence. Connected, it comes from the service (`GET /api/v1/me/context`);
//! local, only the personal rules and a ticket key pattern from `tc config`.
//!
//! Rules are the core's own (`tokencontroller_core::Rule`), so the machine
//! and the service run the same rules the same way (ADR 0010).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use tokencontroller_core::directory;
use tokencontroller_core::pattern::TicketKeyPattern;
use tokencontroller_core::rules::Action;
use tokencontroller_core::{CostSource, Currency, Rule, SeveralKeys, TagName, TagValue, Tags};

use crate::keys::KeyPattern;

/// The person in the directory tc hands the core. The service knows who
/// the person is from the token, so the tag never leaves the machine.
pub const PERSON: &str = "me";

/// The project that holds a pattern from `tc config ticket-key-pattern`,
/// which belongs to no project. Its name never shows.
pub const LOCAL_PATTERN_PROJECT: &str = "tc config ticket-key-pattern";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Context {
    #[serde(default)]
    pub organization: Option<Organization>,
    #[serde(default)]
    pub person: Option<Person>,
    #[serde(default)]
    pub projects: Vec<Project>,
    /// The person's own rules, in order. They run after the organization's.
    #[serde(default)]
    pub rules: Vec<Rule>,
    #[serde(default)]
    pub cadence: Option<Cadence>,
    /// Token entries a people manager sent back, with the reason.
    #[serde(default)]
    pub rejected: Vec<Rejection>,
    /// A controller asked everyone to submit before the monthly close.
    #[serde(default)]
    pub asked_to_submit: Option<AskedToSubmit>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Organization {
    pub name: String,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub tag_names: Vec<String>,
    /// What the "ticket key in prompts" rule does when prompts name several
    /// tickets.
    #[serde(default)]
    pub several_keys: SeveralKeys,
    /// The organization's rules in order, the default ones included.
    #[serde(default = "Rule::defaults")]
    pub rules: Vec<Rule>,
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
    /// The service's word: an earlier cadence period still waits for its
    /// token sheet.
    #[serde(default)]
    pub late: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AskedToSubmit {
    /// The month, such as `2026-09`.
    pub period: String,
    #[serde(default)]
    pub asked_by: Option<String>,
    #[serde(default)]
    pub asked_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Rejection {
    pub local_id: String,
    pub reason: String,
}

/// A rule as the list shows it: its name, and the tags it sets.
pub fn describe(rule: &Rule) -> String {
    match &rule.then {
        Action::SetTags(tags) if !tags.is_empty() => {
            let set: Vec<String> = tags
                .iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect();
            format!("{} → {}", rule.name, set.join(" "))
        }
        Action::SetTags(_) | Action::TicketKeyIn(_) | Action::RepositoryProject => {
            rule.name.clone()
        }
    }
}

/// Tags the core can use: pairs with a name or value it would refuse are
/// left out.
pub fn core_tags(tags: &BTreeMap<String, String>) -> Tags {
    let mut core = Tags::new();
    for (name, value) in tags {
        if let (Ok(name), Ok(value)) = (TagName::new(name), TagValue::new(value)) {
            core.insert(name, value);
        }
    }
    core
}

impl Context {
    /// The compiled ticket key patterns of every project. With no
    /// organization, the one from `tc config ticket-key-pattern` instead.
    pub fn key_patterns(&self, personal: Option<&str>) -> Vec<KeyPattern> {
        let mut patterns: Vec<KeyPattern> = self
            .projects
            .iter()
            .filter_map(|project| {
                let pattern = project.ticket_key_pattern.as_deref()?;
                KeyPattern::new(Some(project.name.clone()), pattern)
            })
            .collect();
        if self.organization.is_none()
            && let Some(pattern) = personal.and_then(|pattern| KeyPattern::new(None, pattern))
        {
            patterns.push(pattern);
        }
        patterns
    }

    /// The organization's rules, or the default ones with no organization.
    pub fn organization_rules(&self) -> Vec<Rule> {
        self.organization
            .as_ref()
            .map_or_else(Rule::defaults, |organization| organization.rules.clone())
    }

    /// Whether a tag name may be set: built-in names always, others only if
    /// the organization lists them. With no organization, any name the core
    /// takes.
    pub fn allows_tag(&self, name: &str) -> bool {
        let Ok(name) = TagName::new(name) else {
            return false;
        };
        name.is_built_in()
            || self.organization.as_ref().is_none_or(|organization| {
                organization
                    .tag_names
                    .iter()
                    .any(|allowed| allowed == name.as_str())
            })
    }

    pub fn several_keys(&self) -> SeveralKeys {
        self.organization
            .as_ref()
            .map(|organization| organization.several_keys)
            .unwrap_or_default()
    }

    /// The organization as the core places token entries with: its tag
    /// names, rules, projects and the person with their tags and rules. With
    /// no organization, the default rules and a pattern from `tc config`.
    pub fn for_the_core(&self, local_pattern: Option<&str>) -> tokencontroller_core::Organization {
        let mut organization =
            tokencontroller_core::Organization::new(Currency::USD, CostSource::ApiKey);
        organization.summary_match = false;
        organization.rules = self.organization_rules();
        organization.several_keys = self.several_keys();
        organization.tag_names = self
            .organization
            .iter()
            .flat_map(|organization| &organization.tag_names)
            .filter_map(|name| TagName::new(name).ok())
            .collect::<BTreeSet<TagName>>();
        if let Ok(person) = TagValue::new(PERSON) {
            organization.directory.people.insert(
                person,
                directory::Person {
                    tags: self
                        .person
                        .as_ref()
                        .map(|person| core_tags(&person.tags))
                        .unwrap_or_default(),
                    rules: self.rules.clone(),
                },
            );
        }
        for project in &self.projects {
            let Ok(name) = TagValue::new(&project.name) else {
                continue;
            };
            organization.directory.projects.insert(
                name,
                directory::Project {
                    tags: core_tags(&project.tags),
                    ticket_key_pattern: project
                        .ticket_key_pattern
                        .as_deref()
                        .and_then(|pattern| TicketKeyPattern::new(pattern).ok()),
                    repositories: project.repositories.clone(),
                },
            );
        }
        if self.organization.is_none()
            && let Some(pattern) = local_pattern.and_then(|p| KeyPattern::new(None, p))
            && let Ok(name) = TagValue::new(LOCAL_PATTERN_PROJECT)
            && let Ok(pattern) = TicketKeyPattern::new(pattern.as_str())
        {
            organization.directory.projects.insert(
                name,
                directory::Project {
                    ticket_key_pattern: Some(pattern),
                    ..directory::Project::default()
                },
            );
        }
        organization
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokencontroller_core::rules::{KeySource, SignalKey, Test};

    fn service_context() -> Context {
        let json = r#"{
            "organization": {"name": "Kestrel Digital", "currency": "EUR", "tag_names": ["team", "client"], "several_keys": "equal_split",
                "rules": [{"name": "ticket key in branch", "then": {"ticket_key_in": "branch"}},
                          {"name": "review branches", "when": [{"signal": "branch", "test": {"starts_with": "review/"}}], "then": {"set_tags": {"purpose": "code-review"}}}]},
            "person": {"email": "m.ortner@kestrel.example", "tags": {"team": "web"}},
            "projects": [{"name": "storefront", "ticket_key_pattern": "^(KD|OPS)-\\d+", "repositories": ["kestrel/storefront"], "tags": {"client": "acme"}}],
            "rules": [{"name": "my spikes", "when": [{"signal": "branch", "test": {"contains": "spike"}}], "then": {"set_tags": {"purpose": "learning"}}}],
            "cadence": null,
            "rejected": [{"local_id": "claude-code:s1", "reason": "wrong client"}],
            "something_new": true
        }"#;
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn the_service_shape_reads() {
        let context = service_context();
        assert_eq!(context.several_keys(), SeveralKeys::EqualSplit);
        assert_eq!(context.key_patterns(None).len(), 1);
        // Connected, each project's pattern applies, not the local one.
        assert_eq!(context.key_patterns(Some(r"APP-\d+")).len(), 1);
        assert!(context.allows_tag("team"));
        assert!(context.allows_tag("ticket"));
        assert!(!context.allows_tag("purpose"));
        assert!(!context.allows_tag("Team"));
        let rules = context.organization_rules();
        assert_eq!(rules.len(), 2);
        assert_eq!(
            rules.first().map(|rule| &rule.then),
            Some(&Action::TicketKeyIn(KeySource::Branch))
        );
        assert_eq!(
            rules.iter().map(describe).collect::<Vec<_>>(),
            [
                "ticket key in branch",
                "review branches → purpose=code-review"
            ]
        );
        let condition = context.rules.first().unwrap().when.first().unwrap();
        assert_eq!(condition.signal, SignalKey::Branch);
        assert_eq!(condition.test, Test::Contains("spike".into()));
    }

    #[test]
    fn the_core_gets_the_organization_person_and_projects() {
        let context = service_context();
        let core = context.for_the_core(Some(r"APP-\d+"));
        assert_eq!(core.rules.len(), 2);
        assert_eq!(core.several_keys, SeveralKeys::EqualSplit);
        assert!(!core.summary_match);
        assert!(core.allows(&TagName::new("team").unwrap()));
        assert!(!core.allows(&TagName::new("purpose").unwrap()));
        let me = TagValue::new(PERSON).unwrap();
        let person = core.directory.people.get(&me).unwrap();
        assert_eq!(person.rules.len(), 1);
        assert_eq!(person.tags, Tags::parse(["team=web"]).unwrap());
        assert_eq!(core.directory.projects.len(), 1);
        assert_eq!(
            core.directory
                .project_of_repository("kestrel/storefront")
                .map(TagValue::as_str),
            Some("storefront")
        );
    }

    #[test]
    fn without_an_organization_the_defaults_and_the_local_pattern_apply() {
        let context = Context::default();
        assert!(context.allows_tag("purpose"));
        assert!(!context.allows_tag("not a name"));
        assert_eq!(context.several_keys(), SeveralKeys::MostOften);
        assert_eq!(context.organization_rules(), Rule::defaults());
        assert_eq!(context.key_patterns(Some(r"APP-\d+")).len(), 1);
        let core = context.for_the_core(Some(r"APP-\d+"));
        let key = TagValue::new("APP-7").unwrap();
        assert_eq!(
            core.directory.project_of_ticket(&key).map(TagValue::as_str),
            Some(LOCAL_PATTERN_PROJECT)
        );
        assert!(context.for_the_core(None).directory.projects.is_empty());
    }

    #[test]
    fn tags_the_core_refuses_are_left_out() {
        let tags = BTreeMap::from([
            ("team".to_owned(), "web".to_owned()),
            ("Bad Name".to_owned(), "x".to_owned()),
            ("client".to_owned(), " ".to_owned()),
        ]);
        assert_eq!(core_tags(&tags), Tags::parse(["team=web"]).unwrap());
    }
}
