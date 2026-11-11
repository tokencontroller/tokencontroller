//! The fold: the log "as of" a moment, and every figure the model answers
//! from it.
//!
//! Every number is computed when you look. [`State::fold`] takes the events
//! recorded at or before the as-of moment, drops the reversed and the
//! superseded ones, and keeps what is in force. From that it answers the
//! formulas of the model:
//!
//! - which messages count: a message whose id a usage endpoint built stops
//!   counting once a message of another source is matched to it, call by
//!   call ([`State::matched`])
//! - the contract of each message, set on the whole message before it is
//!   priced ([`MessageCostLine::contract`])
//! - (1) `C(e)` per message: [`State::message_costs`]
//! - (2) `T*(s)`: the closed tags on every [`Line`], with the contract
//! - (3) `C(F, w)`: [`State::cost`]
//! - (4) partition by one tag name: [`State::by_tag`]
//! - (5, 6) the check of a bill: [`State::reconcile`]
//! - (8) `P(F, w)`: [`State::plan`]
//! - (9) `P(person, w)`: [`State::person_plan`]
//! - (10) `Δ = P − C`: [`State::deviation`]
//! - (11) a forecast: [`State::forecast`]
//! - `can(person, capability, F)`: [`State::can`]

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use time::{Date, Duration, OffsetDateTime};

use crate::access::{self, Capability, Grant};
use crate::bill::{Bill, Reconciliation};
use crate::contract::{ContractAssignment, ContractBy, ContractKind, ContractTerms};
use crate::estimate::{DistributionError, Estimate};
use crate::event::{DateRange, Event, EventId, Record};
use crate::message::{Message, MessageId, ModelRef, SessionId};
use crate::money::{Currency, Money, MoneyError};
use crate::plan::{Share, ShareSum, share_sums};
use crate::price::{CostLabel, MessageCost, PriceTable, date_of};
use crate::rules::{Rule, per_tag_name};
use crate::split::{AssignedBy, Label, Split, SplitError, SplitPart, SplitTarget};
use crate::submit::{Decision, Submission, SubmissionStatus};
use crate::tags::{Filter, TagName, TagValue, Tags};
use crate::tokens::{TokenCounts, TokenKind};
use crate::values::Values;

/// How far apart in time two sources may report the same call and still
/// be matched, and how far apart two sessions may be and still overlap.
const MATCH_WINDOW: Duration = Duration::seconds(30);

/// How far apart two moments are, in nanoseconds.
fn gap(a: OffsetDateTime, b: OffsetDateTime) -> u128 {
    a.unix_timestamp_nanos().abs_diff(b.unix_timestamp_nanos())
}

/// Whether two moments lie within the match window of each other.
fn within_window(a: OffsetDateTime, b: OffsetDateTime) -> bool {
    gap(a, b) <= MATCH_WINDOW.whole_nanoseconds().unsigned_abs()
}

/// A message's key: its session and message ids.
pub type MessageKey = (SessionId, MessageId);

/// A log that cannot be folded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FoldError {
    #[error("event {0} is in the log twice")]
    DuplicateId(EventId),
    #[error("event {event} supersedes {older}, which is not recorded before it")]
    SupersedesLater { event: EventId, older: EventId },
    #[error("event {event}: {error}")]
    Split { event: EventId, error: SplitError },
    #[error("event {event}: {error}")]
    Estimate {
        event: EventId,
        error: DistributionError,
    },
    #[error("event {event}: {error}")]
    Money { event: EventId, error: MoneyError },
}

/// A record with the event that carries it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recorded<T> {
    pub id: EventId,
    #[serde(with = "time::serde::rfc3339")]
    pub recorded_at: OffsetDateTime,
    pub value: T,
}

impl<T: Clone> Recorded<T> {
    fn of(event: &Event, value: &T) -> Self {
        Self {
            id: event.id.clone(),
            recorded_at: event.recorded_at,
            value: value.clone(),
        }
    }
}

/// One part of one message's cost on one tag set: what every report reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    pub session_id: SessionId,
    pub message_id: MessageId,
    pub person: TagValue,
    /// The model, under the provider of the message's contract when it has
    /// one, else the provider the sender reported, if any.
    pub model: ModelRef,
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
    /// The part's place in its split, from 0.
    pub part: usize,
    /// The split's own tags for this part, before defaults.
    pub assigned: Tags,
    /// `T*(s)`: the tags closed under defaults, with the person, their
    /// attributes and the message's contract.
    pub tags: Tags,
    /// `fraction(s) × C(e)`. Zero where the message has no figure; the cost
    /// label says so.
    pub amount: Money,
    pub cost_label: CostLabel,
    pub assigned_by: AssignedBy,
    pub label: Label,
    /// How the message got its contract, if it has one.
    pub contract_by: Option<ContractBy>,
    /// Whether the message's contract is flat: its cost is paid by a fee.
    pub on_a_plan: bool,
}

impl Line {
    /// The message's contract, if it has one.
    #[must_use]
    pub fn contract(&self) -> Option<&TagValue> {
        self.tags.get(&TagName::contract())
    }

    /// The key of the message the line is a part of.
    #[must_use]
    pub fn key(&self) -> MessageKey {
        (self.session_id.clone(), self.message_id.clone())
    }
}

/// `C(F, w)` with what it leaves out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cost {
    pub amount: Money,
    /// Messages in the filter and window with no counts. Their cost is not
    /// in the amount.
    pub count_errors: BTreeSet<MessageKey>,
    /// Messages in the filter and window not priced yet, with their token
    /// counts. Their cost is not in the amount.
    pub not_priced: BTreeMap<MessageKey, TokenCounts>,
    /// The models of the messages not priced yet.
    pub not_priced_models: BTreeSet<ModelRef>,
    /// Token kinds with tokens and no price, left out of the amount.
    pub unpriced_kinds: BTreeSet<TokenKind>,
    /// The part of the amount under a flat contract: paid by a fee.
    pub on_a_plan: Money,
}

/// `Δ(F, w) = P(F, w) − C(F, w)`. Positive is favourable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deviation {
    pub plan: Money,
    pub actual: Cost,
    pub delta: Money,
}

impl Deviation {
    /// Whether the plan covers the actual cost.
    #[must_use]
    pub fn is_favourable(&self) -> bool {
        !self.delta.nanos().is_negative()
    }
}

/// How a deviation is explained. The classifier labels it; the core only
/// carries the label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviationLabel {
    /// The work grew.
    Scope,
    /// The work ran into trouble.
    Trouble,
    /// The estimate itself was off.
    EstimateItself,
}

/// One tag value in `H`, the history the estimator learns from
/// (formula 7 and 12).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagValueHistory {
    pub name: TagName,
    pub key: TagValue,
    pub title: Option<String>,
    pub description: Option<String>,
    pub cost_so_far: Money,
    pub last_activity: Option<Date>,
    pub source_status: Option<String>,
}

/// One counting message's cost as of the fold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageCostLine {
    pub session_id: SessionId,
    pub message_id: MessageId,
    pub person: TagValue,
    /// The model under the provider it was priced at: the contract's
    /// provider when it has a contract, else the one the sender reported.
    pub model: ModelRef,
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
    pub tokens: Option<TokenCounts>,
    pub cost: MessageCost,
    /// The message's contract, if it has one.
    pub contract: Option<TagValue>,
    /// How it got the contract.
    pub contract_by: Option<ContractBy>,
    /// The agent's own figure, kept as a fact.
    pub reported_cost: Option<Money>,
}

/// The log folded as of a moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    as_of: OffsetDateTime,
    prices: PriceTable,
    values: Values,
    messages: BTreeMap<MessageKey, Recorded<Message>>,
    bills: Vec<Recorded<Bill>>,
    estimates: BTreeMap<EventId, Recorded<Estimate>>,
    /// The accepted estimate for each tag set.
    accepts: BTreeMap<Tags, EventId>,
    shares: Vec<Share>,
    splits: BTreeMap<SplitTarget, Recorded<Split>>,
    contract_assignments: BTreeMap<SplitTarget, Recorded<ContractAssignment>>,
    contracts: BTreeMap<TagValue, ContractTerms>,
    grants: Vec<Grant>,
    cutoff: Option<Date>,
    submissions: Vec<Submission>,
    rules: Vec<Recorded<Rule>>,
    /// Built-id messages that stopped counting, with the message they were
    /// matched to.
    matched: BTreeMap<MessageKey, MessageKey>,
    lines: Vec<Line>,
    costs: Vec<MessageCostLine>,
}

impl State {
    /// Folds the log as of a moment: the events recorded at or before it,
    /// less the reversed and the superseded, in the order recorded. Prices
    /// and costs are in `currency`.
    ///
    /// # Errors
    ///
    /// [`FoldError`] when the log cannot be read as one organization: an id
    /// twice, a supersedes pointer to a later event, a split without
    /// weights, an estimate whose bands do not add up, or an amount that
    /// does not fit.
    pub fn fold(
        events: &[Event],
        as_of: OffsetDateTime,
        currency: Currency,
    ) -> Result<Self, FoldError> {
        let mut seen: Vec<&Event> = events
            .iter()
            .filter(|event| event.recorded_at <= as_of)
            .collect();
        seen.sort_by(|a, b| {
            a.recorded_at
                .cmp(&b.recorded_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        let mut ids: BTreeSet<&EventId> = BTreeSet::new();
        for event in &seen {
            if !ids.insert(&event.id) {
                return Err(FoldError::DuplicateId(event.id.clone()));
            }
        }
        let live = live_events(&seen)?;

        let mut state = Self {
            as_of,
            prices: PriceTable::new(currency),
            values: Values::default(),
            messages: BTreeMap::new(),
            bills: Vec::new(),
            estimates: BTreeMap::new(),
            accepts: BTreeMap::new(),
            shares: Vec::new(),
            splits: BTreeMap::new(),
            contract_assignments: BTreeMap::new(),
            contracts: BTreeMap::new(),
            grants: Vec::new(),
            cutoff: None,
            submissions: Vec::new(),
            rules: Vec::new(),
            matched: BTreeMap::new(),
            lines: Vec::new(),
            costs: Vec::new(),
        };
        for event in live {
            state.apply(event)?;
        }
        state.matched = state.match_built_ids();
        state.contracts_of_records_and_bills();
        state.compute_lines()?;
        Ok(state)
    }

    fn apply(&mut self, event: &Event) -> Result<(), FoldError> {
        match &event.record {
            Record::Message(message) => {
                self.messages
                    .insert(message.key(), Recorded::of(event, message));
            }
            Record::Bill(bill) => self.bills.push(Recorded::of(event, bill)),
            Record::Estimate(estimate) => {
                estimate.check().map_err(|error| FoldError::Estimate {
                    event: event.id.clone(),
                    error,
                })?;
                self.estimates
                    .insert(event.id.clone(), Recorded::of(event, estimate));
            }
            Record::TagValue(record) => self.values.insert_tag_value(record.clone()),
            Record::Share(share) => self.shares.push(share.clone()),
            Record::PriceRow(row) => {
                self.prices
                    .insert(row.clone())
                    .map_err(|error| FoldError::Money {
                        event: event.id.clone(),
                        error,
                    })?;
            }
            Record::AttributeValue(value) => self.values.insert_attribute_value(value.clone()),
            Record::PersonAttribute(attribute) => {
                self.values.insert_person_attribute(attribute.clone());
            }
            Record::Split(split) => {
                split.check().map_err(|error| FoldError::Split {
                    event: event.id.clone(),
                    error,
                })?;
                self.splits
                    .insert(split.target.clone(), Recorded::of(event, split));
            }
            Record::ContractAssignment(assignment) => {
                self.contract_assignments
                    .insert(assignment.target.clone(), Recorded::of(event, assignment));
            }
            Record::Accept(accept) => {
                if let Some(estimate) = self.estimates.get(&accept.estimate) {
                    self.accepts
                        .insert(estimate.value.tags.clone(), accept.estimate.clone());
                }
            }
            Record::Grant(grant) => self.grants.push(grant.clone()),
            Record::Cutoff(date) => self.cutoff = Some(*date),
            Record::Submit(submit) => {
                self.submissions.retain(|existing| {
                    existing.person != submit.person || existing.window != submit.window
                });
                self.submissions.push(Submission {
                    id: event.id.clone(),
                    person: submit.person.clone(),
                    window: submit.window,
                    status: SubmissionStatus::Submitted,
                });
            }
            Record::Approval(approval) => {
                if let Some(submission) = self
                    .submissions
                    .iter_mut()
                    .find(|submission| submission.id == approval.submission)
                {
                    submission.status = match &approval.decision {
                        Decision::Approved => SubmissionStatus::Approved,
                        Decision::Rejected { reason } => SubmissionStatus::Rejected {
                            reason: reason.clone(),
                        },
                    };
                }
            }
            Record::Rule(rule) => self.rules.push(Recorded::of(event, rule)),
            Record::Activity(_) | Record::Reversal(_) => {}
        }
        Ok(())
    }

    /// Matches each message whose id a usage endpoint built to a message of
    /// another source for the same call: same person, same model, the same
    /// counts however finely the cache writes are told, and times within
    /// [`MATCH_WINDOW`]. It looks inside the message's own session first;
    /// only a session with no other source's messages looks at the same
    /// person's other sessions, which the time window keeps to those that
    /// overlap it. Session totals are never
    /// used. Each message is matched once, the closest in time first. A
    /// built message with no match keeps counting: it may be a call only
    /// that source sees.
    fn match_built_ids(&self) -> BTreeMap<MessageKey, MessageKey> {
        let others: Vec<&Message> = self
            .messages
            .values()
            .map(|message| &message.value)
            .filter(|message| !message.has_built_id())
            .collect();
        let sessions_with_others: BTreeSet<&SessionId> =
            others.iter().map(|message| &message.session_id).collect();
        let mut taken: BTreeSet<MessageKey> = BTreeSet::new();
        let mut matched = BTreeMap::new();
        for built in self
            .messages
            .values()
            .map(|message| &message.value)
            .filter(|message| message.has_built_id())
        {
            let Some(counts) = built.tokens.as_ref().map(TokenCounts::compared) else {
                continue;
            };
            let own_session = sessions_with_others.contains(&built.session_id);
            let best = others
                .iter()
                .filter(|other| {
                    other.person == built.person
                        && (!own_session || other.session_id == built.session_id)
                        && other.model.model == built.model.model
                        && match (&other.model.provider, &built.model.provider) {
                            (Some(a), Some(b)) => a == b,
                            _ => true,
                        }
                        && other.tokens.as_ref().map(TokenCounts::compared).as_ref()
                            == Some(&counts)
                        && within_window(other.at, built.at)
                        && !taken.contains(&other.key())
                })
                .min_by_key(|other| (gap(other.at, built.at), other.key()));
            if let Some(other) = best {
                taken.insert(other.key());
                matched.insert(built.key(), other.key());
            }
        }
        matched
    }

    /// The contracts that tag value records and bills bring. A bill creates
    /// its contract when no contract of that key exists. A bill that names
    /// none goes to its provider's only contract, else to one named after
    /// the provider. Bills that name their contract are read first.
    fn contracts_of_records_and_bills(&mut self) {
        for record in self.values.tag_values() {
            if record.name == TagName::contract()
                && let Some(terms) = &record.contract
            {
                self.contracts.insert(record.key.clone(), terms.clone());
            }
        }
        let mut bills = std::mem::take(&mut self.bills);
        bills.sort_by_key(|bill| bill.value.contract.is_none());
        for bill in &mut bills {
            let key = self.contract_of_bill(&bill.value);
            self.contracts
                .entry(key.clone())
                .or_insert_with(|| ContractTerms {
                    provider: bill.value.provider.clone(),
                    kind: bill.value.contract_kind.unwrap_or(ContractKind::Metered),
                    fee: None,
                });
            bill.value.contract = Some(key);
        }
        bills.sort_by(|a, b| {
            a.recorded_at
                .cmp(&b.recorded_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        self.bills = bills;
    }

    /// The contract a bill is for: the one it names; else, for a bill read
    /// in this fold, the one it went to; else its provider's only contract;
    /// else a contract named after the provider.
    #[must_use]
    pub fn contract_of_bill(&self, bill: &Bill) -> TagValue {
        if let Some(contract) = &bill.contract {
            return contract.clone();
        }
        if let Some(read) = self.bills.iter().find(|read| {
            read.value.provider == bill.provider
                && read.value.range == bill.range
                && read.value.amount == bill.amount
        }) && let Some(contract) = &read.value.contract
        {
            return contract.clone();
        }
        let mut own = self
            .contracts
            .iter()
            .filter(|(_, terms)| terms.provider == bill.provider);
        if let (Some((only, _)), None) = (own.next(), own.next()) {
            return only.clone();
        }
        TagValue::new(&bill.provider).unwrap_or_else(|_| TagValue::known("unnamed"))
    }

    /// The rules in force for a tag name on a message: a rule recorded by
    /// the time the message arrived, or one that runs back over it. Rules
    /// run back since the cutoff; a rule on `contract` runs back before it
    /// too, since figures as of the cutoff stay as they were.
    fn rules_for(&self, message: &Recorded<Message>) -> impl Iterator<Item = &Rule> {
        let since_cutoff = self
            .cutoff
            .is_none_or(|cutoff| date_of(message.value.at) >= cutoff);
        self.rules
            .iter()
            .filter(move |rule| {
                rule.recorded_at <= message.recorded_at
                    || since_cutoff
                    || *rule.value.gives() == TagName::contract()
            })
            .map(|rule| &rule.value)
    }

    /// The split of a message: by hand on the message or its session; else
    /// the sender's own tags, with the rules filling in the tag names the
    /// sender left out, one rule per tag name; else the default. The
    /// contract is not part of a split.
    fn split_of(&self, message: &Recorded<Message>) -> (Vec<SplitPart>, AssignedBy) {
        let by_message = SplitTarget::Message {
            session_id: message.value.session_id.clone(),
            message_id: message.value.message_id.clone(),
        };
        let by_session = SplitTarget::Session(message.value.session_id.clone());
        if let Some(split) = self
            .splits
            .get(&by_message)
            .or_else(|| self.splits.get(&by_session))
        {
            return (split.value.parts.clone(), AssignedBy::Hand);
        }
        let contract = TagName::contract();
        let sent = message.value.tags.retain(|name| *name != contract);
        let ruled: Tags = per_tag_name(self.rules_for(message), &message.value, &self.values)
            .into_iter()
            .filter(|(name, _)| *name != contract && !sent.contains(name))
            .map(|(name, (_, key))| (name, key))
            .collect();
        let assigned_by = if ruled.is_empty() {
            AssignedBy::Default
        } else {
            AssignedBy::Rule
        };
        (
            self.default_parts(&message.value, &sent.over(&ruled)),
            assigned_by,
        )
    }

    /// The default split over `tags`: one part; when they name no feature
    /// and the session names several, equal shares over them.
    fn default_parts(&self, message: &Message, tags: &Tags) -> Vec<SplitPart> {
        let feature = TagName::feature();
        let mut named: Vec<TagValue> = Vec::new();
        if !tags.contains(&feature) {
            for key in &message.signals.prompt_keys {
                let Ok(key) = TagValue::new(key) else {
                    continue;
                };
                if self.values.is_tag_value(&feature, &key) && !named.contains(&key) {
                    named.push(key);
                }
            }
        }
        if named.is_empty() {
            return vec![SplitPart {
                weight: 1,
                tags: tags.clone(),
            }];
        }
        named
            .into_iter()
            .map(|key| {
                let mut part = tags.clone();
                part.insert(feature.clone(), key);
                SplitPart {
                    weight: 1,
                    tags: part,
                }
            })
            .collect()
    }

    /// The contract set by hand on the message or its session, by the
    /// sender, or by a rule: the organization's, then the one we ship on
    /// how the agent was signed in. A rule that creates its contract does
    /// so here, the first time it gives it.
    fn contract_before_defaults(
        &mut self,
        message: &Recorded<Message>,
    ) -> Option<(TagValue, ContractBy)> {
        let by_message = SplitTarget::Message {
            session_id: message.value.session_id.clone(),
            message_id: message.value.message_id.clone(),
        };
        let by_session = SplitTarget::Session(message.value.session_id.clone());
        if let Some(assignment) = self
            .contract_assignments
            .get(&by_message)
            .or_else(|| self.contract_assignments.get(&by_session))
        {
            return Some((assignment.value.contract.clone(), ContractBy::Hand));
        }
        if let Some(sent) = message.value.tags.get(&TagName::contract()) {
            return Some((sent.clone(), ContractBy::Sender));
        }
        let (key, terms) = {
            let given = per_tag_name(self.rules_for(message), &message.value, &self.values);
            if let Some((rule, key)) = given.get(&TagName::contract()) {
                (key.clone(), rule.contract.clone())
            } else {
                let (key, terms) = crate::rules::signed_in_contract(&message.value)?;
                (key, Some(terms))
            }
        };
        if let Some(terms) = terms {
            self.contracts.entry(key.clone()).or_insert(terms);
        }
        Some((key, ContractBy::Rule))
    }

    /// The contract every split part's closed tags bring, when they all
    /// bring the same one; else the provider's only contract.
    fn contract_by_default(
        &self,
        message: &Recorded<Message>,
        parts: &[SplitPart],
    ) -> Option<(TagValue, ContractBy)> {
        let contract = TagName::contract();
        let day = date_of(message.value.at);
        let brought: BTreeSet<Option<TagValue>> = parts
            .iter()
            .map(|part| {
                self.values
                    .close_on(
                        &part.tags.retain(|name| *name != contract),
                        &message.value.person,
                        day,
                    )
                    .get(&contract)
                    .cloned()
            })
            .collect();
        let mut brought = brought.into_iter();
        if let (Some(Some(key)), None) = (brought.next(), brought.next()) {
            return Some((key, ContractBy::ProjectDefault));
        }
        let provider = message.value.model.provider.as_deref()?;
        let mut own = self
            .contracts
            .iter()
            .filter(|(_, terms)| terms.provider == provider);
        match (own.next(), own.next()) {
            (Some((only, _)), None) => Some((only.clone(), ContractBy::Fallback)),
            _ => None,
        }
    }

    fn compute_lines(&mut self) -> Result<(), FoldError> {
        let counting: Vec<Recorded<Message>> = self
            .messages
            .iter()
            .filter(|(key, _)| !self.matched.contains_key(*key))
            .map(|(_, message)| message.clone())
            .collect();
        // Hand, sender and rules first, so the contracts rules create are
        // there before any fallback counts a provider's contracts.
        let mut first: Vec<Option<(TagValue, ContractBy)>> = Vec::with_capacity(counting.len());
        for message in &counting {
            first.push(self.contract_before_defaults(message));
        }
        let currency = self.prices.currency();
        let mut lines = Vec::new();
        let mut costs = Vec::new();
        for (message, contract) in counting.iter().zip(first) {
            let (parts, assigned_by) = self.split_of(message);
            let contract = contract.or_else(|| self.contract_by_default(message, &parts));
            let (cost_line, terms) = self.cost_line(message, contract)?;
            let weights: Vec<u64> = parts.iter().map(|part| part.weight).collect();
            let amounts = cost_line
                .cost
                .amount_or_zero(currency)
                .allocate(&weights)
                .map_err(|error| FoldError::Money {
                    event: message.id.clone(),
                    error,
                })?;
            for (index, (part, amount)) in parts.iter().zip(amounts).enumerate() {
                let tags = self.closed_tags(message, part, cost_line.contract.as_ref());
                lines.push(Line {
                    session_id: message.value.session_id.clone(),
                    message_id: message.value.message_id.clone(),
                    person: message.value.person.clone(),
                    model: cost_line.model.clone(),
                    at: message.value.at,
                    part: index,
                    assigned: part.tags.clone(),
                    label: Label::of(&tags),
                    tags,
                    amount,
                    cost_label: cost_line.cost.label,
                    assigned_by,
                    contract_by: cost_line.contract_by,
                    on_a_plan: terms.is_some_and(|kind| kind == ContractKind::Flat),
                });
            }
            costs.push(cost_line);
        }
        self.lines = lines;
        self.costs = costs;
        Ok(())
    }

    /// `C(e)` of a message under its contract: priced at the contract's
    /// provider and the contract's own rows when it has a known contract,
    /// else at the provider the sender reported.
    fn cost_line(
        &self,
        message: &Recorded<Message>,
        resolved: Option<(TagValue, ContractBy)>,
    ) -> Result<(MessageCostLine, Option<ContractKind>), FoldError> {
        let (contract, contract_by) =
            resolved.map_or((None, None), |(key, by)| (Some(key), Some(by)));
        let terms = contract.as_ref().and_then(|key| self.contracts.get(key));
        let model = match terms {
            Some(terms) => message.value.model.under(&terms.provider),
            None => message.value.model.clone(),
        };
        let cost = self
            .prices
            .cost_of(
                message.value.tokens.as_ref(),
                model.provider.as_deref(),
                &model.model,
                message.value.at,
                contract.as_ref(),
            )
            .map_err(|error| FoldError::Money {
                event: message.id.clone(),
                error,
            })?;
        Ok((
            MessageCostLine {
                session_id: message.value.session_id.clone(),
                message_id: message.value.message_id.clone(),
                person: message.value.person.clone(),
                model,
                at: message.value.at,
                tokens: message.value.tokens.clone(),
                cost,
                contract,
                contract_by,
                reported_cost: message.value.reported_cost,
            },
            terms.map(|terms| terms.kind),
        ))
    }

    /// `T*(s)` of one part: its tags closed under defaults, with the
    /// message's contract and no other. A default that brings a contract
    /// the message did not get is left out.
    fn closed_tags(
        &self,
        message: &Recorded<Message>,
        part: &SplitPart,
        contract: Option<&TagValue>,
    ) -> Tags {
        let name = TagName::contract();
        let mut own = part.tags.retain(|tag| *tag != name);
        if let Some(contract) = contract {
            own.insert(name.clone(), contract.clone());
        }
        let mut closed =
            self.values
                .close_on(&own, &message.value.person, date_of(message.value.at));
        if contract.is_none() {
            closed.remove(&name);
        }
        closed
    }

    /// The moment the fold is as of. Reports show it.
    #[must_use]
    pub const fn as_of(&self) -> OffsetDateTime {
        self.as_of
    }

    /// The currency of every figure.
    #[must_use]
    pub const fn currency(&self) -> Currency {
        self.prices.currency()
    }

    /// The cutoff in force, if one was set.
    #[must_use]
    pub const fn cutoff(&self) -> Option<Date> {
        self.cutoff
    }

    /// The moment the frozen figures are as of: the start of the cutoff
    /// date in UTC. `None` without a cutoff.
    #[must_use]
    pub fn frozen_as_of(&self) -> Option<OffsetDateTime> {
        self.cutoff.map(|date| date.midnight().assume_utc())
    }

    #[must_use]
    pub const fn prices(&self) -> &PriceTable {
        &self.prices
    }

    #[must_use]
    pub const fn values(&self) -> &Values {
        &self.values
    }

    /// Every message in force, by session and message id, counting or not.
    pub fn messages(&self) -> impl Iterator<Item = &Recorded<Message>> {
        self.messages.values()
    }

    /// Messages whose id a usage endpoint built and that stopped counting,
    /// each with the message of another source it was matched to.
    #[must_use]
    pub const fn matched(&self) -> &BTreeMap<MessageKey, MessageKey> {
        &self.matched
    }

    /// Every contract in force with its terms: from tag value records,
    /// from bills, and from rules that created theirs.
    #[must_use]
    pub const fn contracts(&self) -> &BTreeMap<TagValue, ContractTerms> {
        &self.contracts
    }

    /// `C(e)` for every counting message.
    #[must_use]
    pub fn message_costs(&self) -> &[MessageCostLine] {
        &self.costs
    }

    /// Every line: one part of one counting message on one closed tag set.
    #[must_use]
    pub fn lines(&self) -> &[Line] {
        &self.lines
    }

    /// The bills in force, each with the contract it went to.
    pub fn bills(&self) -> impl Iterator<Item = &Recorded<Bill>> {
        self.bills.iter()
    }

    /// The estimates in force, by event id.
    pub fn estimates(&self) -> impl Iterator<Item = &Recorded<Estimate>> {
        self.estimates.values()
    }

    /// The accepted estimate for a tag set, if one is accepted.
    #[must_use]
    pub fn accepted(&self, tags: &Tags) -> Option<&Recorded<Estimate>> {
        self.accepts.get(tags).and_then(|id| self.estimates.get(id))
    }

    #[must_use]
    pub fn shares(&self) -> &[Share] {
        &self.shares
    }

    /// The splits recorded by hand, by target.
    pub fn splits(&self) -> impl Iterator<Item = &Recorded<Split>> {
        self.splits.values()
    }

    /// The contracts set by hand, by target.
    pub fn contract_assignments(&self) -> impl Iterator<Item = &Recorded<ContractAssignment>> {
        self.contract_assignments.values()
    }

    #[must_use]
    pub fn grants(&self) -> &[Grant] {
        &self.grants
    }

    #[must_use]
    pub fn submissions(&self) -> &[Submission] {
        &self.submissions
    }

    /// The submission a message of a person at a moment is inside, if any.
    #[must_use]
    pub fn submission_of(&self, person: &TagValue, at: OffsetDateTime) -> Option<&Submission> {
        self.submissions
            .iter()
            .find(|submission| submission.covers(person, at))
    }

    pub fn rules(&self) -> impl Iterator<Item = &Recorded<Rule>> {
        self.rules.iter()
    }

    /// The lines a filter and a window select: `F ⊆ T*(s)` and `t(e) ∈ w`.
    pub fn select<'a>(
        &'a self,
        filter: &'a Filter,
        window: DateRange,
    ) -> impl Iterator<Item = &'a Line> {
        self.lines
            .iter()
            .filter(move |line| filter.matches(&line.tags) && window.contains_moment(line.at))
    }

    fn sum<'a>(&self, lines: impl Iterator<Item = &'a Line>) -> Result<Cost, MoneyError> {
        let mut cost = Cost {
            amount: Money::zero(self.currency()),
            count_errors: BTreeSet::new(),
            not_priced: BTreeMap::new(),
            not_priced_models: BTreeSet::new(),
            unpriced_kinds: BTreeSet::new(),
            on_a_plan: Money::zero(self.currency()),
        };
        let mut seen: BTreeSet<MessageKey> = BTreeSet::new();
        for line in lines {
            cost.amount = cost.amount.checked_add(line.amount)?;
            if line.on_a_plan {
                cost.on_a_plan = cost.on_a_plan.checked_add(line.amount)?;
            }
            if !seen.insert(line.key()) {
                continue;
            }
            match line.cost_label {
                CostLabel::Priced => {}
                CostLabel::CountError => {
                    cost.count_errors.insert(line.key());
                }
                CostLabel::NotPricedYet => {
                    cost.not_priced_models.insert(line.model.clone());
                }
            }
            if let Some(message) = self
                .costs
                .binary_search_by(|cost_line| {
                    (&cost_line.session_id, &cost_line.message_id)
                        .cmp(&(&line.session_id, &line.message_id))
                })
                .ok()
                .and_then(|index| self.costs.get(index))
            {
                cost.unpriced_kinds
                    .extend(message.cost.unpriced_kinds.iter().cloned());
                if line.cost_label == CostLabel::NotPricedYet {
                    cost.not_priced
                        .insert(line.key(), message.tokens.clone().unwrap_or_default());
                }
            }
        }
        Ok(cost)
    }

    /// `C(F, w)`: the only formula reports use.
    ///
    /// # Errors
    ///
    /// [`MoneyError::Overflow`] if the sum does not fit.
    pub fn cost(&self, filter: &Filter, window: DateRange) -> Result<Cost, MoneyError> {
        self.sum(self.select(filter, window))
    }

    /// `C(F, w)` partitioned by the values of one tag name. The `None` line
    /// holds the lines without the tag, so the lines add up to `C(F, w)`.
    ///
    /// # Errors
    ///
    /// [`MoneyError::Overflow`] if a sum does not fit.
    pub fn by_tag(
        &self,
        filter: &Filter,
        window: DateRange,
        by: &TagName,
    ) -> Result<Vec<(Option<TagValue>, Money)>, MoneyError> {
        let mut groups: BTreeMap<Option<TagValue>, Money> = BTreeMap::new();
        for line in self.select(filter, window) {
            let group = groups
                .entry(line.tags.get(by).cloned())
                .or_insert_with(|| Money::zero(self.currency()));
            *group = group.checked_add(line.amount)?;
        }
        Ok(groups.into_iter().collect())
    }

    /// `C(F, w)` by derived label: direct, project overhead, organization
    /// overhead. Every label is listed.
    ///
    /// # Errors
    ///
    /// [`MoneyError::Overflow`] if a sum does not fit.
    pub fn by_label(
        &self,
        filter: &Filter,
        window: DateRange,
    ) -> Result<Vec<(Label, Money)>, MoneyError> {
        let mut groups: BTreeMap<Label, Money> = Label::ALL
            .iter()
            .map(|label| (*label, Money::zero(self.currency())))
            .collect();
        for line in self.select(filter, window) {
            if let Some(group) = groups.get_mut(&line.label) {
                *group = group.checked_add(line.amount)?;
            }
        }
        Ok(groups.into_iter().collect())
    }

    /// `C(F, w)` by the kind of the messages' contracts: metered, flat (on a
    /// plan) and no contract, always in that order. A contract with no
    /// known terms counts as metered.
    ///
    /// # Errors
    ///
    /// [`MoneyError::Overflow`] if a sum does not fit.
    pub fn by_contract_kind(
        &self,
        filter: &Filter,
        window: DateRange,
    ) -> Result<Vec<(Option<ContractKind>, Money)>, MoneyError> {
        let zero = Money::zero(self.currency());
        let mut groups = [
            (Some(ContractKind::Metered), zero),
            (Some(ContractKind::Flat), zero),
            (None, zero),
        ];
        for line in self.select(filter, window) {
            let kind = match (line.contract(), line.on_a_plan) {
                (None, _) => None,
                (Some(_), true) => Some(ContractKind::Flat),
                (Some(_), false) => Some(ContractKind::Metered),
            };
            if let Some((_, group)) = groups.iter_mut().find(|(group, _)| *group == kind) {
                *group = group.checked_add(line.amount)?;
            }
        }
        Ok(groups.into())
    }

    /// `C({provider}, r)`: what the messages priced under one provider in a
    /// range cost, whatever their contract.
    ///
    /// # Errors
    ///
    /// [`MoneyError::Overflow`] if the sum does not fit.
    pub fn provider_cost(&self, provider: &str, range: DateRange) -> Result<Cost, MoneyError> {
        self.sum(self.lines.iter().filter(|line| {
            line.model.provider.as_deref() == Some(provider) && range.contains_moment(line.at)
        }))
    }

    /// The check of one bill on its own range, against the messages with
    /// its contract, with the messages of the same provider that have no
    /// contract and those whose contract came from the fallback beside it.
    /// A flat contract's fee comes from the bill, else from the contract
    /// when the bill is for one whole month.
    ///
    /// # Errors
    ///
    /// [`MoneyError::CurrencyMismatch`] if the bill is in another currency
    /// than the prices, or [`MoneyError::Overflow`].
    pub fn reconcile(&self, bill: &Bill) -> Result<Reconciliation, MoneyError> {
        let contract = self.contract_of_bill(bill);
        let terms = self.contracts.get(&contract);
        let provider = terms.map_or(bill.provider.as_str(), |terms| terms.provider.as_str());
        let in_range = |line: &&Line| bill.range.contains_moment(line.at);
        let zero = Money::zero(self.currency());
        let (mut messages, mut fallback, mut no_contract) = (zero, zero, zero);
        for line in self.lines.iter().filter(in_range) {
            match line.contract() {
                Some(own) if *own == contract => {
                    messages = messages.checked_add(line.amount)?;
                    if line.contract_by == Some(ContractBy::Fallback) {
                        fallback = fallback.checked_add(line.amount)?;
                    }
                }
                None if line.model.provider.as_deref() == Some(provider) => {
                    no_contract = no_contract.checked_add(line.amount)?;
                }
                _ => {}
            }
        }
        let beside = (no_contract, fallback);
        let kind = terms
            .map(|terms| terms.kind)
            .or(bill.contract_kind)
            .unwrap_or(ContractKind::Metered);
        match kind {
            ContractKind::Metered => {
                Reconciliation::metered(contract, bill.amount, messages, beside)
            }
            ContractKind::Flat => {
                let fee = bill.fee.or_else(|| {
                    terms
                        .and_then(|terms| terms.fee)
                        .filter(|_| bill.is_full_month())
                });
                Reconciliation::flat(
                    contract,
                    bill.amount,
                    messages,
                    (fee, bill.extra_usage),
                    beside,
                )
            }
        }
    }

    /// `P(F, w)`: the accepted estimate for `F` spread over `w`, or the
    /// person's plan from shares when `F` is one person. `None` when
    /// nothing is planned for `F`.
    ///
    /// # Errors
    ///
    /// [`MoneyError::Overflow`] if an amount does not fit.
    pub fn plan(&self, filter: &Filter, window: DateRange) -> Result<Option<Money>, MoneyError> {
        if let Some(person) = filter.0.get(&TagName::person())
            && filter.0.len() == 1
        {
            return self.person_plan(person, window).map(Some);
        }
        match self.accepted(&filter.0) {
            Some(estimate) => estimate.value.plan(window).map(Some),
            None => Ok(None),
        }
    }

    /// `P(q, w) = Σ over shares of q: fraction × P(project, w ∩ [from, to])`.
    ///
    /// # Errors
    ///
    /// [`MoneyError::Overflow`] if an amount does not fit.
    pub fn person_plan(&self, person: &TagValue, window: DateRange) -> Result<Money, MoneyError> {
        let mut total = Money::zero(self.currency());
        for share in self.shares.iter().filter(|share| share.person == *person) {
            let Some(days) = share.days_in(window) else {
                continue;
            };
            let mut project = Tags::new();
            project.insert(TagName::project(), share.project.clone());
            let Some(estimate) = self.accepted(&project) else {
                continue;
            };
            let project_plan = estimate.value.plan(days)?;
            total = total.checked_add(share.part_of(project_plan)?)?;
        }
        Ok(total)
    }

    /// The fractions of every project's shares in force on a date. A sum
    /// above 1 is shown, never corrected.
    #[must_use]
    pub fn share_sums(&self, on: Date) -> Vec<ShareSum> {
        share_sums(&self.shares, on)
    }

    /// `Δ(F, w) = P(F, w) − C(F, w)`. `None` when nothing is planned for
    /// `F`.
    ///
    /// # Errors
    ///
    /// [`MoneyError::Overflow`] if an amount does not fit.
    pub fn deviation(
        &self,
        filter: &Filter,
        window: DateRange,
    ) -> Result<Option<Deviation>, MoneyError> {
        let Some(plan) = self.plan(filter, window)? else {
            return Ok(None);
        };
        let actual = self.cost(filter, window)?;
        let delta = plan.checked_sub(actual.amount)?;
        Ok(Some(Deviation {
            plan,
            actual,
            delta,
        }))
    }

    /// A forecast for the next window: the cost per day over the past
    /// windows times the days of the next one. A report, not a record.
    ///
    /// # Errors
    ///
    /// [`MoneyError::Overflow`] if an amount does not fit.
    pub fn forecast(
        &self,
        filter: &Filter,
        past: &[DateRange],
        next: DateRange,
    ) -> Result<Money, MoneyError> {
        let mut total = Money::zero(self.currency());
        let mut days = 0_u64;
        for window in past {
            total = total.checked_add(self.cost(filter, *window)?.amount)?;
            days = days.saturating_add(window.days());
        }
        if days == 0 {
            return Ok(total);
        }
        total.checked_mul_ratio(next.days(), days)
    }

    /// `can(person, capability, F)` over the grants in force.
    #[must_use]
    pub fn can(&self, person: &TagValue, capability: Capability, filter: &Filter) -> bool {
        access::can(&self.grants, person, capability, filter)
    }

    /// `H`: every tag value with its cost so far, last activity date and
    /// source status. The estimator learns from it.
    ///
    /// # Errors
    ///
    /// [`MoneyError::Overflow`] if a sum does not fit.
    pub fn history(&self) -> Result<Vec<TagValueHistory>, MoneyError> {
        let mut history = Vec::new();
        for record in self.values.tag_values() {
            let mut cost_so_far = Money::zero(self.currency());
            let mut last_activity: Option<Date> = None;
            for line in self
                .lines
                .iter()
                .filter(|line| line.tags.get(&record.name) == Some(&record.key))
            {
                cost_so_far = cost_so_far.checked_add(line.amount)?;
                let date = date_of(line.at);
                last_activity = Some(last_activity.map_or(date, |last| last.max(date)));
            }
            history.push(TagValueHistory {
                name: record.name.clone(),
                key: record.key.clone(),
                title: record.title.clone(),
                description: record.description.clone(),
                cost_so_far,
                last_activity,
                source_status: record.source_status.clone(),
            });
        }
        Ok(history)
    }
}

/// The events in force: not reversed, and not superseded by an event in
/// force. `seen` is in the order recorded; a supersedes pointer must name
/// an earlier event.
fn live_events<'a>(seen: &[&'a Event]) -> Result<Vec<&'a Event>, FoldError> {
    let mut position: BTreeMap<&EventId, usize> = BTreeMap::new();
    for (index, event) in seen.iter().enumerate() {
        position.insert(&event.id, index);
    }
    let reversed: BTreeSet<&EventId> = seen
        .iter()
        .filter_map(|event| match &event.record {
            Record::Reversal(target) => Some(target),
            _ => None,
        })
        .collect();
    let mut superseded: BTreeSet<&EventId> = BTreeSet::new();
    let mut live: Vec<&Event> = Vec::new();
    for (index, event) in seen.iter().enumerate().rev() {
        if let Some(older) = &event.supersedes
            && position.get(older).is_none_or(|at| *at >= index)
        {
            return Err(FoldError::SupersedesLater {
                event: event.id.clone(),
                older: older.clone(),
            });
        }
        if reversed.contains(&event.id) || superseded.contains(&event.id) {
            continue;
        }
        if let Some(older) = &event.supersedes {
            superseded.insert(older);
        }
        live.push(event);
    }
    live.reverse();
    Ok(live)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::estimate::{Accept, Distribution};
    use crate::message::{SessionFacts, Signals};
    use crate::pattern::TextPattern;
    use crate::price::{PriceRow, PriceSource};
    use crate::rules::{Scope, Signal, Then};
    use crate::submit::{Approval, Submit};
    use crate::tokens::{TokenCounts, TokenKind};
    use crate::values::{AttributeValue, PersonAttribute, TagValueRecord};
    use time::macros::{date, datetime};

    fn usd(text: &str) -> Money {
        Money::parse(text, Currency::USD).unwrap()
    }

    fn name(text: &str) -> TagName {
        TagName::new(text).unwrap()
    }

    fn key(text: &str) -> TagValue {
        TagValue::new(text).unwrap()
    }

    fn tags(pairs: &[&str]) -> Tags {
        Tags::parse(pairs.iter().copied()).unwrap()
    }

    fn filter(pairs: &[&str]) -> Filter {
        Filter(tags(pairs))
    }

    fn march() -> DateRange {
        DateRange::new(date!(2026 - 03 - 01), date!(2026 - 03 - 31)).unwrap()
    }

    const JANUARY: OffsetDateTime = datetime!(2026-01-01 00:00 UTC);
    const APRIL: OffsetDateTime = datetime!(2026-04-05 00:00 UTC);

    fn event(id: &str, recorded_at: OffsetDateTime, record: Record) -> Event {
        Event::new(id, recorded_at, record).unwrap()
    }

    fn tag_value(name_text: &str, key_text: &str, defaults: &[&str]) -> Record {
        Record::TagValue(TagValueRecord {
            name: name(name_text),
            key: key(key_text),
            title: None,
            description: None,
            source_id: None,
            source_status: None,
            defaults: tags(defaults),
            contract: None,
        })
    }

    fn price_row(kind: TokenKind, price: &str) -> Record {
        Record::PriceRow(PriceRow {
            provider: "anthropic".to_owned(),
            model: "claude-sonnet-4-5".to_owned(),
            kind,
            per_million: usd(price),
            valid_from: date!(2026 - 01 - 01),
            source: PriceSource::Managed,
            contract: None,
        })
    }

    fn message(
        session: &str,
        id: &str,
        person: &str,
        at: OffsetDateTime,
        input_millions: u64,
    ) -> Message {
        Message {
            session_id: SessionId::new(session).unwrap(),
            message_id: MessageId::new(id).unwrap(),
            person: key(person),
            model: ModelRef::new("anthropic", "claude-sonnet-4-5"),
            tokens: Some(
                TokenCounts::new().with(TokenKind::INPUT, input_millions.saturating_mul(1_000_000)),
            ),
            at,
            agent_harness: "claude-code".to_owned(),
            agent_name: None,
            reported_cost: None,
            tags: Tags::new(),
            signals: Signals::default(),
            session: SessionFacts::default(),
            received: None,
        }
    }

    fn split(target: SplitTarget, parts: &[(u64, &[&str])]) -> Record {
        Record::Split(Split {
            target,
            parts: parts
                .iter()
                .map(|(weight, pairs)| SplitPart {
                    weight: *weight,
                    tags: tags(pairs),
                })
                .collect(),
        })
    }

    fn session(id: &str) -> SplitTarget {
        SplitTarget::Session(SessionId::new(id).unwrap())
    }

    fn estimate(tags_text: &[&str], median: &str, from: Date, to: Date) -> Record {
        Record::Estimate(Estimate {
            description: "work".to_owned(),
            tags: tags(tags_text),
            distribution: Distribution::point(usd(median)),
            markup_basis_points: 0,
            estimator: "mara".to_owned(),
            range: DateRange::new(from, to).unwrap(),
        })
    }

    /// The worked month of the spec: 10 USD per million input tokens, so
    /// S1 is 400, S2 300 (60/40), S3 100 on the project, S4 150 on nothing.
    fn person_attribute(id: &str, person: &str) -> Event {
        event(
            id,
            JANUARY,
            Record::PersonAttribute(PersonAttribute {
                person: key(person),
                name: name("team"),
                key: key("web"),
                from: None,
            }),
        )
    }

    fn accept(id: &str, estimate: &str) -> Event {
        event(
            id,
            datetime!(2026-01-02 00:00 UTC),
            Record::Accept(Accept {
                estimate: EventId::new(estimate).unwrap(),
            }),
        )
    }

    /// The settings of the worked month: prices, tag values, attributes.
    fn settings() -> Vec<Event> {
        vec![
            event("p1", JANUARY, price_row(TokenKind::INPUT, "10")),
            event(
                "v1",
                JANUARY,
                tag_value("feature", "KD-1", &["project=shop"]),
            ),
            event(
                "v2",
                JANUARY,
                tag_value("feature", "KD-2", &["project=shop"]),
            ),
            event(
                "v3",
                JANUARY,
                tag_value("project", "shop", &["client=acme"]),
            ),
            event("v4", JANUARY, tag_value("client", "acme", &[])),
            event(
                "a1",
                JANUARY,
                Record::AttributeValue(AttributeValue {
                    name: name("team"),
                    key: key("web"),
                    title: None,
                    defaults: tags(&["department=engineering"]),
                }),
            ),
            person_attribute("a2", "mara"),
            person_attribute("a3", "tom"),
        ]
    }

    /// The plans of the worked month: two accepted estimates and a share.
    fn plans() -> Vec<Event> {
        vec![
            event(
                "e1",
                JANUARY,
                estimate(
                    &["project=shop"],
                    "750",
                    date!(2026 - 02 - 01),
                    date!(2026 - 05 - 11),
                ),
            ),
            accept("c1", "e1"),
            event(
                "e2",
                JANUARY,
                estimate(
                    &["feature=KD-1"],
                    "500",
                    date!(2026 - 03 - 01),
                    date!(2026 - 03 - 31),
                ),
            ),
            accept("c2", "e2"),
            event(
                "sh1",
                JANUARY,
                Record::Share(Share {
                    person: key("mara"),
                    project: key("shop"),
                    fraction_basis_points: 6_000,
                    from: date!(2026 - 03 - 01),
                    to: None,
                }),
            ),
        ]
    }

    /// The worked month of the spec: 10 USD per million input tokens, so
    /// S1 is 400, S2 300 (60/40), S3 100 on the project, S4 150 on nothing.
    fn worked_month() -> Vec<Event> {
        let m = |session, id, person, day, millions| {
            Record::Message(Box::new(message(
                session,
                id,
                person,
                date!(2026 - 03 - 01)
                    .replace_day(day)
                    .unwrap()
                    .with_hms(10, 0, 0)
                    .unwrap()
                    .assume_utc(),
                millions,
            )))
        };
        let mut events = settings();
        events.extend([
            event(
                "m1",
                datetime!(2026-03-02 11:00 UTC),
                m("S1", "1", "mara", 2, 40),
            ),
            event(
                "m2",
                datetime!(2026-03-05 11:00 UTC),
                m("S2", "1", "mara", 5, 30),
            ),
            event(
                "m3",
                datetime!(2026-03-09 11:00 UTC),
                m("S3", "1", "tom", 9, 10),
            ),
            event(
                "m4",
                datetime!(2026-03-12 11:00 UTC),
                m("S4", "1", "tom", 12, 15),
            ),
            event(
                "s1",
                datetime!(2026-03-02 12:00 UTC),
                split(session("S1"), &[(1, &["feature=KD-1"])]),
            ),
            event(
                "s2",
                datetime!(2026-03-05 12:00 UTC),
                split(
                    session("S2"),
                    &[(60, &["feature=KD-1"]), (40, &["feature=KD-2"])],
                ),
            ),
            event(
                "s3",
                datetime!(2026-03-09 12:00 UTC),
                split(session("S3"), &[(1, &["project=shop"])]),
            ),
            event(
                "b1",
                datetime!(2026-04-01 00:00 UTC),
                Record::Bill(Bill::of_provider(
                    "anthropic".to_owned(),
                    march(),
                    usd("1000"),
                )),
            ),
        ]);
        events.extend(plans());
        events
    }

    fn state() -> State {
        State::fold(&worked_month(), APRIL, Currency::USD).unwrap()
    }

    fn amounts(groups: Vec<(Option<TagValue>, Money)>) -> Vec<(String, Money)> {
        groups
            .into_iter()
            .map(|(value, amount)| {
                (
                    value.map(|value| value.to_string()).unwrap_or_default(),
                    amount,
                )
            })
            .collect()
    }

    #[test]
    fn every_message_has_its_cost() {
        let state = state();
        let costs: Vec<(String, Option<Money>)> = state
            .message_costs()
            .iter()
            .map(|line| (line.session_id.to_string(), line.cost.amount))
            .collect();
        assert_eq!(
            costs,
            vec![
                ("S1".to_owned(), Some(usd("400"))),
                ("S2".to_owned(), Some(usd("300"))),
                ("S3".to_owned(), Some(usd("100"))),
                ("S4".to_owned(), Some(usd("150"))),
            ]
        );
        assert_eq!(state.as_of(), APRIL);
        assert_eq!(state.currency(), Currency::USD);
        assert_eq!(state.messages().count(), 4);
        assert_eq!(state.prices().rows().count(), 1);
        assert_eq!(state.values().tag_values().count(), 4);
    }

    #[test]
    fn the_total_and_its_slices_are_the_worked_month() {
        let state = state();
        assert_eq!(
            state.cost(&Filter::default(), march()).unwrap().amount,
            usd("950")
        );
        assert_eq!(
            amounts(
                state
                    .by_tag(&Filter::default(), march(), &name("person"))
                    .unwrap()
            ),
            vec![
                ("mara".to_owned(), usd("700")),
                ("tom".to_owned(), usd("250"))
            ]
        );
        assert_eq!(
            amounts(
                state
                    .by_tag(&Filter::default(), march(), &name("feature"))
                    .unwrap()
            ),
            vec![
                (String::new(), usd("250")),
                ("KD-1".to_owned(), usd("580")),
                ("KD-2".to_owned(), usd("120")),
            ]
        );
        assert_eq!(
            amounts(
                state
                    .by_tag(&Filter::default(), march(), &name("project"))
                    .unwrap()
            ),
            vec![(String::new(), usd("150")), ("shop".to_owned(), usd("800"))]
        );
        assert_eq!(
            amounts(
                state
                    .by_tag(&Filter::default(), march(), &name("client"))
                    .unwrap()
            ),
            vec![(String::new(), usd("150")), ("acme".to_owned(), usd("800"))]
        );
        assert_eq!(
            amounts(
                state
                    .by_tag(&Filter::default(), march(), &name("team"))
                    .unwrap()
            ),
            vec![("web".to_owned(), usd("950"))]
        );
        assert_eq!(
            amounts(
                state
                    .by_tag(&Filter::default(), march(), &name("department"))
                    .unwrap()
            ),
            vec![("engineering".to_owned(), usd("950"))]
        );
        assert_eq!(
            state.by_label(&Filter::default(), march()).unwrap(),
            vec![
                (Label::Direct, usd("700")),
                (Label::ProjectOverhead, usd("100")),
                (Label::OrganizationOverhead, usd("150")),
            ]
        );
        assert_eq!(
            state
                .cost(&filter(&["project=shop", "person=mara"]), march())
                .unwrap()
                .amount,
            usd("700")
        );
        let april = DateRange::new(date!(2026 - 04 - 01), date!(2026 - 04 - 30)).unwrap();
        assert_eq!(
            state.cost(&Filter::default(), april).unwrap().amount,
            usd("0")
        );
    }

    #[test]
    fn lines_carry_their_closed_tags_and_labels() {
        let state = state();
        assert_eq!(state.lines().len(), 5);
        let s2: Vec<&Line> = state
            .lines()
            .iter()
            .filter(|line| line.session_id.as_str() == "S2")
            .collect();
        assert_eq!(s2.len(), 2);
        let first = s2.first().unwrap();
        assert_eq!(first.part, 0);
        assert_eq!(first.amount, usd("180"));
        assert_eq!(first.assigned, tags(&["feature=KD-1"]));
        assert_eq!(
            first.tags,
            tags(&[
                "feature=KD-1",
                "project=shop",
                "client=acme",
                "person=mara",
                "team=web",
                "department=engineering",
                "contract=anthropic"
            ])
        );
        // The bill of March created the contract `anthropic`, the provider's
        // only one, so every message of the provider falls back to it.
        assert_eq!(first.contract().unwrap().as_str(), "anthropic");
        assert_eq!(first.contract_by, Some(ContractBy::Fallback));
        assert!(!first.on_a_plan);
        assert_eq!(first.assigned_by, AssignedBy::Hand);
        assert_eq!(first.label, Label::Direct);
        assert_eq!(first.cost_label, CostLabel::Priced);
        let second = s2.get(1).unwrap();
        assert_eq!(second.part, 1);
        assert_eq!(second.amount, usd("120"));
        let s4 = state
            .lines()
            .iter()
            .find(|line| line.session_id.as_str() == "S4")
            .unwrap();
        assert_eq!(s4.assigned_by, AssignedBy::Default);
        assert_eq!(s4.label, Label::OrganizationOverhead);
        assert_eq!(
            s4.tags,
            tags(&[
                "person=tom",
                "team=web",
                "department=engineering",
                "contract=anthropic"
            ])
        );
        assert_eq!(state.splits().count(), 3);
    }

    #[test]
    fn the_bill_reconciles_with_fifty_unexplained() {
        let state = state();
        let bill = &state.bills().next().unwrap().value;
        let reconciliation = state.reconcile(bill).unwrap();
        assert_eq!(reconciliation.messages, usd("950"));
        assert_eq!(reconciliation.unexplained, usd("50"));
        assert!(!reconciliation.a_message_is_wrong());
        let other = Bill::of_provider("openai".to_owned(), march(), usd("10"));
        assert_eq!(state.reconcile(&other).unwrap().unexplained, usd("10"));
        let short = Bill {
            amount: usd("900"),
            ..bill.clone()
        };
        assert!(state.reconcile(&short).unwrap().a_message_is_wrong());
        let euro = Bill {
            amount: Money::zero(Currency::EUR),
            ..bill.clone()
        };
        assert!(state.reconcile(&euro).is_err());
    }

    #[test]
    fn plans_and_deviations_are_the_worked_month() {
        let state = state();
        assert_eq!(
            state.plan(&filter(&["project=shop"]), march()).unwrap(),
            Some(usd("232.50"))
        );
        let shop = state
            .deviation(&filter(&["project=shop"]), march())
            .unwrap()
            .unwrap();
        assert_eq!(shop.actual.amount, usd("800"));
        assert_eq!(shop.delta, usd("-567.50"));
        assert!(!shop.is_favourable());
        let kd1 = state
            .deviation(&filter(&["feature=KD-1"]), march())
            .unwrap()
            .unwrap();
        assert_eq!(kd1.plan, usd("500"));
        assert_eq!(kd1.delta, usd("-80"));
        assert_eq!(
            state.plan(&filter(&["feature=KD-2"]), march()).unwrap(),
            None
        );
        assert_eq!(
            state
                .deviation(&filter(&["feature=KD-2"]), march())
                .unwrap(),
            None
        );
        assert_eq!(state.estimates().count(), 2);
        assert!(state.accepted(&tags(&["project=shop"])).is_some());
        let zero = Deviation {
            plan: usd("1"),
            actual: state.cost(&filter(&["feature=KD-9"]), march()).unwrap(),
            delta: usd("0"),
        };
        assert!(zero.is_favourable());
    }

    #[test]
    fn a_persons_plan_comes_from_their_shares() {
        let state = state();
        assert_eq!(
            state.person_plan(&key("mara"), march()).unwrap(),
            usd("139.50")
        );
        assert_eq!(
            state.plan(&filter(&["person=mara"]), march()).unwrap(),
            Some(usd("139.50"))
        );
        assert_eq!(state.person_plan(&key("tom"), march()).unwrap(), usd("0"));
        let mara = state
            .deviation(&filter(&["person=mara"]), march())
            .unwrap()
            .unwrap();
        assert_eq!(mara.delta, usd("-560.50"));
        // A person with a project named is a filter, not a person.
        assert_eq!(
            state
                .plan(&filter(&["person=mara", "project=shop"]), march())
                .unwrap(),
            None
        );
        let february = DateRange::new(date!(2026 - 02 - 01), date!(2026 - 02 - 28)).unwrap();
        assert_eq!(state.person_plan(&key("mara"), february).unwrap(), usd("0"));
        assert_eq!(state.shares().len(), 1);
        let sums = state.share_sums(date!(2026 - 03 - 15));
        assert_eq!(sums.first().unwrap().basis_points, 6_000);
    }

    #[test]
    fn a_share_of_a_project_without_an_accepted_plan_is_nothing() {
        let mut events = worked_month();
        events.push(event(
            "sh2",
            JANUARY,
            Record::Share(Share {
                person: key("tom"),
                project: key("other"),
                fraction_basis_points: 10_000,
                from: date!(2026 - 03 - 01),
                to: None,
            }),
        ));
        let state = State::fold(&events, APRIL, Currency::USD).unwrap();
        assert_eq!(state.person_plan(&key("tom"), march()).unwrap(), usd("0"));
    }

    #[test]
    fn a_reversal_removes_an_event_and_a_superseding_split_replaces_one() {
        let mut events = worked_month();
        events.push(event(
            "r1",
            datetime!(2026-03-20 00:00 UTC),
            Record::Reversal(EventId::new("m4").unwrap()),
        ));
        events.push(
            event(
                "s2b",
                datetime!(2026-03-21 00:00 UTC),
                split(session("S2"), &[(1, &["feature=KD-2"])]),
            )
            .superseding(EventId::new("s2").unwrap()),
        );
        let current = State::fold(&events, APRIL, Currency::USD).unwrap();
        assert_eq!(
            current.cost(&Filter::default(), march()).unwrap().amount,
            usd("800")
        );
        assert_eq!(
            current
                .cost(&filter(&["feature=KD-2"]), march())
                .unwrap()
                .amount,
            usd("300")
        );
        let before = State::fold(&events, datetime!(2026-03-19 00:00 UTC), Currency::USD).unwrap();
        assert_eq!(
            before.cost(&Filter::default(), march()).unwrap().amount,
            usd("950")
        );
        assert_eq!(
            before
                .cost(&filter(&["feature=KD-2"]), march())
                .unwrap()
                .amount,
            usd("120")
        );
    }

    #[test]
    fn reversing_the_correction_brings_the_original_back() {
        let mut events = worked_month();
        events.push(
            event(
                "s2b",
                datetime!(2026-03-21 00:00 UTC),
                split(session("S2"), &[(1, &["feature=KD-2"])]),
            )
            .superseding(EventId::new("s2").unwrap()),
        );
        events.push(event(
            "r2",
            datetime!(2026-03-22 00:00 UTC),
            Record::Reversal(EventId::new("s2b").unwrap()),
        ));
        let state = State::fold(&events, APRIL, Currency::USD).unwrap();
        assert_eq!(
            state
                .cost(&filter(&["feature=KD-2"]), march())
                .unwrap()
                .amount,
            usd("120")
        );
        // A later split on the same target replaces the earlier one without
        // a pointer, too.
        let mut events = worked_month();
        events.push(event(
            "s2c",
            datetime!(2026-03-23 00:00 UTC),
            split(session("S2"), &[(1, &["feature=KD-2"])]),
        ));
        let state = State::fold(&events, APRIL, Currency::USD).unwrap();
        assert_eq!(
            state
                .cost(&filter(&["feature=KD-2"]), march())
                .unwrap()
                .amount,
            usd("300")
        );
    }

    #[test]
    fn a_split_on_a_message_wins_over_its_session() {
        let mut events = worked_month();
        events.push(event(
            "s1m",
            datetime!(2026-03-23 00:00 UTC),
            split(
                SplitTarget::Message {
                    session_id: SessionId::new("S1").unwrap(),
                    message_id: MessageId::new("1").unwrap(),
                },
                &[(1, &["feature=KD-2"])],
            ),
        ));
        let state = State::fold(&events, APRIL, Currency::USD).unwrap();
        assert_eq!(
            state
                .cost(&filter(&["feature=KD-2"]), march())
                .unwrap()
                .amount,
            usd("520")
        );
    }

    #[test]
    fn a_bad_log_is_refused() {
        let mut twice = worked_month();
        twice.push(event("m1", APRIL, Record::Cutoff(date!(2026 - 03 - 01))));
        assert_eq!(
            State::fold(&twice, APRIL, Currency::USD),
            Err(FoldError::DuplicateId(EventId::new("m1").unwrap()))
        );
        let mut forward = worked_month();
        forward.push(
            event("x", JANUARY, Record::Cutoff(date!(2026 - 03 - 01)))
                .superseding(EventId::new("m1").unwrap()),
        );
        assert!(matches!(
            State::fold(&forward, APRIL, Currency::USD),
            Err(FoldError::SupersedesLater { .. })
        ));
        let mut nowhere = worked_month();
        nowhere.push(
            event("x", APRIL, Record::Cutoff(date!(2026 - 03 - 01)))
                .superseding(EventId::new("gone").unwrap()),
        );
        assert!(matches!(
            State::fold(&nowhere, APRIL, Currency::USD),
            Err(FoldError::SupersedesLater { .. })
        ));
        let mut weightless = worked_month();
        weightless.push(event("x", APRIL, split(session("S4"), &[(0, &[])])));
        assert!(matches!(
            State::fold(&weightless, APRIL, Currency::USD),
            Err(FoldError::Split { .. })
        ));
        let mut bad_bands = worked_month();
        bad_bands.push(event(
            "x",
            APRIL,
            Record::Estimate(Estimate {
                description: "x".to_owned(),
                tags: Tags::new(),
                distribution: Distribution {
                    median: usd("1"),
                    bands: vec![crate::estimate::Band {
                        up_to: None,
                        basis_points: 1,
                    }],
                },
                markup_basis_points: 0,
                estimator: "x".to_owned(),
                range: march(),
            }),
        ));
        assert!(matches!(
            State::fold(&bad_bands, APRIL, Currency::USD),
            Err(FoldError::Estimate { .. })
        ));
        let mut euro = worked_month();
        euro.push(event(
            "x",
            APRIL,
            Record::PriceRow(PriceRow {
                provider: "a".to_owned(),
                model: "b".to_owned(),
                kind: TokenKind::INPUT,
                per_million: Money::zero(Currency::EUR),
                valid_from: date!(2026 - 01 - 01),
                source: PriceSource::Managed,
                contract: None,
            }),
        ));
        assert!(matches!(
            State::fold(&euro, APRIL, Currency::USD),
            Err(FoldError::Money { .. })
        ));
        assert_eq!(
            FoldError::DuplicateId(EventId::new("m1").unwrap()).to_string(),
            "event m1 is in the log twice"
        );
    }

    #[test]
    fn events_after_the_as_of_moment_are_not_in_the_fold() {
        let empty = State::fold(
            &worked_month(),
            datetime!(2025-12-31 00:00 UTC),
            Currency::USD,
        )
        .unwrap();
        assert!(empty.lines().is_empty());
        assert!(empty.bills().next().is_none());
        assert_eq!(
            empty.cost(&Filter::default(), march()).unwrap().amount,
            usd("0")
        );
        let mid = State::fold(
            &worked_month(),
            datetime!(2026-03-05 11:00 UTC),
            Currency::USD,
        )
        .unwrap();
        assert_eq!(
            mid.cost(&Filter::default(), march()).unwrap().amount,
            usd("700")
        );
        // The split of S2 came at 12:00, so at 11:00 S2 is still on nothing.
        assert_eq!(
            mid.cost(&filter(&["feature=KD-1"]), march())
                .unwrap()
                .amount,
            usd("400")
        );
        assert_eq!(
            mid.cost(&filter(&["project=shop"]), march())
                .unwrap()
                .amount,
            usd("400")
        );
    }

    #[test]
    fn the_frozen_figures_are_the_fold_as_of_the_cutoff() {
        let mut events = worked_month();
        events.push(event(
            "k1",
            datetime!(2026-04-01 08:00 UTC),
            Record::Cutoff(date!(2026 - 04 - 01)),
        ));
        events.push(event(
            "s4",
            datetime!(2026-04-03 00:00 UTC),
            split(session("S4"), &[(1, &["project=shop"])]),
        ));
        let current = State::fold(&events, APRIL, Currency::USD).unwrap();
        assert_eq!(
            current
                .cost(&filter(&["project=shop"]), march())
                .unwrap()
                .amount,
            usd("950")
        );
        let frozen_as_of = current.frozen_as_of().unwrap();
        assert_eq!(frozen_as_of, datetime!(2026-04-01 00:00 UTC));
        let frozen = State::fold(&events, frozen_as_of, Currency::USD).unwrap();
        assert_eq!(
            frozen
                .cost(&filter(&["project=shop"]), march())
                .unwrap()
                .amount,
            usd("800")
        );
        assert_eq!(frozen.cutoff(), None);
        assert_eq!(frozen.as_of(), frozen_as_of);
    }

    #[test]
    fn a_count_error_and_an_unknown_model_are_flagged_never_zero() {
        let mut events = worked_month();
        let mut no_counts = message("S5", "1", "tom", datetime!(2026-03-13 10:00 UTC), 1);
        no_counts.tokens = None;
        events.push(event(
            "m5",
            datetime!(2026-03-13 11:00 UTC),
            Record::Message(Box::new(no_counts)),
        ));
        let mut unknown = message("S6", "1", "tom", datetime!(2026-03-14 10:00 UTC), 1);
        unknown.model = ModelRef::new("acme", "mystery-1");
        events.push(event(
            "m6",
            datetime!(2026-03-14 11:00 UTC),
            Record::Message(Box::new(unknown)),
        ));
        let state = State::fold(&events, APRIL, Currency::USD).unwrap();
        let cost = state.cost(&Filter::default(), march()).unwrap();
        assert_eq!(cost.amount, usd("950"));
        assert_eq!(cost.count_errors.len(), 1);
        assert_eq!(cost.count_errors.iter().next().unwrap().0.as_str(), "S5");
        assert_eq!(cost.not_priced_models.len(), 1);
        assert_eq!(
            cost.not_priced_models
                .iter()
                .next()
                .unwrap()
                .provider
                .as_deref(),
            Some("acme")
        );
        let (key, counts) = cost.not_priced.iter().next().unwrap();
        assert_eq!(key.0.as_str(), "S6");
        assert_eq!(counts.get(&TokenKind::INPUT), 1_000_000);
        let mara = state.cost(&filter(&["person=mara"]), march()).unwrap();
        assert!(mara.count_errors.is_empty());
        assert!(mara.not_priced_models.is_empty());
        assert!(mara.not_priced.is_empty());
        let labels: Vec<CostLabel> = state
            .message_costs()
            .iter()
            .map(|line| line.cost.label)
            .collect();
        assert_eq!(
            labels,
            vec![
                CostLabel::Priced,
                CostLabel::Priced,
                CostLabel::Priced,
                CostLabel::Priced,
                CostLabel::CountError,
                CostLabel::NotPricedYet
            ]
        );
        let line = state
            .lines()
            .iter()
            .find(|line| line.session_id.as_str() == "S5")
            .unwrap();
        assert_eq!(line.amount, usd("0"));
        assert_eq!(line.cost_label, CostLabel::CountError);
        // The bill is reconciled against the figures there are.
        let bill = &state.bills().next().unwrap().value;
        assert_eq!(state.reconcile(bill).unwrap().unexplained, usd("50"));
        assert_eq!(
            state
                .provider_cost("acme", march())
                .unwrap()
                .not_priced_models
                .len(),
            1
        );
    }

    fn branch_rule(id: &str, recorded_at: OffsetDateTime) -> Event {
        event(
            id,
            recorded_at,
            Record::Rule(Rule {
                name: "feature key in branch".to_owned(),
                scope: Scope::Organization,
                order: 10,
                signal: Signal::Branch,
                pattern: TextPattern::new(r"KD-\d+").unwrap(),
                then: Then::KeyOf {
                    name: name("feature"),
                },
                contract: None,
            }),
        )
    }

    #[test]
    fn a_rule_in_force_on_arrival_writes_the_split() {
        let mut events = worked_month();
        events.push(branch_rule("rule1", JANUARY));
        let mut on_branch = message("S4", "1", "tom", datetime!(2026-03-12 10:00 UTC), 15);
        on_branch.signals.branch = Some("feature/KD-2-login".to_owned());
        events.retain(|event| event.id.as_str() != "m4");
        events.push(event(
            "m4",
            datetime!(2026-03-12 11:00 UTC),
            Record::Message(Box::new(on_branch)),
        ));
        let state = State::fold(&events, APRIL, Currency::USD).unwrap();
        let s4 = state
            .lines()
            .iter()
            .find(|line| line.session_id.as_str() == "S4")
            .unwrap();
        assert_eq!(s4.assigned_by, AssignedBy::Rule);
        assert_eq!(s4.assigned, tags(&["feature=KD-2"]));
        assert_eq!(s4.label, Label::Direct);
        assert_eq!(
            state
                .cost(&filter(&["feature=KD-2"]), march())
                .unwrap()
                .amount,
            usd("270")
        );
        assert_eq!(state.rules().count(), 1);
    }

    #[test]
    fn a_later_rule_runs_back_only_over_messages_since_the_cutoff() {
        let mut events = worked_month();
        let mut on_branch = message("S4", "1", "tom", datetime!(2026-03-12 10:00 UTC), 15);
        on_branch.signals.branch = Some("feature/KD-2-login".to_owned());
        events.retain(|event| event.id.as_str() != "m4");
        events.push(event(
            "m4",
            datetime!(2026-03-12 11:00 UTC),
            Record::Message(Box::new(on_branch)),
        ));
        let mut february = message("S0", "1", "tom", datetime!(2026-02-20 10:00 UTC), 5);
        february.signals.branch = Some("feature/KD-2-login".to_owned());
        events.push(event(
            "m0",
            datetime!(2026-02-20 11:00 UTC),
            Record::Message(Box::new(february)),
        ));
        events.push(event(
            "k1",
            datetime!(2026-03-01 00:00 UTC),
            Record::Cutoff(date!(2026 - 03 - 01)),
        ));
        events.push(branch_rule("rule1", datetime!(2026-03-20 00:00 UTC)));
        let state = State::fold(&events, APRIL, Currency::USD).unwrap();
        assert_eq!(state.cutoff(), Some(date!(2026 - 03 - 01)));
        assert_eq!(state.frozen_as_of(), Some(datetime!(2026-03-01 00:00 UTC)));
        let s4 = state
            .lines()
            .iter()
            .find(|line| line.session_id.as_str() == "S4")
            .unwrap();
        assert_eq!(s4.assigned_by, AssignedBy::Rule);
        let s0 = state
            .lines()
            .iter()
            .find(|line| line.session_id.as_str() == "S0")
            .unwrap();
        assert_eq!(s0.assigned_by, AssignedBy::Default);
        assert_eq!(s0.label, Label::OrganizationOverhead);
        // A message dated exactly on the cutoff is since the cutoff.
        let mut on_cutoff = message("S7", "1", "tom", datetime!(2026-03-01 00:00 UTC), 1);
        on_cutoff.signals.branch = Some("feature/KD-2".to_owned());
        events.push(event(
            "m7",
            datetime!(2026-03-01 01:00 UTC),
            Record::Message(Box::new(on_cutoff)),
        ));
        let state = State::fold(&events, APRIL, Currency::USD).unwrap();
        let s7 = state
            .lines()
            .iter()
            .find(|line| line.session_id.as_str() == "S7")
            .unwrap();
        assert_eq!(s7.assigned_by, AssignedBy::Rule);
        // Without a cutoff the rule runs back over everything.
        events.retain(|event| event.id.as_str() != "k1");
        let state = State::fold(&events, APRIL, Currency::USD).unwrap();
        let s0 = state
            .lines()
            .iter()
            .find(|line| line.session_id.as_str() == "S0")
            .unwrap();
        assert_eq!(s0.assigned_by, AssignedBy::Rule);
        assert_eq!(state.frozen_as_of(), None);
    }

    #[test]
    fn a_hand_split_wins_over_a_rule_and_a_rule_keeps_the_messages_tags() {
        let mut events = worked_month();
        events.push(branch_rule("rule1", JANUARY));
        let mut on_branch = message("S1", "1", "mara", datetime!(2026-03-02 10:00 UTC), 40);
        on_branch.signals.branch = Some("feature/KD-2-login".to_owned());
        on_branch.tags = tags(&["purpose=review"]);
        events.retain(|event| event.id.as_str() != "m1");
        events.push(event(
            "m1",
            datetime!(2026-03-02 11:00 UTC),
            Record::Message(Box::new(on_branch)),
        ));
        let state = State::fold(&events, APRIL, Currency::USD).unwrap();
        let s1 = state
            .lines()
            .iter()
            .find(|line| line.session_id.as_str() == "S1")
            .unwrap();
        assert_eq!(s1.assigned_by, AssignedBy::Hand);
        assert_eq!(s1.assigned, tags(&["feature=KD-1"]));
        events.retain(|event| event.id.as_str() != "s1");
        let state = State::fold(&events, APRIL, Currency::USD).unwrap();
        let s1 = state
            .lines()
            .iter()
            .find(|line| line.session_id.as_str() == "S1")
            .unwrap();
        assert_eq!(s1.assigned_by, AssignedBy::Rule);
        assert_eq!(s1.assigned, tags(&["feature=KD-2", "purpose=review"]));
    }

    #[test]
    fn several_features_named_and_nobody_deciding_means_equal_shares() {
        let mut events = worked_month();
        let mut several = message("S4", "1", "tom", datetime!(2026-03-12 10:00 UTC), 15);
        several.signals.prompt_keys = vec![
            "KD-1".to_owned(),
            "KD-9".to_owned(),
            "KD-2".to_owned(),
            "KD-1".to_owned(),
        ];
        events.retain(|event| event.id.as_str() != "m4");
        events.push(event(
            "m4",
            datetime!(2026-03-12 11:00 UTC),
            Record::Message(Box::new(several)),
        ));
        let state = State::fold(&events, APRIL, Currency::USD).unwrap();
        let s4: Vec<&Line> = state
            .lines()
            .iter()
            .filter(|line| line.session_id.as_str() == "S4")
            .collect();
        assert_eq!(s4.len(), 2);
        assert_eq!(s4.first().unwrap().amount, usd("75"));
        assert_eq!(s4.first().unwrap().assigned, tags(&["feature=KD-1"]));
        assert_eq!(s4.get(1).unwrap().assigned, tags(&["feature=KD-2"]));
        assert_eq!(s4.first().unwrap().assigned_by, AssignedBy::Default);
        // A message that names its own feature is not split by the prompts.
        let mut own = message("S4", "1", "tom", datetime!(2026-03-12 10:00 UTC), 15);
        own.signals.prompt_keys = vec!["KD-1".to_owned(), "KD-2".to_owned()];
        own.tags = tags(&["feature=KD-2"]);
        events.retain(|event| event.id.as_str() != "m4");
        events.push(event(
            "m4",
            datetime!(2026-03-12 11:00 UTC),
            Record::Message(Box::new(own)),
        ));
        let state = State::fold(&events, APRIL, Currency::USD).unwrap();
        let s4: Vec<&Line> = state
            .lines()
            .iter()
            .filter(|line| line.session_id.as_str() == "S4")
            .collect();
        assert_eq!(s4.len(), 1);
        assert_eq!(s4.first().unwrap().assigned, tags(&["feature=KD-2"]));
        // Keys that are no feature leave the message on its own tags.
        let mut unknown = message("S4", "1", "tom", datetime!(2026-03-12 10:00 UTC), 15);
        unknown.signals.prompt_keys = vec!["KD-9".to_owned(), " ".to_owned()];
        events.retain(|event| event.id.as_str() != "m4");
        events.push(event(
            "m4",
            datetime!(2026-03-12 11:00 UTC),
            Record::Message(Box::new(unknown)),
        ));
        let state = State::fold(&events, APRIL, Currency::USD).unwrap();
        let s4: Vec<&Line> = state
            .lines()
            .iter()
            .filter(|line| line.session_id.as_str() == "S4")
            .collect();
        assert_eq!(s4.len(), 1);
        assert_eq!(s4.first().unwrap().label, Label::OrganizationOverhead);
    }

    #[test]
    fn the_price_override_wins_and_the_price_is_as_of_the_message() {
        let mut events = worked_month();
        events.push(event(
            "p2",
            datetime!(2026-02-01 00:00 UTC),
            Record::PriceRow(PriceRow {
                provider: "anthropic".to_owned(),
                model: "claude-sonnet-4-5".to_owned(),
                kind: TokenKind::INPUT,
                per_million: usd("5"),
                valid_from: date!(2026 - 03 - 10),
                source: PriceSource::Organization,
                contract: None,
            }),
        ));
        let state = State::fold(&events, APRIL, Currency::USD).unwrap();
        // S1, S2 and S3 are before 10 March at 10; S4 is after it at 5.
        assert_eq!(
            state.cost(&Filter::default(), march()).unwrap().amount,
            usd("875")
        );
        assert_eq!(
            state
                .cost(&filter(&["person=tom"]), march())
                .unwrap()
                .amount,
            usd("175")
        );
    }

    #[test]
    fn accepting_another_estimate_replaces_the_plan() {
        let mut events = worked_month();
        events.push(event(
            "e3",
            datetime!(2026-02-01 00:00 UTC),
            estimate(
                &["project=shop"],
                "1000",
                date!(2026 - 03 - 01),
                date!(2026 - 03 - 31),
            ),
        ));
        let unaccepted = State::fold(&events, APRIL, Currency::USD).unwrap();
        assert_eq!(
            unaccepted
                .plan(&filter(&["project=shop"]), march())
                .unwrap(),
            Some(usd("232.50"))
        );
        events.push(event(
            "c3",
            datetime!(2026-02-02 00:00 UTC),
            Record::Accept(Accept {
                estimate: EventId::new("e3").unwrap(),
            }),
        ));
        let accepted = State::fold(&events, APRIL, Currency::USD).unwrap();
        assert_eq!(
            accepted.plan(&filter(&["project=shop"]), march()).unwrap(),
            Some(usd("1000"))
        );
        assert_eq!(
            accepted.person_plan(&key("mara"), march()).unwrap(),
            usd("600")
        );
        assert_eq!(accepted.estimates().count(), 3);
        // An accept of an estimate that is not in the log plans nothing.
        events.push(event(
            "c4",
            datetime!(2026-02-03 00:00 UTC),
            Record::Accept(Accept {
                estimate: EventId::new("nope").unwrap(),
            }),
        ));
        let same = State::fold(&events, APRIL, Currency::USD).unwrap();
        assert_eq!(
            same.plan(&filter(&["project=shop"]), march()).unwrap(),
            Some(usd("1000"))
        );
        // Reversing the estimate leaves its accept pointing at nothing, so
        // the estimate accepted before it stands again.
        events.push(event(
            "r3",
            datetime!(2026-02-04 00:00 UTC),
            Record::Reversal(EventId::new("e3").unwrap()),
        ));
        let reversed = State::fold(&events, APRIL, Currency::USD).unwrap();
        assert_eq!(
            reversed.plan(&filter(&["project=shop"]), march()).unwrap(),
            Some(usd("232.50"))
        );
        // Reversing the first accept as well leaves no plan.
        events.push(event(
            "r5",
            datetime!(2026-02-05 00:00 UTC),
            Record::Reversal(EventId::new("c1").unwrap()),
        ));
        let none = State::fold(&events, APRIL, Currency::USD).unwrap();
        assert_eq!(
            none.plan(&filter(&["project=shop"]), march()).unwrap(),
            None
        );
        assert_eq!(none.person_plan(&key("mara"), march()).unwrap(), usd("0"));
    }

    #[test]
    fn the_forecast_is_the_past_cost_per_day_over_the_next_days() {
        let state = state();
        let april = DateRange::new(date!(2026 - 04 - 01), date!(2026 - 04 - 30)).unwrap();
        // 950 over 31 days, times 30 days.
        assert_eq!(
            state
                .forecast(&Filter::default(), &[march()], april)
                .unwrap(),
            usd("919.354838710")
        );
        let february = DateRange::new(date!(2026 - 02 - 01), date!(2026 - 02 - 28)).unwrap();
        // 950 over 59 days, times 30.
        assert_eq!(
            state
                .forecast(&Filter::default(), &[february, march()], april)
                .unwrap(),
            usd("483.050847458")
        );
        assert_eq!(
            state.forecast(&Filter::default(), &[], april).unwrap(),
            usd("0")
        );
        assert_eq!(
            state
                .forecast(
                    &filter(&["person=tom"]),
                    &[march()],
                    DateRange::day(date!(2026 - 04 - 01))
                )
                .unwrap(),
            usd("8.064516129")
        );
    }

    #[test]
    fn access_comes_from_the_grants_in_force() {
        let mut events = worked_month();
        events.push(event(
            "g1",
            JANUARY,
            Record::Grant(Grant {
                person: key("lea"),
                capability: Capability::View,
                filter: filter(&["client=acme"]),
            }),
        ));
        let state = State::fold(&events, APRIL, Currency::USD).unwrap();
        assert!(state.can(
            &key("lea"),
            Capability::View,
            &filter(&["client=acme", "project=shop"])
        ));
        assert!(!state.can(&key("lea"), Capability::View, &Filter::default()));
        assert!(state.can(&key("tom"), Capability::Record, &Filter::default()));
        assert_eq!(state.grants().len(), 1);
        events.push(event(
            "r4",
            APRIL,
            Record::Reversal(EventId::new("g1").unwrap()),
        ));
        let revoked = State::fold(&events, APRIL, Currency::USD).unwrap();
        assert!(!revoked.can(&key("lea"), Capability::View, &filter(&["client=acme"])));
        assert!(revoked.grants().is_empty());
    }

    #[test]
    fn submissions_are_submitted_until_approved_or_rejected() {
        let mut events = worked_month();
        events.push(event(
            "sub1",
            datetime!(2026-04-01 09:00 UTC),
            Record::Submit(Submit {
                person: key("mara"),
                window: march(),
            }),
        ));
        let submitted = State::fold(&events, APRIL, Currency::USD).unwrap();
        assert_eq!(submitted.submissions().len(), 1);
        assert_eq!(
            submitted.submissions().first().unwrap().status,
            SubmissionStatus::Submitted
        );
        assert!(
            submitted
                .submission_of(&key("mara"), datetime!(2026-03-02 10:00 UTC))
                .is_some()
        );
        assert!(
            submitted
                .submission_of(&key("tom"), datetime!(2026-03-02 10:00 UTC))
                .is_none()
        );
        events.push(event(
            "ap1",
            datetime!(2026-04-02 09:00 UTC),
            Record::Approval(Approval {
                submission: EventId::new("sub1").unwrap(),
                decision: Decision::Rejected {
                    reason: "S2 is all KD-1".to_owned(),
                },
            }),
        ));
        let rejected = State::fold(&events, APRIL, Currency::USD).unwrap();
        assert_eq!(
            rejected.submissions().first().unwrap().status,
            SubmissionStatus::Rejected {
                reason: "S2 is all KD-1".to_owned()
            }
        );
        events.push(event(
            "ap2",
            datetime!(2026-04-03 09:00 UTC),
            Record::Approval(Approval {
                submission: EventId::new("sub1").unwrap(),
                decision: Decision::Approved,
            }),
        ));
        let approved = State::fold(&events, APRIL, Currency::USD).unwrap();
        assert_eq!(
            approved.submissions().first().unwrap().status,
            SubmissionStatus::Approved
        );
        // Submitting the same window again starts over.
        events.push(event(
            "sub2",
            datetime!(2026-04-04 09:00 UTC),
            Record::Submit(Submit {
                person: key("mara"),
                window: march(),
            }),
        ));
        let again = State::fold(&events, APRIL, Currency::USD).unwrap();
        assert_eq!(again.submissions().len(), 1);
        assert_eq!(again.submissions().first().unwrap().id.as_str(), "sub2");
        assert_eq!(
            again.submissions().first().unwrap().status,
            SubmissionStatus::Submitted
        );
        // An approval of an unknown submission changes nothing.
        events.push(event(
            "ap3",
            datetime!(2026-04-04 10:00 UTC),
            Record::Approval(Approval {
                submission: EventId::new("nope").unwrap(),
                decision: Decision::Approved,
            }),
        ));
        let same = State::fold(&events, APRIL, Currency::USD).unwrap();
        assert_eq!(
            same.submissions().first().unwrap().status,
            SubmissionStatus::Submitted
        );
    }

    #[test]
    fn submissions_of_other_people_and_windows_stand_beside_each_other() {
        let mut events = worked_month();
        events.push(event(
            "sub2",
            datetime!(2026-04-04 09:00 UTC),
            Record::Submit(Submit {
                person: key("mara"),
                window: march(),
            }),
        ));
        events.push(event(
            "sub3",
            datetime!(2026-04-04 11:00 UTC),
            Record::Submit(Submit {
                person: key("tom"),
                window: march(),
            }),
        ));
        events.push(event(
            "sub4",
            datetime!(2026-04-04 12:00 UTC),
            Record::Submit(Submit {
                person: key("mara"),
                window: DateRange::new(date!(2026 - 02 - 01), date!(2026 - 02 - 28)).unwrap(),
            }),
        ));
        let three = State::fold(&events, APRIL, Currency::USD).unwrap();
        let ids: Vec<&str> = three
            .submissions()
            .iter()
            .map(|submission| submission.id.as_str())
            .collect();
        assert_eq!(ids, vec!["sub2", "sub3", "sub4"]);
    }

    #[test]
    fn the_history_lists_every_tag_value_with_its_cost_so_far() {
        let state = state();
        let history = state.history().unwrap();
        let summary: Vec<(String, String, Money, Option<Date>)> = history
            .iter()
            .map(|entry| {
                (
                    entry.name.to_string(),
                    entry.key.to_string(),
                    entry.cost_so_far,
                    entry.last_activity,
                )
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                (
                    "client".to_owned(),
                    "acme".to_owned(),
                    usd("800"),
                    Some(date!(2026 - 03 - 09))
                ),
                (
                    "feature".to_owned(),
                    "KD-1".to_owned(),
                    usd("580"),
                    Some(date!(2026 - 03 - 05))
                ),
                (
                    "feature".to_owned(),
                    "KD-2".to_owned(),
                    usd("120"),
                    Some(date!(2026 - 03 - 05))
                ),
                (
                    "project".to_owned(),
                    "shop".to_owned(),
                    usd("800"),
                    Some(date!(2026 - 03 - 09))
                ),
            ]
        );
        let mut events = worked_month();
        events.push(event("v5", JANUARY, tag_value("feature", "KD-3", &[])));
        let with_idle = State::fold(&events, APRIL, Currency::USD).unwrap();
        let idle = with_idle
            .history()
            .unwrap()
            .into_iter()
            .find(|entry| entry.key.as_str() == "KD-3")
            .unwrap();
        assert_eq!(idle.cost_so_far, usd("0"));
        assert_eq!(idle.last_activity, None);
    }

    #[test]
    fn a_later_message_record_for_the_same_message_replaces_the_earlier() {
        let mut events = worked_month();
        let corrected = message("S4", "1", "tom", datetime!(2026-03-12 10:00 UTC), 20);
        events.push(event(
            "m4b",
            datetime!(2026-03-13 00:00 UTC),
            Record::Message(Box::new(corrected)),
        ));
        let state = State::fold(&events, APRIL, Currency::USD).unwrap();
        assert_eq!(state.messages().count(), 4);
        assert_eq!(
            state
                .cost(&filter(&["person=tom"]), march())
                .unwrap()
                .amount,
            usd("300")
        );
    }

    #[test]
    fn a_persons_attributes_follow_the_latest_record() {
        let mut events = worked_month();
        events.push(event(
            "a4",
            datetime!(2026-02-01 00:00 UTC),
            Record::PersonAttribute(PersonAttribute {
                person: key("tom"),
                name: name("team"),
                key: key("data"),
                from: None,
            }),
        ));
        let state = State::fold(&events, APRIL, Currency::USD).unwrap();
        assert_eq!(
            amounts(
                state
                    .by_tag(&Filter::default(), march(), &name("team"))
                    .unwrap()
            ),
            vec![
                ("data".to_owned(), usd("250")),
                ("web".to_owned(), usd("700"))
            ]
        );
        assert_eq!(
            amounts(
                state
                    .by_tag(&Filter::default(), march(), &name("department"))
                    .unwrap()
            ),
            vec![
                (String::new(), usd("250")),
                ("engineering".to_owned(), usd("700"))
            ]
        );
        assert_eq!(
            state
                .cost(&filter(&["department=engineering"]), march())
                .unwrap()
                .amount,
            usd("700")
        );
    }

    #[test]
    fn the_deviation_label_is_carried_not_computed() {
        assert_eq!(
            serde_json::to_string(&DeviationLabel::Scope).unwrap(),
            r#""scope""#
        );
        assert_eq!(
            serde_json::to_string(&DeviationLabel::EstimateItself).unwrap(),
            r#""estimate_itself""#
        );
        assert_eq!(
            serde_json::to_string(&DeviationLabel::Trouble).unwrap(),
            r#""trouble""#
        );
    }

    #[test]
    fn the_match_window_is_thirty_seconds_either_way() {
        let at = datetime!(2026-03-10 09:00 UTC);
        assert_eq!(
            gap(at, at.saturating_add(Duration::seconds(5))),
            5_000_000_000
        );
        assert_eq!(
            gap(at.saturating_add(Duration::seconds(5)), at),
            5_000_000_000
        );
        assert!(within_window(at, at.saturating_add(Duration::seconds(30))));
        assert!(!within_window(at, at.saturating_add(Duration::seconds(31))));
        assert!(!within_window(at.saturating_sub(Duration::seconds(31)), at));
    }

    fn call(
        session: &str,
        id: &str,
        provider: Option<&str>,
        at: OffsetDateTime,
        input: u64,
    ) -> Event {
        let mut sent = message(session, id, "tom", at, 0);
        sent.tokens = Some(TokenCounts::new().with(TokenKind::INPUT, input));
        sent.model = match provider {
            Some(provider) => ModelRef::new(provider, "gpt-5"),
            None => ModelRef::unreported("gpt-5"),
        };
        event(
            &format!("e-{session}-{id}"),
            APRIL,
            Record::Message(Box::new(sent)),
        )
    }

    fn matched_of(events: &[Event]) -> Vec<(String, String)> {
        State::fold(events, APRIL, Currency::USD)
            .unwrap()
            .matched()
            .iter()
            .map(|((session, id), (to_session, to_id))| {
                (format!("{session}/{id}"), format!("{to_session}/{to_id}"))
            })
            .collect()
    }

    #[test]
    fn a_built_id_matches_only_the_same_call() {
        let at = datetime!(2026-03-10 09:00 UTC);
        let later = |seconds| at.saturating_add(Duration::seconds(seconds));
        let file = call("c1", "turn-1", Some("openai"), at, 100);
        // Same call, four seconds later: matched.
        let echo = call("c1", "otel:a", Some("openai"), later(4), 100);
        assert_eq!(
            matched_of(&[file.clone(), echo]),
            [("c1/otel:a".to_owned(), "c1/turn-1".to_owned())]
        );
        // Another provider, other counts, or out of the window: not the same call.
        for other in [
            call("c1", "otel:b", Some("azure_foundry"), later(4), 100),
            call("c1", "otel:c", Some("openai"), later(4), 101),
            call("c1", "otel:d", Some("openai"), later(40), 100),
        ] {
            assert!(matched_of(&[file.clone(), other]).is_empty());
        }
        // A built message without a provider still matches.
        let unreported = call("c1", "otel:e", None, later(4), 100);
        assert_eq!(matched_of(&[file, unreported]).len(), 1);
        // A session with its own file messages looks nowhere else.
        let elsewhere = call("c2", "turn-1", Some("openai"), later(2), 100);
        let mine = call("c1", "turn-9", Some("openai"), later(30), 7);
        let echo = call("c1", "otel:f", Some("openai"), later(4), 100);
        assert!(matched_of(&[elsewhere.clone(), mine, echo]).is_empty());
        // A telemetry-only session looks in the person's other sessions.
        let lonely = call("tel", "otel:g", Some("openai"), later(4), 100);
        assert_eq!(
            matched_of(&[elsewhere, lonely]),
            [("tel/otel:g".to_owned(), "c2/turn-1".to_owned())]
        );
    }

    fn contract_bill(provider: &str, contract: Option<&str>, from: Date, amount: &str) -> Bill {
        let mut bill = Bill::of_provider(
            provider.to_owned(),
            DateRange::new(from, from.saturating_add(Duration::days(27))).unwrap(),
            usd(amount),
        );
        bill.contract = contract.map(key);
        bill
    }

    #[test]
    fn a_bill_read_in_the_fold_keeps_its_contract() {
        let march = date!(2026 - 03 - 01);
        let unnamed = contract_bill("anthropic", None, march, "50");
        let bills = [
            contract_bill("openai", Some("openai-api"), march, "50"),
            contract_bill("anthropic", Some("api"), date!(2026 - 02 - 01), "50"),
            contract_bill("anthropic", Some("team"), march, "70"),
            unnamed.clone(),
        ];
        let events: Vec<Event> = bills
            .into_iter()
            .enumerate()
            .map(|(index, bill)| {
                event(
                    &format!("b{index}"),
                    APRIL.saturating_add(Duration::seconds(i64::try_from(index).unwrap())),
                    Record::Bill(bill),
                )
            })
            .collect();
        let state = State::fold(
            &events,
            APRIL.saturating_add(Duration::hours(1)),
            Currency::USD,
        )
        .unwrap();
        // Anthropic has two contracts, so the bill naming none got one
        // named after its provider, and asking again gives the same.
        assert_eq!(state.contract_of_bill(&unnamed), key("anthropic"));
        assert_eq!(state.contracts().len(), 4);
    }

    #[test]
    fn the_shipped_sign_in_rule_gives_the_signed_in_organizations_contract() {
        let mut signed_in = message("s1", "m1", "mara", datetime!(2026-03-02 10:00 UTC), 1);
        signed_in.signals.resource.insert(
            "organization.id".to_owned(),
            "6F1E2A3B-0000-4000-8000-000000000001".to_owned(),
        );
        let mut by_tc = message("s2", "m1", "mara", datetime!(2026-03-02 10:00 UTC), 1);
        by_tc.signals.other.insert(
            "organization.id".to_owned(),
            serde_json::json!("6f1e2a3b-0000-4000-8000-000000000001"),
        );
        let api_key = message("s3", "m1", "mara", datetime!(2026-03-02 10:00 UTC), 1);
        let events = vec![
            event("p1", JANUARY, price_row(TokenKind::INPUT, "3")),
            event("m1", APRIL, Record::Message(Box::new(signed_in.clone()))),
            event("m2", APRIL, Record::Message(Box::new(by_tc))),
            event("m3", APRIL, Record::Message(Box::new(api_key))),
        ];
        let state = State::fold(&events, APRIL, Currency::USD).unwrap();
        let by_session: BTreeMap<&str, (Option<&TagValue>, Option<ContractBy>)> = state
            .message_costs()
            .iter()
            .map(|line| {
                (
                    line.session_id.as_str(),
                    (line.contract.as_ref(), line.contract_by),
                )
            })
            .collect();
        let created = key("anthropic-6f1e2a3b");
        assert_eq!(
            by_session.get("s1"),
            Some(&(Some(&created), Some(ContractBy::Rule)))
        );
        assert_eq!(
            by_session.get("s2"),
            Some(&(Some(&created), Some(ContractBy::Rule)))
        );
        // An API key names no organization: the fallback finds the one
        // anthropic contract the rule created.
        assert_eq!(
            by_session.get("s3"),
            Some(&(Some(&created), Some(ContractBy::Fallback)))
        );
        assert_eq!(
            state.contracts().get(&created),
            Some(&ContractTerms::metered("anthropic"))
        );

        // The organization's own rule on contract wins over the shipped one.
        let own = Rule {
            name: "plan".to_owned(),
            scope: Scope::Organization,
            order: 1,
            signal: Signal::Named("organization.id".to_owned()),
            pattern: TextPattern::new("^6f1e").unwrap(),
            then: Then::Tag {
                name: TagName::contract(),
                key: key("claude-team"),
            },
            contract: Some(ContractTerms {
                provider: "anthropic".to_owned(),
                kind: ContractKind::Flat,
                fee: None,
            }),
        };
        let mut with_rule = events;
        with_rule.push(event("r1", JANUARY, Record::Rule(own)));
        let state = State::fold(&with_rule, APRIL, Currency::USD).unwrap();
        let first = state
            .message_costs()
            .iter()
            .find(|line| line.session_id.as_str() == "s1")
            .unwrap();
        // The organization id in the resource is upper case; the pattern
        // reads it as sent, so only the open signal of s2 matches.
        assert_eq!(first.contract.as_ref(), Some(&created));
        let second = state
            .message_costs()
            .iter()
            .find(|line| line.session_id.as_str() == "s2")
            .unwrap();
        assert_eq!(second.contract.as_ref(), Some(&key("claude-team")));
        assert_eq!(second.contract_by, Some(ContractBy::Rule));
    }

    #[test]
    fn an_activity_changes_no_figure() {
        let activity = crate::activity::Activity {
            kind: "claude_code.tool_result".to_owned(),
            session_id: Some(SessionId::new("s1").unwrap()),
            person: key("mara"),
            at: datetime!(2026-03-02 10:00 UTC),
            agent_harness: "claude-code".to_owned(),
            signals: Signals::default(),
            received: Some("rcv_1".to_owned()),
        };
        let base = vec![
            event("p1", JANUARY, price_row(TokenKind::INPUT, "3")),
            event(
                "m1",
                APRIL,
                Record::Message(Box::new(message(
                    "s1",
                    "m1",
                    "mara",
                    datetime!(2026-03-02 10:00 UTC),
                    1,
                ))),
            ),
        ];
        let mut with_activity = base.clone();
        with_activity.push(event("a1", APRIL, Record::Activity(Box::new(activity))));
        let without = State::fold(&base, APRIL, Currency::USD).unwrap();
        let with = State::fold(&with_activity, APRIL, Currency::USD).unwrap();
        assert_eq!(with.lines(), without.lines());
        assert_eq!(with.message_costs(), without.message_costs());
    }

    #[test]
    fn a_contract_tag_value_brings_its_terms() {
        let mut team = TagValueRecord {
            name: TagName::contract(),
            key: key("team"),
            title: None,
            description: None,
            source_id: None,
            source_status: None,
            defaults: Tags::new(),
            contract: Some(ContractTerms {
                provider: "anthropic".to_owned(),
                kind: ContractKind::Flat,
                fee: Some(usd("150")),
            }),
        };
        let mut project = team.clone();
        project.name = name("project");
        project.key = key("shop");
        let state = State::fold(
            &[
                event("v1", JANUARY, Record::TagValue(team.clone())),
                event("v2", JANUARY, Record::TagValue(project)),
            ],
            APRIL,
            Currency::USD,
        )
        .unwrap();
        assert_eq!(state.contracts().len(), 1);
        assert_eq!(
            state.contracts().get(&key("team")).map(|terms| terms.kind),
            Some(ContractKind::Flat)
        );
        team.contract = None;
        let state = State::fold(
            &[event("v1", JANUARY, Record::TagValue(team))],
            APRIL,
            Currency::USD,
        )
        .unwrap();
        assert!(state.contracts().is_empty());
    }

    #[test]
    fn hand_contracts_are_listed_and_provider_cost_keeps_to_its_range() {
        let in_march = message("s1", "m1", "tom", datetime!(2026-03-10 09:00 UTC), 1);
        let in_april = message("s1", "m2", "tom", datetime!(2026-04-02 09:00 UTC), 1);
        let state = State::fold(
            &[
                event("p1", JANUARY, price_row(TokenKind::INPUT, "10")),
                event("m1", APRIL, Record::Message(Box::new(in_march))),
                event("m2", APRIL, Record::Message(Box::new(in_april))),
                event(
                    "a1",
                    APRIL,
                    Record::ContractAssignment(ContractAssignment {
                        target: session("s1"),
                        contract: key("api"),
                    }),
                ),
            ],
            APRIL.saturating_add(Duration::hours(1)),
            Currency::USD,
        )
        .unwrap();
        assert_eq!(state.contract_assignments().count(), 1);
        assert_eq!(
            state.provider_cost("anthropic", march()).unwrap().amount,
            usd("10")
        );
        assert_eq!(
            state.provider_cost("openai", march()).unwrap().amount,
            usd("0")
        );
    }
}
