//! Organization rules and personal rules (docs: Attribute > Rules), and the
//! context a local tool reads before it places token entries: the
//! organization's rules, the person's own rules and tags, and the projects.
//!
//! Rules travel in the core crate's `Rule` shape, so the CLI and the
//! service place token entries the same way (ADR 0010).

use std::collections::BTreeSet;

use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};
use sqlx::types::Json as JsonColumn;
use sqlx::{PgConnection, PgPool};
use time::OffsetDateTime;
use tokencontroller_core::rules::{Action, Rule, SeveralKeys};
use utoipa::ToSchema;
use uuid::Uuid;

use super::rule_schema::{RuleShape, SeveralKeysShape};
use super::{Role, record_audit_event};
use crate::AppState;
use crate::error::ApiError;
use crate::projects;
use crate::sign_in::{CredentialKind, Member};
use crate::tags::{self, Tags};

/// The most rules one list holds.
const MAX_RULES: usize = 100;

/// The longest rule name.
const MAX_NAME_CHARS: usize = 100;

/// The organization's rules and how they place token entries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct OrganizationRules {
    /// In order. The default rules until a controller changes them.
    #[schema(value_type = Vec<RuleShape>)]
    pub rules: Vec<Rule>,
    /// What the "ticket key in prompts" rule does when prompts name several
    /// tickets: `most_often`, `equal_split` or `first_named`.
    #[schema(value_type = SeveralKeysShape)]
    pub several_keys: SeveralKeys,
    /// Whether token entries no rule placed are matched to a ticket by
    /// their summary.
    pub summary_match: bool,
}

/// A change to the organization's rules. Fields left out stay as they are.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct OrganizationRulesChange {
    /// Replaces the whole list, in order. An empty list switches every rule
    /// off.
    #[schema(value_type = Option<Vec<RuleShape>>)]
    pub rules: Option<Vec<Rule>>,
    #[schema(value_type = Option<SeveralKeysShape>)]
    pub several_keys: Option<SeveralKeys>,
    pub summary_match: Option<bool>,
}

/// A person's own rules.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PersonalRules {
    /// In order. They run after the organization's.
    #[schema(value_type = Vec<RuleShape>)]
    pub rules: Vec<Rule>,
}

/// Everything a local tool needs to place token entries the way the service
/// does.
#[derive(Debug, Serialize, ToSchema)]
pub struct Context {
    pub organization: ContextOrganization,
    pub person: ContextPerson,
    /// The person's own rules, in order.
    #[schema(value_type = Vec<RuleShape>)]
    pub rules: Vec<Rule>,
    /// Projects that are not archived.
    pub projects: Vec<ContextProject>,
    /// The person's next token sheet. `null` until the close area reports
    /// it.
    pub cadence: Option<Cadence>,
    /// Token entries sent from this person's machines that the service
    /// turned down. Empty until the close area reports them.
    pub rejected: Vec<Rejected>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ContextOrganization {
    pub name: String,
    /// The reporting currency, such as `EUR`.
    pub currency: String,
    /// The organization's tag names, built-in ones included.
    pub tag_names: Vec<String>,
    #[schema(value_type = SeveralKeysShape)]
    pub several_keys: SeveralKeys,
    /// The organization's rules, in order; they run before the person's.
    #[schema(value_type = Vec<RuleShape>)]
    pub rules: Vec<Rule>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ContextPerson {
    pub email: String,
    pub name: String,
    /// Tags the person's token entries inherit, such as `{"team": "web"}`.
    pub tags: Tags,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ContextProject {
    pub name: String,
    pub ticket_key_pattern: Option<String>,
    pub repositories: Vec<String>,
    pub tags: Tags,
}

/// One token sheet period.
#[derive(Debug, Serialize, ToSchema)]
pub struct Cadence {
    /// The first day, such as `2026-10-05`.
    pub period_start: String,
    /// The last day, such as `2026-10-11`.
    pub period_end: String,
    #[serde(with = "time::serde::rfc3339")]
    pub due_at: OffsetDateTime,
}

/// A token entry the service turned down.
#[derive(Debug, Serialize, ToSchema)]
pub struct Rejected {
    /// The ID the machine gave it, such as `claude-code:<session id>`.
    pub local_id: String,
    pub reason: String,
}

/// The organization's rules, the default ones until a controller changes
/// them.
///
/// # Errors
///
/// [`ApiError::NotFound`] if there is no such organization, or a failed
/// query.
pub async fn organization_rules(
    db: &PgPool,
    organization_id: Uuid,
) -> Result<OrganizationRules, ApiError> {
    let row = sqlx::query!(
        r#"SELECT rules AS "rules: JsonColumn<Vec<Rule>>", several_keys, summary_match
           FROM organizations WHERE id = $1"#,
        organization_id
    )
    .fetch_optional(db)
    .await?
    .ok_or(ApiError::NotFound)?;
    Ok(OrganizationRules {
        rules: row.rules.map_or_else(Rule::defaults, |rules| rules.0),
        several_keys: several_keys_from(&row.several_keys),
        summary_match: row.summary_match,
    })
}

/// A person's own rules, in order.
///
/// # Errors
///
/// [`ApiError::NotFound`] if there is no such person, or a failed query.
pub async fn personal_rules(db: &PgPool, person_id: Uuid) -> Result<Vec<Rule>, ApiError> {
    let rules = sqlx::query_scalar!(
        r#"SELECT rules AS "rules: JsonColumn<Vec<Rule>>" FROM people WHERE id = $1"#,
        person_id
    )
    .fetch_optional(db)
    .await?
    .ok_or(ApiError::NotFound)?;
    Ok(rules.0)
}

/// Changes what the `set_tags` rules of an organization and of its people
/// set, in one place for every list: merging tag values renames a value,
/// removing a tag name drops it. Rules that set nothing afterwards stay, so
/// their owners see them.
///
/// # Errors
///
/// A failed query.
pub async fn change_rule_tags(
    conn: &mut PgConnection,
    organization_id: Uuid,
    change: impl Fn(&mut tokencontroller_core::Tags),
) -> Result<(), ApiError> {
    let organization = sqlx::query_scalar!(
        r#"SELECT rules AS "rules: JsonColumn<Vec<Rule>>" FROM organizations
           WHERE id = $1 FOR UPDATE"#,
        organization_id
    )
    .fetch_one(&mut *conn)
    .await?;
    // No list yet means the default rules, which set no tags.
    if let Some(JsonColumn(mut rules)) = organization
        && change_tags_in(&mut rules, &change)
    {
        sqlx::query!(
            "UPDATE organizations SET rules = $2 WHERE id = $1",
            organization_id,
            JsonColumn(&rules) as _
        )
        .execute(&mut *conn)
        .await?;
    }
    let people = sqlx::query!(
        r#"SELECT id, rules AS "rules: JsonColumn<Vec<Rule>>" FROM people
           WHERE organization_id = $1 AND rules <> '[]'::jsonb FOR UPDATE"#,
        organization_id
    )
    .fetch_all(&mut *conn)
    .await?;
    for person in people {
        let JsonColumn(mut rules) = person.rules;
        if change_tags_in(&mut rules, &change) {
            sqlx::query!(
                "UPDATE people SET rules = $2 WHERE id = $1",
                person.id,
                JsonColumn(&rules) as _
            )
            .execute(&mut *conn)
            .await?;
        }
    }
    Ok(())
}

/// Applies `change` to every `set_tags` rule; whether any changed.
fn change_tags_in(rules: &mut [Rule], change: &impl Fn(&mut tokencontroller_core::Tags)) -> bool {
    let mut changed = false;
    for rule in rules {
        if let Action::SetTags(tags) = &mut rule.then {
            let before = tags.clone();
            change(tags);
            changed |= *tags != before;
        }
    }
    changed
}

/// The organization's rules and how they place token entries.
#[utoipa::path(
    get,
    path = "/organization/rules",
    tag = "rules",
    responses((status = OK, body = OrganizationRules)),
)]
pub(super) async fn get_organization_rules(
    State(state): State<AppState>,
    member: Member,
) -> Result<Json<OrganizationRules>, ApiError> {
    Ok(Json(
        organization_rules(&state.db, member.organization_id).await?,
    ))
}

/// Change the organization's rules. Controllers only.
#[utoipa::path(
    put,
    path = "/organization/rules",
    tag = "rules",
    request_body = OrganizationRulesChange,
    responses(
        (status = OK, body = OrganizationRules),
        (status = BAD_REQUEST, body = crate::error::ErrorBody),
        (status = FORBIDDEN, body = crate::error::ErrorBody),
    ),
)]
pub(super) async fn set_organization_rules(
    State(state): State<AppState>,
    member: Member,
    Json(change): Json<OrganizationRulesChange>,
) -> Result<Json<OrganizationRules>, ApiError> {
    member.require_session()?;
    member.require_role(Role::Controller)?;
    if let Some(rules) = &change.rules {
        check_rules(&state.db, member.organization_id, rules).await?;
    }
    sqlx::query!(
        r#"UPDATE organizations SET
             rules = coalesce($2, rules),
             several_keys = coalesce($3, several_keys),
             summary_match = coalesce($4, summary_match)
           WHERE id = $1"#,
        member.organization_id,
        change.rules.as_ref().map(JsonColumn) as _,
        change.several_keys.map(several_keys_text),
        change.summary_match
    )
    .execute(&state.db)
    .await?;
    record_audit_event(
        &state,
        member.organization_id,
        Some(member.person_id),
        "organization.rules_changed",
        None,
    );
    Ok(Json(
        organization_rules(&state.db, member.organization_id).await?,
    ))
}

/// Your own rules, in order. They run after the organization's.
#[utoipa::path(
    get,
    path = "/me/rules",
    tag = "rules",
    responses((status = OK, body = PersonalRules)),
)]
pub(super) async fn get_personal_rules(
    State(state): State<AppState>,
    member: Member,
) -> Result<Json<PersonalRules>, ApiError> {
    Ok(Json(PersonalRules {
        rules: personal_rules(&state.db, member.person_id).await?,
    }))
}

/// Replace your own rules. They run after the organization's.
#[utoipa::path(
    put,
    path = "/me/rules",
    tag = "rules",
    request_body = PersonalRules,
    responses(
        (status = OK, body = PersonalRules),
        (status = BAD_REQUEST, body = crate::error::ErrorBody),
    ),
)]
pub(super) async fn set_personal_rules(
    State(state): State<AppState>,
    member: Member,
    Json(change): Json<PersonalRules>,
) -> Result<Json<PersonalRules>, ApiError> {
    // `tc rules` edits them with the CLI token from `tc connect`.
    member.require_credential(&[CredentialKind::Session])?;
    check_rules(&state.db, member.organization_id, &change.rules).await?;
    sqlx::query!(
        "UPDATE people SET rules = $2 WHERE id = $1",
        member.person_id,
        JsonColumn(&change.rules) as _
    )
    .execute(&state.db)
    .await?;
    Ok(Json(PersonalRules {
        rules: personal_rules(&state.db, member.person_id).await?,
    }))
}

/// What a local tool needs to place token entries the way the service
/// does: the organization's rules and tag names, your own rules and tags,
/// and the projects with their ticket key patterns and repositories.
#[utoipa::path(
    get,
    path = "/me/context",
    tag = "rules",
    responses(
        (status = OK, body = Context),
        (status = UNAUTHORIZED, body = crate::error::ErrorBody),
    ),
)]
pub(super) async fn context(
    State(state): State<AppState>,
    member: Member,
) -> Result<Json<Context>, ApiError> {
    let organization_id = member.organization_id;
    let organization = super::organization(&state.db, organization_id).await?;
    let organization_rules = organization_rules(&state.db, organization_id).await?;
    let tag_names = tags::tag_names(&state.db, organization_id)
        .await?
        .into_iter()
        .map(|tag| tag.name)
        .collect();
    let person = sqlx::query!(
        r#"SELECT email, name, tags AS "tags: JsonColumn<Tags>",
                  rules AS "rules: JsonColumn<Vec<Rule>>"
           FROM people WHERE id = $1"#,
        member.person_id
    )
    .fetch_one(&state.db)
    .await?;
    let projects = projects::projects(&state.db, organization_id)
        .await?
        .into_iter()
        .filter(|project| !project.archived)
        .map(|project| ContextProject {
            name: project.name,
            ticket_key_pattern: project.ticket_key_pattern,
            repositories: project.repositories,
            tags: project.tags,
        })
        .collect();
    Ok(Json(Context {
        organization: ContextOrganization {
            name: organization.name,
            currency: organization.reporting_currency,
            tag_names,
            several_keys: organization_rules.several_keys,
            rules: organization_rules.rules,
        },
        person: ContextPerson {
            email: person.email,
            name: person.name,
            tags: person.tags.0,
        },
        rules: person.rules.0,
        projects,
        cadence: None,
        rejected: Vec::new(),
    }))
}

/// A rule list can hold at most [`MAX_RULES`] rules, each with a name, and
/// set only tags on the organization's list, never `person`: the person is
/// who did the work.
async fn check_rules(db: &PgPool, organization_id: Uuid, rules: &[Rule]) -> Result<(), ApiError> {
    if rules.len() > MAX_RULES {
        return Err(ApiError::BadRequest(format!(
            "A list holds at most {MAX_RULES} rules."
        )));
    }
    let known: BTreeSet<String> = tags::tag_names(db, organization_id)
        .await?
        .into_iter()
        .map(|tag| tag.name)
        .collect();
    for rule in rules {
        check_rule(rule, &known)?;
    }
    Ok(())
}

fn check_rule(rule: &Rule, known: &BTreeSet<String>) -> Result<(), ApiError> {
    let name = rule.name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
        return Err(ApiError::BadRequest(format!(
            "Every rule needs a name of 1 to {MAX_NAME_CHARS} characters."
        )));
    }
    let Action::SetTags(tags) = &rule.then else {
        return Ok(());
    };
    if tags.is_empty() {
        return Err(ApiError::BadRequest(format!(
            "The rule {name:?} sets no tags."
        )));
    }
    for (tag, _) in tags.iter() {
        if tag.as_str() == "person" {
            return Err(ApiError::BadRequest(format!(
                "The rule {name:?} sets person. The person is who did the work; no rule can change it."
            )));
        }
        if !known.contains(tag.as_str()) {
            return Err(ApiError::BadRequest(format!(
                "The rule {name:?} sets {:?}, which is not on the organization's list of tag names.",
                tag.as_str()
            )));
        }
    }
    Ok(())
}

fn several_keys_from(text: &str) -> SeveralKeys {
    match text {
        "equal_split" => SeveralKeys::EqualSplit,
        "first_named" => SeveralKeys::FirstNamed,
        _ => SeveralKeys::MostOften,
    }
}

const fn several_keys_text(several_keys: SeveralKeys) -> &'static str {
    match several_keys {
        SeveralKeys::MostOften => "most_often",
        SeveralKeys::EqualSplit => "equal_split",
        SeveralKeys::FirstNamed => "first_named",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokencontroller_core::tags::{TagName, TagValue};

    fn sets(name: &str, value: &str) -> Rule {
        let mut tags = tokencontroller_core::Tags::new();
        tags.insert(TagName::new(name).unwrap(), TagValue::new(value).unwrap());
        Rule {
            name: format!("{name} rule"),
            when: Vec::new(),
            then: Action::SetTags(tags),
        }
    }

    #[test]
    fn rules_set_only_known_tags_and_never_the_person() {
        let known: BTreeSet<String> = ["person", "ticket", "project", "purpose"]
            .map(str::to_owned)
            .into();
        assert!(check_rule(&sets("purpose", "code-review"), &known).is_ok());
        assert!(check_rule(&sets("ticket", "KD-214"), &known).is_ok());
        assert!(check_rule(&sets("client", "acme"), &known).is_err());
        assert!(check_rule(&sets("person", "m.ortner@kestrel.example"), &known).is_err());
        let mut unnamed = sets("purpose", "code-review");
        unnamed.name = "  ".to_owned();
        assert!(check_rule(&unnamed, &known).is_err());
        assert!(
            Rule::defaults()
                .iter()
                .all(|rule| check_rule(rule, &known).is_ok())
        );
    }

    #[test]
    fn several_keys_round_trip_through_their_stored_text() {
        for several_keys in [
            SeveralKeys::MostOften,
            SeveralKeys::EqualSplit,
            SeveralKeys::FirstNamed,
        ] {
            assert_eq!(
                several_keys_from(several_keys_text(several_keys)),
                several_keys
            );
            // The stored text is the serde name the API uses.
            assert_eq!(
                serde_json::to_value(several_keys).unwrap(),
                several_keys_text(several_keys)
            );
        }
    }
}
