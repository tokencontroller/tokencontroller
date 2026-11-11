//! The daily message limit of one organization. The usage endpoints take every
//! message (ADR 0022: accept everything, reconcile later); the one
//! exception is a service that many organizations share, where the build's
//! hooks can set a limit per organization so one of them cannot fill the
//! database. The open build sets none.

use uuid::Uuid;

use crate::AppState;
use crate::error::ApiError;
use crate::hooks::DailyMessageLimit;

/// The organization's daily message limit, if its build sets one.
///
/// # Errors
///
/// Whatever the hook fails with.
pub(super) async fn limit(
    state: &AppState,
    organization_id: Uuid,
) -> Result<Option<DailyMessageLimit>, ApiError> {
    state
        .hooks
        .daily_message_limit(state, organization_id)
        .await
}

/// [`limit`], or none when `exempt`, as for a re-read of stored records.
///
/// # Errors
///
/// Whatever the hook fails with.
pub(super) async fn limit_unless(
    state: &AppState,
    organization_id: Uuid,
    exempt: bool,
) -> Result<Option<DailyMessageLimit>, ApiError> {
    if exempt {
        return Ok(None);
    }
    limit(state, organization_id).await
}

/// How many messages the organization added since 00:00 UTC. A message
/// event that supersedes the earlier record of the same call is not a new
/// message, so it does not count. Nor does one a re-read made from a
/// record received before today.
async fn messages_today(db: &sqlx::PgPool, organization_id: Uuid) -> Result<i64, ApiError> {
    Ok(sqlx::query_scalar!(
        r#"SELECT count(*) AS "count!" FROM events
           WHERE organization_id = $1 AND kind = 'message' AND supersedes IS NULL
             AND recorded_at >= date_trunc('day', now() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC'
             AND NOT EXISTS (
                 SELECT 1 FROM received_records r
                 WHERE r.organization_id = events.organization_id
                   AND r.id = events.payload->'message'->>'received'
                   AND r.received_at < date_trunc('day', now() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC'
             )"#,
        organization_id
    )
    .fetch_one(db)
    .await?)
}

/// Refuses a batch that would take the organization past its daily limit.
/// The whole batch is refused, so the sender can send it again after
/// 00:00 UTC and nothing is half stored. Two batches at the same moment can
/// pass together and end a little above the limit; it stops the next one.
///
/// # Errors
///
/// [`ApiError::TooManyRequests`] with the limit's message, or a database
/// error.
pub(super) async fn check(
    state: &AppState,
    organization_id: Uuid,
    limit: Option<&DailyMessageLimit>,
    adding: usize,
) -> Result<(), ApiError> {
    let Some(limit) = limit else {
        return Ok(());
    };
    let today = messages_today(&state.db, organization_id).await?;
    let after = today.saturating_add(i64::try_from(adding).unwrap_or(i64::MAX));
    if today >= limit.max || after > limit.max {
        return Err(ApiError::TooManyRequests(format!(
            "{} This organization has {today} of {} messages today; nothing of this batch was stored. Send it again after 00:00 UTC.",
            limit.message, limit.max
        )));
    }
    Ok(())
}
