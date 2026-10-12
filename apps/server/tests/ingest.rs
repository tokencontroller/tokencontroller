//! Storing token entries and telemetry against a real Postgres database:
//! one row per model call, one token entry per session, whichever door a
//! call came through first (ADR 0006, ADR 0017).

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
        "INSERT INTO organizations (name) VALUES ('Kestrel Digital') RETURNING id",
    )
    .fetch_one(pool)
    .await?;
    let person: Uuid = sqlx::query_scalar(
        "INSERT INTO people (organization_id, email, name) VALUES ($1, 'mara@kestrel.example', 'Mara') RETURNING id",
    )
    .bind(organization)
    .fetch_one(pool)
    .await?;
    Ok((organization, person))
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
