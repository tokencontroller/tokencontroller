//! Cost lines: one split of one token entry with its tags, the one shape
//! every report, the cost sheet, reconciliation and token budgets read
//! (ADR 0008: reports filter and group on the split's own tag rows).

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use tokencontroller_core::{Currency, ModelRef, Money, PlacedBy, Split, Status, Valuation};
use utoipa::ToSchema;
use uuid::Uuid;

use super::filter::{Filter, Tags};
use super::scope::Scope;

/// Where a token entry stands (docs: Reference > Data model, status), as the
/// API and the database spell it. What counts where is the core's
/// [`Status`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum EntryStatus {
    Draft,
    Submitted,
    Approved,
    Rejected,
    Locked,
}

impl EntryStatus {
    /// Every status, in the order a token entry moves through them.
    pub const ALL: [Self; 5] = [
        Self::Draft,
        Self::Submitted,
        Self::Approved,
        Self::Rejected,
        Self::Locked,
    ];

    /// The core's status.
    #[must_use]
    pub const fn core(self) -> Status {
        match self {
            Self::Draft => Status::Draft,
            Self::Submitted => Status::Submitted,
            Self::Approved => Status::Approved,
            Self::Rejected => Status::Rejected,
            Self::Locked => Status::Locked,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Submitted => "submitted",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::Locked => "locked",
        }
    }

    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "draft" => Some(Self::Draft),
            "submitted" => Some(Self::Submitted),
            "approved" => Some(Self::Approved),
            "rejected" => Some(Self::Rejected),
            "locked" => Some(Self::Locked),
            _ => None,
        }
    }

    /// Whether a token entry in this status counts in reports. Drafts are
    /// still on the machine and rejected ones do not count until fixed.
    #[must_use]
    pub const fn counts(self) -> bool {
        self.core().counts_in_reports()
    }
}

/// Which counting token entries a report shows (docs: Report > Reading
/// reports, status).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum StatusView {
    /// Submitted, approved and locked.
    #[default]
    All,
    /// Approved and locked only: for organizations with approval on.
    Approved,
}

impl StatusView {
    /// The statuses this view shows: those that count in reports, or with
    /// approval on, those that count on the cost sheet.
    #[must_use]
    pub fn statuses(self) -> Vec<String> {
        EntryStatus::ALL
            .into_iter()
            .filter(|status| match self {
                Self::All => status.core().counts_in_reports(),
                Self::Approved => status.core().counts(true),
            })
            .map(|status| status.as_str().to_owned())
            .collect()
    }
}

/// The statuses that count on the cost sheet: with approval on, approved
/// and locked only.
#[must_use]
pub fn cost_sheet_statuses(approval_on: bool) -> Vec<String> {
    EntryStatus::ALL
        .into_iter()
        .filter(|status| status.core().counts(approval_on))
        .map(|status| status.as_str().to_owned())
        .collect()
}

/// One split of one token entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CostLine {
    pub split_id: Uuid,
    pub entry_id: Uuid,
    pub agent_manager_id: Option<Uuid>,
    pub provider: String,
    pub agent_harness: String,
    pub agent_name: Option<String>,
    pub model: String,
    pub effort: Option<String>,
    pub speed: Option<String>,
    pub started_at: OffsetDateTime,
    pub status: EntryStatus,
    /// Reviewed and submitted on a token sheet; otherwise it arrived
    /// automatically.
    pub on_token_sheet: bool,
    pub summary: String,
    /// The currency of the token entry's values.
    pub currency: Currency,
    /// The split's list value; `None` for an unknown model, which is never
    /// counted as zero.
    pub list_value: Option<Money>,
    /// What the provider actually charged for the split, where known.
    pub billed_value: Option<Money>,
    /// What placed the split, such as `rule: ticket key in branch`.
    pub placed_by: String,
    /// Its ticket matched a pattern but is not known yet.
    pub ticket_not_synced: bool,
    /// Every tag of the split, with `person` always set.
    pub tags: Tags,
}

impl CostLine {
    /// The split's tags as the core reads them. A tag the core cannot read is
    /// left out; the checks on the way in prevent those.
    #[must_use]
    pub fn core_tags(&self) -> tokencontroller_core::Tags {
        crate::close::account::core_tags(&self.tags)
    }

    /// The split as the core computes with it. A tag the core cannot read is
    /// left out; the checks on the way in prevent those.
    #[must_use]
    pub fn split(&self) -> Split {
        let tags = self.core_tags();
        let unknown_models: BTreeSet<ModelRef> = if self.list_value.is_none() {
            BTreeSet::from([ModelRef::new(&self.provider, &self.model)])
        } else {
            BTreeSet::new()
        };
        Split {
            tags,
            value: Valuation {
                list: self.list_value.unwrap_or(Money::zero(self.currency)),
                billed: self.billed_value,
                unknown_models,
            },
            placed_by: placed_by(&self.placed_by),
            ticket_not_synced: self.ticket_not_synced,
        }
    }
}

/// Reads what placed a split back from the way the review list shows it.
fn placed_by(text: &str) -> PlacedBy {
    match text {
        "by hand" | "hand" => PlacedBy::Named,
        "config file" => PlacedBy::Configuration,
        "matched by summary" | "classifier" => PlacedBy::SummaryMatch,
        other => other
            .strip_prefix("rule: ")
            .map_or(PlacedBy::Nothing, |name| PlacedBy::Rule(name.to_owned())),
    }
}

/// Which cost lines to read.
#[derive(Debug, Clone)]
pub struct LineQuery {
    pub organization_id: Uuid,
    /// From this moment on.
    pub from: OffsetDateTime,
    /// Up to, not including, this moment.
    pub until: OffsetDateTime,
    pub statuses: Vec<String>,
    pub filter: Filter,
    /// Also read splits that were posted as late adjustments to a closed
    /// period. Only the review of token entries wants them.
    pub with_late: bool,
}

/// Reads the cost lines `query` names that `scope` may see, oldest first.
///
/// # Errors
///
/// A database error.
pub async fn fetch(
    db: &PgPool,
    query: &LineQuery,
    scope: &Scope,
) -> Result<Vec<CostLine>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"SELECT s.id AS split_id, e.id AS entry_id, e.agent_manager_id, e.provider,
                  e.agent_harness, e.agent_name, e.model, e.effort, e.speed, e.started_at,
                  e.status, e.on_token_sheet, coalesce(e.summary, '') AS "summary!",
                  s.list_value_nanos, s.billed_value_nanos, s.placed_by, s.ticket_not_synced, e.currency,
                  p.email AS "person_email?",
                  coalesce(
                      (SELECT jsonb_object_agg(t.name, t.value) FROM split_tags t WHERE t.split_id = s.id),
                      '{}'::jsonb
                  ) AS "tags!"
           FROM splits s
           JOIN token_entries e ON e.id = s.token_entry_id
           LEFT JOIN people p ON p.id = e.agent_manager_id
           WHERE e.organization_id = $1
             AND e.started_at >= $2 AND e.started_at < $3
             AND e.status = ANY($4)
             AND ($5 OR NOT EXISTS (SELECT 1 FROM adjustments a WHERE a.split_id = s.id AND a.late))
           ORDER BY e.started_at, e.id, s.id"#,
        query.organization_id,
        query.from,
        query.until,
        &query.statuses,
        query.with_late,
    )
    .fetch_all(db)
    .await?;

    let lines = rows
        .into_iter()
        .filter_map(|row| {
            let status = EntryStatus::parse(&row.status)?;
            let currency = Currency::new(&row.currency).ok()?;
            let mut tags = Filter::from_json(&row.tags).0;
            if let Some(email) = row.person_email {
                tags.entry("person".to_owned()).or_insert(email);
            }
            Some(CostLine {
                split_id: row.split_id,
                entry_id: row.entry_id,
                agent_manager_id: row.agent_manager_id,
                provider: row.provider,
                agent_harness: row.agent_harness,
                agent_name: row.agent_name,
                model: row.model,
                effort: row.effort,
                speed: row.speed,
                started_at: row.started_at,
                status,
                on_token_sheet: row.on_token_sheet,
                summary: row.summary,
                currency,
                list_value: row
                    .list_value_nanos
                    .map(|nanos| Money::from_nanos(nanos, currency)),
                billed_value: row
                    .billed_value_nanos
                    .map(|nanos| Money::from_nanos(nanos, currency)),
                placed_by: row.placed_by,
                ticket_not_synced: row.ticket_not_synced,
                tags,
            })
        })
        .filter(|line| query.filter.matches(&line.tags) && scope.sees(line))
        .collect();
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_round_trip_and_count() {
        for status in [
            EntryStatus::Draft,
            EntryStatus::Submitted,
            EntryStatus::Approved,
            EntryStatus::Rejected,
            EntryStatus::Locked,
        ] {
            assert_eq!(EntryStatus::parse(status.as_str()), Some(status));
        }
        assert_eq!(EntryStatus::parse("posted"), None);
        assert!(!EntryStatus::Draft.counts());
        assert!(!EntryStatus::Rejected.counts());
        assert!(EntryStatus::Submitted.counts());
        assert!(EntryStatus::Approved.counts());
        assert!(EntryStatus::Locked.counts());
    }

    #[test]
    fn the_approved_view_leaves_out_submitted() {
        assert_eq!(
            StatusView::All.statuses(),
            ["submitted", "approved", "locked"]
        );
        assert_eq!(StatusView::Approved.statuses(), ["approved", "locked"]);
        assert_eq!(
            cost_sheet_statuses(false),
            ["submitted", "approved", "locked"]
        );
        assert_eq!(cost_sheet_statuses(true), ["approved", "locked"]);
    }

    #[test]
    fn placed_by_reads_back_as_the_review_list_shows_it() {
        assert_eq!(placed_by("by hand"), PlacedBy::Named);
        assert_eq!(placed_by("config file"), PlacedBy::Configuration);
        assert_eq!(placed_by("matched by summary"), PlacedBy::SummaryMatch);
        assert_eq!(
            placed_by("rule: ticket key in branch"),
            PlacedBy::Rule("ticket key in branch".to_owned())
        );
        assert_eq!(placed_by("no ticket"), PlacedBy::Nothing);
        for shown in [
            "by hand",
            "config file",
            "matched by summary",
            "rule: x",
            "no ticket",
        ] {
            assert_eq!(placed_by(shown).to_string(), shown);
        }
    }
}
