//! `GET /events`, `POST /reversals` and `GET /exports/raw`.

use std::collections::BTreeMap;
use std::io::Write as _;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tokencontroller_core::state::State as Fold;
use tokencontroller_core::{Capability, EventId, Filter, Record, SplitTarget, TagValue};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::json::EventView;
use super::{Log, access, prefix};
use crate::AppState;
use crate::error::{ApiError, ErrorBody};
use crate::sign_in::{Caller, CredentialKind, Member};

pub(super) fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_events))
        .routes(routes!(reverse))
        .routes(routes!(raw_export))
}

/// What to list.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct EventsQuery {
    /// The date up to which the log is read, `YYYY-MM-DD`. Default today.
    pub as_of: Option<String>,
    /// One kind only, such as `split` or `tag value`.
    pub kind: Option<String>,
    /// At most this many events, the latest last. Default 500.
    pub limit: Option<usize>,
}

/// Whether the caller may see an event: their own, or one whose figures
/// lie inside what they may view. Org managers see all.
fn visible(fold: &Fold, person: &TagValue, event: &tokencontroller_core::Event) -> bool {
    if access::is_org_manager(fold, person) || event.recorded_by.as_ref() == Some(person) {
        return true;
    }
    match &event.record {
        Record::Message(message) => {
            message.person == *person
                || fold
                    .lines()
                    .iter()
                    .filter(|line| {
                        line.session_id == message.session_id
                            && line.message_id == message.message_id
                    })
                    .any(|line| access::can_view(fold, person, &line.tags))
        }
        Record::Split(split) => fold
            .lines()
            .iter()
            .filter(|line| match &split.target {
                SplitTarget::Session(session) => line.session_id == *session,
                SplitTarget::Message {
                    session_id,
                    message_id,
                } => line.session_id == *session_id && line.message_id == *message_id,
            })
            .any(|line| line.person == *person || access::can_view(fold, person, &line.tags)),
        Record::Estimate(estimate) => {
            fold.can(person, Capability::View, &Filter(estimate.tags.clone()))
        }
        Record::Share(share) => share.person == *person,
        Record::PersonAttribute(attribute) => attribute.person == *person,
        Record::Grant(grant) => grant.person == *person,
        Record::Submit(submit) => submit.person == *person,
        Record::Rule(rule) => {
            matches!(&rule.scope, tokencontroller_core::Scope::Person(p) if p == person)
                || matches!(rule.scope, tokencontroller_core::Scope::Organization)
        }
        Record::TagValue(_)
        | Record::AttributeValue(_)
        | Record::PriceRow(_)
        | Record::Cutoff(_) => true,
        Record::Bill(_) | Record::Accept(_) | Record::Approval(_) | Record::Reversal(_) => {
            access::can_view(fold, person, &tokencontroller_core::Tags::new())
        }
    }
}

/// The log itself, in order, inside what the caller may view.
#[utoipa::path(
    get,
    path = "/events",
    tag = "events",
    params(EventsQuery),
    responses((status = OK, body = Vec<EventView>)),
)]
async fn list_events(
    State(state): State<AppState>,
    member: Member,
    Query(query): Query<EventsQuery>,
) -> Result<Json<Vec<EventView>>, ApiError> {
    let as_of = crate::dates::as_of(query.as_of.as_deref())?;
    let log = Log::load(&state, member.organization_id).await?;
    let fold = log.fold(as_of)?;
    let person = member.person()?;
    let limit = query.limit.unwrap_or(500).clamp(1, 10_000);
    let kind = query.kind.as_deref().map(str::trim);
    let mut shown: Vec<EventView> = log
        .events
        .iter()
        .filter(|event| event.recorded_at <= as_of)
        .filter(|event| kind.is_none_or(|kind| event.record.kind() == kind))
        .filter(|event| visible(&fold, &person, event))
        .map(EventView::of)
        .collect();
    let skip = shown.len().saturating_sub(limit);
    shown.drain(..skip);
    Ok(Json(shown))
}

/// Reverse an event.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ReversalRequest {
    /// The id of the event to reverse.
    pub event_id: String,
    /// When the reversal is dated. Default now. A date before the cutoff
    /// needs adjust.
    #[serde(default, with = "time::serde::rfc3339::option")]
    #[schema(value_type = Option<String>, format = DateTime)]
    pub dated: Option<OffsetDateTime>,
}

/// The reversal that was recorded.
#[derive(Debug, Serialize, ToSchema)]
pub struct Reversed {
    /// The reversal's own event id.
    pub id: String,
    /// The event it reverses.
    pub event_id: String,
}

/// The capability and filter it takes to reverse someone else's event.
fn reversal_needs(
    fold: &Fold,
    event: &tokencontroller_core::Event,
) -> Option<(Capability, Vec<Filter>)> {
    let line_filters = |keep: &dyn Fn(&tokencontroller_core::state::Line) -> bool| {
        fold.lines()
            .iter()
            .filter(|line| keep(line))
            .map(|line| Filter(line.tags.clone()))
            .collect::<Vec<_>>()
    };
    Some(match &event.record {
        Record::Message(message) => (
            Capability::Assign,
            line_filters(&|line| {
                line.session_id == message.session_id && line.message_id == message.message_id
            }),
        ),
        Record::Split(split) => (
            Capability::Assign,
            line_filters(&|line| match &split.target {
                SplitTarget::Session(session) => line.session_id == *session,
                SplitTarget::Message {
                    session_id,
                    message_id,
                } => line.session_id == *session_id && line.message_id == *message_id,
            }),
        ),
        Record::Estimate(estimate) => (Capability::Plan, vec![Filter(estimate.tags.clone())]),
        Record::Accept(accept) => (
            Capability::Plan,
            fold.estimates()
                .find(|estimate| estimate.id == accept.estimate)
                .map(|estimate| Filter(estimate.value.tags.clone()))
                .into_iter()
                .collect(),
        ),
        Record::Share(share) => {
            let mut tags = tokencontroller_core::Tags::new();
            tags.insert(
                tokencontroller_core::TagName::project(),
                share.project.clone(),
            );
            (Capability::Plan, vec![Filter(tags)])
        }
        Record::Grant(grant) => (Capability::Grant, vec![grant.filter.clone()]),
        Record::Approval(approval) => (
            Capability::Approve,
            fold.submissions()
                .iter()
                .filter(|submission| submission.id == approval.submission)
                .flat_map(|submission| {
                    fold.lines()
                        .iter()
                        .filter(|line| submission.covers(&line.person, line.at))
                        .map(|line| Filter(line.tags.clone()))
                        .collect::<Vec<_>>()
                })
                .collect(),
        ),
        Record::Bill(_)
        | Record::TagValue(_)
        | Record::PriceRow(_)
        | Record::AttributeValue(_)
        | Record::PersonAttribute(_)
        | Record::Cutoff(_)
        | Record::Submit(_)
        | Record::Rule(_)
        | Record::Reversal(_) => return None,
    })
}

/// Whether the person may reverse an event: the one who recorded it, an org
/// manager, or someone holding the capability the event itself needed on
/// its filters.
///
/// # Errors
///
/// [`ApiError::NoCapability`].
pub(crate) fn require_reversal(
    fold: &Fold,
    person: &TagValue,
    event: &tokencontroller_core::Event,
) -> Result<(), ApiError> {
    if event.recorded_by.as_ref() == Some(person) || access::is_org_manager(fold, person) {
        return Ok(());
    }
    if let Some((capability, filters)) = reversal_needs(fold, event)
        && !filters.is_empty()
        && filters
            .iter()
            .all(|filter| fold.can(person, capability, filter))
    {
        return Ok(());
    }
    Err(ApiError::NoCapability(format!(
        "reversing this {} needs the person who recorded it, the capability it took, or an org manager",
        event.record.kind()
    )))
}

/// Reverse any event by id, as if it had never been recorded. The event
/// stays in the log. Whoever recorded it, whoever holds the capability it
/// took on its filter, or an org manager. A saved report is reversed the
/// same way.
#[utoipa::path(
    post,
    path = "/reversals",
    tag = "events",
    request_body = ReversalRequest,
    responses(
        (status = CREATED, body = Reversed),
        (status = CONFLICT, description = "Already reversed", body = ErrorBody),
        (status = FORBIDDEN, body = ErrorBody),
        (status = NOT_FOUND, body = ErrorBody),
    ),
)]
async fn reverse(
    State(state): State<AppState>,
    caller: Caller,
    Json(request): Json<ReversalRequest>,
) -> Result<(StatusCode, Json<Reversed>), ApiError> {
    caller.require_credential(&[CredentialKind::Session, CredentialKind::Service])?;
    let person = caller.person()?;
    let log = Log::load(&state, caller.organization_id).await?;
    let fold = log.fold(OffsetDateTime::now_utc())?;
    let target = request.event_id.trim();
    if target.starts_with(prefix::SAVED_REPORT) {
        crate::reports::reverse_saved(&state, &caller, &fold, target).await?;
        return Ok((
            StatusCode::CREATED,
            Json(Reversed {
                id: target.to_owned(),
                event_id: target.to_owned(),
            }),
        ));
    }
    let event = log.find(target).ok_or(ApiError::NotFound)?;
    if event.id.as_str().starts_with(super::MANAGED_ROW_PREFIX) {
        return Err(ApiError::Conflict(
            "a managed price table row is not reversed; add a row of your own to override it"
                .to_owned(),
        ));
    }
    if log.reversed().contains(&event.id) {
        return Err(ApiError::Conflict(format!("{target} is already reversed")));
    }
    if matches!(event.record, Record::Reversal(_)) {
        return Err(ApiError::Conflict(
            "a reversal is not reversed; record the event again instead".to_owned(),
        ));
    }
    require_reversal(&fold, &person, event)?;
    let dated = access::dated(&fold, &person, request.dated)?;
    let reversal = tokencontroller_core::Event::new(
        &super::new_id(prefix::REVERSAL),
        dated,
        Record::Reversal(
            EventId::new(target).map_err(|error| ApiError::BadRequest(error.to_string()))?,
        ),
    )
    .map_err(|error| ApiError::Internal(error.into()))?
    .by(person);
    super::append_all(
        &state.db,
        caller.organization_id,
        std::slice::from_ref(&reversal),
    )
    .await?;
    crate::organizations::record_audit_event(
        &state,
        caller.organization_id,
        Some(caller.person_id),
        "event.reversed",
        None,
    );
    Ok((
        StatusCode::CREATED,
        Json(Reversed {
            id: reversal.id.to_string(),
            event_id: target.to_owned(),
        }),
    ))
}

/// What the raw export covers.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct RawExportQuery {
    /// The date up to which the log is read, `YYYY-MM-DD`. Default today.
    pub as_of: Option<String>,
}

/// The whole log as a zip of JSON files, one per kind of record, plus the
/// price table rows in force and the grants. Org managers only. Reading it
/// back as of any date gives the same figures the service shows.
#[utoipa::path(
    get,
    path = "/exports/raw",
    tag = "events",
    params(RawExportQuery),
    responses(
        (status = OK, description = "A zip file", content_type = "application/zip"),
        (status = FORBIDDEN, body = ErrorBody),
    ),
)]
async fn raw_export(
    State(state): State<AppState>,
    member: Member,
    Query(query): Query<RawExportQuery>,
) -> Result<Response, ApiError> {
    let as_of = crate::dates::as_of(query.as_of.as_deref())?;
    let log = Log::load(&state, member.organization_id).await?;
    let fold = log.fold(as_of)?;
    let person = member.person()?;
    access::require_org_manager(&fold, &person)?;
    let mut by_kind: BTreeMap<String, Vec<EventView>> = BTreeMap::new();
    for event in log.own().filter(|event| event.recorded_at <= as_of) {
        by_kind
            .entry(event.record.kind().replace(' ', "_"))
            .or_default()
            .push(EventView::of(event));
    }
    let mut files: Vec<(String, serde_json::Value)> = by_kind
        .into_iter()
        .map(|(kind, events)| (format!("events/{kind}.json"), serde_json::json!(events)))
        .collect();
    files.push((
        "price_table.json".to_owned(),
        serde_json::json!(fold.prices().rows().collect::<Vec<_>>()),
    ));
    files.push(("grants.json".to_owned(), serde_json::json!(fold.grants())));
    files.push((
        "about.json".to_owned(),
        serde_json::json!({
            "organization_id": member.organization_id,
            "as_of": as_of.format(&time::format_description::well_known::Rfc3339).unwrap_or_default(),
            "currency": fold.currency().code(),
            "cutoff": fold.cutoff().map(crate::dates::text),
        }),
    ));
    let bytes = zip_of(&files)?;
    Ok((
        StatusCode::OK,
        [
            (CONTENT_TYPE, "application/zip".to_owned()),
            (
                CONTENT_DISPOSITION,
                format!(
                    "attachment; filename=\"token-controller-export-{}.zip\"",
                    crate::dates::text(as_of.date())
                ),
            ),
        ],
        bytes,
    )
        .into_response())
}

fn zip_of(files: &[(String, serde_json::Value)]) -> Result<Vec<u8>, ApiError> {
    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buffer);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, value) in files {
            zip.start_file(name, options)
                .map_err(|error| ApiError::Internal(anyhow::anyhow!("zip: {error}")))?;
            let text = serde_json::to_vec_pretty(value)
                .map_err(|error| ApiError::Internal(anyhow::anyhow!("json: {error}")))?;
            zip.write_all(&text)
                .map_err(|error| ApiError::Internal(anyhow::anyhow!("zip: {error}")))?;
        }
        zip.finish()
            .map_err(|error| ApiError::Internal(anyhow::anyhow!("zip: {error}")))?;
    }
    Ok(buffer.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_export_is_a_zip_with_one_file_per_name() {
        let bytes = zip_of(&[
            ("a.json".to_owned(), serde_json::json!({"a": 1})),
            ("events/split.json".to_owned(), serde_json::json!([])),
        ])
        .unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let names: Vec<String> = (0..archive.len())
            .map(|index| {
                archive
                    .by_index(index)
                    .unwrap()
                    .name()
                    .unwrap()
                    .into_owned()
            })
            .collect();
        assert_eq!(names, vec!["a.json", "events/split.json"]);
    }
}
