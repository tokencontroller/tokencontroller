//! Token entries from the endpoint, token sheets, manual entry and CSV
//! import. A token entry for a session telemetry already reported takes
//! over that session's token entry (ADR 0014).

use sqlx::PgConnection;
use tokencontroller_core::{Currency, Money, TokenCounts};
use uuid::Uuid;

use super::rebuild::{OrgContext, org_context, rebuild};
use super::{Door, Pricing, StoreError, StoredEntry, to_i64};
use crate::ingest::entry::{CheckedMessage, CheckedTokenEntry, NewTokenEntry};
use crate::ingest::provider;

/// Checks and stores a batch of token entries for one agent manager, in
/// order. Sending a token entry again with the same session ID or
/// `local_id` updates it instead of adding a second one.
///
/// # Errors
///
/// [`StoreError::Invalid`] for the first token entry that breaks the
/// schema, before anything is written; [`StoreError::Locked`] when one is in
/// a closed period. Run it in a transaction to store all or nothing.
pub async fn store_token_entries(
    conn: &mut PgConnection,
    organization_id: Uuid,
    agent_manager_id: Uuid,
    entries: Vec<NewTokenEntry>,
    door: Door,
    pricing: &Pricing,
) -> Result<Vec<StoredEntry>, StoreError> {
    let checked = entries
        .into_iter()
        .enumerate()
        .map(|(index, entry)| {
            entry.check().map_err(|error| StoreError::Invalid {
                index,
                message: error.0,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let context = org_context(conn, organization_id).await?;
    let batch = Batch {
        organization_id,
        agent_manager_id,
        door,
        pricing,
        context: &context,
    };
    let mut stored = Vec::with_capacity(checked.len());
    for (index, entry) in checked.into_iter().enumerate() {
        stored.push(store_one(conn, &batch, entry, index).await?);
    }
    Ok(stored)
}

/// Stores one token entry that already passed [`NewTokenEntry::check`].
///
/// # Errors
///
/// As [`store_token_entries`].
pub async fn store_checked(
    conn: &mut PgConnection,
    organization_id: Uuid,
    agent_manager_id: Uuid,
    entry: CheckedTokenEntry,
    door: Door,
    pricing: &Pricing,
) -> Result<StoredEntry, StoreError> {
    let context = org_context(conn, organization_id).await?;
    let batch = Batch {
        organization_id,
        agent_manager_id,
        door,
        pricing,
        context: &context,
    };
    store_one(conn, &batch, entry, 0).await
}

/// The messages' own reported costs, or the token entry's reported cost
/// divided over them by tokens when none of them has one.
fn message_costs(
    messages: &[CheckedMessage],
    entry_cost: Option<Money>,
) -> Result<Vec<Option<Money>>, StoreError> {
    let own: Vec<Option<Money>> = messages
        .iter()
        .map(|message| message.reported_cost)
        .collect();
    let Some(entry_cost) = entry_cost else {
        return Ok(own);
    };
    if own.iter().any(Option::is_some) {
        return Ok(own);
    }
    let weights: Vec<u64> = messages
        .iter()
        .map(|message| message.tokens.total().unwrap_or_default())
        .collect();
    if weights.iter().all(|weight| *weight == 0) {
        return Ok(own);
    }
    Ok(entry_cost
        .allocate(&weights)?
        .into_iter()
        .map(Some)
        .collect())
}

/// Who and how a batch of token entries is stored for.
struct Batch<'a> {
    organization_id: Uuid,
    agent_manager_id: Uuid,
    door: Door,
    pricing: &'a Pricing,
    context: &'a OrgContext,
}

async fn store_one(
    conn: &mut PgConnection,
    batch: &Batch<'_>,
    checked: CheckedTokenEntry,
    index: usize,
) -> Result<StoredEntry, StoreError> {
    let Batch {
        organization_id,
        context,
        ..
    } = *batch;
    let CheckedTokenEntry {
        entry,
        reported_cost,
        messages,
    } = checked;

    let existing = sqlx::query!(
        r#"SELECT id, status FROM token_entries
           WHERE organization_id = $1
             AND (($2::text IS NOT NULL AND session_id = $2) OR ($3::text IS NOT NULL AND local_id = $3))
           ORDER BY (session_id IS NOT DISTINCT FROM $2) DESC
           LIMIT 1"#,
        organization_id,
        entry.session_id,
        entry.local_id
    )
    .fetch_optional(&mut *conn)
    .await?;
    if existing.as_ref().is_some_and(|row| row.status == "locked") {
        return Err(StoreError::Locked { index });
    }

    let entry_id = upsert_entry(conn, batch, &entry, existing.map(|row| row.id)).await?;
    write_messages(conn, batch, entry_id, &entry, reported_cost, &messages).await?;
    let rebuilt = rebuild(conn, organization_id, entry_id, context).await?;
    Ok(StoredEntry {
        local_id: entry.local_id,
        id: entry_id,
        status: rebuilt.status,
        list_value: rebuilt.list_value,
        currency: Currency::USD,
        cost_kind: rebuilt.cost_kind,
    })
}

/// Writes the token entry's own fields, as a new row or over the row of
/// the same session or sender ID.
async fn upsert_entry(
    conn: &mut PgConnection,
    batch: &Batch<'_>,
    entry: &NewTokenEntry,
    existing: Option<Uuid>,
) -> Result<Uuid, StoreError> {
    let Batch {
        organization_id,
        agent_manager_id,
        door,
        pricing,
        ..
    } = *batch;
    let provider = provider::infer(
        entry.provider.as_deref(),
        &entry.agent_harness,
        &entry.model,
    );
    let mut sent_tags = entry.tags.clone();
    if let Some(ticket) = &entry.ticket {
        sent_tags.insert("ticket".to_owned(), ticket.clone());
    }
    if let Some(project) = &entry.project {
        sent_tags.insert("project".to_owned(), project.clone());
    }
    let sent_tags = serde_json::to_value(&sent_tags).unwrap_or_default();
    let split_plan = serde_json::to_value(&entry.splits).unwrap_or_default();
    let signals = serde_json::to_value(&entry.signals).unwrap_or_default();

    Ok(sqlx::query_scalar!(
        r#"INSERT INTO token_entries (id, organization_id, agent_manager_id, agent_harness, agent_name,
               model, provider, effort, speed, started_at, ended_at, currency, price_table_version_id,
               summary, session_id, local_id, signals, sent_tags, split_plan, placed_by,
               status, on_token_sheet, door)
           VALUES (COALESCE($1, gen_random_uuid()), $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 'USD', $12,
               $13, $14, $15, $16, $17, $18, $19, $20, $21, $22)
           ON CONFLICT (id) DO UPDATE SET
               agent_manager_id = EXCLUDED.agent_manager_id,
               agent_harness = EXCLUDED.agent_harness,
               agent_name = EXCLUDED.agent_name,
               model = EXCLUDED.model,
               provider = EXCLUDED.provider,
               effort = COALESCE(EXCLUDED.effort, token_entries.effort),
               speed = COALESCE(EXCLUDED.speed, token_entries.speed),
               started_at = EXCLUDED.started_at,
               ended_at = EXCLUDED.ended_at,
               price_table_version_id = EXCLUDED.price_table_version_id,
               summary = EXCLUDED.summary,
               session_id = COALESCE(EXCLUDED.session_id, token_entries.session_id),
               local_id = COALESCE(EXCLUDED.local_id, token_entries.local_id),
               signals = EXCLUDED.signals,
               sent_tags = EXCLUDED.sent_tags,
               split_plan = EXCLUDED.split_plan,
               placed_by = EXCLUDED.placed_by,
               status = EXCLUDED.status,
               rejected_reason = NULL,
               on_token_sheet = EXCLUDED.on_token_sheet,
               door = EXCLUDED.door,
               updated_at = now()
           RETURNING id"#,
        existing,
        organization_id,
        agent_manager_id,
        entry.agent_harness,
        entry.agent_name,
        entry.model,
        provider,
        entry.effort,
        entry.speed,
        entry.started_at,
        entry.ended_at,
        pricing.version_id,
        entry.summary,
        entry.session_id,
        entry.local_id,
        signals,
        sent_tags,
        split_plan,
        entry.placed_by,
        door.status(),
        door.on_token_sheet(),
        door.as_str()
    )
    .fetch_one(&mut *conn)
    .await?)
}

/// Writes the calls a token entry sent, or a stand-in when it sent none and
/// telemetry did not report the session, and claims the session's calls.
async fn write_messages(
    conn: &mut PgConnection,
    batch: &Batch<'_>,
    entry_id: Uuid,
    entry: &NewTokenEntry,
    reported_cost: Option<Money>,
    messages: &[CheckedMessage],
) -> Result<(), StoreError> {
    let organization_id = batch.organization_id;
    let session = entry
        .session_id
        .clone()
        .unwrap_or_else(|| format!("entry:{entry_id}"));
    let sent = Sent {
        entry_id,
        entry,
        reported_cost,
        session: &session,
    };
    if messages.is_empty() {
        write_stand_in(conn, batch, &sent).await?;
    } else {
        write_sent(conn, batch, &sent, messages).await?;
    }
    sqlx::query!(
        "UPDATE messages SET token_entry_id = $3
         WHERE organization_id = $1 AND session_id = $2",
        organization_id,
        session,
        entry_id
    )
    .execute(&mut *conn)
    .await?;

    Ok(())
}

/// One token entry being written, with the session its calls belong to.
struct Sent<'a> {
    entry_id: Uuid,
    entry: &'a NewTokenEntry,
    reported_cost: Option<Money>,
    session: &'a str,
}

/// Without messages, telemetry's calls of the session count; only a session
/// nobody else reported gets a stand-in message.
async fn write_stand_in(
    conn: &mut PgConnection,
    batch: &Batch<'_>,
    sent: &Sent<'_>,
) -> Result<(), StoreError> {
    let Batch {
        organization_id,
        pricing,
        ..
    } = *batch;
    let Sent {
        entry_id,
        entry,
        reported_cost,
        session,
    } = *sent;
    let telemetry_seen = sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM messages
              WHERE organization_id = $1 AND session_id = $2 AND from_telemetry) AS "seen!""#,
        organization_id,
        session
    )
    .fetch_one(&mut *conn)
    .await?;
    if telemetry_seen {
        sqlx::query!(
            "DELETE FROM messages WHERE organization_id = $1 AND session_id = $2 AND kind = 'stand_in'",
            organization_id,
            session
        )
        .execute(&mut *conn)
        .await?;
        return Ok(());
    }
    let stand_in_id = format!("entry:{entry_id}");
    let value = pricing.list_value(&entry.model, entry.tokens, reported_cost);
    upsert_entry_message(
        conn,
        organization_id,
        session,
        entry_id,
        &entry.agent_harness,
        &EntryMessage {
            message_id: &stand_in_id,
            model: &entry.model,
            occurred_at: entry.started_at,
            request_id: None,
            tokens: entry.tokens,
            reported_cost,
            list_value: value,
            kind: "stand_in",
            split_position: None,
        },
    )
    .await
}

/// Writes the calls the token entry sent; telemetry events that could not
/// be matched to them stop counting.
async fn write_sent(
    conn: &mut PgConnection,
    batch: &Batch<'_>,
    sent: &Sent<'_>,
    messages: &[CheckedMessage],
) -> Result<(), StoreError> {
    let Batch {
        organization_id,
        pricing,
        ..
    } = *batch;
    let Sent {
        entry_id,
        entry,
        reported_cost,
        session,
    } = *sent;
    sqlx::query!(
        "DELETE FROM messages WHERE organization_id = $1 AND session_id = $2 AND kind = 'stand_in'",
        organization_id,
        session
    )
    .execute(&mut *conn)
    .await?;
    let positions: std::collections::BTreeMap<&str, i32> = entry
        .splits
        .iter()
        .enumerate()
        .flat_map(|(position, split)| {
            let position = i32::try_from(position).unwrap_or(i32::MAX);
            split
                .message_ids
                .iter()
                .map(move |id| (id.as_str(), position))
        })
        .collect();
    let costs = message_costs(messages, reported_cost)?;
    let ids: Vec<String> = messages
        .iter()
        .map(|message| message.message_id.clone())
        .collect();
    for (message, cost) in messages.iter().zip(costs) {
        let value = pricing.list_value(&message.model, message.tokens, cost);
        upsert_entry_message(
            conn,
            organization_id,
            session,
            entry_id,
            &entry.agent_harness,
            &EntryMessage {
                message_id: &message.message_id,
                model: &message.model,
                occurred_at: message.occurred_at,
                request_id: message.request_id.as_deref(),
                tokens: message.tokens,
                reported_cost: cost,
                list_value: value,
                kind: "real",
                split_position: positions.get(message.message_id.as_str()).copied(),
            },
        )
        .await?;
    }
    // Calls an earlier version of this token entry sent and this one does
    // not are gone, unless telemetry saw them too.
    sqlx::query!(
        "DELETE FROM messages
         WHERE organization_id = $1 AND session_id = $2 AND from_entry AND NOT from_telemetry
           AND message_id <> ALL($3)",
        organization_id,
        session,
        &ids
    )
    .execute(&mut *conn)
    .await?;
    // Telemetry events without IDs of their own cannot be matched to these
    // calls; the token entry's messages count instead.
    sqlx::query!(
        "UPDATE messages SET counts = false
         WHERE organization_id = $1 AND session_id = $2 AND kind = 'built'",
        organization_id,
        session
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

struct EntryMessage<'a> {
    message_id: &'a str,
    model: &'a str,
    occurred_at: time::OffsetDateTime,
    request_id: Option<&'a str>,
    tokens: TokenCounts,
    reported_cost: Option<Money>,
    list_value: Option<Money>,
    kind: &'static str,
    split_position: Option<i32>,
}

/// Writes one message a token entry sent. A call telemetry already
/// reported keeps its row: the token entry only claims it and fills in
/// what telemetry did not know.
async fn upsert_entry_message(
    conn: &mut PgConnection,
    organization_id: Uuid,
    session: &str,
    entry_id: Uuid,
    agent_harness: &str,
    message: &EntryMessage<'_>,
) -> Result<(), StoreError> {
    sqlx::query!(
        "INSERT INTO messages (organization_id, session_id, message_id, token_entry_id, agent_harness,
             model, occurred_at, input_tokens, output_tokens, cache_write_tokens, cache_read_tokens,
             reported_cost_nanos, list_value_nanos, request_id, kind, from_entry, counts, split_position)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, true, true, $16)
         ON CONFLICT (organization_id, session_id, message_id) DO UPDATE SET
             token_entry_id = EXCLUDED.token_entry_id,
             from_entry = true,
             counts = true,
             request_id = COALESCE(messages.request_id, EXCLUDED.request_id),
             reported_cost_nanos = CASE WHEN messages.from_telemetry
                 THEN COALESCE(messages.reported_cost_nanos, EXCLUDED.reported_cost_nanos)
                 ELSE EXCLUDED.reported_cost_nanos END,
             list_value_nanos = CASE WHEN messages.from_telemetry
                 THEN COALESCE(messages.list_value_nanos, EXCLUDED.list_value_nanos)
                 ELSE EXCLUDED.list_value_nanos END,
             model = CASE WHEN messages.from_telemetry THEN messages.model ELSE EXCLUDED.model END,
             occurred_at = CASE WHEN messages.from_telemetry THEN messages.occurred_at ELSE EXCLUDED.occurred_at END,
             split_position = EXCLUDED.split_position,
             input_tokens = CASE WHEN messages.from_telemetry THEN messages.input_tokens ELSE EXCLUDED.input_tokens END,
             output_tokens = CASE WHEN messages.from_telemetry THEN messages.output_tokens ELSE EXCLUDED.output_tokens END,
             cache_write_tokens = CASE WHEN messages.from_telemetry THEN messages.cache_write_tokens ELSE EXCLUDED.cache_write_tokens END,
             cache_read_tokens = CASE WHEN messages.from_telemetry THEN messages.cache_read_tokens ELSE EXCLUDED.cache_read_tokens END",
        organization_id,
        session,
        message.message_id,
        entry_id,
        agent_harness,
        message.model,
        message.occurred_at,
        to_i64(message.tokens.input)?,
        to_i64(message.tokens.output)?,
        to_i64(message.tokens.cache_write)?,
        to_i64(message.tokens.cache_read)?,
        message.reported_cost.map(Money::nanos),
        message.list_value.map(Money::nanos),
        message.request_id,
        message.kind,
        message.split_position
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(id: &str, input: u64, cost: Option<i64>) -> CheckedMessage {
        CheckedMessage {
            message_id: id.to_owned(),
            model: "claude-sonnet-5".to_owned(),
            occurred_at: time::OffsetDateTime::UNIX_EPOCH,
            request_id: None,
            tokens: TokenCounts {
                input,
                ..TokenCounts::default()
            },
            reported_cost: cost.map(|nanos| Money::from_nanos(nanos, Currency::USD)),
        }
    }

    #[test]
    fn an_entry_cost_is_divided_over_messages_without_their_own() {
        let messages = [message("a", 1, None), message("b", 3, None)];
        let costs = message_costs(&messages, Some(Money::from_nanos(100, Currency::USD))).unwrap();
        assert_eq!(
            costs,
            vec![
                Some(Money::from_nanos(25, Currency::USD)),
                Some(Money::from_nanos(75, Currency::USD)),
            ]
        );
    }

    #[test]
    fn messages_with_their_own_costs_keep_them() {
        let messages = [message("a", 1, Some(7)), message("b", 3, None)];
        let costs = message_costs(&messages, Some(Money::from_nanos(100, Currency::USD))).unwrap();
        assert_eq!(costs, vec![Some(Money::from_nanos(7, Currency::USD)), None]);
        assert_eq!(
            message_costs(&messages, None).unwrap(),
            vec![Some(Money::from_nanos(7, Currency::USD)), None]
        );
    }
}
