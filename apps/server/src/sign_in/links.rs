//! Magic links: sign-up, sign-in, the page behind the link, the local tool's
//! poll, sign-out and `GET /me`.
//!
//! A link works once and for 15 minutes. A GET on the link only shows a page
//! with a "Sign in" button; the POST from that button uses the link up. Mail
//! link scanners open every link with a GET before the person clicks.

use axum::Form;
use axum::Json;
use axum::extract::{Query, State};
use axum::http::header::{LOCATION, SET_COOKIE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, PgPool};
use time::{Duration, OffsetDateTime};
use utoipa::ToSchema;
use uuid::Uuid;

use super::caller::{Member, SESSION_COOKIE, bearer_token, session_cookie};
use super::secret;
use crate::error::ApiError;
use crate::mail::{self, Mail, escape_html, render};
use crate::organizations;
use crate::{AppState, Config};

/// How long a sign-in link works.
pub const LINK_MINUTES: i64 = 15;
/// How many links one address may ask for within [`LINK_MINUTES`].
const LINKS_PER_WINDOW: i64 = 5;
/// How long a browser stays signed in.
const BROWSER_SESSION_DAYS: i64 = 30;
/// How long a local tool such as the CLI stays signed in.
const LOCAL_TOOL_SESSION_DAYS: i64 = 365;

const SIGN_IN_HTML: &str = include_str!("mail/sign-in.html");
const SIGN_IN_TEXT: &str = include_str!("mail/sign-in.txt");
const NO_ACCOUNT_HTML: &str = include_str!("mail/no-account.html");
const NO_ACCOUNT_TEXT: &str = include_str!("mail/no-account.txt");

/// Ask for a sign-in link.
#[derive(Debug, Deserialize, ToSchema)]
pub struct SignInRequest {
    pub email: String,
    /// `true` for a local tool such as `tc connect`: the answer then carries
    /// a poll token to collect the session with.
    #[serde(default)]
    pub local_tool: bool,
}

/// Create an organization: the link signs in its first controller.
#[derive(Debug, Deserialize, ToSchema)]
pub struct SignUpRequest {
    pub email: String,
    /// Such as `Kestrel Digital`.
    pub organization_name: String,
    #[serde(default)]
    pub local_tool: bool,
}

/// The same answer whether or not the address belongs to anyone.
#[derive(Debug, Serialize, ToSchema)]
pub struct LinkSent {
    /// For a local tool: pass it to `POST /sign-in/poll`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poll_token: Option<String>,
}

/// Send a sign-in link to a member.
///
/// Always answers 202, so nobody learns whether an address is a member. An
/// address that belongs to no organization gets a mail that says so.
#[utoipa::path(
    post,
    path = "/sign-in",
    tag = "sign-in",
    request_body = SignInRequest,
    responses(
        (status = ACCEPTED, description = "A mail is on its way", body = LinkSent),
        (status = BAD_REQUEST, description = "Not an email address", body = crate::error::ErrorBody),
        (status = TOO_MANY_REQUESTS, description = "Too many links for this address", body = crate::error::ErrorBody),
    ),
)]
pub(super) async fn sign_in(
    State(state): State<AppState>,
    Json(request): Json<SignInRequest>,
) -> Result<(StatusCode, Json<LinkSent>), ApiError> {
    let email = normalize_email(&request.email)?;
    let sent = request_link(&state, &email, None, request.local_tool).await?;
    Ok((StatusCode::ACCEPTED, Json(sent)))
}

/// Create an organization and send its first controller a sign-in link.
///
/// The organization exists once the link is used. An address that is
/// already a member gets an ordinary sign-in link instead: one account
/// belongs to one organization.
#[utoipa::path(
    post,
    path = "/sign-up",
    tag = "sign-in",
    request_body = SignUpRequest,
    responses(
        (status = ACCEPTED, description = "A mail is on its way", body = LinkSent),
        (status = BAD_REQUEST, description = "Not an email address, or no name", body = crate::error::ErrorBody),
        (status = TOO_MANY_REQUESTS, description = "Too many links for this address", body = crate::error::ErrorBody),
    ),
)]
pub(super) async fn sign_up(
    State(state): State<AppState>,
    Json(request): Json<SignUpRequest>,
) -> Result<(StatusCode, Json<LinkSent>), ApiError> {
    let email = normalize_email(&request.email)?;
    let name = organizations::validate_name(&request.organization_name)?;
    let sent = request_link(&state, &email, Some(name), request.local_tool).await?;
    Ok((StatusCode::ACCEPTED, Json(sent)))
}

async fn request_link(
    state: &AppState,
    email: &str,
    sign_up_name: Option<String>,
    local_tool: bool,
) -> Result<LinkSent, ApiError> {
    let recent = sqlx::query_scalar!(
        r#"SELECT count(*) AS "count!" FROM sign_in_links
           WHERE email = $1 AND created_at > now() - make_interval(mins => $2::bigint::int)"#,
        email,
        LINK_MINUTES
    )
    .fetch_one(&state.db)
    .await?;
    if recent >= LINKS_PER_WINDOW {
        return Err(ApiError::TooManyRequests(format!(
            "Too many sign-in links for this address. Wait {LINK_MINUTES} minutes and try again."
        )));
    }

    let member = sqlx::query!(
        r#"SELECT o.sign_in_domains
           FROM people p JOIN organizations o ON o.id = p.organization_id
           WHERE p.email = $1 AND p.removed_at IS NULL"#,
        email
    )
    .fetch_optional(&state.db)
    .await?;
    let allowed = member
        .as_ref()
        .is_some_and(|member| domain_allowed(email, &member.sign_in_domains));
    // A sign-up from an address that is already a member is a sign-in.
    let sign_up_name = if member.is_some() { None } else { sign_up_name };

    let link = new_link(
        &state.db,
        &state.config,
        email,
        sign_up_name.as_deref(),
        local_tool,
    )
    .await?;
    let poll_token = link.poll.map(|poll| poll.token);

    if allowed || sign_up_name.is_some() {
        let url = link_url(&state.config, &link.secret.token);
        send_quietly(
            &state.db,
            &state.config,
            sign_in_mail(&state.config, email, &url),
        )
        .await;
    } else if member.is_none() {
        send_quietly(
            &state.db,
            &state.config,
            no_account_mail(&state.config, email),
        )
        .await;
    }
    Ok(LinkSent { poll_token })
}

/// A link that was stored, with the secrets to hand out.
struct NewLink {
    secret: secret::Secret,
    poll: Option<secret::Secret>,
}

async fn new_link(
    db: &PgPool,
    config: &Config,
    email: &str,
    sign_up_name: Option<&str>,
    local_tool: bool,
) -> Result<NewLink, ApiError> {
    let secret = new_secret(secret::LINK, config)?;
    let poll = if local_tool {
        Some(new_secret(secret::POLL, config)?)
    } else {
        None
    };
    sqlx::query!(
        r#"INSERT INTO sign_in_links (token_hash, email, sign_up_organization_name, poll_hash, expires_at)
           VALUES ($1, $2, $3, $4, now() + make_interval(mins => $5::bigint::int))"#,
        secret.hash,
        email,
        sign_up_name,
        poll.as_ref().map(|poll| poll.hash.clone()),
        LINK_MINUTES
    )
    .execute(db)
    .await?;
    Ok(NewLink { secret, poll })
}

/// A sign-in link for a member, as `tokencontroller sign-in-link` prints it:
/// for instances without a mail server, and for screenshots on staging.
///
/// # Errors
///
/// Fails if the address belongs to no member or a query fails.
pub async fn link_for_member(db: &PgPool, config: &Config, email: &str) -> anyhow::Result<String> {
    let email = normalize_email(email).map_err(|error| anyhow::anyhow!("{error}"))?;
    let member = sqlx::query_scalar!(
        "SELECT id FROM people WHERE email = $1 AND removed_at IS NULL",
        email
    )
    .fetch_optional(db)
    .await?;
    if member.is_none() {
        anyhow::bail!("{email} is not a member of any organization on this instance");
    }
    let link = new_link(db, config, &email, None, false)
        .await
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    Ok(link_url(config, &link.secret.token))
}

fn link_url(config: &Config, token: &str) -> String {
    format!("{}/api/v1/sign-in/link?token={token}", config.public_url)
}

fn sign_in_mail(config: &Config, email: &str, url: &str) -> Mail {
    let minutes = LINK_MINUTES.to_string();
    let values = [
        ("email", email),
        ("sign_in_url", url),
        ("host", mail::host_of(&config.public_url)),
        ("expires_minutes", minutes.as_str()),
    ];
    Mail {
        kind: "sign_in",
        to: email.to_owned(),
        subject: "Sign in to Token Controller".to_owned(),
        text: render(SIGN_IN_TEXT, &values, false),
        html: render(SIGN_IN_HTML, &values, true),
    }
}

fn no_account_mail(config: &Config, email: &str) -> Mail {
    let sign_up_url = format!("{}/signup", config.public_url);
    let values = [
        ("email", email),
        ("sign_up_url", sign_up_url.as_str()),
        ("host", mail::host_of(&config.public_url)),
    ];
    Mail {
        kind: "no_account",
        to: email.to_owned(),
        subject: "No Token Controller account for this address".to_owned(),
        text: render(NO_ACCOUNT_TEXT, &values, false),
        html: render(NO_ACCOUNT_HTML, &values, true),
    }
}

/// Sends a mail and only logs a failure, so the answer never says whether
/// a mail went out.
async fn send_quietly(db: &PgPool, config: &Config, mail: Mail) {
    if let Err(error) = mail::send(db, config, mail).await {
        tracing::warn!("sign-in mail failed: {error:#}");
    }
}

/// The token in a sign-in link.
#[derive(Debug, Deserialize, utoipa::IntoParams, ToSchema)]
pub struct LinkToken {
    pub token: String,
}

/// The page a sign-in link opens: a "Sign in" button that posts the link.
#[utoipa::path(
    get,
    path = "/sign-in/link",
    tag = "sign-in",
    params(LinkToken),
    responses(
        (status = OK, description = "The page with the Sign in button", content_type = "text/html", body = String),
        (status = GONE, description = "The link was used, has expired or never existed", content_type = "text/html", body = String),
    ),
)]
pub(super) async fn link_page(
    State(state): State<AppState>,
    Query(link): Query<LinkToken>,
) -> Result<Response, ApiError> {
    let hash = secret::hash(&link.token, &state.config.secret_key);
    let row = sqlx::query!(
        r#"SELECT email, sign_up_organization_name, poll_hash IS NOT NULL AS "local_tool!"
           FROM sign_in_links
           WHERE token_hash = $1 AND used_at IS NULL AND expires_at > now()"#,
        hash
    )
    .fetch_optional(&state.db)
    .await?;
    let Some(row) = row else {
        return Ok(page(
            StatusCode::GONE,
            "This link no longer works",
            &expired_text(&state.config),
        ));
    };
    let what = match (&row.sign_up_organization_name, row.local_tool) {
        (Some(name), _) => format!(
            "Create <strong>{}</strong> and sign in as <strong>{}</strong>.",
            escape_html(name),
            escape_html(&row.email)
        ),
        (None, true) => format!(
            "Sign in your command line tool as <strong>{}</strong>.",
            escape_html(&row.email)
        ),
        (None, false) => format!("Sign in as <strong>{}</strong>.", escape_html(&row.email)),
    };
    let body = format!(
        r#"<p>{what}</p>
<form method="post" action="/api/v1/sign-in/link">
<input type="hidden" name="token" value="{}">
<button type="submit">Sign in</button>
</form>"#,
        escape_html(&link.token)
    );
    Ok(page(StatusCode::OK, "Sign in to Token Controller", &body))
}

/// Use a sign-in link: the form on the link's page posts here.
///
/// A browser gets its session cookie and goes to the web app. A local tool's
/// link only marks the link used; the tool collects its session with
/// `POST /sign-in/poll`.
#[utoipa::path(
    post,
    path = "/sign-in/link",
    tag = "sign-in",
    request_body(content = LinkToken, content_type = "application/x-www-form-urlencoded"),
    responses(
        (status = SEE_OTHER, description = "Signed in; on to the web app"),
        (status = OK, description = "A local tool is signed in", content_type = "text/html", body = String),
        (status = GONE, description = "The link was used, has expired or never existed", content_type = "text/html", body = String),
    ),
)]
pub(super) async fn use_link(
    State(state): State<AppState>,
    Form(link): Form<LinkToken>,
) -> Result<Response, ApiError> {
    let mut tx = state.db.begin().await?;
    let used = consume_link(&mut tx, &state.config, &link.token).await?;
    let response = match used {
        Used::Gone => page(
            StatusCode::GONE,
            "This link no longer works",
            &expired_text(&state.config),
        ),
        Used::LocalTool => page(
            StatusCode::OK,
            "You are signed in",
            "<p>Your command line tool is signed in. You can close this tab and go back to your terminal.</p>",
        ),
        Used::Browser { person_id } => {
            let session = create_session(&mut tx, &state.config, person_id, false).await?;
            let mut response = StatusCode::SEE_OTHER.into_response();
            let headers = response.headers_mut();
            headers.insert(LOCATION, HeaderValue::from_static("/"));
            headers.insert(SET_COOKIE, session_cookie_header(&state.config, &session)?);
            response
        }
    };
    tx.commit().await?;
    Ok(response)
}

enum Used {
    Gone,
    Browser { person_id: Uuid },
    LocalTool,
}

async fn consume_link(
    tx: &mut PgConnection,
    config: &Config,
    token: &str,
) -> Result<Used, ApiError> {
    let hash = secret::hash(token, &config.secret_key);
    let Some(link) = sqlx::query!(
        r#"SELECT email, sign_up_organization_name, poll_hash IS NOT NULL AS "local_tool!"
           FROM sign_in_links
           WHERE token_hash = $1 AND used_at IS NULL AND expires_at > now()
           FOR UPDATE"#,
        hash
    )
    .fetch_optional(&mut *tx)
    .await?
    else {
        return Ok(Used::Gone);
    };

    let member = sqlx::query_scalar!(
        "SELECT id FROM people WHERE email = $1 AND removed_at IS NULL",
        link.email
    )
    .fetch_optional(&mut *tx)
    .await?;
    let person_id = match (member, link.sign_up_organization_name) {
        (Some(person_id), _) => {
            sqlx::query!(
                "UPDATE people SET joined_at = coalesce(joined_at, now()) WHERE id = $1",
                person_id
            )
            .execute(&mut *tx)
            .await?;
            person_id
        }
        (None, Some(name)) => {
            organizations::create(&mut *tx, &name, &link.email)
                .await?
                .person_id
        }
        // The person was removed after the link went out.
        (None, None) => return Ok(Used::Gone),
    };

    sqlx::query!(
        "UPDATE sign_in_links SET used_at = now(), person_id = $2 WHERE token_hash = $1",
        hash,
        person_id
    )
    .execute(&mut *tx)
    .await?;
    Ok(if link.local_tool {
        Used::LocalTool
    } else {
        Used::Browser { person_id }
    })
}

/// Collect a local tool's session once its link was used.
#[derive(Debug, Deserialize, ToSchema)]
pub struct PollRequest {
    pub poll_token: String,
}

/// The answer to a poll.
#[derive(Debug, Serialize, ToSchema)]
pub struct PollAnswer {
    /// `waiting` until the person used the link, then `signed_in`.
    pub status: PollStatus,
    /// Once, with `signed_in`: use it as `Authorization: Bearer`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_token: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PollStatus {
    Waiting,
    SignedIn,
}

/// Collect a local tool's session after the person used the link.
#[utoipa::path(
    post,
    path = "/sign-in/poll",
    tag = "sign-in",
    request_body = PollRequest,
    responses(
        (status = ACCEPTED, description = "The link was not used yet; ask again in a few seconds", body = PollAnswer),
        (status = OK, description = "Signed in. The session token comes only once", body = PollAnswer),
        (status = GONE, description = "The link expired, or the session was already collected", body = crate::error::ErrorBody),
        (status = NOT_FOUND, description = "No such poll token", body = crate::error::ErrorBody),
    ),
)]
pub(super) async fn poll(
    State(state): State<AppState>,
    Json(request): Json<PollRequest>,
) -> Result<(StatusCode, Json<PollAnswer>), ApiError> {
    let hash = secret::hash(&request.poll_token, &state.config.secret_key);
    let mut tx = state.db.begin().await?;
    let link = sqlx::query!(
        r#"SELECT person_id, polled_at IS NOT NULL AS "polled!", expires_at < now() AS "expired!"
           FROM sign_in_links WHERE poll_hash = $1 FOR UPDATE"#,
        hash
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ApiError::NotFound)?;

    if link.polled {
        return Err(ApiError::Gone(
            "This session was already collected. Sign in again.".to_owned(),
        ));
    }
    let Some(person_id) = link.person_id else {
        if link.expired {
            return Err(ApiError::Gone(format!(
                "The sign-in link expired after {LINK_MINUTES} minutes. Sign in again."
            )));
        }
        return Ok((
            StatusCode::ACCEPTED,
            Json(PollAnswer {
                status: PollStatus::Waiting,
                session_token: None,
            }),
        ));
    };
    sqlx::query!(
        "UPDATE sign_in_links SET polled_at = now() WHERE poll_hash = $1",
        hash
    )
    .execute(&mut *tx)
    .await?;
    let session = create_session(&mut tx, &state.config, person_id, true).await?;
    tx.commit().await?;
    Ok((
        StatusCode::OK,
        Json(PollAnswer {
            status: PollStatus::SignedIn,
            session_token: Some(session.token),
        }),
    ))
}

async fn create_session(
    tx: &mut PgConnection,
    config: &Config,
    person_id: Uuid,
    local_tool: bool,
) -> Result<secret::Secret, ApiError> {
    let session = new_secret(secret::SESSION, config)?;
    let (client, days) = if local_tool {
        ("local_tool", LOCAL_TOOL_SESSION_DAYS)
    } else {
        ("browser", BROWSER_SESSION_DAYS)
    };
    let expires_at = OffsetDateTime::now_utc()
        .checked_add(Duration::days(days))
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("session expiry out of range")))?;
    sqlx::query!(
        "INSERT INTO sessions (token_hash, person_id, client, expires_at) VALUES ($1, $2, $3, $4)",
        session.hash,
        person_id,
        client,
        expires_at
    )
    .execute(&mut *tx)
    .await?;
    Ok(session)
}

fn session_cookie_header(
    config: &Config,
    session: &secret::Secret,
) -> Result<HeaderValue, ApiError> {
    let max_age = BROWSER_SESSION_DAYS
        .checked_mul(24 * 60 * 60)
        .unwrap_or(i64::MAX);
    let secure = if config.public_url.starts_with("https://") {
        "; Secure"
    } else {
        ""
    };
    HeaderValue::from_str(&format!(
        "{SESSION_COOKIE}={}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{secure}",
        session.token
    ))
    .map_err(|error| ApiError::Internal(error.into()))
}

/// Sign out: ends the session in the cookie or the bearer token.
#[utoipa::path(
    post,
    path = "/sign-out",
    tag = "sign-in",
    responses((status = NO_CONTENT, description = "Signed out")),
)]
pub(super) async fn sign_out(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let token = bearer_token(&headers)
        .filter(|token| token.starts_with(secret::SESSION))
        .or_else(|| session_cookie(&headers));
    if let Some(token) = token {
        let hash = secret::hash(token, &state.config.secret_key);
        sqlx::query!("DELETE FROM sessions WHERE token_hash = $1", hash)
            .execute(&state.db)
            .await?;
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(
        SET_COOKIE,
        HeaderValue::from_static("tc_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0"),
    );
    Ok(response)
}

/// Who is signed in.
#[derive(Debug, Serialize, ToSchema)]
pub struct Me {
    pub person: organizations::MemberView,
    pub organization: organizations::OrganizationSummary,
    /// `session` for a person, `read` for a read token.
    pub credential: super::CredentialKind,
}

/// The signed-in person and their organization.
#[utoipa::path(
    get,
    path = "/me",
    tag = "sign-in",
    responses(
        (status = OK, body = Me),
        (status = UNAUTHORIZED, body = crate::error::ErrorBody),
    ),
)]
pub(super) async fn me(
    State(state): State<AppState>,
    member: Member,
) -> Result<Json<Me>, ApiError> {
    let person = organizations::member(&state.db, member.organization_id, member.person_id).await?;
    let organization = organizations::summary(&state.db, member.organization_id).await?;
    Ok(Json(Me {
        person,
        organization,
        credential: member.credential.kind(),
    }))
}

/// Lower-cases and checks an email address.
///
/// # Errors
///
/// [`ApiError::BadRequest`] when it does not look like an address.
pub fn normalize_email(email: &str) -> Result<String, ApiError> {
    let email = email.trim().to_lowercase();
    let valid = email.len() <= 254
        && !email.chars().any(char::is_whitespace)
        && email.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
        });
    if valid {
        Ok(email)
    } else {
        Err(ApiError::BadRequest(format!(
            "{email:?} is not an email address"
        )))
    }
}

/// Whether `email` may sign in, given the organization's sign-in domains.
#[must_use]
pub fn domain_allowed(email: &str, domains: &[String]) -> bool {
    domains.is_empty()
        || email
            .rsplit_once('@')
            .is_some_and(|(_, domain)| domains.iter().any(|allowed| allowed == domain))
}

fn new_secret(prefix: &str, config: &Config) -> Result<secret::Secret, ApiError> {
    secret::generate(prefix, &config.secret_key)
        .map_err(|error| ApiError::Internal(anyhow::anyhow!("no randomness: {error}")))
}

fn expired_text(config: &Config) -> String {
    format!(
        r#"<p>The link was already used, or it is older than {LINK_MINUTES} minutes. <a href="{}/">Ask for a new one</a>.</p>"#,
        escape_html(&config.public_url)
    )
}

/// A small page in the product's colors, for the few HTML answers the
/// service gives itself.
fn page(status: StatusCode, title: &str, body: &str) -> Response {
    let html = format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="referrer" content="no-referrer">
<title>{title} · Token Controller</title>
<style>
body{{margin:0;background:#F6F7F8;color:#0E1116;font:16px/1.5 Inter,-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,Helvetica,Arial,sans-serif}}
main{{max-width:440px;margin:12vh auto;padding:32px 36px;background:#fff;border:1px solid #E7E9EC;border-radius:14px}}
h1{{margin:0 0 12px;font-size:24px;line-height:1.25;letter-spacing:-.02em}}
button{{margin-top:12px;padding:13px 22px;font:600 16px/1 inherit;color:#fff;background:#0E1116;border:0;border-radius:10px;cursor:pointer}}
a{{color:#0E7C66}}
</style>
</head>
<body><main><h1>{title}</h1>
{body}
</main></body>
</html>
"#,
        title = escape_html(title),
    );
    (status, Html(html)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_addresses_are_lower_cased_and_checked() {
        assert_eq!(
            normalize_email("  M.Ortner@Kestrel.Example ").unwrap(),
            "m.ortner@kestrel.example"
        );
        for bad in [
            "",
            "mara",
            "@kestrel.example",
            "mara@",
            "mara@kestrel",
            "ma ra@x.io",
            "m@.io",
        ] {
            assert!(normalize_email(bad).is_err(), "{bad:?} passed");
        }
    }

    #[test]
    fn sign_in_domains_limit_who_gets_a_link() {
        let none: Vec<String> = Vec::new();
        let kestrel = vec!["kestrel.example".to_owned()];
        assert!(domain_allowed("m.ortner@kestrel.example", &none));
        assert!(domain_allowed("m.ortner@kestrel.example", &kestrel));
        assert!(!domain_allowed("m.ortner@gmail.example", &kestrel));
    }

    #[test]
    fn the_sign_in_mail_has_both_parts_and_the_link() {
        let config = Config {
            database_url: String::new(),
            public_url: "https://app.tokencontroller.com".to_owned(),
            secret_key: String::new(),
            smtp_url: None,
            smtp_from: None,
            prices_refresh: true,
            customer_relations: crate::customer_relations::Settings::default(),
            stripe: None,
        };
        let url = link_url(&config, "tc_link_abc");
        assert_eq!(
            url,
            "https://app.tokencontroller.com/api/v1/sign-in/link?token=tc_link_abc"
        );
        let mail = sign_in_mail(&config, "m.ortner@kestrel.example", &url);
        assert_eq!(mail.subject, "Sign in to Token Controller");
        assert!(mail.text.contains(&url));
        assert!(mail.html.contains(&url));
        assert!(mail.text.contains("works once and for 15 minutes"));
        assert!(mail.html.contains("app.tokencontroller.com"));
        assert!(!mail.html.contains("{{"));
        assert!(!mail.text.contains("{{"));
        let none = no_account_mail(&config, "x@y.example");
        assert!(none.text.contains("https://app.tokencontroller.com/signup"));
        assert!(!none.html.contains("{{"));
    }
}
