//! The review list in the web app (docs: Attribute > Review and submit).
//! Drafts live on the person's machine until they are submitted, so the
//! service's list holds what came back: token entries a people manager
//! rejected, with the reason. Beside it are the person's token sheets.

use axum::Json;
use axum::extract::State;
use serde::Serialize;
use time::{Date, OffsetDateTime};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::late::{SheetContext, context_for};
use crate::AppState;
use crate::error::ApiError;
use crate::reports::entries::{self, EntryView};
use crate::reports::filter::Filter;
use crate::reports::lines::{self, LineQuery};
use crate::reports::scope::Scope;
use crate::sign_in::Caller;

pub(super) fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(review))
}

/// One of the person's token sheets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct SheetSummary {
    #[schema(value_type = String, format = Date)]
    pub period_start: Date,
    #[schema(value_type = String, format = Date)]
    pub period_end: Date,
    pub token_entries: i64,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub last_submitted_at: OffsetDateTime,
}

/// The caller's review list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct Review {
    /// Sent back with a reason: fix and submit again.
    pub rejected: Vec<EntryView>,
    pub context: SheetContext,
    /// The person's token sheets, newest first.
    pub token_sheets: Vec<SheetSummary>,
}

/// The caller's own review list: rejected token entries and token sheets.
#[utoipa::path(
    get,
    path = "/token-sheets/review",
    tag = "token_sheets",
    responses((status = OK, description = "The review list", body = Review)),
)]
pub(crate) async fn review(State(state): State<AppState>, caller: Caller) -> Result<Json<Review>, ApiError> {
    let own = Scope::of_person(caller.person_id, Vec::new());
    let query = LineQuery {
        organization_id: caller.organization_id,
        from: OffsetDateTime::UNIX_EPOCH,
        until: OffsetDateTime::now_utc()
            .checked_add(time::Duration::days(366))
            .unwrap_or(OffsetDateTime::UNIX_EPOCH),
        statuses: vec!["rejected".to_owned()],
        filter: Filter::default(),
        with_late: true,
    };
    let lines = lines::fetch(&state.db, &query, &own).await?;
    let rejected = entries::entries(&state.db, lines).await?;
    let context = context_for(&state.db, caller.organization_id, caller.person_id).await?;
    let token_sheets = sqlx::query!(
        r#"SELECT ts.starts_on, ts.ends_on, ts.last_submitted_at,
                  (SELECT count(*) FROM token_sheet_entries tse WHERE tse.token_sheet_id = ts.id) AS "entries!"
           FROM token_sheets ts
           WHERE ts.organization_id = $1 AND ts.person_id = $2
           ORDER BY ts.starts_on DESC
           LIMIT 52"#,
        caller.organization_id,
        caller.person_id,
    )
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .map(|row| SheetSummary {
        period_start: row.starts_on,
        period_end: row.ends_on,
        token_entries: row.entries,
        last_submitted_at: row.last_submitted_at,
    })
    .collect();
    Ok(Json(Review {
        rejected,
        context,
        token_sheets,
    }))
}
