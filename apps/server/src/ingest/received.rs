//! Received records: what a usage endpoint received, stored before it is
//! read (ADR 0028, "collection is holy"). They live outside the event log,
//! with a time after which they are deleted and a real delete. Content text
//! is removed before they are stored unless the organization keeps it.
//! Messages and activities read from them point back by id.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::AppState;
use crate::error::{ApiError, ErrorBody};
use crate::organizations::Role;
use crate::sign_in::Member;

/// How long a received record is kept: 400 days, so a year's figures and
/// the bills that come late can be read again.
pub const RETENTION_DAYS: i64 = 400;

/// The background job that deletes received records past their time.
pub const JOB: &str = "received-records";

/// How often the job runs.
pub const JOB_EVERY: std::time::Duration = std::time::Duration::from_hours(1);

/// The most records one run of the job deletes; the rest wait an hour.
const DELETE_PER_RUN: i64 = 50_000;

/// The start of every received record's id.
pub const ID_PREFIX: &str = "rcv_";

/// The usage endpoints that store what they received.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endpoint {
    TokenEntries,
    TokenEntriesCsv,
    OtlpLogs,
    OtlpTraces,
    OtlpMetrics,
}

impl Endpoint {
    /// The name stored with the record.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TokenEntries => "token-entries",
            Self::TokenEntriesCsv => "token-entries-csv",
            Self::OtlpLogs => "otlp-logs",
            Self::OtlpTraces => "otlp-traces",
            Self::OtlpMetrics => "otlp-metrics",
        }
    }
}

/// What one usage endpoint received, ready to be stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arrival<'a> {
    pub endpoint: Endpoint,
    /// The person the credential belongs to, by email.
    pub received_by: Option<&'a str>,
    /// How `body` is written.
    pub content_type: &'a str,
    /// What arrived, with content text removed unless `content_kept`.
    pub body: &'a [u8],
    /// The size as it arrived, which the daily byte cap counts.
    pub bytes: usize,
    pub content_kept: bool,
}

/// Whether the organization keeps content text (prompts, responses, tool
/// output, file contents) in its received records. Off unless an org
/// manager switches it on.
///
/// # Errors
///
/// A database error.
pub async fn keeps_content_text(
    db: &sqlx::PgPool,
    organization_id: Uuid,
) -> Result<bool, ApiError> {
    Ok(sqlx::query_scalar!(
        "SELECT keep_content_text FROM organizations WHERE id = $1",
        organization_id
    )
    .fetch_optional(db)
    .await?
    .unwrap_or(false))
}

/// Refuses an arrival that would take the organization past its daily
/// byte cap, when its build sets one. The cap counts the bytes as they
/// arrived since 00:00 UTC; records read again by a newer reader add none.
///
/// # Errors
///
/// [`ApiError::TooManyRequests`] with the cap's message, or a database
/// error.
pub async fn check_bytes(
    state: &AppState,
    organization_id: Uuid,
    adding: usize,
) -> Result<(), ApiError> {
    let Some(limit) = state
        .hooks
        .daily_received_bytes_limit(state, organization_id)
        .await?
    else {
        return Ok(());
    };
    let today = sqlx::query_scalar!(
        r#"SELECT coalesce(sum(bytes), 0)::bigint AS "bytes!" FROM received_records
           WHERE organization_id = $1
             AND received_at >= date_trunc('day', now() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC'"#,
        organization_id
    )
    .fetch_one(&state.db)
    .await?;
    let after = today.saturating_add(i64::try_from(adding).unwrap_or(i64::MAX));
    if today >= limit.max || after > limit.max {
        return Err(ApiError::TooManyRequests(format!(
            "{} This organization received {today} of {} bytes today; nothing of this request was stored. Send it again after 00:00 UTC.",
            limit.message, limit.max
        )));
    }
    Ok(())
}

/// Stores what arrived and answers its id, such as `rcv_…`. It is deleted
/// after [`RETENTION_DAYS`].
///
/// # Errors
///
/// A database error.
pub async fn store(
    db: &sqlx::PgPool,
    organization_id: Uuid,
    arrival: &Arrival<'_>,
) -> Result<String, ApiError> {
    let id = format!("{ID_PREFIX}{}", Uuid::new_v4().simple());
    sqlx::query!(
        r#"INSERT INTO received_records
             (id, organization_id, endpoint, received_by, content_type, body, bytes, content_kept, delete_after)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, now() + make_interval(days => $9))"#,
        id,
        organization_id,
        arrival.endpoint.as_str(),
        arrival.received_by,
        arrival.content_type,
        arrival.body,
        i64::try_from(arrival.bytes).unwrap_or(i64::MAX),
        arrival.content_kept,
        i32::try_from(RETENTION_DAYS).unwrap_or(i32::MAX),
    )
    .execute(db)
    .await?;
    Ok(id)
}

/// Deletes the received records past their time, at most
/// [`DELETE_PER_RUN`] at once, and answers how many.
///
/// # Errors
///
/// A database error.
pub async fn delete_expired(db: &sqlx::PgPool, now: OffsetDateTime) -> Result<u64, ApiError> {
    let deleted = sqlx::query!(
        r#"DELETE FROM received_records WHERE id IN (
             SELECT id FROM received_records WHERE delete_after <= $1
             ORDER BY delete_after LIMIT $2)"#,
        now,
        DELETE_PER_RUN
    )
    .execute(db)
    .await?;
    Ok(deleted.rows_affected())
}

/// The background job: deletes received records past their time.
///
/// # Errors
///
/// When the database cannot be reached.
pub async fn run_job(state: AppState) -> anyhow::Result<String> {
    let deleted = delete_expired(&state.db, OffsetDateTime::now_utc())
        .await
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    Ok(format!(
        "{deleted} received records past their time deleted"
    ))
}

/// One received record, without its body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ReceivedRecordView {
    pub id: String,
    /// `token-entries`, `token-entries-csv`, `otlp-logs`, `otlp-traces` or
    /// `otlp-metrics`.
    pub endpoint: String,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub received_at: OffsetDateTime,
    /// The person the credential belongs to, by email.
    pub received_by: Option<String>,
    pub content_type: String,
    /// The size as it arrived.
    pub bytes: i64,
    /// The size as stored, after content text was removed.
    pub stored_bytes: i64,
    /// Whether content text was kept.
    pub content_kept: bool,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub delete_after: OffsetDateTime,
}

/// Which received records to list or delete.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ReceivedQuery {
    /// Only records received before this moment or day (the day's start):
    /// RFC 3339 or `YYYY-MM-DD`.
    pub before: Option<String>,
    /// At most this many records, the latest first. Default 100, at most
    /// 1,000. Listing only.
    pub limit: Option<i64>,
}

fn before_of(query: &ReceivedQuery) -> Result<Option<OffsetDateTime>, ApiError> {
    let Some(text) = query.before.as_deref().map(str::trim) else {
        return Ok(None);
    };
    if let Ok(moment) = OffsetDateTime::parse(text, &time::format_description::well_known::Rfc3339)
    {
        return Ok(Some(moment));
    }
    let day = crate::dates::parse(text)?;
    Ok(Some(day.midnight().assume_utc()))
}

fn require_org_manager(member: &Member) -> Result<(), ApiError> {
    member.require_session()?;
    member.require_role(Role::OrgManager)
}

/// The organization's received records, the latest first, without their
/// bodies. Org managers only.
#[utoipa::path(
    get,
    path = "/received-records",
    tag = "received records",
    params(ReceivedQuery),
    responses(
        (status = OK, body = Vec<ReceivedRecordView>),
        (status = FORBIDDEN, body = ErrorBody),
    ),
)]
pub(super) async fn list_received(
    State(state): State<AppState>,
    member: Member,
    Query(query): Query<ReceivedQuery>,
) -> Result<Json<Vec<ReceivedRecordView>>, ApiError> {
    require_org_manager(&member)?;
    let before = before_of(&query)?;
    let limit = query.limit.unwrap_or(100).clamp(1, 1_000);
    let rows = sqlx::query!(
        r#"SELECT id, endpoint, received_at, received_by, content_type, bytes,
                  length(body)::bigint AS "stored_bytes!", content_kept, delete_after
           FROM received_records
           WHERE organization_id = $1 AND ($2::timestamptz IS NULL OR received_at < $2)
           ORDER BY received_at DESC, id DESC LIMIT $3"#,
        member.organization_id,
        before,
        limit
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|row| ReceivedRecordView {
                id: row.id,
                endpoint: row.endpoint,
                received_at: row.received_at,
                received_by: row.received_by,
                content_type: row.content_type,
                bytes: row.bytes,
                stored_bytes: row.stored_bytes,
                content_kept: row.content_kept,
                delete_after: row.delete_after,
            })
            .collect(),
    ))
}

/// How many received records a delete removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
pub struct ReceivedDeleted {
    pub deleted: u64,
}

/// Delete one received record for good. The messages and activities read
/// from it stay. Org managers only.
#[utoipa::path(
    delete,
    path = "/received-records/{id}",
    tag = "received records",
    params(("id" = String, Path, description = "The record's id, such as rcv_…")),
    responses(
        (status = OK, body = ReceivedDeleted),
        (status = NOT_FOUND, body = ErrorBody),
        (status = FORBIDDEN, body = ErrorBody),
    ),
)]
pub(super) async fn delete_received(
    State(state): State<AppState>,
    member: Member,
    Path(id): Path<String>,
) -> Result<Json<ReceivedDeleted>, ApiError> {
    require_org_manager(&member)?;
    let deleted = sqlx::query!(
        "DELETE FROM received_records WHERE organization_id = $1 AND id = $2",
        member.organization_id,
        id
    )
    .execute(&state.db)
    .await?
    .rows_affected();
    if deleted == 0 {
        return Err(ApiError::NotFound);
    }
    record_deletion(&state, &member);
    Ok(Json(ReceivedDeleted { deleted }))
}

/// Delete for good every received record before a moment or day, or all
/// of them without `before`. The messages and activities read from them
/// stay. Org managers only.
#[utoipa::path(
    delete,
    path = "/received-records",
    tag = "received records",
    params(ReceivedQuery),
    responses(
        (status = OK, body = ReceivedDeleted),
        (status = FORBIDDEN, body = ErrorBody),
    ),
)]
pub(super) async fn delete_received_before(
    State(state): State<AppState>,
    member: Member,
    Query(query): Query<ReceivedQuery>,
) -> Result<(StatusCode, Json<ReceivedDeleted>), ApiError> {
    require_org_manager(&member)?;
    let before = before_of(&query)?;
    let deleted = sqlx::query!(
        "DELETE FROM received_records WHERE organization_id = $1 AND ($2::timestamptz IS NULL OR received_at < $2)",
        member.organization_id,
        before
    )
    .execute(&state.db)
    .await?
    .rows_affected();
    record_deletion(&state, &member);
    Ok((StatusCode::OK, Json(ReceivedDeleted { deleted })))
}

fn record_deletion(state: &AppState, member: &Member) {
    crate::organizations::record_audit_event(
        state,
        member.organization_id,
        Some(member.person_id),
        "received_records.deleted",
        None,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_are_named_as_stored() {
        let names: Vec<&str> = [
            Endpoint::TokenEntries,
            Endpoint::TokenEntriesCsv,
            Endpoint::OtlpLogs,
            Endpoint::OtlpTraces,
            Endpoint::OtlpMetrics,
        ]
        .iter()
        .map(|endpoint| endpoint.as_str())
        .collect();
        assert_eq!(
            names,
            vec![
                "token-entries",
                "token-entries-csv",
                "otlp-logs",
                "otlp-traces",
                "otlp-metrics"
            ]
        );
    }

    #[test]
    fn before_reads_a_moment_or_a_day() {
        let query = |before: &str| ReceivedQuery {
            before: Some(before.to_owned()),
            limit: None,
        };
        assert_eq!(
            before_of(&query("2026-11-01")).unwrap(),
            Some(time::macros::datetime!(2026-11-01 00:00 UTC))
        );
        assert_eq!(
            before_of(&query("2026-11-01T10:30:00Z")).unwrap(),
            Some(time::macros::datetime!(2026-11-01 10:30 UTC))
        );
        assert!(before_of(&query("soon")).is_err());
        assert_eq!(before_of(&ReceivedQuery::default()).unwrap(), None);
    }
}
