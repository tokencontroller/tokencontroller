//! The event log on this machine and what it folds to: the messages read
//! from the session files, the tag values, rules and price table in force,
//! the splits and submits recorded here, folded with the core's
//! [`State::fold`] exactly as the service folds its own log.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tokencontroller_core::{
    AssignedBy, Currency, Event, Label, Message, MessageId, ModelRef, Money, PriceRow, PriceSource,
    Record, Rule, Scope, SessionFacts, SessionId, Signal, Signals, Split, SplitTarget, State,
    SubmissionStatus, TagName, TagValue, TagValueRecord, Tags, Then, TokenCounts, TokenKind,
};

use crate::context::{Context, PERSON};
use crate::error::{Error, Result};
use crate::keys::KeyPattern;
use crate::output;
use crate::sources::Harness;
use crate::store::{StoredMessage, local_id};

/// The tags and signals of this run, from `TC_FEATURE`, `TC_PROJECT`,
/// `TC_TAGS`, `TC_SIGNALS` and the flags of `tc submit`. They apply to every
/// session this run reads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Run {
    pub tags: Tags,
    pub signals: RunSignals,
}

/// Signals the session files lack, as the GitHub Action passes them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct RunSignals {
    pub repository: Option<String>,
    pub branch: Option<String>,
    pub pull_request: Option<serde_json::Value>,
    pub pull_request_title: Option<String>,
}

/// The names `--signal` and `TC_SIGNALS` take.
pub const RUN_SIGNALS: [&str; 4] = ["repository", "branch", "pull_request", "pull_request_title"];

impl Run {
    pub fn from_env() -> Self {
        let var = |name: &str| {
            std::env::var(name)
                .ok()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        };
        let mut tags = Tags::new();
        if let Some(text) = var("TC_TAGS") {
            tags = Tags::parse(
                text.split(',')
                    .map(str::trim)
                    .filter(|pair| !pair.is_empty()),
            )
            .unwrap_or_default();
        }
        for (name, variable) in [
            (TagName::feature(), "TC_FEATURE"),
            (TagName::project(), "TC_PROJECT"),
        ] {
            if let Some(value) = var(variable).and_then(|value| TagValue::new(&value).ok()) {
                tags.insert(name, value);
            }
        }
        let signals = var("TC_SIGNALS")
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Self { tags, signals }
    }

    /// Sets one signal by the name `--signal` takes.
    pub fn set_signal(&mut self, name: &str, value: &str) -> Result<()> {
        match name {
            "repository" => self.signals.repository = Some(value.to_owned()),
            "branch" => self.signals.branch = Some(value.to_owned()),
            "pull_request" => self.signals.pull_request = Some(serde_json::Value::from(value)),
            "pull_request_title" => self.signals.pull_request_title = Some(value.to_owned()),
            _ => {
                return Err(Error::usage(format!(
                    "{name} is not a signal tc submit takes; use {}",
                    RUN_SIGNALS.join(", ")
                )));
            }
        }
        Ok(())
    }

    /// The pull request number as text, from a number or a string.
    pub fn pull_request(&self) -> Option<String> {
        pull_request_text(self.signals.pull_request.as_ref())
    }
}

fn pull_request_text(value: Option<&serde_json::Value>) -> Option<String> {
    match value? {
        serde_json::Value::Number(number) => Some(number.to_string()),
        serde_json::Value::String(text) if !text.trim().is_empty() => Some(text.trim().to_owned()),
        _ => None,
    }
}

/// Session facts another tool knows, as `tc record` takes them.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Facts {
    pub turns: Option<u64>,
    pub tool_calls: Option<u64>,
    pub retries: Option<u64>,
    pub reruns: Option<u64>,
    pub duration_seconds: Option<u64>,
    pub subagents: Option<u64>,
    pub files_touched: Option<u64>,
    pub result: Option<String>,
}

impl Facts {
    /// Adds later facts: a value given wins over one missing.
    pub fn merge(&mut self, later: Self) {
        self.turns = later.turns.or(self.turns);
        self.tool_calls = later.tool_calls.or(self.tool_calls);
        self.retries = later.retries.or(self.retries);
        self.reruns = later.reruns.or(self.reruns);
        self.duration_seconds = later.duration_seconds.or(self.duration_seconds);
        self.subagents = later.subagents.or(self.subagents);
        self.files_touched = later.files_touched.or(self.files_touched);
        self.result = later.result.or(self.result.take());
    }

    fn core(&self, summary: Option<String>) -> SessionFacts {
        SessionFacts {
            turns: self.turns,
            tool_calls: self.tool_calls,
            retries: self.retries,
            reruns: self.reruns,
            duration_seconds: self.duration_seconds,
            subagents: self.subagents,
            files_touched: self.files_touched,
            result: self.result.clone(),
            summary,
        }
    }
}

/// What `tc record` was told about a session, kept on this machine.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Recorded {
    pub summary: Option<String>,
    pub tags: BTreeMap<String, String>,
    pub pull_request: Option<String>,
    pub pull_request_title: Option<String>,
    pub facts: Facts,
}

impl Recorded {
    /// Adds a later record: its values win.
    pub fn merge(&mut self, later: Self) {
        if later.summary.is_some() {
            self.summary = later.summary;
        }
        self.tags.extend(later.tags);
        self.pull_request = later.pull_request.or(self.pull_request.take());
        self.pull_request_title = later.pull_request_title.or(self.pull_request_title.take());
        self.facts.merge(later.facts);
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Everything the log on this machine is built from.
pub struct Inputs<'a> {
    pub messages: &'a [StoredMessage],
    /// Feature keys found in prompts and titles, by local id.
    pub keys: &'a BTreeMap<String, Vec<String>>,
    pub titles: &'a BTreeMap<String, String>,
    pub records: &'a BTreeMap<String, Recorded>,
    pub run: &'a Run,
    pub context: &'a Context,
    pub connected: bool,
    pub pattern: Option<&'a KeyPattern>,
    /// The person's own price file, used with no service connected.
    pub price_file: Option<&'a Path>,
    /// Splits, rules, submits and reversals recorded on this machine.
    pub local_events: Vec<Event>,
    pub now: OffsetDateTime,
}

/// Who charges for a model, from its name, as the token entry schema says.
pub fn provider_of(model: &str) -> &'static str {
    let lower = model.to_ascii_lowercase();
    if lower.starts_with("claude") {
        "anthropic"
    } else if lower.starts_with("gpt")
        || lower.starts_with("codex")
        || lower.starts_with("o1")
        || lower.starts_with("o3")
        || lower.starts_with("o4")
    {
        "openai"
    } else if lower.starts_with("gemini") {
        "google"
    } else {
        "other"
    }
}

/// Reads the person's own price table: a CSV file with the columns
/// `provider`, `model`, `kind`, `price_per_million` and `valid_from`, prices
/// in US dollars.
pub fn read_price_file(path: &Path) -> Result<Vec<PriceRow>> {
    let text = std::fs::read_to_string(path).map_err(|source| Error::file(path, source))?;
    let bad = |line: usize, what: &str| {
        Error::usage(format!(
            "{}, line {line}: {what}; the columns are provider, model, kind, price_per_million, valid_from",
            path.display()
        ))
    };
    let mut lines = text.lines().enumerate();
    let (_, header) = lines.next().ok_or_else(|| bad(1, "the file is empty"))?;
    let columns: Vec<&str> = header.split([',', ';']).map(str::trim).collect();
    let index = |name: &str| columns.iter().position(|column| *column == name);
    let (Some(provider), Some(model), Some(kind), Some(price), Some(from)) = (
        index("provider"),
        index("model"),
        index("kind"),
        index("price_per_million"),
        index("valid_from"),
    ) else {
        return Err(bad(1, "a column is missing"));
    };
    let mut rows = Vec::new();
    for (number, line) in lines {
        if line.trim().is_empty() {
            continue;
        }
        let number = number.saturating_add(1);
        let cells: Vec<&str> = line.split([',', ';']).map(str::trim).collect();
        let cell = |at: usize| cells.get(at).copied().unwrap_or_default();
        let kind = match cell(kind) {
            "input" => TokenKind::Input,
            "output" => TokenKind::Output,
            "cache_write" => TokenKind::CacheWrite,
            "cache_read" => TokenKind::CacheRead,
            other => return Err(bad(number, &format!("{other} is not a token kind"))),
        };
        let per_million = Money::parse(cell(price), Currency::USD)
            .map_err(|_| bad(number, &format!("{} is not a price", cell(price))))?;
        let valid_from = output::parse_date(cell(from))
            .ok_or_else(|| bad(number, &format!("{} is not a day", cell(from))))?;
        rows.push(PriceRow {
            provider: cell(provider).to_owned(),
            model: cell(model).to_owned(),
            kind,
            per_million,
            valid_from,
            source: PriceSource::Organization,
        });
    }
    Ok(rows)
}

/// The default rules with no service connected: the feature key in the
/// branch, then in the pull request title. Keys in the prompts are the
/// core's default split, equal over the features named.
pub fn local_rules(pattern: &KeyPattern) -> Vec<Rule> {
    let Some(text) = pattern.rule_pattern() else {
        return Vec::new();
    };
    [
        ("key in branch", Signal::Branch),
        ("key in PR title", Signal::PullRequestTitle),
    ]
    .into_iter()
    .enumerate()
    .map(|(order, (name, signal))| Rule {
        name: name.to_owned(),
        scope: Scope::Organization,
        order: u32::try_from(order).unwrap_or(u32::MAX),
        signal,
        pattern: text.clone(),
        then: Then::KeyOf {
            name: TagName::feature(),
        },
    })
    .collect()
}

/// The events of the context: prices, tag values, attributes, rules, the
/// cutoff and the submissions.
fn context_events(inputs: &Inputs<'_>, currency: Currency) -> Result<Vec<Event>> {
    let epoch = OffsetDateTime::UNIX_EPOCH;
    let mut events: Vec<Event> = Vec::new();
    // Prices: the service's table when connected, else the person's file.
    if inputs.connected {
        events.extend(inputs.context.price_events(currency)?);
    } else if let Some(path) = inputs.price_file {
        for (index, row) in read_price_file(path)?.into_iter().enumerate() {
            events.push(Event::new(
                &format!("price-{index}"),
                epoch,
                Record::PriceRow(row),
            )?);
        }
    }
    for (index, value) in inputs.context.tag_values.iter().enumerate() {
        events.push(Event::new(
            &format!("value-{index}"),
            epoch,
            Record::TagValue(value.clone()),
        )?);
    }
    for (index, value) in inputs.context.attribute_values.iter().enumerate() {
        events.push(Event::new(
            &format!("attribute-value-{index}"),
            epoch,
            Record::AttributeValue(value.clone()),
        )?);
    }
    for (index, attribute) in inputs.context.attributes.iter().enumerate() {
        events.push(Event::new(
            &format!("attribute-{index}"),
            epoch,
            Record::PersonAttribute(attribute.clone()),
        )?);
    }
    let rules: Vec<Rule> = if inputs.connected {
        inputs
            .context
            .rules
            .iter()
            .chain(&inputs.context.my_rules)
            .cloned()
            .collect()
    } else {
        inputs.pattern.map(local_rules).unwrap_or_default()
    };
    for (index, rule) in rules.into_iter().enumerate() {
        events.push(Event::new(
            &format!("rule-{index}"),
            epoch,
            Record::Rule(rule),
        )?);
    }
    if !inputs.connected {
        return Ok(events);
    }
    if let Some(cutoff) = inputs
        .context
        .cutoff
        .as_deref()
        .and_then(output::parse_date)
    {
        events.push(Event::new("cutoff", epoch, Record::Cutoff(cutoff))?);
    }
    events.extend(inputs.context.submission_events()?);
    Ok(events)
}

/// Builds the log and folds it as of now.
pub fn fold(inputs: &Inputs<'_>) -> Result<State> {
    let currency = if inputs.connected {
        inputs.context.currency()
    } else {
        Currency::USD
    };
    let epoch = OffsetDateTime::UNIX_EPOCH;
    let person = TagValue::new(PERSON)?;
    let mut events = context_events(inputs, currency)?;
    let mut features: BTreeSet<String> = BTreeSet::new();
    let mut config_cache: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for stored in inputs.messages {
        let local = local_id(stored.harness, &stored.session_id);
        let record = inputs.records.get(&local);
        let message = message_of(stored, &local, inputs, record, &person, &mut config_cache)?;
        if !inputs.connected
            && let Some(pattern) = inputs.pattern
        {
            for text in message
                .signals
                .branch
                .iter()
                .chain(message.signals.pull_request_title.iter())
            {
                features.extend(pattern.find_all(text));
            }
            features.extend(message.signals.prompt_keys.iter().cloned());
        }
        events.push(Event::new(
            &format!("msg:{local}:{}", message.message_id),
            message.at,
            Record::Message(Box::new(message)),
        )?);
    }
    // With no service, every key found is a feature on record.
    for key in features {
        let Ok(value) = TagValue::new(&key) else {
            continue;
        };
        events.push(Event::new(
            &format!("feature:{key}"),
            epoch,
            Record::TagValue(TagValueRecord {
                name: TagName::feature(),
                key: value,
                title: None,
                description: None,
                source_id: None,
                source_status: None,
                defaults: Tags::new(),
            }),
        )?);
    }
    events.extend(inputs.local_events.iter().cloned());
    Ok(State::fold(&events, inputs.now, currency)?)
}

/// An id as the core takes it: never empty.
fn id(text: &str) -> &str {
    let text = text.trim();
    if text.is_empty() { "unknown" } else { text }
}

fn message_of(
    stored: &StoredMessage,
    local: &str,
    inputs: &Inputs<'_>,
    record: Option<&Recorded>,
    person: &TagValue,
    config_cache: &mut BTreeMap<String, BTreeMap<String, String>>,
) -> Result<Message> {
    let run = inputs.run;
    let session_id =
        SessionId::new(id(&stored.session_id)).map_err(|error| Error::Data(error.to_string()))?;
    let message_id =
        MessageId::new(id(&stored.message_id)).map_err(|error| Error::Data(error.to_string()))?;
    let at = output::moment(stored.at_ms)
        .ok_or_else(|| Error::Data(format!("message {} has no time", stored.message_id)))?;
    let mut tags = run.tags.clone();
    if let Some(record) = record {
        for (name, value) in &record.tags {
            if let (Ok(name), Ok(value)) = (TagName::new(name), TagValue::new(value)) {
                tags.insert(name, value);
            }
        }
    }
    let pull_request_title = run
        .signals
        .pull_request_title
        .clone()
        .or_else(|| record.and_then(|record| record.pull_request_title.clone()));
    let mut prompt_keys: Vec<String> = inputs.keys.get(local).cloned().unwrap_or_default();
    if let (Some(title), Some(pattern)) = (&pull_request_title, inputs.pattern) {
        for key in pattern.find_all(title) {
            if !prompt_keys.contains(&key) {
                prompt_keys.push(key);
            }
        }
    }
    let config = match &stored.directory {
        Some(directory) => config_cache
            .entry(directory.clone())
            .or_insert_with(|| crate::repo::config_tags(Path::new(directory)))
            .clone(),
        None => BTreeMap::new(),
    };
    let mut resource = BTreeMap::new();
    if let Some(directory) = &stored.directory {
        resource.insert("working_directory".to_owned(), directory.clone());
    }
    let pull_request = run
        .pull_request()
        .or_else(|| record.and_then(|record| record.pull_request.clone()));
    if let Some(number) = &pull_request {
        resource.insert("pull_request".to_owned(), number.clone());
    }
    let summary = record
        .and_then(|record| record.summary.clone())
        .or_else(|| inputs.titles.get(local).cloned());
    Ok(Message {
        session_id,
        message_id,
        person: person.clone(),
        model: ModelRef::new(provider_of(&stored.model), &stored.model),
        tokens: Some(stored.tokens.kinds()),
        at,
        agent_harness: stored.harness.name().to_owned(),
        agent_name: stored.subagent.clone(),
        reported_cost: stored
            .reported_cost
            .map(|nanos| Money::from_nanos(nanos, Currency::USD)),
        tags,
        signals: Signals {
            branch: run.signals.branch.clone().or_else(|| stored.branch.clone()),
            repository: run
                .signals
                .repository
                .as_deref()
                .and_then(crate::repo::name_from_url)
                .or_else(|| run.signals.repository.clone())
                .or_else(|| stored.repository.clone()),
            working_directory: stored.directory.clone(),
            pull_request: pull_request
                .as_deref()
                .and_then(|number| number.parse().ok()),
            pull_request_title,
            subagent_type: None,
            prompt_keys,
            config,
            resource,
        },
        session: record
            .map(|record| record.facts.clone())
            .unwrap_or_default()
            .core(summary),
    })
}

/// Where a session stands with the service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Open,
    Submitted,
    Rejected(String),
}

impl Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Submitted => "submitted",
            Self::Rejected(_) => "rejected",
        }
    }
}

/// One part of a session's split, as the review list shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    /// The share of the session, in basis points.
    pub basis_points: u64,
    /// The tags assigned, before defaults.
    pub tags: Tags,
    /// The tags closed under defaults, with the person.
    pub closed: Tags,
    pub amount: Money,
}

impl Part {
    /// The fraction as a decimal string, such as `0.6` or `1`.
    pub fn fraction(&self) -> String {
        fraction_text(self.basis_points)
    }
}

/// Basis points as a fraction, such as `0.6`, `0.3333` or `1`.
pub fn fraction_text(basis_points: u64) -> String {
    let whole = basis_points.checked_div(10_000).unwrap_or(0);
    let rest = basis_points.checked_rem(10_000).unwrap_or(0);
    if rest == 0 {
        return whole.to_string();
    }
    let digits = format!("{rest:04}");
    format!("{whole}.{}", digits.trim_end_matches('0'))
}

/// One message of a session, as it is sent and exported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageView {
    pub message_id: String,
    pub at: OffsetDateTime,
    pub model: ModelRef,
    pub tokens: TokenCounts,
    pub reported_cost: Option<Money>,
    pub amount: Money,
    pub priced: bool,
}

/// A session as `tc review` lists it: its messages with their cost, its
/// split and where it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionView {
    pub number: i64,
    pub local_id: String,
    pub harness: Harness,
    pub session_id: String,
    pub started_at: OffsetDateTime,
    pub ended_at: OffsetDateTime,
    /// The model of the first message.
    pub model: ModelRef,
    pub tokens: TokenCounts,
    /// The cost of the priced messages, `None` while none is priced.
    pub cost: Option<Money>,
    pub count_errors: usize,
    pub unknown_models: Vec<String>,
    pub reported_cost: Option<Money>,
    pub summary: Option<String>,
    pub facts: SessionFacts,
    pub signals: Signals,
    pub subagent_types: Vec<String>,
    pub status: Status,
    pub assigned_by: String,
    pub label: Label,
    pub parts: Vec<Part>,
    pub messages: Vec<MessageView>,
    /// The split recorded by hand on the session, if any.
    pub hand_split: Option<Split>,
    /// The tags given with the messages: the run's tags and the recorded
    /// ones, before any rule.
    pub tags: Tags,
}

impl SessionView {
    /// The cost on features, for the share.
    pub fn on_features(&self) -> Money {
        let currency = self
            .parts
            .first()
            .map_or(Currency::USD, |part| part.amount.currency());
        self.parts
            .iter()
            .filter(|part| part.closed.contains(&TagName::feature()))
            .fold(Money::zero(currency), |sum, part| {
                sum.checked_add(part.amount).unwrap_or(sum)
            })
    }
}

/// The fold with the lookups the session views need.
struct Folded<'a> {
    state: &'a State,
    person: TagValue,
    currency: Currency,
    lines: BTreeMap<(SessionId, MessageId), Vec<&'a tokencontroller_core::state::Line>>,
    costs: BTreeMap<(SessionId, MessageId), tokencontroller_core::MessageCost>,
    splits: BTreeMap<&'a SplitTarget, &'a Split>,
    rules: Vec<&'a Rule>,
}

/// What one message adds to its session's view.
struct Counted {
    amount: Money,
    label: Option<tokencontroller_core::CostLabel>,
}

/// The parts of a session being gathered, by assigned tags.
type Parts = BTreeMap<Tags, (u64, Tags, Money)>;

impl<'a> Folded<'a> {
    fn new(state: &'a State) -> Result<Self> {
        let mut lines: BTreeMap<(SessionId, MessageId), Vec<&tokencontroller_core::state::Line>> =
            BTreeMap::new();
        for line in state.lines() {
            lines
                .entry((line.session_id.clone(), line.message_id.clone()))
                .or_default()
                .push(line);
        }
        Ok(Self {
            state,
            person: TagValue::new(PERSON)?,
            currency: state.currency(),
            lines,
            costs: state
                .message_costs()
                .iter()
                .map(|line| {
                    (
                        (line.session_id.clone(), line.message_id.clone()),
                        line.cost,
                    )
                })
                .collect(),
            splits: state
                .splits()
                .map(|split| (&split.value.target, &split.value))
                .collect(),
            rules: state.rules().map(|rule| &rule.value).collect(),
        })
    }

    /// Adds one message's lines to the session's parts. Returns the
    /// message's amount and cost label, and what assigned its split.
    fn count(
        &self,
        message: &Message,
        session_target: &SplitTarget,
        count: u64,
        parts: &mut Parts,
        assigned_by: &mut Option<String>,
    ) -> Result<Counted> {
        let key = message.key();
        let lines = self.lines.get(&key).cloned().unwrap_or_default();
        let weights: Vec<u64> = self
            .splits
            .get(&SplitTarget::Message {
                session_id: key.0.clone(),
                message_id: key.1.clone(),
            })
            .or(self.splits.get(session_target))
            .filter(|split| split.parts.len() == lines.len())
            .map_or_else(|| vec![1; lines.len()], |split| split.weights());
        let weight_sum = weights
            .iter()
            .fold(0_u64, |sum, w| sum.saturating_add(*w))
            .max(1);
        let mut amount = Money::zero(self.currency);
        for (line, weight) in lines.iter().zip(weights) {
            amount = amount.checked_add(line.amount)?;
            let share = weight
                .saturating_mul(10_000)
                .checked_div(weight_sum)
                .unwrap_or(0)
                .checked_div(count)
                .unwrap_or(0);
            let part = parts
                .entry(line.assigned.clone())
                .or_insert_with(|| (0, line.tags.clone(), Money::zero(self.currency)));
            part.0 = part.0.saturating_add(share);
            part.2 = part.2.checked_add(line.amount)?;
            if assigned_by.is_none() {
                *assigned_by = Some(describe_assignment(
                    line.assigned_by,
                    line,
                    message,
                    &self.rules,
                    self.state,
                ));
            }
        }
        Ok(Counted {
            amount,
            label: self.costs.get(&key).map(|cost| cost.label),
        })
    }

    /// Open while any message is outside a submission; otherwise rejected
    /// with the first reason given, or submitted.
    fn status_of(&self, messages: &[&tokencontroller_core::state::Recorded<Message>]) -> Status {
        let mut rejected: Option<String> = None;
        for message in messages {
            match self.state.submission_of(&self.person, message.value.at) {
                None => return Status::Open,
                Some(submission) => rejected = rejected.or_else(|| rejection(submission)),
            }
        }
        rejected.map_or(Status::Submitted, Status::Rejected)
    }

    fn view(
        &self,
        row: &crate::store::SessionRow,
        messages: &[&tokencontroller_core::state::Recorded<Message>],
    ) -> Result<Option<SessionView>> {
        let Some(first) = messages.first() else {
            return Ok(None);
        };
        let session_target = SplitTarget::Session(first.value.session_id.clone());
        let hand_split = self
            .splits
            .get(&session_target)
            .map(|split| (*split).clone());
        let mut parts: Parts = BTreeMap::new();
        let mut message_views = Vec::new();
        let mut tokens = TokenCounts::default();
        let mut cost: Option<Money> = None;
        let mut reported: Option<Money> = None;
        let mut count_errors = 0_usize;
        let mut unknown_models: Vec<String> = Vec::new();
        let mut assigned_by: Option<String> = None;
        let mut subagent_types: Vec<String> = Vec::new();
        let count = u64::try_from(messages.len()).unwrap_or(u64::MAX).max(1);
        for message in messages {
            let counted = self.count(
                &message.value,
                &session_target,
                count,
                &mut parts,
                &mut assigned_by,
            )?;
            match counted.label {
                Some(tokencontroller_core::CostLabel::Priced) => {
                    cost = Some(
                        cost.map_or(Ok(counted.amount), |sum| sum.checked_add(counted.amount))?,
                    );
                }
                Some(tokencontroller_core::CostLabel::CountError) => {
                    count_errors = count_errors.saturating_add(1);
                }
                Some(tokencontroller_core::CostLabel::UnknownModel) | None => {
                    let model = message.value.model.to_string();
                    if !unknown_models.contains(&model) {
                        unknown_models.push(model);
                    }
                }
            }
            if let Some(counts) = message.value.tokens {
                tokens = tokens.checked_add(counts).unwrap_or(tokens);
            }
            if let Some(fact) = message.value.reported_cost {
                reported = Some(reported.map_or(Ok(fact), |sum| sum.checked_add(fact))?);
            }
            if let Some(name) = &message.value.agent_name
                && !subagent_types.contains(name)
            {
                subagent_types.push(name.clone());
            }
            message_views.push(MessageView {
                message_id: message.value.message_id.to_string(),
                at: message.value.at,
                model: message.value.model.clone(),
                tokens: message.value.tokens.unwrap_or_default(),
                reported_cost: message.value.reported_cost,
                amount: counted.amount,
                priced: matches!(counted.label, Some(tokencontroller_core::CostLabel::Priced)),
            });
        }
        let status = self.status_of(messages);
        let parts = ordered_parts(parts);
        let label = parts
            .first()
            .map_or(Label::OrganizationOverhead, |part| Label::of(&part.closed));
        let last = messages.last().unwrap_or(first);
        Ok(Some(SessionView {
            number: row.number,
            local_id: row.local_id.clone(),
            harness: row.harness,
            session_id: first.value.session_id.to_string(),
            started_at: first.value.at,
            ended_at: last.value.at,
            model: first.value.model.clone(),
            tokens,
            cost,
            count_errors,
            unknown_models,
            reported_cost: reported,
            summary: first.value.session.summary.clone(),
            facts: first.value.session.clone(),
            signals: first.value.signals.clone(),
            subagent_types,
            status,
            assigned_by: assigned_by.unwrap_or_else(|| "no rule matched".to_owned()),
            label,
            parts,
            messages: message_views,
            hand_split,
            tags: first.value.tags.clone(),
        }))
    }
}

/// The reason a submission was rejected, if it was.
fn rejection(submission: &tokencontroller_core::Submission) -> Option<String> {
    match &submission.status {
        SubmissionStatus::Rejected { reason } => Some(reason.clone()),
        _ => None,
    }
}

/// The parts of a session, the largest first.
fn ordered_parts(parts: Parts) -> Vec<Part> {
    let mut parts: Vec<Part> = parts
        .into_iter()
        .map(|(tags, (basis_points, closed, amount))| Part {
            basis_points,
            tags,
            closed,
            amount,
        })
        .collect();
    parts.sort_by(|a, b| {
        b.basis_points
            .cmp(&a.basis_points)
            .then_with(|| a.tags.to_string().cmp(&b.tags.to_string()))
    });
    parts
}

/// The sessions of the fold, numbered as the store numbers them.
pub fn sessions(state: &State, numbers: &[crate::store::SessionRow]) -> Result<Vec<SessionView>> {
    let folded = Folded::new(state)?;
    let mut by_session: BTreeMap<String, Vec<&tokencontroller_core::state::Recorded<Message>>> =
        BTreeMap::new();
    for message in state.messages() {
        by_session
            .entry(local_of(&message.value))
            .or_default()
            .push(message);
    }
    let mut views = Vec::new();
    for row in numbers {
        let Some(messages) = by_session.get(&row.local_id) else {
            continue;
        };
        if let Some(view) = folded.view(row, messages)? {
            views.push(view);
        }
    }
    Ok(views)
}

fn local_of(message: &Message) -> String {
    format!("{}:{}", message.agent_harness, message.session_id)
}

/// What wrote a split, as the review list says it: `hand`, `rule: <name>`
/// or `no rule matched`.
fn describe_assignment(
    by: AssignedBy,
    line: &tokencontroller_core::state::Line,
    message: &Message,
    rules: &[&Rule],
    state: &State,
) -> String {
    match by {
        AssignedBy::Hand => "hand".to_owned(),
        AssignedBy::Rule => {
            tokencontroller_core::rules::first_match(rules.iter().copied(), message, state.values())
                .map_or_else(
                    || "rule".to_owned(),
                    |(rule, _)| format!("rule: {}", rule.name),
                )
        }
        AssignedBy::Default => {
            if line.assigned.contains(&TagName::feature())
                && !message.tags.contains(&TagName::feature())
            {
                "rule: key in prompts".to_owned()
            } else if line.assigned.is_empty() {
                "no rule matched".to_owned()
            } else {
                "hand".to_owned()
            }
        }
    }
}

/// Sessions for the tests of the commands: three sessions folded with the
/// local rules and a price file.
#[cfg(test)]
pub mod fixtures {
    use super::*;
    use crate::store::{SessionRow, StoredMessage};
    use crate::tokens::Tokens;
    use time::macros::datetime;

    pub fn stored(session: &str, id: &str, at_ms: i64, branch: &str) -> StoredMessage {
        StoredMessage {
            harness: Harness::ClaudeCode,
            session_id: session.into(),
            message_id: id.into(),
            request_id: None,
            at_ms,
            model: "claude-sonnet-4-5".into(),
            tokens: Tokens {
                input: 1_000_000,
                output: 100_000,
                ..Tokens::default()
            },
            reported_cost: Some(500_000_000),
            branch: Some(branch.into()),
            directory: Some("/w".into()),
            repository: Some("kestrel/storefront".into()),
            subagent: None,
        }
    }

    pub fn prices(dir: &Path) -> std::path::PathBuf {
        let file = dir.join("prices.csv");
        std::fs::write(
            &file,
            "provider,model,kind,price_per_million,valid_from\n\
             anthropic,claude-sonnet-4-5,input,3,2026-01-01\n\
             anthropic,claude-sonnet-4-5,output,15,2026-01-01\n",
        )
        .unwrap();
        file
    }

    pub fn rows(sessions: &[&str]) -> Vec<SessionRow> {
        sessions
            .iter()
            .enumerate()
            .map(|(index, session)| SessionRow {
                local_id: format!("claude-code:{session}"),
                number: i64::try_from(index).unwrap().saturating_add(1),
                harness: Harness::ClaudeCode,
                session_id: (*session).to_owned(),
            })
            .collect()
    }

    /// Three sessions on 2026-04-12: `s1` with the key in its branch and two
    /// messages, `s2` with two keys in its prompts, `s3` split by hand
    /// 60/40 and recorded with a summary. The price file prices Sonnet 4.5
    /// when not connected.
    pub fn folded(
        run: &Run,
        context: &Context,
        connected: bool,
    ) -> (tempfile::TempDir, State, Vec<SessionView>) {
        let dir = tempfile::tempdir().unwrap();
        let file = prices(dir.path());
        let pattern = KeyPattern::new(r"^KD-\d+").unwrap();
        let messages = vec![
            stored("s1", "m1", 1_776_000_020_000, "KD-214-fix"),
            stored("s1", "m2", 1_776_000_030_000, "KD-214-fix"),
            stored("s2", "m3", 1_776_000_040_000, "main"),
            stored("s3", "m4", 1_776_000_050_000, "main"),
        ];
        let keys = BTreeMap::from([(
            "claude-code:s2".to_owned(),
            vec!["KD-1".to_owned(), "KD-2".to_owned()],
        )]);
        let titles = BTreeMap::from([("claude-code:s1".to_owned(), "Fix checkout".to_owned())]);
        let records = BTreeMap::from([(
            "claude-code:s3".to_owned(),
            Recorded {
                summary: Some("By hand".into()),
                facts: Facts {
                    turns: Some(3),
                    ..Facts::default()
                },
                ..Recorded::default()
            },
        )]);
        let hand = Event::new(
            "local-1",
            datetime!(2026-04-12 20:00 UTC),
            Record::Split(Split {
                target: SplitTarget::Session(SessionId::new("s3").unwrap()),
                parts: vec![
                    tokencontroller_core::SplitPart {
                        weight: 60,
                        tags: Tags::parse(["feature=KD-9"]).unwrap(),
                    },
                    tokencontroller_core::SplitPart {
                        weight: 40,
                        tags: Tags::parse(["feature=KD-8"]).unwrap(),
                    },
                ],
            }),
        )
        .unwrap();
        let inputs = Inputs {
            messages: &messages,
            keys: &keys,
            titles: &titles,
            records: &records,
            run,
            context,
            connected,
            pattern: Some(&pattern),
            price_file: Some(&file),
            local_events: vec![hand],
            now: datetime!(2026-10-13 09:00 UTC),
        };
        let state = fold(&inputs).unwrap();
        let views = sessions(&state, &rows(&["s1", "s2", "s3"])).unwrap();
        (dir, state, views)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::Tokens;
    use time::macros::datetime;

    fn stored(session: &str, id: &str, at_ms: i64, branch: &str) -> StoredMessage {
        StoredMessage {
            harness: Harness::ClaudeCode,
            session_id: session.into(),
            message_id: id.into(),
            request_id: None,
            at_ms,
            model: "claude-sonnet-4-5".into(),
            tokens: Tokens {
                input: 1_000_000,
                output: 100_000,
                ..Tokens::default()
            },
            reported_cost: Some(500_000_000),
            branch: Some(branch.into()),
            directory: Some("/w".into()),
            repository: Some("kestrel/storefront".into()),
            subagent: None,
        }
    }

    fn prices(dir: &Path) -> std::path::PathBuf {
        let file = dir.join("prices.csv");
        std::fs::write(
            &file,
            "provider,model,kind,price_per_million,valid_from\n\
             anthropic,claude-sonnet-4-5,input,3,2026-01-01\n\
             anthropic,claude-sonnet-4-5,output,15,2026-01-01\n",
        )
        .unwrap();
        file
    }

    fn rows(sessions: &[&str]) -> Vec<crate::store::SessionRow> {
        sessions
            .iter()
            .enumerate()
            .map(|(index, session)| crate::store::SessionRow {
                local_id: format!("claude-code:{session}"),
                number: i64::try_from(index).unwrap().saturating_add(1),
                harness: Harness::ClaudeCode,
                session_id: (*session).to_owned(),
            })
            .collect()
    }

    #[test]
    fn providers_follow_from_model_names() {
        assert_eq!(provider_of("claude-sonnet-4-5"), "anthropic");
        assert_eq!(provider_of("gpt-5"), "openai");
        assert_eq!(provider_of("o3-mini"), "openai");
        assert_eq!(provider_of("gemini-2.5-pro"), "google");
        assert_eq!(provider_of("llama-3"), "other");
    }

    #[test]
    fn price_files_are_read_and_checked() {
        let dir = tempfile::tempdir().unwrap();
        let file = prices(dir.path());
        let rows = read_price_file(&file).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows.first().unwrap().kind, TokenKind::Input);
        assert_eq!(rows.first().unwrap().per_million.nanos(), 3_000_000_000);
        std::fs::write(
            &file,
            "provider,model,kind,price_per_million,valid_from\nx,y,tokens,1,2026-01-01\n",
        )
        .unwrap();
        assert!(read_price_file(&file).is_err());
        std::fs::write(&file, "provider,model\n").unwrap();
        assert!(read_price_file(&file).is_err());
        std::fs::write(&file, "").unwrap();
        assert!(read_price_file(&file).is_err());
        assert!(read_price_file(&dir.path().join("missing.csv")).is_err());
    }

    #[test]
    fn fractions_and_facts_read_well() {
        assert_eq!(fraction_text(10_000), "1");
        assert_eq!(fraction_text(6_000), "0.6");
        assert_eq!(fraction_text(3_333), "0.3333");
        assert_eq!(fraction_text(0), "0");
        let mut facts = Facts {
            turns: Some(1),
            ..Facts::default()
        };
        facts.merge(Facts {
            duration_seconds: Some(9),
            ..Facts::default()
        });
        assert_eq!(facts.turns, Some(1));
        assert_eq!(facts.duration_seconds, Some(9));
        let mut recorded = Recorded {
            summary: Some("first".into()),
            ..Recorded::default()
        };
        assert!(!recorded.is_empty());
        recorded.merge(Recorded {
            tags: BTreeMap::from([("feature".to_owned(), "KD-2".to_owned())]),
            ..Recorded::default()
        });
        assert_eq!(recorded.summary.as_deref(), Some("first"));
        assert_eq!(
            recorded.tags.get("feature").map(String::as_str),
            Some("KD-2")
        );
        assert!(Recorded::default().is_empty());
    }

    #[test]
    fn run_signals_are_named_and_checked() {
        let mut run = Run::default();
        run.set_signal("pull_request", "12").unwrap();
        run.set_signal("branch", "main").unwrap();
        assert_eq!(run.pull_request().as_deref(), Some("12"));
        assert!(run.set_signal("weather", "sunny").is_err());
        let parsed: RunSignals =
            serde_json::from_str(r#"{"pull_request": 7, "repository": "x"}"#).unwrap();
        assert_eq!(
            pull_request_text(parsed.pull_request.as_ref()).as_deref(),
            Some("7")
        );
        assert_eq!(pull_request_text(Some(&serde_json::Value::Null)), None);
    }

    #[test]
    fn the_log_folds_with_local_rules_prompt_keys_and_hand_splits() {
        let dir = tempfile::tempdir().unwrap();
        let file = prices(dir.path());
        let pattern = KeyPattern::new(r"^KD-\d+").unwrap();
        let messages = vec![
            stored("s1", "m1", 1_776_000_020_000, "KD-214-fix"),
            stored("s1", "m2", 1_776_000_030_000, "KD-214-fix"),
            stored("s2", "m3", 1_776_000_040_000, "main"),
            stored("s3", "m4", 1_776_000_050_000, "main"),
        ];
        let keys = BTreeMap::from([(
            "claude-code:s2".to_owned(),
            vec!["KD-1".to_owned(), "KD-2".to_owned()],
        )]);
        let titles = BTreeMap::from([("claude-code:s1".to_owned(), "Fix checkout".to_owned())]);
        let records = BTreeMap::from([(
            "claude-code:s3".to_owned(),
            Recorded {
                summary: Some("By hand".into()),
                tags: BTreeMap::from([("purpose".to_owned(), "learning".to_owned())]),
                facts: Facts {
                    turns: Some(3),
                    ..Facts::default()
                },
                ..Recorded::default()
            },
        )]);
        let hand = Event::new(
            "local-1",
            datetime!(2026-10-12 09:00 UTC),
            Record::Split(Split {
                target: SplitTarget::Session(SessionId::new("s3").unwrap()),
                parts: vec![
                    tokencontroller_core::SplitPart {
                        weight: 60,
                        tags: Tags::parse(["feature=KD-9"]).unwrap(),
                    },
                    tokencontroller_core::SplitPart {
                        weight: 40,
                        tags: Tags::parse(["feature=KD-8"]).unwrap(),
                    },
                ],
            }),
        )
        .unwrap();
        let context = Context::default();
        let run = Run::default();
        let inputs = Inputs {
            messages: &messages,
            keys: &keys,
            titles: &titles,
            records: &records,
            run: &run,
            context: &context,
            connected: false,
            pattern: Some(&pattern),
            price_file: Some(&file),
            local_events: vec![hand],
            now: datetime!(2026-10-13 09:00 UTC),
        };
        let state = fold(&inputs).unwrap();
        let views = sessions(&state, &rows(&["s1", "s2", "s3"])).unwrap();
        assert_eq!(views.len(), 3);

        let s1 = &views[0];
        assert_eq!(s1.assigned_by, "rule: key in branch");
        assert_eq!(
            s1.parts
                .first()
                .unwrap()
                .tags
                .get_str("feature")
                .map(TagValue::as_str),
            Some("KD-214")
        );
        assert_eq!(s1.cost.unwrap().nanos(), 9_000_000_000);
        assert_eq!(s1.tokens.input, 2_000_000);
        assert_eq!(s1.summary.as_deref(), Some("Fix checkout"));
        assert_eq!(s1.status, Status::Open);
        assert_eq!(s1.label, Label::Direct);
        assert_eq!(s1.parts.first().unwrap().fraction(), "1");
        assert_eq!(s1.reported_cost.unwrap().nanos(), 1_000_000_000);

        let s2 = &views[1];
        assert_eq!(s2.assigned_by, "rule: key in prompts");
        assert_eq!(s2.parts.len(), 2);
        assert_eq!(s2.parts[0].fraction(), "0.5");
        assert_eq!(s2.parts[0].amount.nanos(), 2_250_000_000);

        let s3 = &views[2];
        assert_eq!(s3.assigned_by, "hand");
        assert_eq!(s3.parts[0].fraction(), "0.6");
        assert_eq!(s3.parts[0].amount.nanos(), 2_700_000_000);
        assert_eq!(s3.parts[1].fraction(), "0.4");
        assert_eq!(s3.facts.turns, Some(3));
        assert_eq!(s3.summary.as_deref(), Some("By hand"));
        assert!(s3.hand_split.is_some());
        assert_eq!(s3.on_features().nanos(), 4_500_000_000);
    }

    #[test]
    fn without_prices_the_model_is_unknown_and_run_tags_are_the_default() {
        let messages = vec![stored("s1", "m1", 1_776_000_020_000, "main")];
        let keys = BTreeMap::new();
        let titles = BTreeMap::new();
        let records = BTreeMap::new();
        let context = Context::default();
        let run = Run {
            tags: Tags::parse(["purpose=code-review", "project=shop"]).unwrap(),
            signals: RunSignals {
                repository: Some("github.com/kestrel/ops".into()),
                pull_request: Some(serde_json::json!(12)),
                ..RunSignals::default()
            },
        };
        let inputs = Inputs {
            messages: &messages,
            keys: &keys,
            titles: &titles,
            records: &records,
            run: &run,
            context: &context,
            connected: false,
            pattern: None,
            price_file: None,
            local_events: Vec::new(),
            now: datetime!(2026-10-13 09:00 UTC),
        };
        let state = fold(&inputs).unwrap();
        let views = sessions(&state, &rows(&["s1"])).unwrap();
        let s1 = views.first().unwrap();
        assert_eq!(s1.cost, None);
        assert_eq!(s1.unknown_models, ["anthropic/claude-sonnet-4-5"]);
        assert_eq!(s1.assigned_by, "hand");
        assert_eq!(s1.label, Label::ProjectOverhead);
        assert_eq!(s1.signals.repository.as_deref(), Some("kestrel/ops"));
        assert_eq!(
            s1.signals.resource.get("pull_request").map(String::as_str),
            Some("12")
        );
        assert_eq!(
            s1.parts[0].closed.get_str("purpose").map(TagValue::as_str),
            Some("code-review")
        );
        assert!(sessions(&state, &rows(&["other"])).unwrap().is_empty());
    }

    #[test]
    fn the_local_rules_find_the_key_in_the_branch_then_the_title() {
        let rules = local_rules(&KeyPattern::new(r"^KD-\d+").unwrap());
        let names: Vec<&str> = rules.iter().map(|rule| rule.name.as_str()).collect();
        assert_eq!(names, ["key in branch", "key in PR title"]);
        assert_eq!(rules[0].signal, Signal::Branch);
        assert_eq!(rules[1].signal, Signal::PullRequestTitle);
        assert_eq!(rules[0].scope, Scope::Organization);
        assert!(matches!(&rules[0].then, Then::KeyOf { name } if *name == TagName::feature()));
        assert_eq!(rules[1].pattern.key_in("KD-7: tidy"), Some("KD-7"));
        assert_eq!(rules[1].pattern.key_in("XKD-7"), None);
    }

    #[test]
    fn status_words_are_plain() {
        assert_eq!(Status::Open.as_str(), "open");
        assert_eq!(Status::Submitted.as_str(), "submitted");
        assert_eq!(Status::Rejected("wrong client".into()).as_str(), "rejected");
    }

    #[test]
    fn the_run_tags_ride_with_every_session_and_rules_add_to_them() {
        let run = Run {
            tags: Tags::parse(["purpose=code-review"]).unwrap(),
            ..Run::default()
        };
        let (_dir, _state, views) = super::fixtures::folded(&run, &Context::default(), false);
        let s1 = &views[0];
        assert_eq!(
            s1.tags.get_str("purpose").map(TagValue::as_str),
            Some("code-review")
        );
        assert_eq!(s1.assigned_by, "rule: key in branch");
        let closed = &s1.parts[0].closed;
        assert_eq!(
            closed.get_str("feature").map(TagValue::as_str),
            Some("KD-214")
        );
        assert_eq!(
            closed.get_str("purpose").map(TagValue::as_str),
            Some("code-review")
        );
        assert_eq!(closed.get_str("person").map(TagValue::as_str), Some("me"));
        // A split by hand stands on its own: its parts say all the tags.
        let s3 = &views[2];
        assert_eq!(s3.assigned_by, "hand");
        assert_eq!(
            s3.tags.get_str("purpose").map(TagValue::as_str),
            Some("code-review")
        );
        assert_eq!(s3.parts[1].closed.get_str("purpose"), None);
    }

    #[test]
    fn a_submitted_window_leaves_its_sessions_out_of_the_open_ones() {
        // Submissions are the service's, so they count once connected.
        let context = Context {
            submissions: vec![crate::context::SubmissionLine {
                id: "sub-1".into(),
                from: "2026-04-01".into(),
                to: "2026-04-12".into(),
                status: Some("rejected".into()),
                reason: Some("wrong client".into()),
            }],
            ..Context::default()
        };
        let (_dir, state, views) = super::fixtures::folded(&Run::default(), &context, true);
        assert_eq!(state.submissions().len(), 1);
        assert!(
            views
                .iter()
                .all(|view| view.status == Status::Rejected("wrong client".into()))
        );
        let approved = Context {
            submissions: vec![crate::context::SubmissionLine {
                id: "sub-1".into(),
                from: "2026-04-01".into(),
                to: "2026-04-12".into(),
                status: Some("approved".into()),
                reason: None,
            }],
            ..Context::default()
        };
        let (_dir, _state, views) = super::fixtures::folded(&Run::default(), &approved, true);
        assert!(views.iter().all(|view| view.status == Status::Submitted));
        let before = Context {
            submissions: vec![crate::context::SubmissionLine {
                id: "sub-1".into(),
                from: "2026-04-01".into(),
                to: "2026-04-11".into(),
                status: None,
                reason: None,
            }],
            ..Context::default()
        };
        let (_dir, _state, views) = super::fixtures::folded(&Run::default(), &before, true);
        assert!(views.iter().all(|view| view.status == Status::Open));
    }

    #[test]
    fn connected_the_organization_s_rules_prices_and_currency_apply() {
        let context: Context = serde_json::from_str(
            r#"{
            "organization": {"name": "Kestrel Digital", "currency": "EUR"},
            "tag_names": ["purpose"],
            "tag_values": [{"name": "feature", "key": "KD-214", "defaults": {"project": "storefront"}}],
            "rules": [
                {"name": "key in branch", "scope": "organization", "order": 0, "signal": "branch",
                 "pattern": "(?:^|[^A-Za-z0-9_])(?P<key>KD-\\d+)", "then": {"key_of": {"name": "feature"}}},
                {"name": "storefront work", "scope": "organization", "order": 1, "signal": "repository",
                 "pattern": "^kestrel/storefront$", "then": {"tag": {"name": "purpose", "key": "product"}}}
            ],
            "price_table": [
                {"provider": "anthropic", "model": "claude-sonnet-4-5", "kind": "input", "per_million": "2", "valid_from": "2026-01-01"},
                {"provider": "anthropic", "model": "claude-sonnet-4-5", "kind": "output", "per_million": "10", "valid_from": "2026-01-01"}
            ]
        }"#,
        )
        .unwrap();
        let (_dir, state, views) = super::fixtures::folded(&Run::default(), &context, true);
        assert_eq!(state.currency(), Currency::EUR);
        let s1 = &views[0];
        assert_eq!(s1.assigned_by, "rule: key in branch");
        // 2 messages × (1M × 2 + 100k × 10) per million = 6 EUR, not the file's 9.
        assert_eq!(s1.cost.unwrap().nanos(), 6_000_000_000);
        assert_eq!(s1.cost.unwrap().currency(), Currency::EUR);
        assert_eq!(
            s1.parts[0].closed.get_str("project").map(TagValue::as_str),
            Some("storefront")
        );
        let s2 = &views[1];
        assert_eq!(s2.assigned_by, "rule: storefront work");
        assert_eq!(s2.label, Label::OrganizationOverhead);
    }
}
