//! Token entries as tables show them: one row per token entry with its
//! splits and tags, for review, approval and the token entries table, and
//! the same as CSV (docs: Report > Exports).

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use tokencontroller_core::Money;
use utoipa::ToSchema;
use uuid::Uuid;

use super::csv::Csv;
use super::filter::Tags;
use super::lines::{CostLine, EntryStatus};
use crate::bills::amount::format_amount;

/// One split of a token entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct SplitView {
    pub id: Uuid,
    /// `null` for an unknown model.
    pub list_value: Option<String>,
    #[schema(value_type = BTreeMap<String, String>)]
    pub tags: Tags,
}

/// One token entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct EntryView {
    pub id: Uuid,
    pub agent_harness: String,
    pub agent_name: Option<String>,
    pub model: String,
    pub provider: String,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub started_at: OffsetDateTime,
    pub status: EntryStatus,
    /// Arrived with no person involved, such as from CI.
    pub automatic: bool,
    pub summary: String,
    pub agent_manager: Option<String>,
    /// The list value of all splits; `null` when the model is unknown.
    pub list_value: Option<String>,
    #[schema(value_type = String, example = "USD")]
    pub currency: String,
    /// `list-price estimate`, or `unknown model`.
    pub label: &'static str,
    pub splits: Vec<SplitView>,
    /// Why a people manager sent it back.
    pub rejected_reason: Option<String>,
}

/// Groups cost lines into token entries, in the order they came.
///
/// # Errors
///
/// A database error while reading names and reasons.
pub async fn entries(db: &PgPool, lines: Vec<CostLine>) -> Result<Vec<EntryView>, sqlx::Error> {
    let people: Vec<Uuid> = lines
        .iter()
        .filter_map(|line| line.agent_manager_id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let names: BTreeMap<Uuid, String> =
        sqlx::query!("SELECT id, name FROM people WHERE id = ANY($1)", &people)
            .fetch_all(db)
            .await?
            .into_iter()
            .map(|row| (row.id, row.name))
            .collect();
    let rejected: Vec<Uuid> = lines
        .iter()
        .filter(|line| line.status == EntryStatus::Rejected)
        .map(|line| line.entry_id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let reasons: BTreeMap<Uuid, String> = sqlx::query!(
        r#"SELECT DISTINCT ON (token_entry_id) token_entry_id, reason AS "reason!"
           FROM token_entry_decisions
           WHERE token_entry_id = ANY($1) AND decision = 'rejected'
           ORDER BY token_entry_id, decided_at DESC"#,
        &rejected,
    )
    .fetch_all(db)
    .await?
    .into_iter()
    .map(|row| (row.token_entry_id, row.reason))
    .collect();

    let mut order: Vec<Uuid> = Vec::new();
    let mut grouped: BTreeMap<Uuid, Vec<CostLine>> = BTreeMap::new();
    for line in lines {
        if !grouped.contains_key(&line.entry_id) {
            order.push(line.entry_id);
        }
        grouped.entry(line.entry_id).or_default().push(line);
    }
    Ok(order
        .into_iter()
        .filter_map(|id| {
            let splits = grouped.remove(&id)?;
            let first = splits.first()?.clone();
            let currency = first.list_value.map(Money::currency);
            let total = currency.and_then(|currency| {
                let values: Option<Vec<Money>> =
                    splits.iter().map(|split| split.list_value).collect();
                values.and_then(|values| Money::sum(values, currency).ok())
            });
            Some(EntryView {
                id,
                agent_harness: first.agent_harness.clone(),
                agent_name: first.agent_name.clone(),
                model: first.model.clone(),
                provider: first.provider.clone(),
                started_at: first.started_at,
                status: first.status,
                automatic: !first.on_token_sheet,
                summary: first.summary.clone(),
                agent_manager: first
                    .agent_manager_id
                    .and_then(|person| names.get(&person).cloned()),
                list_value: total.map(format_amount),
                currency: currency
                    .map_or_else(|| "USD".to_owned(), tokencontroller_core::Currency::code),
                label: if total.is_some() {
                    "list-price estimate"
                } else {
                    "unknown model"
                },
                rejected_reason: reasons.get(&id).cloned(),
                splits: splits
                    .into_iter()
                    .map(|split| SplitView {
                        id: split.split_id,
                        list_value: split.list_value.map(format_amount),
                        tags: split.tags,
                    })
                    .collect(),
            })
        })
        .collect())
}

/// Token entries as CSV: one row per split, with a column per tag name.
#[must_use]
pub fn to_csv(entries: &[EntryView]) -> Csv {
    let tag_names: BTreeSet<&String> = entries
        .iter()
        .flat_map(|entry| entry.splits.iter().flat_map(|split| split.tags.keys()))
        .collect();
    let mut header: Vec<String> = [
        "token_entry",
        "started_at",
        "agent_harness",
        "agent_name",
        "model",
        "provider",
        "agent_manager",
        "status",
        "automatic",
        "summary",
    ]
    .map(str::to_owned)
    .to_vec();
    header.extend(tag_names.iter().map(|name| (*name).clone()));
    header.extend(["list_value", "currency", "label"].map(str::to_owned));
    let mut csv = Csv::new(header);
    for entry in entries {
        let started_at = entry
            .started_at
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        for split in &entry.splits {
            let mut cells = vec![
                entry.id.to_string(),
                started_at.clone(),
                entry.agent_harness.clone(),
                entry.agent_name.clone().unwrap_or_default(),
                entry.model.clone(),
                entry.provider.clone(),
                entry.agent_manager.clone().unwrap_or_default(),
                entry.status.as_str().to_owned(),
                if entry.automatic { "yes" } else { "no" }.to_owned(),
                entry.summary.clone(),
            ];
            cells.extend(
                tag_names
                    .iter()
                    .map(|name| split.tags.get(*name).cloned().unwrap_or_default()),
            );
            cells.extend([
                split.list_value.clone().unwrap_or_default(),
                entry.currency.clone(),
                entry.label.to_owned(),
            ]);
            csv.row(cells);
        }
    }
    csv
}
