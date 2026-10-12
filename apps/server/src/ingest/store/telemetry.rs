//! Telemetry: one message per model call. A session nobody has claimed gets
//! an automatic token entry built from its calls; a claimed session only
//! gains the calls its token entry did not know (ADR 0017).

use std::collections::BTreeMap;

use sqlx::PgConnection;
use tokencontroller_core::Money;
use uuid::Uuid;

use super::rebuild::{org_context, rebuild};
use super::{Pricing, StoreError, to_i64};
use crate::ingest::model::TelemetryMessage;
use crate::ingest::provider;

/// Attribute names telemetry uses for the effort and speed settings.
const EFFORT_ATTRIBUTES: [&str; 3] = [
    "effort",
    "reasoning_effort",
    "gen_ai.request.reasoning_effort",
];
const SPEED_ATTRIBUTES: [&str; 2] = ["speed", "service_tier"];

/// What one telemetry batch did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TelemetryOutcome {
    /// Calls seen for the first time.
    pub new_messages: u64,
    /// Calls already known, from an earlier export or another door.
    pub known_messages: u64,
    /// Calls for a session whose period is closed. They are kept without a
    /// token entry, for an adjustment.
    pub late_messages: u64,
    /// Token entries created or updated.
    pub token_entries: u64,
}

fn first_attribute(message: &TelemetryMessage, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| message.attributes.get(*name))
        .filter(|value| !value.trim().is_empty())
        .cloned()
}

/// Stores a batch of telemetry messages for one organization and builds or
/// updates the token entries of their sessions.
///
/// # Errors
///
/// Fails if a query fails. Run it in a transaction to store all or nothing.
pub async fn record_telemetry(
    conn: &mut PgConnection,
    organization_id: Uuid,
    messages: Vec<TelemetryMessage>,
    pricing: &Pricing,
) -> Result<TelemetryOutcome, StoreError> {
    let mut outcome = TelemetryOutcome::default();
    let mut sessions: BTreeMap<String, Vec<TelemetryMessage>> = BTreeMap::new();
    for message in messages {
        sessions
            .entry(message.session_id.clone())
            .or_default()
            .push(message);
    }
    if sessions.is_empty() {
        return Ok(outcome);
    }
    let context = org_context(conn, organization_id).await?;

    for (session, calls) in sessions {
        insert_calls(
            conn,
            organization_id,
            &session,
            &calls,
            pricing,
            &mut outcome,
        )
        .await?;

        let Some(latest) = calls.iter().max_by_key(|call| call.occurred_at) else {
            continue;
        };
        let existing = sqlx::query!(
            "SELECT id, status, door FROM token_entries WHERE organization_id = $1 AND session_id = $2",
            organization_id,
            session
        )
        .fetch_optional(&mut *conn)
        .await?;
        let entry_id = match existing {
            Some(entry) if entry.status == "locked" => {
                let late = u64::try_from(calls.len()).unwrap_or(u64::MAX);
                outcome.late_messages = outcome.late_messages.saturating_add(late);
                continue;
            }
            Some(entry) => {
                // A token entry that sent its own calls outranks telemetry
                // events that could not be matched to them, and real calls
                // outrank a stand-in.
                sqlx::query!(
                    "UPDATE messages SET counts = false
                     WHERE organization_id = $1 AND session_id = $2
                       AND (kind = 'stand_in'
                            OR (kind = 'built' AND EXISTS (
                                SELECT 1 FROM messages m
                                WHERE m.organization_id = $1 AND m.session_id = $2
                                  AND m.kind = 'real' AND m.from_entry)))",
                    organization_id,
                    session
                )
                .execute(&mut *conn)
                .await?;
                if entry.door == "telemetry" {
                    refresh_automatic(conn, entry.id, latest, &calls).await?;
                }
                entry.id
            }
            None => create_automatic(conn, organization_id, &session, latest, &calls).await?,
        };
        sqlx::query!(
            "UPDATE messages SET token_entry_id = $3 WHERE organization_id = $1 AND session_id = $2",
            organization_id,
            session,
            entry_id
        )
        .execute(&mut *conn)
        .await?;
        rebuild(conn, organization_id, entry_id, &context).await?;
        outcome.token_entries = outcome.token_entries.saturating_add(1);
    }
    Ok(outcome)
}

/// Stores each call of a session as a message, or marks a known one as seen
/// by telemetry.
async fn insert_calls(
    conn: &mut PgConnection,
    organization_id: Uuid,
    session: &str,
    calls: &[TelemetryMessage],
    pricing: &Pricing,
    outcome: &mut TelemetryOutcome,
) -> Result<(), StoreError> {
    for call in calls {
        let kind = if call.message_id.starts_with("otel:") {
            "built"
        } else {
            "real"
        };
        let value = pricing.list_value(&call.model, call.tokens, call.reported_cost);
        let inserted = sqlx::query_scalar!(
            r#"INSERT INTO messages (organization_id, session_id, message_id, agent_harness, model,
                   occurred_at, input_tokens, output_tokens, cache_write_tokens, cache_read_tokens,
                   reported_cost_nanos, list_value_nanos, kind, from_telemetry, counts)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, true, true)
               ON CONFLICT (organization_id, session_id, message_id) DO UPDATE SET
                   from_telemetry = true,
                   reported_cost_nanos = COALESCE(messages.reported_cost_nanos, EXCLUDED.reported_cost_nanos)
               RETURNING (xmax = 0) AS "inserted!""#,
            organization_id,
            session,
            call.message_id,
            call.harness.name(),
            call.model,
            call.occurred_at,
            to_i64(call.tokens.input)?,
            to_i64(call.tokens.output)?,
            to_i64(call.tokens.cache_write)?,
            to_i64(call.tokens.cache_read)?,
            call.reported_cost.map(Money::nanos),
            value.map(Money::nanos),
            kind
        )
        .fetch_one(&mut *conn)
        .await?;
        if inserted {
            outcome.new_messages = outcome.new_messages.saturating_add(1);
        } else {
            outcome.known_messages = outcome.known_messages.saturating_add(1);
        }
    }
    Ok(())
}

/// The member a telemetry event names, by email address.
async fn person_for(
    conn: &mut PgConnection,
    organization_id: Uuid,
    calls: &[TelemetryMessage],
) -> Result<Option<Uuid>, StoreError> {
    let Some(email) = calls
        .iter()
        .rev()
        .find_map(|call| call.person_email.as_deref())
    else {
        return Ok(None);
    };
    Ok(sqlx::query_scalar!(
        "SELECT id FROM people WHERE organization_id = $1 AND lower(email) = lower($2)
         ORDER BY removed_at IS NULL DESC LIMIT 1",
        organization_id,
        email
    )
    .fetch_optional(&mut *conn)
    .await?)
}

fn attributes(calls: &[TelemetryMessage]) -> serde_json::Value {
    let merged: BTreeMap<&String, &String> = calls
        .iter()
        .flat_map(|call| call.attributes.iter())
        .collect();
    serde_json::to_value(merged).unwrap_or_default()
}

fn signals(calls: &[TelemetryMessage]) -> serde_json::Value {
    calls
        .iter()
        .rev()
        .find_map(|call| call.repository.as_ref())
        .map_or_else(
            || serde_json::json!({}),
            |repository| serde_json::json!({ "repository": repository }),
        )
}

/// Builds the automatic token entry of a session nobody has claimed.
async fn create_automatic(
    conn: &mut PgConnection,
    organization_id: Uuid,
    session: &str,
    latest: &TelemetryMessage,
    calls: &[TelemetryMessage],
) -> Result<Uuid, StoreError> {
    let person = person_for(conn, organization_id, calls).await?;
    let started = calls
        .iter()
        .map(|call| call.occurred_at)
        .min()
        .unwrap_or(latest.occurred_at);
    let harness = latest.harness.name();
    Ok(sqlx::query_scalar!(
        "INSERT INTO token_entries (organization_id, agent_manager_id, agent_harness, model, provider,
             effort, speed, started_at, ended_at, session_id, signals, telemetry_attributes,
             status, on_token_sheet, door)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, 'submitted', false, 'telemetry')
         RETURNING id",
        organization_id,
        person,
        harness,
        latest.model,
        provider::infer(None, harness, &latest.model),
        first_attribute(latest, &EFFORT_ATTRIBUTES),
        first_attribute(latest, &SPEED_ATTRIBUTES),
        started,
        latest.occurred_at,
        session,
        signals(calls),
        attributes(calls)
    )
    .fetch_one(&mut *conn)
    .await?)
}

/// Brings an automatic token entry up to date with more of its calls.
async fn refresh_automatic(
    conn: &mut PgConnection,
    entry_id: Uuid,
    latest: &TelemetryMessage,
    calls: &[TelemetryMessage],
) -> Result<(), StoreError> {
    let organization_id = sqlx::query_scalar!(
        "SELECT organization_id FROM token_entries WHERE id = $1",
        entry_id
    )
    .fetch_one(&mut *conn)
    .await?;
    let person = person_for(conn, organization_id, calls).await?;
    sqlx::query!(
        "UPDATE token_entries SET
             agent_manager_id = COALESCE(agent_manager_id, $2),
             model = $3,
             effort = COALESCE($4, effort),
             speed = COALESCE($5, speed),
             signals = signals || $6,
             telemetry_attributes = telemetry_attributes || $7
         WHERE id = $1",
        entry_id,
        person,
        latest.model,
        first_attribute(latest, &EFFORT_ATTRIBUTES),
        first_attribute(latest, &SPEED_ATTRIBUTES),
        signals(calls),
        attributes(calls)
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}
