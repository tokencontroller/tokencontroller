//! Token entries that arrive after their period closed (docs: Account >
//! Cadence and token sheets, late; Monthly close, after the close). A late
//! token sheet, or telemetry that arrives late, cannot change the closed
//! period, so each of its splits is posted to that month as an adjustment
//! marked late, and the token entry is locked with it.

use std::collections::BTreeMap;

use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use super::account;
use super::period::Period;
use crate::error::ApiError;
use crate::reports::filter::Filter;
use crate::reports::lines::{self, LineQuery, StatusView};
use crate::reports::scope::Scope;

/// Posts every late token entry of an organization's closed periods as
/// adjustments. Returns how many token entries were posted. Safe to run at
/// any time and more than once.
///
/// # Errors
///
/// A database error, or a missing exchange rate for a closed period.
pub async fn post_late_entries(db: &PgPool, organization_id: Uuid) -> Result<usize, ApiError> {
    let periods = sqlx::query_scalar!(
        r#"SELECT c.period FROM closed_periods c
           WHERE c.organization_id = $1 AND EXISTS (
               SELECT 1 FROM token_entries e
               WHERE e.organization_id = c.organization_id
                 AND e.status IN ('submitted', 'approved')
                 AND e.started_at >= c.period::timestamp AT TIME ZONE 'UTC'
                 AND e.started_at < (c.period + interval '1 month') AT TIME ZONE 'UTC'
           )
           ORDER BY c.period"#,
        organization_id,
    )
    .fetch_all(db)
    .await?;
    let mut posted = 0_usize;
    for first_day in periods {
        let period = Period::of_date(first_day);
        posted = posted.saturating_add(post_period(db, organization_id, period).await?);
    }
    Ok(posted)
}

async fn post_period(db: &PgPool, organization_id: Uuid, period: Period) -> Result<usize, ApiError> {
    let closed = account::load(
        db,
        organization_id,
        period,
        StatusView::All,
        &Scope::everything(),
        &Filter::default(),
    )
    .await?;
    let query = LineQuery {
        organization_id,
        from: period.starts_at(),
        until: period
            .ends_at()
            .map_err(|error| ApiError::BadRequest(error.to_string()))?,
        statuses: StatusView::All
            .statuses()
            .into_iter()
            .filter(|status| status != "locked")
            .collect(),
        filter: Filter::default(),
        with_late: false,
    };
    let late = lines::fetch(db, &query, &Scope::everything()).await?;
    let mut by_entry: BTreeMap<Uuid, Vec<lines::CostLine>> = BTreeMap::new();
    for line in late {
        by_entry.entry(line.entry_id).or_default().push(line);
    }
    let now = OffsetDateTime::now_utc();
    let mut posted = 0_usize;
    for (entry_id, splits) in by_entry {
        // A model missing from the price table has no value to post; the
        // entry waits until its price is known.
        let mut values = Vec::new();
        for split in &splits {
            let Some(list_value) = split.list_value else {
                values.clear();
                break;
            };
            let value = closed
                .converter
                .convert(Some(&split.provider), list_value)
                .map_err(|error| ApiError::Conflict(error.to_string()))?;
            values.push((split, value));
        }
        if values.is_empty() {
            continue;
        }
        let reason = if splits.iter().any(|split| split.automatic) {
            format!("Arrived after {period} closed, on {}", now.date())
        } else {
            format!("Late token sheet, submitted after {period} closed, on {}", now.date())
        };
        let mut tx = db.begin().await?;
        for (split, value) in values {
            if value.is_zero() {
                continue;
            }
            sqlx::query!(
                r#"INSERT INTO adjustments
                       (organization_id, period, amount_nanos, currency, tags, reason, late,
                        token_entry_id, split_id)
                   VALUES ($1, $2, $3, $4, $5, $6, true, $7, $8)
                   ON CONFLICT (split_id) WHERE late DO NOTHING"#,
                organization_id,
                period.first_day(),
                value.nanos(),
                value.currency().code(),
                Filter(split.tags.clone()).to_json(),
                reason,
                entry_id,
                split.split_id,
            )
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query!(
            "UPDATE token_entries SET status = 'locked' WHERE id = $1 AND status IN ('submitted', 'approved')",
            entry_id,
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        posted = posted.saturating_add(1);
    }
    if posted > 0 {
        tracing::info!(%organization_id, %period, posted, "late token entries posted as adjustments");
    }
    Ok(posted)
}
