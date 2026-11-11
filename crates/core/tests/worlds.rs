//! Generated worlds: the computer makes up small organizations, with
//! messages from several providers and agents, bills under several
//! contracts, tags, hand splits and rules, and checks that the model holds
//! together in each of them (ADR 0028):
//!
//! - cost by any tag name adds up to the total, and so do the contracts
//!   plus "no contract" and the kinds of contract;
//! - the result does not depend on the order in which messages, bills and
//!   tags arrived;
//! - a second source reporting the same calls under built ids changes no
//!   figure.

use proptest::prelude::*;
use time::macros::{date, datetime};
use time::{Duration, OffsetDateTime};
use tokencontroller_core::{
    Bill, ContractAssignment, ContractKind, ContractTerms, Currency, DateRange, Event, Filter,
    Message, MessageId, ModelRef, Money, PriceRow, PriceSource, Record, Rule, Scope, SessionFacts,
    SessionId, Signal, Signals, Split, SplitPart, SplitTarget, State, TagName, TagValue,
    TagValueRecord, Tags, TextPattern, Then, TokenCounts, TokenKind,
};

type Outcome<T> = Result<T, Box<dyn std::error::Error>>;

const AS_OF: OffsetDateTime = datetime!(2026-05-01 00:00 UTC);

/// One made-up message: who, which route, which counts, when, what the
/// sender sent with it.
#[derive(Debug, Clone)]
struct Made {
    person: u8,
    route: u8,
    input: u64,
    output: u64,
    cache_write: u64,
    day: u8,
    minute: u16,
    sent: u8,
    signed_in: bool,
    echo: bool,
}

/// One made-up organization.
#[derive(Debug, Clone)]
struct World {
    messages: Vec<Made>,
    bills: Vec<bool>,
    splits: Vec<(usize, u8)>,
    assignments: Vec<(usize, u8)>,
    team_rule: bool,
    order_seed: u64,
}

fn made() -> impl Strategy<Value = Made> {
    (
        (0_u8..2, 0_u8..5, 0_u64..5_000, 0_u64..2_000, 0_u64..1_000),
        (
            1_u8..29,
            0_u16..1_440,
            0_u8..5,
            any::<bool>(),
            any::<bool>(),
        ),
    )
        .prop_map(
            |(
                (person, route, input, output, cache_write),
                (day, minute, sent, signed_in, echo),
            )| {
                Made {
                    person,
                    route,
                    input,
                    output,
                    cache_write,
                    day,
                    minute,
                    sent,
                    signed_in,
                    echo,
                }
            },
        )
}

fn world() -> impl Strategy<Value = World> {
    (
        prop::collection::vec(made(), 1..16),
        prop::collection::vec(any::<bool>(), 4),
        prop::collection::vec((0_usize..16, 0_u8..4), 0..4),
        prop::collection::vec((0_usize..16, 0_u8..3), 0..3),
        any::<bool>(),
        any::<u64>(),
    )
        .prop_map(
            |(messages, bills, splits, assignments, team_rule, order_seed)| World {
                messages,
                bills,
                splits,
                assignments,
                team_rule,
                order_seed,
            },
        )
}

fn value(text: &str) -> Outcome<TagValue> {
    Ok(TagValue::new(text)?)
}

fn tags(pairs: &[&str]) -> Outcome<Tags> {
    Ok(Tags::parse(pairs.iter().copied())?)
}

fn usd(text: &str) -> Outcome<Money> {
    Ok(Money::parse(text, Currency::USD)?)
}

/// Prices: Claude at Anthropic and Bedrock alike, GPT-5 at `OpenAI` and
/// Azure apart, and the contract `openai-api` with its own input price.
fn prices() -> Outcome<Vec<Record>> {
    let mut rows = Vec::new();
    for (provider, model, input, output, contract) in [
        ("anthropic", "claude-sonnet-4-5", "3", "15", None),
        ("aws_bedrock", "claude-sonnet-4-5", "3", "15", None),
        ("openai", "gpt-5", "1.25", "10", None),
        ("azure_foundry", "gpt-5", "1.5", "10", None),
        ("openai", "gpt-5", "1", "10", Some("openai-api")),
    ] {
        for (kind, price) in [(TokenKind::INPUT, input), (TokenKind::OUTPUT, output)] {
            rows.push(Record::PriceRow(PriceRow {
                provider: provider.to_owned(),
                model: model.to_owned(),
                kind,
                per_million: usd(price)?,
                valid_from: date!(2026 - 01 - 01),
                source: PriceSource::Managed,
                contract: contract.map(value).transpose()?,
            }));
        }
    }
    rows.push(Record::PriceRow(PriceRow {
        provider: "anthropic".to_owned(),
        model: "claude-sonnet-4-5".to_owned(),
        kind: TokenKind::CACHE_WRITE,
        per_million: usd("3.75")?,
        valid_from: date!(2026 - 01 - 01),
        source: PriceSource::Managed,
        contract: None,
    }));
    Ok(rows)
}

fn tag_value(name: &str, key: &str, defaults: &[&str]) -> Outcome<Record> {
    Ok(Record::TagValue(TagValueRecord {
        name: TagName::new(name)?,
        key: value(key)?,
        title: None,
        description: None,
        source_id: None,
        source_status: None,
        defaults: tags(defaults)?,
        contract: None,
    }))
}

/// The provider and model of a route; route 4 reports no provider.
fn route(made: &Made) -> ModelRef {
    match made.route {
        0 => ModelRef::new("anthropic", "claude-sonnet-4-5"),
        1 => ModelRef::new("aws_bedrock", "claude-sonnet-4-5"),
        2 => ModelRef::new("openai", "gpt-5"),
        3 => ModelRef::unreported("claude-sonnet-4-5"),
        _ => ModelRef::unreported("gpt-5"),
    }
}

/// The tags a sender sent: nothing, a project, a feature, a contract, or a
/// project and a contract.
fn sent(made: &Made) -> Outcome<Tags> {
    tags(match made.sent {
        0 => &[],
        1 => &["project=shop"],
        2 => &["feature=KD-1"],
        3 => &["contract=anthropic-api"],
        _ => &["project=bot", "contract=claude-team"],
    })
}

fn at(made: &Made) -> Outcome<OffsetDateTime> {
    Ok(date!(2026 - 03 - 01)
        .replace_day(made.day)?
        .midnight()
        .assume_utc()
        .saturating_add(Duration::minutes(i64::from(made.minute))))
}

/// The message a made-up one becomes. Each message's counts are unique in
/// its world, so a second source's copy has exactly one twin.
fn message(index: usize, made: &Made) -> Outcome<Message> {
    let unique = u64::try_from(index)?;
    let tokens = TokenCounts::new()
        .with(
            TokenKind::INPUT,
            made.input
                .saturating_mul(1_000)
                .saturating_add(unique)
                .saturating_add(1),
        )
        .with(TokenKind::OUTPUT, made.output.saturating_mul(1_000))
        .with(
            TokenKind::CACHE_WRITE,
            made.cache_write.saturating_mul(1_000),
        );
    let mut signals = Signals::default();
    if made.signed_in {
        signals
            .resource
            .insert("organization.id".to_owned(), "org-team".to_owned());
    }
    Ok(Message {
        session_id: SessionId::new(&format!("s{}", index % 5))?,
        message_id: MessageId::new(&format!("m{index}"))?,
        person: value(if made.person == 0 { "mara" } else { "tom" })?,
        model: route(made),
        tokens: Some(tokens),
        at: at(made)?,
        agent_harness: "claude-code".to_owned(),
        agent_name: None,
        reported_cost: None,
        tags: sent(made)?,
        signals,
        session: SessionFacts::default(),
    })
}

/// The same call as a second source reports it: a built id, a few seconds
/// later, and its cache writes told as 5-minute writes.
fn echo(message: &Message) -> Outcome<Message> {
    let mut copy = message.clone();
    copy.message_id = MessageId::new(&format!("otel:{}", message.message_id))?;
    copy.at = message.at.saturating_add(Duration::seconds(4));
    let mut counts = message.tokens.clone().unwrap_or_default();
    let writes = counts.get(&TokenKind::CACHE_WRITE);
    counts.set(TokenKind::CACHE_WRITE, 0);
    counts.set(TokenKind::CACHE_WRITE_5M, writes);
    copy.tokens = Some(counts);
    copy.tags = Tags::new();
    Ok(copy)
}

/// Every record of a world, with or without the second source's copies.
fn records(world: &World, with_echoes: bool) -> Outcome<Vec<Record>> {
    let mut all = prices()?;
    all.push(tag_value("project", "shop", &[])?);
    all.push(tag_value("project", "bot", &["contract=anthropic-api"])?);
    all.push(tag_value("feature", "KD-1", &["project=shop"])?);
    let march = DateRange::new(date!(2026 - 03 - 01), date!(2026 - 03 - 31))?;
    let bills = [
        ("anthropic", Some("anthropic-api"), ContractKind::Metered),
        ("anthropic", Some("claude-team"), ContractKind::Flat),
        ("openai", Some("openai-api"), ContractKind::Metered),
        ("aws_bedrock", None, ContractKind::Metered),
    ];
    for ((provider, contract, kind), on) in bills.iter().zip(&world.bills) {
        if *on {
            all.push(Record::Bill(Bill {
                provider: (*provider).to_owned(),
                contract: contract.map(value).transpose()?,
                contract_kind: Some(*kind),
                range: march,
                amount: usd("100")?,
                fee: None,
                extra_usage: None,
            }));
        }
    }
    if world.team_rule {
        all.push(Record::Rule(Rule {
            name: "team sign-in".to_owned(),
            scope: Scope::Organization,
            order: 1,
            signal: Signal::Resource("organization.id".to_owned()),
            pattern: TextPattern::new("^org-team$")?,
            then: Then::Tag {
                name: TagName::contract(),
                key: value("claude-team")?,
            },
            contract: Some(ContractTerms {
                provider: "anthropic".to_owned(),
                kind: ContractKind::Flat,
                fee: Some(usd("20")?),
            }),
        }));
    }
    for (index, made) in world.messages.iter().enumerate() {
        let message = message(index, made)?;
        if with_echoes && made.echo {
            all.push(Record::Message(Box::new(echo(&message)?)));
        }
        all.push(Record::Message(Box::new(message)));
    }
    for (session, shape) in &world.splits {
        let parts: Vec<(u64, &[&str])> = match shape {
            0 => vec![(1, &["project=shop"])],
            1 => vec![(1, &["project=bot"])],
            2 => vec![(50, &["project=bot"]), (50, &["project=shop"])],
            _ => vec![(3, &["feature=KD-1"]), (1, &[])],
        };
        all.push(Record::Split(Split {
            target: SplitTarget::Session(SessionId::new(&format!("s{}", session % 5))?),
            parts: parts
                .into_iter()
                .map(|(weight, pairs)| {
                    Ok(SplitPart {
                        weight,
                        tags: tags(pairs)?,
                    })
                })
                .collect::<Outcome<Vec<_>>>()?,
        }));
    }
    for (session, contract) in &world.assignments {
        let key = ["anthropic-api", "claude-team", "openai-api"]
            .get(usize::from(*contract))
            .copied()
            .unwrap_or("anthropic-api");
        all.push(Record::ContractAssignment(ContractAssignment {
            target: SplitTarget::Session(SessionId::new(&format!("s{}", session % 5))?),
            contract: value(key)?,
        }));
    }
    Ok(all)
}

/// The records as events, recorded in an order the seed shuffles. Later
/// assignments of the same target keep their place after earlier ones.
fn events(records: &[Record], seed: u64) -> Outcome<Vec<Event>> {
    let count = records.len();
    let mut order: Vec<usize> = (0..count).collect();
    let mut state = seed | 1;
    for slot in (1..count).rev() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let other = usize::try_from(
            state
                .checked_rem(u64::try_from(slot.saturating_add(1))?)
                .ok_or("no slot")?,
        )?;
        order.swap(slot, other);
    }
    // Hand assignments on one session supersede each other in the order
    // given, so they keep that order among themselves.
    let mut assignments: Vec<usize> = order
        .iter()
        .copied()
        .filter(|index| {
            matches!(
                records.get(*index),
                Some(Record::Split(_) | Record::ContractAssignment(_))
            )
        })
        .collect();
    assignments.sort_unstable();
    let mut sorted = assignments.into_iter();
    for slot in &mut order {
        if matches!(
            records.get(*slot),
            Some(Record::Split(_) | Record::ContractAssignment(_))
        ) && let Some(next) = sorted.next()
        {
            *slot = next;
        }
    }
    let mut events = Vec::with_capacity(count);
    for (position, index) in order.into_iter().enumerate() {
        let record = records.get(index).ok_or("no record")?.clone();
        let mut event = Event::new(&format!("e{index}"), AS_OF, record)?;
        event.recorded_at = datetime!(2026-04-01 00:00 UTC)
            .saturating_add(Duration::seconds(i64::try_from(position)?));
        events.push(event);
    }
    Ok(events)
}

fn fold(world: &World, with_echoes: bool, seed: u64) -> Outcome<State> {
    let events = events(&records(world, with_echoes)?, seed)?;
    Ok(State::fold(&events, AS_OF, Currency::USD)?)
}

fn march() -> Outcome<DateRange> {
    Ok(DateRange::new(
        date!(2026 - 03 - 01),
        date!(2026 - 03 - 31),
    )?)
}

/// Every figure a world answers, as text, to compare two folds.
fn figures(state: &State) -> Outcome<Vec<String>> {
    let everything = Filter::default();
    let mut out = vec![format!("{:?}", state.cost(&everything, march()?)?)];
    for name in ["person", "project", "feature", "contract"] {
        out.push(format!(
            "{:?}",
            state.by_tag(&everything, march()?, &TagName::new(name)?)?
        ));
    }
    out.push(format!(
        "{:?}",
        state.by_contract_kind(&everything, march()?)?
    ));
    let mut checks = Vec::new();
    for bill in state.bills() {
        checks.push(format!("{:?}", state.reconcile(&bill.value)?));
    }
    checks.sort();
    out.extend(checks);
    out.push(format!("{:?}", state.contracts()));
    Ok(out)
}

fn adds_up(state: &State) -> Outcome<()> {
    let everything = Filter::default();
    let total = state.cost(&everything, march()?)?.amount;
    for name in ["person", "project", "feature", "client", "contract"] {
        let parts = state.by_tag(&everything, march()?, &TagName::new(name)?)?;
        let sum = Money::sum(parts.into_iter().map(|(_, amount)| amount), Currency::USD)?;
        if sum != total {
            return Err(format!("{name} adds up to {sum}, not {total}").into());
        }
    }
    let kinds = state.by_contract_kind(&everything, march()?)?;
    let sum = Money::sum(kinds.into_iter().map(|(_, amount)| amount), Currency::USD)?;
    if sum != total {
        return Err(format!("the kinds of contract add up to {sum}, not {total}").into());
    }
    for line in state.lines() {
        let contract = line.tags.get(&TagName::contract());
        let cost = state
            .message_costs()
            .iter()
            .find(|cost| cost.session_id == line.session_id && cost.message_id == line.message_id)
            .ok_or("a line without its message")?;
        if contract != cost.contract.as_ref() {
            return Err("a line's contract is not its message's".into());
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn cost_by_any_tag_name_adds_up_to_the_total(world in world()) {
        let state = fold(&world, true, world.order_seed).unwrap();
        prop_assert!(adds_up(&state).is_ok(), "{:?}", adds_up(&state));
    }

    #[test]
    fn the_order_records_arrive_in_changes_nothing(world in world(), other_seed in any::<u64>()) {
        let one = figures(&fold(&world, true, world.order_seed).unwrap()).unwrap();
        let other = figures(&fold(&world, true, other_seed).unwrap()).unwrap();
        prop_assert_eq!(one, other);
    }

    #[test]
    fn a_second_source_of_the_same_calls_changes_no_figure(world in world()) {
        let alone = fold(&world, false, world.order_seed).unwrap();
        let both = fold(&world, true, world.order_seed).unwrap();
        prop_assert_eq!(figures(&alone).unwrap(), figures(&both).unwrap());
        let echoes = world.messages.iter().filter(|made| made.echo).count();
        prop_assert_eq!(both.matched().len(), echoes);
    }
}
