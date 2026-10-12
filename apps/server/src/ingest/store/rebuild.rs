//! Rebuilding a token entry from its messages: the totals, then every split
//! with its values and its tags copied in (ADR 0008). Runs whenever a door
//! adds to a token entry, so all doors share one way of adding up. The core
//! places each split, as `tc` does on the machine (ADR 0010).

use std::collections::{BTreeMap, BTreeSet};

use sqlx::PgConnection;
use time::OffsetDateTime;
use tokencontroller_core::{
    CostKind, Currency, Money, PlacedBy, PlacementRequest, Signals, TagName, TagValue, Tags,
    TokenCounts,
};
use uuid::Uuid;

use super::place::{OrgContext, candidate_keys, signals, tags_from, with_tickets};
use super::{StoreError, to_i64, to_u64};
use crate::ingest::entry::{self, NewSplit};

/// One split's share of a token entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Values {
    tokens: TokenCounts,
    list_value: Option<Money>,
}

#[derive(Debug, Clone)]
struct CountingMessage {
    occurred_at: OffsetDateTime,
    tokens: TokenCounts,
    list_value_nanos: Option<i64>,
    split_position: Option<i32>,
}

fn add(
    values: &mut Values,
    message: &CountingMessage,
    currency: Currency,
) -> Result<(), StoreError> {
    values.tokens = values
        .tokens
        .checked_add(message.tokens)
        .ok_or(StoreError::Overflow)?;
    values.list_value = match (values.list_value, message.list_value_nanos) {
        (Some(sum), Some(nanos)) => Some(sum.checked_add(Money::from_nanos(nanos, currency))?),
        _ => None,
    };
    Ok(())
}

/// The result of a rebuild.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Rebuilt {
    pub status: String,
    pub list_value: Option<Money>,
    pub currency: Currency,
    pub cost_kind: CostKind,
    /// Tag names that were sent but are not on the organization's list, so
    /// they were left out.
    pub dropped_tags: Vec<String>,
}

/// The token entry fields a rebuild reads.
struct EntryRow {
    agent_manager_id: Option<Uuid>,
    currency: Currency,
    status: String,
    from_telemetry: bool,
    plan: Vec<NewSplit>,
    sent: BTreeMap<String, String>,
    telemetry: BTreeMap<String, String>,
    signals: entry::Signals,
    summary: Option<String>,
}

async fn entry_row(
    conn: &mut PgConnection,
    organization_id: Uuid,
    entry_id: Uuid,
) -> Result<EntryRow, StoreError> {
    let row = sqlx::query!(
        r#"SELECT agent_manager_id, currency, door, status, sent_tags, split_plan,
                  telemetry_attributes, signals, summary
           FROM token_entries WHERE id = $1 AND organization_id = $2"#,
        entry_id,
        organization_id
    )
    .fetch_one(&mut *conn)
    .await?;
    Ok(EntryRow {
        agent_manager_id: row.agent_manager_id,
        currency: Currency::new(&row.currency).unwrap_or(Currency::USD),
        status: row.status,
        from_telemetry: row.door == "telemetry",
        plan: serde_json::from_value(row.split_plan).unwrap_or_default(),
        sent: serde_json::from_value(row.sent_tags).unwrap_or_default(),
        telemetry: serde_json::from_value(row.telemetry_attributes).unwrap_or_default(),
        signals: serde_json::from_value(row.signals).unwrap_or_default(),
        summary: row.summary,
    })
}

/// Adds up the counting messages and writes the token entry's totals. An
/// automatic token entry also takes its start and end from its calls.
async fn write_totals(
    conn: &mut PgConnection,
    entry_id: Uuid,
    entry: &EntryRow,
) -> Result<(Vec<CountingMessage>, Values), StoreError> {
    let rows = sqlx::query!(
        r#"SELECT occurred_at, input_tokens, output_tokens, cache_write_tokens, cache_read_tokens,
                  reported_cost_nanos, list_value_nanos, split_position
           FROM messages WHERE token_entry_id = $1 AND counts
           ORDER BY occurred_at, message_id"#,
        entry_id
    )
    .fetch_all(&mut *conn)
    .await?;
    let messages: Vec<CountingMessage> = rows
        .iter()
        .map(|row| CountingMessage {
            occurred_at: row.occurred_at,
            tokens: TokenCounts {
                input: to_u64(row.input_tokens),
                output: to_u64(row.output_tokens),
                cache_write: to_u64(row.cache_write_tokens),
                cache_read: to_u64(row.cache_read_tokens),
            },
            list_value_nanos: row.list_value_nanos,
            split_position: row.split_position,
        })
        .collect();
    let mut total = Values {
        tokens: TokenCounts::default(),
        list_value: Some(Money::zero(entry.currency)),
    };
    for message in &messages {
        add(&mut total, message, entry.currency)?;
    }
    let reported = rows
        .iter()
        .map(|row| row.reported_cost_nanos)
        .try_fold(0_i64, |sum, nanos| {
            nanos.and_then(|nanos| sum.checked_add(nanos))
        })
        .filter(|_| !rows.is_empty());
    sqlx::query!(
        r#"UPDATE token_entries SET
               input_tokens = $2, output_tokens = $3, cache_write_tokens = $4, cache_read_tokens = $5,
               reported_cost_nanos = $6, list_value_nanos = $7,
               started_at = CASE WHEN $8 AND $9::timestamptz IS NOT NULL THEN $9 ELSE started_at END,
               ended_at = CASE WHEN $8 AND $10::timestamptz IS NOT NULL THEN $10 ELSE ended_at END,
               updated_at = now()
           WHERE id = $1"#,
        entry_id,
        to_i64(total.tokens.input)?,
        to_i64(total.tokens.output)?,
        to_i64(total.tokens.cache_write)?,
        to_i64(total.tokens.cache_read)?,
        reported,
        total.list_value.map(Money::nanos),
        entry.from_telemetry,
        messages.first().map(|message| message.occurred_at),
        messages.last().map(|message| message.occurred_at)
    )
    .execute(&mut *conn)
    .await?;
    Ok((messages, total))
}

/// One part of a token entry before placing: the whole of it, a segment
/// by messages, or a share by hand.
struct Piece {
    method: &'static str,
    share_percent: Option<u32>,
    /// Tags named by hand or sent, the split's own over the token entry's.
    named: Tags,
    signals: Signals,
    /// The split's own signals as sent, kept on the split.
    sent_signals: serde_json::Value,
    values: Values,
}

fn pieces(entry: &EntryRow, values: Vec<Values>) -> Vec<Piece> {
    let sent = tags_from(&entry.sent);
    if entry.plan.is_empty() {
        return values
            .into_iter()
            .map(|values| Piece {
                method: "whole",
                share_percent: None,
                named: sent.clone(),
                signals: signals(&entry.signals, None),
                sent_signals: serde_json::json!({}),
                values,
            })
            .collect();
    }
    entry
        .plan
        .iter()
        .zip(values)
        .map(|(split, values)| Piece {
            method: if split.message_ids.is_empty() {
                "share"
            } else {
                "by_message"
            },
            share_percent: split.share_percent,
            named: tags_from(&split.tags).over(&sent),
            signals: signals(&entry.signals, Some(&split.signals)),
            sent_signals: serde_json::to_value(&split.signals).unwrap_or_default(),
            values,
        })
        .collect()
}

/// One split after placing.
struct PlacedSplit {
    method: &'static str,
    share_percent: Option<u32>,
    values: Values,
    tags: Tags,
    /// Where each tag came from, by name.
    sources: BTreeMap<TagName, &'static str>,
    placed_by: PlacedBy,
    ticket_not_synced: bool,
    signals: serde_json::Value,
}

/// Where a placed tag came from, as `split_tags.source` names it.
fn source(
    (name, value): (&TagName, &TagValue),
    placed: &Tags,
    named: &Tags,
    configured: &Tags,
    placed_by: &PlacedBy,
) -> &'static str {
    if *name == TagName::person() {
        "person"
    } else if named.get(name) == Some(value) {
        "sent"
    } else if configured.get(name) == Some(value) {
        "telemetry"
    } else if *name == TagName::project() && placed.contains(&TagName::ticket()) {
        // A placed ticket brings its project.
        "ticket"
    } else if *name == TagName::ticket() || *name == TagName::project() {
        match placed_by {
            PlacedBy::Rule(_) => "rule",
            PlacedBy::SummaryMatch => "summary",
            PlacedBy::Configuration => "telemetry",
            PlacedBy::Named | PlacedBy::Nothing => "ticket",
        }
    } else {
        "inherited"
    }
}

/// Divides values in proportion to `weights` by the largest remainder, so
/// the parts always add up.
fn divide(values: &Values, weights: &[u64], currency: Currency) -> Result<Vec<Values>, StoreError> {
    let by_kind = |count: u64| -> Result<Vec<u64>, StoreError> {
        Money::from_nanos(to_i64(count)?, currency)
            .allocate(weights)?
            .into_iter()
            .map(|part| u64::try_from(part.nanos()).map_err(|_| StoreError::Overflow))
            .collect()
    };
    let input = by_kind(values.tokens.input)?;
    let output = by_kind(values.tokens.output)?;
    let cache_write = by_kind(values.tokens.cache_write)?;
    let cache_read = by_kind(values.tokens.cache_read)?;
    let list_values: Vec<Option<Money>> = match values.list_value {
        Some(value) => value.allocate(weights)?.into_iter().map(Some).collect(),
        None => vec![None; weights.len()],
    };
    Ok((0..weights.len())
        .map(|i| Values {
            tokens: TokenCounts {
                input: input.get(i).copied().unwrap_or_default(),
                output: output.get(i).copied().unwrap_or_default(),
                cache_write: cache_write.get(i).copied().unwrap_or_default(),
                cache_read: cache_read.get(i).copied().unwrap_or_default(),
            },
            list_value: list_values.get(i).copied().flatten(),
        })
        .collect())
}

fn add_values(sum: &mut Values, more: &Values) -> Result<(), StoreError> {
    sum.tokens = sum
        .tokens
        .checked_add(more.tokens)
        .ok_or(StoreError::Overflow)?;
    sum.list_value = match (sum.list_value, more.list_value) {
        (Some(left), Some(right)) => Some(left.checked_add(right)?),
        _ => None,
    };
    Ok(())
}

/// Places every piece with the core and joins pieces that end up with the
/// same tags into one split.
async fn place_pieces(
    conn: &mut PgConnection,
    organization_id: Uuid,
    entry: &EntryRow,
    pieces: Vec<Piece>,
    context: &OrgContext,
) -> Result<(Vec<PlacedSplit>, Vec<String>), StoreError> {
    let person = entry
        .agent_manager_id
        .and_then(|id| context.people.get(&id));
    let mut keys = BTreeSet::new();
    for piece in &pieces {
        keys.extend(candidate_keys(
            &context.organization,
            &piece.named,
            &piece.signals,
        ));
    }
    let organization = with_tickets(conn, organization_id, context, &keys).await?;
    let configured = tags_from(&entry.telemetry).retain(|name| organization.allows(name));

    let mut placed: Vec<PlacedSplit> = Vec::new();
    let mut dropped: BTreeSet<String> = BTreeSet::new();
    for piece in pieces {
        let placement = organization.place(
            &PlacementRequest {
                person,
                named: &piece.named,
                configured: &configured,
                signals: &piece.signals,
                summary: entry.summary.as_deref(),
            },
            None,
        );
        dropped.extend(
            placement
                .dropped
                .iter()
                .map(|name| name.as_str().to_owned()),
        );
        let weights: Vec<u64> = placement.shares.iter().map(|share| share.weight).collect();
        let parts = divide(&piece.values, &weights, entry.currency)?;
        for (share, values) in placement.shares.into_iter().zip(parts) {
            if let Some(split) = placed.iter_mut().find(|split| split.tags == share.tags) {
                add_values(&mut split.values, &values)?;
                split.share_percent = match (split.share_percent, piece.share_percent) {
                    (Some(left), Some(right)) => left.checked_add(right),
                    _ => None,
                };
                continue;
            }
            let sources = share
                .tags
                .iter()
                .map(|(name, value)| {
                    (
                        name.clone(),
                        source(
                            (name, value),
                            &share.tags,
                            &piece.named,
                            &configured,
                            &share.placed_by,
                        ),
                    )
                })
                .collect();
            placed.push(PlacedSplit {
                method: piece.method,
                share_percent: piece.share_percent,
                values,
                tags: share.tags,
                sources,
                placed_by: share.placed_by,
                ticket_not_synced: share.ticket_not_synced,
                signals: piece.sent_signals.clone(),
            });
        }
    }
    Ok((placed, dropped.into_iter().collect()))
}

async fn insert_split(
    conn: &mut PgConnection,
    organization_id: Uuid,
    entry_id: Uuid,
    position: usize,
    split: &PlacedSplit,
) -> Result<(), StoreError> {
    let position = i32::try_from(position).map_err(|_| StoreError::Overflow)?;
    let share_percent = split
        .share_percent
        .map(i32::try_from)
        .transpose()
        .map_err(|_| StoreError::Overflow)?;
    let split_id = sqlx::query_scalar!(
        r#"INSERT INTO splits (token_entry_id, position, method, share_percent,
               input_tokens, output_tokens, cache_write_tokens, cache_read_tokens,
               list_value_nanos, signals, placed_by, ticket_not_synced)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12) RETURNING id"#,
        entry_id,
        position,
        split.method,
        share_percent,
        to_i64(split.values.tokens.input)?,
        to_i64(split.values.tokens.output)?,
        to_i64(split.values.tokens.cache_write)?,
        to_i64(split.values.tokens.cache_read)?,
        split.values.list_value.map(Money::nanos),
        split.signals,
        split.placed_by.to_string(),
        split.ticket_not_synced
    )
    .fetch_one(&mut *conn)
    .await?;
    for (name, value) in split.tags.iter() {
        let source = split.sources.get(name).copied().unwrap_or("inherited");
        sqlx::query!(
            "INSERT INTO split_tags (split_id, organization_id, name, value, source)
             VALUES ($1, $2, $3, $4, $5)",
            split_id,
            organization_id,
            name.as_str(),
            value.as_str(),
            source
        )
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// Adds up the token entry's counting messages, writes the totals, and
/// builds its splits and their tags again.
pub(super) async fn rebuild(
    conn: &mut PgConnection,
    organization_id: Uuid,
    entry_id: Uuid,
    context: &OrgContext,
) -> Result<Rebuilt, StoreError> {
    let entry = entry_row(conn, organization_id, entry_id).await?;
    let (messages, total) = write_totals(conn, entry_id, &entry).await?;
    let values = split_values(&entry.plan, &messages, &total, entry.currency)?;
    let pieces = pieces(&entry, values);
    let (placed, dropped_tags) =
        place_pieces(conn, organization_id, &entry, pieces, context).await?;

    sqlx::query!("DELETE FROM splits WHERE token_entry_id = $1", entry_id)
        .execute(&mut *conn)
        .await?;
    let mut largest: Option<(u64, CostKind)> = None;
    for (position, split) in placed.iter().enumerate() {
        let kind = context.organization.tag_map.cost_kind(&split.tags);
        let weight = split.values.tokens.total().unwrap_or_default();
        if largest.is_none_or(|(heaviest, _)| weight > heaviest) {
            largest = Some((weight, kind));
        }
        insert_split(conn, organization_id, entry_id, position, split).await?;
    }

    Ok(Rebuilt {
        status: entry.status,
        list_value: total.list_value,
        currency: entry.currency,
        cost_kind: largest.map_or(CostKind::OrganizationOverhead, |(_, kind)| kind),
        dropped_tags,
    })
}

/// Each split's tokens and list value. Splits by message add up their own
/// messages, and a message no split names goes to the segment it falls in
/// by time. Splits by share divide the totals by the largest remainder, so
/// the parts always add up.
fn split_values(
    plan: &[NewSplit],
    messages: &[CountingMessage],
    total: &Values,
    currency: Currency,
) -> Result<Vec<Values>, StoreError> {
    if plan.is_empty() {
        return Ok(vec![total.clone()]);
    }
    if plan.iter().all(|split| !split.message_ids.is_empty()) {
        let mut values = vec![
            Values {
                tokens: TokenCounts::default(),
                list_value: Some(Money::zero(currency)),
            };
            plan.len()
        ];
        let last = plan.len().saturating_sub(1);
        // A segment starts at its first message; an unnamed message goes to
        // the latest segment started before it.
        let mut starts: Vec<Option<OffsetDateTime>> = vec![None; plan.len()];
        for message in messages {
            if let Some(position) = message.split_position.and_then(|p| usize::try_from(p).ok())
                && let Some(start) = starts.get_mut(position)
            {
                *start = Some(start.map_or(message.occurred_at, |at| at.min(message.occurred_at)));
            }
        }
        for message in messages {
            let position = message
                .split_position
                .and_then(|p| usize::try_from(p).ok())
                .filter(|p| *p <= last)
                .unwrap_or_else(|| {
                    starts
                        .iter()
                        .enumerate()
                        .filter(|(_, start)| start.is_some_and(|at| at <= message.occurred_at))
                        .max_by_key(|(_, start)| *start)
                        .map_or(0, |(position, _)| position)
                });
            if let Some(split) = values.get_mut(position) {
                add(split, message, currency)?;
            }
        }
        return Ok(values);
    }
    let weights: Vec<u64> = plan
        .iter()
        .map(|split| u64::from(split.share_percent.unwrap_or_default()))
        .collect();
    divide(total, &weights, currency)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(at: i64, input: u64, value: Option<i64>, split: Option<i32>) -> CountingMessage {
        CountingMessage {
            occurred_at: OffsetDateTime::from_unix_timestamp(at).unwrap(),
            tokens: TokenCounts {
                input,
                ..TokenCounts::default()
            },
            list_value_nanos: value,
            split_position: split,
        }
    }

    fn by_message(ids: &[&str]) -> NewSplit {
        NewSplit {
            message_ids: ids.iter().map(|id| (*id).to_owned()).collect(),
            share_percent: None,
            tags: BTreeMap::new(),
            signals: crate::ingest::entry::Signals::default(),
        }
    }

    #[test]
    fn segments_add_up_their_messages_and_take_unnamed_ones_by_time() {
        let plan = [by_message(&["a"]), by_message(&["b"])];
        let messages = [
            message(100, 1, Some(10), Some(0)),
            message(200, 2, Some(20), Some(1)),
            message(250, 4, Some(40), None),
            message(50, 8, None, None),
        ];
        let total = Values {
            tokens: TokenCounts::default(),
            list_value: None,
        };
        let values = split_values(&plan, &messages, &total, Currency::USD).unwrap();
        assert_eq!(values[0].tokens.input, 9);
        assert_eq!(values[0].list_value, None);
        assert_eq!(values[1].tokens.input, 6);
        assert_eq!(
            values[1].list_value,
            Some(Money::from_nanos(60, Currency::USD))
        );
    }

    #[test]
    fn shares_divide_without_losing_anything() {
        let share = |percent| NewSplit {
            share_percent: Some(percent),
            ..by_message(&[])
        };
        let plan = [share(60), share(40)];
        let total = Values {
            tokens: TokenCounts {
                input: 5,
                output: 3,
                ..TokenCounts::default()
            },
            list_value: Some(Money::from_nanos(101, Currency::USD)),
        };
        let values = split_values(&plan, &[], &total, Currency::USD).unwrap();
        assert_eq!(values[0].tokens.input + values[1].tokens.input, 5);
        assert_eq!(values[0].tokens.output + values[1].tokens.output, 3);
        assert_eq!(
            values[0].list_value.unwrap().nanos() + values[1].list_value.unwrap().nanos(),
            101
        );
        assert_eq!(values[0].list_value.unwrap().nanos(), 61);
    }
}
