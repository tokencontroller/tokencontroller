//! Messages and sessions as the fold shows them (docs: HTTP API, `GET
//! /messages` and `GET /sessions`): each with its cost, its parts with
//! their tags and labels, its facts and summary, inside the caller's view
//! grants.
//!
//! Owner: the service platform work. Keep this area's code, routes and
//! queries in this folder.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Query, State};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tokencontroller_core::state::State as Fold;
use tokencontroller_core::{CostLabel, DateRange, Filter, Line, SubmissionStatus, TagValue};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::AppState;
use crate::error::ApiError;
use crate::events::json::{self, TagsSchema};
use crate::events::{Log, access};
use crate::sign_in::Member;

/// This area's routes, relative to `/api/v1`.
#[must_use]
pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_messages))
        .routes(routes!(list_sessions))
}

/// What a list of messages or sessions is cut by.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct MessagesQuery {
    /// The first day, `YYYY-MM-DD`. Default: the beginning.
    pub from: Option<String>,
    /// The last day, `YYYY-MM-DD`. Default: the end.
    pub to: Option<String>,
    /// The date up to which the log is read. Default today.
    pub as_of: Option<String>,
    /// A filter, such as `client:acme,team:web`.
    pub filter: Option<String>,
    /// `true` shows approved messages only.
    pub approved: Option<bool>,
    /// One session.
    pub session_id: Option<String>,
    /// Only messages that still have no feature and no project:
    /// organization overhead, the check list.
    pub unassigned: Option<bool>,
    /// At most this many, newest first. Default 200.
    pub limit: Option<usize>,
}

/// Where a message stands in submit and approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Submitted {
    /// Not inside any submission of its person.
    Open,
    Submitted,
    Approved,
    Rejected,
}

/// One part of a message's cost on one tag set.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PartView {
    pub fraction: String,
    /// The split's own tags for this part.
    #[schema(value_type = TagsSchema)]
    pub assigned: BTreeMap<String, String>,
    /// The tags closed under defaults, with the person and their
    /// attributes.
    #[schema(value_type = TagsSchema)]
    pub tags: BTreeMap<String, String>,
    pub amount: String,
    /// `direct`, `project_overhead` or `organization_overhead`.
    pub label: &'static str,
}

/// One message as the API shows it.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct MessageView {
    pub session_id: String,
    pub message_id: String,
    pub person: String,
    pub provider: String,
    pub model: String,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub at: OffsetDateTime,
    pub agent_harness: String,
    pub agent_name: Option<String>,
    #[schema(value_type = Option<Object>)]
    pub tokens: Option<tokencontroller_core::TokenCounts>,
    /// `C(e)` in US dollars, or `null` with a cost label other than
    /// `priced`.
    pub cost: Option<String>,
    /// `priced`, `count_error` or `unknown_model`.
    pub cost_label: &'static str,
    pub reported_cost: Option<String>,
    /// `hand`, `rule` or `default`.
    pub assigned_by: &'static str,
    pub parts: Vec<PartView>,
    #[schema(value_type = Object)]
    pub signals: tokencontroller_core::Signals,
    #[schema(value_type = Object)]
    pub facts: tokencontroller_core::SessionFacts,
    pub submitted: Submitted,
}

/// One session: its messages added up.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SessionView {
    pub session_id: String,
    pub person: String,
    pub agent_harness: String,
    pub agent_name: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub started_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub ended_at: OffsetDateTime,
    pub messages: usize,
    /// The cost of every priced message, in US dollars.
    pub cost: String,
    pub count_errors: usize,
    pub unknown_models: Vec<String>,
    /// Cost by tag set, the session's splits added up.
    pub parts: Vec<PartView>,
    pub summary: Option<String>,
    #[schema(value_type = Object)]
    pub signals: tokencontroller_core::Signals,
    #[schema(value_type = Object)]
    pub facts: tokencontroller_core::SessionFacts,
    pub assigned_by: &'static str,
    pub submitted: Submitted,
}

/// A message list with its as-of moment.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct MessageList {
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub as_of: OffsetDateTime,
    pub messages: Vec<MessageView>,
}

/// A session list with its as-of moment.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SessionList {
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub as_of: OffsetDateTime,
    pub sessions: Vec<SessionView>,
}

const fn cost_label(label: CostLabel) -> &'static str {
    match label {
        CostLabel::Priced => "priced",
        CostLabel::CountError => "count_error",
        CostLabel::UnknownModel => "unknown_model",
    }
}

const fn assigned_by(by: tokencontroller_core::AssignedBy) -> &'static str {
    match by {
        tokencontroller_core::AssignedBy::Hand => "hand",
        tokencontroller_core::AssignedBy::Rule => "rule",
        tokencontroller_core::AssignedBy::Default => "default",
    }
}

/// Where a message stands in submit and approval.
#[must_use]
pub fn submitted(fold: &Fold, person: &TagValue, at: OffsetDateTime) -> Submitted {
    match fold
        .submission_of(person, at)
        .map(|submission| &submission.status)
    {
        None => Submitted::Open,
        Some(SubmissionStatus::Submitted) => Submitted::Submitted,
        Some(SubmissionStatus::Approved) => Submitted::Approved,
        Some(SubmissionStatus::Rejected { .. }) => Submitted::Rejected,
    }
}

/// The lines of the fold the caller may view, inside the filter, window
/// and the other cuts of the query, grouped by message.
fn visible_lines<'a>(
    fold: &'a Fold,
    person: &TagValue,
    filter: &'a Filter,
    window: DateRange,
    query: &MessagesQuery,
) -> BTreeMap<(String, String), Vec<&'a Line>> {
    let mut by_message: BTreeMap<(String, String), Vec<&Line>> = BTreeMap::new();
    for line in fold.select(filter, window) {
        if !access::can_view(fold, person, &line.tags) {
            continue;
        }
        if query
            .session_id
            .as_deref()
            .is_some_and(|id| id != line.session_id.as_str())
        {
            continue;
        }
        if query.approved == Some(true)
            && submitted(fold, &line.person, line.at) != Submitted::Approved
        {
            continue;
        }
        by_message
            .entry((line.session_id.to_string(), line.message_id.to_string()))
            .or_default()
            .push(line);
    }
    if query.unassigned == Some(true) {
        by_message.retain(|_, lines| {
            lines
                .iter()
                .all(|line| line.label == tokencontroller_core::Label::OrganizationOverhead)
        });
    }
    by_message
}

fn part_views(lines: &[&Line]) -> Vec<PartView> {
    let total: i64 = lines.iter().map(|line| line.amount.nanos()).sum();
    lines
        .iter()
        .map(|line| PartView {
            fraction: if total == 0 {
                crate::splits::fraction_text(1, lines.len().try_into().unwrap_or(1))
            } else {
                crate::splits::fraction_text(
                    line.amount.nanos().try_into().unwrap_or(0),
                    total.try_into().unwrap_or(1),
                )
            },
            assigned: json::tags_map(&line.assigned),
            tags: json::tags_map(&line.tags),
            amount: json::money(line.amount),
            label: line.label.as_str(),
        })
        .collect()
}

/// The messages of the fold as views, newest first.
#[must_use]
pub fn message_views(
    fold: &Fold,
    person: &TagValue,
    filter: &Filter,
    window: DateRange,
    query: &MessagesQuery,
) -> Vec<MessageView> {
    let by_message = visible_lines(fold, person, filter, window, query);
    let costs: BTreeMap<(String, String), &tokencontroller_core::MessageCostLine> = fold
        .message_costs()
        .iter()
        .map(|cost| {
            (
                (cost.session_id.to_string(), cost.message_id.to_string()),
                cost,
            )
        })
        .collect();
    let messages: BTreeMap<
        (String, String),
        &tokencontroller_core::Recorded<tokencontroller_core::Message>,
    > = fold
        .messages()
        .map(|message| {
            (
                (
                    message.value.session_id.to_string(),
                    message.value.message_id.to_string(),
                ),
                message,
            )
        })
        .collect();
    let mut views: Vec<MessageView> = by_message
        .into_iter()
        .filter_map(|(key, lines)| {
            let message = &messages.get(&key)?.value;
            let cost = costs.get(&key)?;
            let first = lines.first()?;
            Some(MessageView {
                session_id: key.0,
                message_id: key.1,
                person: message.person.to_string(),
                provider: message.model.provider.clone(),
                model: message.model.model.clone(),
                at: message.at,
                agent_harness: message.agent_harness.clone(),
                agent_name: message.agent_name.clone(),
                tokens: message.tokens,
                cost: cost.cost.amount.map(json::money),
                cost_label: cost_label(cost.cost.label),
                reported_cost: message.reported_cost.map(json::money_exact),
                assigned_by: assigned_by(first.assigned_by),
                parts: part_views(&lines),
                signals: message.signals.clone(),
                facts: message.session.clone(),
                submitted: submitted(fold, &message.person, message.at),
            })
        })
        .collect();
    views.sort_by(|a, b| {
        b.at.cmp(&a.at)
            .then_with(|| a.message_id.cmp(&b.message_id))
    });
    views.truncate(query.limit.unwrap_or(200));
    views
}

/// A session's parts by their tags: the amount so far, the assigned tags,
/// the closed tags and the label.
type SessionParts = BTreeMap<
    String,
    (
        tokencontroller_core::Money,
        BTreeMap<String, String>,
        BTreeMap<String, String>,
        &'static str,
    ),
>;

/// The cost of a session and its parts as views, each with its fraction of
/// the whole.
fn session_part_views(
    currency: tokencontroller_core::Currency,
    session_parts: SessionParts,
) -> (String, Vec<PartView>) {
    let total: i64 = session_parts.values().map(|part| part.0.nanos()).sum();
    let cost = json::money(tokencontroller_core::Money::from_nanos(total, currency));
    let parts = session_parts
        .into_values()
        .map(|(amount, assigned, tags, label)| PartView {
            fraction: if total == 0 {
                "1".to_owned()
            } else {
                crate::splits::fraction_text(
                    amount.nanos().try_into().unwrap_or(0),
                    total.try_into().unwrap_or(1),
                )
            },
            assigned,
            tags,
            amount: json::money(amount),
            label,
        })
        .collect();
    (cost, parts)
}

/// The sessions of the fold as views, newest first.
#[must_use]
pub fn session_views(
    fold: &Fold,
    person: &TagValue,
    filter: &Filter,
    window: DateRange,
    query: &MessagesQuery,
) -> Vec<SessionView> {
    let by_message = visible_lines(fold, person, filter, window, query);
    let messages: BTreeMap<
        (String, String),
        &tokencontroller_core::Recorded<tokencontroller_core::Message>,
    > = fold
        .messages()
        .map(|message| {
            (
                (
                    message.value.session_id.to_string(),
                    message.value.message_id.to_string(),
                ),
                message,
            )
        })
        .collect();
    let mut sessions: BTreeMap<String, SessionView> = BTreeMap::new();
    let mut parts: BTreeMap<String, SessionParts> = BTreeMap::new();
    for (key, lines) in by_message {
        let Some(message) = messages.get(&key).map(|message| &message.value) else {
            continue;
        };
        let Some(first) = lines.first() else {
            continue;
        };
        let session = sessions
            .entry(key.0.clone())
            .or_insert_with(|| SessionView {
                session_id: key.0.clone(),
                person: message.person.to_string(),
                agent_harness: message.agent_harness.clone(),
                agent_name: message.agent_name.clone(),
                started_at: message.at,
                ended_at: message.at,
                messages: 0,
                cost: String::new(),
                count_errors: 0,
                unknown_models: Vec::new(),
                parts: Vec::new(),
                summary: message.session.summary.clone(),
                signals: message.signals.clone(),
                facts: message.session.clone(),
                assigned_by: assigned_by(first.assigned_by),
                submitted: submitted(fold, &message.person, message.at),
            });
        session.messages = session.messages.saturating_add(1);
        session.started_at = session.started_at.min(message.at);
        if message.at >= session.ended_at {
            session.ended_at = message.at;
            if message.session.summary.is_some() {
                session.summary.clone_from(&message.session.summary);
                session.facts = message.session.clone();
                session.signals = message.signals.clone();
            }
        }
        match first.cost_label {
            CostLabel::Priced => {}
            CostLabel::CountError => session.count_errors = session.count_errors.saturating_add(1),
            CostLabel::UnknownModel => {
                let model = message.model.to_string();
                if !session.unknown_models.contains(&model) {
                    session.unknown_models.push(model);
                }
            }
        }
        let session_parts = parts.entry(key.0.clone()).or_default();
        for line in lines {
            let tags = json::tags_map(&line.tags);
            let id = serde_json::to_string(&tags).unwrap_or_default();
            let entry = session_parts.entry(id).or_insert_with(|| {
                (
                    tokencontroller_core::Money::zero(fold.currency()),
                    json::tags_map(&line.assigned),
                    tags,
                    line.label.as_str(),
                )
            });
            entry.0 = entry.0.checked_add(line.amount).unwrap_or(entry.0);
        }
    }
    let mut views: Vec<SessionView> = sessions
        .into_iter()
        .map(|(id, mut session)| {
            let session_parts = parts.remove(&id).unwrap_or_default();
            let (cost, parts) = session_part_views(fold.currency(), session_parts);
            session.cost = cost;
            session.parts = parts;
            session
        })
        .collect();
    views.sort_by(|a, b| {
        b.ended_at
            .cmp(&a.ended_at)
            .then_with(|| a.session_id.cmp(&b.session_id))
    });
    views.truncate(query.limit.unwrap_or(200));
    views
}

/// Messages with their cost, parts, tags, facts and where they stand in
/// submit and approval, inside what you may view. Newest first.
#[utoipa::path(
    get,
    path = "/messages",
    tag = "messages",
    params(MessagesQuery),
    responses((status = OK, body = MessageList)),
)]
async fn list_messages(
    State(state): State<AppState>,
    member: Member,
    Query(query): Query<MessagesQuery>,
) -> Result<Json<MessageList>, ApiError> {
    let filter = json::filter(query.filter.as_deref())?;
    let window = crate::dates::range(query.from.as_deref(), query.to.as_deref())?;
    let log = Log::load(&state, member.organization_id).await?;
    let fold = log.fold(crate::dates::as_of(query.as_of.as_deref())?)?;
    let person = member.person()?;
    Ok(Json(MessageList {
        as_of: fold.as_of(),
        messages: message_views(&fold, &person, &filter, window, &query),
    }))
}

/// Sessions: their messages added up, with the cost by tag set, the
/// summary and the facts. Newest first.
#[utoipa::path(
    get,
    path = "/sessions",
    tag = "messages",
    params(MessagesQuery),
    responses((status = OK, body = SessionList)),
)]
async fn list_sessions(
    State(state): State<AppState>,
    member: Member,
    Query(query): Query<MessagesQuery>,
) -> Result<Json<SessionList>, ApiError> {
    let filter = json::filter(query.filter.as_deref())?;
    let window = crate::dates::range(query.from.as_deref(), query.to.as_deref())?;
    let log = Log::load(&state, member.organization_id).await?;
    let fold = log.fold(crate::dates::as_of(query.as_of.as_deref())?)?;
    let person = member.person()?;
    Ok(Json(SessionList {
        as_of: fold.as_of(),
        sessions: session_views(&fold, &person, &filter, window, &query),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_have_their_words() {
        assert_eq!(cost_label(CostLabel::CountError), "count_error");
        assert_eq!(assigned_by(tokencontroller_core::AssignedBy::Rule), "rule");
        let query: MessagesQuery = serde_json::from_str(r#"{"approved": true}"#).unwrap();
        assert_eq!(query.approved, Some(true));
        assert!(query.limit.is_none());
    }
}
