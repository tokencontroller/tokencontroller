//! The two doors into the support inbox: mail handed over by the mail worker,
//! and the contact form on the public website.

use std::sync::OnceLock;
use std::time::Duration;

use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use hmac::{KeyInit as _, Mac as _};
use serde::Deserialize;
use utoipa::ToSchema;

use super::support::{self, Inbound};
use super::web::{self, Limit, Refusal};
use crate::AppState;
use crate::error::{ApiError, ErrorBody};

type HmacSha256 = hmac::Hmac<sha2::Sha256>;

/// The biggest mail Cloudflare Email Routing delivers, with room for headers.
pub const MAX_MAIL_BYTES: usize = 26 * 1024 * 1024;

/// The longest contact form message.
const MAX_MESSAGE: usize = 5_000;

/// Contact form messages per visitor per hour.
const MESSAGES_PER_VISITOR: usize = 5;

/// Whether `signature` is the hex HMAC-SHA256 of the id, a newline and the
/// body, with `secret`. Compared in constant time.
#[must_use]
pub fn signed(secret: &str, id: &str, body: &[u8], signature: &str) -> bool {
    let Ok(expected) = hex::decode(signature.trim()) else {
        return false;
    };
    let Ok(mut mac) = HmacSha256::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    mac.update(id.as_bytes());
    mac.update(b"\n");
    mac.update(body);
    mac.verify_slice(&expected).is_ok()
}

/// Take a mail from the mail worker.
///
/// The body is the raw message (`message/rfc822`). The worker signs it; see
/// the mail decision record. A repeated id is accepted and ignored.
#[utoipa::path(
    post,
    path = "/inbound/mail",
    tag = "customer_relations",
    request_body(content = String, content_type = "message/rfc822", description = "The raw message"),
    params(
        ("x-tc-message-id" = String, Header, description = "The worker's id for this message"),
        ("x-tc-mailbox" = String, Header, description = "The mailbox it was sent to, such as `support`"),
        ("x-tc-envelope-from" = String, Header, description = "The envelope sender"),
        ("x-tc-envelope-to" = String, Header, description = "The envelope recipient"),
        ("x-tc-signature" = String, Header, description = "Hex HMAC-SHA256 of the id, a newline and the body"),
    ),
    responses(
        (status = NO_CONTENT, description = "Stored, or stored before"),
        (status = BAD_REQUEST, description = "A header is missing", body = ErrorBody),
        (status = UNAUTHORIZED, description = "The signature does not match", body = ErrorBody),
        (status = NOT_FOUND, description = "The support inbox is off", body = ErrorBody),
    ),
)]
pub async fn receive_mail(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    let Some(secret) = state
        .config
        .customer_relations
        .inbound_mail_secret
        .as_deref()
    else {
        return Err(ApiError::NotFound);
    };
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| ApiError::BadRequest(format!("The header {name} is missing.")))
    };
    let message_id = header("x-tc-message-id")?;
    let signature = header("x-tc-signature")?;
    if !signed(secret, message_id, &body, signature) {
        return Err(ApiError::Unauthorized);
    }
    let inbound = Inbound {
        message_id: message_id.to_owned(),
        mailbox: header("x-tc-mailbox")?.to_lowercase(),
        envelope_from: headers
            .get("x-tc-envelope-from")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .trim()
            .to_owned(),
        envelope_to: header("x-tc-envelope-to")?.to_owned(),
    };
    if support::receive(&state.db, &inbound, &body).await? {
        super::wake();
    }
    Ok(StatusCode::NO_CONTENT)
}

/// A message through the contact form.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ContactMessage {
    pub name: Option<String>,
    /// Where the answer goes.
    pub email: String,
    pub message: String,
    /// Left empty by people. Forms that fill it are taken to be robots.
    #[serde(default)]
    pub website: String,
}

fn contact_limit() -> &'static Limit {
    static LIMIT: OnceLock<Limit> = OnceLock::new();
    LIMIT.get_or_init(|| Limit::new(MESSAGES_PER_VISITOR, Duration::from_hours(1)))
}

/// Send a message through the contact form. The answer comes by mail.
#[utoipa::path(
    post,
    path = "/contact",
    tag = "customer_relations",
    request_body = ContactMessage,
    responses(
        (status = ACCEPTED, description = "Received; the answer comes by mail"),
        (status = BAD_REQUEST, description = "A field is missing or too long", body = ErrorBody),
        (status = NOT_FOUND, description = "The contact form is off", body = ErrorBody),
        (status = TOO_MANY_REQUESTS, description = "Too many messages from here", body = ErrorBody),
    ),
)]
pub async fn contact(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(message): Json<ContactMessage>,
) -> Response {
    let settings = &state.config.customer_relations;
    let response = match take_contact(&state, &headers, message).await {
        Ok(()) => StatusCode::ACCEPTED.into_response(),
        Err(refusal) => refusal.into_response(),
    };
    web::cors(settings, &headers, response)
}

/// The browser's preflight for [`contact`].
pub async fn contact_preflight(State(state): State<AppState>, headers: HeaderMap) -> Response {
    web::preflight(&state.config.customer_relations, &headers)
}

async fn take_contact(
    state: &AppState,
    headers: &HeaderMap,
    message: ContactMessage,
) -> Result<(), Refusal> {
    let settings = &state.config.customer_relations;
    if settings.support_from.is_none() || settings.site_url.is_none() {
        return Err(ApiError::NotFound.into());
    }
    let email = message.email.trim();
    if email.parse::<lettre::Address>().is_err() {
        return Err(ApiError::BadRequest(
            "Enter the mail address the answer should go to.".to_owned(),
        )
        .into());
    }
    let text = message.message.trim();
    if text.is_empty() {
        return Err(ApiError::BadRequest("Write a message.".to_owned()).into());
    }
    if text.chars().count() > MAX_MESSAGE {
        return Err(ApiError::BadRequest(format!(
            "Keep the message under {MAX_MESSAGE} characters."
        ))
        .into());
    }
    if !contact_limit().allow(&web::visitor(headers)) {
        return Err(Refusal::Busy(
            "Too many messages from here. Try again later.",
        ));
    }
    // A robot filled the hidden field: accept quietly and keep nothing.
    if !message.website.trim().is_empty() {
        return Ok(());
    }
    let name: Option<String> = message.name.map(|name| name.chars().take(200).collect());
    support::receive_contact_form(&state.db, email, name.as_deref(), text).await?;
    super::wake();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Made by the mail worker's `sign()` (Web Crypto in a Cloudflare
    /// Worker) for these inputs.
    const WORKER_SIGNATURE: &str =
        "92a5db593562acbce168ac3fe90658334f6a97e15a514a826a30556257ef9f9f";
    const ID: &str = "2026-10-12T00:00:00.000Z-abc";

    #[test]
    fn the_signature_matches_the_mail_worker() {
        assert!(signed("secret", ID, b"raw mail", WORKER_SIGNATURE));
        assert!(signed(
            "secret",
            ID,
            b"raw mail",
            &WORKER_SIGNATURE.to_uppercase()
        ));
        assert!(!signed("secret", "other-id", b"raw mail", WORKER_SIGNATURE));
        assert!(!signed("secret", ID, b"other mail", WORKER_SIGNATURE));
        assert!(!signed("other", ID, b"raw mail", WORKER_SIGNATURE));
        assert!(!signed("secret", ID, b"raw mail", "not hex"));
    }
}
