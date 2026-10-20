//! Submit and approval (docs: Account > Submit and approval). A submit
//! covers all of a person's own messages from their last submit to now;
//! an organization with approval on lets people managers approve or
//! reject each submission. Before moving the cutoff, an org manager asks
//! everyone to submit; each person gets one email.
//!
//! Owner: the service platform work. Keep this area's code, routes and
//! queries in this folder, and its migrations in `migrations/` named
//! `<timestamp>_submissions_<what>.sql`.

use std::time::Duration;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use time::{Date, OffsetDateTime};
use tokencontroller_core::state::State as Fold;
use tokencontroller_core::{
    Approval, Capability, DateRange, Decision, EventId, Record, Submission, SubmissionStatus,
    Submit, TagValue, price::date_of,
};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::AppState;
use crate::error::ApiError;
use crate::events::json;
use crate::events::{self, Log, access};
use crate::mail::{self, Mail, render};
use crate::sign_in::Member;

const REMINDER_TEXT: &str = include_str!("submit-reminder.txt");
const REMINDER_HTML: &str = include_str!("submit-reminder.html");

/// The job that mails the reminders of open submit requests.
pub const JOB: &str = "submit-reminders";
/// How often it runs.
pub const JOB_EVERY: Duration = Duration::from_mins(15);

/// This area's routes, relative to `/api/v1`.
#[must_use]
pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_submissions, submit))
        .routes(routes!(approve))
        .routes(routes!(reject))
        .routes(routes!(list_submit_requests, add_submit_request))
}

/// A submission as the API shows it.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SubmissionView {
    /// The submit event's id.
    pub id: String,
    pub person: String,
    #[serde(with = "crate::dates::day")]
    #[schema(value_type = String, format = Date)]
    pub from: Date,
    #[serde(with = "crate::dates::day")]
    #[schema(value_type = String, format = Date)]
    pub to: Date,
    /// `submitted`, `approved` or `rejected`.
    pub status: &'static str,
    /// Why it was rejected.
    pub reason: Option<String>,
    /// The person's messages inside the window.
    pub messages: usize,
    /// Their cost, in US dollars.
    pub cost: String,
}

/// A new submit: your messages since your last submit.
#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct NewSubmit {
    /// The last day to submit, `YYYY-MM-DD`. Default today.
    #[serde(default)]
    pub to: Option<String>,
}

/// Why a submission is rejected.
#[derive(Debug, Deserialize, ToSchema)]
pub struct Rejection {
    pub reason: String,
}

/// What a list is cut by.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SubmissionsQuery {
    /// One person, by email.
    pub person: Option<String>,
    /// The date up to which the log is read. Default today.
    pub as_of: Option<String>,
}

/// A request to everyone to submit by a day.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SubmitRequestView {
    pub id: Uuid,
    pub requested_by: String,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub requested_at: OffsetDateTime,
    #[serde(with = "crate::dates::day")]
    #[schema(value_type = String, format = Date)]
    pub until: Date,
    pub message: Option<String>,
    /// Whether you have submitted up to the request's day.
    pub done: bool,
}

/// A new request.
#[derive(Debug, Deserialize, ToSchema)]
pub struct NewSubmitRequest {
    /// The last day to submit by, `YYYY-MM-DD`.
    pub until: String,
    /// A note for the email, such as the month being closed.
    #[serde(default)]
    pub message: Option<String>,
}

fn status_word(status: &SubmissionStatus) -> (&'static str, Option<String>) {
    match status {
        SubmissionStatus::Submitted => ("submitted", None),
        SubmissionStatus::Approved => ("approved", None),
        SubmissionStatus::Rejected { reason } => ("rejected", Some(reason.clone())),
    }
}

/// The view of a submission with the fold's figures.
#[must_use]
pub fn view(fold: &Fold, submission: &Submission) -> SubmissionView {
    let mut messages = std::collections::BTreeSet::new();
    let mut cost = tokencontroller_core::Money::zero(fold.currency());
    for line in fold
        .lines()
        .iter()
        .filter(|line| submission.covers(&line.person, line.at))
    {
        messages.insert((line.session_id.clone(), line.message_id.clone()));
        cost = cost.checked_add(line.amount).unwrap_or(cost);
    }
    let (status, reason) = status_word(&submission.status);
    SubmissionView {
        id: submission.id.to_string(),
        person: submission.person.to_string(),
        from: submission.window.from,
        to: submission.window.to,
        status,
        reason,
        messages: messages.len(),
        cost: json::money(cost),
    }
}

/// Whether the caller may see a submission: their own, or one with a
/// message in a filter they view, or an empty one of a person whose
/// messages they could view.
fn may_see(fold: &Fold, caller: &TagValue, submission: &Submission) -> bool {
    if submission.person == *caller || access::is_org_manager(fold, caller) {
        return true;
    }
    fold.lines()
        .iter()
        .filter(|line| submission.covers(&line.person, line.at))
        .any(|line| access::can_view(fold, caller, &line.tags))
}

/// The first day a person's next submit covers: the day after their last
/// submit, or the day of their earliest message, or today.
#[must_use]
pub fn next_window_start(fold: &Fold, person: &TagValue, today: Date) -> Date {
    // A rejected submission goes back to its person: its window is open
    // again, so it does not move the start.
    let last = fold
        .submissions()
        .iter()
        .filter(|submission| submission.person == *person)
        .filter(|submission| !matches!(submission.status, SubmissionStatus::Rejected { .. }))
        .map(|submission| submission.window.to)
        .max();
    match last {
        Some(last) => last.next_day().unwrap_or(last),
        None => fold
            .messages()
            .filter(|message| message.value.person == *person)
            .map(|message| date_of(message.value.at))
            .min()
            .unwrap_or(today)
            .min(today),
    }
}

/// Submissions you may see: your own, and those with messages inside a
/// filter you view.
#[utoipa::path(
    get,
    path = "/submissions",
    tag = "submissions",
    params(SubmissionsQuery),
    responses((status = OK, body = Vec<SubmissionView>)),
)]
async fn list_submissions(
    State(state): State<AppState>,
    member: Member,
    Query(query): Query<SubmissionsQuery>,
) -> Result<Json<Vec<SubmissionView>>, ApiError> {
    let log = Log::load(&state, member.organization_id).await?;
    let fold = log.fold(crate::dates::as_of(query.as_of.as_deref())?)?;
    let caller = member.person()?;
    let mut views: Vec<SubmissionView> = fold
        .submissions()
        .iter()
        .filter(|submission| {
            query
                .person
                .as_deref()
                .is_none_or(|person| person.eq_ignore_ascii_case(submission.person.as_str()))
        })
        .filter(|submission| may_see(&fold, &caller, submission))
        .map(|submission| view(&fold, submission))
        .collect();
    views.sort_by(|a, b| b.to.cmp(&a.to).then(a.person.cmp(&b.person)));
    Ok(Json(views))
}

/// Submit your messages since your last submit, up to today. An empty
/// submit says there was nothing to add.
#[utoipa::path(
    post,
    path = "/submissions",
    tag = "submissions",
    request_body = NewSubmit,
    responses(
        (status = CREATED, body = SubmissionView),
        (status = BAD_REQUEST, body = crate::error::ErrorBody),
    ),
)]
async fn submit(
    State(state): State<AppState>,
    member: Member,
    body: Option<Json<NewSubmit>>,
) -> Result<(StatusCode, Json<SubmissionView>), ApiError> {
    member.require_person()?;
    let request = body.map(|Json(request)| request).unwrap_or_default();
    let today = date_of(OffsetDateTime::now_utc());
    let to = crate::dates::parse_optional(request.to.as_deref())?.unwrap_or(today);
    if to > today {
        return Err(ApiError::BadRequest(
            "a submit ends today at the latest".to_owned(),
        ));
    }
    let log = Log::load(&state, member.organization_id).await?;
    let fold = log.fold(OffsetDateTime::now_utc())?;
    let person = member.person()?;
    let from = next_window_start(&fold, &person, to);
    if from > to {
        return Err(ApiError::Conflict(format!(
            "you already submitted up to {}",
            crate::dates::text(from.previous_day().unwrap_or(from))
        )));
    }
    let window =
        DateRange::new(from, to).map_err(|error| ApiError::BadRequest(error.to_string()))?;
    let event = events::event(Record::Submit(Submit {
        person: person.clone(),
        window,
    }))?
    .by(person.clone());
    events::write(&state, &log, std::slice::from_ref(&event)).await?;
    let submission = Submission {
        id: event.id.clone(),
        person,
        window,
        status: SubmissionStatus::Submitted,
    };
    Ok((StatusCode::CREATED, Json(view(&fold, &submission))))
}

async fn decide(
    state: &AppState,
    member: &Member,
    id: &str,
    decision: Decision,
) -> Result<Json<SubmissionView>, ApiError> {
    member.require_session()?;
    let submission_id = EventId::new(id).map_err(|_| ApiError::NotFound)?;
    let log = Log::load(state, member.organization_id).await?;
    let fold = log.fold(OffsetDateTime::now_utc())?;
    let caller = member.person()?;
    let submission = fold
        .submissions()
        .iter()
        .find(|submission| submission.id == submission_id)
        .ok_or(ApiError::NotFound)?
        .clone();
    if submission.person == caller {
        return Err(ApiError::NoCapability(
            "nobody approves their own submission".to_owned(),
        ));
    }
    let lines: Vec<_> = fold
        .lines()
        .iter()
        .filter(|line| submission.covers(&line.person, line.at))
        .collect();
    let allowed = if lines.is_empty() {
        access::is_org_manager(&fold, &caller)
    } else {
        lines
            .iter()
            .any(|line| access::covers(&fold, &caller, Capability::Approve, &line.tags))
    };
    if !allowed {
        return Err(ApiError::NoCapability(
            "this needs approve on a filter the submission's messages are in".to_owned(),
        ));
    }
    let event = events::event(Record::Approval(Approval {
        submission: submission_id,
        decision: decision.clone(),
    }))?
    .by(caller);
    events::write(state, &log, std::slice::from_ref(&event)).await?;
    let decided = Submission {
        status: match decision {
            Decision::Approved => SubmissionStatus::Approved,
            Decision::Rejected { reason } => SubmissionStatus::Rejected { reason },
        },
        ..submission
    };
    Ok(Json(view(&fold, &decided)))
}

/// Approve a submission. Needs approve on a filter its messages are in,
/// never your own.
#[utoipa::path(
    post,
    path = "/submissions/{id}/approve",
    tag = "submissions",
    params(("id" = String, Path, description = "The submit event's id")),
    responses(
        (status = OK, body = SubmissionView),
        (status = FORBIDDEN, body = crate::error::ErrorBody),
        (status = NOT_FOUND, body = crate::error::ErrorBody),
    ),
)]
async fn approve(
    State(state): State<AppState>,
    member: Member,
    Path(id): Path<String>,
) -> Result<Json<SubmissionView>, ApiError> {
    decide(&state, &member, &id, Decision::Approved).await
}

/// Reject a submission with a reason. It goes back to its person's check
/// list.
#[utoipa::path(
    post,
    path = "/submissions/{id}/reject",
    tag = "submissions",
    params(("id" = String, Path, description = "The submit event's id")),
    request_body = Rejection,
    responses(
        (status = OK, body = SubmissionView),
        (status = BAD_REQUEST, body = crate::error::ErrorBody),
        (status = FORBIDDEN, body = crate::error::ErrorBody),
        (status = NOT_FOUND, body = crate::error::ErrorBody),
    ),
)]
async fn reject(
    State(state): State<AppState>,
    member: Member,
    Path(id): Path<String>,
    Json(request): Json<Rejection>,
) -> Result<Json<SubmissionView>, ApiError> {
    let reason = request.reason.trim();
    if reason.is_empty() || reason.chars().count() > 1_000 {
        return Err(ApiError::BadRequest(
            "a rejection needs a reason of 1 to 1,000 characters".to_owned(),
        ));
    }
    decide(
        &state,
        &member,
        &id,
        Decision::Rejected {
            reason: reason.to_owned(),
        },
    )
    .await
}

/// Whether a person has submitted up to a day.
fn submitted_until(fold: &Fold, person: &TagValue, day: Date) -> bool {
    fold.submissions()
        .iter()
        .filter(|submission| submission.person == *person)
        .filter(|submission| !matches!(submission.status, SubmissionStatus::Rejected { .. }))
        .any(|submission| submission.window.to >= day)
}

/// The open requests to submit: those whose day is today or later, with
/// whether you are done.
#[utoipa::path(
    get,
    path = "/submit-requests",
    tag = "submissions",
    responses((status = OK, body = Vec<SubmitRequestView>)),
)]
async fn list_submit_requests(
    State(state): State<AppState>,
    member: Member,
) -> Result<Json<Vec<SubmitRequestView>>, ApiError> {
    let log = Log::load(&state, member.organization_id).await?;
    let fold = log.fold(OffsetDateTime::now_utc())?;
    let person = member.person()?;
    let rows = sqlx::query!(
        r#"SELECT r.id, p.email AS requested_by, r.requested_at, r.until_day, r.message
           FROM submit_requests r JOIN people p ON p.id = r.requested_by
           WHERE r.organization_id = $1 AND r.until_day >= current_date - 7
           ORDER BY r.requested_at DESC"#,
        member.organization_id
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|row| SubmitRequestView {
                id: row.id,
                requested_by: row.requested_by,
                requested_at: row.requested_at,
                until: row.until_day,
                message: Some(row.message).filter(|message| !message.is_empty()),
                done: submitted_until(&fold, &person, row.until_day),
            })
            .collect(),
    ))
}

/// Ask everyone to submit by a day, the way finance asks for timesheets
/// before month end. Each person who has not submitted gets one email.
/// Org managers only.
#[utoipa::path(
    post,
    path = "/submit-requests",
    tag = "submissions",
    request_body = NewSubmitRequest,
    responses(
        (status = CREATED, body = SubmitRequestView),
        (status = BAD_REQUEST, body = crate::error::ErrorBody),
        (status = FORBIDDEN, body = crate::error::ErrorBody),
    ),
)]
async fn add_submit_request(
    State(state): State<AppState>,
    member: Member,
    Json(request): Json<NewSubmitRequest>,
) -> Result<(StatusCode, Json<SubmitRequestView>), ApiError> {
    member.require_session()?;
    let until = crate::dates::parse(&request.until)?;
    let message = request
        .message
        .map(|message| message.trim().to_owned())
        .filter(|message| !message.is_empty());
    if message
        .as_ref()
        .is_some_and(|message| message.chars().count() > 500)
    {
        return Err(ApiError::BadRequest(
            "the note has at most 500 characters".to_owned(),
        ));
    }
    let log = Log::load(&state, member.organization_id).await?;
    let fold = log.fold(OffsetDateTime::now_utc())?;
    let person = member.person()?;
    access::require_org_manager(&fold, &person)?;
    let row = sqlx::query!(
        r#"INSERT INTO submit_requests (organization_id, requested_by, until_day, message)
           VALUES ($1, $2, $3, $4) RETURNING id, requested_at"#,
        member.organization_id,
        member.person_id,
        until,
        message.clone().unwrap_or_default()
    )
    .fetch_one(&state.db)
    .await?;
    crate::organizations::record_audit_event(
        &state,
        member.organization_id,
        Some(member.person_id),
        "submit_request.added",
        Some(row.id),
    );
    if let Err(error) = send_reminders(&state).await {
        tracing::warn!("submit reminders: {error:#}");
    }
    Ok((
        StatusCode::CREATED,
        Json(SubmitRequestView {
            id: row.id,
            requested_by: person.to_string(),
            requested_at: row.requested_at,
            until,
            message,
            done: submitted_until(&fold, &person, until),
        }),
    ))
}

/// Mails everyone who has not submitted up to an open request's day and
/// was not mailed for it yet. Returns how many mails went out.
///
/// # Errors
///
/// Fails if a query fails; a mail that fails is logged and tried again on
/// the next run.
pub async fn send_reminders(state: &AppState) -> anyhow::Result<usize> {
    let requests = sqlx::query!(
        r#"SELECT r.id, r.organization_id, r.until_day, r.message,
                  coalesce(nullif(p.name, ''), p.email) AS "requester!", o.name AS organization
           FROM submit_requests r
           JOIN people p ON p.id = r.requested_by
           JOIN organizations o ON o.id = r.organization_id
           WHERE r.until_day >= current_date"#
    )
    .fetch_all(&state.db)
    .await?;
    let mut sent = 0_usize;
    for request in requests {
        let log = Log::load(state, request.organization_id).await?;
        let fold = log.fold(OffsetDateTime::now_utc())?;
        let members = sqlx::query!(
            r#"SELECT id, email, name FROM people
               WHERE organization_id = $1 AND removed_at IS NULL AND joined_at IS NOT NULL
                 AND id NOT IN (SELECT person_id FROM submit_reminders WHERE request_id = $2)"#,
            request.organization_id,
            request.id
        )
        .fetch_all(&state.db)
        .await?;
        for member in members {
            let person = events::person(&member.email)?;
            if submitted_until(&fold, &person, request.until_day) {
                sqlx::query!(
                    "INSERT INTO submit_reminders (request_id, person_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
                    request.id,
                    member.id
                )
                .execute(&state.db)
                .await?;
                continue;
            }
            let logo = mail::logo_url(&state.config.public_url);
            let note = if request.message.trim().is_empty() {
                String::new()
            } else {
                format!(" {}", request.message.trim())
            };
            let name = if member.name.is_empty() {
                member.email.clone()
            } else {
                member.name.clone()
            };
            let until = crate::dates::text(request.until_day);
            let values = [
                ("logo_url", logo.as_str()),
                ("name", name.as_str()),
                ("requester", request.requester.as_str()),
                ("organization", request.organization.as_str()),
                ("until", until.as_str()),
                ("note", note.as_str()),
                ("app_url", state.config.public_url.as_str()),
                ("host", mail::host_of(&state.config.public_url)),
            ];
            let reminder = Mail {
                kind: "submit_reminder",
                to: member.email.clone(),
                subject: format!("Please submit your sessions by {until}"),
                text: render(REMINDER_TEXT, &values, false),
                html: render(REMINDER_HTML, &values, true),
            };
            match mail::send(&state.db, &state.config, reminder).await {
                Ok(mail::Delivery::Sent) => {
                    sqlx::query!(
                        "INSERT INTO submit_reminders (request_id, person_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
                        request.id,
                        member.id
                    )
                    .execute(&state.db)
                    .await?;
                    sent = sent.saturating_add(1);
                }
                // Without a mail server the reminder waits; nothing is kept.
                Ok(mail::Delivery::NoMailServer) => return Ok(sent),
                Err(error) => tracing::warn!("submit reminder to {}: {error:#}", member.email),
            }
        }
    }
    Ok(sent)
}

/// The job: mails the reminders that are due.
///
/// # Errors
///
/// As [`send_reminders`].
pub async fn run_job(state: AppState) -> anyhow::Result<String> {
    let sent = send_reminders(&state).await?;
    Ok(format!("{sent} reminders sent"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::{date, datetime};
    use tokencontroller_core::{Currency, Event};

    #[test]
    fn the_next_window_starts_after_the_last_submit() {
        let mara = TagValue::new("mara").unwrap();
        let events = vec![
            Event::new(
                "sub_1",
                datetime!(2026-03-15 10:00 UTC),
                Record::Submit(Submit {
                    person: mara.clone(),
                    window: DateRange::new(date!(2026 - 03 - 01), date!(2026 - 03 - 14)).unwrap(),
                }),
            )
            .unwrap(),
        ];
        let fold = Fold::fold(&events, datetime!(2026-04-01 00:00 UTC), Currency::USD).unwrap();
        assert_eq!(
            next_window_start(&fold, &mara, date!(2026 - 03 - 31)),
            date!(2026 - 03 - 15)
        );
        let tom = TagValue::new("tom").unwrap();
        assert_eq!(
            next_window_start(&fold, &tom, date!(2026 - 03 - 31)),
            date!(2026 - 03 - 31)
        );
        assert!(submitted_until(&fold, &mara, date!(2026 - 03 - 14)));
        assert!(!submitted_until(&fold, &mara, date!(2026 - 03 - 15)));
        let submission = &fold.submissions()[0];
        let view = view(&fold, submission);
        assert_eq!(view.status, "submitted");
        assert_eq!(view.messages, 0);
        assert_eq!(view.cost, "0.00");
        assert!(may_see(&fold, &mara, submission));
        assert!(!may_see(&fold, &tom, submission));
    }
}
