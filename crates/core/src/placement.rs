//! Placing a token entry: the order that decides its ticket and project, and
//! the tags it ends up with.
//!
//! The first step that finds a place wins:
//!
//! 1. a ticket named in the token entry, by hand or sent with it;
//! 2. rules that find a ticket, organization rules before personal ones;
//! 3. the summary match, in the service only;
//! 4. a project named in the token entry, then one from the repository's
//!    config file or the agent's settings, then rules that place on a
//!    project;
//! 5. nothing: the token entry stays organization overhead with its person.
//!
//! A placed ticket brings its project. The other tags are layered, the upper
//! winning: tags named in the token entry, then rules (the organization's
//! and the first listed winning), then the config file and agent settings,
//! then tags inherited from the ticket, the project and the person.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::message::Signals;
use crate::organization::Organization;
use crate::rules::{Action, KeySource, Rule, SeveralKeys};
use crate::tags::{TagName, TagValue, Tags};

/// Finds the open ticket a summary clearly fits, when no rule found one.
/// The service implements it; the CLI does not.
pub trait SummaryMatcher {
    /// The ticket the summary clearly fits, among the open tickets of
    /// `project`, or of the whole organization when there is none.
    fn match_summary(&self, summary: &str, project: Option<&TagValue>) -> Option<TagValue>;
}

/// What a token entry, or one segment or share of it, says about itself.
#[derive(Debug, Clone, Copy)]
pub struct PlacementRequest<'a> {
    /// The agent manager.
    pub person: Option<&'a TagValue>,
    /// Tags named by hand or sent with the token entry. They win over every
    /// rule.
    pub named: &'a Tags,
    /// Tags from the repository's `.tokencontroller.toml` or the agent's
    /// telemetry resource attributes.
    pub configured: &'a Tags,
    pub signals: &'a Signals,
    pub summary: Option<&'a str>,
}

/// What placed a token entry on its ticket or project.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "by", content = "rule")]
pub enum PlacedBy {
    /// Named by hand or sent with the token entry.
    Named,
    /// The repository's config file or the agent's settings.
    Configuration,
    /// The rule with this name.
    Rule(String),
    /// Matched by summary. Always marked as such.
    SummaryMatch,
    /// Nothing found a ticket or project.
    Nothing,
}

impl fmt::Display for PlacedBy {
    /// Shows the source the way the review list does, such as
    /// `rule: ticket key in branch`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Named => f.write_str("by hand"),
            Self::Configuration => f.write_str("config file"),
            Self::Rule(name) => write!(f, "rule: {name}"),
            Self::SummaryMatch => f.write_str("matched by summary"),
            Self::Nothing => f.write_str("no ticket"),
        }
    }
}

/// One share of a placed token entry. Most placements have one; prompts
/// naming several tickets can give equal shares.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacedShare {
    /// The share's weight among the placement's shares.
    pub weight: u64,
    pub tags: Tags,
    pub placed_by: PlacedBy,
    /// Whether the ticket is not known yet: it fills in at the next sync.
    pub ticket_not_synced: bool,
}

/// The result of placing a token entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placement {
    pub shares: Vec<PlacedShare>,
    /// Tag names that were sent or set but are not on the organization's
    /// list, so they were dropped.
    pub dropped: Vec<TagName>,
}

impl Organization {
    /// Places a token entry, or one segment or share of it.
    #[must_use]
    pub fn place(
        &self,
        request: &PlacementRequest<'_>,
        matcher: Option<&dyn SummaryMatcher>,
    ) -> Placement {
        let rules: Vec<&Rule> = self.rules_for(request.person).collect();
        let tickets = self.find_tickets(request, &rules, matcher);
        let mut dropped: Vec<TagName> = request
            .named
            .iter()
            .chain(request.configured.iter())
            .map(|(name, _)| name)
            .filter(|name| !self.allows(name))
            .cloned()
            .collect();
        dropped.sort();
        dropped.dedup();

        let shares = match tickets {
            Some((tickets, placed_by)) => tickets
                .into_iter()
                .map(|ticket| self.share(request, &rules, Some((ticket, placed_by.clone()))))
                .collect(),
            None => vec![self.share(request, &rules, None)],
        };
        Placement { shares, dropped }
    }

    /// Steps 1 to 3: the tickets, with equal weights when there are several.
    fn find_tickets(
        &self,
        request: &PlacementRequest<'_>,
        rules: &[&Rule],
        matcher: Option<&dyn SummaryMatcher>,
    ) -> Option<(Vec<TagValue>, PlacedBy)> {
        if let Some(ticket) = request.named.get(&TagName::ticket()) {
            return Some((vec![ticket.clone()], PlacedBy::Named));
        }
        for rule in rules {
            if !rule.applies(request.signals, None) {
                continue;
            }
            let tickets = match &rule.then {
                Action::TicketKeyIn(source) => self.tickets_in(*source, request.signals),
                Action::SetTags(_) => rule.sets_ticket().cloned().into_iter().collect(),
                Action::RepositoryProject => Vec::new(),
            };
            if !tickets.is_empty() {
                return Some((tickets, PlacedBy::Rule(rule.name.clone())));
            }
        }
        let summary = request.summary.filter(|_| self.summary_match)?;
        let project = self
            .named_project(request, rules)
            .map(|(project, _)| project);
        matcher?
            .match_summary(summary, project.as_ref())
            .filter(|ticket| !self.missing_tickets.contains(ticket))
            .map(|ticket| (vec![ticket], PlacedBy::SummaryMatch))
    }

    /// The tickets a key source names, after the organization's choice for
    /// prompts that name several.
    fn tickets_in(&self, source: KeySource, signals: &Signals) -> Vec<TagValue> {
        let usable = |key: &TagValue| {
            !self.missing_tickets.contains(key) && self.directory.is_ticket_key(key)
        };
        let from_list = |keys: &[String]| -> Vec<TagValue> {
            keys.iter()
                .filter_map(|key| TagValue::new(key).ok())
                .filter(|key| usable(key))
                .collect()
        };
        match source {
            KeySource::Branch => signals
                .branch
                .as_deref()
                .map(|branch| self.directory.find_ticket_keys(branch))
                .unwrap_or_default()
                .into_iter()
                .map(|found| found.key)
                .find(|key| !self.missing_tickets.contains(key))
                .into_iter()
                .collect(),
            KeySource::PullRequestTitle => from_list(&signals.pull_request_title_ticket_keys)
                .into_iter()
                .take(1)
                .collect(),
            KeySource::PromptsOrSessionTitle => {
                let mentions: Vec<TagValue> = from_list(&signals.session_title_ticket_keys)
                    .into_iter()
                    .chain(from_list(&signals.prompt_ticket_keys))
                    .collect();
                choose(&mentions, self.several_keys)
            }
        }
    }

    /// Step 4: a project named in the token entry, then one configured,
    /// then the first rule that places on a project.
    fn named_project(
        &self,
        request: &PlacementRequest<'_>,
        rules: &[&Rule],
    ) -> Option<(TagValue, PlacedBy)> {
        let project = TagName::project();
        if let Some(named) = request.named.get(&project) {
            return Some((named.clone(), PlacedBy::Named));
        }
        if let Some(configured) = request.configured.get(&project) {
            return Some((configured.clone(), PlacedBy::Configuration));
        }
        rules.iter().find_map(|rule| {
            if !rule.applies(request.signals, None) {
                return None;
            }
            let found = match &rule.then {
                Action::RepositoryProject => request
                    .signals
                    .repository
                    .as_deref()
                    .and_then(|repository| self.directory.project_of_repository(repository)),
                Action::SetTags(tags) => tags.get(&project),
                Action::TicketKeyIn(_) => None,
            };
            found.map(|found| (found.clone(), PlacedBy::Rule(rule.name.clone())))
        })
    }

    /// One share: its ticket and project, then every other tag.
    fn share(
        &self,
        request: &PlacementRequest<'_>,
        rules: &[&Rule],
        ticket: Option<(TagValue, PlacedBy)>,
    ) -> PlacedShare {
        let ticket_project = ticket
            .as_ref()
            .and_then(|(ticket, _)| self.directory.project_of_ticket(ticket))
            .cloned();
        let project = self.named_project(request, rules);
        let placed_by = match (&ticket, &project) {
            (Some((_, by)), _) | (None, Some((_, by))) => by.clone(),
            (None, None) => PlacedBy::Nothing,
        };
        let project = ticket_project.or(project.map(|(project, _)| project));
        let ticket = ticket.map(|(ticket, _)| ticket);

        let from_rules = rules
            .iter()
            .rev()
            .fold(Tags::new(), |below, rule| match &rule.then {
                Action::SetTags(tags) if rule.applies(request.signals, ticket.as_ref()) => {
                    tags.over(&below)
                }
                Action::SetTags(_) | Action::TicketKeyIn(_) | Action::RepositoryProject => below,
            });
        let inherited =
            self.directory
                .inherited_tags(request.person, project.as_ref(), ticket.as_ref());
        let layered = request
            .named
            .over(&from_rules.over(&request.configured.over(&inherited)));

        let mut tags = layered.retain(|name| !name.is_built_in() && self.allows(name));
        let built_in = [
            (TagName::person(), request.person.cloned()),
            (TagName::ticket(), ticket.clone()),
            (TagName::project(), project),
        ];
        for (name, value) in built_in {
            if let Some(value) = value {
                tags.insert(name, value);
            }
        }
        PlacedShare {
            weight: 1,
            tags,
            placed_by,
            ticket_not_synced: ticket
                .as_ref()
                .is_some_and(|ticket| !self.directory.tickets.contains_key(ticket)),
        }
    }
}

/// Picks the ticket among keys named in prompts, in the order they were
/// named, once per mention.
fn choose(mentions: &[TagValue], several: SeveralKeys) -> Vec<TagValue> {
    match several {
        SeveralKeys::FirstNamed => mentions.first().cloned().into_iter().collect(),
        SeveralKeys::EqualSplit => {
            let mut distinct: Vec<TagValue> = Vec::new();
            for key in mentions {
                if !distinct.contains(key) {
                    distinct.push(key.clone());
                }
            }
            distinct
        }
        SeveralKeys::MostOften => {
            // (mentions, position of the last mention)
            let mut counts: BTreeMap<&TagValue, (usize, usize)> = BTreeMap::new();
            for (position, key) in mentions.iter().enumerate() {
                let entry = counts.entry(key).or_insert((0, position));
                *entry = (entry.0.saturating_add(1), position);
            }
            counts
                .into_iter()
                .max_by_key(|(_, count)| *count)
                .map(|(key, _)| key.clone())
                .into_iter()
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::directory::{Person, Project, Ticket};
    use crate::money::Currency;
    use crate::pattern::TicketKeyPattern;
    use crate::rules::{Condition, SignalKey, Test};
    use crate::status::CostSource;

    fn value(text: &str) -> TagValue {
        TagValue::new(text).unwrap()
    }

    fn name(text: &str) -> TagName {
        TagName::new(text).unwrap()
    }

    fn tags(pairs: &[&str]) -> Tags {
        Tags::parse(pairs.iter().copied()).unwrap()
    }

    fn keys(list: &[&str]) -> Vec<String> {
        list.iter().map(|key| (*key).to_owned()).collect()
    }

    /// Kestrel Digital from the quick start: one project with Jira keys and
    /// a mapped repository.
    fn kestrel() -> Organization {
        let mut organization = Organization::new(Currency::USD, CostSource::ApiKey);
        for tag in ["team", "client", "purpose"] {
            organization.tag_names.insert(name(tag));
        }
        organization.directory.people.insert(
            value("mara"),
            Person {
                tags: tags(&["team=web"]),
                rules: Vec::new(),
            },
        );
        organization.directory.projects.insert(
            value("storefront"),
            Project {
                tags: tags(&["client=acme"]),
                ticket_key_pattern: Some(TicketKeyPattern::new(r"^KD-\d+").unwrap()),
                repositories: vec!["kestrel/storefront".to_owned()],
            },
        );
        organization.directory.projects.insert(
            value("ops"),
            Project {
                tags: tags(&["client=internal"]),
                ticket_key_pattern: Some(TicketKeyPattern::new(r"^OPS-\d+").unwrap()),
                repositories: Vec::new(),
            },
        );
        for key in ["KD-214", "KD-231", "KD-240"] {
            organization.directory.tickets.insert(
                value(key),
                Ticket {
                    project: Some(value("storefront")),
                    ..Ticket::default()
                },
            );
        }
        organization
    }

    struct Place<'a> {
        organization: &'a Organization,
        named: Tags,
        configured: Tags,
        signals: Signals,
        summary: Option<&'a str>,
    }

    impl<'a> Place<'a> {
        fn new(organization: &'a Organization) -> Self {
            Self {
                organization,
                named: Tags::new(),
                configured: Tags::new(),
                signals: Signals::default(),
                summary: None,
            }
        }

        fn run(&self, matcher: Option<&dyn SummaryMatcher>) -> Placement {
            let mara = value("mara");
            self.organization.place(
                &PlacementRequest {
                    person: Some(&mara),
                    named: &self.named,
                    configured: &self.configured,
                    signals: &self.signals,
                    summary: self.summary,
                },
                matcher,
            )
        }

        fn one(&self) -> PlacedShare {
            let placement = self.run(None);
            assert_eq!(placement.shares.len(), 1);
            placement.shares.into_iter().next().unwrap()
        }
    }

    struct Fits(&'static str);

    impl SummaryMatcher for Fits {
        fn match_summary(&self, _summary: &str, project: Option<&TagValue>) -> Option<TagValue> {
            assert_eq!(project.map(TagValue::as_str), Some("storefront"));
            Some(value(self.0))
        }
    }

    #[test]
    fn a_ticket_key_in_the_branch_places_on_the_ticket_and_its_project() {
        let organization = kestrel();
        let mut place = Place::new(&organization);
        place.signals.branch = Some("KD-214-fix".to_owned());
        let share = place.one();
        assert_eq!(
            share.tags,
            tags(&[
                "person=mara",
                "team=web",
                "ticket=KD-214",
                "project=storefront",
                "client=acme"
            ])
        );
        assert_eq!(
            share.placed_by,
            PlacedBy::Rule("ticket key in branch".to_owned())
        );
        assert_eq!(share.placed_by.to_string(), "rule: ticket key in branch");
        assert!(!share.ticket_not_synced);
        assert_eq!(share.weight, 1);
    }

    #[test]
    fn the_branch_wins_over_the_pull_request_title_and_prompts() {
        let organization = kestrel();
        let mut place = Place::new(&organization);
        place.signals.branch = Some("KD-214-fix".to_owned());
        place.signals.pull_request_title_ticket_keys = keys(&["KD-231"]);
        place.signals.prompt_ticket_keys = keys(&["KD-240"]);
        assert_eq!(
            place.one().tags.get(&TagName::ticket()),
            Some(&value("KD-214"))
        );
        place.signals.branch = Some("main".to_owned());
        let share = place.one();
        assert_eq!(share.tags.get(&TagName::ticket()), Some(&value("KD-231")));
        assert_eq!(
            share.placed_by.to_string(),
            "rule: ticket key in pull request title"
        );
        place.signals.pull_request_title_ticket_keys = keys(&["NOPE-1", "KD-240", "KD-231"]);
        assert_eq!(
            place.one().tags.get(&TagName::ticket()),
            Some(&value("KD-240"))
        );
        place.signals.pull_request_title_ticket_keys.clear();
        let share = place.one();
        assert_eq!(share.tags.get(&TagName::ticket()), Some(&value("KD-240")));
        assert_eq!(
            share.placed_by.to_string(),
            "rule: ticket key in prompts or session title"
        );
    }

    #[test]
    fn a_named_ticket_wins_over_every_rule() {
        let organization = kestrel();
        let mut place = Place::new(&organization);
        place.signals.branch = Some("KD-214-fix".to_owned());
        place.named = tags(&["ticket=KD-231"]);
        let share = place.one();
        assert_eq!(share.tags.get(&TagName::ticket()), Some(&value("KD-231")));
        assert_eq!(share.placed_by, PlacedBy::Named);
        assert_eq!(share.placed_by.to_string(), "by hand");
    }

    #[test]
    fn a_repository_rule_places_on_the_project_as_overhead() {
        let organization = kestrel();
        let mut place = Place::new(&organization);
        place.signals.repository = Some("kestrel/storefront".to_owned());
        let share = place.one();
        assert_eq!(
            share.tags,
            tags(&[
                "person=mara",
                "team=web",
                "project=storefront",
                "client=acme"
            ])
        );
        assert_eq!(share.placed_by.to_string(), "rule: repository");
        place.signals.repository = Some("kestrel/elsewhere".to_owned());
        let share = place.one();
        assert_eq!(share.tags, tags(&["person=mara", "team=web"]));
        assert_eq!(share.placed_by, PlacedBy::Nothing);
        assert_eq!(share.placed_by.to_string(), "no ticket");
    }

    #[test]
    fn a_named_or_configured_project_wins_over_the_repository() {
        let organization = kestrel();
        let mut place = Place::new(&organization);
        place.signals.repository = Some("kestrel/storefront".to_owned());
        place.configured = tags(&["project=ops", "purpose=product"]);
        let share = place.one();
        assert_eq!(share.tags.get(&TagName::project()), Some(&value("ops")));
        assert_eq!(share.tags.get_str("purpose"), Some(&value("product")));
        assert_eq!(share.tags.get_str("client"), Some(&value("internal")));
        assert_eq!(share.placed_by, PlacedBy::Configuration);
        assert_eq!(share.placed_by.to_string(), "config file");
        place.named = tags(&["project=storefront"]);
        let share = place.one();
        assert_eq!(
            share.tags.get(&TagName::project()),
            Some(&value("storefront"))
        );
        assert_eq!(share.placed_by, PlacedBy::Named);
    }

    #[test]
    fn a_ticket_found_by_a_rule_wins_over_the_config_file_and_brings_its_project() {
        let organization = kestrel();
        let mut place = Place::new(&organization);
        place.configured = tags(&["project=ops"]);
        place.signals.branch = Some("KD-231-x".to_owned());
        let share = place.one();
        assert_eq!(
            share.tags.get(&TagName::project()),
            Some(&value("storefront"))
        );
        assert_eq!(share.tags.get_str("client"), Some(&value("acme")));
    }

    #[test]
    fn a_ticket_not_synced_yet_is_placed_and_marked() {
        let mut organization = kestrel();
        let mut place = Place::new(&organization);
        place.signals.branch = Some("KD-999-new".to_owned());
        let share = place.one();
        assert_eq!(share.tags.get(&TagName::ticket()), Some(&value("KD-999")));
        assert_eq!(
            share.tags.get(&TagName::project()),
            Some(&value("storefront"))
        );
        assert!(share.ticket_not_synced);

        // The next sync did not find it: the rules run again without it.
        organization.missing_tickets.insert(value("KD-999"));
        let mut place = Place::new(&organization);
        place.signals.branch = Some("KD-999-new".to_owned());
        place.signals.prompt_ticket_keys = keys(&["KD-999", "KD-240"]);
        let share = place.one();
        assert_eq!(share.tags.get(&TagName::ticket()), Some(&value("KD-240")));
        place.signals.prompt_ticket_keys = keys(&["KD-999"]);
        assert_eq!(place.one().placed_by, PlacedBy::Nothing);
    }

    #[test]
    fn several_keys_in_prompts_follow_the_organizations_choice() {
        let mut organization = kestrel();
        let mentions = keys(&["KD-214", "KD-231", "KD-231", "KD-214", "KD-240"]);
        let ticket_of = |organization: &Organization, mentions: &[String]| {
            let mut place = Place::new(organization);
            place.signals.prompt_ticket_keys = mentions.to_vec();
            place
                .run(None)
                .shares
                .into_iter()
                .map(|share| {
                    share
                        .tags
                        .get(&TagName::ticket())
                        .unwrap()
                        .as_str()
                        .to_owned()
                })
                .collect::<Vec<_>>()
        };
        // Two each for KD-214 and KD-231: the one named last wins.
        assert_eq!(ticket_of(&organization, &mentions), vec!["KD-214"]);
        assert_eq!(
            ticket_of(&organization, &keys(&["KD-214", "KD-231", "KD-231"])),
            vec!["KD-231"]
        );
        assert_eq!(
            ticket_of(&organization, &keys(&["KD-231", "KD-231", "KD-214"])),
            vec!["KD-231"]
        );
        organization.several_keys = SeveralKeys::FirstNamed;
        assert_eq!(ticket_of(&organization, &mentions), vec!["KD-214"]);
        assert_eq!(
            ticket_of(&organization, &keys(&["KD-240", "KD-214"])),
            vec!["KD-240"]
        );
        organization.several_keys = SeveralKeys::EqualSplit;
        assert_eq!(
            ticket_of(&organization, &mentions),
            vec!["KD-214", "KD-231", "KD-240"]
        );
    }

    #[test]
    fn session_title_keys_count_before_prompt_keys() {
        let mut organization = kestrel();
        organization.several_keys = SeveralKeys::FirstNamed;
        let mut place = Place::new(&organization);
        place.signals.session_title_ticket_keys = keys(&["KD-240"]);
        place.signals.prompt_ticket_keys = keys(&["KD-214"]);
        assert_eq!(
            place.one().tags.get(&TagName::ticket()),
            Some(&value("KD-240"))
        );
    }

    #[test]
    fn keys_from_the_machine_must_be_ticket_keys() {
        let organization = kestrel();
        let mut place = Place::new(&organization);
        place.signals.prompt_ticket_keys = keys(&["not a key", "", "JIRA-1"]);
        assert_eq!(place.one().placed_by, PlacedBy::Nothing);
    }

    #[test]
    fn the_summary_match_runs_only_when_no_rule_finds_a_ticket() {
        let mut organization = kestrel();
        let mut place = Place::new(&organization);
        place.signals.repository = Some("kestrel/storefront".to_owned());
        place.summary = Some("Fixed the checkout total");
        let placement = place.run(Some(&Fits("KD-231")));
        let share = placement.shares.first().unwrap();
        assert_eq!(share.tags.get(&TagName::ticket()), Some(&value("KD-231")));
        assert_eq!(share.placed_by, PlacedBy::SummaryMatch);
        assert_eq!(share.placed_by.to_string(), "matched by summary");

        place.signals.branch = Some("KD-214-x".to_owned());
        let share = place
            .run(Some(&Fits("KD-231")))
            .shares
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(share.tags.get(&TagName::ticket()), Some(&value("KD-214")));

        // Without a summary, a matcher, or with matching switched off, the
        // repository places it.
        place.signals.branch = None;
        place.summary = None;
        let share = place
            .run(Some(&Fits("KD-231")))
            .shares
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(share.placed_by, PlacedBy::Rule("repository".to_owned()));
        place.summary = Some("Fixed the checkout total");
        assert_eq!(
            place.one().placed_by,
            PlacedBy::Rule("repository".to_owned())
        );
        organization.summary_match = false;
        let mut place = Place::new(&organization);
        place.signals.repository = Some("kestrel/storefront".to_owned());
        place.summary = Some("Fixed the checkout total");
        let share = place
            .run(Some(&Fits("KD-231")))
            .shares
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(share.placed_by, PlacedBy::Rule("repository".to_owned()));

        // A ticket the last sync did not find is never matched.
        let mut organization = kestrel();
        organization.missing_tickets.insert(value("KD-231"));
        let mut place = Place::new(&organization);
        place.signals.repository = Some("kestrel/storefront".to_owned());
        place.summary = Some("Fixed the checkout total");
        let share = place
            .run(Some(&Fits("KD-231")))
            .shares
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(share.placed_by, PlacedBy::Rule("repository".to_owned()));
    }

    #[test]
    fn tag_rules_all_run_and_the_first_listed_wins() {
        let mut organization = kestrel();
        let review = Rule {
            name: "review branches".to_owned(),
            when: vec![Condition {
                signal: SignalKey::Branch,
                test: Test::StartsWith("review/".to_owned()),
            }],
            then: Action::SetTags(tags(&["purpose=code-review", "team=platform"])),
        };
        let fallback = Rule {
            name: "everything".to_owned(),
            when: Vec::new(),
            then: Action::SetTags(tags(&["purpose=product", "client=house"])),
        };
        organization.rules.push(review);
        organization.rules.push(fallback);
        organization
            .directory
            .people
            .get_mut(&value("mara"))
            .unwrap()
            .rules = vec![Rule {
            name: "mine".to_owned(),
            when: Vec::new(),
            then: Action::SetTags(tags(&["purpose=learning", "client=mine"])),
        }];
        let mut place = Place::new(&organization);
        place.signals.branch = Some("review/KD-214".to_owned());
        let share = place.one();
        assert_eq!(share.tags.get_str("purpose"), Some(&value("code-review")));
        // A rule's tag wins over an inherited one.
        assert_eq!(share.tags.get_str("team"), Some(&value("platform")));
        assert_eq!(share.tags.get_str("client"), Some(&value("house")));
        // Named tags win over rules.
        place.named = tags(&["purpose=research"]);
        assert_eq!(
            place.one().tags.get_str("purpose"),
            Some(&value("research"))
        );
    }

    #[test]
    fn rules_for_other_tags_can_test_the_placed_ticket() {
        let mut organization = kestrel();
        organization.rules.push(Rule {
            name: "ops tickets".to_owned(),
            when: vec![Condition {
                signal: SignalKey::Ticket,
                test: Test::StartsWith("OPS-".to_owned()),
            }],
            then: Action::SetTags(tags(&["purpose=operations"])),
        });
        let mut place = Place::new(&organization);
        place.signals.branch = Some("OPS-12-rotate".to_owned());
        let share = place.one();
        assert_eq!(share.tags.get_str("purpose"), Some(&value("operations")));
        assert_eq!(share.tags.get(&TagName::project()), Some(&value("ops")));
        place.signals.branch = Some("KD-214".to_owned());
        assert_eq!(place.one().tags.get_str("purpose"), None);
    }

    #[test]
    fn a_rule_can_set_a_ticket_or_a_project_directly() {
        let mut organization = kestrel();
        organization.rules.insert(
            0,
            Rule {
                name: "dependency updates".to_owned(),
                when: vec![Condition {
                    signal: SignalKey::Branch,
                    test: Test::StartsWith("deps/".to_owned()),
                }],
                then: Action::SetTags(tags(&["project=ops", "purpose=maintenance"])),
            },
        );
        organization.rules.insert(
            0,
            Rule {
                name: "on call".to_owned(),
                when: vec![Condition {
                    signal: SignalKey::Branch,
                    test: Test::Is("hotfix".to_owned()),
                }],
                then: Action::SetTags(tags(&["ticket=OPS-1"])),
            },
        );
        let mut place = Place::new(&organization);
        place.signals.branch = Some("deps/bump".to_owned());
        place.signals.repository = Some("kestrel/storefront".to_owned());
        let share = place.one();
        assert_eq!(share.tags.get(&TagName::project()), Some(&value("ops")));
        assert_eq!(share.tags.get(&TagName::ticket()), None);
        assert_eq!(share.tags.get_str("purpose"), Some(&value("maintenance")));
        assert_eq!(share.placed_by.to_string(), "rule: dependency updates");
        place.signals.branch = Some("hotfix".to_owned());
        let share = place.one();
        assert_eq!(share.tags.get(&TagName::ticket()), Some(&value("OPS-1")));
        assert_eq!(share.tags.get(&TagName::project()), Some(&value("ops")));
        assert_eq!(share.placed_by.to_string(), "rule: on call");
    }

    #[test]
    fn unlisted_tag_names_are_dropped_and_reported() {
        let organization = kestrel();
        let mut place = Place::new(&organization);
        place.named = tags(&["team=data", "mood=happy"]);
        place.configured = tags(&["colour=blue", "mood=calm"]);
        let placement = place.run(None);
        let share = placement.shares.first().unwrap();
        assert_eq!(share.tags, tags(&["person=mara", "team=data"]));
        assert_eq!(placement.dropped, vec![name("colour"), name("mood")]);
    }

    #[test]
    fn the_person_tag_is_always_the_agent_manager() {
        let organization = kestrel();
        let mut place = Place::new(&organization);
        place.named = tags(&["person=someone-else"]);
        assert_eq!(
            place.one().tags.get(&TagName::person()),
            Some(&value("mara"))
        );
        let nobody = organization.place(
            &PlacementRequest {
                person: None,
                named: &tags(&["person=someone-else"]),
                configured: &Tags::new(),
                signals: &Signals::default(),
                summary: None,
            },
            None,
        );
        assert_eq!(nobody.shares.first().unwrap().tags, Tags::new());
    }

    #[test]
    fn placements_read_and_write_as_json() {
        let organization = kestrel();
        let mut place = Place::new(&organization);
        place.signals.branch = Some("KD-214-fix".to_owned());
        let placement = place.run(None);
        let json = serde_json::to_string(&placement).unwrap();
        assert!(json.contains(r#""placed_by":{"by":"rule","rule":"ticket key in branch"}"#));
        assert_eq!(serde_json::from_str::<Placement>(&json).unwrap(), placement);
    }
}
