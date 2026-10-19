//! Rules (docs: Rules): a pattern over signals that gives a tag value.
//! Organization rules run before personal ones; a new rule runs on
//! arrival and back over every message since the cutoff that has no hand
//! assignment, which the core's fold does.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tokencontroller_core::state::State as Fold;
use tokencontroller_core::{
    Capability, Event, Filter, Record, Rule, Scope, Signal, TagValue, Tags, TextPattern, Then,
};
use utoipa::ToSchema;

use crate::AppState;
use crate::error::ApiError;
use crate::events::json;
use crate::events::{self, Log, access, prefix};
use crate::sign_in::Member;

/// A rule as the API takes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NewRule {
    /// The name people see, such as `feature key in branch`.
    pub name: String,
    /// Lower runs first. Default: after the rules there are.
    #[serde(default)]
    pub order: Option<u32>,
    /// The signal: `branch`, `working_directory`, `repository`,
    /// `pull_request_title`, `subagent_type`, `prompt_keys`, `agent_name`,
    /// `config:<name>` or `resource:<name>`.
    pub signal: String,
    /// A regular expression. A group named `key` picks the tag value's key;
    /// without one, the whole match is the key.
    pub pattern: String,
    /// The tag name the rule sets, such as `feature` or `project`.
    pub tag_name: String,
    /// A fixed key, such as `shop`. Left out, the rule sets the key the
    /// pattern finds: "that feature".
    #[serde(default)]
    pub key: Option<String>,
}

/// A rule as the API shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct RuleView {
    /// The rule's event id; reverse it to remove the rule.
    pub id: String,
    pub name: String,
    /// `organization`, or the person's email.
    pub scope: String,
    pub order: u32,
    pub signal: String,
    pub pattern: String,
    pub tag_name: String,
    pub key: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub recorded_at: OffsetDateTime,
}

fn signal_word(signal: &Signal) -> String {
    match signal {
        Signal::Branch => "branch".to_owned(),
        Signal::WorkingDirectory => "working_directory".to_owned(),
        Signal::Repository => "repository".to_owned(),
        Signal::PullRequestTitle => "pull_request_title".to_owned(),
        Signal::SubagentType => "subagent_type".to_owned(),
        Signal::PromptKeys => "prompt_keys".to_owned(),
        Signal::AgentName => "agent_name".to_owned(),
        Signal::Config(name) => format!("config:{name}"),
        Signal::Resource(name) => format!("resource:{name}"),
    }
}

fn parse_signal(text: &str) -> Result<Signal, ApiError> {
    let text = text.trim();
    Ok(match text {
        "branch" => Signal::Branch,
        "working_directory" => Signal::WorkingDirectory,
        "repository" => Signal::Repository,
        "pull_request_title" => Signal::PullRequestTitle,
        "subagent_type" => Signal::SubagentType,
        "prompt_keys" | "keys" => Signal::PromptKeys,
        "agent_name" => Signal::AgentName,
        _ => {
            if let Some(name) = text.strip_prefix("config:") {
                Signal::Config(name.trim().to_owned())
            } else if let Some(name) = text.strip_prefix("resource:") {
                Signal::Resource(name.trim().to_owned())
            } else {
                return Err(ApiError::BadRequest(format!(
                    "{text:?} is not a signal; use branch, working_directory, repository, pull_request_title, subagent_type, prompt_keys, agent_name, config:<name> or resource:<name>"
                )));
            }
        }
    })
}

impl RuleView {
    /// The view of a rule event.
    #[must_use]
    pub fn of(event: &Event, rule: &Rule) -> Self {
        Self::recorded(event.id.as_str(), event.recorded_at, rule)
    }

    /// The view of a rule by its event id and moment.
    #[must_use]
    pub fn recorded(id: &str, recorded_at: OffsetDateTime, rule: &Rule) -> Self {
        let (tag_name, key) = match &rule.then {
            Then::Tag { name, key } => (name.to_string(), Some(key.to_string())),
            Then::KeyOf { name } => (name.to_string(), None),
        };
        Self {
            id: id.to_owned(),
            name: rule.name.clone(),
            scope: match &rule.scope {
                Scope::Organization => "organization".to_owned(),
                Scope::Person(person) => person.to_string(),
            },
            order: rule.order,
            signal: signal_word(&rule.signal),
            pattern: rule.pattern.as_str().to_owned(),
            tag_name,
            key,
            recorded_at,
        }
    }
}

/// Refuses a rule whose tag name is on neither list, or whose fixed key is
/// no value on record.
async fn check_then(
    state: &AppState,
    fold: &Fold,
    organization_id: uuid::Uuid,
    rule: &Rule,
) -> Result<(), ApiError> {
    let mut tags = Tags::new();
    match &rule.then {
        Then::Tag { name, key } => {
            tags.insert(name.clone(), key.clone());
            crate::tags::check_names(&state.db, organization_id, &tags).await?;
            if let Some(first) = crate::tags::unknown_values(fold, &tags).first() {
                return Err(ApiError::BadRequest(format!(
                    "{first} is not a value on record; add it first"
                )));
            }
        }
        Then::KeyOf { name } => {
            tags.insert(
                name.clone(),
                TagValue::new("x")
                    .map_err(|error| ApiError::Internal(anyhow::anyhow!("{error}")))?,
            );
            crate::tags::check_names(&state.db, organization_id, &tags).await?;
        }
    }
    Ok(())
}

/// Reads a rule for a scope.
///
/// # Errors
///
/// [`ApiError::BadRequest`] naming what is wrong.
pub fn parse_rule(new: &NewRule, scope: Scope, next_order: u32) -> Result<Rule, ApiError> {
    let name = new.name.trim();
    if name.is_empty() || name.chars().count() > 100 {
        return Err(ApiError::BadRequest(
            "a rule's name has 1 to 100 characters".to_owned(),
        ));
    }
    let pattern = TextPattern::new(new.pattern.trim())
        .map_err(|error| ApiError::BadRequest(format!("pattern: {error}")))?;
    let tag_name = json::tag_name(&new.tag_name)?;
    if tag_name.as_str() == "person" {
        return Err(ApiError::BadRequest(
            "a rule cannot set the person; the person comes from the token".to_owned(),
        ));
    }
    let then = match new
        .key
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty())
    {
        Some(key) => Then::Tag {
            name: tag_name.clone(),
            key: json::tag_value(tag_name.as_str(), key)?,
        },
        None => Then::KeyOf { name: tag_name },
    };
    Ok(Rule {
        name: name.to_owned(),
        scope,
        order: new.order.unwrap_or(next_order),
        signal: parse_signal(&new.signal)?,
        pattern,
        then,
    })
}

/// The rules in force as views: the organization's and, when `person` is
/// given, that person's own.
#[must_use]
pub fn views(fold: &Fold, person: Option<&TagValue>) -> Vec<RuleView> {
    let mut rules: Vec<&tokencontroller_core::Recorded<Rule>> = fold
        .rules()
        .filter(|rule| match &rule.value.scope {
            Scope::Organization => true,
            Scope::Person(owner) => person == Some(owner),
        })
        .collect();
    rules.sort_by(|a, b| {
        a.value
            .scope
            .cmp(&b.value.scope)
            .then(a.value.order.cmp(&b.value.order))
            .then(a.recorded_at.cmp(&b.recorded_at))
    });
    rules
        .into_iter()
        .map(|rule| RuleView::recorded(rule.id.as_str(), rule.recorded_at, &rule.value))
        .collect()
}

fn next_order(fold: &Fold, scope: &Scope) -> u32 {
    fold.rules()
        .filter(|rule| rule.value.scope == *scope)
        .map(|rule| rule.value.order.saturating_add(10))
        .max()
        .unwrap_or(10)
}

/// Refuses a rule the caller may not add: a fixed tag needs assign on
/// that tag; "that feature" needs assign somewhere.
fn require_for(fold: &Fold, person: &TagValue, rule: &Rule) -> Result<(), ApiError> {
    match &rule.then {
        Then::Tag { name, key } => {
            let mut tags = Tags::new();
            tags.insert(name.clone(), key.clone());
            let closed = crate::tags::closed(fold, name, key, &Tags::new());
            if access::can_view(fold, person, &closed)
                && access::covers(fold, person, Capability::Assign, &closed)
            {
                Ok(())
            } else {
                access::require(fold, person, Capability::Assign, &Filter(tags))
            }
        }
        Then::KeyOf { .. } => access::require_any(fold, person, Capability::Assign),
    }
}

/// The organization's rules and your own, in the order they run.
#[utoipa::path(
    get,
    path = "/rules",
    tag = "rules",
    responses((status = OK, body = Vec<RuleView>)),
)]
pub(super) async fn list_rules(
    State(state): State<AppState>,
    member: Member,
) -> Result<Json<Vec<RuleView>>, ApiError> {
    let log = Log::load(&state, member.organization_id).await?;
    let fold = log.fold(OffsetDateTime::now_utc())?;
    Ok(Json(views(&fold, Some(&member.person()?))))
}

/// Add an organization rule. It runs before everyone's personal rules, on
/// arrival and back over unassigned messages since the cutoff. Needs
/// assign on the tag it sets.
#[utoipa::path(
    post,
    path = "/rules",
    tag = "rules",
    request_body = NewRule,
    responses(
        (status = CREATED, body = RuleView),
        (status = BAD_REQUEST, body = crate::error::ErrorBody),
        (status = FORBIDDEN, body = crate::error::ErrorBody),
    ),
)]
pub(super) async fn add_rule(
    State(state): State<AppState>,
    member: Member,
    Json(request): Json<NewRule>,
) -> Result<(StatusCode, Json<RuleView>), ApiError> {
    member.require_session()?;
    let log = Log::load(&state, member.organization_id).await?;
    let fold = log.fold(OffsetDateTime::now_utc())?;
    let person = member.person()?;
    let rule = parse_rule(
        &request,
        Scope::Organization,
        next_order(&fold, &Scope::Organization),
    )?;
    check_then(&state, &fold, member.organization_id, &rule).await?;
    require_for(&fold, &person, &rule)?;
    let event = events::event(Record::Rule(rule.clone()))?.by(person);
    events::write(&state, &log, std::slice::from_ref(&event)).await?;
    super::record_audit_event(
        &state,
        member.organization_id,
        Some(member.person_id),
        "rule.added",
        None,
    );
    Ok((StatusCode::CREATED, Json(RuleView::of(&event, &rule))))
}

/// Your own rules, in the order they run.
#[utoipa::path(
    get,
    path = "/me/rules",
    tag = "rules",
    responses((status = OK, body = Vec<RuleView>)),
)]
pub(super) async fn get_my_rules(
    State(state): State<AppState>,
    member: Member,
) -> Result<Json<Vec<RuleView>>, ApiError> {
    let log = Log::load(&state, member.organization_id).await?;
    let fold = log.fold(OffsetDateTime::now_utc())?;
    let person = member.person()?;
    Ok(Json(
        views(&fold, Some(&person))
            .into_iter()
            .filter(|rule| rule.scope == person.as_str())
            .collect(),
    ))
}

/// Replace your own rules. They run after the organization's, on your
/// messages only, and back over your unassigned messages since the
/// cutoff.
#[utoipa::path(
    put,
    path = "/me/rules",
    tag = "rules",
    request_body = Vec<NewRule>,
    responses(
        (status = OK, body = Vec<RuleView>),
        (status = BAD_REQUEST, body = crate::error::ErrorBody),
    ),
)]
pub(super) async fn set_my_rules(
    State(state): State<AppState>,
    member: Member,
    Json(request): Json<Vec<NewRule>>,
) -> Result<Json<Vec<RuleView>>, ApiError> {
    member.require_session()?;
    let log = Log::load(&state, member.organization_id).await?;
    let fold = log.fold(OffsetDateTime::now_utc())?;
    let person = member.person()?;
    let scope = Scope::Person(person.clone());
    let mut new_events = Vec::new();
    let reversed = log.reversed();
    for event in &log.events {
        if let Record::Rule(rule) = &event.record
            && rule.scope == scope
            && !reversed.contains(&event.id)
        {
            new_events.push(
                Event::new(
                    &events::new_id(prefix::REVERSAL),
                    OffsetDateTime::now_utc(),
                    Record::Reversal(event.id.clone()),
                )
                .map_err(|error| ApiError::Internal(error.into()))?
                .by(person.clone()),
            );
        }
    }
    let mut rules = Vec::with_capacity(request.len());
    for (index, new) in request.iter().enumerate() {
        let order = u32::try_from(index)
            .unwrap_or(u32::MAX)
            .saturating_add(1)
            .saturating_mul(10);
        let rule = parse_rule(new, scope.clone(), order)?;
        check_then(&state, &fold, member.organization_id, &rule).await?;
        let event = Event::new(
            &events::new_id(prefix::RULE),
            OffsetDateTime::now_utc().saturating_add(time::Duration::microseconds(
                i64::try_from(index).unwrap_or(0).saturating_add(1),
            )),
            Record::Rule(rule.clone()),
        )
        .map_err(|error| ApiError::Internal(error.into()))?
        .by(person.clone());
        rules.push(RuleView::of(&event, &rule));
        new_events.push(event);
    }
    events::write(&state, &log, &new_events).await?;
    let _ = fold;
    Ok(Json(rules))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rule_reads_from_its_words() {
        let new = NewRule {
            name: "feature key in branch".to_owned(),
            order: None,
            signal: "branch".to_owned(),
            pattern: r"(?P<key>KD-\d+)".to_owned(),
            tag_name: "feature".to_owned(),
            key: None,
        };
        let rule = parse_rule(&new, Scope::Organization, 10).unwrap();
        assert_eq!(rule.order, 10);
        assert_eq!(rule.signal, Signal::Branch);
        assert!(matches!(rule.then, Then::KeyOf { .. }));
        let event = Event::new(
            "rul_1",
            OffsetDateTime::UNIX_EPOCH,
            Record::Rule(rule.clone()),
        )
        .unwrap();
        let view = RuleView::of(&event, &rule);
        assert_eq!(view.scope, "organization");
        assert_eq!(view.signal, "branch");
        assert!(view.key.is_none());

        let fixed = NewRule {
            key: Some("shop".to_owned()),
            signal: "config:project".to_owned(),
            tag_name: "project".to_owned(),
            ..new.clone()
        };
        let rule = parse_rule(&fixed, Scope::Person(TagValue::new("mara").unwrap()), 20).unwrap();
        assert_eq!(rule.signal, Signal::Config("project".to_owned()));
        assert_eq!(signal_word(&rule.signal), "config:project");
        assert!(matches!(rule.then, Then::Tag { .. }));

        assert!(
            parse_rule(
                &NewRule {
                    signal: "mood".to_owned(),
                    ..new.clone()
                },
                Scope::Organization,
                1
            )
            .is_err()
        );
        assert!(
            parse_rule(
                &NewRule {
                    pattern: "(".to_owned(),
                    ..new.clone()
                },
                Scope::Organization,
                1
            )
            .is_err()
        );
        assert!(
            parse_rule(
                &NewRule {
                    tag_name: "person".to_owned(),
                    ..new.clone()
                },
                Scope::Organization,
                1
            )
            .is_err()
        );
        assert!(
            parse_rule(
                &NewRule {
                    name: " ".to_owned(),
                    ..new
                },
                Scope::Organization,
                1
            )
            .is_err()
        );
    }
}
