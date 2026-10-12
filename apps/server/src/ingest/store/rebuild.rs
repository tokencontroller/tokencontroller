//! Rebuilding a token entry from its messages: the totals, then every split
//! with its values and its tags copied in (ADR 0008). Runs whenever a door
//! adds to a token entry, so all doors share one way of adding up.

use std::collections::{BTreeMap, BTreeSet};

use sqlx::PgConnection;
use time::OffsetDateTime;
use tokencontroller_core::{
    CostKind, Currency, Money, TagMap, TagName, TagValue, Tags, TokenCounts,
};
use uuid::Uuid;

use super::{StoreError, to_i64, to_u64};
use crate::ingest::entry::{NewSplit, PERSON_TAG};

/// What an organization's settings say about tags.
#[derive(Debug, Clone)]
pub(super) struct OrgContext {
    /// Tag names on the organization's list, built-in ones included.
    pub tag_names: BTreeSet<String>,
    pub tag_map: TagMap,
}

pub(super) async fn org_context(
    conn: &mut PgConnection,
    organization_id: Uuid,
) -> Result<OrgContext, StoreError> {
    let mut tag_names: BTreeSet<String> = sqlx::query_scalar!(
        "SELECT name FROM tag_names WHERE organization_id = $1",
        organization_id
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .collect();
    for built_in in [PERSON_TAG, "ticket", "project"] {
        tag_names.insert(built_in.to_owned());
    }
    let map = sqlx::query!(
        "SELECT cost_center, ladder FROM tag_map_versions
         WHERE organization_id = $1 ORDER BY id DESC LIMIT 1",
        organization_id
    )
    .fetch_optional(&mut *conn)
    .await?;
    let tag_map = map
        .and_then(|row| {
            let cost_center = TagName::new(&row.cost_center).ok()?;
            let ladder = row
                .ladder
                .iter()
                .map(|rung| TagName::new(rung))
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            TagMap::new(cost_center, ladder).ok()
        })
        .unwrap_or_default();
    Ok(OrgContext { tag_names, tag_map })
}

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
    repository: Option<String>,
}

async fn entry_row(
    conn: &mut PgConnection,
    organization_id: Uuid,
    entry_id: Uuid,
) -> Result<EntryRow, StoreError> {
    let row = sqlx::query!(
        r#"SELECT agent_manager_id, currency, door, status, sent_tags, split_plan,
                  telemetry_attributes, signals
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
        repository: row
            .signals
            .get("repository")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
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

/// One split as the plan describes it.
struct Shape {
    method: &'static str,
    share_percent: Option<u32>,
    tags: BTreeMap<String, String>,
    signals: serde_json::Value,
}

fn shapes(plan: &[NewSplit]) -> Vec<Shape> {
    if plan.is_empty() {
        return vec![Shape {
            method: "whole",
            share_percent: None,
            tags: BTreeMap::new(),
            signals: serde_json::json!({}),
        }];
    }
    plan.iter()
        .map(|split| Shape {
            method: if split.message_ids.is_empty() {
                "share"
            } else {
                "by_message"
            },
            share_percent: split.share_percent,
            tags: split.tags.clone(),
            signals: serde_json::to_value(&split.signals).unwrap_or_default(),
        })
        .collect()
}

async fn insert_split(
    conn: &mut PgConnection,
    organization_id: Uuid,
    entry_id: Uuid,
    position: usize,
    shape: &Shape,
    values: &Values,
    tags: &BTreeMap<String, (String, &'static str)>,
) -> Result<(), StoreError> {
    let position = i32::try_from(position).map_err(|_| StoreError::Overflow)?;
    let share_percent = shape
        .share_percent
        .map(i32::try_from)
        .transpose()
        .map_err(|_| StoreError::Overflow)?;
    let split_id = sqlx::query_scalar!(
        r#"INSERT INTO splits (token_entry_id, position, method, share_percent,
               input_tokens, output_tokens, cache_write_tokens, cache_read_tokens,
               list_value_nanos, signals)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) RETURNING id"#,
        entry_id,
        position,
        shape.method,
        share_percent,
        to_i64(values.tokens.input)?,
        to_i64(values.tokens.output)?,
        to_i64(values.tokens.cache_write)?,
        to_i64(values.tokens.cache_read)?,
        values.list_value.map(Money::nanos),
        shape.signals
    )
    .fetch_one(&mut *conn)
    .await?;
    for (name, (value, source)) in tags {
        sqlx::query!(
            "INSERT INTO split_tags (split_id, organization_id, name, value, source)
             VALUES ($1, $2, $3, $4, $5)",
            split_id,
            organization_id,
            name,
            value,
            *source
        )
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

async fn person_tags(
    conn: &mut PgConnection,
    person: Option<Uuid>,
) -> Result<BTreeMap<String, String>, StoreError> {
    let Some(person) = person else {
        return Ok(BTreeMap::new());
    };
    let tags = sqlx::query_scalar!("SELECT tags FROM people WHERE id = $1", person)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(tags
        .and_then(|tags| serde_json::from_value(tags).ok())
        .unwrap_or_default())
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
    let person = person_tags(conn, entry.agent_manager_id).await?;
    let values = split_values(&entry.plan, &messages, &total, entry.currency)?;

    sqlx::query!("DELETE FROM splits WHERE token_entry_id = $1", entry_id)
        .execute(&mut *conn)
        .await?;
    let mut largest: Option<(u64, CostKind)> = None;
    for (position, (shape, split_values)) in shapes(&entry.plan).iter().zip(values).enumerate() {
        let layers = Layers {
            person: &person,
            telemetry: &entry.telemetry,
            sent: &entry.sent,
            split: &shape.tags,
        };
        let tags = place(
            conn,
            organization_id,
            &layers,
            entry.agent_manager_id,
            entry.repository.as_deref(),
            context,
        )
        .await?;
        let core_tags: Tags = tags
            .iter()
            .filter_map(|(name, (value, _))| {
                Some((TagName::new(name).ok()?, TagValue::new(value).ok()?))
            })
            .collect();
        let kind = context.tag_map.cost_kind(&core_tags);
        let weight = split_values.tokens.total().unwrap_or_default();
        if largest.is_none_or(|(heaviest, _)| weight > heaviest) {
            largest = Some((weight, kind));
        }
        insert_split(
            conn,
            organization_id,
            entry_id,
            position,
            shape,
            &split_values,
            &tags,
        )
        .await?;
    }

    Ok(Rebuilt {
        status: entry.status,
        list_value: total.list_value,
        currency: entry.currency,
        cost_kind: largest.map_or(CostKind::OrganizationOverhead, |(_, kind)| kind),
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
    let by_kind = |count: u64| -> Result<Vec<u64>, StoreError> {
        Money::from_nanos(to_i64(count)?, currency)
            .allocate(&weights)?
            .into_iter()
            .map(|part| u64::try_from(part.nanos()).map_err(|_| StoreError::Overflow))
            .collect()
    };
    let input = by_kind(total.tokens.input)?;
    let output = by_kind(total.tokens.output)?;
    let cache_write = by_kind(total.tokens.cache_write)?;
    let cache_read = by_kind(total.tokens.cache_read)?;
    let list_values: Vec<Option<Money>> = match total.list_value {
        Some(value) => value.allocate(&weights)?.into_iter().map(Some).collect(),
        None => vec![None; plan.len()],
    };
    Ok((0..plan.len())
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

/// The tags a split gets from each source, lowest first.
struct Layers<'a> {
    person: &'a BTreeMap<String, String>,
    telemetry: &'a BTreeMap<String, String>,
    sent: &'a BTreeMap<String, String>,
    split: &'a BTreeMap<String, String>,
}

/// Places one split: picks its ticket and project, copies the tags of its
/// person, project and ticket, and lays telemetry's and the sender's tags on
/// top. Only tag names on the organization's list are kept.
async fn place(
    conn: &mut PgConnection,
    organization_id: Uuid,
    layers: &Layers<'_>,
    agent_manager: Option<Uuid>,
    repository: Option<&str>,
    context: &OrgContext,
) -> Result<BTreeMap<String, (String, &'static str)>, StoreError> {
    let named = |name: &str| {
        layers
            .split
            .get(name)
            .or_else(|| layers.sent.get(name))
            .cloned()
    };
    let ticket_key = named("ticket");
    let ticket = match &ticket_key {
        Some(key) => {
            sqlx::query!(
                r#"SELECT t.tags, p.name AS "project?", p.tags AS "project_tags?"
               FROM tickets t LEFT JOIN projects p ON p.id = t.project_id
               WHERE t.organization_id = $1 AND t.key = $2"#,
                organization_id,
                key
            )
            .fetch_optional(&mut *conn)
            .await?
        }
        None => None,
    };
    let mut project_source = "sent";
    let mut project_name = named("project");
    if project_name.is_none() {
        project_name = ticket.as_ref().and_then(|ticket| ticket.project.clone());
        project_source = "ticket";
    }
    if project_name.is_none()
        && let Some(repository) = repository.and_then(repository_key)
    {
        project_name = sqlx::query_scalar!(
            "SELECT p.name FROM project_repositories r JOIN projects p ON p.id = r.project_id
             WHERE r.organization_id = $1 AND lower(r.repository) = $2",
            organization_id,
            repository
        )
        .fetch_optional(&mut *conn)
        .await?;
        project_source = "repository";
    }
    let project_tags = match &project_name {
        Some(name) => sqlx::query_scalar!(
            "SELECT tags FROM projects WHERE organization_id = $1 AND name = $2",
            organization_id,
            name
        )
        .fetch_optional(&mut *conn)
        .await?
        .and_then(|tags| serde_json::from_value::<BTreeMap<String, String>>(tags).ok())
        .unwrap_or_default(),
        None => BTreeMap::new(),
    };
    let ticket_tags: BTreeMap<String, String> = ticket
        .as_ref()
        .and_then(|ticket| serde_json::from_value(ticket.tags.clone()).ok())
        .unwrap_or_default();

    let mut tags: BTreeMap<String, (String, &'static str)> = BTreeMap::new();
    let mut lay = |source: &'static str, layer: &BTreeMap<String, String>| {
        for (name, value) in layer {
            if context.tag_names.contains(name) && !value.trim().is_empty() {
                tags.insert(name.clone(), (value.clone(), source));
            }
        }
    };
    lay("person", layers.person);
    lay("project", &project_tags);
    lay("ticket", &ticket_tags);
    lay("telemetry", layers.telemetry);
    lay("sent", layers.sent);
    lay("sent", layers.split);
    tags.remove(PERSON_TAG);
    tags.remove("ticket");
    tags.remove("project");
    if let Some(person) = agent_manager {
        tags.insert(PERSON_TAG.to_owned(), (person.to_string(), "person"));
    }
    if let Some(key) = ticket_key {
        tags.insert("ticket".to_owned(), (key, "sent"));
    }
    if let Some(name) = project_name {
        tags.insert("project".to_owned(), (name, project_source));
    }
    Ok(tags)
}

/// The `owner/name` a project's repository mapping uses, from a repository
/// as agents report it, such as `github.com/kestrel/storefront` or
/// `https://github.com/kestrel/storefront.git`.
#[must_use]
pub(crate) fn repository_key(repository: &str) -> Option<String> {
    let trimmed = repository
        .trim()
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .to_ascii_lowercase();
    let without_scheme = trimmed
        .split_once("://")
        .map_or(trimmed.as_str(), |(_, rest)| rest);
    let path = without_scheme
        .rsplit_once(':')
        .filter(|(host, _)| host.contains('@'))
        .map_or(without_scheme, |(_, path)| path);
    let mut parts = path.rsplit('/').filter(|part| !part.is_empty());
    let name = parts.next()?;
    let owner = parts.next()?;
    Some(format!("{owner}/{name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repositories_become_owner_and_name() {
        for repository in [
            "github.com/kestrel/storefront",
            "https://github.com/Kestrel/storefront.git",
            "git@github.com:kestrel/storefront.git",
            "kestrel/storefront",
            "dev.azure.com/acme/kestrel/storefront/",
        ] {
            assert_eq!(
                repository_key(repository).as_deref(),
                Some("kestrel/storefront"),
                "{repository}"
            );
        }
        assert_eq!(repository_key("storefront"), None);
    }

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
