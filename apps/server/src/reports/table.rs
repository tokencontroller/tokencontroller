//! The token entries table (docs: Report > Exports): every token entry the
//! caller may see, with its splits, tags and summary, as JSON pages or one
//! CSV file. `tc export --csv` reads it.

use axum::Json;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::READERS;
use super::entries::{self, EntryView};
use super::filter::Filter;
use super::lines::{self, EntryStatus, LineQuery};
use super::scope::Scope;
use crate::AppState;
use crate::close::period::Period;
use crate::error::ApiError;
use crate::sign_in::Caller;

/// Token entries per JSON page when the query names no limit.
const DEFAULT_LIMIT: usize = 200;
/// The largest JSON page.
const MAX_LIMIT: usize = 5_000;

pub(super) fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(token_entries))
}

/// Which token entries to list.
#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct TableQuery {
    /// The period, such as `2026-09`. Left out: all time.
    #[param(value_type = Option<String>, example = "2026-09")]
    pub period: Option<Period>,
    /// A tag filter, such as `client:acme,team:web`.
    #[param(example = "client:acme")]
    pub filter: Option<String>,
    /// Statuses, comma separated, out of `submitted`, `approved`,
    /// `rejected` and `locked`. Left out: every status that counts.
    #[param(example = "submitted,approved")]
    pub status: Option<String>,
    /// Only this agent manager's token entries.
    pub person: Option<uuid::Uuid>,
    /// `csv` for a CSV file of every matching token entry, one row per
    /// split.
    pub format: Option<String>,
    /// Token entries per page, up to 5000.
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// One page of token entries, oldest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TokenEntriesPage {
    pub token_entries: Vec<EntryView>,
    /// Matching token entries on all pages.
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
}

/// Reads `submitted,approved` into statuses. Drafts stay their agent
/// manager's own until submitted, so the table never lists them.
fn statuses(text: Option<&str>) -> Result<Vec<String>, ApiError> {
    let Some(text) = text.map(str::trim).filter(|text| !text.is_empty()) else {
        return Ok([
            EntryStatus::Submitted,
            EntryStatus::Approved,
            EntryStatus::Locked,
        ]
        .map(|status| status.as_str().to_owned())
        .to_vec());
    };
    text.split(',')
        .map(str::trim)
        .map(|part| match EntryStatus::parse(part) {
            Some(EntryStatus::Draft) | None => Err(ApiError::BadRequest(format!(
                "{part:?} is not a status: use submitted, approved, rejected or locked"
            ))),
            Some(status) => Ok(status.as_str().to_owned()),
        })
        .collect()
}

/// Token entries with their splits, tags and summaries, within the caller's
/// role.
#[utoipa::path(
    get,
    path = "/reports/token-entries",
    tag = "reports",
    params(TableQuery),
    responses(
        (status = OK, description = "Token entries, as a JSON page or one CSV file", content((TokenEntriesPage = "application/json"), (String = "text/csv"))),
        (status = BAD_REQUEST, description = "The filter or a status cannot be read", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn token_entries(
    State(state): State<AppState>,
    caller: Caller,
    Query(query): Query<TableQuery>,
) -> Result<Response, ApiError> {
    caller.require_credential(READERS)?;
    let filter = Filter::parse(query.filter.as_deref().unwrap_or(""))
        .map_err(|error| ApiError::BadRequest(error.to_string()))?;
    let (from, until) = crate::token_sheets::approval::range(query.period)?;
    let line_query = LineQuery {
        organization_id: caller.organization_id,
        from,
        until,
        statuses: statuses(query.status.as_deref())?,
        filter,
        with_late: true,
    };
    let scope = Scope::of(&state.db, &caller).await?;
    let lines = lines::fetch(&state.db, &line_query, &scope)
        .await?
        .into_iter()
        .filter(|line| {
            query
                .person
                .is_none_or(|person| line.agent_manager_id == Some(person))
        })
        .collect();
    let views = entries::entries(&state.db, lines).await?;
    if query.format.as_deref() == Some("csv") {
        let filename = query.period.map_or_else(
            || "token-entries.csv".to_owned(),
            |period| format!("token-entries-{period}.csv"),
        );
        return Ok(entries::to_csv(&views).into_download(&filename));
    }
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let offset = query.offset.unwrap_or(0);
    let total = views.len();
    Ok(Json(TokenEntriesPage {
        token_entries: views.into_iter().skip(offset).take(limit).collect(),
        total,
        limit,
        offset,
    })
    .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_default_to_the_ones_that_count() {
        assert_eq!(statuses(None).unwrap(), ["submitted", "approved", "locked"]);
        assert_eq!(
            statuses(Some(" ")).unwrap(),
            ["submitted", "approved", "locked"]
        );
        assert_eq!(
            statuses(Some("rejected, locked")).unwrap(),
            ["rejected", "locked"]
        );
        assert!(statuses(Some("draft")).is_err());
        assert!(statuses(Some("posted")).is_err());
    }
}
