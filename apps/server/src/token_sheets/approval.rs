//! Approval and reject (docs: Account > Approval). With approval on, a
//! people manager approves submitted token entries for the filters they are
//! assigned, one by one or in bulk after filtering, and a period cannot
//! close while one is unapproved. With approval on or off, a people manager
//! can reject a token entry back to its agent manager with a reason.

use std::collections::BTreeSet;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::AppState;
use crate::close::period::{Period, PeriodDates};
use crate::error::ApiError;
use crate::organizations::Role;
use crate::reports::entries::{self, EntryView};
use crate::reports::filter::Filter;
use crate::reports::lines::{self, CostLine, EntryStatus, LineQuery};
use crate::reports::scope::Scope;
use crate::sign_in::Caller;

pub(super) fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(approval_queue))
        .routes(routes!(approve))
        .routes(routes!(reject))
}

/// Which token entries to show or approve.
#[derive(Debug, Clone, Default, Deserialize, IntoParams, ToSchema)]
#[into_params(parameter_in = Query)]
pub struct ApprovalQuery {
    /// The period, such as `2026-09`. Left out: every open period.
    #[param(value_type = Option<String>, example = "2026-09")]
    #[schema(value_type = Option<String>, example = "2026-09")]
    pub period: Option<Period>,
    /// A tag filter in the URL form, such as `team:web,client:acme`.
    #[param(example = "team:web")]
    #[schema(example = "team:web")]
    pub filter: Option<String>,
    /// `csv` for a CSV file.
    #[serde(default)]
    pub format: Option<String>,
}

/// The time range of a period, or of all time.
pub(crate) fn range(period: Option<Period>) -> Result<(OffsetDateTime, OffsetDateTime), ApiError> {
    match period {
        Some(period) => Ok((
            period.starts_at(),
            period
                .ends_at()
                .map_err(|error| ApiError::BadRequest(error.to_string()))?,
        )),
        None => Ok((
            OffsetDateTime::UNIX_EPOCH,
            OffsetDateTime::now_utc()
                .checked_add(time::Duration::days(366))
                .unwrap_or(OffsetDateTime::UNIX_EPOCH),
        )),
    }
}

fn parse_filter(text: Option<&str>) -> Result<Filter, ApiError> {
    Filter::parse(text.unwrap_or("")).map_err(|error| ApiError::BadRequest(error.to_string()))
}

/// The cost lines in `statuses` that the caller may approve or reject.
async fn decidable(
    db: &PgPool,
    caller: &Caller,
    statuses: &[EntryStatus],
    period: Option<Period>,
    filter: Filter,
) -> Result<Vec<CostLine>, ApiError> {
    let scope = Scope::of(db, caller).await?;
    let (from, until) = range(period)?;
    let query = LineQuery {
        organization_id: caller.organization_id,
        from,
        until,
        statuses: statuses
            .iter()
            .map(|status| status.as_str().to_owned())
            .collect(),
        filter,
        with_late: false,
    };
    let lines = lines::fetch(db, &query, &scope).await?;
    // A token entry is decided as a whole: the caller may decide it when
    // they may decide any of its splits.
    let allowed: BTreeSet<Uuid> = lines
        .iter()
        .filter(|line| scope.may_decide(line))
        .map(|line| line.entry_id)
        .collect();
    Ok(lines
        .into_iter()
        .filter(|line| allowed.contains(&line.entry_id))
        .collect())
}

/// Submitted token entries waiting for approval, within the caller's
/// filters.
#[utoipa::path(
    get,
    path = "/token-entries/approval",
    tag = "token_sheets",
    params(ApprovalQuery),
    responses(
        (status = OK, description = "Token entries waiting for approval, as JSON or CSV", content((Vec<EntryView> = "application/json"), (String = "text/csv"))),
        (status = FORBIDDEN, description = "Only people managers approve", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn approval_queue(
    State(state): State<AppState>,
    caller: Caller,
    Query(query): Query<ApprovalQuery>,
) -> Result<Response, ApiError> {
    caller.require_role(Role::PeopleManager)?;
    let filter = parse_filter(query.filter.as_deref())?;
    let lines = decidable(
        &state.db,
        &caller,
        &[EntryStatus::Submitted],
        query.period,
        filter,
    )
    .await?;
    let views = entries::entries(&state.db, lines).await?;
    if query.format.as_deref() == Some("csv") {
        return Ok(entries::to_csv(&views).into_download("approval.csv"));
    }
    Ok(Json(views).into_response())
}

/// Token entries to approve: by ID, or everything matching a filter and
/// period.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
pub struct ApproveInput {
    /// These token entries. Left out: every submitted token entry matching
    /// `period` and `filter` that the caller may approve.
    pub token_entry_ids: Option<Vec<Uuid>>,
    #[schema(value_type = Option<String>, example = "2026-09")]
    pub period: Option<Period>,
    #[schema(example = "team:web")]
    pub filter: Option<String>,
}

/// How many token entries changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct Decided {
    pub approved: usize,
    /// Asked for by ID, but not submitted or not within the caller's
    /// filters.
    pub skipped: usize,
}

/// Approve submitted token entries, one by one or in bulk.
#[utoipa::path(
    post,
    path = "/token-entries/approve",
    tag = "token_sheets",
    request_body = ApproveInput,
    responses(
        (status = OK, description = "Approved", body = Decided),
        (status = CONFLICT, description = "Approval is off", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn approve(
    State(state): State<AppState>,
    caller: Caller,
    Json(input): Json<ApproveInput>,
) -> Result<Json<Decided>, ApiError> {
    caller.require_role(Role::PeopleManager)?;
    caller.require_session()?;
    let approval = sqlx::query_scalar!(
        "SELECT approval FROM organizations WHERE id = $1",
        caller.organization_id
    )
    .fetch_one(&state.db)
    .await?;
    if !approval {
        return Err(ApiError::Conflict(
            "approval is off for this organization: submitted token entries count as they are"
                .to_owned(),
        ));
    }
    let filter = parse_filter(input.filter.as_deref())?;
    let lines = decidable(
        &state.db,
        &caller,
        &[EntryStatus::Submitted],
        input.period,
        filter,
    )
    .await?;
    let allowed: BTreeSet<Uuid> = lines.iter().map(|line| line.entry_id).collect();
    let (ids, skipped): (Vec<Uuid>, usize) = match input.token_entry_ids {
        Some(asked) => {
            let asked: BTreeSet<Uuid> = asked.into_iter().collect();
            let ids: Vec<Uuid> = asked.intersection(&allowed).copied().collect();
            let skipped = asked.len().saturating_sub(ids.len());
            (ids, skipped)
        }
        None => (allowed.into_iter().collect(), 0),
    };
    let mut tx = state.db.begin().await?;
    let changed: Vec<Uuid> = sqlx::query_scalar!(
        r#"UPDATE token_entries SET status = 'approved'
           WHERE id = ANY($1) AND organization_id = $2 AND status = 'submitted'
           RETURNING id"#,
        &ids,
        caller.organization_id,
    )
    .fetch_all(&mut *tx)
    .await?;
    sqlx::query!(
        r#"INSERT INTO token_entry_decisions (token_entry_id, decision, decided_by)
           SELECT id, 'approved', $2 FROM unnest($1::uuid[]) AS id"#,
        &changed,
        caller.person_id,
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(Decided {
        approved: changed.len(),
        skipped: skipped.saturating_add(ids.len().saturating_sub(changed.len())),
    }))
}

/// Why a token entry goes back.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct RejectInput {
    #[schema(example = "This was the Globex pitch, not ACME: please move it to GLOBEX-7")]
    pub reason: String,
}

/// Send a token entry back to its agent manager with a reason. It does not
/// count until it is fixed and submitted again.
#[utoipa::path(
    post,
    path = "/token-entries/{id}/reject",
    tag = "token_sheets",
    params(("id" = Uuid, Path, description = "The token entry")),
    request_body = RejectInput,
    responses(
        (status = OK, description = "Rejected", body = EntryView),
        (status = BAD_REQUEST, description = "A reason is needed", body = crate::error::ErrorBody),
        (status = CONFLICT, description = "The token entry is in a closed period", body = crate::error::ErrorBody),
        (status = NOT_FOUND, description = "No such token entry within your filters", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn reject(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<Uuid>,
    Json(input): Json<RejectInput>,
) -> Result<Json<EntryView>, ApiError> {
    caller.require_role(Role::PeopleManager)?;
    caller.require_session()?;
    let reason = input.reason.trim();
    if reason.is_empty() {
        return Err(ApiError::BadRequest(
            "say why the token entry goes back".to_owned(),
        ));
    }
    let status = sqlx::query_scalar!(
        "SELECT status FROM token_entries WHERE id = $1 AND organization_id = $2",
        id,
        caller.organization_id,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;
    if status == "locked" {
        return Err(ApiError::Conflict(
            "the token entry is in a closed period; a controller can post an adjustment instead"
                .to_owned(),
        ));
    }
    let lines = decidable(
        &state.db,
        &caller,
        &[EntryStatus::Submitted, EntryStatus::Approved],
        None,
        Filter::default(),
    )
    .await?;
    if !lines.iter().any(|line| line.entry_id == id) {
        return Err(ApiError::NotFound);
    }
    let mut tx = state.db.begin().await?;
    let changed = sqlx::query!(
        r#"UPDATE token_entries SET status = 'rejected'
           WHERE id = $1 AND status IN ('submitted', 'approved')"#,
        id,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if changed == 0 {
        return Err(ApiError::Conflict(
            "the token entry changed meanwhile; look again".to_owned(),
        ));
    }
    sqlx::query!(
        r#"INSERT INTO token_entry_decisions (token_entry_id, decision, reason, decided_by)
           VALUES ($1, 'rejected', $2, $3)"#,
        id,
        reason,
        caller.person_id,
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    let query = LineQuery {
        organization_id: caller.organization_id,
        from: OffsetDateTime::UNIX_EPOCH,
        until: range(None)?.1,
        statuses: vec!["rejected".to_owned()],
        filter: Filter::default(),
        with_late: true,
    };
    let lines: Vec<CostLine> = lines::fetch(&state.db, &query, &Scope::everything())
        .await?
        .into_iter()
        .filter(|line| line.entry_id == id)
        .collect();
    entries::entries(&state.db, lines)
        .await?
        .into_iter()
        .next()
        .map(Json)
        .ok_or(ApiError::NotFound)
}
