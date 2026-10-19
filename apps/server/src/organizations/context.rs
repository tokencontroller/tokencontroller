//! `GET /me/context`: what the CLI needs to split sessions like the
//! service does, so a split shown on the machine is the split the fold
//! gives.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::State;
use serde::Serialize;
use time::OffsetDateTime;
use utoipa::ToSchema;

use super::rules::{self, RuleView};
use crate::AppState;
use crate::error::ApiError;
use crate::events::Log;
use crate::events::json::TagsSchema;
use crate::price_table::PriceRowView;
use crate::sign_in::Member;
use crate::tags::{self, AttributeValueView, TagValueView};

/// A tag or attribute name on the organization's list.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct NameView {
    pub name: String,
    /// `tag` or `attribute`.
    pub kind: tags::Kind,
    pub built_in: bool,
}

/// What a local tool needs to split sessions like the service does.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Context {
    pub organization: super::OrganizationSummary,
    /// You, as the tag value `person`.
    pub person: String,
    pub reporting_currency: String,
    /// The cutoff, `YYYY-MM-DD`, if one was set.
    pub cutoff: Option<String>,
    /// The organization's rules and your own, in the order they run.
    pub rules: Vec<RuleView>,
    pub names: Vec<NameView>,
    /// The tag values you may view.
    pub tag_values: Vec<TagValueView>,
    pub attribute_values: Vec<AttributeValueView>,
    /// Your attributes today.
    #[schema(value_type = TagsSchema)]
    pub attributes: BTreeMap<String, String>,
    /// The price table rows in force.
    pub price_table: Vec<PriceRowView>,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub as_of: OffsetDateTime,
}

/// What the CLI needs to split sessions like the service does: the
/// organization's rules, the tag names and the values you may see, your
/// own rules and attributes, and the price table.
#[utoipa::path(
    get,
    path = "/me/context",
    tag = "sign-in",
    responses((status = OK, body = Context)),
)]
pub(super) async fn context(
    State(state): State<AppState>,
    member: Member,
) -> Result<Json<Context>, ApiError> {
    let log = Log::load(&state, member.organization_id).await?;
    let now = OffsetDateTime::now_utc();
    let fold = log.fold(now)?;
    let person = member.person()?;
    let mut names = Vec::new();
    for (kind, list) in [
        (
            tags::Kind::Tag,
            tags::names(&state.db, member.organization_id, tags::Kind::Tag).await?,
        ),
        (
            tags::Kind::Attribute,
            tags::names(&state.db, member.organization_id, tags::Kind::Attribute).await?,
        ),
    ] {
        names.extend(list.into_iter().map(|(name, built_in)| NameView {
            name,
            kind,
            built_in,
        }));
    }
    let organization = super::organization(&state.db, member.organization_id).await?;
    Ok(Json(Context {
        organization: super::summary(&state.db, member.organization_id).await?,
        person: person.to_string(),
        reporting_currency: organization.reporting_currency,
        cutoff: fold.cutoff().map(crate::dates::text),
        rules: rules::views(&fold, Some(&person)),
        names,
        tag_values: tags::values_view(&log, &fold, &person),
        attribute_values: tags::attributes::values_view(&log, &fold),
        attributes: tags::attributes::attributes_of(
            &fold,
            &person,
            tokencontroller_core::price::date_of(now),
        ),
        price_table: crate::price_table::rows_view(&log, &fold),
        as_of: fold.as_of(),
    }))
}
