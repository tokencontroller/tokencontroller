//! Token entries, their segments, and the splits they turn into.
//!
//! A token entry is one agent session's account of what it spent. Cost is
//! measured per message (ADR 0006): a session that changes branch or
//! working directory is cut into segments where it changed, and each segment
//! is placed on its own signals, so the split by segment is exact. A person
//! can instead split a token entry by hand, such as 60 and 40 percent; those
//! shares are a judgment. Every split carries its own copy of every tag
//! (ADR 0008).

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::message::{Message, SessionId, Signals, Usage, dedupe};
use crate::money::MoneyError;
use crate::organization::Organization;
use crate::period::Period;
use crate::placement::{PlacedBy, PlacedShare, PlacementRequest, SummaryMatcher};
use crate::price::{PriceTable, Valuation};
use crate::status::{Arrival, Status};
use crate::tags::{TagName, TagValue, Tags};

/// A stretch of a session on one branch and one working directory. A
/// subagent's calls form their own segment, placed on the subagent's own
/// signals when it has them, otherwise following the session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    pub signals: Signals,
    /// The usage of each message in the segment, or the token entry's
    /// totals when it arrived without messages.
    pub usage: Vec<Usage>,
}

/// One share of a token entry split by hand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandShare {
    /// The share's weight, such as 60 for 60 percent.
    pub weight: u64,
    /// The tags of this share, such as `ticket: ACME-50`. They win over the
    /// token entry's own.
    pub tags: Tags,
}

/// One agent session's account of what it spent and on what work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenEntry {
    /// The product the agent ran on, such as Claude Code.
    pub agent_harness: String,
    /// An optional label, such as "review bot".
    #[serde(default)]
    pub agent_name: Option<String>,
    /// Who answers for the cost. A token entry without one shows as an
    /// error until someone takes it.
    pub agent_manager: Option<TagValue>,
    #[serde(default)]
    pub session_id: Option<SessionId>,
    #[serde(with = "time::serde::rfc3339")]
    pub started_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub ended_at: OffsetDateTime,
    #[serde(default)]
    pub summary: Option<String>,
    pub arrival: Arrival,
    pub status: Status,
    /// Tags named by hand or sent with the token entry, including a ticket
    /// or project named in it.
    #[serde(default)]
    pub tags: Tags,
    /// Tags from the repository's config file or the agent's telemetry
    /// resource attributes.
    #[serde(default)]
    pub configured: Tags,
    /// The session's signals.
    #[serde(default)]
    pub signals: Signals,
    pub segments: Vec<Segment>,
    /// The shares of a split by hand; empty unless a person split it.
    #[serde(default)]
    pub hand_split: Vec<HandShare>,
}

/// A share of a token entry with its own tags.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Split {
    pub tags: Tags,
    pub value: Valuation,
    pub placed_by: PlacedBy,
    /// Whether its ticket is not known yet.
    pub ticket_not_synced: bool,
}

/// A token entry turned into splits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Splits {
    pub splits: Vec<Split>,
    /// Tag names that were sent or set but are not on the organization's
    /// list.
    pub dropped: Vec<TagName>,
}

impl TokenEntry {
    /// Cuts a session's messages into segments. Reports of the same message
    /// are joined first (see [`dedupe`]). Messages are taken in time order;
    /// a message without a branch or working directory keeps the session's.
    /// Segments are in the order the session first reached them, and
    /// returning to a branch adds to its segment.
    #[must_use]
    pub fn segments_from_messages(signals: &Signals, messages: Vec<Message>) -> Vec<Segment> {
        let mut messages = dedupe(messages);
        messages.sort_by_key(|message| message.at);
        let mut segments: Vec<(SegmentKey, Segment)> = Vec::new();
        for message in messages {
            let key = SegmentKey {
                branch: message
                    .signals
                    .branch
                    .clone()
                    .or_else(|| signals.branch.clone()),
                working_directory: message
                    .signals
                    .working_directory
                    .clone()
                    .or_else(|| signals.working_directory.clone()),
                subagent_type: message.signals.subagent_type.clone(),
            };
            let index =
                if let Some(index) = segments.iter().position(|(existing, _)| *existing == key) {
                    index
                } else {
                    let mut segment_signals = signals.clone();
                    segment_signals.branch.clone_from(&key.branch);
                    segment_signals
                        .working_directory
                        .clone_from(&key.working_directory);
                    if key.subagent_type.is_some() {
                        segment_signals.subagent_type.clone_from(&key.subagent_type);
                    }
                    segments.push((
                        key,
                        Segment {
                            signals: segment_signals,
                            usage: Vec::new(),
                        },
                    ));
                    segments.len().saturating_sub(1)
                };
            if let Some((_, segment)) = segments.get_mut(index) {
                if let Some(repository) = &message.signals.repository {
                    segment
                        .signals
                        .repository
                        .get_or_insert_with(|| repository.clone());
                }
                if let Some(pull_request) = message.signals.pull_request {
                    segment.signals.pull_request.get_or_insert(pull_request);
                }
                segment
                    .signals
                    .prompt_ticket_keys
                    .extend(message.signals.prompt_ticket_keys);
                segment.usage.push(message.usage);
            }
        }
        segments.into_iter().map(|(_, segment)| segment).collect()
    }

    /// The period the token entry belongs to: the month it started in, in
    /// UTC.
    #[must_use]
    pub fn period(&self) -> Period {
        Period::containing(self.started_at)
    }

    /// What the whole token entry is worth.
    ///
    /// # Errors
    ///
    /// As [`PriceTable::value`].
    pub fn value(&self, prices: &PriceTable) -> Result<Valuation, MoneyError> {
        prices.value_all(self.segments.iter().flat_map(|segment| &segment.usage))
    }

    /// Places the token entry and divides its value into splits: one per
    /// segment, or one per share of a split by hand. Splits that end up with
    /// the same tags are joined. The splits always add up to
    /// [`TokenEntry::value`].
    ///
    /// # Errors
    ///
    /// As [`PriceTable::value`] and [`Valuation::allocate`].
    pub fn split(
        &self,
        organization: &Organization,
        prices: &PriceTable,
        matcher: Option<&dyn SummaryMatcher>,
    ) -> Result<Splits, MoneyError> {
        let mut dropped = Vec::new();
        let mut place = |named: &Tags, signals: &Signals| {
            let placement = organization.place(
                &PlacementRequest {
                    person: self.agent_manager.as_ref(),
                    named,
                    configured: &self.configured,
                    signals,
                    summary: self.summary.as_deref(),
                },
                matcher,
            );
            dropped.extend(placement.dropped);
            placement.shares
        };

        let mut pieces: Vec<(Vec<PlacedShare>, Valuation)> = Vec::new();
        if self.hand_split.is_empty() {
            let placed: Vec<Vec<PlacedShare>> = self
                .segments
                .iter()
                .map(|segment| place(&self.tags, &segment.signals))
                .collect();
            for (index, segment) in self.segments.iter().enumerate() {
                let shares = self
                    .followed_session(index, &placed)
                    .or_else(|| placed.get(index))
                    .cloned()
                    .unwrap_or_default();
                pieces.push((shares, prices.value_all(&segment.usage)?));
            }
            if self.segments.is_empty() {
                pieces.push((
                    place(&self.tags, &self.signals),
                    Valuation::zero(prices.currency()),
                ));
            }
        } else {
            let signals = self
                .segments
                .first()
                .map_or(&self.signals, |segment| &segment.signals);
            let weights: Vec<u64> = self.hand_split.iter().map(|share| share.weight).collect();
            let values = self.value(prices)?.allocate(&weights)?;
            for (share, value) in self.hand_split.iter().zip(values) {
                pieces.push((place(&share.tags.over(&self.tags), signals), value));
            }
        }

        let mut splits: Vec<Split> = Vec::new();
        for (shares, value) in pieces {
            let weights: Vec<u64> = shares.iter().map(|share| share.weight).collect();
            for (share, value) in shares.into_iter().zip(value.allocate(&weights)?) {
                // Equal tags mean an equal ticket, so the splits agree on
                // whether it is synced.
                if let Some(split) = splits.iter_mut().find(|split| split.tags == share.tags) {
                    split.value = split.value.checked_add(&value)?;
                } else {
                    splits.push(Split {
                        tags: share.tags,
                        value,
                        placed_by: share.placed_by,
                        ticket_not_synced: share.ticket_not_synced,
                    });
                }
            }
        }
        dropped.sort();
        dropped.dedup();
        Ok(Splits { splits, dropped })
    }

    /// For a subagent's segment that its own signals placed nowhere, the
    /// placement of the session segment on the same branch and working
    /// directory.
    fn followed_session<'a>(
        &self,
        index: usize,
        placed: &'a [Vec<PlacedShare>],
    ) -> Option<&'a Vec<PlacedShare>> {
        let segment = self.segments.get(index)?;
        segment.signals.subagent_type.as_ref()?;
        let own = placed.get(index)?;
        if own.iter().any(|share| share.placed_by != PlacedBy::Nothing) {
            return None;
        }
        self.segments
            .iter()
            .zip(placed)
            .find(|(session, _)| {
                session.signals.subagent_type.is_none()
                    && session.signals.branch == segment.signals.branch
                    && session.signals.working_directory == segment.signals.working_directory
            })
            .map(|(_, shares)| shares)
    }
}

/// What a message's segment is decided by.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SegmentKey {
    branch: Option<String>,
    working_directory: Option<String>,
    subagent_type: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::directory::{Project, Ticket};
    use crate::message::{MessageId, MessageSignals, ModelRef};
    use crate::money::{Currency, Money};
    use crate::pattern::TicketKeyPattern;
    use crate::price::ModelPrice;
    use crate::rules::{Action, Condition, Rule, SeveralKeys, SignalKey, Test};
    use crate::status::CostSource;
    use crate::tokens::TokenCounts;
    use proptest::prelude::*;
    use time::Duration;
    use time::macros::datetime;

    fn value(text: &str) -> TagValue {
        TagValue::new(text).unwrap()
    }

    fn tags(pairs: &[&str]) -> Tags {
        Tags::parse(pairs.iter().copied()).unwrap()
    }

    fn usd(nanos: i64) -> Money {
        Money::from_nanos(nanos, Currency::USD)
    }

    fn sonnet() -> ModelRef {
        ModelRef::new("anthropic", "claude-sonnet-4-5")
    }

    fn prices() -> PriceTable {
        let mut table = PriceTable::new(Currency::USD);
        let per_million = Money::parse("1", Currency::USD).unwrap();
        table
            .insert(
                sonnet(),
                ModelPrice {
                    input: per_million,
                    output: per_million,
                    cache_write: per_million,
                    cache_read: per_million,
                },
            )
            .unwrap();
        table
    }

    fn brightline() -> Organization {
        let mut organization = Organization::new(Currency::USD, CostSource::ApiKey);
        for name in ["team", "client", "purpose"] {
            organization.tag_names.insert(TagName::new(name).unwrap());
        }
        organization.directory.people.insert(
            value("lena"),
            crate::directory::Person {
                tags: tags(&["team=data"]),
                rules: Vec::new(),
            },
        );
        for (project, client, pattern) in [
            ("acme-shop", "acme", r"^ACME-\d+"),
            ("globex-app", "globex", r"^GLOBEX-\d+"),
        ] {
            organization.directory.projects.insert(
                value(project),
                Project {
                    tags: tags(&[&format!("client={client}")]),
                    ticket_key_pattern: Some(TicketKeyPattern::new(pattern).unwrap()),
                    repositories: Vec::new(),
                },
            );
        }
        for (ticket, project) in [("ACME-50", "acme-shop"), ("GLOBEX-7", "globex-app")] {
            organization.directory.tickets.insert(
                value(ticket),
                Ticket {
                    project: Some(value(project)),
                    ..Ticket::default()
                },
            );
        }
        organization
    }

    fn message(id: &str, minute: i64, branch: Option<&str>, tokens: u64) -> Message {
        Message {
            session_id: SessionId::new("s1").unwrap(),
            message_id: MessageId::new(id).unwrap(),
            request_id: None,
            at: datetime!(2026-10-14 09:00 UTC)
                .checked_add(Duration::minutes(minute))
                .unwrap(),
            usage: Usage {
                model: sonnet(),
                tokens: TokenCounts {
                    input: tokens,
                    ..TokenCounts::default()
                },
                reported_cost: None,
                billed_cost: None,
            },
            signals: MessageSignals {
                branch: branch.map(str::to_owned),
                ..MessageSignals::default()
            },
        }
    }

    fn entry(segments: Vec<Segment>) -> TokenEntry {
        TokenEntry {
            agent_harness: "Claude Code".to_owned(),
            agent_name: None,
            agent_manager: Some(value("lena")),
            session_id: Some(SessionId::new("s1").unwrap()),
            started_at: datetime!(2026-10-14 09:00 UTC),
            ended_at: datetime!(2026-10-14 11:00 UTC),
            summary: None,
            arrival: Arrival::TokenSheet,
            status: Status::Submitted,
            tags: Tags::new(),
            configured: Tags::new(),
            signals: Signals::default(),
            segments,
            hand_split: Vec::new(),
        }
    }

    fn split_lists(splits: &Splits) -> Vec<(String, i64)> {
        splits
            .splits
            .iter()
            .map(|split| {
                (
                    split
                        .tags
                        .get(&TagName::ticket())
                        .map_or_else(String::new, |ticket| ticket.as_str().to_owned()),
                    split.value.list.nanos(),
                )
            })
            .collect()
    }

    #[test]
    fn a_session_on_two_branches_splits_exactly_by_message() {
        let messages = vec![
            message("m1", 0, Some("ACME-50-total"), 1_000_000),
            message("m3", 20, Some("GLOBEX-7-login"), 3_000_000),
            message("m2", 10, Some("ACME-50-total"), 2_000_000),
            message("m4", 30, Some("ACME-50-total"), 4_000_000),
        ];
        let segments = TokenEntry::segments_from_messages(&Signals::default(), messages);
        assert_eq!(segments.len(), 2);
        let entry = entry(segments);
        let splits = entry.split(&brightline(), &prices(), None).unwrap();
        assert_eq!(
            split_lists(&splits),
            vec![
                ("ACME-50".to_owned(), 7_000_000_000),
                ("GLOBEX-7".to_owned(), 3_000_000_000)
            ]
        );
        let acme = splits.splits.first().unwrap();
        assert_eq!(
            acme.tags,
            tags(&[
                "person=lena",
                "team=data",
                "ticket=ACME-50",
                "project=acme-shop",
                "client=acme"
            ])
        );
        assert_eq!(entry.value(&prices()).unwrap().list, usd(10_000_000_000));
        assert_eq!(entry.period().to_string(), "2026-10");
    }

    #[test]
    fn a_message_seen_twice_is_counted_once_and_keeps_the_sessions_branch() {
        let telemetry = message("m1", 0, None, 1_000_000);
        let session_file = message("m1", 0, Some("ACME-50-total"), 1_000_000);
        let only_telemetry = message("m2", 5, None, 2_000_000);
        let session = Signals {
            branch: Some("GLOBEX-7-login".to_owned()),
            ..Signals::default()
        };
        let segments = TokenEntry::segments_from_messages(
            &session,
            vec![telemetry, only_telemetry, session_file],
        );
        let splits = entry(segments)
            .split(&brightline(), &prices(), None)
            .unwrap();
        assert_eq!(
            split_lists(&splits),
            vec![
                ("ACME-50".to_owned(), 1_000_000_000),
                ("GLOBEX-7".to_owned(), 2_000_000_000)
            ]
        );
    }

    #[test]
    fn segments_carry_their_messages_repository_pull_request_and_prompt_keys() {
        let mut first = message("m1", 0, Some("main"), 1);
        first.signals.repository = Some("acme/shop".to_owned());
        first.signals.pull_request = Some(12);
        first.signals.prompt_ticket_keys = vec!["ACME-50".to_owned()];
        let mut second = message("m2", 1, Some("main"), 1);
        second.signals.repository = Some("acme/other".to_owned());
        second.signals.pull_request = Some(13);
        second.signals.prompt_ticket_keys = vec!["GLOBEX-7".to_owned()];
        let session = Signals {
            working_directory: Some("/work".to_owned()),
            prompt_ticket_keys: vec!["ACME-1".to_owned()],
            ..Signals::default()
        };
        let segments = TokenEntry::segments_from_messages(&session, vec![first, second]);
        let segment = segments.first().unwrap();
        assert_eq!(segments.len(), 1);
        assert_eq!(segment.signals.repository.as_deref(), Some("acme/shop"));
        assert_eq!(segment.signals.pull_request, Some(12));
        assert_eq!(segment.signals.working_directory.as_deref(), Some("/work"));
        assert_eq!(
            segment.signals.prompt_ticket_keys,
            vec!["ACME-1", "ACME-50", "GLOBEX-7"]
        );
        assert_eq!(segment.usage.len(), 2);
    }

    #[test]
    fn a_subagent_without_signals_of_its_own_follows_the_session() {
        let mut organization = brightline();
        organization.rules.push(Rule {
            name: "reviews".to_owned(),
            when: vec![Condition {
                signal: SignalKey::SubagentType,
                test: Test::Is("code-reviewer".to_owned()),
            }],
            then: Action::SetTags(tags(&["purpose=code-review"])),
        });
        let session = Signals {
            prompt_ticket_keys: vec!["ACME-50".to_owned()],
            ..Signals::default()
        };
        let mut helper = message("m2", 1, Some("main"), 2_000_000);
        helper.signals.subagent_type = Some("explorer".to_owned());
        let mut reviewer = message("m3", 2, Some("main"), 4_000_000);
        reviewer.signals.subagent_type = Some("code-reviewer".to_owned());
        let segments = TokenEntry::segments_from_messages(
            &session,
            vec![message("m1", 0, Some("main"), 1_000_000), helper, reviewer],
        );
        assert_eq!(segments.len(), 3);
        let splits = entry(segments)
            .split(&organization, &prices(), None)
            .unwrap();
        // The explorer has the same tags as the session, so they join.
        assert_eq!(
            split_lists(&splits),
            vec![
                ("ACME-50".to_owned(), 3_000_000_000),
                ("ACME-50".to_owned(), 4_000_000_000)
            ]
        );
        let review = splits.splits.get(1).unwrap();
        assert_eq!(review.tags.get_str("purpose"), Some(&value("code-review")));

        // A subagent placed nowhere takes the session's placement.
        let mut lost = message("m5", 1, Some("main"), 2_000_000);
        lost.signals.subagent_type = Some("explorer".to_owned());
        let mut segments = TokenEntry::segments_from_messages(
            &Signals::default(),
            vec![message("m4", 0, Some("main"), 1_000_000), lost],
        );
        if let Some(main) = segments.first_mut() {
            main.signals.prompt_ticket_keys = vec!["GLOBEX-7".to_owned()];
        }
        let splits = entry(segments)
            .split(&organization, &prices(), None)
            .unwrap();
        assert_eq!(
            split_lists(&splits),
            vec![("GLOBEX-7".to_owned(), 3_000_000_000)]
        );

        // Without a session segment on its branch and working directory to
        // follow, it stays where it is.
        for (branch, directory) in [("other", "/work"), ("GLOBEX-7-x", "/elsewhere")] {
            let mut alone = message("m6", 1, Some(branch), 2_000_000);
            alone.signals.subagent_type = Some("explorer".to_owned());
            alone.signals.working_directory = Some(directory.to_owned());
            let mut main = message("m7", 0, Some("GLOBEX-7-x"), 1_000_000);
            main.signals.working_directory = Some("/work".to_owned());
            let segments =
                TokenEntry::segments_from_messages(&Signals::default(), vec![main, alone]);
            let splits = entry(segments)
                .split(&organization, &prices(), None)
                .unwrap();
            let expected = if branch == "other" {
                vec![
                    ("GLOBEX-7".to_owned(), 1_000_000_000),
                    (String::new(), 2_000_000_000),
                ]
            } else {
                // Its own branch places it.
                vec![("GLOBEX-7".to_owned(), 3_000_000_000)]
            };
            assert_eq!(split_lists(&splits), expected);
        }
        let mut alone = message("m6", 1, Some("main"), 2_000_000);
        alone.signals.subagent_type = Some("explorer".to_owned());
        alone.signals.working_directory = Some("/elsewhere".to_owned());
        let mut main = message("m7", 0, Some("main"), 1_000_000);
        main.signals.working_directory = Some("/work".to_owned());
        main.signals.prompt_ticket_keys = vec!["GLOBEX-7".to_owned()];
        let segments = TokenEntry::segments_from_messages(&Signals::default(), vec![main, alone]);
        let splits = entry(segments)
            .split(&organization, &prices(), None)
            .unwrap();
        assert_eq!(
            split_lists(&splits),
            vec![
                ("GLOBEX-7".to_owned(), 1_000_000_000),
                (String::new(), 2_000_000_000)
            ]
        );
    }

    #[test]
    fn a_split_by_hand_divides_the_whole_by_its_shares() {
        let segments = TokenEntry::segments_from_messages(
            &Signals::default(),
            vec![
                message("m1", 0, Some("ACME-50-total"), 9_000_000),
                message("m2", 1, Some("GLOBEX-7-login"), 1_000_001),
            ],
        );
        let mut entry = entry(segments);
        entry.tags = tags(&["purpose=product"]);
        entry.hand_split = vec![
            HandShare {
                weight: 60,
                tags: tags(&["ticket=ACME-50"]),
            },
            HandShare {
                weight: 40,
                tags: tags(&["ticket=GLOBEX-7"]),
            },
        ];
        let splits = entry.split(&brightline(), &prices(), None).unwrap();
        // 10.000001 USD: 60 percent is 6.0000006, 40 percent 4.0000004.
        assert_eq!(
            split_lists(&splits),
            vec![
                ("ACME-50".to_owned(), 6_000_000_600),
                ("GLOBEX-7".to_owned(), 4_000_000_400)
            ]
        );
        let globex = splits.splits.get(1).unwrap();
        assert_eq!(
            globex.tags,
            tags(&[
                "person=lena",
                "team=data",
                "ticket=GLOBEX-7",
                "project=globex-app",
                "client=globex",
                "purpose=product"
            ])
        );
        assert_eq!(globex.placed_by, PlacedBy::Named);
    }

    #[test]
    fn prompts_naming_two_tickets_can_split_equally() {
        let mut organization = brightline();
        organization.several_keys = SeveralKeys::EqualSplit;
        let session = Signals {
            prompt_ticket_keys: vec!["ACME-50".to_owned(), "GLOBEX-7".to_owned()],
            ..Signals::default()
        };
        let segments =
            TokenEntry::segments_from_messages(&session, vec![message("m1", 0, None, 3)]);
        let splits = entry(segments)
            .split(&organization, &prices(), None)
            .unwrap();
        assert_eq!(
            split_lists(&splits),
            vec![("ACME-50".to_owned(), 1500), ("GLOBEX-7".to_owned(), 1500)]
        );
    }

    #[test]
    fn an_entry_without_usage_is_one_split_worth_nothing() {
        let mut entry = entry(Vec::new());
        entry.tags = tags(&["ticket=ACME-50", "mood=good"]);
        let splits = entry.split(&brightline(), &prices(), None).unwrap();
        assert_eq!(split_lists(&splits), vec![("ACME-50".to_owned(), 0)]);
        assert_eq!(splits.dropped, vec![TagName::new("mood").unwrap()]);
        assert!(splits.splits.first().unwrap().value.is_complete());
    }

    #[test]
    fn a_ticket_not_synced_yet_is_marked_on_the_split() {
        let segments = TokenEntry::segments_from_messages(
            &Signals::default(),
            vec![
                message("m1", 0, Some("ACME-77-new"), 1),
                message("m2", 1, Some("ACME-77-new-again"), 1),
            ],
        );
        let splits = entry(segments)
            .split(&brightline(), &prices(), None)
            .unwrap();
        assert_eq!(splits.splits.len(), 1);
        let split = splits.splits.first().unwrap();
        assert!(split.ticket_not_synced);
        assert_eq!(split.value.list, usd(2000));
    }

    #[test]
    fn a_split_by_hand_needs_a_weight() {
        let mut entry = entry(Vec::new());
        entry.hand_split = vec![HandShare {
            weight: 0,
            tags: Tags::new(),
        }];
        assert_eq!(
            entry.split(&brightline(), &prices(), None),
            Err(MoneyError::NoWeights)
        );
    }

    #[test]
    fn token_entries_read_and_write_as_json() {
        let segments = TokenEntry::segments_from_messages(
            &Signals::default(),
            vec![message("m1", 0, Some("ACME-50-total"), 1)],
        );
        let entry = entry(segments);
        let json = serde_json::to_string(&entry).unwrap();
        assert_eq!(serde_json::from_str::<TokenEntry>(&json).unwrap(), entry);
    }

    proptest! {
        #[test]
        fn splits_always_add_up_to_the_token_entry(
            messages in prop::collection::vec((0_usize..4, 0_u64..5_000_000, any::<bool>()), 1..30),
            hand in prop::collection::vec(1_u64..100, 0..4),
        ) {
            let branches = ["ACME-50-a", "GLOBEX-7-b", "main", "ACME-9-c"];
            let list: Vec<Message> = messages
                .iter()
                .enumerate()
                .map(|(index, (branch, tokens, subagent))| {
                    let mut message = message(
                        &format!("m{index}"),
                        i64::try_from(index).unwrap(),
                        branches.get(*branch).copied(),
                        *tokens,
                    );
                    if *subagent {
                        message.signals.subagent_type = Some("helper".to_owned());
                    }
                    message
                })
                .collect();
            let mut entry = entry(TokenEntry::segments_from_messages(&Signals::default(), list));
            entry.hand_split = hand
                .iter()
                .map(|weight| HandShare { weight: *weight, tags: Tags::new() })
                .collect();
            let prices = prices();
            let splits = entry.split(&brightline(), &prices, None).unwrap();
            let total = Money::sum(splits.splits.iter().map(|split| split.value.list), Currency::USD).unwrap();
            prop_assert_eq!(total, entry.value(&prices).unwrap().list);
            let mut seen: Vec<&Tags> = Vec::new();
            for split in &splits.splits {
                prop_assert!(!seen.contains(&&split.tags));
                seen.push(&split.tags);
            }
        }
    }
}
