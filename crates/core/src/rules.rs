//! Rules: a pattern over a signal that gives a tag value.
//!
//! A rule has a scope (the organization or one person) and an order. Rules
//! run in order, lower first, the organization's before a person's at the
//! same order, and resolve per tag name: for each tag name the first rule
//! that matches gives its value, so a rule on `contract` and a rule on
//! `feature` both apply to one message. A signal that matches no tag value
//! leaves the message without that tag. "Remember" on a hand assignment
//! creates a rule. Rules apply on arrival and run back over messages since
//! the cutoff; a rule on `contract` runs back before it too (see
//! [`crate::State`]). A rule may create the contract it gives the first
//! time it matches, such as a shipped rule on how an agent was signed in.

use serde::{Deserialize, Serialize};

use std::collections::BTreeMap;

use crate::contract::ContractTerms;
use crate::message::{Message, Signals};
use crate::pattern::TextPattern;
use crate::tags::{TagName, TagValue, Tags};
use crate::values::Values;

/// Whose rule it is.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Organization,
    Person(TagValue),
}

/// The signal a rule reads. Signals are not tags.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Signal {
    Branch,
    WorkingDirectory,
    Repository,
    PullRequestTitle,
    SubagentType,
    /// Each key found in the prompts, one at a time.
    PromptKeys,
    AgentName,
    /// A value of the config file in the repository, by name.
    Config(String),
    /// A telemetry resource attribute, by name.
    Resource(String),
    /// Any signal, by the name it came with, such as `organization.id`:
    /// a text value among the open signals, else a telemetry attribute of
    /// that name (ADR 0028).
    Named(String),
}

/// What a matching rule writes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Then {
    /// A fixed tag, such as `project: shop`.
    Tag { name: TagName, key: TagValue },
    /// The key the pattern finds, as a value of this tag name. The key must
    /// be a tag value on record, else the rule does not match.
    KeyOf { name: TagName },
}

/// One rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    /// The name people see, such as `feature key in branch`.
    pub name: String,
    pub scope: Scope,
    /// Lower runs first.
    pub order: u32,
    pub signal: Signal,
    pub pattern: TextPattern,
    pub then: Then,
    /// The terms of the contract this rule gives, created the first time
    /// the rule matches when no contract of that key exists yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract: Option<ContractTerms>,
}

impl Rule {
    /// Whether the rule runs for a message: the organization's rules run
    /// for everyone, a person's only for their own messages.
    #[must_use]
    pub fn runs_for(&self, message: &Message) -> bool {
        match &self.scope {
            Scope::Organization => true,
            Scope::Person(person) => *person == message.person,
        }
    }

    /// The texts of the rule's signal on a message.
    #[must_use]
    pub fn texts<'m>(&self, message: &'m Message) -> Vec<&'m str> {
        let signals = &message.signals;
        let one = |text: &'m Option<String>| text.iter().map(String::as_str).collect();
        match &self.signal {
            Signal::Branch => one(&signals.branch),
            Signal::WorkingDirectory => one(&signals.working_directory),
            Signal::SubagentType => one(&signals.subagent_type),
            Signal::Repository => one(&signals.repository),
            Signal::PullRequestTitle => one(&signals.pull_request_title),
            Signal::AgentName => one(&message.agent_name),
            Signal::PromptKeys => signals.prompt_keys.iter().map(String::as_str).collect(),
            Signal::Config(name) => signals
                .config
                .get(name)
                .map(String::as_str)
                .into_iter()
                .collect(),
            Signal::Resource(name) => signals
                .resource
                .get(name)
                .map(String::as_str)
                .into_iter()
                .collect(),
            Signal::Named(name) if name == AGENT_HARNESS => vec![message.agent_harness.as_str()],
            Signal::Named(name) => named(signals, name).into_iter().collect(),
        }
    }

    /// The tag the rule gives a message, or `None` when it does not match.
    /// A key the rule finds must be a tag value on record, unless the rule
    /// creates its contract.
    /// The first text of the signal that matches decides.
    #[must_use]
    pub fn apply(&self, message: &Message, values: &Values) -> Option<(TagName, TagValue)> {
        if !self.runs_for(message) {
            return None;
        }
        for text in self.texts(message) {
            let Some(found) = self.pattern.key_in(text) else {
                continue;
            };
            return Some(match &self.then {
                Then::Tag { name, key } => (name.clone(), key.clone()),
                Then::KeyOf { name } => {
                    let Ok(key) = TagValue::new(found) else {
                        continue;
                    };
                    if self.contract.is_none() && !values.is_tag_value(name, &key) {
                        continue;
                    }
                    (name.clone(), key)
                }
            });
        }
        None
    }

    /// The tag name the rule gives a value of.
    #[must_use]
    pub const fn gives(&self) -> &TagName {
        match &self.then {
            Then::Tag { name, .. } | Then::KeyOf { name } => name,
        }
    }
}

/// The text of a signal by the name it came with: an open signal that is
/// text, else a telemetry attribute of that name.
#[must_use]
pub fn named<'m>(signals: &'m Signals, name: &str) -> Option<&'m str> {
    signals
        .other
        .get(name)
        .and_then(serde_json::Value::as_str)
        .or_else(|| signals.resource.get(name).map(String::as_str))
}

/// The name a rule reads the agent by, such as `claude-code`: the
/// message's own field, so the review list can remember "every message of
/// this agent is under this contract".
pub const AGENT_HARNESS: &str = "agent_harness";

/// The signal that names the account an agent was signed in with, where
/// the agent records one: Claude Code sends the organization of a
/// claude.ai or Console sign-in, and nothing for an API key.
pub const SIGNED_IN_ORGANIZATION: &str = "organization.id";

/// The rule we ship on how an agent was signed in: a message whose agent
/// names the Anthropic organization it was signed in with goes to that
/// organization's own contract, `anthropic-` and the first eight characters
/// of its id, created metered the first time it matches. It runs after the
/// organization's own rules, so any of theirs on `contract` wins. An API
/// key names no organization, so its messages get their contract from the
/// other steps.
#[must_use]
pub fn signed_in_contract(message: &Message) -> Option<(TagValue, ContractTerms)> {
    let id = named(&message.signals, SIGNED_IN_ORGANIZATION)?;
    let short: String = id.chars().take(8).collect::<String>().to_ascii_lowercase();
    let readable = short.chars().count() == 8 && short.chars().all(|c| c.is_ascii_hexdigit());
    if !readable {
        return None;
    }
    let key = TagValue::new(&format!("anthropic-{short}")).ok()?;
    Some((key, ContractTerms::metered("anthropic")))
}

/// The rules in the order they run: by order, the organization's before a
/// person's.
#[must_use]
pub fn in_order<'a>(rules: impl IntoIterator<Item = &'a Rule>) -> Vec<&'a Rule> {
    let mut ordered: Vec<&Rule> = rules.into_iter().collect();
    ordered.sort_by(|a, b| {
        a.order
            .cmp(&b.order)
            .then_with(|| a.scope.cmp(&b.scope))
            .then_with(|| a.name.cmp(&b.name))
    });
    ordered
}

/// For each tag name, the value the first matching rule gives a message,
/// with that rule. Rules on one tag name never stop rules on another.
#[must_use]
pub fn per_tag_name<'a>(
    rules: impl IntoIterator<Item = &'a Rule>,
    message: &Message,
    values: &Values,
) -> BTreeMap<TagName, (&'a Rule, TagValue)> {
    let mut given: BTreeMap<TagName, (&'a Rule, TagValue)> = BTreeMap::new();
    for rule in in_order(rules) {
        if given.contains_key(rule.gives()) {
            continue;
        }
        if let Some((name, key)) = rule.apply(message, values) {
            given.insert(name, (rule, key));
        }
    }
    given
}

/// The tags the rules give a message, one value per tag name.
#[must_use]
pub fn tags_of<'a>(
    rules: impl IntoIterator<Item = &'a Rule>,
    message: &Message,
    values: &Values,
) -> Tags {
    per_tag_name(rules, message, values)
        .into_iter()
        .map(|(name, (_, key))| (name, key))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{MessageId, ModelRef, SessionFacts, SessionId, Signals};
    use crate::values::TagValueRecord;
    use time::macros::datetime;

    fn name(text: &str) -> TagName {
        TagName::new(text).unwrap()
    }

    fn key(text: &str) -> TagValue {
        TagValue::new(text).unwrap()
    }

    fn tags(pairs: &[&str]) -> Tags {
        Tags::parse(pairs.iter().copied()).unwrap()
    }

    fn values() -> Values {
        let mut values = Values::default();
        for feature in ["KD-1", "KD-2"] {
            values.insert_tag_value(TagValueRecord {
                name: name("feature"),
                key: key(feature),
                title: None,
                description: None,
                source_id: None,
                source_status: None,
                defaults: tags(&["project=shop"]),
                contract: None,
            });
        }
        values
    }

    fn message(person: &str) -> Message {
        Message {
            session_id: SessionId::new("s1").unwrap(),
            message_id: MessageId::new("m1").unwrap(),
            person: key(person),
            model: ModelRef::new("anthropic", "claude-sonnet-4-5"),
            tokens: None,
            at: datetime!(2026-03-01 10:00 UTC),
            agent_harness: "claude-code".to_owned(),
            agent_name: Some("review bot".to_owned()),
            reported_cost: None,
            tags: Tags::new(),
            signals: Signals {
                branch: Some("feature/KD-1-login".to_owned()),
                working_directory: Some("/work/shop".to_owned()),
                repository: Some("kestrel/shop".to_owned()),
                pull_request: Some(12),
                pull_request_title: Some("KD-9: fix".to_owned()),
                subagent_type: Some("review".to_owned()),
                prompt_keys: vec!["KD-9".to_owned(), "KD-2".to_owned()],
                config: [("project".to_owned(), "shop".to_owned())]
                    .into_iter()
                    .collect(),
                resource: [("service.name".to_owned(), "bot".to_owned())]
                    .into_iter()
                    .collect(),
                other: BTreeMap::new(),
            },
            session: SessionFacts::default(),
            received: None,
        }
    }

    fn rule(
        name_text: &str,
        scope: Scope,
        order: u32,
        signal: Signal,
        pattern: &str,
        then: Then,
    ) -> Rule {
        Rule {
            name: name_text.to_owned(),
            scope,
            order,
            signal,
            pattern: TextPattern::new(pattern).unwrap(),
            then,
            contract: None,
        }
    }

    fn applied(rule: &Rule, message: &Message) -> Option<Tags> {
        rule.apply(message, &values())
            .map(|(name, key)| [(name, key)].into_iter().collect())
    }

    fn key_of_feature() -> Then {
        Then::KeyOf {
            name: name("feature"),
        }
    }

    #[test]
    fn a_key_in_the_branch_becomes_the_feature() {
        let branch = rule(
            "feature key in branch",
            Scope::Organization,
            10,
            Signal::Branch,
            r"KD-\d+",
            key_of_feature(),
        );
        assert_eq!(
            applied(&branch, &message("mara")),
            Some(tags(&["feature=KD-1"]))
        );
        let mut main = message("mara");
        main.signals.branch = Some("main".to_owned());
        assert_eq!(applied(&branch, &main), None);
        let mut none = message("mara");
        none.signals.branch = None;
        assert_eq!(applied(&branch, &none), None);
    }

    #[test]
    fn a_key_that_is_no_tag_value_leaves_the_message_alone() {
        let title = rule(
            "key in title",
            Scope::Organization,
            10,
            Signal::PullRequestTitle,
            r"KD-\d+",
            key_of_feature(),
        );
        assert_eq!(applied(&title, &message("mara")), None);
        // The first prompt key is unknown, the second is a feature.
        let prompts = rule(
            "key in prompts",
            Scope::Organization,
            10,
            Signal::PromptKeys,
            r"KD-\d+",
            key_of_feature(),
        );
        assert_eq!(
            applied(&prompts, &message("mara")),
            Some(tags(&["feature=KD-2"]))
        );
    }

    #[test]
    fn a_fixed_tag_comes_from_any_signal() {
        let fixed = Then::Tag {
            name: name("project"),
            key: key("shop"),
        };
        for signal in [
            Signal::Repository,
            Signal::WorkingDirectory,
            Signal::SubagentType,
            Signal::AgentName,
            Signal::Config("project".to_owned()),
            Signal::Resource("service.name".to_owned()),
        ] {
            let rule = rule(
                "r",
                Scope::Organization,
                1,
                signal.clone(),
                ".",
                fixed.clone(),
            );
            assert_eq!(
                applied(&rule, &message("mara")),
                Some(tags(&["project=shop"])),
                "{signal:?}"
            );
        }
        let missing = rule(
            "r",
            Scope::Organization,
            1,
            Signal::Config("other".to_owned()),
            ".",
            fixed.clone(),
        );
        assert_eq!(applied(&missing, &message("mara")), None);
        let missing = rule(
            "r",
            Scope::Organization,
            1,
            Signal::Resource("other".to_owned()),
            ".",
            fixed,
        );
        assert_eq!(applied(&missing, &message("mara")), None);
        assert_eq!(missing.texts(&message("mara")), Vec::<&str>::new());
    }

    #[test]
    fn a_named_signal_reads_open_signals_then_telemetry_attributes() {
        let mut with = message("mara");
        with.signals
            .other
            .insert("auth_type".to_owned(), serde_json::json!("vertex-ai"));
        with.signals
            .other
            .insert("seats".to_owned(), serde_json::json!(3));
        with.signals
            .resource
            .insert("seats".to_owned(), "three".to_owned());
        let read = |signal: &str| {
            rule(
                "r",
                Scope::Organization,
                1,
                Signal::Named(signal.to_owned()),
                ".",
                Then::Tag {
                    name: name("project"),
                    key: key("shop"),
                },
            )
            .texts(&with)
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<String>>()
        };
        assert_eq!(read("auth_type"), vec!["vertex-ai"]);
        // A value that is no text falls back to the telemetry attribute.
        assert_eq!(read("seats"), vec!["three"]);
        assert_eq!(read("service.name"), vec!["bot"]);
        assert!(read("missing").is_empty());
        // The agent is the message's own field, read by its name, even
        // when a signal of that name came with the message.
        assert_eq!(read(AGENT_HARNESS), vec![with.agent_harness.clone()]);
        let mut sent = with.clone();
        sent.signals
            .other
            .insert(AGENT_HARNESS.to_owned(), serde_json::json!("sent"));
        let agent = rule(
            "agent",
            Scope::Organization,
            1,
            Signal::Named(AGENT_HARNESS.to_owned()),
            ".",
            Then::Tag {
                name: name("project"),
                key: key("shop"),
            },
        );
        assert_eq!(agent.texts(&sent), vec![sent.agent_harness.as_str()]);
    }

    #[test]
    fn a_rule_that_creates_its_contract_takes_a_key_not_on_record() {
        let mut signed_in = message("mara");
        signed_in
            .signals
            .other
            .insert("workspace".to_owned(), serde_json::json!("ws-team"));
        let mut creating = rule(
            "workspace",
            Scope::Organization,
            1,
            Signal::Named("workspace".to_owned()),
            "ws-(?P<key>.+)",
            Then::KeyOf {
                name: TagName::contract(),
            },
        );
        assert_eq!(creating.apply(&signed_in, &values()), None);
        creating.contract = Some(ContractTerms::metered("anthropic"));
        assert_eq!(
            creating.apply(&signed_in, &values()),
            Some((TagName::contract(), key("team")))
        );
    }

    #[test]
    fn the_shipped_sign_in_rule_reads_the_first_eight_characters() {
        let with = |id: serde_json::Value| {
            let mut message = message("mara");
            message
                .signals
                .other
                .insert(SIGNED_IN_ORGANIZATION.to_owned(), id);
            signed_in_contract(&message).map(|(key, terms)| (key.to_string(), terms.provider))
        };
        assert_eq!(
            with(serde_json::json!("6F1E2A3B-0000-4000-8000-000000000001")),
            Some(("anthropic-6f1e2a3b".to_owned(), "anthropic".to_owned()))
        );
        assert_eq!(
            with(serde_json::json!("6f1e2a3b")),
            Some(("anthropic-6f1e2a3b".to_owned(), "anthropic".to_owned()))
        );
        assert_eq!(with(serde_json::json!("6f1e2a3")), None);
        assert_eq!(with(serde_json::json!("6f1e2a3g-0000")), None);
        assert_eq!(with(serde_json::json!(12_345_678)), None);
        assert_eq!(signed_in_contract(&message("mara")), None);
    }

    #[test]
    fn a_persons_rule_runs_only_for_their_messages() {
        let own = rule(
            "mine",
            Scope::Person(key("tom")),
            1,
            Signal::Repository,
            "shop",
            Then::Tag {
                name: name("project"),
                key: key("shop"),
            },
        );
        assert!(own.runs_for(&message("tom")));
        assert!(!own.runs_for(&message("mara")));
        assert_eq!(applied(&own, &message("mara")), None);
        assert_eq!(
            applied(&own, &message("tom")),
            Some(tags(&["project=shop"]))
        );
    }

    #[test]
    fn rules_run_by_order_the_organizations_first() {
        let late_org = rule(
            "org late",
            Scope::Organization,
            20,
            Signal::Repository,
            "shop",
            Then::Tag {
                name: name("project"),
                key: key("late"),
            },
        );
        let early_person = rule(
            "person early",
            Scope::Person(key("mara")),
            10,
            Signal::Repository,
            "shop",
            Then::Tag {
                name: name("project"),
                key: key("early"),
            },
        );
        let same_order_org = rule(
            "org same",
            Scope::Organization,
            10,
            Signal::Repository,
            "shop",
            Then::Tag {
                name: name("project"),
                key: key("same"),
            },
        );
        let rules = vec![late_org, early_person, same_order_org];
        let names: Vec<&str> = in_order(&rules)
            .into_iter()
            .map(|rule| rule.name.as_str())
            .collect();
        assert_eq!(names, vec!["org same", "person early", "org late"]);
        let given = per_tag_name(&rules, &message("mara"), &values());
        let (matched, key) = given.get(&name("project")).unwrap();
        assert_eq!(matched.name, "org same");
        assert_eq!(key.as_str(), "same");
        let for_tom = per_tag_name(&rules, &message("tom"), &values());
        assert_eq!(for_tom.get(&name("project")).unwrap().0.name, "org same");
        let mut other = message("tom");
        other.signals.repository = Some("other".to_owned());
        assert!(per_tag_name(&rules, &other, &values()).is_empty());
        assert!(per_tag_name(&Vec::<Rule>::new(), &message("tom"), &values()).is_empty());
    }

    #[test]
    fn rules_resolve_per_tag_name() {
        let contract = rule(
            "team sign-in",
            Scope::Organization,
            20,
            Signal::Resource("service.name".to_owned()),
            "^bot$",
            Then::Tag {
                name: name("contract"),
                key: key("claude-team"),
            },
        );
        let feature = rule(
            "feature key in branch",
            Scope::Organization,
            10,
            Signal::Branch,
            r"KD-\d+",
            key_of_feature(),
        );
        let second_feature = rule(
            "feature key in prompts",
            Scope::Organization,
            15,
            Signal::PromptKeys,
            r"KD-\d+",
            key_of_feature(),
        );
        assert_eq!(contract.gives(), &name("contract"));
        assert_eq!(feature.gives(), &name("feature"));
        let rules = vec![contract, second_feature, feature];
        // The feature rule matching first does not stop the contract rule,
        // and the second feature rule loses to the first.
        assert_eq!(
            tags_of(&rules, &message("mara"), &values()),
            tags(&["contract=claude-team", "feature=KD-1"])
        );
        let given = per_tag_name(&rules, &message("mara"), &values());
        assert_eq!(
            given.get(&name("feature")).unwrap().0.name,
            "feature key in branch"
        );
        let mut on_main = message("mara");
        on_main.signals.branch = Some("main".to_owned());
        assert_eq!(
            tags_of(&rules, &on_main, &values()),
            tags(&["contract=claude-team", "feature=KD-2"])
        );
    }

    #[test]
    fn rules_read_and_write_as_json() {
        let json = r#"{"name": "feature key in branch", "scope": "organization", "order": 10,
            "signal": "branch", "pattern": "KD-\\d+", "then": {"key_of": {"name": "feature"}}}"#;
        let rule: Rule = serde_json::from_str(json).unwrap();
        assert_eq!(rule.then, key_of_feature());
        let again: Rule = serde_json::from_str(&serde_json::to_string(&rule).unwrap()).unwrap();
        assert_eq!(again, rule);
        let personal: Rule = serde_json::from_str(
            r#"{"name": "bot", "scope": {"person": "tom"}, "order": 1, "signal": {"config": "project"},
            "pattern": "shop", "then": {"tag": {"name": "project", "key": "shop"}}}"#,
        )
        .unwrap();
        assert_eq!(personal.scope, Scope::Person(key("tom")));
        assert_eq!(personal.signal, Signal::Config("project".to_owned()));
        assert_eq!(personal.contract, None);
        let creates: Rule = serde_json::from_str(
            r#"{"name": "team sign-in", "scope": "organization", "order": 1,
            "signal": {"resource": "organization.id"}, "pattern": "^org-1$",
            "then": {"tag": {"name": "contract", "key": "claude-team"}},
            "contract": {"provider": "anthropic", "kind": "flat"}}"#,
        )
        .unwrap();
        assert_eq!(
            creates.contract.unwrap().kind,
            crate::contract::ContractKind::Flat
        );
    }
}
