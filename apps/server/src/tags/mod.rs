//! Tags with one value per tag name, and the tag map that fills cost center and
//! cost object (docs: Account > Tags and the tag map).
//!
//! Tag values live as JSON objects on the records that carry them, such as
//! `{"team": "web"}` on a person; [`Tags`] is that object. This module keeps
//! the list of tag names and the versions of the tag map. Names and values
//! follow the rules of the core crate's [`tokencontroller_core::Tags`]; use
//! [`to_core`] and [`tag_map_in_force`] to compute with them.
//!
//! Owner: the service platform work. Keep this area's code, routes and queries in
//! this folder, and its migrations in `migrations/` named
//! `<timestamp>_tags_<what>.sql`.

use std::collections::{BTreeMap, BTreeSet};

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, PgPool};
use time::OffsetDateTime;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use tokencontroller_core::Version;
use tokencontroller_core::tags::TagError;

use crate::AppState;
use crate::error::ApiError;
use crate::organizations::{self, Role};
use crate::sign_in::Member;

mod merge;

pub use merge::MergeValues;

/// Tags on one record: tag name to value, one value per tag name.
pub type Tags = BTreeMap<String, String>;

/// The tag names Token Controller keeps records behind.
pub const BUILT_IN: [&str; 3] = ["person", "ticket", "project"];

/// This area's routes, relative to `/api/v1`.
#[must_use]
pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_tag_names, add_tag_name))
        .routes(routes!(remove_tag_name))
        .routes(routes!(tag_values))
        .routes(routes!(merge::merge_values))
        .routes(routes!(get_tag_map, set_tag_map))
}

/// A tag name on the organization's list.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TagName {
    /// Such as `team`.
    pub name: String,
    /// `person`, `ticket` and `project` are built in and cannot be removed.
    pub built_in: bool,
}

/// One version of the tag map (docs: Account > Tags and the tag map).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TagMap {
    /// This version, counted from 1. The highest is in force; a closed
    /// period keeps the version it closed with.
    pub version: i32,
    /// The tag name that fills the cost center. Default `person`.
    pub cost_center: String,
    /// The tag names that fill the cost object, most specific first. The
    /// organization is the last rung and is not listed. Default `ticket`,
    /// `project`.
    pub ladder: Vec<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

/// A change to the tag map.
#[derive(Debug, Deserialize, ToSchema)]
pub struct TagMapChange {
    pub cost_center: String,
    pub ladder: Vec<String>,
}

/// A new tag name.
#[derive(Debug, Deserialize, ToSchema)]
pub struct NewTagName {
    pub name: String,
}

/// One value of a tag name and how many records carry it.
#[derive(Debug, Serialize, ToSchema)]
pub struct TagValue {
    pub value: String,
    /// People, projects and tickets that carry it.
    pub records: i64,
}

/// Adds the built-in tag names and the default tag map to a new
/// organization.
///
/// # Errors
///
/// Fails if a query fails.
pub async fn create_defaults(
    conn: &mut PgConnection,
    organization_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"INSERT INTO tag_names (organization_id, name, built_in)
           SELECT $1, name, true FROM unnest($2::text[]) AS name
           ON CONFLICT DO NOTHING"#,
        organization_id,
        &BUILT_IN.map(str::to_owned)
    )
    .execute(&mut *conn)
    .await?;
    sqlx::query!(
        "INSERT INTO tag_map_versions (organization_id, version) VALUES ($1, 1) ON CONFLICT DO NOTHING",
        organization_id
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// The tag map in force for an organization, as the API shows it.
///
/// # Errors
///
/// Fails if a query fails, or the organization has no tag map.
pub async fn current_tag_map(db: &PgPool, organization_id: Uuid) -> Result<TagMap, sqlx::Error> {
    sqlx::query_as!(
        TagMap,
        r#"SELECT version, cost_center, ladder, created_at FROM tag_map_versions
           WHERE organization_id = $1 ORDER BY version DESC LIMIT 1"#,
        organization_id
    )
    .fetch_one(db)
    .await
}

/// The tag map in force and its version, to compute cost center, cost
/// object and cost kind with. A closed period records the version.
///
/// # Errors
///
/// Fails if a query fails, or the stored map breaks the core rules.
pub async fn tag_map_in_force(
    db: &PgPool,
    organization_id: Uuid,
) -> Result<(Version, tokencontroller_core::TagMap), ApiError> {
    let map = current_tag_map(db, organization_id).await?;
    let version = to_version(map.version)?;
    Ok((version, map.to_core()?))
}

/// One version of the tag map, such as the one a closed period closed with.
///
/// # Errors
///
/// [`ApiError::NotFound`] if the organization has no such version.
pub async fn tag_map_version(
    db: &PgPool,
    organization_id: Uuid,
    version: Version,
) -> Result<tokencontroller_core::TagMap, ApiError> {
    let number = i32::try_from(version.number()).map_err(|_| ApiError::NotFound)?;
    let map = sqlx::query_as!(
        TagMap,
        r#"SELECT version, cost_center, ladder, created_at FROM tag_map_versions
           WHERE organization_id = $1 AND version = $2"#,
        organization_id,
        number
    )
    .fetch_optional(db)
    .await?
    .ok_or(ApiError::NotFound)?;
    map.to_core()
}

fn to_version(number: i32) -> Result<Version, ApiError> {
    u32::try_from(number)
        .ok()
        .and_then(Version::new)
        .ok_or_else(|| {
            ApiError::Internal(anyhow::anyhow!("tag map version {number} is not a version"))
        })
}

impl TagMap {
    /// This version as the core crate computes with it.
    ///
    /// # Errors
    ///
    /// [`ApiError::Internal`] if a stored name breaks the core rules, which
    /// the checks on the way in prevent.
    pub fn to_core(&self) -> Result<tokencontroller_core::TagMap, ApiError> {
        let broken = |error: &dyn std::fmt::Display| {
            ApiError::Internal(anyhow::anyhow!("stored tag map: {error}"))
        };
        let cost_center = core_name(&self.cost_center).map_err(|error| broken(&error))?;
        let ladder = self
            .ladder
            .iter()
            .map(|rung| core_name(rung))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| broken(&error))?;
        tokencontroller_core::TagMap::new(cost_center, ladder).map_err(|error| broken(&error))
    }
}

/// Tags as the core crate computes with them, such as a person's tags
/// under a token entry's split.
///
/// # Errors
///
/// [`TagError`] if a name or value breaks the core rules, which the checks
/// on the way in prevent.
pub fn to_core(tags: &Tags) -> Result<tokencontroller_core::Tags, TagError> {
    tags.iter()
        .map(|(name, value)| {
            Ok((
                core_name(name)?,
                tokencontroller_core::TagValue::new(value)?,
            ))
        })
        .collect()
}

fn core_name(name: &str) -> Result<tokencontroller_core::TagName, TagError> {
    tokencontroller_core::TagName::new(name)
}

/// The organization's tag names.
///
/// # Errors
///
/// Fails if a query fails.
pub async fn tag_names(db: &PgPool, organization_id: Uuid) -> Result<Vec<TagName>, sqlx::Error> {
    sqlx::query_as!(
        TagName,
        r#"SELECT name, built_in FROM tag_names WHERE organization_id = $1
           ORDER BY built_in DESC, name"#,
        organization_id
    )
    .fetch_all(db)
    .await
}

/// Checks tags someone set on a person, project, ticket or filter: every
/// name must be on the organization's list, built-in names only where
/// `allow_built_in` says so, and every value non-empty. Values are trimmed.
///
/// # Errors
///
/// [`ApiError::BadRequest`] naming the first problem.
pub async fn validate(
    db: &PgPool,
    organization_id: Uuid,
    tags: &Tags,
    allow_built_in: bool,
) -> Result<Tags, ApiError> {
    if tags.is_empty() {
        return Ok(Tags::new());
    }
    let known: BTreeSet<String> = tag_names(db, organization_id)
        .await?
        .into_iter()
        .map(|tag| tag.name)
        .collect();
    check_tags(tags, &known, allow_built_in)
}

fn check_tags(
    tags: &Tags,
    known: &BTreeSet<String>,
    allow_built_in: bool,
) -> Result<Tags, ApiError> {
    let mut checked = Tags::new();
    for (name, value) in tags {
        if !known.contains(name) {
            return Err(ApiError::BadRequest(format!(
                "{name:?} is not on the organization's list of tag names. A controller adds it under Tags."
            )));
        }
        if !allow_built_in && BUILT_IN.contains(&name.as_str()) {
            return Err(ApiError::BadRequest(format!(
                "{name:?} is a built-in tag and cannot be set here."
            )));
        }
        let value = tokencontroller_core::TagValue::new(value).map_err(|_| {
            ApiError::BadRequest(format!(
                "The value of {name:?} must have 1 to 200 characters and no line breaks."
            ))
        })?;
        checked.insert(name.clone(), value.into());
    }
    Ok(checked)
}

/// The organization's tag names.
#[utoipa::path(get, path = "/tags", tag = "tags", responses((status = OK, body = Vec<TagName>)))]
async fn list_tag_names(
    State(state): State<AppState>,
    member: Member,
) -> Result<Json<Vec<TagName>>, ApiError> {
    Ok(Json(tag_names(&state.db, member.organization_id).await?))
}

/// Add a tag name, such as `team` or `client`. Controllers only.
#[utoipa::path(
    post,
    path = "/tags",
    tag = "tags",
    request_body = NewTagName,
    responses(
        (status = CREATED, body = TagName),
        (status = BAD_REQUEST, body = crate::error::ErrorBody),
        (status = CONFLICT, body = crate::error::ErrorBody),
    ),
)]
async fn add_tag_name(
    State(state): State<AppState>,
    member: Member,
    Json(request): Json<NewTagName>,
) -> Result<(StatusCode, Json<TagName>), ApiError> {
    member.require_session()?;
    member.require_role(Role::Controller)?;
    let name = request.name.trim().to_lowercase();
    if !valid_tag_name(&name) {
        return Err(ApiError::BadRequest(format!(
            "{name:?} is not a tag name: use 1 to 40 lower-case letters, digits, - or _, starting with a letter, and not `organization`."
        )));
    }
    let added = sqlx::query!(
        "INSERT INTO tag_names (organization_id, name) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        member.organization_id,
        name
    )
    .execute(&state.db)
    .await?;
    if added.rows_affected() == 0 {
        return Err(ApiError::Conflict(format!(
            "The tag name {name:?} is already on the list."
        )));
    }
    organizations::record_audit_event(
        &state,
        member.organization_id,
        Some(member.person_id),
        "tag_name.added",
        None,
    );
    Ok((
        StatusCode::CREATED,
        Json(TagName {
            name,
            built_in: false,
        }),
    ))
}

/// The core rules for a tag name, and not `organization`, which is the
/// ladder's last rung.
fn valid_tag_name(name: &str) -> bool {
    name != "organization" && core_name(name).is_ok()
}

/// Remove a tag name. People, projects, tickets and rules lose its values;
/// token entries keep theirs. Controllers only; not while the tag map uses it.
#[utoipa::path(
    delete,
    path = "/tags/{name}",
    tag = "tags",
    params(("name" = String, Path)),
    responses(
        (status = NO_CONTENT, description = "Removed"),
        (status = CONFLICT, body = crate::error::ErrorBody),
        (status = NOT_FOUND, body = crate::error::ErrorBody),
    ),
)]
async fn remove_tag_name(
    State(state): State<AppState>,
    member: Member,
    Path(name): Path<String>,
) -> Result<StatusCode, ApiError> {
    member.require_session()?;
    member.require_role(Role::Controller)?;
    if BUILT_IN.contains(&name.as_str()) {
        return Err(ApiError::Conflict(format!(
            "{name:?} is built in and cannot be removed."
        )));
    }
    let map = current_tag_map(&state.db, member.organization_id).await?;
    if map.cost_center == name || map.ladder.contains(&name) {
        return Err(ApiError::Conflict(format!(
            "The tag map uses {name:?}. Change the tag map first."
        )));
    }
    let mut tx = state.db.begin().await?;
    let removed = sqlx::query!(
        "DELETE FROM tag_names WHERE organization_id = $1 AND name = $2",
        member.organization_id,
        name
    )
    .execute(&mut *tx)
    .await?;
    if removed.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    sqlx::query!(
        "UPDATE people SET tags = tags - $2 WHERE organization_id = $1 AND tags ? $2",
        member.organization_id,
        name
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        "UPDATE projects SET tags = tags - $2 WHERE organization_id = $1 AND tags ? $2",
        member.organization_id,
        name
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        "UPDATE tickets SET tags = tags - $2 WHERE organization_id = $1 AND tags ? $2",
        member.organization_id,
        name
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        "DELETE FROM people_manager_filters WHERE organization_id = $1 AND filter ? $2",
        member.organization_id,
        name
    )
    .execute(&mut *tx)
    .await?;
    if let Ok(tag_name) = core_name(&name) {
        organizations::change_rule_tags(&mut tx, member.organization_id, |tags| {
            tags.remove(&tag_name);
        })
        .await?;
    }
    tx.commit().await?;
    organizations::record_audit_event(
        &state,
        member.organization_id,
        Some(member.person_id),
        "tag_name.removed",
        None,
    );
    Ok(StatusCode::NO_CONTENT)
}

/// The values of one tag name on people, projects and tickets. For the
/// built-in names: the members' emails, the projects and the ticket keys.
#[utoipa::path(
    get,
    path = "/tags/{name}/values",
    tag = "tags",
    params(("name" = String, Path)),
    responses((status = OK, body = Vec<TagValue>)),
)]
async fn tag_values(
    State(state): State<AppState>,
    member: Member,
    Path(name): Path<String>,
) -> Result<Json<Vec<TagValue>>, ApiError> {
    Ok(Json(
        values_of(&state.db, member.organization_id, &name).await?,
    ))
}

/// The values of one tag name, with how many records carry each.
async fn values_of(
    db: &PgPool,
    organization_id: Uuid,
    name: &str,
) -> Result<Vec<TagValue>, sqlx::Error> {
    sqlx::query_as!(
        TagValue,
        r#"
        WITH v(value) AS (
            SELECT tags ->> $2 FROM people WHERE organization_id = $1 AND removed_at IS NULL AND tags ? $2
            UNION ALL SELECT tags ->> $2 FROM projects WHERE organization_id = $1 AND tags ? $2
            UNION ALL SELECT tags ->> $2 FROM tickets WHERE organization_id = $1 AND tags ? $2
            UNION ALL SELECT email FROM people WHERE $2 = 'person' AND organization_id = $1 AND removed_at IS NULL
            UNION ALL SELECT name FROM projects WHERE $2 = 'project' AND organization_id = $1 AND archived_at IS NULL
            UNION ALL SELECT key FROM tickets WHERE $2 = 'ticket' AND organization_id = $1
        )
        SELECT value AS "value!", count(*) AS "records!" FROM v
        WHERE value IS NOT NULL GROUP BY value ORDER BY value
        "#,
        organization_id,
        name
    )
    .fetch_all(db)
    .await
}

/// The tag map in force.
#[utoipa::path(get, path = "/tag-map", tag = "tags", responses((status = OK, body = TagMap)))]
async fn get_tag_map(
    State(state): State<AppState>,
    member: Member,
) -> Result<Json<TagMap>, ApiError> {
    Ok(Json(
        current_tag_map(&state.db, member.organization_id).await?,
    ))
}

/// Change the tag map. A new version is kept; closed periods keep theirs.
/// Controllers only.
#[utoipa::path(
    put,
    path = "/tag-map",
    tag = "tags",
    request_body = TagMapChange,
    responses(
        (status = OK, body = TagMap),
        (status = BAD_REQUEST, body = crate::error::ErrorBody),
    ),
)]
async fn set_tag_map(
    State(state): State<AppState>,
    member: Member,
    Json(change): Json<TagMapChange>,
) -> Result<Json<TagMap>, ApiError> {
    member.require_session()?;
    member.require_role(Role::Controller)?;
    let known: BTreeSet<String> = tag_names(&state.db, member.organization_id)
        .await?
        .into_iter()
        .map(|tag| tag.name)
        .collect();
    check_tag_map(&change, &known)?;
    let mut tx = state.db.begin().await?;
    // One change at a time per organization, so version numbers never clash.
    sqlx::query!(
        "SELECT id FROM organizations WHERE id = $1 FOR UPDATE",
        member.organization_id
    )
    .fetch_one(&mut *tx)
    .await?;
    let map = sqlx::query_as!(
        TagMap,
        r#"INSERT INTO tag_map_versions (organization_id, version, cost_center, ladder, created_by)
           SELECT $1, coalesce(max(version), 0) + 1, $2, $3, $4 FROM tag_map_versions WHERE organization_id = $1
           RETURNING version, cost_center, ladder, created_at"#,
        member.organization_id,
        change.cost_center,
        &change.ladder,
        member.person_id
    )
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    organizations::record_audit_event(
        &state,
        member.organization_id,
        Some(member.person_id),
        "tag_map.changed",
        None,
    );
    Ok(Json(map))
}

fn check_tag_map(change: &TagMapChange, known: &BTreeSet<String>) -> Result<(), ApiError> {
    if !known.contains(&change.cost_center) {
        return Err(ApiError::BadRequest(format!(
            "The cost center {:?} is not on the organization's list of tag names.",
            change.cost_center
        )));
    }
    for rung in &change.ladder {
        if !known.contains(rung) {
            return Err(ApiError::BadRequest(format!(
                "The rung {rung:?} is not on the organization's list of tag names."
            )));
        }
        if rung == "person" {
            return Err(ApiError::BadRequest(
                "A person answers for cost; it cannot be a rung of the ladder.".to_owned(),
            ));
        }
    }
    let ladder = change
        .ladder
        .iter()
        .map(|rung| core_name(rung))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| ApiError::BadRequest(error.to_string()))?;
    let cost_center =
        core_name(&change.cost_center).map_err(|error| ApiError::BadRequest(error.to_string()))?;
    tokencontroller_core::TagMap::new(cost_center, ladder)
        .map_err(|_| ApiError::BadRequest("A rung is on the ladder twice.".to_owned()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known() -> BTreeSet<String> {
        ["person", "ticket", "project", "team", "client"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    }

    fn tags(pairs: &[(&str, &str)]) -> Tags {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn tags_must_be_on_the_list() {
        let checked = check_tags(&tags(&[("team", " web ")]), &known(), false).unwrap();
        assert_eq!(checked, tags(&[("team", "web")]));
        assert!(check_tags(&tags(&[("guild", "rust")]), &known(), false).is_err());
    }

    #[test]
    fn built_in_tags_only_where_allowed() {
        assert!(check_tags(&tags(&[("project", "storefront")]), &known(), false).is_err());
        assert!(check_tags(&tags(&[("project", "storefront")]), &known(), true).is_ok());
    }

    #[test]
    fn values_need_one_to_two_hundred_characters() {
        assert!(check_tags(&tags(&[("team", "  ")]), &known(), false).is_err());
        let long = "x".repeat(201);
        assert!(check_tags(&tags(&[("team", &long)]), &known(), false).is_err());
        let longest = "x".repeat(200);
        assert!(check_tags(&tags(&[("team", &longest)]), &known(), false).is_ok());
    }

    #[test]
    fn stored_tags_and_maps_turn_into_core_types() {
        let core = to_core(&tags(&[("team", "web")])).unwrap();
        assert_eq!(core.get_str("team").unwrap().as_str(), "web");
        assert!(to_core(&tags(&[("Team", "web")])).is_err());
        let map = TagMap {
            version: 2,
            cost_center: "person".to_owned(),
            ladder: vec!["ticket".to_owned(), "project".to_owned()],
            created_at: OffsetDateTime::UNIX_EPOCH,
        };
        assert_eq!(
            map.to_core().unwrap(),
            tokencontroller_core::TagMap::default()
        );
        assert!(to_version(0).is_err());
        assert_eq!(to_version(2).unwrap().number(), 2);
    }

    #[test]
    fn tag_names_are_simple_lower_case_words() {
        for good in ["team", "cost-center", "guild_2", "a"] {
            assert!(valid_tag_name(good), "{good}");
        }
        for bad in [
            "",
            "Team",
            "2team",
            "team name",
            "organization",
            &"x".repeat(41),
        ] {
            assert!(!valid_tag_name(bad), "{bad}");
        }
    }

    #[test]
    fn the_ladder_uses_known_names_once_and_never_person() {
        let change = |cost_center: &str, ladder: &[&str]| TagMapChange {
            cost_center: cost_center.to_owned(),
            ladder: ladder.iter().map(|rung| (*rung).to_owned()).collect(),
        };
        assert!(check_tag_map(&change("team", &["ticket", "project", "client"]), &known()).is_ok());
        assert!(check_tag_map(&change("person", &[]), &known()).is_ok());
        assert!(check_tag_map(&change("guild", &["ticket"]), &known()).is_err());
        assert!(check_tag_map(&change("person", &["epic"]), &known()).is_err());
        assert!(check_tag_map(&change("person", &["ticket", "ticket"]), &known()).is_err());
        assert!(check_tag_map(&change("team", &["person"]), &known()).is_err());
    }
}
