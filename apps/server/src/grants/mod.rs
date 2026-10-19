//! Grants (ADR 0023): (person, capability, filter), the assignment access is
//! computed from. A role is a bundle of grants written here when a member
//! is invited or their role or filters change; `POST /grants` hands single
//! capabilities on. A new grant for the same person and capability
//! supersedes the old one; a reversal takes it away.
//!
//! Owner: the service platform work.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use time::OffsetDateTime;
use tokencontroller_core::state::State as Fold;
use tokencontroller_core::{Capability, Event, EventId, Filter, Grant, Record, TagValue, Tags};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::AppState;
use crate::error::{ApiError, ErrorBody};
use crate::events::json::{TagsSchema, tags_map};
use crate::events::{self, Log, access, prefix};
use crate::organizations::Role;
use crate::sign_in::{Caller, CredentialKind, Member};

/// This area's routes, relative to `/api/v1`.
#[must_use]
pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(list_grants, add_grant))
}

/// The id prefix of the grants a role gave a person, so a role change can
/// reverse them: `grt_role_<person id>_`.
#[must_use]
pub fn role_prefix(person_id: Uuid) -> String {
    format!("{}role_{}_", prefix::GRANT, person_id.simple())
}

/// Writes the grants of a role for a person, reversing the grants the
/// earlier role gave. A people manager gets the bundle on each filter.
///
/// # Errors
///
/// Fails if a query fails.
pub async fn set_role_grants(
    conn: &mut PgConnection,
    log: &Log,
    person_id: Uuid,
    person: &TagValue,
    role: Role,
    filters: &[Filter],
    by: Option<&TagValue>,
) -> Result<(), ApiError> {
    let now = OffsetDateTime::now_utc();
    let prefix = role_prefix(person_id);
    for old in log.live_with_prefix(&prefix) {
        let reversal = Event::new(
            &events::new_id(crate::events::prefix::REVERSAL),
            now,
            Record::Reversal(old.id.clone()),
        )
        .map_err(|error| ApiError::Internal(error.into()))?;
        let reversal = match by {
            Some(by) => reversal.by(by.clone()),
            None => reversal,
        };
        events::append(conn, log.organization_id, &reversal).await?;
    }
    for grant in role.core().grants(person, filters) {
        let event = Event::new(
            &format!("{prefix}{}", Uuid::new_v4().simple()),
            now,
            Record::Grant(grant),
        )
        .map_err(|error| ApiError::Internal(error.into()))?;
        let event = match by {
            Some(by) => event.by(by.clone()),
            None => event,
        };
        events::append(conn, log.organization_id, &event).await?;
    }
    Ok(())
}

/// The filters a people manager's role grants were written for: the
/// distinct filters of their live role grants.
#[must_use]
pub fn role_filters(log: &Log, person_id: Uuid) -> Vec<Tags> {
    let mut filters: Vec<Tags> = Vec::new();
    for event in log.live_with_prefix(&role_prefix(person_id)) {
        if let Record::Grant(grant) = &event.record
            && !grant.filter.is_everything()
            && !filters.contains(&grant.filter.0)
        {
            filters.push(grant.filter.0.clone());
        }
    }
    filters
}

/// One grant as the API shows it.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct GrantView {
    /// The grant's event id; reverse it to take the grant away.
    pub id: String,
    /// The person, by email.
    pub person: String,
    #[schema(value_type = String, example = "view")]
    pub capability: Capability,
    /// Empty is the whole organization.
    #[schema(value_type = TagsSchema)]
    pub filter: BTreeMap<String, String>,
    /// Whether a role gave it, rather than `POST /grants`.
    pub from_role: bool,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub recorded_at: OffsetDateTime,
}

/// What to list.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct GrantsQuery {
    /// One person's grants, by email.
    pub person: Option<String>,
    /// `YYYY-MM-DD`; default today.
    pub as_of: Option<String>,
}

/// The grants in force. Everyone sees their own; whoever holds grant on a
/// filter sees the grants inside it; org managers see all.
#[utoipa::path(
    get,
    path = "/grants",
    tag = "grants",
    params(GrantsQuery),
    responses((status = OK, body = Vec<GrantView>)),
)]
async fn list_grants(
    State(state): State<AppState>,
    member: Member,
    Query(query): Query<GrantsQuery>,
) -> Result<Json<Vec<GrantView>>, ApiError> {
    let as_of = crate::dates::as_of(query.as_of.as_deref())?;
    let log = Log::load(&state, member.organization_id).await?;
    let fold = log.fold(as_of)?;
    let me = member.person()?;
    let only = query
        .person
        .as_deref()
        .map(crate::sign_in::normalize_email)
        .transpose()?;
    let reversed = log.reversed();
    let superseded = log.superseded(as_of);
    let shown = log
        .events
        .iter()
        .filter(|event| event.recorded_at <= as_of && !reversed.contains(&event.id))
        .filter(|event| !superseded.contains(&event.id))
        .filter_map(|event| match &event.record {
            Record::Grant(grant) => Some((event, grant)),
            _ => None,
        })
        .filter(|(_, grant)| {
            only.as_deref()
                .is_none_or(|only| grant.person.as_str() == only)
        })
        .filter(|(_, grant)| {
            grant.person == me
                || access::is_org_manager(&fold, &me)
                || fold.can(&me, Capability::Grant, &grant.filter)
        })
        .map(|(event, grant)| GrantView {
            id: event.id.to_string(),
            person: grant.person.to_string(),
            capability: grant.capability,
            filter: tags_map(&grant.filter.0),
            from_role: event
                .id
                .as_str()
                .starts_with(&format!("{}role_", prefix::GRANT)),
            recorded_at: event.recorded_at,
        })
        .collect();
    Ok(Json(shown))
}

/// A grant to hand on.
#[derive(Debug, Deserialize, ToSchema)]
pub struct NewGrant {
    /// The person, by email. A member of the organization.
    pub person: String,
    /// One of `view`, `assign`, `approve`, `plan` or `grant`.
    #[schema(value_type = String, example = "view")]
    pub capability: Capability,
    /// The filter; empty is the whole organization.
    #[serde(default)]
    #[schema(value_type = TagsSchema)]
    pub filter: BTreeMap<String, String>,
}

/// Hand a capability to someone for a filter inside your own: view, assign,
/// approve, plan or grant. A new grant for the same person and capability
/// supersedes the old one.
#[utoipa::path(
    post,
    path = "/grants",
    tag = "grants",
    request_body = NewGrant,
    responses(
        (status = CREATED, body = GrantView),
        (status = BAD_REQUEST, body = ErrorBody),
        (status = FORBIDDEN, body = ErrorBody),
    ),
)]
async fn add_grant(
    State(state): State<AppState>,
    caller: Caller,
    Json(request): Json<NewGrant>,
) -> Result<(StatusCode, Json<GrantView>), ApiError> {
    caller.require_credential(&[CredentialKind::Session])?;
    if !Capability::GRANTABLE.contains(&request.capability) {
        return Err(ApiError::BadRequest(format!(
            "{} cannot be granted; grant one of view, assign, approve, plan or grant",
            access::capability_word(request.capability)
        )));
    }
    let email = crate::sign_in::normalize_email(&request.person)?;
    crate::organizations::member_by_email(&state.db, caller.organization_id, &email)
        .await?
        .ok_or_else(|| ApiError::BadRequest(format!("{email} is not a member")))?;
    let filter = Filter(crate::events::json::tags(&request.filter)?);
    crate::tags::check_names(&state.db, caller.organization_id, &filter.0).await?;
    let log = Log::load(&state, caller.organization_id).await?;
    let fold = log.fold(OffsetDateTime::now_utc())?;
    if let Some(first) = crate::tags::unknown_values(&fold, &filter.0).first() {
        return Err(ApiError::BadRequest(format!(
            "{first} is not a value on record; add it first"
        )));
    }
    let me = caller.person()?;
    access::require(&fold, &me, Capability::Grant, &filter)?;
    let person = events::person(&email)?;
    let grant = Grant {
        person,
        capability: request.capability,
        filter,
    };
    let superseded = earlier_grant(&log, &fold, &grant);
    let mut event = events::event(Record::Grant(grant.clone()))?.by(me);
    if let Some(older) = superseded {
        event = event.superseding(older);
    }
    events::append_all(
        &state.db,
        caller.organization_id,
        std::slice::from_ref(&event),
    )
    .await?;
    crate::organizations::record_audit_event(
        &state,
        caller.organization_id,
        Some(caller.person_id),
        "grant.added",
        None,
    );
    Ok((
        StatusCode::CREATED,
        Json(GrantView {
            id: event.id.to_string(),
            person: grant.person.to_string(),
            capability: grant.capability,
            filter: tags_map(&grant.filter.0),
            from_role: false,
            recorded_at: event.recorded_at,
        }),
    ))
}

/// The live hand-written grant for the same person and capability, which
/// a new one supersedes. Role grants are left alone.
fn earlier_grant(log: &Log, fold: &Fold, grant: &Grant) -> Option<EventId> {
    let reversed = log.reversed();
    let superseded = log.superseded(fold.as_of());
    log.events
        .iter()
        .rev()
        .filter(|event| !reversed.contains(&event.id) && !superseded.contains(&event.id))
        .filter(|event| {
            !event
                .id
                .as_str()
                .starts_with(&format!("{}role_", prefix::GRANT))
        })
        .find(|event| {
            matches!(&event.record, Record::Grant(existing)
                if existing.person == grant.person && existing.capability == grant.capability)
        })
        .map(|event| event.id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_grant_ids_name_their_person() {
        let id = Uuid::nil();
        assert_eq!(
            role_prefix(id),
            "grt_role_00000000000000000000000000000000_"
        );
    }

    #[test]
    fn the_filters_of_a_people_manager_come_from_their_role_grants() {
        let person = TagValue::new("priya@kestrel.example").unwrap();
        let id = Uuid::from_u128(7);
        let web = Filter(Tags::parse(["team=web"]).unwrap());
        let acme = Filter(Tags::parse(["client=acme"]).unwrap());
        let mut events = Vec::new();
        for (index, grant) in Role::PeopleManager
            .core()
            .grants(&person, &[web.clone(), acme.clone()])
            .into_iter()
            .enumerate()
        {
            events.push(
                Event::new(
                    &format!("{}{index}", role_prefix(id)),
                    OffsetDateTime::UNIX_EPOCH,
                    Record::Grant(grant),
                )
                .unwrap(),
            );
        }
        events.push(
            Event::new(
                "grt_hand",
                OffsetDateTime::UNIX_EPOCH,
                Record::Grant(Grant {
                    person: person.clone(),
                    capability: Capability::View,
                    filter: Filter(Tags::parse(["team=data"]).unwrap()),
                }),
            )
            .unwrap(),
        );
        let log = Log {
            organization_id: Uuid::nil(),
            events,
        };
        assert_eq!(role_filters(&log, id), vec![web.0.clone(), acme.0.clone()]);
        assert_eq!(role_filters(&log, Uuid::from_u128(8)), Vec::<Tags>::new());
        let fold = log.fold(OffsetDateTime::UNIX_EPOCH).unwrap();
        let newer = Grant {
            person: person.clone(),
            capability: Capability::View,
            filter: Filter::default(),
        };
        assert_eq!(
            earlier_grant(&log, &fold, &newer).unwrap().as_str(),
            "grt_hand"
        );
        let other = Grant {
            capability: Capability::Approve,
            ..newer
        };
        assert_eq!(earlier_grant(&log, &fold, &other), None);
    }
}
