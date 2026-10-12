//! Feedback reports from the web app's feedback button: a bug, an idea or a
//! question, an optional screenshot, and diagnostics. The docs list exactly
//! what a report contains (Security and privacy > What a feedback report
//! contains); change both together.
//!
//! The operators read every report. An instance without operators (a
//! self-hosted one) shows each organization's reports to its controllers.
//! Notices by mail carry a link and the kind, never the report.

use std::sync::OnceLock;
use std::time::Duration;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS};
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use lettre::message::Mailbox;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use sqlx::types::Json as JsonColumn;
use time::OffsetDateTime;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::admin::{self, CustomerLink, Reviewer, Scope};
use super::mail::{self, Outgoing};
use super::web::Limit;
use crate::error::{ApiError, ErrorBody};
use crate::hosted::Hosted;
use crate::sign_in::Member;

/// The largest screenshot, decoded.
pub const MAX_SCREENSHOT_BYTES: usize = 3 * 1024 * 1024;

/// The largest request: a screenshot grows by a third in base64.
pub const MAX_REQUEST_BYTES: usize = 5 * 1024 * 1024;

/// The longest report text.
const MAX_TEXT: usize = 5_000;

/// Reports per person per hour.
const REPORTS_PER_HOUR: usize = 10;

/// Error messages and request IDs kept per report.
const MAX_RECENT: usize = 10;

/// What a report is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Bug,
    Idea,
    Question,
}

impl Kind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Bug => "bug",
            Self::Idea => "idea",
            Self::Question => "question",
        }
    }

    fn parse(value: &str) -> Result<Self, ApiError> {
        match value {
            "bug" => Ok(Self::Bug),
            "idea" => Ok(Self::Idea),
            "question" => Ok(Self::Question),
            other => Err(ApiError::Internal(anyhow::anyhow!(
                "unknown report kind {other:?}"
            ))),
        }
    }
}

/// Where a report stands, as the person who sent it sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Waiting for us.
    Received,
    /// We answered.
    Answered,
    /// The fix shipped, in the version `fixed_in` names.
    Fixed,
}

impl Status {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Received => "received",
            Self::Answered => "answered",
            Self::Fixed => "fixed",
        }
    }

    fn parse(value: &str) -> Result<Self, ApiError> {
        match value {
            "received" => Ok(Self::Received),
            "answered" => Ok(Self::Answered),
            "fixed" => Ok(Self::Fixed),
            other => Err(ApiError::Internal(anyhow::anyhow!(
                "unknown report status {other:?}"
            ))),
        }
    }
}

/// Sent with every report, and shown to the person before they send it.
/// Nothing else rides along: unknown fields are refused.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Diagnostics {
    /// The page's path, such as `/reports/cost-sheet`. Anything after `?` or
    /// `#` is dropped.
    #[serde(default)]
    pub page: String,
    #[serde(default)]
    pub app_version: String,
    /// The browser's user agent.
    #[serde(default)]
    pub browser: String,
    /// Such as `1440x900`.
    #[serde(default)]
    pub screen: String,
    /// The last error messages the app showed, at most 10.
    #[serde(default)]
    pub errors: Vec<String>,
    /// The `x-request-id` of the last requests to the service, at most 10.
    #[serde(default)]
    pub request_ids: Vec<String>,
}

fn short(value: &str, max: usize, what: &str) -> Result<String, ApiError> {
    let value = value.trim();
    if value.chars().count() > max {
        return Err(ApiError::BadRequest(format!(
            "The {what} is longer than {max} characters."
        )));
    }
    Ok(value.to_owned())
}

fn recent(values: &[String], max_each: usize, what: &str) -> Result<Vec<String>, ApiError> {
    let start = values.len().saturating_sub(MAX_RECENT);
    values
        .get(start..)
        .unwrap_or_default()
        .iter()
        .map(|value| short(value, max_each, what))
        .collect()
}

impl Diagnostics {
    fn cleaned(&self) -> Result<Self, ApiError> {
        let page = self
            .page
            .split(['?', '#'])
            .next()
            .unwrap_or_default()
            .trim();
        if !page.is_empty() && !page.starts_with('/') {
            return Err(ApiError::BadRequest(
                "The page is a path such as /reports.".to_owned(),
            ));
        }
        Ok(Self {
            page: short(page, 300, "page")?,
            app_version: short(&self.app_version, 50, "app version")?,
            browser: short(&self.browser, 300, "browser")?,
            screen: short(&self.screen, 20, "screen size")?,
            errors: recent(&self.errors, 500, "error message")?,
            request_ids: recent(&self.request_ids, 100, "request ID")?,
        })
    }
}

/// A screenshot the person saw, and blurred where they wanted, before
/// sending.
#[derive(Debug, Deserialize, ToSchema)]
pub struct Screenshot {
    /// `image/png` or `image/jpeg`.
    #[serde(rename = "type")]
    pub content_type: String,
    /// The image in base64, at most 3 MB decoded.
    pub data: String,
}

impl Screenshot {
    fn decoded(&self) -> Result<(&'static str, Vec<u8>), ApiError> {
        let refuse = || {
            ApiError::BadRequest(
                "The screenshot is a PNG or JPEG image in base64, at most 3 MB.".to_owned(),
            )
        };
        let bytes = BASE64.decode(self.data.trim()).map_err(|_| refuse())?;
        if bytes.len() > MAX_SCREENSHOT_BYTES {
            return Err(refuse());
        }
        let content_type = match self.content_type.as_str() {
            "image/png" if bytes.starts_with(b"\x89PNG\r\n\x1a\n") => "image/png",
            "image/jpeg" if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) => "image/jpeg",
            _ => return Err(refuse()),
        };
        Ok((content_type, bytes))
    }
}

/// A report from the feedback button.
#[derive(Debug, Deserialize, ToSchema)]
pub struct NewReport {
    pub kind: Kind,
    pub text: String,
    /// Off unless the person turned it on.
    pub screenshot: Option<Screenshot>,
    #[serde(default)]
    pub diagnostics: Diagnostics,
}

/// A report as its sender sees it.
#[derive(Debug, Serialize, ToSchema)]
pub struct ReportView {
    pub id: Uuid,
    pub kind: Kind,
    pub text: String,
    /// Fetch it from `/feedback/{id}/screenshot`. Screenshots are deleted
    /// after 90 days.
    pub has_screenshot: bool,
    pub diagnostics: Diagnostics,
    pub status: Status,
    pub answer: Option<String>,
    /// The version the fix shipped in.
    pub fixed_in: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

/// Who sent a report.
#[derive(Debug, Serialize, ToSchema)]
pub struct Reporter {
    pub person_id: Uuid,
    pub name: String,
    pub email: String,
}

/// A report as the people who handle it see it.
#[derive(Debug, Serialize, ToSchema)]
pub struct ReceivedReport {
    pub report: ReportView,
    pub organization: CustomerLink,
    pub reporter: Reporter,
}

fn per_person() -> &'static Limit {
    static LIMIT: OnceLock<Limit> = OnceLock::new();
    LIMIT.get_or_init(|| Limit::new(REPORTS_PER_HOUR, Duration::from_hours(1)))
}

/// Reports `scope` may read, with `status` or any, or just report `only`.
pub(super) async fn received(
    db: &PgPool,
    scope: Scope,
    status: Option<Status>,
    only: Option<Uuid>,
) -> Result<Vec<ReceivedReport>, ApiError> {
    let rows = sqlx::query!(
        r#"SELECT f.id, f.kind, f.body, f.screenshot IS NOT NULL AS "has_screenshot!",
                  f.diagnostics AS "diagnostics: JsonColumn<Diagnostics>", f.status, f.answer,
                  f.fixed_in, f.created_at, f.updated_at,
                  o.id AS organization_id, o.name AS organization_name,
                  p.id AS person_id, p.name AS person_name, p.email
           FROM feedback_reports f
           JOIN organizations o ON o.id = f.organization_id
           JOIN people p ON p.id = f.person_id
           WHERE ($1::uuid IS NULL OR f.organization_id = $1)
             AND ($2::text IS NULL OR f.status = $2)
             AND ($3::uuid IS NULL OR f.id = $3)
           ORDER BY f.created_at DESC, f.id
           LIMIT 200"#,
        scope.organization(),
        status.map(Status::as_str),
        only,
    )
    .fetch_all(db)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(ReceivedReport {
                report: ReportView {
                    id: row.id,
                    kind: Kind::parse(&row.kind)?,
                    text: row.body,
                    has_screenshot: row.has_screenshot,
                    diagnostics: row.diagnostics.0,
                    status: Status::parse(&row.status)?,
                    answer: row.answer,
                    fixed_in: row.fixed_in,
                    created_at: row.created_at,
                    updated_at: row.updated_at,
                },
                organization: CustomerLink {
                    id: row.organization_id,
                    name: row.organization_name,
                },
                reporter: Reporter {
                    person_id: row.person_id,
                    name: row.person_name,
                    email: row.email,
                },
            })
        })
        .collect()
}

async fn own(
    db: &PgPool,
    person_id: Uuid,
    only: Option<Uuid>,
) -> Result<Vec<ReportView>, ApiError> {
    let rows = sqlx::query!(
        r#"SELECT id, kind, body, screenshot IS NOT NULL AS "has_screenshot!",
                  diagnostics AS "diagnostics: JsonColumn<Diagnostics>", status, answer, fixed_in,
                  created_at, updated_at
           FROM feedback_reports
           WHERE person_id = $1 AND ($2::uuid IS NULL OR id = $2)
           ORDER BY created_at DESC, id
           LIMIT 200"#,
        person_id,
        only,
    )
    .fetch_all(db)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(ReportView {
                id: row.id,
                kind: Kind::parse(&row.kind)?,
                text: row.body,
                has_screenshot: row.has_screenshot,
                diagnostics: row.diagnostics.0,
                status: Status::parse(&row.status)?,
                answer: row.answer,
                fixed_in: row.fixed_in,
                created_at: row.created_at,
                updated_at: row.updated_at,
            })
        })
        .collect()
}

/// Sends a report from the feedback button. A signed-in person only.
#[utoipa::path(
    post,
    path = "/feedback",
    tag = "feedback",
    request_body = NewReport,
    responses(
        (status = CREATED, body = ReportView),
        (status = BAD_REQUEST, body = ErrorBody),
        (status = TOO_MANY_REQUESTS, body = ErrorBody),
    ),
)]
pub async fn send_report(
    State(state): State<Hosted>,
    member: Member,
    Json(report): Json<NewReport>,
) -> Result<(StatusCode, Json<ReportView>), ApiError> {
    member.require_session()?;
    let text = report.text.trim();
    if text.is_empty() {
        return Err(ApiError::BadRequest(
            "Write what happened first.".to_owned(),
        ));
    }
    if text.chars().count() > MAX_TEXT {
        return Err(ApiError::BadRequest(format!(
            "The text is longer than {MAX_TEXT} characters."
        )));
    }
    let diagnostics = report.diagnostics.cleaned()?;
    let screenshot = report
        .screenshot
        .as_ref()
        .map(Screenshot::decoded)
        .transpose()?;
    if !per_person().allow(&member.person_id.to_string()) {
        return Err(ApiError::TooManyRequests(
            "That is a lot of reports at once. Try again in an hour.".to_owned(),
        ));
    }
    let (screenshot_type, screenshot) = screenshot.unzip();
    let id = sqlx::query_scalar!(
        "INSERT INTO feedback_reports
           (organization_id, person_id, kind, body, screenshot, screenshot_type, diagnostics)
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
        member.organization_id,
        member.person_id,
        report.kind.as_str(),
        text,
        screenshot,
        screenshot_type,
        JsonColumn(&diagnostics) as _,
    )
    .fetch_one(&state.db)
    .await?;
    super::wake();
    let view = own(&state.db, member.person_id, Some(id))
        .await?
        .into_iter()
        .next()
        .ok_or(ApiError::NotFound)?;
    Ok((StatusCode::CREATED, Json(view)))
}

/// The reports the signed-in person sent, newest first, with where each
/// stands.
#[utoipa::path(
    get,
    path = "/feedback",
    tag = "feedback",
    responses((status = OK, body = Vec<ReportView>)),
)]
pub async fn my_reports(
    State(state): State<Hosted>,
    member: Member,
) -> Result<Json<Vec<ReportView>>, ApiError> {
    Ok(Json(own(&state.db, member.person_id, None).await?))
}

/// A report's screenshot, for its sender and the people who handle it.
#[utoipa::path(
    get,
    path = "/feedback/{id}/screenshot",
    tag = "feedback",
    params(("id" = Uuid, Path)),
    responses(
        (status = OK, description = "The PNG or JPEG image", content_type = "image/png"),
        (status = NOT_FOUND, body = ErrorBody),
    ),
)]
pub async fn screenshot(
    State(state): State<Hosted>,
    member: Member,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let row = sqlx::query!(
        "SELECT organization_id, person_id, screenshot, screenshot_type FROM feedback_reports WHERE id = $1",
        id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;
    let allowed = row.person_id == member.person_id
        || admin::review_scope(&state, &member)
            .await?
            .is_some_and(|scope| scope.covers(row.organization_id));
    let (true, Some(bytes), Some(content_type)) = (allowed, row.screenshot, row.screenshot_type)
    else {
        return Err(ApiError::NotFound);
    };
    Ok((
        [
            (CONTENT_TYPE, content_type),
            (CACHE_CONTROL, "private, no-store".to_owned()),
            // A browser never reads the image as anything else.
            (X_CONTENT_TYPE_OPTIONS, "nosniff".to_owned()),
        ],
        bytes,
    )
        .into_response())
}

/// Which reports to list.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ReportFilter {
    /// `received`, `answered` or `fixed`; without it, all.
    pub status: Option<Status>,
}

/// Feedback reports, newest first: every organization's for the operators,
/// the own organization's for a controller on an instance without operators.
#[utoipa::path(
    get,
    path = "/admin/feedback",
    tag = "admin",
    params(ReportFilter),
    responses(
        (status = OK, body = Vec<ReceivedReport>),
        (status = NOT_FOUND, description = "Not someone who handles reports", body = ErrorBody),
    ),
)]
pub async fn list_received(
    State(state): State<Hosted>,
    reviewer: Reviewer,
    Query(filter): Query<ReportFilter>,
) -> Result<Json<Vec<ReceivedReport>>, ApiError> {
    Ok(Json(
        received(&state.db, reviewer.scope, filter.status, None).await?,
    ))
}

async fn one_received(db: &PgPool, scope: Scope, id: Uuid) -> Result<ReceivedReport, ApiError> {
    received(db, scope, None, Some(id))
        .await?
        .into_iter()
        .next()
        .ok_or(ApiError::NotFound)
}

/// One feedback report.
#[utoipa::path(
    get,
    path = "/admin/feedback/{id}",
    tag = "admin",
    params(("id" = Uuid, Path)),
    responses(
        (status = OK, body = ReceivedReport),
        (status = NOT_FOUND, body = ErrorBody),
    ),
)]
pub async fn get_received(
    State(state): State<Hosted>,
    reviewer: Reviewer,
    Path(id): Path<Uuid>,
) -> Result<Json<ReceivedReport>, ApiError> {
    Ok(Json(one_received(&state.db, reviewer.scope, id).await?))
}

/// An answer to a report, or news on it. Fields left out stay as they are.
#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct ReportChange {
    /// Without it, an answer makes the report `answered` and a version
    /// makes it `fixed`.
    pub status: Option<Status>,
    /// Plain text the sender reads in the app.
    pub answer: Option<String>,
    /// The version the fix shipped in, such as `1.0.1`.
    pub fixed_in: Option<String>,
}

fn version(value: &str) -> Result<String, ApiError> {
    let value = value.trim();
    let parts: Vec<&str> = value.split('.').collect();
    let valid = parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty() && part.len() <= 5 && part.bytes().all(|b| b.is_ascii_digit())
        });
    if valid {
        Ok(value.to_owned())
    } else {
        Err(ApiError::BadRequest(
            "fixed_in is a version such as 1.0.1.".to_owned(),
        ))
    }
}

/// Answers a report or marks it fixed. The sender gets a notice by mail
/// that links to the app.
#[utoipa::path(
    patch,
    path = "/admin/feedback/{id}",
    tag = "admin",
    params(("id" = Uuid, Path)),
    request_body = ReportChange,
    responses(
        (status = OK, body = ReceivedReport),
        (status = BAD_REQUEST, body = ErrorBody),
        (status = NOT_FOUND, body = ErrorBody),
    ),
)]
pub async fn update_received(
    State(state): State<Hosted>,
    reviewer: Reviewer,
    Path(id): Path<Uuid>,
    Json(change): Json<ReportChange>,
) -> Result<Json<ReceivedReport>, ApiError> {
    reviewer.member.require_session()?;
    let current = one_received(&state.db, reviewer.scope, id).await?.report;
    let answer = change
        .answer
        .as_deref()
        .map(|answer| admin::text(answer, "answer"))
        .transpose()?;
    let fixed_in = change.fixed_in.as_deref().map(version).transpose()?;
    let status = change.status.unwrap_or(if fixed_in.is_some() {
        Status::Fixed
    } else if answer.is_some() && current.status == Status::Received {
        Status::Answered
    } else {
        current.status
    });
    if status == Status::Fixed && fixed_in.is_none() && current.fixed_in.is_none() {
        return Err(ApiError::BadRequest(
            "Say which version the fix shipped in, such as 1.0.1.".to_owned(),
        ));
    }
    let news = status != Status::Received
        && (status != current.status
            || answer.is_some() && answer != current.answer
            || fixed_in.is_some() && fixed_in != current.fixed_in);
    sqlx::query!(
        "UPDATE feedback_reports
         SET status = $2, answer = coalesce($3, answer), fixed_in = coalesce($4, fixed_in),
             updated_at = now(),
             reporter_notified_at = CASE WHEN $5 THEN NULL ELSE reporter_notified_at END
         WHERE id = $1",
        id,
        status.as_str(),
        answer,
        fixed_in,
        news,
    )
    .execute(&state.db)
    .await?;
    if news {
        super::wake();
    }
    Ok(Json(one_received(&state.db, reviewer.scope, id).await?))
}

/// Tells the people who handle reports about new ones, and senders about
/// answers and fixes. Each notice is a link and the kind, never the report.
///
/// # Errors
///
/// When the database fails. A notice that fails is tried again next run.
pub async fn notify(state: &Hosted) -> anyhow::Result<()> {
    let Some(smtp_url) = &state.config.smtp_url else {
        return Ok(());
    };
    let from = state
        .settings
        .customer_relations
        .support_from
        .clone()
        .unwrap_or_else(|| crate::mail::sender(&state.config));
    let notices = Notices { smtp_url, from };
    notify_reviewers(state, &notices).await?;
    notify_senders(state, &notices).await
}

struct Notices<'a> {
    smtp_url: &'a str,
    from: String,
}

impl Notices<'_> {
    async fn send(&self, to: &str, subject: &str, text: String) -> anyhow::Result<()> {
        let outgoing = Outgoing {
            from: self.from.clone(),
            to: to.parse::<Mailbox>()?,
            reply_to: None,
            subject: subject.to_owned(),
            text,
            message_id: None,
            references: Vec::new(),
            headers: Vec::new(),
        };
        mail::send_smtp(self.smtp_url, &outgoing).await
    }
}

async fn notify_reviewers(state: &Hosted, notices: &Notices<'_>) -> anyhow::Result<()> {
    let new = sqlx::query!(
        "SELECT id, organization_id, kind FROM feedback_reports
         WHERE reviewers_notified_at IS NULL ORDER BY created_at LIMIT 20"
    )
    .fetch_all(&state.db)
    .await?;
    let operators = &state.settings.customer_relations.operator_emails;
    for report in new {
        let reviewers = if operators.is_empty() {
            sqlx::query_scalar!(
                "SELECT email FROM people
                 WHERE organization_id = $1 AND role = 'controller' AND removed_at IS NULL",
                report.organization_id
            )
            .fetch_all(&state.db)
            .await?
        } else {
            operators.clone()
        };
        let text = format!(
            "A feedback report came in (kind: {}).\n\nOpen it here: {}/admin/feedback/{}\n\nThe report is only in the app.",
            report.kind, state.config.public_url, report.id
        );
        let mut all_sent = true;
        for reviewer in &reviewers {
            if let Err(error) = notices
                .send(reviewer, "Feedback: a new report", text.clone())
                .await
            {
                tracing::warn!(report = %report.id, "could not send a feedback notice: {error:#}");
                all_sent = false;
            }
        }
        if all_sent {
            sqlx::query!(
                "UPDATE feedback_reports SET reviewers_notified_at = now() WHERE id = $1",
                report.id
            )
            .execute(&state.db)
            .await?;
        }
    }
    Ok(())
}

async fn notify_senders(state: &Hosted, notices: &Notices<'_>) -> anyhow::Result<()> {
    let news = sqlx::query!(
        "SELECT f.id, f.status, f.fixed_in, f.created_at, p.email
         FROM feedback_reports f JOIN people p ON p.id = f.person_id
         WHERE f.status <> 'received' AND f.reporter_notified_at IS NULL AND p.removed_at IS NULL
         ORDER BY f.updated_at LIMIT 20"
    )
    .fetch_all(&state.db)
    .await?;
    for report in news {
        let date = report.created_at.date();
        let (subject, what) = match (report.status.as_str(), report.fixed_in.as_deref()) {
            ("fixed", Some(version)) => (
                "Your feedback report: fixed",
                format!("What you reported on {date} is fixed in version {version}."),
            ),
            _ => (
                "Your feedback report: answered",
                format!("Your feedback report from {date} has an answer."),
            ),
        };
        let text = format!(
            "{what}\n\nRead it in Token Controller: {}/feedback\n\n--\nToken Controller",
            state.config.public_url
        );
        match notices.send(&report.email, subject, text).await {
            Ok(()) => {
                sqlx::query!(
                    "UPDATE feedback_reports SET reporter_notified_at = now() WHERE id = $1",
                    report.id
                )
                .execute(&state.db)
                .await?;
            }
            Err(error) => {
                tracing::warn!(report = %report.id, "could not tell the sender: {error:#}");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_keep_the_path_and_the_last_ten() {
        let errors: Vec<String> = (0..12).map(|n| format!("error {n}")).collect();
        let cleaned = Diagnostics {
            page: "/reports/cost-sheet?client=acme#top".to_owned(),
            errors,
            ..Diagnostics::default()
        }
        .cleaned()
        .unwrap();
        assert_eq!(cleaned.page, "/reports/cost-sheet");
        assert_eq!(cleaned.errors.len(), 10);
        assert_eq!(cleaned.errors.first().unwrap(), "error 2");

        let full_url = Diagnostics {
            page: "https://app.example/reports".to_owned(),
            ..Diagnostics::default()
        };
        assert!(full_url.cleaned().is_err());
    }

    #[test]
    fn diagnostics_refuse_anything_else() {
        let extra = serde_json::json!({"page": "/", "request_body": "{\"prompt\": \"secret\"}"});
        assert!(serde_json::from_value::<Diagnostics>(extra).is_err());
    }

    #[test]
    fn a_screenshot_must_be_what_it_says() {
        let png = Screenshot {
            content_type: "image/png".to_owned(),
            data: BASE64.encode(b"\x89PNG\r\n\x1a\nrest"),
        };
        assert_eq!(png.decoded().unwrap().0, "image/png");
        let lie = Screenshot {
            content_type: "image/jpeg".to_owned(),
            data: png.data.clone(),
        };
        assert!(lie.decoded().is_err());
        let svg = Screenshot {
            content_type: "image/svg+xml".to_owned(),
            data: BASE64.encode(b"<svg/>"),
        };
        assert!(svg.decoded().is_err());
    }

    #[test]
    fn versions_have_three_numbers() {
        assert_eq!(version(" 1.0.1 ").unwrap(), "1.0.1");
        assert!(version("1.0").is_err());
        assert!(version("v1.0.1").is_err());
        assert!(version("1.0.1-beta").is_err());
    }
}
