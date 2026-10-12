//! The machine pass of the rules (Rules: "Where rules run"). The core places
//! each segment (`Organization::place`), so the machine and the service
//! decide alike (ADR 0010, ADR 0015). This module hands the core what the
//! session files say, and lets the classifier have the last word where the
//! docs give it one: when the ticket came only from a mention in prompts or
//! the title, or when nothing found a ticket.

use std::collections::BTreeMap;

use tokencontroller_core::rules::{Action, KeySource};
use tokencontroller_core::{PlacedBy, PlacementRequest, TagName, TagValue, Tags};

use crate::context::{Context, LOCAL_PATTERN_PROJECT, PERSON, core_tags};
use crate::keys::KeyPattern;

/// Ticket keys found in one place, with how often and when they were first
/// and last named.
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

    /// The signals as the core reads them. Keys in prompts become one
    /// mention each, in the order they were named: the first mention of a
    /// key at the time it was first named and the others at the time it was
    /// last named, so "first named", "most often" and "on a tie, the latest"
    /// come out as they did in the session. Title keys count as named at the
    /// start.
    pub fn for_the_core(&self) -> tokencontroller_core::Signals {
        let mut mentions: Vec<(i64, &str)> = Vec::new();
        for count in &self.prompt_keys {
            if count.count == 0 {
                continue;
            }
            mentions.push((count.first_at_ms, count.key.as_str()));
            for _ in 1..count.count {
                mentions.push((count.last_at_ms, count.key.as_str()));
            }
        }
        mentions.sort_by_key(|(at_ms, _)| *at_ms);
        tokencontroller_core::Signals {
            branch: self.branch.clone(),
            working_directory: self.directory.clone(),
            repository: self.repository.clone(),
            pull_request: self
                .pull_request
                .as_deref()
                .and_then(|number| number.trim().trim_start_matches('#').parse().ok()),
            subagent_type: self.subagent.clone(),
            prompt_ticket_keys: mentions
                .into_iter()
                .map(|(_, key)| key.to_owned())
                .collect(),
            session_title_ticket_keys: Vec::new(),
            pull_request_title_ticket_keys: self.pull_request_title_keys.clone(),
        }
    }
}

/// Tags named in the token entry itself: set by hand with `tc tag`, sent by
/// the mod with `tc record`, or given to the Action. They win over rules.
pub type Named = BTreeMap<String, String>;

/// One share of a placed segment. Most placements have one; prompts naming
/// several tickets can give equal shares.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Share {
    pub weight: u64,
    pub tags: BTreeMap<String, String>,
}

/// Where a segment landed and why.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Placement {
    /// At least one share.
    pub shares: Vec<Share>,
    /// What placed the ticket or project, as the review list shows it after
    /// "placed by": `hand`, `config file`, `rule: …`, `classifier` or
    /// `none`.
    pub placed_by: String,
    /// Whether the ticket came only from a mention in prompts or the title,
    /// which is when the classifier may have the last word.
    pub ticket_only_mentioned: bool,
}

impl Placement {
    /// Whether a share landed on a ticket.
    pub fn has_ticket(&self) -> bool {
        self.shares
            .iter()
            .any(|share| share.tags.contains_key("ticket"))
    }

    /// The tags of the first share.
    pub fn tags(&self) -> BTreeMap<String, String> {
        self.shares
            .first()
            .map(|share| share.tags.clone())
            .unwrap_or_default()
    }
}

/// The classifier's answer for a session, when it ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    Ticket(String),
    Nothing,
}

/// Places segments with the core, for one context.
pub struct Placer {
    organization: tokencontroller_core::Organization,
    /// With no organization, every tag name is allowed.
    open_tag_names: bool,
    /// The names of the rules that find a ticket in prompts or the title.
    mention_rules: Vec<String>,
    person: Option<TagValue>,
}

impl Placer {
    pub fn new(context: &Context, local_pattern: Option<&str>) -> Self {
        let organization = context.for_the_core(local_pattern);
        let person = TagValue::new(PERSON).ok();
        let mention_rules = organization
            .rules_for(person.as_ref())
            .filter(|rule| rule.then == Action::TicketKeyIn(KeySource::PromptsOrSessionTitle))
            .map(|rule| rule.name.clone())
            .collect();
        Self {
            organization,
            open_tag_names: context.organization.is_none(),
            mention_rules,
            person,
        }
    }

    /// Places one segment.
    pub fn place(&self, signals: &Signals, named: &Named, pick: Option<&Pick>) -> Placement {
        let core_signals = signals.for_the_core();
        let mut named = core_tags(named);
        let configured = core_tags(&signals.config_file_tags);
        let first = self.run(&named, &configured, &core_signals);
        let mentioned = first.shares.first().is_some_and(|share| {
            matches!(&share.placed_by, PlacedBy::Rule(name) if self.mention_rules.contains(name))
        });
        let has_ticket = first
            .shares
            .iter()
            .any(|share| share.tags.contains(&TagName::ticket()));
        if mentioned || !has_ticket {
            match pick {
                Some(Pick::Ticket(key)) => {
                    if let Ok(ticket) = TagValue::new(key) {
                        named.insert(TagName::ticket(), ticket);
                        let picked = self.run(&named, &configured, &core_signals);
                        return finish(picked, Some("classifier"), false);
                    }
                }
                Some(Pick::Nothing) if mentioned => {
                    let mut unmentioned = core_signals.clone();
                    unmentioned.prompt_ticket_keys.clear();
                    unmentioned.session_title_ticket_keys.clear();
                    return finish(self.run(&named, &configured, &unmentioned), None, false);
                }
                Some(Pick::Nothing) | None => {}
            }
        }
        finish(first, None, mentioned)
    }

    fn run(
        &self,
        named: &Tags,
        configured: &Tags,
        signals: &tokencontroller_core::Signals,
    ) -> tokencontroller_core::Placement {
        let request = PlacementRequest {
            person: self.person.as_ref(),
            named,
            configured,
            signals,
            summary: None,
        };
        if !self.open_tag_names {
            return self.organization.place(&request, None);
        }
        let mut organization = self.organization.clone();
        let rule_tags = organization
            .rules_for(self.person.as_ref())
            .filter_map(|rule| match &rule.then {
                Action::SetTags(tags) => Some(tags.clone()),
                Action::TicketKeyIn(_) | Action::RepositoryProject => None,
            })
            .collect::<Vec<Tags>>();
        let tag_names = named
            .iter()
            .chain(configured.iter())
            .map(|(name, _)| name.clone())
            .chain(
                rule_tags
                    .iter()
                    .flat_map(|tags| tags.iter().map(|(name, _)| name.clone())),
            )
            .collect::<Vec<TagName>>();
        organization.tag_names.extend(tag_names);
        organization.place(&request, None)
    }
}

/// The core's placement as tc shows it: tags without the person, who is
/// known from the token, and without the project that only holds a local
/// pattern.
fn finish(
    placement: tokencontroller_core::Placement,
    placed_by: Option<&str>,
    ticket_only_mentioned: bool,
) -> Placement {
    let by = placement
        .shares
        .first()
        .map_or_else(|| "none".to_owned(), |share| shown(&share.placed_by));
    let shares = placement
        .shares
        .into_iter()
        .map(|share| Share {
            weight: share.weight,
            tags: share
                .tags
                .iter()
                .filter(|(name, value)| {
                    **name != TagName::person()
                        && !(**name == TagName::project()
                            && value.as_str() == LOCAL_PATTERN_PROJECT)
                })
                .map(|(name, value)| (name.as_str().to_owned(), value.as_str().to_owned()))
                .collect(),
        })
        .collect();
    Placement {
        shares,
        placed_by: placed_by.map_or(by, str::to_owned),
        ticket_only_mentioned,
    }
}

/// What placed a share, as the review list shows it after "placed by".
fn shown(placed_by: &PlacedBy) -> String {
    match placed_by {
        PlacedBy::Named => "hand".to_owned(),
        PlacedBy::Configuration => "config file".to_owned(),
        PlacedBy::Rule(name) => format!("rule: {name}"),
        PlacedBy::SummaryMatch => "summary".to_owned(),
        PlacedBy::Nothing => "none".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{Organization, Person, Project};
    use tokencontroller_core::rules::{Condition, SignalKey, Test};
    use tokencontroller_core::{Rule, SeveralKeys};

    fn rule(name: &str, signal: SignalKey, test: Test, set: &[&str]) -> Rule {
        Rule {
            name: name.into(),
            when: vec![Condition { signal, test }],
            then: Action::SetTags(Tags::parse(set.iter().copied()).unwrap()),
        }
    }

    fn context() -> Context {
        // The organization put its own rules before the default ones, so
        // the docs rule places on a project before the repository does.
        let mut rules = vec![
            rule(
                "review branches",
                SignalKey::Branch,
                Test::StartsWith("review/".into()),
                &["purpose=code-review"],
            ),
            rule(
                "docs",
                SignalKey::WorkingDirectory,
                Test::Contains("/docs".into()),
                &["project=intranet"],
            ),
        ];
        rules.extend(Rule::defaults());
        Context {
            organization: Some(Organization {
                name: "Kestrel Digital".into(),
                currency: Some("EUR".into()),
                tag_names: vec!["team".into(), "client".into(), "purpose".into()],
                several_keys: SeveralKeys::MostOften,
                rules,
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
            rules: vec![rule(
                "my reviews",
                SignalKey::Branch,
                Test::StartsWith("review/".into()),
                &["purpose=learning"],
            )],
            cadence: None,
            rejected: Vec::new(),
        }
    }

    fn signals(branch: &str) -> Signals {
        Signals {
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
        Placer::new(&context(), None).place(signals, named, pick)
    }

    fn tag(placement: &Placement, name: &str) -> Option<String> {
        placement.tags().get(name).cloned()
    }

    #[test]
    fn a_key_in_the_branch_places_the_ticket_and_its_project() {
        let placement = placed(&signals("KD-214-fix"), &Named::new(), None);
        assert_eq!(tag(&placement, "ticket").as_deref(), Some("KD-214"));
        assert_eq!(tag(&placement, "project").as_deref(), Some("storefront"));
        assert_eq!(tag(&placement, "client").as_deref(), Some("acme"));
        assert_eq!(tag(&placement, "team").as_deref(), Some("web"));
        assert_eq!(tag(&placement, "person"), None);
        assert_eq!(placement.placed_by, "rule: ticket key in branch");
        assert!(!placement.ticket_only_mentioned);
        assert!(placement.has_ticket());
    }

    #[test]
    fn a_named_ticket_wins_over_every_rule() {
        let named = Named::from([("ticket".into(), "INT-5".into())]);
        let placement = placed(&signals("KD-214-fix"), &named, None);
        assert_eq!(tag(&placement, "ticket").as_deref(), Some("INT-5"));
        assert_eq!(tag(&placement, "project").as_deref(), Some("intranet"));
        assert_eq!(tag(&placement, "client").as_deref(), Some("kestrel"));
        assert_eq!(placement.placed_by, "hand");
    }

    #[test]
    fn the_pull_request_title_comes_before_prompts() {
        let mut s = signals("main");
        s.pull_request_title_keys = vec!["KD-231".into()];
        s.prompt_keys = vec![key("KD-240", 5, 9)];
        let placement = placed(&s, &Named::new(), None);
        assert_eq!(tag(&placement, "ticket").as_deref(), Some("KD-231"));
        assert_eq!(
            placement.placed_by,
            "rule: ticket key in pull request title"
        );
    }

    #[test]
    fn the_most_named_prompt_key_wins_and_a_tie_goes_to_the_latest() {
        let mut s = signals("main");
        s.prompt_keys = vec![key("KD-1", 2, 10), key("KD-2", 3, 5)];
        assert_eq!(
            tag(&placed(&s, &Named::new(), None), "ticket").as_deref(),
            Some("KD-2")
        );
        s.prompt_keys = vec![key("KD-1", 3, 10), key("KD-2", 3, 5)];
        let placement = placed(&s, &Named::new(), None);
        assert_eq!(tag(&placement, "ticket").as_deref(), Some("KD-1"));
        assert!(placement.ticket_only_mentioned);
        assert_eq!(
            placement.placed_by,
            "rule: ticket key in prompts or session title"
        );
    }

    #[test]
    fn the_organization_can_take_the_first_key_or_split() {
        let mut s = signals("main");
        s.prompt_keys = vec![
            KeyCount {
                key: "KD-1".into(),
                count: 2,
                first_at_ms: 1,
                last_at_ms: 10,
            },
            key("KD-2", 3, 5),
        ];
        let mut context = context();
        if let Some(organization) = context.organization.as_mut() {
            organization.several_keys = SeveralKeys::FirstNamed;
        }
        let first = Placer::new(&context, None).place(&s, &Named::new(), None);
        assert_eq!(tag(&first, "ticket").as_deref(), Some("KD-1"));
        if let Some(organization) = context.organization.as_mut() {
            organization.several_keys = SeveralKeys::EqualSplit;
        }
        let placer = Placer::new(&context, None);
        let split = placer.place(&s, &Named::new(), None);
        let tickets: Vec<String> = split
            .shares
            .iter()
            .filter_map(|share| share.tags.get("ticket").cloned())
            .collect();
        assert_eq!(tickets, ["KD-1", "KD-2"]);
        assert!(split.shares.iter().all(|share| share.weight == 1));
        s.prompt_keys = vec![key("KD-1", 1, 10)];
        assert_eq!(placer.place(&s, &Named::new(), None).shares.len(), 1);
    }

    #[test]
    fn the_classifier_has_the_last_word_on_mentions_only() {
        let mut s = signals("main");
        s.prompt_keys = vec![key("KD-1", 1, 10), key("KD-2", 1, 5)];
        let picked = placed(&s, &Named::new(), Some(&Pick::Ticket("KD-2".into())));
        assert_eq!(tag(&picked, "ticket").as_deref(), Some("KD-2"));
        assert_eq!(picked.placed_by, "classifier");
        let nothing = placed(&s, &Named::new(), Some(&Pick::Nothing));
        assert_eq!(tag(&nothing, "ticket"), None);
        // With no ticket, the repository mapping places the project.
        assert_eq!(tag(&nothing, "project").as_deref(), Some("storefront"));
        assert_eq!(nothing.placed_by, "rule: repository");
        // A key in the branch is not a mention: the classifier is not asked.
        let branch = placed(
            &signals("KD-7"),
            &Named::new(),
            Some(&Pick::Ticket("KD-2".into())),
        );
        assert_eq!(tag(&branch, "ticket").as_deref(), Some("KD-7"));
    }

    #[test]
    fn the_classifier_can_place_a_session_rules_could_not() {
        let s = signals("main");
        let placement = placed(&s, &Named::new(), Some(&Pick::Ticket("INT-3".into())));
        assert_eq!(tag(&placement, "ticket").as_deref(), Some("INT-3"));
        assert_eq!(tag(&placement, "project").as_deref(), Some("intranet"));
        let ignored = placed(&s, &Named::new(), Some(&Pick::Nothing));
        assert_eq!(tag(&ignored, "project").as_deref(), Some("storefront"));
    }

    #[test]
    fn organization_rules_win_over_personal_ones() {
        let placement = placed(&signals("review/checkout"), &Named::new(), None);
        assert_eq!(tag(&placement, "purpose").as_deref(), Some("code-review"));
        let named = Named::from([("purpose".into(), "spike".into())]);
        assert_eq!(
            tag(&placed(&signals("review/x"), &named, None), "purpose").as_deref(),
            Some("spike")
        );
    }

    #[test]
    fn the_config_file_names_a_project_before_rules_do() {
        let mut s = signals("main");
        s.directory = Some("/work/storefront/docs".into());
        let by_rule = placed(&s, &Named::new(), None);
        assert_eq!(tag(&by_rule, "project").as_deref(), Some("intranet"));
        assert_eq!(by_rule.placed_by, "rule: docs");

        let mut s = signals("main");
        s.config_file_tags = BTreeMap::from([
            ("project".into(), "intranet".into()),
            ("purpose".into(), "product".into()),
            ("unlisted".into(), "x".into()),
        ]);
        let by_file = placed(&s, &Named::new(), None);
        assert_eq!(tag(&by_file, "project").as_deref(), Some("intranet"));
        assert_eq!(tag(&by_file, "purpose").as_deref(), Some("product"));
        assert_eq!(tag(&by_file, "unlisted"), None);
        assert_eq!(by_file.placed_by, "config file");

        // A ticket found by a rule wins over the file's project.
        let mut s = signals("KD-9");
        s.config_file_tags = BTreeMap::from([("project".into(), "intranet".into())]);
        assert_eq!(
            tag(&placed(&s, &Named::new(), None), "project").as_deref(),
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
        assert_eq!(tag(&placement, "team").as_deref(), Some("web"));
        assert_eq!(placement.placed_by, "none");
        assert!(!placement.has_ticket());
    }

    #[test]
    fn rules_can_set_a_ticket_or_test_the_pull_request() {
        let mut context = context();
        context.rules.push(rule(
            "release notes",
            SignalKey::SubagentType,
            Test::Is("release-notes".into()),
            &["ticket=OPS-1"],
        ));
        context.rules.push(rule(
            "pull request reviews",
            SignalKey::PullRequest,
            Test::IsSet,
            &["purpose=code-review"],
        ));
        let placer = Placer::new(&context, None);
        let mut s = signals("main");
        s.subagent = Some("release-notes".into());
        s.pull_request = Some("#12".into());
        let placement = placer.place(&s, &Named::new(), None);
        assert_eq!(tag(&placement, "ticket").as_deref(), Some("OPS-1"));
        assert_eq!(tag(&placement, "purpose").as_deref(), Some("code-review"));
        assert_eq!(placement.placed_by, "rule: release notes");
    }

    #[test]
    fn with_no_organization_any_tag_and_the_local_pattern_work() {
        let context = Context {
            rules: vec![rule(
                "spikes",
                SignalKey::Branch,
                Test::Contains("spike".into()),
                &["purpose=learning"],
            )],
            ..Context::default()
        };
        let placer = Placer::new(&context, Some(r"^APP-\d+"));
        let named = Named::from([("client".into(), "acme".into())]);
        let placement = placer.place(&signals("APP-3-spike"), &named, None);
        assert_eq!(tag(&placement, "ticket").as_deref(), Some("APP-3"));
        assert_eq!(tag(&placement, "project"), None);
        assert_eq!(tag(&placement, "purpose").as_deref(), Some("learning"));
        assert_eq!(tag(&placement, "client").as_deref(), Some("acme"));
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

    #[test]
    fn mentions_keep_their_order_and_number() {
        let s = Signals {
            prompt_keys: vec![
                KeyCount {
                    key: "KD-1".into(),
                    count: 3,
                    first_at_ms: 1,
                    last_at_ms: 9,
                },
                key("KD-2", 1, 5),
                key("KD-3", 0, 2),
            ],
            pull_request: Some("12".into()),
            ..Signals::default()
        };
        let core = s.for_the_core();
        assert_eq!(core.prompt_ticket_keys, ["KD-1", "KD-2", "KD-1", "KD-1"]);
        assert_eq!(core.pull_request, Some(12));
    }
}
