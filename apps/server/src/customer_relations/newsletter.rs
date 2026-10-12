//! The newsletter: sign-up with double opt-in, one-click unsubscribe, and
//! sending approved issues through Resend, one mail per subscriber.
//!
//! Confirm and unsubscribe links point at pages on the public website, which
//! show one button that posts back here, so a mail scanner that opens every
//! link confirms or unsubscribes nobody. Mail clients use the one-click
//! `List-Unsubscribe-Post` header (RFC 8058), which posts straight here.

use std::sync::OnceLock;
use std::time::Duration;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use lettre::message::Mailbox;
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use utoipa::{IntoParams, ToSchema};

use super::mail::{self, Outgoing, Sent};
use super::settings::Settings;
use super::web::{self, Limit, Refusal};
use crate::error::{ApiError, ErrorBody};
use crate::hosted::Hosted;

/// Sign-ups per visitor per hour.
const SIGN_UPS_PER_VISITOR: usize = 5;

/// Mails sent per run of the background loop, to stay inside Resend's limits.
const PER_RUN: i64 = 50;

/// A sign-up.
#[derive(Debug, Deserialize, ToSchema)]
pub struct SignUp {
    pub email: String,
    /// Where the sign-up came from, such as `/blog`.
    pub source: Option<String>,
}

/// What happened, in one word.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct Done {
    /// `check_your_email`, `confirmed` or `unsubscribed`.
    pub status: String,
}

/// The token from a confirm or unsubscribe link.
#[derive(Debug, Deserialize, ToSchema, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct Token {
    pub token: String,
}

fn sign_up_limit() -> &'static Limit {
    static LIMIT: OnceLock<Limit> = OnceLock::new();
    LIMIT.get_or_init(|| Limit::new(SIGN_UPS_PER_VISITOR, Duration::from_hours(1)))
}

fn enabled(settings: &Settings) -> Result<&str, ApiError> {
    match (&settings.resend_api_key, &settings.site_url) {
        (Some(_), Some(site)) => Ok(site),
        _ => Err(ApiError::NotFound),
    }
}

fn token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

/// Sign up for the newsletter. A confirmation mail goes out; nothing else is
/// sent until the link in it is used.
#[utoipa::path(
    post,
    path = "/newsletter/subscribers",
    tag = "customer_relations",
    request_body = SignUp,
    responses(
        (status = ACCEPTED, description = "A confirmation mail is on its way, unless the address is already confirmed", body = Done),
        (status = BAD_REQUEST, description = "Not a mail address", body = ErrorBody),
        (status = NOT_FOUND, description = "The newsletter is off", body = ErrorBody),
        (status = TOO_MANY_REQUESTS, description = "Too many sign-ups from here", body = ErrorBody),
    ),
)]
pub async fn sign_up(
    State(state): State<Hosted>,
    headers: HeaderMap,
    Json(sign_up): Json<SignUp>,
) -> Response {
    let settings = &state.settings.customer_relations;
    let response = match subscribe(&state, &headers, sign_up).await {
        Ok(()) => (
            StatusCode::ACCEPTED,
            Json(Done {
                status: "check_your_email".to_owned(),
            }),
        )
            .into_response(),
        Err(error) => error.into_response(),
    };
    web::cors(settings, &headers, response)
}

/// The browser's preflight for [`sign_up`].
pub async fn sign_up_preflight(State(state): State<Hosted>, headers: HeaderMap) -> Response {
    web::preflight(&state.settings.customer_relations, &headers)
}

async fn subscribe(state: &Hosted, headers: &HeaderMap, sign_up: SignUp) -> Result<(), Refusal> {
    enabled(&state.settings.customer_relations)?;
    let email = sign_up.email.trim().to_lowercase();
    if email.parse::<lettre::Address>().is_err() || email.chars().count() > 254 {
        return Err(ApiError::BadRequest("Enter a valid mail address.".to_owned()).into());
    }
    if !sign_up_limit().allow(&web::visitor(headers)) {
        return Err(Refusal::Busy(
            "Too many sign-ups from here. Try again later.",
        ));
    }
    let source: String = sign_up
        .source
        .unwrap_or_else(|| "website".to_owned())
        .chars()
        .take(200)
        .collect();
    // A new address, or a pending one asking again: (re)send the confirmation.
    // A confirmed address gets nothing, and the answer is the same, so the
    // form does not tell who is subscribed.
    let id = sqlx::query_scalar!(
        "INSERT INTO newsletter_subscribers (email, source, confirm_token, unsubscribe_token)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (lower(email)) DO UPDATE SET confirmation_sent_at = NULL
           WHERE newsletter_subscribers.status = 'pending'
             AND (newsletter_subscribers.confirmation_sent_at IS NULL
                  OR newsletter_subscribers.confirmation_sent_at < now() - interval '10 minutes')
         RETURNING id",
        email,
        source,
        token(),
        token(),
    )
    .fetch_optional(&state.db)
    .await?;
    if id.is_some() {
        super::wake();
    }
    Ok(())
}

/// Confirm a newsletter sign-up with the token from the confirmation mail.
#[utoipa::path(
    post,
    path = "/newsletter/confirm",
    tag = "customer_relations",
    request_body = Token,
    responses(
        (status = OK, description = "Subscribed", body = Done),
        (status = NOT_FOUND, description = "The link is unknown or expired, or the newsletter is off", body = ErrorBody),
    ),
)]
pub async fn confirm(
    State(state): State<Hosted>,
    headers: HeaderMap,
    Json(token): Json<Token>,
) -> Response {
    let settings = &state.settings.customer_relations;
    let result = async {
        enabled(settings)?;
        let confirmed = sqlx::query_scalar!(
            "UPDATE newsletter_subscribers
             SET status = 'confirmed', confirmed_at = coalesce(confirmed_at, now())
             WHERE confirm_token = $1
             RETURNING id",
            token.token.trim(),
        )
        .fetch_optional(&state.db)
        .await?;
        confirmed.ok_or(ApiError::NotFound)?;
        Ok::<_, ApiError>(Json(Done {
            status: "confirmed".to_owned(),
        }))
    }
    .await;
    web::cors(settings, &headers, result.into_response())
}

/// The browser's preflight for [`confirm`].
pub async fn confirm_preflight(State(state): State<Hosted>, headers: HeaderMap) -> Response {
    web::preflight(&state.settings.customer_relations, &headers)
}

/// Unsubscribe with one click. The address is deleted from the list.
///
/// Mail clients post here straight from the `List-Unsubscribe` header
/// (RFC 8058); the website's unsubscribe page posts here too.
#[utoipa::path(
    post,
    path = "/newsletter/unsubscribe",
    tag = "customer_relations",
    params(Token),
    responses(
        (status = OK, description = "Unsubscribed, or was not subscribed", body = Done),
        (status = NOT_FOUND, description = "The newsletter is off", body = ErrorBody),
    ),
)]
pub async fn unsubscribe(
    State(state): State<Hosted>,
    headers: HeaderMap,
    Query(token): Query<Token>,
) -> Response {
    let settings = &state.settings.customer_relations;
    let result = async {
        enabled(settings)?;
        sqlx::query!(
            "DELETE FROM newsletter_subscribers WHERE unsubscribe_token = $1",
            token.token.trim()
        )
        .execute(&state.db)
        .await?;
        Ok::<_, ApiError>(Json(Done {
            status: "unsubscribed".to_owned(),
        }))
    }
    .await;
    web::cors(settings, &headers, result.into_response())
}

/// The browser's preflight for [`unsubscribe`].
pub async fn unsubscribe_preflight(State(state): State<Hosted>, headers: HeaderMap) -> Response {
    web::preflight(&state.settings.customer_relations, &headers)
}

fn unsubscribe_headers(state: &Hosted, unsubscribe_token: &str) -> Vec<(&'static str, String)> {
    vec![
        (
            "List-Unsubscribe",
            format!(
                "<{}/api/v1/newsletter/unsubscribe?token={unsubscribe_token}>",
                state.config.public_url
            ),
        ),
        (
            "List-Unsubscribe-Post",
            "List-Unsubscribe=One-Click".to_owned(),
        ),
    ]
}

fn reply_to(settings: &Settings) -> Option<String> {
    settings
        .support_from
        .as_deref()
        .and_then(|from| from.parse::<Mailbox>().ok())
        .map(|mailbox| mailbox.email.to_string())
}

/// Sends confirmation mails that have not gone out yet, and deletes sign-ups
/// not confirmed within 7 days.
///
/// # Errors
///
/// When the database fails or Resend refuses a mail.
pub async fn send_confirmations(state: &Hosted) -> anyhow::Result<()> {
    let settings = &state.settings.customer_relations;
    let (Some(from), Some(site)) = (&settings.newsletter_from, &settings.site_url) else {
        return Ok(());
    };
    sqlx::query!(
        "DELETE FROM newsletter_subscribers WHERE status = 'pending' AND requested_at < now() - interval '7 days'"
    )
    .execute(&state.db)
    .await?;
    let waiting = sqlx::query!(
        "SELECT id, email, confirm_token, unsubscribe_token FROM newsletter_subscribers
         WHERE status = 'pending' AND confirmation_sent_at IS NULL
         ORDER BY requested_at LIMIT $1",
        PER_RUN,
    )
    .fetch_all(&state.db)
    .await?;
    for subscriber in waiting {
        let text = format!(
            "Someone, hopefully you, asked for the Token Controller newsletter at this address.\n\n\
             Confirm here:\n{site}/newsletter/confirm?token={}\n\n\
             If it was not you, ignore this mail. You will get nothing else from us, and the address is deleted in 7 days.\n\n\
             --\nToken Controller",
            subscriber.confirm_token
        );
        let outgoing = Outgoing {
            from: from.clone(),
            to: subscriber.email.parse()?,
            reply_to: reply_to(settings),
            subject: "Confirm the Token Controller newsletter".to_owned(),
            text,
            message_id: None,
            references: Vec::new(),
            headers: unsubscribe_headers(state, &subscriber.unsubscribe_token),
        };
        if mail::send_resend(settings, &outgoing).await? == Sent::Later {
            break;
        }
        sqlx::query!(
            "UPDATE newsletter_subscribers SET confirmation_sent_at = now() WHERE id = $1",
            subscriber.id
        )
        .execute(&state.db)
        .await?;
    }
    Ok(())
}

/// Sends the next batch of every issue that is going out, and marks an issue
/// sent when nobody is left.
///
/// # Errors
///
/// When the database fails or Resend refuses a mail.
pub async fn send_issues(state: &Hosted) -> anyhow::Result<()> {
    let settings = &state.settings.customer_relations;
    let (Some(from), Some(site)) = (&settings.newsletter_from, &settings.site_url) else {
        return Ok(());
    };
    let batch = sqlx::query!(
        "SELECT i.id AS issue_id, i.subject, i.body, s.id AS subscriber_id, s.email, s.unsubscribe_token
         FROM newsletter_deliveries d
         JOIN newsletter_issues i ON i.id = d.issue_id
         JOIN newsletter_subscribers s ON s.id = d.subscriber_id
         WHERE i.status = 'sending' AND d.sent_at IS NULL
         ORDER BY i.send_started_at, s.email LIMIT $1",
        PER_RUN,
    )
    .fetch_all(&state.db)
    .await?;
    for delivery in batch {
        let text = format!(
            "{}\n\n--\nToken Controller\nYou get this because you subscribed. Unsubscribe with one click:\n{site}/newsletter/unsubscribe?token={}",
            delivery.body.trim(),
            delivery.unsubscribe_token
        );
        let outgoing = Outgoing {
            from: from.clone(),
            to: delivery.email.parse()?,
            reply_to: reply_to(settings),
            subject: delivery.subject,
            text,
            message_id: None,
            references: Vec::new(),
            headers: unsubscribe_headers(state, &delivery.unsubscribe_token),
        };
        if mail::send_resend(settings, &outgoing).await? == Sent::Later {
            break;
        }
        sqlx::query!(
            "UPDATE newsletter_deliveries SET sent_at = now() WHERE issue_id = $1 AND subscriber_id = $2",
            delivery.issue_id,
            delivery.subscriber_id
        )
        .execute(&state.db)
        .await?;
    }
    sqlx::query!(
        "UPDATE newsletter_issues i SET status = 'sent', sent_at = now()
         WHERE i.status = 'sending'
           AND NOT EXISTS (SELECT 1 FROM newsletter_deliveries d WHERE d.issue_id = i.id AND d.sent_at IS NULL)"
    )
    .execute(&state.db)
    .await?;
    Ok(())
}
