//! Storing token entries and telemetry against a real Postgres database:
//! one row per model call, one token entry per session, whichever door a
//! call came through first (ADR 0006, ADR 0018).

use std::collections::BTreeMap;

use sqlx::PgPool;
use time::OffsetDateTime;
use time::macros::datetime;
use tokencontroller_core::{Currency, Money, TokenCounts};
use tokencontroller_server::ingest::entry::NewTokenEntry;
use tokencontroller_server::ingest::model::{TelemetryHarness, TelemetryMessage};
use tokencontroller_server::ingest::store::{self, Door, Pricing, StoreError, TelemetryOutcome};
use uuid::Uuid;

async fn organization(pool: &PgPool) -> Result<(Uuid, Uuid), sqlx::Error> {
    let organization: Uuid = sqlx::query_scalar(
        "INSERT INTO organizations (name, slug) VALUES ('Kestrel Digital', 'kestrel') RETURNING id",
    )
    .fetch_one(pool)
    .await?;
    let person: Uuid = sqlx::query_scalar(
        "INSERT INTO people (organization_id, email, name, tags)
         VALUES ($1, 'mara@kestrel.example', 'Mara', '{\"team\": \"web\"}') RETURNING id",
    )
    .bind(organization)
    .fetch_one(pool)
    .await?;
    sqlx::query(
        "INSERT INTO tag_names (organization_id, name) VALUES ($1, 'team'), ($1, 'client')",
    )
    .bind(organization)
    .execute(pool)
    .await?;
    let project: Uuid = sqlx::query_scalar(
        "INSERT INTO projects (organization_id, name, ticket_key_pattern, tags)
         VALUES ($1, 'storefront', '^KD-\\d+', '{\"client\": \"acme\"}') RETURNING id",
    )
    .bind(organization)
    .fetch_one(pool)
    .await?;
    sqlx::query(
        "INSERT INTO project_repositories (organization_id, repository, project_id)
         VALUES ($1, 'kestrel/storefront', $2)",
    )
    .bind(organization)
    .bind(project)
    .execute(pool)
    .await?;
    sqlx::query("INSERT INTO tickets (organization_id, key, project_id, title) VALUES ($1, 'KD-214', $2, 'Cart total')")
        .bind(organization)
        .bind(project)
        .execute(pool)
        .await?;
    Ok((organization, person))
}

async fn split_tags(
    pool: &PgPool,
    entry: Uuid,
) -> Result<Vec<Vec<(String, String, String)>>, sqlx::Error> {
    let rows: Vec<(i32, String, String, String)> = sqlx::query_as(
        "SELECT s.position, t.name, t.value, t.source FROM split_tags t JOIN splits s ON s.id = t.split_id
         WHERE s.token_entry_id = $1 ORDER BY s.position, t.name",
    )
    .bind(entry)
    .fetch_all(pool)
    .await?;
    let mut splits: Vec<Vec<(String, String, String)>> = Vec::new();
    for (position, name, value, source) in rows {
        let position = usize::try_from(position).unwrap_or_default();
        while splits.len() <= position {
            splits.push(Vec::new());
        }
        if let Some(split) = splits.get_mut(position) {
            split.push((name, value, source));
        }
    }
    Ok(splits)
}

fn tag(name: &str, value: &str, source: &str) -> (String, String, String) {
    (name.to_owned(), value.to_owned(), source.to_owned())
}

/// Prices every model at one nano-dollar per token.
fn pricing() -> Pricing {
    Pricing::new(None, |_, tokens: TokenCounts| {
        let total = i64::try_from(tokens.total()?).ok()?;
        Some(Money::from_nanos(total, Currency::USD))
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
        person_email: Some("Mara@kestrel.example".to_owned()),
        person_account_id: None,
        repository: None,
        attributes: BTreeMap::new(),
    }
}

fn entry(value: &serde_json::Value) -> Result<NewTokenEntry, serde_json::Error> {
    let mut base = serde_json::json!({
        "agent_harness": "claude-code",
        "model": "claude-sonnet-5",
        "started_at": "2026-10-05T09:00:00Z",
        "ended_at": "2026-10-05T10:00:00Z",
        "tokens": { "input": 100 },
    });
    if let (Some(base), Some(extra)) = (base.as_object_mut(), value.as_object()) {
        for (key, value) in extra {
            base.insert(key.clone(), value.clone());
        }
    }
    serde_json::from_value(base)
}

async fn record(
    pool: &PgPool,
    organization: Uuid,
    calls: Vec<TelemetryMessage>,
) -> Result<TelemetryOutcome, StoreError> {
    let mut tx = pool.begin().await?;
    let outcome = store::record_telemetry(&mut tx, organization, calls, &pricing()).await?;
    tx.commit().await?;
    Ok(outcome)
}

async fn submit(
    pool: &PgPool,
    organization: Uuid,
    person: Uuid,
    entries: Vec<NewTokenEntry>,
    door: Door,
) -> Result<Vec<store::StoredEntry>, StoreError> {
    let mut tx = pool.begin().await?;
    let stored =
        store::store_token_entries(&mut tx, organization, person, entries, door, &pricing())
            .await?;
    tx.commit().await?;
    Ok(stored)
}

#[derive(Debug, PartialEq, Eq)]
struct Row {
    door: String,
    status: String,
    input_tokens: i64,
    list_value_nanos: Option<i64>,
    agent_manager_id: Option<Uuid>,
}

async fn entries_of(pool: &PgPool, session: &str) -> Result<Vec<Row>, sqlx::Error> {
    sqlx::query_as!(
        Row,
        "SELECT door, status, input_tokens, list_value_nanos, agent_manager_id
         FROM token_entries WHERE session_id = $1",
        session
    )
    .fetch_all(pool)
    .await
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn telemetry_builds_one_automatic_token_entry_per_session(pool: PgPool) {
    let (organization, person) = organization(&pool).await.unwrap();
    let calls = vec![
        call("s1", "msg_1", datetime!(2026-10-05 09:00 UTC), 30),
        call("s1", "msg_2", datetime!(2026-10-05 09:10 UTC), 70),
    ];

    let first = record(&pool, organization, calls.clone()).await.unwrap();
    assert_eq!(first.new_messages, 2);
    assert_eq!(first.token_entries, 1);
    let again = record(&pool, organization, calls).await.unwrap();
    assert_eq!(again.new_messages, 0);
    assert_eq!(again.known_messages, 2);

    assert_eq!(
        entries_of(&pool, "s1").await.unwrap(),
        vec![Row {
            door: "telemetry".to_owned(),
            status: "submitted".to_owned(),
            input_tokens: 100,
            list_value_nanos: Some(100),
            agent_manager_id: Some(person),
        }]
    );
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_token_sheet_entry_takes_over_the_session_and_counts_each_call_once(pool: PgPool) {
    let (organization, person) = organization(&pool).await.unwrap();
    record(
        &pool,
        organization,
        vec![
            call("s1", "msg_1", datetime!(2026-10-05 09:00 UTC), 30),
            call("s1", "msg_2", datetime!(2026-10-05 09:10 UTC), 70),
        ],
    )
    .await
    .unwrap();

    // The session file knows a third call telemetry missed.
    let sent = entry(&serde_json::json!({
        "session_id": "s1",
        "tokens": { "input": 105 },
        "messages": [
            { "message_id": "msg_1", "at": "2026-10-05T09:00:00Z", "tokens": { "input": 30 } },
            { "message_id": "msg_2", "at": "2026-10-05T09:10:00Z", "tokens": { "input": 70 } },
            { "message_id": "msg_3", "at": "2026-10-05T09:20:00Z", "tokens": { "input": 5 } },
        ],
    }))
    .unwrap();
    let stored = submit(&pool, organization, person, vec![sent], Door::TokenSheet)
        .await
        .unwrap();
    assert_eq!(stored.len(), 1);

    assert_eq!(
        entries_of(&pool, "s1").await.unwrap(),
        vec![Row {
            door: "token_sheet".to_owned(),
            status: "submitted".to_owned(),
            input_tokens: 105,
            list_value_nanos: Some(105),
            agent_manager_id: Some(person),
        }]
    );
    let messages: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE session_id = 's1'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(messages, 3);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn an_entry_without_messages_counts_until_telemetry_reports_the_session(pool: PgPool) {
    let (organization, person) = organization(&pool).await.unwrap();
    let sent =
        entry(&serde_json::json!({ "session_id": "s2", "tokens": { "input": 100 } })).unwrap();
    submit(&pool, organization, person, vec![sent], Door::TokenSheet)
        .await
        .unwrap();
    assert_eq!(entries_of(&pool, "s2").await.unwrap()[0].input_tokens, 100);

    record(
        &pool,
        organization,
        vec![
            call("s2", "msg_1", datetime!(2026-10-05 09:00 UTC), 50),
            call("s2", "msg_2", datetime!(2026-10-05 09:05 UTC), 30),
        ],
    )
    .await
    .unwrap();

    let rows = entries_of(&pool, "s2").await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].door, "token_sheet");
    assert_eq!(rows[0].input_tokens, 80);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn built_ids_stop_counting_when_the_entry_sends_real_calls(pool: PgPool) {
    let (organization, person) = organization(&pool).await.unwrap();
    // Telemetry without message IDs of its own: built from the event.
    record(
        &pool,
        organization,
        vec![
            call("s3", "otel:a", datetime!(2026-10-05 09:00 UTC), 40),
            call("s3", "otel:b", datetime!(2026-10-05 09:05 UTC), 60),
        ],
    )
    .await
    .unwrap();
    let sent = entry(&serde_json::json!({
        "session_id": "s3",
        "tokens": { "input": 90 },
        "messages": [
            { "message_id": "msg_1", "at": "2026-10-05T09:00:00Z", "tokens": { "input": 40 } },
            { "message_id": "msg_2", "at": "2026-10-05T09:05:00Z", "tokens": { "input": 50 } },
        ],
    }))
    .unwrap();
    submit(&pool, organization, person, vec![sent], Door::TokenSheet)
        .await
        .unwrap();
    assert_eq!(entries_of(&pool, "s3").await.unwrap()[0].input_tokens, 90);

    // A later export of the same built calls changes nothing.
    record(
        &pool,
        organization,
        vec![call("s3", "otel:c", datetime!(2026-10-05 09:10 UTC), 10)],
    )
    .await
    .unwrap();
    assert_eq!(entries_of(&pool, "s3").await.unwrap()[0].input_tokens, 90);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn sending_again_with_the_same_local_id_updates_the_entry(pool: PgPool) {
    let (organization, person) = organization(&pool).await.unwrap();
    let first = entry(
        &serde_json::json!({ "local_id": "support-desk:1", "agent_harness": "support-desk" }),
    )
    .unwrap();
    let second = entry(&serde_json::json!({
        "local_id": "support-desk:1",
        "agent_harness": "support-desk",
        "tokens": { "input": 250 },
    }))
    .unwrap();
    let a = submit(&pool, organization, person, vec![first], Door::Endpoint)
        .await
        .unwrap();
    let b = submit(&pool, organization, person, vec![second], Door::Endpoint)
        .await
        .unwrap();
    assert_eq!(a[0].id, b[0].id);
    assert_eq!(b[0].list_value, Some(Money::from_nanos(250, Currency::USD)));
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM token_entries")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_closed_period_refuses_changes_and_keeps_late_calls_aside(pool: PgPool) {
    let (organization, person) = organization(&pool).await.unwrap();
    let sent = entry(&serde_json::json!({ "session_id": "s4" })).unwrap();
    submit(
        &pool,
        organization,
        person,
        vec![sent.clone()],
        Door::TokenSheet,
    )
    .await
    .unwrap();
    sqlx::query("UPDATE token_entries SET status = 'locked'")
        .execute(&pool)
        .await
        .unwrap();

    let refused = submit(&pool, organization, person, vec![sent], Door::TokenSheet).await;
    assert!(matches!(refused, Err(StoreError::Locked { index: 0 })));

    let late = record(
        &pool,
        organization,
        vec![call("s4", "msg_9", datetime!(2026-10-05 09:00 UTC), 10)],
    )
    .await
    .unwrap();
    assert_eq!(late.late_messages, 1);
    assert_eq!(late.token_entries, 0);
    assert_eq!(entries_of(&pool, "s4").await.unwrap()[0].input_tokens, 100);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn splits_by_share_add_up_to_the_entry(pool: PgPool) {
    let (organization, person) = organization(&pool).await.unwrap();
    let sent = entry(&serde_json::json!({
        "tokens": { "input": 101 },
        "splits": [
            { "share_percent": 60, "tags": { "ticket": "KD-214" } },
            { "share_percent": 40, "tags": { "ticket": "KD-215" } },
        ],
    }))
    .unwrap();
    let stored = submit(&pool, organization, person, vec![sent], Door::Manual)
        .await
        .unwrap();
    assert_eq!(stored[0].status, "draft");

    let splits: Vec<(i64, Option<i64>)> = sqlx::query_as(
        "SELECT input_tokens, list_value_nanos FROM splits WHERE token_entry_id = $1 ORDER BY position",
    )
    .bind(stored[0].id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(splits, vec![(61, Some(61)), (40, Some(40))]);
    let tickets: Vec<String> = sqlx::query_scalar(
        "SELECT t.value FROM split_tags t JOIN splits s ON s.id = t.split_id
         WHERE s.token_entry_id = $1 AND t.name = 'ticket' ORDER BY s.position",
    )
    .bind(stored[0].id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(tickets, vec!["KD-214".to_owned(), "KD-215".to_owned()]);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_broken_entry_stores_nothing(pool: PgPool) {
    let (organization, person) = organization(&pool).await.unwrap();
    let good = entry(&serde_json::json!({ "session_id": "s5" })).unwrap();
    let broken = entry(&serde_json::json!({
        "session_id": "s6",
        "messages": [
            { "message_id": "msg_1", "at": "2026-10-05T09:00:00Z", "tokens": { "input": 1 } },
        ],
    }))
    .unwrap();
    let refused = submit(
        &pool,
        organization,
        person,
        vec![good, broken],
        Door::Endpoint,
    )
    .await;
    assert!(matches!(refused, Err(StoreError::Invalid { index: 1, .. })));
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM token_entries")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_core_places_a_ticket_in_the_branch_with_its_project_and_tags(pool: PgPool) {
    let (organization, person) = organization(&pool).await.unwrap();
    let sent = entry(&serde_json::json!({
        "signals": { "branch": "feature/KD-214-cart-total" },
        "tags": { "purpose": "feature" },
    }))
    .unwrap();
    let stored = submit(&pool, organization, person, vec![sent], Door::TokenSheet)
        .await
        .unwrap();
    assert_eq!(stored[0].dropped_tags, vec!["purpose".to_owned()]);
    assert_eq!(
        split_tags(&pool, stored[0].id).await.unwrap(),
        vec![vec![
            tag("client", "acme", "inherited"),
            tag("person", "mara@kestrel.example", "person"),
            tag("project", "storefront", "ticket"),
            tag("team", "web", "inherited"),
            tag("ticket", "KD-214", "rule"),
        ]]
    );
    let placed_by: String =
        sqlx::query_scalar("SELECT placed_by FROM splits WHERE token_entry_id = $1")
            .bind(stored[0].id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(placed_by, "rule: ticket key in branch");
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_organizations_and_the_persons_own_rules_place_token_entries(pool: PgPool) {
    let (organization, person) = organization(&pool).await.unwrap();
    sqlx::query("INSERT INTO tag_names (organization_id, name) VALUES ($1, 'purpose')")
        .bind(organization)
        .execute(&pool)
        .await
        .unwrap();
    // The organization switched every rule off; Mara has one of her own.
    sqlx::query("UPDATE organizations SET rules = '[]' WHERE id = $1")
        .bind(organization)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE people SET rules = $2 WHERE id = $1")
        .bind(person)
        .bind(serde_json::json!([{
            "name": "review branches",
            "when": [{ "signal": "branch", "test": { "starts_with": "review/" } }],
            "then": { "set_tags": { "purpose": "code-review" } }
        }]))
        .execute(&pool)
        .await
        .unwrap();
    let sent = entry(&serde_json::json!({
        "signals": { "branch": "review/KD-214-cart-total" },
    }))
    .unwrap();
    let stored = submit(&pool, organization, person, vec![sent], Door::TokenSheet)
        .await
        .unwrap();
    assert_eq!(
        split_tags(&pool, stored[0].id).await.unwrap(),
        vec![vec![
            tag("person", "mara@kestrel.example", "person"),
            tag("purpose", "code-review", "rule"),
            tag("team", "web", "inherited"),
        ]]
    );
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn telemetry_lands_on_the_project_of_its_repository(pool: PgPool) {
    let (organization, _) = organization(&pool).await.unwrap();
    let mut first = call("s7", "msg_1", datetime!(2026-10-05 09:00 UTC), 10);
    first.repository = Some("github.com/kestrel/storefront".to_owned());
    first
        .attributes
        .insert("team".to_owned(), "platform".to_owned());
    first
        .attributes
        .insert("os.type".to_owned(), "linux".to_owned());
    record(&pool, organization, vec![first]).await.unwrap();

    let entry: Uuid = sqlx::query_scalar("SELECT id FROM token_entries WHERE session_id = 's7'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        split_tags(&pool, entry).await.unwrap(),
        vec![vec![
            tag("client", "acme", "inherited"),
            tag("person", "mara@kestrel.example", "person"),
            tag("project", "storefront", "rule"),
            tag("team", "platform", "telemetry"),
        ]]
    );
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn segments_that_end_up_with_the_same_tags_are_one_split(pool: PgPool) {
    let (organization, person) = organization(&pool).await.unwrap();
    let sent = entry(&serde_json::json!({
        "session_id": "s8",
        "tokens": { "input": 60 },
        "messages": [
            { "message_id": "m1", "at": "2026-10-05T09:00:00Z", "tokens": { "input": 10 } },
            { "message_id": "m2", "at": "2026-10-05T09:10:00Z", "tokens": { "input": 20 } },
            { "message_id": "m3", "at": "2026-10-05T09:20:00Z", "tokens": { "input": 30 } },
        ],
        "splits": [
            { "message_ids": ["m1"], "signals": { "branch": "KD-214-cart" } },
            { "message_ids": ["m2"], "signals": { "branch": "main" } },
            { "message_ids": ["m3"], "signals": { "branch": "KD-214-cart" } },
        ],
    }))
    .unwrap();
    let stored = submit(&pool, organization, person, vec![sent], Door::TokenSheet)
        .await
        .unwrap();
    let splits: Vec<(i64, String)> = sqlx::query_as(
        "SELECT s.input_tokens, COALESCE(t.value, '') FROM splits s
         LEFT JOIN split_tags t ON t.split_id = s.id AND t.name = 'ticket'
         WHERE s.token_entry_id = $1 ORDER BY s.position",
    )
    .bind(stored[0].id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(splits, vec![(40, "KD-214".to_owned()), (20, String::new())]);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_new_price_table_prices_calls_whose_model_was_unknown(pool: PgPool) {
    use tokencontroller_server::integrations::prices::{self, ModelPrice};

    let (organization, person) = organization(&pool).await.unwrap();
    let sent = entry(
        &serde_json::json!({ "model": "claude-brand-new", "tokens": { "input": 2_000_000 } }),
    )
    .unwrap();
    let mut tx = pool.begin().await.unwrap();
    let stored = store::store_token_entries(
        &mut tx,
        organization,
        person,
        vec![sent],
        Door::Endpoint,
        &prices::current_pricing(&pool).await.unwrap(),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(stored[0].list_value, None);

    let price = Money::from_nanos(3_000_000_000, Currency::USD);
    let table = vec![ModelPrice {
        model: "claude-brand-new".to_owned(),
        provider: Some("anthropic".to_owned()),
        input: Some(price),
        output: Some(price),
        cache_write: Some(price),
        cache_read: Some(price),
    }];
    let saved = prices::save(&pool, &table, &[]).await.unwrap();
    assert!(matches!(
        saved,
        prices::Refreshed::NewVersion { models: 1, .. }
    ));
    assert!(matches!(
        prices::save(&pool, &table, &[]).await.unwrap(),
        prices::Refreshed::Unchanged { .. }
    ));

    let pricing = prices::current_pricing(&pool).await.unwrap();
    assert_eq!(store::reprice_unknown(&pool, &pricing).await.unwrap(), 1);
    let value: Option<i64> =
        sqlx::query_scalar("SELECT list_value_nanos FROM token_entries WHERE id = $1")
            .bind(stored[0].id)
            .fetch_one(&pool)
            .await
            .unwrap();
    // Two million tokens at 3 USD per million.
    assert_eq!(value, Some(6_000_000_000));
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_seed_has_token_entries_placed_on_tickets(pool: PgPool) {
    tokencontroller_server::seed::load(&pool).await.unwrap();
    let count = async || -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM token_entries")
            .fetch_one(&pool)
            .await
            .unwrap()
    };
    let first = count().await;
    assert!(first > 0);
    // Loading again puts the same token entries back.
    tokencontroller_server::seed::load(&pool).await.unwrap();
    assert_eq!(count().await, first);

    let on_tickets: Vec<(String, String)> = sqlx::query_as(
        "SELECT DISTINCT t.value, p.value FROM split_tags t
         JOIN split_tags p ON p.split_id = t.split_id AND p.name = 'project'
         WHERE t.name = 'ticket' ORDER BY t.value",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    for (ticket, project) in &on_tickets {
        let expected = match ticket.split_once('-').map(|(prefix, _)| prefix) {
            Some("KD") => "storefront",
            Some("CP") => "claims-portal",
            Some("INT") => "intranet",
            _ => "",
        };
        assert_eq!(project, expected, "{ticket}");
    }
    assert!(on_tickets.len() >= 3, "{on_tickets:?}");
    let unsynced: i64 = sqlx::query_scalar("SELECT count(*) FROM splits WHERE ticket_not_synced")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(unsynced, 0, "every seeded ticket exists");
    let without_cost: i64 =
        sqlx::query_scalar("SELECT count(*) FROM token_entries WHERE reported_cost_nanos IS NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(without_cost, 0);
}
