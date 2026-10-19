//! Tags and attributes (docs: Account > Tags, Account > Attributes). A tag
//! says what work was for; an attribute says who a person is. Both are
//! names with values, and every value is a record in the log with its key,
//! title, description, source id and default tags; this module keeps the
//! list of names in `tag_names` and writes the value records as events.
//!
//! Owner: the service platform work. Keep this area's code, routes and
//! queries in this folder, and its migrations in `migrations/` named
//! `<timestamp>_tags_<what>.sql`.

pub mod attributes;

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, PgPool};
use time::OffsetDateTime;
use tokencontroller_core::state::State as Fold;
use tokencontroller_core::{
    Capability, Event, Filter, Record, TagName, TagValue, TagValueRecord, Tags,
};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::AppState;
use crate::error::ApiError;
use crate::events::json::{self, TagsSchema};
use crate::events::{self, Log, access, prefix};
use crate::sign_in::Member;

pub use attributes::{AttributeNameView, AttributeValueView, PersonAttributeView};

/// The built-in tag names.
pub const BUILT_IN_TAGS: [&str; 3] = ["project", "feature", "client"];
/// The built-in attribute names.
pub const BUILT_IN_ATTRIBUTES: [&str; 3] = ["team", "department", "location"];

/// Whether a name is a tag name or an attribute name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Tag,
    Attribute,
}

impl Kind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tag => "tag",
            Self::Attribute => "attribute",
        }
    }
}

/// This area's routes, relative to `/api/v1`.
#[must_use]
pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_tags, add_tag_name))
        .routes(routes!(remove_tag_name))
        .routes(routes!(add_tag_value))
        .routes(routes!(
            attributes::list_attributes,
            attributes::add_attribute_name
        ))
        .routes(routes!(attributes::add_attribute_value))
        .routes(routes!(attributes::set_person_attribute))
}

/// A tag name with its values.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TagNameView {
    /// Such as `project`.
    pub name: String,
    /// `project`, `feature` and `client` are built in and stay.
    pub built_in: bool,
    /// The values on record that the caller may view, by key.
    pub values: Vec<TagValueView>,
}

/// One tag value as the API shows it.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TagValueView {
    /// The event id of the record; reverse it to retire the value.
    pub id: String,
    pub name: String,
    /// The short name used in tags, such as `KD-214` or `shop`.
    pub key: String,
    pub title: Option<String>,
    pub description: Option<String>,
    /// The id in the ticket system that owns it, such as a Jira issue id.
    pub source_id: Option<String>,
    /// The status in the ticket system, such as `done`.
    pub source_status: Option<String>,
    /// The tags this value brings with it, such as `{"project": "shop"}`.
    #[schema(value_type = TagsSchema)]
    pub defaults: BTreeMap<String, String>,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub recorded_at: OffsetDateTime,
}

/// A new tag name.
#[derive(Debug, Deserialize, ToSchema)]
pub struct NewName {
    /// Lower-case letters, digits, `-` and `_`, up to 40 characters.
    pub name: String,
}

/// A new tag value, or a change to one: a record with the same name and
/// key supersedes the earlier one.
#[derive(Debug, Deserialize, ToSchema)]
pub struct NewTagValue {
    pub key: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// The tags this value brings, such as `{"client": "acme"}` on a
    /// project. Each must be a value on record.
    #[serde(default)]
    #[schema(value_type = Option<TagsSchema>)]
    pub defaults: Option<BTreeMap<String, String>>,
    /// When the record is dated; default now. Before the cutoff needs
    /// adjust.
    #[serde(default, with = "time::serde::rfc3339::option")]
    #[schema(value_type = Option<String>, format = DateTime)]
    pub dated: Option<OffsetDateTime>,
}

/// `as_of` on a list.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AsOfQuery {
    /// The date up to which the log is read, `YYYY-MM-DD`. Default today.
    pub as_of: Option<String>,
}

/// Adds the built-in tag and attribute names to a new organization.
///
/// # Errors
///
/// Fails if a query fails.
pub async fn create_defaults(
    conn: &mut PgConnection,
    organization_id: Uuid,
) -> Result<(), sqlx::Error> {
    for (names, kind) in [
        (&BUILT_IN_TAGS, Kind::Tag),
        (&BUILT_IN_ATTRIBUTES, Kind::Attribute),
    ] {
        sqlx::query!(
            r#"INSERT INTO tag_names (organization_id, name, built_in, kind)
               SELECT $1, name, true, $3 FROM unnest($2::text[]) AS name
               ON CONFLICT DO NOTHING"#,
            organization_id,
            &names.map(str::to_owned),
            kind.as_str()
        )
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// The names of one kind on the organization's list, built-in first.
///
/// # Errors
///
/// Fails if a query fails.
pub async fn names(
    db: &PgPool,
    organization_id: Uuid,
    kind: Kind,
) -> Result<Vec<(String, bool)>, ApiError> {
    let rows = sqlx::query!(
        r#"SELECT name, built_in FROM tag_names
           WHERE organization_id = $1 AND kind = $2
           ORDER BY built_in DESC, name"#,
        organization_id,
        kind.as_str()
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| (row.name, row.built_in))
        .collect())
}

/// The kind of a name on the list, if it is on it.
///
/// # Errors
///
/// Fails if a query fails.
pub async fn kind_of(
    db: &PgPool,
    organization_id: Uuid,
    name: &str,
) -> Result<Option<Kind>, ApiError> {
    let kind = sqlx::query_scalar!(
        "SELECT kind FROM tag_names WHERE organization_id = $1 AND name = $2",
        organization_id,
        name
    )
    .fetch_optional(db)
    .await?;
    Ok(kind.map(|kind| {
        if kind == "attribute" {
            Kind::Attribute
        } else {
            Kind::Tag
        }
    }))
}

/// Refuses tags with a name that is on neither list. `person` is always a
/// name.
///
/// # Errors
///
/// [`ApiError::BadRequest`] naming the first unknown name.
pub async fn check_names(db: &PgPool, organization_id: Uuid, tags: &Tags) -> Result<(), ApiError> {
    let known = sqlx::query_scalar!(
        "SELECT name FROM tag_names WHERE organization_id = $1",
        organization_id
    )
    .fetch_all(db)
    .await?;
    for (name, _) in tags.iter() {
        if name.as_str() != "person" && !known.iter().any(|known| known == name.as_str()) {
            return Err(ApiError::BadRequest(format!(
                "{name} is not a tag or attribute name of the organization; an org manager adds names"
            )));
        }
    }
    Ok(())
}

/// The tags among `tags` whose value is not on record, as `name: key`
/// words. Ingest answers them as `unknown_tags`.
#[must_use]
pub fn unknown_values(fold: &Fold, tags: &Tags) -> Vec<String> {
    tags.iter()
        .filter(|(name, key)| {
            name.as_str() != "person"
                && fold.values().tag_value(name, key).is_none()
                && fold.values().attribute_value(name, key).is_none()
        })
        .map(|(name, key)| format!("{name}: {key}"))
        .collect()
}

/// A value's tags closed under its defaults: the filter a grant must cover
/// to add or view it.
#[must_use]
pub fn closed(fold: &Fold, name: &TagName, key: &TagValue, defaults: &Tags) -> Tags {
    let mut tags = defaults.clone();
    tags.insert(name.clone(), key.clone());
    let bound = fold
        .values()
        .tag_values()
        .count()
        .saturating_add(fold.values().attribute_values().count());
    for _ in 0..=bound {
        let mut added = false;
        for (name, key) in tags.clone().iter() {
            let defaults = fold
                .values()
                .tag_value(name, key)
                .map(|record| record.defaults.clone())
                .or_else(|| {
                    fold.values()
                        .attribute_value(name, key)
                        .map(|value| value.defaults.clone())
                });
            for (default_name, default_key) in defaults.iter().flat_map(Tags::iter) {
                if !tags.contains(default_name) {
                    tags.insert(default_name.clone(), default_key.clone());
                    added = true;
                }
            }
        }
        if !added {
            break;
        }
    }
    tags
}

/// Whether the caller may view a value: its closed tags lie inside one of
/// their view grants.
#[must_use]
pub fn may_view(
    fold: &Fold,
    person: &TagValue,
    name: &TagName,
    key: &TagValue,
    defaults: &Tags,
) -> bool {
    access::can_view(fold, person, &closed(fold, name, key, defaults))
}

/// The live tag value event for a name and key, if there is one.
#[must_use]
pub fn live_value_event<'a>(log: &'a Log, name: &TagName, key: &TagValue) -> Option<&'a Event> {
    let reversed = log.reversed();
    let mut superseded = std::collections::BTreeSet::new();
    let mut found: Option<&Event> = None;
    for event in log.events.iter().rev() {
        if reversed.contains(&event.id) || superseded.contains(&event.id) {
            continue;
        }
        if let Some(older) = &event.supersedes {
            superseded.insert(older.clone());
        }
        if let Record::TagValue(record) = &event.record
            && record.name == *name
            && record.key == *key
        {
            found = Some(event);
            break;
        }
    }
    found
}

impl TagValueView {
    fn of(event: &Event, record: &TagValueRecord) -> Self {
        Self {
            id: event.id.to_string(),
            name: record.name.to_string(),
            key: record.key.to_string(),
            title: record.title.clone(),
            description: record.description.clone(),
            source_id: record.source_id.clone(),
            source_status: record.source_status.clone(),
            defaults: json::tags_map(&record.defaults),
            recorded_at: event.recorded_at,
        }
    }
}

/// The tag values in force as of the fold that `person` may view, by
/// name.
#[must_use]
pub fn values_view(log: &Log, fold: &Fold, person: &TagValue) -> Vec<TagValueView> {
    let mut views = Vec::new();
    for record in fold.values().tag_values() {
        if !may_view(fold, person, &record.name, &record.key, &record.defaults) {
            continue;
        }
        if let Some(event) = live_value_event(log, &record.name, &record.key) {
            views.push(TagValueView::of(event, record));
        }
    }
    views
}

/// Tag names with their values. Values are those the caller may view.
#[utoipa::path(
    get,
    path = "/tags",
    tag = "tags",
    params(AsOfQuery),
    responses((status = OK, body = Vec<TagNameView>)),
)]
async fn list_tags(
    State(state): State<AppState>,
    member: Member,
    Query(query): Query<AsOfQuery>,
) -> Result<Json<Vec<TagNameView>>, ApiError> {
    let log = Log::load(&state, member.organization_id).await?;
    let fold = log.fold(crate::dates::as_of(query.as_of.as_deref())?)?;
    let person = member.person()?;
    let values = values_view(&log, &fold, &person);
    let names = names(&state.db, member.organization_id, Kind::Tag).await?;
    Ok(Json(
        names
            .into_iter()
            .map(|(name, built_in)| TagNameView {
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

/// Checks a new name.
///
/// # Errors
///
/// [`ApiError::BadRequest`] when it breaks the rules.
pub fn check_name(name: &str) -> Result<String, ApiError> {
    let name = json::tag_name(name)?;
    if name.as_str().len() > 40 {
        return Err(ApiError::BadRequest(
            "a name has at most 40 characters".to_owned(),
        ));
    }
    if name.as_str() == "person" {
        return Err(ApiError::BadRequest(
            "person is the name of the person on every message; it is not added".to_owned(),
        ));
    }
    Ok(name.to_string())
}

/// Adds a name of `kind` to the organization's list. Org managers only.
pub(crate) async fn add_name(
    state: &AppState,
    member: &Member,
    name: &str,
    kind: Kind,
) -> Result<StatusCode, ApiError> {
    member.require_session()?;
    let log = Log::load(state, member.organization_id).await?;
    let fold = log.fold(OffsetDateTime::now_utc())?;
    access::require_org_manager(&fold, &member.person()?)?;
    let name = check_name(name)?;
    let existing = kind_of(&state.db, member.organization_id, &name).await?;
    match existing {
        Some(existing) if existing == kind => return Ok(StatusCode::OK),
        Some(existing) => {
            return Err(ApiError::Conflict(format!(
                "{name} is already {} name",
                match existing {
                    Kind::Tag => "a tag",
                    Kind::Attribute => "an attribute",
                }
            )));
        }
        None => {}
    }
    sqlx::query!(
        "INSERT INTO tag_names (organization_id, name, kind) VALUES ($1, $2, $3)",
        member.organization_id,
        name,
        kind.as_str()
    )
    .execute(&state.db)
    .await?;
    crate::organizations::record_audit_event(
        state,
        member.organization_id,
        Some(member.person_id),
        "tag_name.added",
        None,
    );
    Ok(StatusCode::CREATED)
}

/// Add a tag name, such as `purpose`. Org managers only.
#[utoipa::path(
    post,
    path = "/tags",
    tag = "tags",
    request_body = NewName,
    responses(
        (status = CREATED, description = "Added"),
        (status = OK, description = "Already on the list"),
        (status = BAD_REQUEST, body = crate::error::ErrorBody),
        (status = FORBIDDEN, body = crate::error::ErrorBody),
    ),
)]
async fn add_tag_name(
    State(state): State<AppState>,
    member: Member,
    Json(request): Json<NewName>,
) -> Result<StatusCode, ApiError> {
    add_name(&state, &member, &request.name, Kind::Tag).await
}

/// Remove a tag name you added. The built-in names stay; values on record
/// stay in the log. Org managers only.
#[utoipa::path(
    delete,
    path = "/tags/{name}",
    tag = "tags",
    params(("name" = String, Path)),
    responses(
        (status = NO_CONTENT, description = "Removed"),
        (status = CONFLICT, description = "A built-in name", body = crate::error::ErrorBody),
        (status = NOT_FOUND, body = crate::error::ErrorBody),
    ),
)]
async fn remove_tag_name(
    State(state): State<AppState>,
    member: Member,
    Path(name): Path<String>,
) -> Result<StatusCode, ApiError> {
    member.require_session()?;
    let log = Log::load(&state, member.organization_id).await?;
    let fold = log.fold(OffsetDateTime::now_utc())?;
    access::require_org_manager(&fold, &member.person()?)?;
    let row = sqlx::query!(
        "SELECT built_in FROM tag_names WHERE organization_id = $1 AND name = $2",
        member.organization_id,
        name
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;
    if row.built_in {
        return Err(ApiError::Conflict(format!("{name} is built in and stays")));
    }
    sqlx::query!(
        "DELETE FROM tag_names WHERE organization_id = $1 AND name = $2",
        member.organization_id,
        name
    )
    .execute(&state.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Writes a tag value record, superseding the live record with the same
/// name and key. The caller checks access. Returns the event written.
///
/// # Errors
///
/// [`ApiError::BadRequest`] when the log would not fold, or a database
/// error.
pub async fn write_value(
    state: &AppState,
    log: &Log,
    record: TagValueRecord,
    by: Option<&TagValue>,
    at: OffsetDateTime,
) -> Result<Event, ApiError> {
    let older = live_value_event(log, &record.name, &record.key);
    let at = older.map_or(at, |older| Log::after(older, at));
    let mut event = Event::new(
        &events::new_id(prefix::TAG_VALUE),
        at,
        Record::TagValue(record),
    )
    .map_err(|error| ApiError::Internal(error.into()))?;
    if let Some(older) = older {
        event = event.superseding(older.id.clone());
    }
    if let Some(by) = by {
        event = event.by(by.clone());
    }
    events::write(state, log, std::slice::from_ref(&event)).await?;
    Ok(event)
}

/// Checks a value's defaults: known names, values on record, and never
/// the value's own name.
///
/// # Errors
///
/// [`ApiError::BadRequest`] naming what is wrong.
pub async fn check_defaults(
    state: &AppState,
    fold: &Fold,
    organization_id: Uuid,
    name: &TagName,
    defaults: Option<&BTreeMap<String, String>>,
) -> Result<Tags, ApiError> {
    let defaults = json::tags(defaults.unwrap_or(&BTreeMap::new()))?;
    check_names(&state.db, organization_id, &defaults).await?;
    if defaults.contains(name) {
        return Err(ApiError::BadRequest(format!(
            "a {name} cannot bring a {name} by default"
        )));
    }
    let unknown = unknown_values(fold, &defaults);
    if let Some(first) = unknown.first() {
        return Err(ApiError::BadRequest(format!(
            "{first} is not a value on record; add it first"
        )));
    }
    Ok(defaults)
}

/// Add a tag value, or change one: the same key again supersedes the
/// earlier record. Needs assign on the value's tags with their defaults,
/// such as `project: shop, client: acme`; an org manager holds it
/// everywhere.
#[utoipa::path(
    post,
    path = "/tags/{name}/values",
    tag = "tags",
    params(("name" = String, Path, description = "The tag name, such as `feature`")),
    request_body = NewTagValue,
    responses(
        (status = CREATED, body = TagValueView),
        (status = BAD_REQUEST, body = crate::error::ErrorBody),
        (status = FORBIDDEN, body = crate::error::ErrorBody),
        (status = NOT_FOUND, description = "No such tag name", body = crate::error::ErrorBody),
    ),
)]
async fn add_tag_value(
    State(state): State<AppState>,
    member: Member,
    Path(name): Path<String>,
    Json(request): Json<NewTagValue>,
) -> Result<(StatusCode, Json<TagValueView>), ApiError> {
    member.require_session()?;
    if kind_of(&state.db, member.organization_id, &name).await? != Some(Kind::Tag) {
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
    let at = access::dated(&fold, &person, request.dated)?;
    let earlier = fold.values().tag_value(&name, &key);
    let record = TagValueRecord {
        name,
        key,
        title: request
            .title
            .map(|title| title.trim().to_owned())
            .filter(|title| !title.is_empty()),
        description: request.description.filter(|text| !text.trim().is_empty()),
        source_id: earlier.and_then(|earlier| earlier.source_id.clone()),
        source_status: earlier.and_then(|earlier| earlier.source_status.clone()),
        defaults,
    };
    let event = write_value(&state, &log, record.clone(), Some(&person), at).await?;
    crate::organizations::record_audit_event(
        &state,
        member.organization_id,
        Some(member.person_id),
        "tag_value.recorded",
        None,
    );
    Ok((StatusCode::CREATED, Json(TagValueView::of(&event, &record))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;
    use tokencontroller_core::{Currency, Role};

    fn value(name: &str, key: &str, defaults: &[&str]) -> TagValueRecord {
        TagValueRecord {
            name: TagName::new(name).unwrap(),
            key: TagValue::new(key).unwrap(),
            title: None,
            description: None,
            source_id: None,
            source_status: None,
            defaults: Tags::parse(defaults.iter().copied()).unwrap(),
        }
    }

    fn log() -> Log {
        let at = datetime!(2026-03-01 00:00 UTC);
        let mut events = vec![
            Event::new("tag_1", at, Record::TagValue(value("client", "acme", &[]))).unwrap(),
            Event::new(
                "tag_2",
                at,
                Record::TagValue(value("project", "shop", &["client=acme"])),
            )
            .unwrap(),
            Event::new(
                "tag_3",
                at,
                Record::TagValue(value("feature", "KD-1", &["project=shop"])),
            )
            .unwrap(),
            Event::new(
                "tag_4",
                at,
                Record::TagValue(value("project", "other", &[])),
            )
            .unwrap(),
        ];
        let priya = TagValue::new("priya@kestrel.example").unwrap();
        let filter = Filter(Tags::parse(["client=acme"]).unwrap());
        for (index, grant) in Role::PeopleManager
            .grants(&priya, std::slice::from_ref(&filter))
            .into_iter()
            .enumerate()
        {
            events.push(Event::new(&format!("grt_{index}"), at, Record::Grant(grant)).unwrap());
        }
        Log {
            organization_id: Uuid::nil(),
            events,
        }
    }

    #[test]
    fn a_value_closes_under_its_defaults() {
        let log = log();
        let fold = Fold::fold(&log.events, datetime!(2026-04-01 00:00 UTC), Currency::USD).unwrap();
        let closed = closed(
            &fold,
            &TagName::feature(),
            &TagValue::new("KD-1").unwrap(),
            &Tags::parse(["project=shop"]).unwrap(),
        );
        assert_eq!(
            closed,
            Tags::parse(["feature=KD-1", "project=shop", "client=acme"]).unwrap()
        );
        let priya = TagValue::new("priya@kestrel.example").unwrap();
        let names: Vec<String> = values_view(&log, &fold, &priya)
            .into_iter()
            .map(|view| view.key)
            .collect();
        assert_eq!(names, vec!["acme", "KD-1", "shop"]);
        assert_eq!(
            unknown_values(
                &fold,
                &Tags::parse(["feature=KD-9", "client=acme"]).unwrap()
            ),
            vec!["feature: KD-9"]
        );
    }

    #[test]
    fn the_live_record_of_a_key_is_the_last_not_reversed() {
        let mut log = log();
        let shop = TagValue::new("shop").unwrap();
        let first = live_value_event(&log, &TagName::project(), &shop).unwrap();
        assert_eq!(first.id.as_str(), "tag_2");
        let later = Event::new(
            "tag_5",
            datetime!(2026-03-02 00:00 UTC),
            Record::TagValue(value("project", "shop", &[])),
        )
        .unwrap()
        .superseding(first.id.clone());
        log.events.push(later);
        assert_eq!(
            live_value_event(&log, &TagName::project(), &shop)
                .unwrap()
                .id
                .as_str(),
            "tag_5"
        );
        log.events.push(
            Event::new(
                "rev_1",
                datetime!(2026-03-03 00:00 UTC),
                Record::Reversal(tokencontroller_core::EventId::new("tag_5").unwrap()),
            )
            .unwrap(),
        );
        assert_eq!(
            live_value_event(&log, &TagName::project(), &shop)
                .unwrap()
                .id
                .as_str(),
            "tag_2"
        );
        assert!(log.check_with(&[]).is_ok());
    }

    #[test]
    fn names_follow_the_rules() {
        assert_eq!(check_name(" purpose ").unwrap(), "purpose");
        assert!(check_name("Purpose").is_err());
        assert!(check_name("person").is_err());
        assert!(check_name(&"a".repeat(41)).is_err());
        assert_eq!(Kind::Attribute.as_str(), "attribute");
    }
}
