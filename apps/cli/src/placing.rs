//! The machine pass of the rules (Rules: "Where rules run"): turns a
//! session's signals into tags, in the order the docs give.
//!
//! 1. A ticket named in the token entry: by hand, by the mod, by the Action.
//! 2. Rules that find a ticket: the key in the branch, then in the pull
//!    request title, then in prompts or the session title; then rules that set
//!    a ticket, organization before personal.
//! 3. The summary match runs in the service only.
//! 4. Rules that place on a project, then the repository's config file, then
//!    the repository mapping.
//! 5. Nothing: organization overhead with its person.
//!
//! Other tags: tags named in the entry win; then every rule runs, and when two
//! set the same name the organization rule wins, then the one listed first;
//! then what the project and the person pass on.

use std::collections::BTreeMap;

use crate::context::{Context, Scope, SeveralKeys, Signal};
use crate::keys::{KeyPattern, project_of};

/// Ticket keys found in one place, with how often and when they were last
/// named.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeyCount {
    pub key: String,
    pub count: u64,
    pub first_at_ms: i64,
    pub last_at_ms: i64,
}

/// What is known about one segment of a session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Signals {
    pub agent_harness: String,
    pub branch: Option<String>,
    pub directory: Option<String>,
    pub repository: Option<String>,
    pub pull_request: Option<String>,
    pub subagent: Option<String>,
    /// Keys in the pull request title, in order.
    pub pull_request_title_keys: Vec<String>,
    /// Keys in prompts and the session title.
    pub prompt_keys: Vec<KeyCount>,
    /// Tags from the repository's `.tokencontroller.toml`.
    pub config_file_tags: BTreeMap<String, String>,
}

impl Signals {
    fn value(&self, signal: Signal) -> Option<&str> {
        match signal {
            Signal::Branch => self.branch.as_deref(),
            Signal::Directory => self.directory.as_deref(),
            Signal::Repository => self.repository.as_deref(),
            Signal::PullRequest => self.pull_request.as_deref(),
            Signal::Subagent => self.subagent.as_deref(),
            Signal::AgentHarness => Some(self.agent_harness.as_str()),
        }
    }

    /// Every key found anywhere in the session, in order, once.
    pub fn all_keys(&self, patterns: &[KeyPattern]) -> Vec<String> {
        let mut keys: Vec<String> = Vec::new();
        let branch_keys = self
            .branch
            .as_deref()
            .map(|branch| crate::keys::find_all(patterns, branch))
            .unwrap_or_default();
        let found = branch_keys
            .into_iter()
            .chain(self.pull_request_title_keys.iter().cloned())
            .chain(self.prompt_keys.iter().map(|count| count.key.clone()));
        for key in found {
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        keys
    }
}

/// Tags named in the token entry itself: set by hand with `tc tag`, sent by
/// the mod with `tc record`, or given to the Action. They win over rules.
pub type Named = BTreeMap<String, String>;

/// Where a segment landed and why.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Placement {
    pub tags: BTreeMap<String, String>,
    /// What placed the ticket or project: `hand`, a rule's name, `classifier`
    /// or `none`.
    pub placed_by: String,
    /// When prompts named several tickets and the organization splits
    /// equally: the tickets to split between.
    pub equal_split: Vec<String>,
    /// Whether the ticket came only from a mention in prompts or the title,
    /// which is when the classifier may have the last word.
    pub ticket_only_mentioned: bool,
}

/// The classifier's answer for a session, when it ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    Ticket(String),
    Nothing,
}

/// Places one segment.
pub fn place(
    signals: &Signals,
    named: &Named,
    context: &Context,
    patterns: &[KeyPattern],
    pick: Option<&Pick>,
) -> Placement {
    let mut placement = Placement::default();

    // 1 and 2: the ticket.
    let mut ticket: Option<(String, String)> = named
        .get("ticket")
        .map(|ticket| (ticket.clone(), "hand".to_owned()));
    if ticket.is_none() {
        ticket = find_ticket(signals, context, patterns, &mut placement);
    }
    if placement.ticket_only_mentioned || ticket.is_none() {
        match pick {
            Some(Pick::Ticket(key)) => {
                ticket = Some((key.clone(), "classifier".to_owned()));
                placement.ticket_only_mentioned = false;
                placement.equal_split.clear();
            }
            Some(Pick::Nothing) if placement.ticket_only_mentioned => {
                ticket = None;
                placement.ticket_only_mentioned = false;
                placement.equal_split.clear();
            }
            _ => {}
        }
    }

    // 4: the project, unless the ticket's project decides it.
    let mut project: Option<(String, String)> = named
        .get("project")
        .map(|project| (project.clone(), "hand".to_owned()));
    if let Some((key, _)) = &ticket
        && project.is_none()
    {
        project = project_of(patterns, key).map(|name| (name, String::new()));
    }
    if ticket.is_none() && project.is_none() {
        project = find_project(signals, context);
    }

    // Other tags: rules first, so what is named and inherited can be laid
    // over them in the right order.
    let mut tags = rule_tags(signals, context);
    for (name, value) in &signals.config_file_tags {
        if name != "project" && name != "ticket" && context.allows_tag(name) {
            tags.insert(name.clone(), value.clone());
        }
    }
    for (name, value) in named {
        if context.allows_tag(name) {
            tags.insert(name.clone(), value.clone());
        }
    }
    tags.remove("ticket");
    tags.remove("project");
    let project_record = project.as_ref().and_then(|(name, _)| context.project(name));
    let inherited = project_record
        .map(|project| project.tags.iter())
        .into_iter()
        .flatten()
        .chain(
            context
                .person
                .as_ref()
                .map(|person| person.tags.iter())
                .into_iter()
                .flatten(),
        );
    for (name, value) in inherited {
        tags.entry(name.clone()).or_insert_with(|| value.clone());
    }

    placement.placed_by = match (&ticket, &project) {
        (Some((_, by)), _) | (None, Some((_, by))) => by.clone(),
        (None, None) => "none".to_owned(),
    };
    if let Some((key, _)) = ticket {
        tags.insert("ticket".to_owned(), key);
    }
    if let Some((name, _)) = project {
        tags.insert("project".to_owned(), name);
    }
    placement.tags = tags;
    placement
}

/// Step 2: rules that find a ticket. Returns the ticket and the rule's name.
fn find_ticket(
    signals: &Signals,
    context: &Context,
    patterns: &[KeyPattern],
    placement: &mut Placement,
) -> Option<(String, String)> {
    if let Some(key) = signals
        .branch
        .as_deref()
        .and_then(|branch| crate::keys::find_all(patterns, branch).into_iter().next())
    {
        return Some((key, "rule: ticket key in branch".to_owned()));
    }
    if let Some(key) = signals.pull_request_title_keys.first() {
        return Some((key.clone(), "rule: ticket key in PR title".to_owned()));
    }
    if let Some(key) = from_prompts(&signals.prompt_keys, context.several_keys(), placement) {
        placement.ticket_only_mentioned = true;
        return Some((key, "rule: ticket key in prompts".to_owned()));
    }
    matching_rules(signals, context)
        .find_map(|(rule_name, set)| set.get("ticket").map(|ticket| (ticket.clone(), rule_name)))
}

/// The ticket from keys in prompts and the title, by the organization's
/// setting. With an equal split, the first ticket stands for the entry and
/// the split lists them all.
fn from_prompts(keys: &[KeyCount], mode: SeveralKeys, placement: &mut Placement) -> Option<String> {
    match mode {
        SeveralKeys::First => keys
            .iter()
            .min_by_key(|count| count.first_at_ms)
            .map(|count| count.key.clone()),
        SeveralKeys::Split => {
            if keys.len() > 1 {
                placement.equal_split = keys.iter().map(|count| count.key.clone()).collect();
            }
            keys.first().map(|count| count.key.clone())
        }
        SeveralKeys::MostNamed => keys
            .iter()
            .max_by(|a, b| a.count.cmp(&b.count).then(a.last_at_ms.cmp(&b.last_at_ms)))
            .map(|count| count.key.clone()),
    }
}

/// Step 4: rules that place on a project, the config file, the repository
/// mapping.
fn find_project(signals: &Signals, context: &Context) -> Option<(String, String)> {
    if let Some(found) = matching_rules(signals, context).find_map(|(rule_name, set)| {
        set.get("project")
            .map(|project| (project.clone(), rule_name))
    }) {
        return Some(found);
    }
    if let Some(project) = signals.config_file_tags.get("project") {
        return Some((project.clone(), "repository config file".to_owned()));
    }
    let repository = signals.repository.as_deref()?;
    context
        .project_of_repository(repository)
        .map(|project| (project.name.clone(), "rule: repository".to_owned()))
}

/// Rules whose condition holds, organization rules first, each with the name
/// shown in the review list.
fn matching_rules<'a>(
    signals: &'a Signals,
    context: &'a Context,
) -> impl Iterator<Item = (String, &'a BTreeMap<String, String>)> + 'a {
    let organization = context
        .rules
        .iter()
        .filter(|rule| rule.scope == Scope::Organization);
    let personal = context
        .rules
        .iter()
        .filter(|rule| rule.scope == Scope::Personal);
    organization.chain(personal).filter_map(move |rule| {
        let value = signals.value(rule.when.signal)?;
        rule.when.test.holds(value, &rule.when.value).then(|| {
            (
                format!("rule: {} {}", rule.when.signal.word(), rule.when.value),
                &rule.set,
            )
        })
    })
}

/// Every other tag the rules set: the first rule to set a name wins.
fn rule_tags(signals: &Signals, context: &Context) -> BTreeMap<String, String> {
    let mut tags = BTreeMap::new();
    for (_, set) in matching_rules(signals, context) {
        for (name, value) in set {
            if context.allows_tag(name) {
                tags.entry(name.clone()).or_insert_with(|| value.clone());
            }
        }
    }
    tags
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{Condition, Organization, Person, Project, Rule, Test};

    fn context() -> Context {
        Context {
            organization: Some(Organization {
                name: "Kestrel Digital".into(),
                currency: Some("EUR".into()),
                tag_names: vec!["team".into(), "client".into(), "purpose".into()],
                several_keys: SeveralKeys::MostNamed,
            }),
            person: Some(Person {
                email: "m.ortner@kestrel.example".into(),
                name: None,
                tags: BTreeMap::from([("team".into(), "web".into())]),
            }),
            projects: vec![
                Project {
                    name: "storefront".into(),
                    ticket_key_pattern: Some(r"^(KD|OPS)-\d+".into()),
                    repositories: vec!["kestrel/storefront".into()],
                    tags: BTreeMap::from([("client".into(), "acme".into())]),
                },
                Project {
                    name: "intranet".into(),
                    ticket_key_pattern: Some(r"^INT-\d+".into()),
                    repositories: vec!["kestrel/intranet".into()],
                    tags: BTreeMap::from([("client".into(), "kestrel".into())]),
                },
            ],
            rules: vec![
                rule(
                    Scope::Personal,
                    Signal::Branch,
                    Test::StartsWith,
                    "review/",
                    &[("purpose", "learning")],
                ),
                rule(
                    Scope::Organization,
                    Signal::Branch,
                    Test::StartsWith,
                    "review/",
                    &[("purpose", "code-review")],
                ),
                rule(
                    Scope::Organization,
                    Signal::Directory,
                    Test::Contains,
                    "/docs",
                    &[("project", "intranet")],
                ),
            ],
            cadence: None,
            rejected: Vec::new(),
        }
    }

    fn rule(scope: Scope, signal: Signal, test: Test, value: &str, set: &[(&str, &str)]) -> Rule {
        Rule {
            scope,
            when: Condition {
                signal,
                test,
                value: value.into(),
            },
            set: set
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect(),
        }
    }

    fn signals(branch: &str) -> Signals {
        Signals {
            agent_harness: "claude-code".into(),
            branch: Some(branch.into()),
            repository: Some("kestrel/storefront".into()),
            ..Signals::default()
        }
    }

    fn key(key: &str, count: u64, last_at_ms: i64) -> KeyCount {
        KeyCount {
            key: key.into(),
            count,
            first_at_ms: last_at_ms,
            last_at_ms,
        }
    }

    fn placed(signals: &Signals, named: &Named, pick: Option<&Pick>) -> Placement {
        let context = context();
        let patterns = context.key_patterns(None);
        place(signals, named, &context, &patterns, pick)
    }

    fn tag<'a>(placement: &'a Placement, name: &str) -> Option<&'a str> {
        placement.tags.get(name).map(String::as_str)
    }

    #[test]
    fn a_key_in_the_branch_places_the_ticket_and_its_project() {
        let placement = placed(&signals("KD-214-fix"), &Named::new(), None);
        assert_eq!(tag(&placement, "ticket"), Some("KD-214"));
        assert_eq!(tag(&placement, "project"), Some("storefront"));
        assert_eq!(tag(&placement, "client"), Some("acme"));
        assert_eq!(tag(&placement, "team"), Some("web"));
        assert_eq!(placement.placed_by, "rule: ticket key in branch");
        assert!(!placement.ticket_only_mentioned);
    }

    #[test]
    fn a_named_ticket_wins_over_every_rule() {
        let named = Named::from([("ticket".into(), "INT-5".into())]);
        let placement = placed(&signals("KD-214-fix"), &named, None);
        assert_eq!(tag(&placement, "ticket"), Some("INT-5"));
        assert_eq!(tag(&placement, "project"), Some("intranet"));
        assert_eq!(tag(&placement, "client"), Some("kestrel"));
        assert_eq!(placement.placed_by, "hand");
    }

    #[test]
    fn the_pull_request_title_comes_before_prompts() {
        let mut s = signals("main");
        s.pull_request_title_keys = vec!["KD-231".into()];
        s.prompt_keys = vec![key("KD-240", 5, 9)];
        let placement = placed(&s, &Named::new(), None);
        assert_eq!(tag(&placement, "ticket"), Some("KD-231"));
        assert_eq!(placement.placed_by, "rule: ticket key in PR title");
    }

    #[test]
    fn the_most_named_prompt_key_wins_and_a_tie_goes_to_the_latest() {
        let mut s = signals("main");
        s.prompt_keys = vec![key("KD-1", 2, 10), key("KD-2", 3, 5)];
        assert_eq!(
            tag(&placed(&s, &Named::new(), None), "ticket"),
            Some("KD-2")
        );
        s.prompt_keys = vec![key("KD-1", 3, 10), key("KD-2", 3, 5)];
        let placement = placed(&s, &Named::new(), None);
        assert_eq!(tag(&placement, "ticket"), Some("KD-1"));
        assert!(placement.ticket_only_mentioned);
        assert_eq!(placement.placed_by, "rule: ticket key in prompts");
    }

    #[test]
    fn the_organization_can_take_the_first_key_or_split() {
        let mut s = signals("main");
        s.prompt_keys = vec![key("KD-1", 1, 10), key("KD-2", 3, 5)];
        let mut context = context();
        let patterns = context.key_patterns(None);
        if let Some(organization) = context.organization.as_mut() {
            organization.several_keys = SeveralKeys::First;
        }
        let first = place(&s, &Named::new(), &context, &patterns, None);
        assert_eq!(tag(&first, "ticket"), Some("KD-2"));
        if let Some(organization) = context.organization.as_mut() {
            organization.several_keys = SeveralKeys::Split;
        }
        let split = place(&s, &Named::new(), &context, &patterns, None);
        assert_eq!(split.equal_split, ["KD-1", "KD-2"]);
        s.prompt_keys = vec![key("KD-1", 1, 10)];
        let single = place(&s, &Named::new(), &context, &patterns, None);
        assert!(single.equal_split.is_empty());
    }

    #[test]
    fn the_classifier_has_the_last_word_on_mentions_only() {
        let mut s = signals("main");
        s.prompt_keys = vec![key("KD-1", 1, 10), key("KD-2", 1, 5)];
        let picked = placed(&s, &Named::new(), Some(&Pick::Ticket("KD-2".into())));
        assert_eq!(tag(&picked, "ticket"), Some("KD-2"));
        assert_eq!(picked.placed_by, "classifier");
        let nothing = placed(&s, &Named::new(), Some(&Pick::Nothing));
        assert_eq!(tag(&nothing, "ticket"), None);
        // With no ticket, the repository mapping places the project.
        assert_eq!(tag(&nothing, "project"), Some("storefront"));
        assert_eq!(nothing.placed_by, "rule: repository");
        // A key in the branch is not a mention: the classifier is not asked.
        let branch = placed(
            &signals("KD-7"),
            &Named::new(),
            Some(&Pick::Ticket("KD-2".into())),
        );
        assert_eq!(tag(&branch, "ticket"), Some("KD-7"));
    }

    #[test]
    fn the_classifier_can_place_a_session_rules_could_not() {
        let s = signals("main");
        let placement = placed(&s, &Named::new(), Some(&Pick::Ticket("INT-3".into())));
        assert_eq!(tag(&placement, "ticket"), Some("INT-3"));
        assert_eq!(tag(&placement, "project"), Some("intranet"));
        let ignored = placed(&s, &Named::new(), Some(&Pick::Nothing));
        assert_eq!(tag(&ignored, "project"), Some("storefront"));
    }

    #[test]
    fn organization_rules_win_over_personal_ones() {
        let placement = placed(&signals("review/checkout"), &Named::new(), None);
        assert_eq!(tag(&placement, "purpose"), Some("code-review"));
        let named = Named::from([("purpose".into(), "spike".into())]);
        assert_eq!(
            tag(&placed(&signals("review/x"), &named, None), "purpose"),
            Some("spike")
        );
    }

    #[test]
    fn project_rules_come_before_the_config_file_and_the_mapping() {
        let mut s = signals("main");
        s.directory = Some("/work/storefront/docs".into());
        let by_rule = placed(&s, &Named::new(), None);
        assert_eq!(tag(&by_rule, "project"), Some("intranet"));
        assert_eq!(by_rule.placed_by, "rule: directory /docs");

        let mut s = signals("main");
        s.config_file_tags = BTreeMap::from([
            ("project".into(), "intranet".into()),
            ("purpose".into(), "product".into()),
            ("unlisted".into(), "x".into()),
        ]);
        let by_file = placed(&s, &Named::new(), None);
        assert_eq!(tag(&by_file, "project"), Some("intranet"));
        assert_eq!(tag(&by_file, "purpose"), Some("product"));
        assert_eq!(tag(&by_file, "unlisted"), None);
        assert_eq!(by_file.placed_by, "repository config file");

        // A ticket found by a rule wins over the file's project.
        let mut s = signals("KD-9");
        s.config_file_tags = BTreeMap::from([("project".into(), "intranet".into())]);
        assert_eq!(
            tag(&placed(&s, &Named::new(), None), "project"),
            Some("storefront")
        );
    }

    #[test]
    fn with_nothing_to_go_on_it_stays_organization_overhead() {
        let mut s = signals("main");
        s.repository = Some("kestrel/unmapped".into());
        let placement = placed(&s, &Named::new(), None);
        assert_eq!(tag(&placement, "ticket"), None);
        assert_eq!(tag(&placement, "project"), None);
        assert_eq!(tag(&placement, "team"), Some("web"));
        assert_eq!(placement.placed_by, "none");
    }

    #[test]
    fn rules_can_set_a_ticket_or_a_project() {
        let mut context = context();
        context.rules.push(rule(
            Scope::Personal,
            Signal::Subagent,
            Test::Is,
            "release-notes",
            &[("ticket", "OPS-1")],
        ));
        let patterns = context.key_patterns(None);
        let mut s = signals("main");
        s.subagent = Some("release-notes".into());
        let placement = place(&s, &Named::new(), &context, &patterns, None);
        assert_eq!(tag(&placement, "ticket"), Some("OPS-1"));
        assert_eq!(placement.placed_by, "rule: subagent release-notes");
    }

    #[test]
    fn all_keys_lists_every_key_once() {
        let context = context();
        let patterns = context.key_patterns(None);
        let mut s = signals("KD-1-x");
        s.pull_request_title_keys = vec!["KD-2".into(), "KD-1".into()];
        s.prompt_keys = vec![key("KD-3", 1, 1), key("KD-2", 1, 1)];
        assert_eq!(s.all_keys(&patterns), ["KD-1", "KD-2", "KD-3"]);
    }
}
