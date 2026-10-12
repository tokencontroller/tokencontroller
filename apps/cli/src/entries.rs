//! Token entries as people see them: a session's model calls added up,
//! split into segments where the branch, working directory or subagent
//! changes, and placed with the rules (Sessions and subagents; ADR 0006).

use std::collections::BTreeMap;
use std::path::Path;

use tokencontroller_core::{Currency, Money};

use crate::error::Result;
use crate::keys::KeyPattern;
use crate::placing::{Named, Pick, Placement, Placer, Signals};
use crate::store::{EntryRow, Status, Store, StoredMessage};
use crate::tokens::Tokens;

/// List value is computed in US dollars, the currency of the price list.
pub const CURRENCY: Currency = Currency::USD;

/// What this run names for every entry it reads: tags from `TC_TICKET`,
/// `TC_PROJECT` and `TC_TAGS` or `tc submit --tag`, and signals from
/// `tc submit --signal`, as the GitHub Action passes them. Tags named here
/// win over every rule; signals fill in what the session files lack.
#[derive(Debug, Clone, Default)]
pub struct Run {
    pub tags: BTreeMap<String, String>,
    pub signals: BTreeMap<String, String>,
}

/// The signals `tc submit --signal` takes.
pub const RUN_SIGNALS: [&str; 4] = ["repository", "branch", "pull_request", "pull_request_title"];

impl Run {
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
        let signals = std::env::var("TC_SIGNALS")
            .ok()
            .map(|text| parse_signals(&text))
            .unwrap_or_default();
        Self { tags, signals }
    }
}

/// `TC_SIGNALS` as the GitHub Action sets it: a JSON object such as
/// `{"repository":"kestrel/storefront","pull_request":12}`. Names tc does
/// not take are left out.
pub fn parse_signals(text: &str) -> BTreeMap<String, String> {
    let Ok(serde_json::Value::Object(object)) = serde_json::from_str(text) else {
        return BTreeMap::new();
    };
    object
        .into_iter()
        .filter(|(name, _)| RUN_SIGNALS.contains(&name.as_str()))
        .filter_map(|(name, value)| match value {
            serde_json::Value::String(text) if !text.trim().is_empty() => {
                Some((name, text.trim().to_owned()))
            }
            serde_json::Value::Number(number) => Some((name, number.to_string())),
            _ => None,
        })
        .collect()
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

/// The list value of one model call: the agent's reported cost where it has
/// one, otherwise its tokens at the price list (Bills: list value).
pub fn message_value(message: &StoredMessage) -> Value {
    match message
        .reported_cost
        .or_else(|| crate::prices::list_value(&message.model, &message.tokens))
    {
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
    /// Tags named by hand, by `tc record` or for this run: they win over
    /// every rule and are sent as named.
    pub named: Named,
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
    pub placer: &'a Placer,
    pub patterns: &'a [KeyPattern],
    pub run: &'a Run,
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

    let mut named: Named = placing.run.tags.clone();
    named.extend(row.named_tags.clone());
    let pick = row.pick.as_deref().map(|pick| {
        if pick == "none" {
            Pick::Nothing
        } else {
            Pick::Ticket(pick.to_owned())
        }
    });
    let segments = segments_of(&row, &messages, &keys, placing);

    let mut value = Value::default();
    let mut tokens = Tokens::default();
    for message in &messages {
        value.add(&message_value(message));
        tokens = tokens.plus(message.tokens);
    }

    let placements: Vec<Placement> = segments
        .iter()
        .map(|(signals, _, _)| placing.placer.place(signals, &named, pick.as_ref()))
        .collect();
    let asks_classifier = placements
        .iter()
        .any(|placement| placement.ticket_only_mentioned || !placement.has_ticket());

    let mut splits = if let Some(hand) = &row.hand_split {
        let signals = segments
            .first()
            .map(|(signals, _, _)| signals.clone())
            .unwrap_or_default();
        let hand: Vec<HandShare> = hand
            .iter()
            .map(|(ticket, weight)| {
                let mut named = named.clone();
                named.insert("ticket".to_owned(), ticket.clone());
                HandShare {
                    weight: *weight,
                    tags: placing.placer.place(&signals, &named, None).tags(),
                }
            })
            .collect();
        by_hand(&hand, &signals, &value, tokens)?
    } else {
        by_segment(&segments, &placements, &messages)?
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
        named,
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
    placing: &Placing<'_>,
) -> Vec<Segment> {
    let run = |name: &str| placing.run.signals.get(name).cloned();
    // The Action may spell it `github.com/kestrel/storefront`; projects map
    // `kestrel/storefront`, the name tc reads from a git remote.
    let run_repository =
        || run("repository").map(|named| crate::repo::name_from_url(&named).unwrap_or(named));
    let mut pull_request_title_keys: Vec<String> = row
        .signals
        .get("pull_request_title_keys")
        .map(|keys| {
            keys.split(',')
                .map(str::to_owned)
                .filter(|k| !k.is_empty())
                .collect()
        })
        .unwrap_or_default();
    if let Some(title) = placing.run.signals.get("pull_request_title") {
        for key in crate::keys::find_all(placing.patterns, title) {
            if !pull_request_title_keys.contains(&key) {
                pull_request_title_keys.push(key);
            }
        }
    }
    let pull_request = run("pull_request").or_else(|| row.signals.get("pull_request").cloned());
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
                branch: message.branch.clone().or_else(|| run("branch")),
                directory: message.directory.clone(),
                repository: message.repository.clone().or_else(run_repository),
                pull_request: pull_request.clone(),
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

/// A split to add: its tags, value, tokens, model calls and signals.
struct Piece {
    tags: BTreeMap<String, String>,
    value_nanos: i64,
    tokens: Tokens,
    message_ids: Option<Vec<String>>,
    signals: Signals,
}

/// Adds a piece to the splits; pieces with the same tags become one split.
fn add_piece(splits: &mut Vec<Split>, piece: Piece) {
    if let Some(split) = splits.iter_mut().find(|split| split.tags == piece.tags) {
        split.value_nanos = split.value_nanos.saturating_add(piece.value_nanos);
        split.tokens = split.tokens.plus(piece.tokens);
        split.message_ids = match (split.message_ids.take(), piece.message_ids) {
            (Some(mut existing), Some(ids)) => {
                existing.extend(ids);
                Some(existing)
            }
            _ => None,
        };
    } else {
        splits.push(Split {
            tags: piece.tags,
            share_percent: 0,
            value_nanos: piece.value_nanos,
            tokens: piece.tokens,
            message_ids: piece.message_ids,
            signals: piece.signals,
        });
    }
}

/// One split per segment, or per share where prompts named several tickets
/// and the organization splits equally; pieces that landed on the same tags
/// become one split.
fn by_segment(
    segments: &[Segment],
    placements: &[Placement],
    messages: &[StoredMessage],
) -> Result<Vec<Split>> {
    let mut splits: Vec<Split> = Vec::new();
    for ((signals, indexes, value), placement) in segments.iter().zip(placements) {
        let calls: Vec<&StoredMessage> = indexes
            .iter()
            .filter_map(|index| messages.get(*index))
            .collect();
        let tokens = calls
            .iter()
            .fold(Tokens::default(), |sum, message| sum.plus(message.tokens));
        if let [share] = placement.shares.as_slice() {
            add_piece(
                &mut splits,
                Piece {
                    tags: share.tags.clone(),
                    value_nanos: value.nanos,
                    tokens,
                    message_ids: Some(
                        calls
                            .iter()
                            .map(|message| message.message_id.clone())
                            .collect(),
                    ),
                    signals: signals.clone(),
                },
            );
            continue;
        }
        let weights: Vec<u64> = placement.shares.iter().map(|share| share.weight).collect();
        let values = Money::from_nanos(value.nanos, CURRENCY).allocate(&weights)?;
        let token_parts = divide_tokens(tokens, &weights)?;
        for ((share, part), tokens) in placement.shares.iter().zip(values).zip(token_parts) {
            add_piece(
                &mut splits,
                Piece {
                    tags: share.tags.clone(),
                    value_nanos: part.nanos(),
                    tokens,
                    message_ids: None,
                    signals: signals.clone(),
                },
            );
        }
    }
    Ok(splits)
}

/// One share of a split by hand, already placed on its ticket.
struct HandShare {
    weight: u64,
    tags: BTreeMap<String, String>,
}

/// Splits by hand: the entry's value and tokens divided by share.
fn by_hand(
    shares: &[HandShare],
    signals: &Signals,
    value: &Value,
    tokens: Tokens,
) -> Result<Vec<Split>> {
    let weights: Vec<u64> = shares.iter().map(|share| share.weight).collect();
    let values = Money::from_nanos(value.nanos, CURRENCY).allocate(&weights)?;
    let token_parts = divide_tokens(tokens, &weights)?;
    let mut splits = Vec::new();
    for ((share, part), tokens) in shares.iter().zip(values).zip(token_parts) {
        add_piece(
            &mut splits,
            Piece {
                tags: share.tags.clone(),
                value_nanos: part.nanos(),
                tokens,
                message_ids: None,
                signals: signals.clone(),
            },
        );
    }
    Ok(splits)
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
    use crate::context::{Context, Organization, Project};
    use crate::placing::KeyCount;
    use crate::sources::{FileContents, Harness, Message};
    use crate::store::local_id;
    use tokencontroller_core::{Rule, SeveralKeys};

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
                several_keys: SeveralKeys::EqualSplit,
                rules: Rule::defaults(),
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
        let placer = Placer::new(&context, None);
        let run = Run::default();
        let placing = Placing {
            placer: &placer,
            patterns: &patterns,
            run: &run,
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
        // OPS-1 matches no project's pattern, so the repository's project
        // stays, as the core places it.
        let second = hand.splits.get(1).unwrap();
        assert_eq!(second.tags.get("ticket").map(String::as_str), Some("OPS-1"));
        assert_eq!(
            second.tags.get("project").map(String::as_str),
            Some("storefront")
        );
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
    fn signals_read_like_the_action_writes_them() {
        let signals = parse_signals(
            r#"{"repository":"kestrel/storefront","pull_request":12,"pull_request_title":"KD-1: x","branch":"","weather":"sunny"}"#,
        );
        assert_eq!(signals.get("pull_request").map(String::as_str), Some("12"));
        assert_eq!(signals.len(), 3);
        assert!(parse_signals("not json").is_empty());
    }

    #[test]
    fn tag_lists_read_like_the_action_writes_them() {
        let tags = parse_tag_list("purpose=code-review, team=web\nbad x=");
        assert_eq!(tags.len(), 2);
        assert_eq!(tags.get("team").map(String::as_str), Some("web"));
    }
}
