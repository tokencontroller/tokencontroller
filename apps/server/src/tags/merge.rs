//! Merging tag values (docs: Account > Tags): hand-typed values drift, so
//! `Acme` and `acme-corp` become `acme` on every record that carries them.
//! The token entries of open periods carry tags too; the area that stores
//! them renames their values in [`rename_values`] as well.

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use sqlx::PgConnection;
use utoipa::ToSchema;
use uuid::Uuid;

use super::{BUILT_IN, TagValue, tag_names, values_of};
use crate::AppState;
use crate::error::ApiError;
use crate::organizations::{self, Role};
use crate::sign_in::Member;

/// The most values one merge takes.
const MAX_FROM: usize = 100;

/// Values of one tag name to merge into one.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MergeValues {
    /// The values to replace, such as `Acme` and `acme-corp`.
    pub from: Vec<String>,
    /// The value they become, such as `acme`. It may be one of them, or new.
    pub into: String,
}

/// Merge values of a tag name into one, on people, projects, tickets,
/// people managers' filters and rules. Controllers only. The built-in names
/// cannot be merged: their values are members, projects and tickets.
#[utoipa::path(
    post,
    path = "/tags/{name}/merge",
    tag = "tags",
    params(("name" = String, Path)),
    request_body = MergeValues,
    responses(
        (status = OK, description = "The tag name's values after the merge", body = Vec<TagValue>),
        (status = BAD_REQUEST, body = crate::error::ErrorBody),
        (status = NOT_FOUND, body = crate::error::ErrorBody),
    ),
)]
pub(super) async fn merge_values(
    State(state): State<AppState>,
    member: Member,
    Path(name): Path<String>,
    Json(request): Json<MergeValues>,
) -> Result<Json<Vec<TagValue>>, ApiError> {
    member.require_session()?;
    member.require_role(Role::Controller)?;
    if BUILT_IN.contains(&name.as_str()) {
        return Err(ApiError::BadRequest(format!(
            "{name:?} is built in: its values are members, projects and tickets, which cannot be merged."
        )));
    }
    let on_list = tag_names(&state.db, member.organization_id)
        .await?
        .iter()
        .any(|tag| tag.name == name);
    if !on_list {
        return Err(ApiError::NotFound);
    }
    if request.from.is_empty() || request.from.len() > MAX_FROM {
        return Err(ApiError::BadRequest(format!(
            "Name 1 to {MAX_FROM} values to merge."
        )));
    }
    let tag_name = super::core_name(&name).map_err(|_| ApiError::NotFound)?;
    let into = tokencontroller_core::TagValue::new(&request.into).map_err(|_| {
        ApiError::BadRequest(
            "The merged value must have 1 to 200 characters and no line breaks.".to_owned(),
        )
    })?;
    let from = request.from;

    let mut tx = state.db.begin().await?;
    rename_values(&mut tx, member.organization_id, &name, &from, into.as_str()).await?;
    organizations::change_rule_tags(&mut tx, member.organization_id, |tags| {
        if tags
            .get(&tag_name)
            .is_some_and(|value| from.iter().any(|old| old == value.as_str()))
        {
            tags.insert(tag_name.clone(), into.clone());
        }
    })
    .await?;
    tx.commit().await?;
    organizations::record_audit_event(
        &state,
        member.organization_id,
        Some(member.person_id),
        "tag_values.merged",
        None,
    );
    Ok(Json(
        values_of(&state.db, member.organization_id, &name).await?,
    ))
}

/// Renames the values `from` of the tag name `name` to `into` on people,
/// projects, tickets and people managers' filters.
async fn rename_values(
    conn: &mut PgConnection,
    organization_id: Uuid,
    name: &str,
    from: &[String],
    into: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"UPDATE people SET tags = jsonb_set(tags, ARRAY[$2], to_jsonb($3::text))
           WHERE organization_id = $1 AND tags ->> $2 = ANY($4)"#,
        organization_id,
        name,
        into,
        from
    )
    .execute(&mut *conn)
    .await?;
    sqlx::query!(
        r#"UPDATE projects SET tags = jsonb_set(tags, ARRAY[$2], to_jsonb($3::text))
           WHERE organization_id = $1 AND tags ->> $2 = ANY($4)"#,
        organization_id,
        name,
        into,
        from
    )
    .execute(&mut *conn)
    .await?;
    sqlx::query!(
        r#"UPDATE tickets SET tags = jsonb_set(tags, ARRAY[$2], to_jsonb($3::text))
           WHERE organization_id = $1 AND tags ->> $2 = ANY($4)"#,
        organization_id,
        name,
        into,
        from
    )
    .execute(&mut *conn)
    .await?;
    // A people manager keeps each filter once: drop the ones the merge
    // makes equal to another of theirs, keeping one.
    sqlx::query!(
        r#"DELETE FROM people_manager_filters f USING people_manager_filters g
           WHERE f.organization_id = $1 AND g.person_id = f.person_id AND g.id <> f.id
             AND f.filter ->> $2 = ANY($4)
             AND jsonb_set(f.filter, ARRAY[$2], to_jsonb($3::text)) =
                 CASE WHEN g.filter ->> $2 = ANY($4)
                      THEN jsonb_set(g.filter, ARRAY[$2], to_jsonb($3::text))
                      ELSE g.filter END
             AND (g.filter ->> $2 IS NULL OR g.filter ->> $2 <> ALL($4) OR g.id < f.id)"#,
        organization_id,
        name,
        into,
        from
    )
    .execute(&mut *conn)
    .await?;
    sqlx::query!(
        r#"UPDATE people_manager_filters SET filter = jsonb_set(filter, ARRAY[$2], to_jsonb($3::text))
           WHERE organization_id = $1 AND filter ->> $2 = ANY($4)"#,
        organization_id,
        name,
        into,
        from
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}
