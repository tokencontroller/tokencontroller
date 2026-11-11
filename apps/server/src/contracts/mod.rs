//! Contracts (docs: Bills, data model "Which contract"): the contracts an
//! organization has, and setting one by hand on the sessions the review
//! list asks about.
//!
//! A contract comes from a bill, a tag value of `contract` or a rule that
//! creates it; the fold knows them all. Setting one by hand writes one
//! contract assignment per session, which wins over every rule.

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tokencontroller_core::state::State as Fold;
use tokencontroller_core::{ContractAssignment, Record, SessionId, SplitTarget, TagName};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::AppState;
use crate::error::ApiError;
use crate::events::{self, Log, json};
use crate::sign_in::Member;
use crate::tags::ContractTermsView;

/// This area's routes, relative to `/api/v1`.
#[must_use]
pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_contracts))
        .routes(routes!(assign_contract))
}

/// One contract as the API shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ContractView {
    /// The contract's key, a value of the tag name `contract`.
    #[schema(example = "anthropic-api")]
    pub key: String,
    /// The title of its tag value, when it has one.
    pub title: Option<String>,
    #[serde(flatten)]
    pub terms: ContractTermsView,
}

/// `as_of` on the list.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ContractsQuery {
    /// The moment to look back to; default now.
    pub as_of: Option<String>,
}

/// The contracts of the fold, by key.
#[must_use]
pub fn views(fold: &Fold) -> Vec<ContractView> {
    fold.contracts()
        .iter()
        .map(|(key, terms)| ContractView {
            key: key.to_string(),
            title: fold
                .values()
                .tag_value(&TagName::contract(), key)
                .and_then(|record| record.title.clone()),
            terms: ContractTermsView::of(terms),
        })
        .collect()
}

/// Every contract of the organization: from its bills, its tag values of
/// `contract` and the rules that created one.
#[utoipa::path(
    get,
    path = "/contracts",
    tag = "bills",
    params(ContractsQuery),
    responses((status = OK, body = Vec<ContractView>)),
)]
async fn list_contracts(
    State(state): State<AppState>,
    member: Member,
    Query(query): Query<ContractsQuery>,
) -> Result<Json<Vec<ContractView>>, ApiError> {
    let log = Log::load(&state, member.organization_id).await?;
    let fold = log.fold(crate::dates::as_of(query.as_of.as_deref())?)?;
    Ok(Json(views(&fold)))
}

/// One contract set by hand on sessions.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct NewContractAssignment {
    /// The contract's key. It must be a contract the organization has.
    #[schema(example = "anthropic-api")]
    pub contract: String,
    /// The sessions it goes on, every message of each. Your own, or with
    /// assign on a filter each session is in.
    pub sessions: Vec<String>,
}

/// What was written.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ContractAssignmentResult {
    pub contract: String,
    /// The sessions that now carry it.
    pub sessions: usize,
}

/// Set a contract by hand on whole sessions: the answer to "which
/// contract?" in the review list. It wins over every rule, and may cover
/// messages before the cutoff, since figures as of the cutoff stay as they
/// were. A later one on the same session supersedes it.
#[utoipa::path(
    post,
    path = "/contract-assignments",
    tag = "splits",
    request_body = NewContractAssignment,
    responses(
        (status = CREATED, body = ContractAssignmentResult),
        (status = BAD_REQUEST, body = crate::error::ErrorBody),
        (status = FORBIDDEN, body = crate::error::ErrorBody),
        (status = NOT_FOUND, description = "No such session", body = crate::error::ErrorBody),
    ),
)]
async fn assign_contract(
    State(state): State<AppState>,
    member: Member,
    Json(request): Json<NewContractAssignment>,
) -> Result<(StatusCode, Json<ContractAssignmentResult>), ApiError> {
    member.require_person()?;
    if request.sessions.is_empty() || request.sessions.len() > 1000 {
        return Err(ApiError::BadRequest("name 1 to 1000 sessions".to_owned()));
    }
    let contract = json::tag_value("contract", request.contract.trim())?;
    let log = Log::load(&state, member.organization_id).await?;
    let fold = log.fold(OffsetDateTime::now_utc())?;
    if !fold.contracts().contains_key(&contract) {
        return Err(ApiError::BadRequest(format!(
            "contract: {contract} is not a contract on record; add it first"
        )));
    }
    let person = member.person()?;
    let mut written = Vec::new();
    for session in &request.sessions {
        let session_id = SessionId::new(session.trim())
            .map_err(|error| ApiError::BadRequest(error.to_string()))?;
        let target = SplitTarget::Session(session_id);
        crate::splits::require_assign(&fold, &person, &target)?;
        let record = Record::ContractAssignment(ContractAssignment {
            target,
            contract: contract.clone(),
        });
        written.push(events::event(record)?.by(person.clone()));
    }
    events::write(&state, &log, &written).await?;
    crate::organizations::record_audit_event(
        &state,
        member.organization_id,
        Some(member.person_id),
        "contract.assigned",
        None,
    );
    Ok((
        StatusCode::CREATED,
        Json(ContractAssignmentResult {
            contract: contract.to_string(),
            sessions: written.len(),
        }),
    ))
}
