//! The admin area for the people who run the instance (`TC_OPERATOR_EMAILS`):
//! the support inbox, the site assistant's questions and the newsletter.
//!
//! Everyone else gets 404, so the area does not show it exists. Operators
//! sign in like any member; a read token of theirs works on reads, so a
//! routine can read what they see.

use axum::Json;
use axum::extract::{FromRequestParts, Path, Query, State};
use axum::http::StatusCode;
use axum::http::request::Parts;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::support;
use crate::AppState;
use crate::error::{ApiError, ErrorBody};
use crate::organizations::Role;
use crate::sign_in::Member;

/// The longest reply, note or newsletter text the admin area takes.
pub const MAX_TEXT: usize = 20_000;

/// How many rows a list shows, newest first.
const LIST_LIMIT: i64 = 200;

/// Signs operator replies.
const OPERATOR_FOOTER: &str = "--\nToken Controller support";

/// A person who runs this instance: signed in, and their address is in
/// `TC_OPERATOR_EMAILS`.
#[derive(Debug, Clone)]
pub struct Operator {
    /// Their address, as notes they write are signed.
    pub email: String,
}

/// Who may read feedback reports, and whose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The operators read every organization's reports.
    All,
    /// Without operators, an organization's controllers read its own.
    Organization(Uuid),
}

impl Scope {
    /// Whether this scope covers a report of `organization_id`.
    #[must_use]
    pub fn covers(self, organization_id: Uuid) -> bool {
        match self {
            Self::All => true,
            Self::Organization(own) => own == organization_id,
        }
    }

    /// The organization to filter by, if any.
    #[must_use]
    pub const fn organization(self) -> Option<Uuid> {
        match self {
            Self::All => None,
            Self::Organization(own) => Some(own),
        }
    }
}

/// A person who reads feedback reports: an operator, or a controller when
/// the instance has no operators (a self-hosted instance).
#[derive(Debug, Clone)]
pub struct Reviewer {
    pub member: Member,
    pub scope: Scope,
}

async fn email_of(db: &PgPool, person_id: Uuid) -> Result<String, sqlx::Error> {
    sqlx::query_scalar!("SELECT email FROM people WHERE id = $1", person_id)
        .fetch_one(db)
        .await
}

/// What `member` may read of feedback reports, if anything.
///
/// # Errors
///
/// When the database fails.
pub async fn review_scope(state: &AppState, member: &Member) -> Result<Option<Scope>, ApiError> {
    let operators = &state.config.customer_relations.operator_emails;
    if operators.is_empty() {
        return Ok((member.role == Role::Controller)
            .then_some(Scope::Organization(member.organization_id)));
    }
    let email = email_of(&state.db, member.person_id).await?;
    Ok(operators.contains(&email).then_some(Scope::All))
}

impl FromRequestParts<AppState> for Operator {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let member = Member::from_request_parts(parts, state)
            .await
            .map_err(|_| ApiError::NotFound)?;
        let email = email_of(&state.db, member.person_id).await?;
        if state
            .config
            .customer_relations
            .operator_emails
            .contains(&email)
        {
            Ok(Self { email })
        } else {
            Err(ApiError::NotFound)
        }
    }
}

impl FromRequestParts<AppState> for Reviewer {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let member = Member::from_request_parts(parts, state)
            .await
            .map_err(|_| ApiError::NotFound)?;
        match review_scope(state, &member).await? {
            Some(scope) => Ok(Self { member, scope }),
            None => Err(ApiError::NotFound),
        }
    }
}

/// Text from a form: trimmed, not empty, not too long.
///
/// # Errors
///
/// [`ApiError::BadRequest`] naming `what`.
pub fn text(value: &str, what: &str) -> Result<String, ApiError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(ApiError::BadRequest(format!("Write the {what} first.")));
    }
    if value.chars().count() > MAX_TEXT {
        return Err(ApiError::BadRequest(format!(
            "The {what} is longer than {MAX_TEXT} characters."
        )));
    }
    Ok(value.to_owned())
}

/// What waits for the operators.
#[derive(Debug, Serialize, ToSchema)]
pub struct Overview {
    /// Support conversations that need a person.
    pub support_escalated: i64,
    /// Feedback reports nobody answered yet.
    pub feedback_received: i64,
    /// Site assistant questions of the last 7 days the docs did not answer.
    pub assistant_missed_7_days: i64,
    /// Confirmed newsletter subscribers.
    pub newsletter_subscribers: i64,
}

/// What waits for the operators. Answers 404 to everyone else, so the web
/// app can ask it to decide whether to show the admin area.
#[utoipa::path(
    get,
    path = "/admin",
    tag = "admin",
    responses(
        (status = OK, body = Overview),
        (status = NOT_FOUND, description = "Not an operator", body = ErrorBody),
    ),
)]
pub async fn overview(
    State(state): State<AppState>,
    _operator: Operator,
) -> Result<Json<Overview>, ApiError> {
    let row = sqlx::query!(
        r#"SELECT
             (SELECT count(*) FROM support_conversations WHERE state = 'escalated') AS "support!",
             (SELECT count(*) FROM feedback_reports WHERE status = 'received') AS "feedback!",
             (SELECT count(*) FROM assistant_questions
              WHERE NOT answered AND created_at > now() - interval '7 days') AS "missed!",
             (SELECT count(*) FROM newsletter_subscribers WHERE status = 'confirmed') AS "subscribers!""#
    )
    .fetch_one(&state.db)
    .await?;
    Ok(Json(Overview {
        support_escalated: row.support,
        feedback_received: row.feedback,
        assistant_missed_7_days: row.missed,
        newsletter_subscribers: row.subscribers,
    }))
}

// --- Support ---

const STATES: [&str; 4] = ["new", "answered", "escalated", "closed"];

/// Which conversations to list.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SupportFilter {
    /// `new`, `answered`, `escalated` or `closed`. Without it, every
    /// conversation that is not closed.
    pub state: Option<String>,
}

/// A support conversation in a list.
#[derive(Debug, Serialize, ToSchema)]
pub struct ConversationSummary {
    pub id: Uuid,
    /// `mail`, `contact_form` or `app`.
    pub channel: String,
    pub contact_email: String,
    pub contact_name: Option<String>,
    pub subject: String,
    /// `question`, `feature_request`, `bug`, `privacy_request`, `sales`,
    /// `automatic`, `spam` or `other`.
    pub kind: String,
    /// `new`, `answered`, `escalated` or `closed`.
    pub state: String,
    pub messages: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

/// One message of a conversation.
#[derive(Debug, Serialize, ToSchema)]
pub struct MessageView {
    pub id: Uuid,
    /// `customer`, `assistant` (the AI support answerer) or `operator`.
    pub author: String,
    pub body: String,
    /// Docs pages the answer links to.
    pub links: Vec<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    /// When our reply went out by mail; empty while it waits.
    #[serde(with = "time::serde::rfc3339::option")]
    pub sent_at: Option<OffsetDateTime>,
}

/// The organization a contact belongs to, if they are a member.
#[derive(Debug, Serialize, ToSchema)]
pub struct CustomerLink {
    pub id: Uuid,
    pub name: String,
}

/// A whole support conversation.
#[derive(Debug, Serialize, ToSchema)]
pub struct ConversationView {
    pub conversation: ConversationSummary,
    /// Why it went to the operators.
    pub escalation_reason: Option<String>,
    /// The answer the AI support answerer proposes; edit it before sending.
    pub suggested_answer: Option<String>,
    pub messages: Vec<MessageView>,
    /// The contact's organization, when their address is a member's.
    pub customer: Option<CustomerLink>,
}

/// Support conversations in `state`, or every one that is not closed; with
/// `emails`, only those whose contact is one of them, closed ones too.
pub(super) async fn conversations(
    db: &PgPool,
    state: Option<&str>,
    emails: Option<&[String]>,
) -> Result<Vec<ConversationSummary>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"SELECT c.id, c.channel, c.contact_email, c.contact_name, c.subject, c.kind, c.state,
                  c.created_at, c.updated_at,
                  (SELECT count(*) FROM support_messages m WHERE m.conversation_id = c.id) AS "messages!"
           FROM support_conversations c
           WHERE ($1::text IS NULL AND ($2::text[] IS NOT NULL OR c.state <> 'closed') OR c.state = $1)
             AND ($2::text[] IS NULL OR lower(c.contact_email) = ANY($2))
           ORDER BY c.updated_at DESC, c.id
           LIMIT $3"#,
        state,
        emails,
        LIST_LIMIT,
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| ConversationSummary {
            id: row.id,
            channel: row.channel,
            contact_email: row.contact_email,
            contact_name: row.contact_name,
            subject: row.subject,
            kind: row.kind,
            state: row.state,
            messages: row.messages,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
        .collect())
}

/// Support conversations, the ones that need a person first.
#[utoipa::path(
    get,
    path = "/admin/support",
    tag = "admin",
    params(SupportFilter),
    responses(
        (status = OK, body = Vec<ConversationSummary>),
        (status = BAD_REQUEST, body = ErrorBody),
        (status = NOT_FOUND, description = "Not an operator", body = ErrorBody),
    ),
)]
pub async fn list_support(
    State(state): State<AppState>,
    _operator: Operator,
    Query(filter): Query<SupportFilter>,
) -> Result<Json<Vec<ConversationSummary>>, ApiError> {
    if let Some(wanted) = filter.state.as_deref()
        && !STATES.contains(&wanted)
    {
        return Err(ApiError::BadRequest(
            "state is one of new, answered, escalated and closed.".to_owned(),
        ));
    }
    let mut list = conversations(&state.db, filter.state.as_deref(), None).await?;
    // Escalated first: those wait for a person.
    list.sort_by_key(|conversation| conversation.state != "escalated");
    Ok(Json(list))
}

async fn conversation(db: &PgPool, id: Uuid) -> Result<ConversationView, ApiError> {
    let row = sqlx::query!(
        r#"SELECT c.id, c.channel, c.contact_email, c.contact_name, c.subject, c.kind, c.state,
                  c.escalation_reason, c.suggested_answer, c.created_at, c.updated_at
           FROM support_conversations c WHERE c.id = $1"#,
        id
    )
    .fetch_optional(db)
    .await?
    .ok_or(ApiError::NotFound)?;
    let messages: Vec<MessageView> = sqlx::query!(
        "SELECT id, author, body, links, created_at, sent_at FROM support_messages
         WHERE conversation_id = $1 ORDER BY created_at, id",
        id
    )
    .fetch_all(db)
    .await?
    .into_iter()
    .map(|message| MessageView {
        id: message.id,
        author: message.author,
        body: message.body,
        links: message.links,
        created_at: message.created_at,
        sent_at: message.sent_at,
    })
    .collect();
    let customer = sqlx::query_as!(
        CustomerLink,
        "SELECT o.id, o.name FROM people p JOIN organizations o ON o.id = p.organization_id
         WHERE p.email = lower($1) AND p.removed_at IS NULL",
        row.contact_email
    )
    .fetch_optional(db)
    .await?;
    Ok(ConversationView {
        conversation: ConversationSummary {
            id: row.id,
            channel: row.channel,
            contact_email: row.contact_email,
            contact_name: row.contact_name,
            subject: row.subject,
            kind: row.kind,
            state: row.state,
            messages: i64::try_from(messages.len()).unwrap_or(i64::MAX),
            created_at: row.created_at,
            updated_at: row.updated_at,
        },
        escalation_reason: row.escalation_reason,
        suggested_answer: row.suggested_answer,
        messages,
        customer,
    })
}

/// One support conversation with every message.
#[utoipa::path(
    get,
    path = "/admin/support/{id}",
    tag = "admin",
    params(("id" = Uuid, Path)),
    responses(
        (status = OK, body = ConversationView),
        (status = NOT_FOUND, body = ErrorBody),
    ),
)]
pub async fn get_support(
    State(state): State<AppState>,
    _operator: Operator,
    Path(id): Path<Uuid>,
) -> Result<Json<ConversationView>, ApiError> {
    Ok(Json(conversation(&state.db, id).await?))
}

/// An operator's answer.
#[derive(Debug, Deserialize, ToSchema)]
pub struct Reply {
    /// Plain text. It goes out by mail, signed "Token Controller support".
    pub text: String,
}

/// Answers a conversation by hand. The mail goes out within a minute, and
/// the conversation then waits for the person.
#[utoipa::path(
    post,
    path = "/admin/support/{id}/replies",
    tag = "admin",
    params(("id" = Uuid, Path)),
    request_body = Reply,
    responses(
        (status = CREATED, body = ConversationView),
        (status = BAD_REQUEST, body = ErrorBody),
        (status = CONFLICT, description = "Support mail is off", body = ErrorBody),
        (status = NOT_FOUND, body = ErrorBody),
    ),
)]
pub async fn reply(
    State(state): State<AppState>,
    _operator: Operator,
    Path(id): Path<Uuid>,
    Json(reply): Json<Reply>,
) -> Result<(StatusCode, Json<ConversationView>), ApiError> {
    if state.config.customer_relations.support_from.is_none() || state.config.smtp_url.is_none() {
        return Err(ApiError::Conflict(
            "Support mail is off: set TC_SUPPORT_FROM and TC_SMTP_URL.".to_owned(),
        ));
    }
    let body = format!("{}\n\n{OPERATOR_FOOTER}", text(&reply.text, "reply")?);
    let exists = sqlx::query_scalar!("SELECT id FROM support_conversations WHERE id = $1", id)
        .fetch_optional(&state.db)
        .await?;
    if exists.is_none() {
        return Err(ApiError::NotFound);
    }
    support::add_reply(&state, id, "operator", &body, &[]).await?;
    sqlx::query!(
        "UPDATE support_conversations SET state = 'answered', updated_at = now() WHERE id = $1",
        id
    )
    .execute(&state.db)
    .await?;
    super::wake();
    Ok((
        StatusCode::CREATED,
        Json(conversation(&state.db, id).await?),
    ))
}

/// Closes a conversation: nothing left to do. A new mail from the person
/// opens it again.
#[utoipa::path(
    post,
    path = "/admin/support/{id}/close",
    tag = "admin",
    params(("id" = Uuid, Path)),
    responses(
        (status = OK, body = ConversationView),
        (status = NOT_FOUND, body = ErrorBody),
    ),
)]
pub async fn close(
    State(state): State<AppState>,
    _operator: Operator,
    Path(id): Path<Uuid>,
) -> Result<Json<ConversationView>, ApiError> {
    let closed = sqlx::query_scalar!(
        "UPDATE support_conversations SET state = 'closed', updated_at = now() WHERE id = $1 RETURNING id",
        id
    )
    .fetch_optional(&state.db)
    .await?;
    if closed.is_none() {
        return Err(ApiError::NotFound);
    }
    Ok(Json(conversation(&state.db, id).await?))
}

// --- Site assistant ---

/// Which questions to list.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct QuestionFilter {
    /// `false` lists only the questions the docs did not answer: each one is
    /// a docs change to make.
    pub answered: Option<bool>,
}

/// A question asked to the site assistant on the website.
#[derive(Debug, Serialize, ToSchema)]
pub struct QuestionView {
    pub id: Uuid,
    pub question: String,
    /// The website page it was asked on.
    pub page: Option<String>,
    pub answer: String,
    /// Whether the docs answered it.
    pub answered: bool,
    pub links: Vec<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

/// The site assistant's questions of the last 90 days, newest first.
#[utoipa::path(
    get,
    path = "/admin/assistant/questions",
    tag = "admin",
    params(QuestionFilter),
    responses(
        (status = OK, body = Vec<QuestionView>),
        (status = NOT_FOUND, description = "Not an operator", body = ErrorBody),
    ),
)]
pub async fn list_questions(
    State(state): State<AppState>,
    _operator: Operator,
    Query(filter): Query<QuestionFilter>,
) -> Result<Json<Vec<QuestionView>>, ApiError> {
    let questions = sqlx::query_as!(
        QuestionView,
        "SELECT id, question, page, answer, answered, links, created_at FROM assistant_questions
         WHERE $1::boolean IS NULL OR answered = $1
         ORDER BY created_at DESC, id LIMIT $2",
        filter.answered,
        LIST_LIMIT,
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(questions))
}

// --- Newsletter ---

/// A newsletter issue.
#[derive(Debug, Serialize, ToSchema)]
pub struct IssueView {
    pub id: Uuid,
    pub subject: String,
    /// Plain text. Every mail adds the one-click unsubscribe link below it.
    pub body: String,
    /// `draft`, `sending` or `sent`.
    pub status: String,
    /// Subscribers it goes or went to; 0 for a draft.
    pub recipients: i64,
    /// How many of them have it.
    pub delivered: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub send_started_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub sent_at: Option<OffsetDateTime>,
}

/// The newsletter list and its issues.
#[derive(Debug, Serialize, ToSchema)]
pub struct NewsletterView {
    /// Whether the newsletter can be sent (`TC_RESEND_API_KEY` and
    /// `TC_NEWSLETTER_FROM` are set).
    pub on: bool,
    /// Subscribers who confirmed.
    pub confirmed: i64,
    /// Sign-ups waiting for their confirmation, deleted after 7 days.
    pub pending: i64,
    /// Newest first.
    pub issues: Vec<IssueView>,
}

async fn issues(db: &PgPool, only: Option<Uuid>) -> Result<Vec<IssueView>, sqlx::Error> {
    sqlx::query_as!(
        IssueView,
        r#"SELECT i.id, i.subject, i.body, i.status, i.created_at, i.send_started_at, i.sent_at,
                  (SELECT count(*) FROM newsletter_deliveries d WHERE d.issue_id = i.id) AS "recipients!",
                  (SELECT count(*) FROM newsletter_deliveries d
                   WHERE d.issue_id = i.id AND d.sent_at IS NOT NULL) AS "delivered!"
           FROM newsletter_issues i
           WHERE $1::uuid IS NULL OR i.id = $1
           ORDER BY i.created_at DESC, i.id"#,
        only
    )
    .fetch_all(db)
    .await
}

async fn issue(db: &PgPool, id: Uuid) -> Result<IssueView, ApiError> {
    issues(db, Some(id))
        .await?
        .into_iter()
        .next()
        .ok_or(ApiError::NotFound)
}

fn newsletter_on(state: &AppState) -> bool {
    let settings = &state.config.customer_relations;
    settings.resend_api_key.is_some() && settings.newsletter_from.is_some()
}

/// The newsletter list and every issue.
#[utoipa::path(
    get,
    path = "/admin/newsletter",
    tag = "admin",
    responses(
        (status = OK, body = NewsletterView),
        (status = NOT_FOUND, description = "Not an operator", body = ErrorBody),
    ),
)]
pub async fn newsletter(
    State(state): State<AppState>,
    _operator: Operator,
) -> Result<Json<NewsletterView>, ApiError> {
    let counts = sqlx::query!(
        r#"SELECT count(*) FILTER (WHERE status = 'confirmed') AS "confirmed!",
                  count(*) FILTER (WHERE status = 'pending') AS "pending!"
           FROM newsletter_subscribers"#
    )
    .fetch_one(&state.db)
    .await?;
    Ok(Json(NewsletterView {
        on: newsletter_on(&state),
        confirmed: counts.confirmed,
        pending: counts.pending,
        issues: issues(&state.db, None).await?,
    }))
}

/// A new issue, or a change to a draft. Fields left out stay as they are.
#[derive(Debug, Deserialize, ToSchema)]
pub struct IssueText {
    pub subject: Option<String>,
    pub body: Option<String>,
}

fn subject(value: &str) -> Result<String, ApiError> {
    let value = text(value, "subject")?;
    if value.chars().count() > 200 || value.contains(['\r', '\n']) {
        return Err(ApiError::BadRequest(
            "The subject is one line of at most 200 characters.".to_owned(),
        ));
    }
    Ok(value)
}

/// Writes a draft. Nothing goes out until it is sent.
#[utoipa::path(
    post,
    path = "/admin/newsletter/issues",
    tag = "admin",
    request_body = IssueText,
    responses(
        (status = CREATED, body = IssueView),
        (status = BAD_REQUEST, body = ErrorBody),
        (status = NOT_FOUND, description = "Not an operator", body = ErrorBody),
    ),
)]
pub async fn create_issue(
    State(state): State<AppState>,
    _operator: Operator,
    Json(draft): Json<IssueText>,
) -> Result<(StatusCode, Json<IssueView>), ApiError> {
    let subject = subject(draft.subject.as_deref().unwrap_or_default())?;
    let body = text(draft.body.as_deref().unwrap_or_default(), "text")?;
    let id = sqlx::query_scalar!(
        "INSERT INTO newsletter_issues (subject, body) VALUES ($1, $2) RETURNING id",
        subject,
        body
    )
    .fetch_one(&state.db)
    .await?;
    Ok((StatusCode::CREATED, Json(issue(&state.db, id).await?)))
}

/// Changes a draft.
#[utoipa::path(
    patch,
    path = "/admin/newsletter/issues/{id}",
    tag = "admin",
    params(("id" = Uuid, Path)),
    request_body = IssueText,
    responses(
        (status = OK, body = IssueView),
        (status = BAD_REQUEST, body = ErrorBody),
        (status = CONFLICT, description = "Already sent", body = ErrorBody),
        (status = NOT_FOUND, body = ErrorBody),
    ),
)]
pub async fn update_issue(
    State(state): State<AppState>,
    _operator: Operator,
    Path(id): Path<Uuid>,
    Json(change): Json<IssueText>,
) -> Result<Json<IssueView>, ApiError> {
    let subject = change.subject.as_deref().map(subject).transpose()?;
    let body = change
        .body
        .as_deref()
        .map(|body| text(body, "text"))
        .transpose()?;
    let current = issue(&state.db, id).await?;
    if current.status != "draft" {
        return Err(ApiError::Conflict(
            "This issue is no longer a draft.".to_owned(),
        ));
    }
    sqlx::query!(
        "UPDATE newsletter_issues SET subject = coalesce($2, subject), body = coalesce($3, body)
         WHERE id = $1 AND status = 'draft'",
        id,
        subject,
        body
    )
    .execute(&state.db)
    .await?;
    Ok(Json(issue(&state.db, id).await?))
}

/// Sends a draft to every confirmed subscriber. The service sends it in
/// small batches over the next minutes.
#[utoipa::path(
    post,
    path = "/admin/newsletter/issues/{id}/send",
    tag = "admin",
    params(("id" = Uuid, Path)),
    responses(
        (status = OK, body = IssueView),
        (status = CONFLICT, description = "Not a draft, or the newsletter is off", body = ErrorBody),
        (status = NOT_FOUND, body = ErrorBody),
    ),
)]
pub async fn send_issue(
    State(state): State<AppState>,
    _operator: Operator,
    Path(id): Path<Uuid>,
) -> Result<Json<IssueView>, ApiError> {
    if !newsletter_on(&state) {
        return Err(ApiError::Conflict(
            "The newsletter is off: set TC_RESEND_API_KEY and TC_NEWSLETTER_FROM.".to_owned(),
        ));
    }
    issue(&state.db, id).await?;
    let mut tx = state.db.begin().await?;
    let started = sqlx::query_scalar!(
        "UPDATE newsletter_issues SET status = 'sending', send_started_at = now()
         WHERE id = $1 AND status = 'draft' RETURNING id",
        id
    )
    .fetch_optional(&mut *tx)
    .await?;
    if started.is_none() {
        return Err(ApiError::Conflict(
            "This issue is no longer a draft.".to_owned(),
        ));
    }
    sqlx::query!(
        "INSERT INTO newsletter_deliveries (issue_id, subscriber_id)
         SELECT $1, id FROM newsletter_subscribers WHERE status = 'confirmed'
         ON CONFLICT DO NOTHING",
        id
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    super::wake();
    Ok(Json(issue(&state.db, id).await?))
}
