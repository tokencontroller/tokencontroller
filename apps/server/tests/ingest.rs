//! Ingest against a real Postgres database: telemetry and session records
//! become message events, splits divide a session, rules and defaults tag
//! the lines, and the price table gives every figure.

use std::collections::BTreeMap;
use std::sync::Arc;

use sqlx::PgPool;
use time::OffsetDateTime;
use time::macros::{date, datetime};
use tokencontroller_core::rules::Scope;
use tokencontroller_core::{
    Currency, DateRange, Filter, Money, NoExtension, PriceRow, Record, TagName, TagValue,
    TagValueRecord, Tags, TokenCounts, TokenKind,
};
use tokencontroller_server::events::{self, Log};
use tokencontroller_server::ingest::entry::NewTokenEntry;
use tokencontroller_server::ingest::model::{TelemetryHarness, TelemetryMessage};
use tokencontroller_server::ingest::store::{self, StoredEntry, TelemetryOutcome};
use tokencontroller_server::organizations::rules::{NewRule, parse_rule};
use tokencontroller_server::{AppState, Config, price_table};
use uuid::Uuid;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

fn config() -> Config {
    Config {
        database_url: "postgres://unused".to_owned(),
        public_url: "http://localhost:8080".to_owned(),
        secret_key: "0123456789abcdef0123456789abcdef".to_owned(),
        smtp_url: None,
        smtp_from: None,
        price_table: None,
        rates_refresh: false,
        classifier: None,
    }
}

struct Kestrel {
    state: AppState,
    organization_id: Uuid,
    mara: TagValue,
}

/// Kestrel Digital with Mara as its org manager, the feature values KD-214
/// and KD-215 under the project shop, and a managed price of 10 US dollars
/// per million input tokens for the model the tests send.
async fn kestrel(pool: &PgPool) -> TestResult<Kestrel> {
    let state = AppState::new(pool.clone(), config(), Arc::new(NoExtension));
    let mut conn = pool.acquire().await?;
    let created = tokencontroller_server::organizations::create(
        &mut conn,
        "Kestrel Digital",
        "mara@kestrel.example",
    )
    .await
    .map_err(|error| error.to_string())?;
    let organization_id = created.organization_id;
    let shop = value("project", "shop", &[])?;
    let kd_214 = value("feature", "KD-214", &[("project", "shop")])?;
    let kd_215 = value("feature", "KD-215", &[("project", "shop")])?;
    events::append_all(pool, organization_id, &[shop, kd_214, kd_215]).await?;
    price_table::store_managed(pool, &[price("claude-sonnet-5", TokenKind::Input, "10")?]).await?;
    Ok(Kestrel {
        state,
        organization_id,
        mara: TagValue::new("mara@kestrel.example")?,
    })
}

fn value(
    name: &str,
    key: &str,
    defaults: &[(&str, &str)],
) -> TestResult<tokencontroller_core::Event> {
    let mut tags = Tags::new();
    for (name, key) in defaults {
        tags.insert(TagName::new(name)?, TagValue::new(key)?);
    }
    Ok(events::event(Record::TagValue(TagValueRecord {
        name: TagName::new(name)?,
        key: TagValue::new(key)?,
        title: None,
        description: None,
        source_id: None,
        source_status: None,
        defaults: tags,
    }))?)
}

fn price(model: &str, kind: TokenKind, per_million: &str) -> TestResult<PriceRow> {
    Ok(PriceRow {
        provider: "anthropic".to_owned(),
        model: model.to_owned(),
        kind,
        per_million: Money::parse(per_million, Currency::USD)?,
        valid_from: date!(2020 - 01 - 01),
        source: tokencontroller_core::PriceSource::Managed,
    })
}

fn call(session: &str, id: &str, at: OffsetDateTime, input: u64) -> TelemetryMessage {
    TelemetryMessage {
        harness: TelemetryHarness::ClaudeCode,
        session_id: session.to_owned(),
        message_id: id.to_owned(),
        model: "claude-sonnet-5".to_owned(),
        occurred_at: at,
        tokens: TokenCounts {
            input,
            ..TokenCounts::default()
        },
        reported_cost: None,
        person_email: None,
        person_account_id: None,
        repository: None,
        attributes: BTreeMap::new(),
    }
}

/// A session record with the fields the test leaves out filled in.
fn entry(value: &serde_json::Value) -> TestResult<NewTokenEntry> {
    let mut full = serde_json::json!({
        "agent_harness": "claude-code",
        "model": "claude-sonnet-5",
        "started_at": "2026-10-05T09:00:00Z",
        "ended_at": "2026-10-05T09:30:00Z",
        "tokens": { "input": 100, "output": 0, "cache_read": 0, "cache_write": 0 },
    });
    if let (Some(full), Some(given)) = (full.as_object_mut(), value.as_object()) {
        for (key, value) in given {
            full.insert(key.clone(), value.clone());
        }
    }
    Ok(serde_json::from_value(full)?)
}

async fn record(kestrel: &Kestrel, calls: Vec<TelemetryMessage>) -> TestResult<TelemetryOutcome> {
    Ok(store::store_telemetry(
        &kestrel.state,
        kestrel.organization_id,
        &kestrel.mara,
        calls,
    )
    .await?)
}

async fn submit(kestrel: &Kestrel, entries: Vec<NewTokenEntry>) -> TestResult<Vec<StoredEntry>> {
    let mut checked = Vec::new();
    for entry in entries {
        checked.push(entry.check().map_err(|error| error.to_string())?);
    }
    Ok(store::store_entries(
        &kestrel.state,
        kestrel.organization_id,
        &kestrel.mara,
        checked,
    )
    .await?)
}

async fn fold(kestrel: &Kestrel) -> TestResult<tokencontroller_core::state::State> {
    let log = Log::load(&kestrel.state, kestrel.organization_id).await?;
    Ok(log.fold(datetime!(2027-01-01 00:00 UTC))?)
}

const OCTOBER: DateRange = DateRange {
    from: date!(2026 - 10 - 01),
    to: date!(2026 - 10 - 31),
};

fn usd(text: &str) -> Money {
    Money::parse(text, Currency::USD).unwrap_or_else(|_| Money::zero(Currency::USD))
}

async fn message_events(pool: &PgPool, session: &str) -> TestResult<i64> {
    Ok(
        sqlx::query_scalar("SELECT COUNT(*) FROM message_events WHERE session_id = $1")
            .bind(session)
            .fetch_one(pool)
            .await?,
    )
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn telemetry_counts_each_model_call_once(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    let calls = vec![
        call("s1", "msg_1", datetime!(2026-10-05 09:00 UTC), 300_000),
        call("s1", "msg_2", datetime!(2026-10-05 09:10 UTC), 700_000),
    ];
    let first = record(&kestrel, calls.clone()).await.unwrap();
    assert_eq!(first.new_messages, 2);
    assert_eq!(first.known_messages, 0);
    let again = record(&kestrel, calls).await.unwrap();
    assert_eq!(again.new_messages, 0);
    assert_eq!(again.known_messages, 2);
    assert_eq!(message_events(&pool, "s1").await.unwrap(), 2);

    let fold = fold(&kestrel).await.unwrap();
    assert_eq!(fold.lines().len(), 2);
    assert!(fold.lines().iter().all(|line| line.person == kestrel.mara));
    // One million input tokens at 10 US dollars per million.
    let cost = fold.cost(&Filter(Tags::new()), OCTOBER).unwrap();
    assert_eq!(cost.amount, usd("10.00"));
    assert!(cost.unknown_models.is_empty());
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_session_record_adds_the_calls_telemetry_missed(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    record(
        &kestrel,
        vec![
            call("s1", "msg_1", datetime!(2026-10-05 09:00 UTC), 30),
            call("s1", "msg_2", datetime!(2026-10-05 09:10 UTC), 70),
        ],
    )
    .await
    .unwrap();
    let sent = entry(&serde_json::json!({
        "session_id": "s1",
        "tokens": { "input": 105, "output": 0, "cache_read": 0, "cache_write": 0 },
        "messages": [
            { "message_id": "msg_1", "at": "2026-10-05T09:00:00Z", "tokens": { "input": 30, "output": 0, "cache_read": 0, "cache_write": 0 } },
            { "message_id": "msg_2", "at": "2026-10-05T09:10:00Z", "tokens": { "input": 70, "output": 0, "cache_read": 0, "cache_write": 0 } },
            { "message_id": "msg_3", "at": "2026-10-05T09:20:00Z", "tokens": { "input": 5, "output": 0, "cache_read": 0, "cache_write": 0 } },
        ],
    }))
    .unwrap();
    let stored = submit(&kestrel, vec![sent]).await.unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].session_id, "s1");
    assert_eq!(stored[0].messages, 3);
    assert_eq!(message_events(&pool, "s1").await.unwrap(), 3);
    let fold = fold(&kestrel).await.unwrap();
    let input: u64 = fold
        .messages()
        .filter_map(|message| message.value.tokens.as_ref())
        .map(|tokens| tokens.input)
        .sum();
    assert_eq!(input, 105, "each call counts once");
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_record_without_messages_is_one_message_under_its_session(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    let sent = entry(&serde_json::json!({ "session_id": "s2", "feature": "KD-214" })).unwrap();
    let stored = submit(&kestrel, vec![sent]).await.unwrap();
    assert_eq!(stored[0].messages, 1);
    assert_eq!(stored[0].label, "direct");
    assert!(stored[0].unknown_tags.is_empty(), "KD-214 is a known tag");
    assert_eq!(message_events(&pool, "s2").await.unwrap(), 1);

    // Without a session id the local id stands in; without either, a fresh one.
    let local = entry(&serde_json::json!({ "local_id": "desk:7" })).unwrap();
    let stored = submit(&kestrel, vec![local]).await.unwrap();
    assert_eq!(stored[0].session_id, "desk:7");
    let fresh = entry(&serde_json::json!({})).unwrap();
    let stored = submit(&kestrel, vec![fresh]).await.unwrap();
    assert!(stored[0].session_id.starts_with("entry-"));
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_same_local_id_adds_nothing(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    let first = entry(&serde_json::json!({ "local_id": "support-desk:1" })).unwrap();
    let second = entry(&serde_json::json!({
        "local_id": "support-desk:1",
        "tokens": { "input": 250, "output": 0, "cache_read": 0, "cache_write": 0 },
    }))
    .unwrap();
    let a = submit(&kestrel, vec![first]).await.unwrap();
    let b = submit(&kestrel, vec![second]).await.unwrap();
    assert_eq!(a[0].session_id, b[0].session_id);
    assert_eq!(a[0].cost, b[0].cost);
    let messages: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events WHERE kind = 'message'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(messages, 1);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_split_divides_the_session_by_fraction(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    let sent = entry(&serde_json::json!({
        "session_id": "s4",
        "tokens": { "input": 1_000_000, "output": 0, "cache_read": 0, "cache_write": 0 },
        "splits": [
            { "fraction": "0.6", "tags": { "feature": "KD-214" } },
            { "fraction": 0.4, "tags": { "feature": "KD-215" } }
        ]
    }))
    .unwrap();
    let stored = submit(&kestrel, vec![sent]).await.unwrap();
    assert_eq!(stored[0].cost, "10.00");
    let fold = fold(&kestrel).await.unwrap();
    let mut parts: Vec<(String, Money)> = fold
        .lines()
        .iter()
        .map(|line| {
            (
                line.tags
                    .get(&TagName::feature())
                    .map(ToString::to_string)
                    .unwrap_or_default(),
                line.amount,
            )
        })
        .collect();
    parts.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        parts,
        [
            ("KD-214".to_owned(), usd("6.00")),
            ("KD-215".to_owned(), usd("4.00")),
        ]
    );
    // Both parts bring the project the feature belongs to.
    assert!(fold.lines().iter().all(|line| {
        line.tags
            .get(&TagName::project())
            .is_some_and(|key| key.as_str() == "shop")
    }));
    let mut shop = Tags::new();
    shop.insert(TagName::project(), TagValue::new("shop").unwrap());
    assert_eq!(
        fold.cost(&Filter(shop), OCTOBER).unwrap().amount,
        usd("10.00")
    );
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn an_organization_rule_reads_the_feature_from_the_branch(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    let rule = parse_rule(
        &NewRule {
            name: "feature key in branch".to_owned(),
            order: None,
            signal: "branch".to_owned(),
            pattern: r"(?P<key>KD-\d+)".to_owned(),
            tag_name: "feature".to_owned(),
            key: None,
        },
        Scope::Organization,
        1,
    )
    .unwrap();
    let rule = events::event(Record::Rule(rule)).unwrap();
    events::append_all(&pool, kestrel.organization_id, &[rule])
        .await
        .unwrap();

    let sent = entry(&serde_json::json!({
        "session_id": "s5",
        "signals": { "branch": "feature/KD-215-cart-total" },
    }))
    .unwrap();
    submit(&kestrel, vec![sent]).await.unwrap();
    // A key that is no value on record does not match.
    let unknown = entry(&serde_json::json!({
        "session_id": "s6",
        "signals": { "branch": "feature/KD-999-nothing" },
    }))
    .unwrap();
    submit(&kestrel, vec![unknown]).await.unwrap();

    let fold = fold(&kestrel).await.unwrap();
    let feature_of = |session: &str| {
        fold.lines()
            .iter()
            .find(|line| line.session_id.as_str() == session)
            .and_then(|line| line.tags.get(&TagName::feature()))
            .map(ToString::to_string)
    };
    assert_eq!(feature_of("s5").as_deref(), Some("KD-215"));
    assert_eq!(feature_of("s6"), None);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn telemetry_keeps_the_attributes_on_the_tag_list_and_the_members_email(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO people (organization_id, email, name, joined_at) VALUES ($1, 'tom@kestrel.example', 'Tom', now())",
    )
    .bind(kestrel.organization_id)
    .execute(&pool)
    .await
    .unwrap();
    let mut first = call("s7", "msg_1", datetime!(2026-10-05 09:00 UTC), 10);
    first.repository = Some("github.com/kestrel/storefront".to_owned());
    first.person_email = Some("Tom@kestrel.example".to_owned());
    first
        .attributes
        .insert("team".to_owned(), "platform".to_owned());
    first
        .attributes
        .insert("os.type".to_owned(), "linux".to_owned());
    let mut second = call("s7", "msg_2", datetime!(2026-10-05 09:05 UTC), 10);
    second.person_email = Some("nobody@elsewhere.example".to_owned());
    record(&kestrel, vec![first, second]).await.unwrap();

    let fold = fold(&kestrel).await.unwrap();
    let mut messages: Vec<_> = fold
        .messages()
        .map(|message| message.value.clone())
        .collect();
    messages.sort_by(|a, b| a.message_id.cmp(&b.message_id));
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].person.as_str(), "tom@kestrel.example");
    assert_eq!(
        messages[0]
            .tags
            .get(&TagName::new("team").unwrap())
            .map(ToString::to_string)
            .as_deref(),
        Some("platform")
    );
    assert!(messages[0].tags.get(&TagName::new("os").unwrap()).is_none());
    assert_eq!(
        messages[0].signals.repository.as_deref(),
        Some("github.com/kestrel/storefront")
    );
    assert_eq!(
        messages[0]
            .signals
            .resource
            .get("os.type")
            .map(String::as_str),
        Some("linux")
    );
    // An address that is no member's falls back to the token's creator.
    assert_eq!(messages[1].person, kestrel.mara);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_new_price_row_prices_messages_whose_model_was_unknown(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    let sent = entry(&serde_json::json!({
        "session_id": "s8",
        "model": "claude-brand-new",
        "tokens": { "input": 2_000_000, "output": 0, "cache_read": 0, "cache_write": 0 },
    }))
    .unwrap();
    let stored = submit(&kestrel, vec![sent]).await.unwrap();
    assert_eq!(stored[0].cost, "0.00");
    let before = fold(&kestrel).await.unwrap();
    let cost = before.cost(&Filter(Tags::new()), OCTOBER).unwrap();
    assert_eq!(cost.amount, usd("0.00"));
    assert_eq!(cost.unknown_models.len(), 1);

    price_table::store_managed(
        &pool,
        &[price("claude-brand-new", TokenKind::Input, "3").unwrap()],
    )
    .await
    .unwrap();
    let after = fold(&kestrel).await.unwrap();
    let cost = after.cost(&Filter(Tags::new()), OCTOBER).unwrap();
    // Two million tokens at 3 US dollars per million.
    assert_eq!(cost.amount, usd("6.00"));
    assert!(cost.unknown_models.is_empty());
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_broken_record_stores_nothing(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    let backwards = entry(&serde_json::json!({ "ended_at": "2026-10-05T08:00:00Z" })).unwrap();
    assert!(backwards.check().is_err());
    let bad_fraction = entry(&serde_json::json!({
        "splits": [{ "fraction": "0.5", "tags": { "feature": "KD-214" } }]
    }))
    .unwrap();
    assert!(bad_fraction.check().is_err());

    // A tag name the organization has not is refused by the store.
    let guild = entry(&serde_json::json!({ "tags": { "guild": "rust" } })).unwrap();
    let refused = submit(&kestrel, vec![guild]).await;
    assert!(refused.is_err());
    let messages: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events WHERE kind = 'message'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(messages, 0);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_seed_is_the_worked_month_and_loads_twice(pool: PgPool) {
    tokencontroller_server::seed::load(&pool).await.unwrap();
    let count = async || -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM events")
            .fetch_one(&pool)
            .await
            .unwrap()
    };
    let first = count().await;
    assert!(first > 0);
    tokencontroller_server::seed::load(&pool).await.unwrap();
    assert_eq!(count().await, first, "loading again adds nothing");

    let state = AppState::new(pool.clone(), config(), Arc::new(NoExtension));
    let log = Log::load(&state, tokencontroller_server::seed::ids::ORGANIZATION)
        .await
        .unwrap();
    let fold = log.fold(datetime!(2026-04-01 00:00 UTC)).unwrap();
    let march = DateRange::new(date!(2026 - 03 - 01), date!(2026 - 03 - 31)).unwrap();
    let total = fold.cost(&Filter(Tags::new()), march).unwrap();
    assert_eq!(total.amount, usd("950.00"));
    assert!(total.unknown_models.is_empty());
    assert!(total.count_errors.is_empty());
    let mut shop = Tags::new();
    shop.insert(TagName::project(), TagValue::new("shop").unwrap());
    assert_eq!(
        fold.cost(&Filter(shop), march).unwrap().amount,
        usd("800.00")
    );
}
