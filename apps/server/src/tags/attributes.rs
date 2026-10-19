//! Attributes on people (docs: Account > Attributes): names such as
//! `team`, values such as `web` with their default attributes, and a
//! person's attribute from a date.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use time::{Date, OffsetDateTime};
use tokencontroller_core::state::State as Fold;
use tokencontroller_core::{
    AttributeValue, Capability, Event, Filter, PersonAttribute, Record, TagValue,
};
use utoipa::ToSchema;
use uuid::Uuid;

use super::{AsOfQuery, Kind, NewName, check_defaults, closed, kind_of};
use crate::AppState;
use crate::error::ApiError;
use crate::events::json::{self, TagsSchema};
use crate::events::{self, Log, access, prefix};
use crate::sign_in::Member;

/// An attribute name with its values.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AttributeNameView {
    /// Such as `team`.
    pub name: String,
    /// `team`, `department` and `location` are built in and stay.
    pub built_in: bool,
    pub values: Vec<AttributeValueView>,
}

/// One attribute value.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AttributeValueView {
    /// The event id of the record.
    pub id: String,
    pub name: String,
    pub key: String,
    pub title: Option<String>,
    /// The attributes this value brings, such as `{"department": "engineering"}` on a team.
    #[schema(value_type = TagsSchema)]
    pub defaults: BTreeMap<String, String>,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub recorded_at: OffsetDateTime,
}

/// A new attribute value, or a change to one.
#[derive(Debug, Deserialize, ToSchema)]
pub struct NewAttributeValue {
    pub key: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    #[schema(value_type = Option<TagsSchema>)]
    pub defaults: Option<BTreeMap<String, String>>,
}

/// An attribute on a person from a date.
#[derive(Debug, Deserialize, ToSchema)]
pub struct NewPersonAttribute {
    /// The attribute name, such as `team`.
    pub name: String,
    /// The value's key, such as `web`.
    pub key: String,
    /// The first day it holds, `YYYY-MM-DD`. Left out, it holds for every
    /// day.
    #[serde(default)]
    pub from: Option<String>,
}

/// One attribute of a person.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PersonAttributeView {
    pub id: String,
    pub person: String,
    pub name: String,
    pub key: String,
    /// The first day it holds, or `null` for always.
    #[serde(with = "crate::dates::option_day")]
    #[schema(value_type = Option<String>, format = Date)]
    pub from: Option<Date>,
}

/// The attribute values in force as of the fold.
#[must_use]
pub fn values_view(log: &Log, fold: &Fold) -> Vec<AttributeValueView> {
    let mut views = Vec::new();
    for value in fold.values().attribute_values() {
        let event = log.events.iter().rev().find(|event| {
            matches!(&event.record, Record::AttributeValue(record) if record.name == value.name && record.key == value.key)
                && log.is_live(&event.id)
        });
        if let Some(event) = event {
            views.push(AttributeValueView {
                id: event.id.to_string(),
                name: value.name.to_string(),
                key: value.key.to_string(),
                title: value.title.clone(),
                defaults: json::tags_map(&value.defaults),
                recorded_at: event.recorded_at,
            });
        }
    }
    views
}

/// Attribute names with their values.
#[utoipa::path(
    get,
    path = "/attributes",
    tag = "attributes",
    params(AsOfQuery),
    responses((status = OK, body = Vec<AttributeNameView>)),
)]
pub(super) async fn list_attributes(
    State(state): State<AppState>,
    member: Member,
    Query(query): Query<AsOfQuery>,
) -> Result<Json<Vec<AttributeNameView>>, ApiError> {
    let log = Log::load(&state, member.organization_id).await?;
    let fold = log.fold(crate::dates::as_of(query.as_of.as_deref())?)?;
    let values = values_view(&log, &fold);
    let names = super::names(&state.db, member.organization_id, Kind::Attribute).await?;
    Ok(Json(
        names
            .into_iter()
            .map(|(name, built_in)| AttributeNameView {
                values: values
                    .iter()
                    .filter(|value| value.name == name)
                    .cloned()
                    .collect(),
                name,
                built_in,
            })
            .collect(),
    ))
}

/// Add an attribute name, such as `guild`. Org managers only.
#[utoipa::path(
    post,
    path = "/attributes",
    tag = "attributes",
    request_body = NewName,
    responses(
        (status = CREATED, description = "Added"),
        (status = OK, description = "Already on the list"),
        (status = BAD_REQUEST, body = crate::error::ErrorBody),
        (status = FORBIDDEN, body = crate::error::ErrorBody),
    ),
)]
pub(super) async fn add_attribute_name(
    State(state): State<AppState>,
    member: Member,
    Json(request): Json<NewName>,
) -> Result<StatusCode, ApiError> {
    super::add_name(&state, &member, &request.name, Kind::Attribute).await
}

/// Add an attribute value, or change one: the same key again supersedes
/// the earlier record. Needs assign on the value with its defaults; an
/// org manager holds it everywhere.
#[utoipa::path(
    post,
    path = "/attributes/{name}/values",
    tag = "attributes",
    params(("name" = String, Path, description = "The attribute name, such as `team`")),
    request_body = NewAttributeValue,
    responses(
        (status = CREATED, body = AttributeValueView),
        (status = BAD_REQUEST, body = crate::error::ErrorBody),
        (status = FORBIDDEN, body = crate::error::ErrorBody),
        (status = NOT_FOUND, description = "No such attribute name", body = crate::error::ErrorBody),
    ),
)]
pub(super) async fn add_attribute_value(
    State(state): State<AppState>,
    member: Member,
    Path(name): Path<String>,
    Json(request): Json<NewAttributeValue>,
) -> Result<(StatusCode, Json<AttributeValueView>), ApiError> {
    member.require_session()?;
    if kind_of(&state.db, member.organization_id, &name).await? != Some(Kind::Attribute) {
        return Err(ApiError::NotFound);
    }
    let name = json::tag_name(&name)?;
    let key = json::tag_value(name.as_str(), request.key.trim())?;
    let log = Log::load(&state, member.organization_id).await?;
    let fold = log.fold(OffsetDateTime::now_utc())?;
    let person = member.person()?;
    let defaults = check_defaults(
        &state,
        &fold,
        member.organization_id,
        &name,
        request.defaults.as_ref(),
    )
    .await?;
    let filter = Filter(closed(&fold, &name, &key, &defaults));
    access::require(&fold, &person, Capability::Assign, &filter)?;
    let value = AttributeValue {
        name,
        key,
        title: request
            .title
            .map(|title| title.trim().to_owned())
            .filter(|title| !title.is_empty()),
        defaults,
    };
    let older = log.events.iter().rev().find(|event| {
        matches!(&event.record, Record::AttributeValue(record) if record.name == value.name && record.key == value.key)
            && log.is_live(&event.id)
    });
    let now = OffsetDateTime::now_utc();
    let at = older.map_or(now, |older| Log::after(older, now));
    let mut event = Event::new(
        &events::new_id(prefix::ATTRIBUTE_VALUE),
        at,
        Record::AttributeValue(value.clone()),
    )
    .map_err(|error| ApiError::Internal(error.into()))?
    .by(person);
    if let Some(older) = older {
        event = event.superseding(older.id.clone());
    }
    events::write(&state, &log, std::slice::from_ref(&event)).await?;
    Ok((
        StatusCode::CREATED,
        Json(AttributeValueView {
            id: event.id.to_string(),
            name: value.name.to_string(),
            key: value.key.to_string(),
            title: value.title,
            defaults: json::tags_map(&value.defaults),
            recorded_at: event.recorded_at,
        }),
    ))
}

/// Put an attribute on a person from a date, such as `team: web` from
/// 1 April. The person keeps the earlier value for earlier days. Needs
/// assign on the value with its defaults.
#[utoipa::path(
    post,
    path = "/people/{id}/attributes",
    tag = "attributes",
    params(("id" = Uuid, Path, description = "The member's id")),
    request_body = NewPersonAttribute,
    responses(
        (status = CREATED, body = PersonAttributeView),
        (status = BAD_REQUEST, body = crate::error::ErrorBody),
        (status = FORBIDDEN, body = crate::error::ErrorBody),
        (status = NOT_FOUND, body = crate::error::ErrorBody),
    ),
)]
pub(super) async fn set_person_attribute(
    State(state): State<AppState>,
    member: Member,
    Path(id): Path<Uuid>,
    Json(request): Json<NewPersonAttribute>,
) -> Result<(StatusCode, Json<PersonAttributeView>), ApiError> {
    member.require_session()?;
    let target = crate::organizations::member(&state, member.organization_id, id).await?;
    let name = json::tag_name(&request.name)?;
    if kind_of(&state.db, member.organization_id, name.as_str()).await? != Some(Kind::Attribute) {
        return Err(ApiError::BadRequest(format!(
            "{name} is not an attribute name of the organization"
        )));
    }
    let key = json::tag_value(name.as_str(), request.key.trim())?;
    let from = crate::dates::parse_optional(request.from.as_deref())?;
    let log = Log::load(&state, member.organization_id).await?;
    let fold = log.fold(OffsetDateTime::now_utc())?;
    let Some(value) = fold.values().attribute_value(&name, &key) else {
        return Err(ApiError::BadRequest(format!(
            "{name}: {key} is not a value on record; add it first"
        )));
    };
    let caller = member.person()?;
    let filter = Filter(closed(&fold, &name, &key, &value.defaults));
    access::require(&fold, &caller, Capability::Assign, &filter)?;
    let person = events::person(&target.email)?;
    let event = events::event(Record::PersonAttribute(PersonAttribute {
        person: person.clone(),
        name: name.clone(),
        key: key.clone(),
        from,
    }))?
    .by(caller);
    events::write(&state, &log, std::slice::from_ref(&event)).await?;
    crate::organizations::record_audit_event(
        &state,
        member.organization_id,
        Some(member.person_id),
        "member.attribute_set",
        Some(id),
    );
    Ok((
        StatusCode::CREATED,
        Json(PersonAttributeView {
            id: event.id.to_string(),
            person: person.to_string(),
            name: name.to_string(),
            key: key.to_string(),
            from,
        }),
    ))
}

/// A person's attributes as of a day, as `name: key` pairs.
#[must_use]
pub fn attributes_of(fold: &Fold, person: &TagValue, day: Date) -> BTreeMap<String, String> {
    json::tags_map(&fold.values().attributes_of_on(person, day))
}
