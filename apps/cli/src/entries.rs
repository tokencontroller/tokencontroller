//! Token entries as people see them: a session's model calls added up,
//! split into segments where the branch, working directory or subagent
//! changes, and placed with the rules (Sessions and subagents; ADR 0006).

use std::collections::BTreeMap;
use std::path::Path;

use tokencontroller_core::{Currency, Money};

use crate::context::Context;
use crate::error::Result;
use crate::keys::{KeyPattern, project_of};
use crate::placing::{Named, Pick, Placement, Signals, place};
use crate::store::{EntryRow, Status, Store, StoredMessage};
use crate::tokens::Tokens;

/// List value is computed in US dollars, the currency of the price list.
pub const CURRENCY: Currency = Currency::USD;

/// What `TC_TICKET`, `TC_PROJECT` and `TC_TAGS` name for every entry in this
/// run, as the GitHub Action sets them.
#[derive(Debug, Clone, Default)]
pub struct RunTags(pub BTreeMap<String, String>);

impl RunTags {
    pub fn from_env() -> Self {
        let mut tags = BTreeMap::new();
        if let Ok(list) = std::env::var("TC_TAGS") {
            tags.extend(parse_tag_list(&list));
        }
        for (variable, name) in [("TC_PROJECT", "project"), ("TC_TICKET", "ticket")] {
            if let Ok(value) = std::env::var(variable)
                && !value.trim().is_empty()
            {
                tags.insert(name.to_owned(), value.trim().to_owned());
            }
        }
        Self(tags)
    }
}

/// `team=web,purpose=code-review` → tags.
pub fn parse_tag_list(list: &str) -> BTreeMap<String, String> {
    list.split([',', ' ', '\n'])
        .filter_map(|pair| pair.split_once('='))
        .map(|(name, value)| (name.trim().to_owned(), value.trim().to_owned()))
        .filter(|(name, value)| !name.is_empty() && !value.is_empty())
        .collect()
}

/// Value in nano-units of [`CURRENCY`], and the models it could not price.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Value {
    pub nanos: i64,
    pub unknown_models: Vec<String>,
}

impl Value {
    fn add(&mut self, other: &Self) {
        self.nanos = self.nanos.saturating_add(other.nanos);
        for model in &other.unknown_models {
            if !self.unknown_models.contains(model) {
                self.unknown_models.push(model.clone());
            }
        }
    }

    pub fn is_known(&self) -> bool {
        self.unknown_models.is_empty()
    }
}

/// The value of one model call: its list value, or the agent's own figure
/// when the model is not on the price list.
pub fn message_value(message: &StoredMessage) -> Value {
    match crate::prices::list_value(&message.model, &message.tokens).or(message.reported_cost) {
        Some(nanos) => Value {
            nanos,
            unknown_models: Vec::new(),
        },
        None => Value {
            nanos: 0,
            unknown_models: vec![if message.model.is_empty() {
                "unknown".to_owned()
            } else {
                message.model.clone()
            }],
        },
    }
}

/// One share of a token entry with its own tags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Split {
    pub tags: BTreeMap<String, String>,
    pub share_percent: u64,
    pub value_nanos: i64,
    /// The split's tokens: exact for a segment, divided by share otherwise.
    pub tokens: Tokens,
    /// The model calls of a segment, so the split is exact; `None` for a
    /// share set by hand or an equal split.
    pub message_ids: Option<Vec<String>>,
    pub signals: Signals,
}

/// A token entry with everything added up and placed.
#[derive(Debug, Clone)]
pub struct Entry {
    pub row: EntryRow,
    pub model: String,
    pub started_ms: i64,
    pub ended_ms: i64,
    pub tokens: Tokens,
    pub value: Value,
    pub messages: Vec<StoredMessage>,
    pub splits: Vec<Split>,
    pub placed_by: String,
    pub candidates: Vec<String>,
    pub summary: Option<String>,
    /// Whether the classifier may decide this entry's ticket.
    pub asks_classifier: bool,
}

impl Entry {
    /// The ticket of the entry when it has one, or of its largest split.
    pub fn ticket(&self) -> Option<&str> {
        self.splits
            .iter()
            .max_by_key(|split| split.share_percent)
            .and_then(|split| split.tags.get("ticket"))
            .map(String::as_str)
    }

    /// The signals of the largest segment, sent with the entry.
    pub fn main_signals(&self) -> Signals {
        self.splits
            .iter()
            .max_by_key(|split| split.share_percent)
            .map(|split| split.signals.clone())
            .unwrap_or_default()
    }

    /// The value on tickets, in nano-units.
    pub fn on_tickets_nanos(&self) -> i64 {
        self.splits
            .iter()
            .filter(|split| split.tags.contains_key("ticket"))
            .fold(0_i64, |sum, split| sum.saturating_add(split.value_nanos))
    }
}

/// Everything placing needs besides the entry itself.
pub struct Placing<'a> {
    pub context: &'a Context,
    pub patterns: &'a [KeyPattern],
    pub run_tags: &'a RunTags,
}

/// Builds the entries with the given statuses.
pub fn load(store: &Store, statuses: &[Status], placing: &Placing<'_>) -> Result<Vec<Entry>> {
    store
        .entries(statuses)?
        .into_iter()
        .map(|row| build(store, row, placing))
        .collect()
}

/// Builds one entry.
pub fn build(store: &Store, row: EntryRow, placing: &Placing<'_>) -> Result<Entry> {
    let messages = store.messages(row.harness, &row.session_id)?;
    let keys = store.keys(row.harness, &row.session_id)?;
    let title = store.title(row.harness, &row.session_id)?;

    let mut named: Named = placing.run_tags.0.clone();
    named.extend(row.named_tags.clone());
    let pick = row.pick.as_deref().map(|pick| {
        if pick == "none" {
            Pick::Nothing
        } else {
            Pick::Ticket(pick.to_owned())
        }
    });
    let segments = segments_of(&row, &messages, &keys);

    let mut value = Value::default();
    let mut tokens = Tokens::default();
    for message in &messages {
        value.add(&message_value(message));
        tokens = tokens.plus(message.tokens);
    }

    let placements: Vec<Placement> = segments
        .iter()
        .map(|(signals, _, _)| {
            place(
                signals,
                &named,
                placing.context,
                placing.patterns,
                pick.as_ref(),
            )
        })
        .collect();
    let asks_classifier = placements
        .iter()
        .any(|placement| placement.ticket_only_mentioned || !placement.tags.contains_key("ticket"));

    let mut splits = if let Some(hand) = &row.hand_split {
        shares(
            hand,
            placements.first(),
            segments.first().map(|s| &s.0),
            placing.patterns,
            &value,
            tokens,
        )?
    } else if let ([placement], [(signals, _, _)]) = (placements.as_slice(), segments.as_slice())
        && placement.equal_split.len() > 1
    {
        let equal: Vec<(String, u64)> = placement
            .equal_split
            .iter()
            .map(|ticket| (ticket.clone(), 1))
            .collect();
        shares(
            &equal,
            Some(placement),
            Some(signals),
            placing.patterns,
            &value,
            tokens,
        )?
    } else {
        by_segment(&segments, &placements, &messages)
    };
    set_percents(&mut splits)?;

    let candidates = candidates_of(&segments, &splits, placing.patterns);

    let placed_by = if row.hand_split.is_some() {
        "hand".to_owned()
    } else {
        placements
            .iter()
            .map(|placement| placement.placed_by.clone())
            .next()
            .unwrap_or_else(|| "none".to_owned())
    };
    let model = main_model(&messages);
    let summary = row.summary.clone().or(title);
    Ok(Entry {
        started_ms: messages.first().map_or(0, |m| m.at_ms),
        ended_ms: messages.iter().map(|m| m.at_ms).max().unwrap_or(0),
        row,
        model,
        tokens,
        value,
        messages,
        splits,
        placed_by,
        candidates,
        summary,
        asks_classifier,
    })
}

/// A segment: its signals, the positions of its model calls, its value.
type Segment = (Signals, Vec<usize>, Value);

/// Runs of one branch, working directory and subagent.
fn segments_of(
    row: &EntryRow,
    messages: &[StoredMessage],
    keys: &[crate::placing::KeyCount],
) -> Vec<Segment> {
    let pull_request_title_keys: Vec<String> = row
        .signals
        .get("pull_request_title_keys")
        .map(|keys| {
            keys.split(',')
                .map(str::to_owned)
                .filter(|k| !k.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let mut segments: Vec<Segment> = Vec::new();
    for (index, message) in messages.iter().enumerate() {
        let position = segments.iter().position(|(signals, _, _)| {
            signals.branch == message.branch
                && signals.directory == message.directory
                && signals.subagent == message.subagent
        });
        let value = message_value(message);
        if let Some(segment) = position.and_then(|position| segments.get_mut(position)) {
            segment.1.push(index);
            segment.2.add(&value);
        } else {
            let config_file_tags = message
                .directory
                .as_deref()
                .map(|directory| crate::repo::config_tags(Path::new(directory)))
                .unwrap_or_default();
            let signals = Signals {
                agent_harness: row.harness.name().to_owned(),
                branch: message.branch.clone(),
                directory: message.directory.clone(),
                repository: message.repository.clone(),
                pull_request: row.signals.get("pull_request").cloned(),
                subagent: message.subagent.clone(),
                pull_request_title_keys: pull_request_title_keys.clone(),
                prompt_keys: keys.to_vec(),
                config_file_tags,
            };
            segments.push((signals, vec![index], value));
        }
    }
    segments
}

/// Keys found in the session that no split landed on.
fn candidates_of(segments: &[Segment], splits: &[Split], patterns: &[KeyPattern]) -> Vec<String> {
    let all_keys: Vec<String> = segments
        .first()
        .map(|(signals, _, _)| signals.all_keys(patterns))
        .unwrap_or_default();
    let chosen: Vec<&str> = splits
        .iter()
        .filter_map(|split| split.tags.get("ticket").map(String::as_str))
        .collect();
    all_keys
        .into_iter()
        .filter(|key| !chosen.contains(&key.as_str()))
        .collect()
}

/// One split per segment; segments that landed on the same tags become one.
fn by_segment(
    segments: &[Segment],
    placements: &[Placement],
    messages: &[StoredMessage],
) -> Vec<Split> {
    let mut splits: Vec<Split> = Vec::new();
    for ((signals, indexes, value), placement) in segments.iter().zip(placements) {
        let ids: Vec<String> = indexes
            .iter()
            .filter_map(|index| messages.get(*index))
            .map(|message| message.message_id.clone())
            .collect();
        let tokens = indexes
            .iter()
            .filter_map(|index| messages.get(*index))
            .fold(Tokens::default(), |sum, message| sum.plus(message.tokens));
        if let Some(split) = splits.iter_mut().find(|split| split.tags == placement.tags) {
            split.value_nanos = split.value_nanos.saturating_add(value.nanos);
            split.tokens = split.tokens.plus(tokens);
            if let Some(existing) = split.message_ids.as_mut() {
                existing.extend(ids);
            }
        } else {
            splits.push(Split {
                tags: placement.tags.clone(),
                share_percent: 0,
                value_nanos: value.nanos,
                tokens,
                message_ids: Some(ids),
                signals: signals.clone(),
            });
        }
    }
    splits
}

/// Splits by share: the tags of the whole entry, with each share's ticket
/// and that ticket's project.
fn shares(
    shares: &[(String, u64)],
    placement: Option<&Placement>,
    signals: Option<&Signals>,
    patterns: &[KeyPattern],
    value: &Value,
    tokens: Tokens,
) -> Result<Vec<Split>> {
    let weights: Vec<u64> = shares.iter().map(|(_, weight)| *weight).collect();
    let values = Money::from_nanos(value.nanos, CURRENCY).allocate(&weights)?;
    let token_parts = divide_tokens(tokens, &weights)?;
    Ok(shares
        .iter()
        .zip(values)
        .zip(token_parts)
        .map(|(((ticket, _), part), tokens)| {
            let mut tags = placement.map(|p| p.tags.clone()).unwrap_or_default();
            tags.insert("ticket".to_owned(), ticket.clone());
            match project_of(patterns, ticket) {
                Some(project) => {
                    tags.insert("project".to_owned(), project);
                }
                None => {
                    tags.remove("project");
                }
            }
            Split {
                tags,
                share_percent: 0,
                value_nanos: part.nanos(),
                tokens,
                message_ids: None,
                signals: signals.cloned().unwrap_or_default(),
            }
        })
        .collect())
}

/// Divides token counts by weight with the largest-remainder method, so the
/// parts add up to the whole.
fn divide_tokens(tokens: Tokens, weights: &[u64]) -> Result<Vec<Tokens>> {
    let divide = |count: u64| -> Result<Vec<u64>> {
        let count = i64::try_from(count).unwrap_or(i64::MAX);
        Ok(Money::from_nanos(count, CURRENCY)
            .allocate(weights)?
            .into_iter()
            .map(|part| u64::try_from(part.nanos()).unwrap_or(0))
            .collect())
    };
    let input = divide(tokens.input)?;
    let output = divide(tokens.output)?;
    let short_writes = divide(tokens.cache_write_5m)?;
    let long_writes = divide(tokens.cache_write_1h)?;
    let read = divide(tokens.cache_read)?;
    Ok((0..weights.len())
        .map(|index| Tokens {
            input: input.get(index).copied().unwrap_or(0),
            output: output.get(index).copied().unwrap_or(0),
            cache_write_5m: short_writes.get(index).copied().unwrap_or(0),
            cache_write_1h: long_writes.get(index).copied().unwrap_or(0),
            cache_read: read.get(index).copied().unwrap_or(0),
        })
        .collect())
}

/// Each split's share of the entry in whole percent, adding up to 100.
fn set_percents(splits: &mut [Split]) -> Result<()> {
    let weights: Vec<u64> = splits
        .iter()
        .map(|split| u64::try_from(split.value_nanos).unwrap_or(0))
        .collect();
    let weights = if weights.iter().all(|weight| *weight == 0) {
        vec![1; splits.len()]
    } else {
        weights
    };
    if splits.is_empty() {
        return Ok(());
    }
    let percents = Money::from_nanos(100, CURRENCY).allocate(&weights)?;
    for (split, percent) in splits.iter_mut().zip(percents) {
        split.share_percent = u64::try_from(percent.nanos()).unwrap_or(0);
    }
    Ok(())
}

/// The model that did most of the work, by value, then by tokens.
fn main_model(messages: &[StoredMessage]) -> String {
    let mut by_model: BTreeMap<&str, (i64, u64)> = BTreeMap::new();
    for message in messages {
        let entry = by_model.entry(message.model.as_str()).or_insert((0, 0));
        entry.0 = entry.0.saturating_add(message_value(message).nanos);
        let tokens = message.tokens.kinds();
        entry.1 = entry
            .1
            .saturating_add(tokens.input)
            .saturating_add(tokens.output)
            .saturating_add(tokens.cache_write)
            .saturating_add(tokens.cache_read);
    }
    by_model
        .into_iter()
        .max_by_key(|(_, weight)| *weight)
        .map(|(model, _)| model.to_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{Organization, Project, SeveralKeys};
    use crate::placing::KeyCount;
    use crate::sources::{FileContents, Harness, Message};
    use crate::store::local_id;

    fn message(key: &str, at_ms: i64, branch: &str, model: &str, input: u64) -> Message {
        Message {
            session_id: "s1".into(),
            key: key.into(),
            id: key.into(),
            request_id: None,
            at_ms,
            model: model.into(),
            tokens: Tokens {
                input,
                ..Tokens::default()
            },
            reported_cost: None,
            branch: Some(branch.into()),
            directory: Some("/nowhere/storefront".into()),
            repository: Some("kestrel/storefront".into()),
            agent_id: None,
            sidechain: false,
        }
    }

    fn context() -> Context {
        Context {
            organization: Some(Organization {
                name: "Kestrel".into(),
                currency: None,
                tag_names: vec!["team".into()],
                several_keys: SeveralKeys::Split,
            }),
            projects: vec![Project {
                name: "storefront".into(),
                ticket_key_pattern: Some(r"^KD-\d+".into()),
                repositories: vec!["kestrel/storefront".into()],
                tags: BTreeMap::new(),
            }],
            ..Context::default()
        }
    }

    fn store_with(messages: Vec<Message>, keys: Vec<KeyCount>) -> Store {
        let mut store = Store::in_memory().unwrap();
        let contents = FileContents {
            messages,
            titles: vec![("s1".into(), "Checkout fixes".into())],
            ..FileContents::default()
        };
        let keys: Vec<(String, KeyCount)> =
            keys.into_iter().map(|k| ("s1".to_owned(), k)).collect();
        let batch = store.batch().unwrap();
        batch
            .add_file("/f", Harness::ClaudeCode, 1, 1, &contents, &keys)
            .unwrap();
        batch.commit().unwrap();
        store.create_entries(0).unwrap();
        store
    }

    fn one(store: &Store) -> Entry {
        let context = context();
        let patterns = context.key_patterns(None);
        let run_tags = RunTags::default();
        let placing = Placing {
            context: &context,
            patterns: &patterns,
            run_tags: &run_tags,
        };
        load(store, &[Status::Draft], &placing)
            .unwrap()
            .into_iter()
            .next()
            .unwrap()
    }

    #[test]
    fn a_session_that_changes_branch_is_split_exactly() {
        let store = store_with(
            vec![
                message("m1", 1, "KD-1-a", "claude-sonnet-4-5", 1_000_000),
                message("m2", 2, "KD-2-b", "claude-sonnet-4-5", 3_000_000),
                message("m3", 3, "KD-1-a", "claude-sonnet-4-5", 1_000_000),
            ],
            Vec::new(),
        );
        let entry = one(&store);
        assert_eq!(entry.value.nanos, 15_000_000_000);
        assert_eq!(entry.splits.len(), 2);
        let first = entry.splits.first().unwrap();
        assert_eq!(first.tags.get("ticket").map(String::as_str), Some("KD-1"));
        assert_eq!(first.value_nanos, 6_000_000_000);
        assert_eq!(first.tokens.input, 2_000_000);
        assert_eq!(first.share_percent, 40);
        assert_eq!(
            first.message_ids.as_deref(),
            Some(&["m1".to_owned(), "m3".to_owned()][..])
        );
        assert_eq!(entry.splits.get(1).unwrap().share_percent, 60);
        assert_eq!(entry.ticket(), Some("KD-2"));
        assert_eq!(entry.on_tickets_nanos(), 15_000_000_000);
        assert_eq!(entry.summary.as_deref(), Some("Checkout fixes"));
        assert_eq!(entry.started_ms, 1);
        assert_eq!(entry.ended_ms, 3);
        assert_eq!(entry.tokens.input, 5_000_000);
        assert!(!entry.asks_classifier);
    }

    #[test]
    fn segments_on_the_same_tags_are_one_split() {
        let store = store_with(
            vec![
                message("m1", 1, "main", "claude-sonnet-4-5", 10),
                message("m2", 2, "develop", "claude-sonnet-4-5", 10),
            ],
            Vec::new(),
        );
        let entry = one(&store);
        assert_eq!(entry.splits.len(), 1);
        assert_eq!(entry.splits.first().unwrap().share_percent, 100);
        assert_eq!(entry.placed_by, "rule: repository");
        assert!(entry.asks_classifier);
    }

    #[test]
    fn hand_splits_and_equal_splits_divide_the_value_by_share() {
        let keys = vec![
            KeyCount {
                key: "KD-7".into(),
                count: 1,
                first_at_ms: 1,
                last_at_ms: 1,
            },
            KeyCount {
                key: "KD-8".into(),
                count: 1,
                first_at_ms: 2,
                last_at_ms: 2,
            },
        ];
        let store = store_with(
            vec![message("m1", 1, "main", "claude-sonnet-4-5", 1_000_000)],
            keys,
        );
        let equal = one(&store);
        let percents: Vec<u64> = equal.splits.iter().map(|s| s.share_percent).collect();
        assert_eq!(percents, [50, 50]);
        assert_eq!(equal.splits.first().unwrap().value_nanos, 1_500_000_000);
        assert!(equal.candidates.is_empty());

        let local = local_id(Harness::ClaudeCode, "s1");
        store
            .set_hand_split(&local, Some(&[("KD-231".into(), 60), ("OPS-1".into(), 40)]))
            .unwrap();
        let hand = one(&store);
        assert_eq!(hand.placed_by, "hand");
        let first = hand.splits.first().unwrap();
        assert_eq!(first.share_percent, 60);
        assert_eq!(first.value_nanos, 1_800_000_000);
        assert_eq!(first.tokens.input, 600_000);
        assert_eq!(hand.splits.get(1).unwrap().tokens.input, 400_000);
        assert_eq!(
            first.tags.get("project").map(String::as_str),
            Some("storefront")
        );
        let second = hand.splits.get(1).unwrap();
        assert_eq!(second.tags.get("ticket").map(String::as_str), Some("OPS-1"));
        assert_eq!(second.tags.get("project"), None);
        assert!(first.message_ids.is_none());
        assert_eq!(hand.candidates, ["KD-7", "KD-8"]);
    }

    #[test]
    fn unknown_models_are_named_not_counted_as_zero() {
        let mut priced_by_agent = message("m2", 2, "main", "house-model", 10);
        priced_by_agent.reported_cost = Some(500);
        let store = store_with(
            vec![
                message("m1", 1, "main", "mystery-model", 10),
                priced_by_agent,
                message("m3", 3, "main", "", 10),
            ],
            Vec::new(),
        );
        let entry = one(&store);
        assert_eq!(entry.value.nanos, 500);
        assert_eq!(entry.value.unknown_models, ["mystery-model", "unknown"]);
        assert!(!entry.value.is_known());
    }

    #[test]
    fn the_main_model_did_most_of_the_work() {
        let store = store_with(
            vec![
                message("m1", 1, "main", "claude-haiku-4-5", 1_000),
                message("m2", 2, "main", "claude-opus-4-5", 1_000),
            ],
            Vec::new(),
        );
        assert_eq!(one(&store).model, "claude-opus-4-5");
    }

    #[test]
    fn tag_lists_read_like_the_action_writes_them() {
        let tags = parse_tag_list("purpose=code-review, team=web\nbad x=");
        assert_eq!(tags.len(), 2);
        assert_eq!(tags.get("team").map(String::as_str), Some("web"));
    }
}
