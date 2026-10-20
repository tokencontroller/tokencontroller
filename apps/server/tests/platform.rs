//! The service platform against a real Postgres database: sign-in,
//! organizations, members, API tokens, tags, projects, tickets, status, the
//! MCP server, the plug-in points and the seed data.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use sqlx::PgPool;
use tokencontroller_core::NoExtension;
use tokencontroller_server::sign_in::secret;
use tokencontroller_server::{AppState, Config, router};
use tower::ServiceExt as _;
use uuid::Uuid;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

/// The secret key of this test run: made once, never written down.
fn key() -> &'static str {
    static KEY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    KEY.get_or_init(|| uuid::Uuid::new_v4().simple().to_string())
}

fn config() -> Config {
    Config {
        database_url: "postgres://unused".to_owned(),
        public_url: "http://localhost:8080".to_owned(),
        secret_key: key().to_owned(),
        smtp_url: None,
        smtp_from: None,
        price_table: None,
        rates_refresh: false,
        classifier: None,
    }
}

fn app(pool: &PgPool) -> Router {
    router(AppState::new(pool.clone(), config(), Arc::new(NoExtension)))
}

/// How a test request proves who it is.
#[derive(Clone, Copy)]
enum Auth<'a> {
    None,
    Cookie(&'a str),
    Bearer(&'a str),
}

struct Answer {
    status: StatusCode,
    headers: HeaderMap,
    json: Value,
    text: String,
}

async fn send(
    app: &Router,
    method: Method,
    path: &str,
    auth: Auth<'_>,
    body: Option<Value>,
) -> TestResult<Answer> {
    let mut request = Request::builder().method(method).uri(path);
    match auth {
        Auth::None => {}
        Auth::Cookie(token) => {
            request = request.header(header::COOKIE, format!("tc_session={token}"));
        }
        Auth::Bearer(token) => {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
    }
    let request = match body {
        Some(body) => request
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&body)?))?,
        None => request.body(Body::empty())?,
    };
    answer(app, request).await
}

async fn answer(app: &Router, request: Request<Body>) -> TestResult<Answer> {
    let response = app.clone().oneshot(request).await?;
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await?.to_bytes();
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    Ok(Answer {
        status,
        headers,
        json,
        text,
    })
}

async fn post_form(app: &Router, path: &str, form: &str) -> TestResult<Answer> {
    let request = Request::post(path)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(form.to_owned()))?;
    answer(app, request).await
}

/// A new organization with a signed-in org manager; returns the session.
async fn organization(pool: &PgPool, name: &str, email: &str) -> TestResult<(Uuid, String)> {
    let mut conn = pool.acquire().await?;
    let created = tokencontroller_server::organizations::create(&mut conn, name, email)
        .await
        .map_err(|error| error.to_string())?;
    let session = session_for(pool, created.person_id, "browser").await?;
    Ok((created.organization_id, session))
}

async fn session_for(pool: &PgPool, person_id: Uuid, client: &str) -> TestResult<String> {
    let prefix = if client == "local_tool" {
        secret::CLI
    } else {
        secret::SESSION
    };
    let session = secret::generate(prefix, key())?;
    sqlx::query("INSERT INTO sessions (token_hash, person_id, client, expires_at) VALUES ($1, $2, $3, now() + interval '1 day')")
        .bind(&session.hash)
        .bind(person_id)
        .bind(client)
        .execute(pool)
        .await?;
    Ok(session.token)
}

/// Stores a sign-in link the way the service does, and returns its token.
async fn link_for(pool: &PgPool, email: &str) -> TestResult<String> {
    let link = secret::generate(secret::LINK, key())?;
    sqlx::query("INSERT INTO sign_in_links (token_hash, email, expires_at) VALUES ($1, $2, now() + interval '15 minutes')")
        .bind(&link.hash)
        .bind(email)
        .execute(pool)
        .await?;
    Ok(link.token)
}

fn cookie_token(headers: &HeaderMap) -> Option<String> {
    let cookie = headers.get(header::SET_COOKIE)?.to_str().ok()?;
    let value = cookie.strip_prefix("tc_session=")?.split(';').next()?;
    Some(value.to_owned())
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_sign_up_link_creates_the_organization_and_signs_in(pool: PgPool) {
    let app = app(&pool);
    let sent = send(
        &app,
        Method::POST,
        "/api/v1/sign-up",
        Auth::None,
        Some(json!({
            "email": "N.Berger@Kestrel.Example",
            "organization_name": "Kestrel Digital",
            "name": " Nora Berger ",
            "cost_source": "seat",
            "reporting_currency": "eur",
            "manages": "people",
        })),
    )
    .await
    .unwrap();
    assert_eq!(sent.status, StatusCode::ACCEPTED);
    assert!(sent.json.get("poll_token").is_none());

    // The mail is not readable here, so make a fresh link for the same
    // pending sign-up by reading the organization name from the stored link.
    let token = secret::generate(secret::LINK, key()).unwrap();
    sqlx::query(
        "UPDATE sign_in_links SET token_hash = $1 WHERE email = 'n.berger@kestrel.example'",
    )
    .bind(&token.hash)
    .execute(&pool)
    .await
    .unwrap();

    let page = send(
        &app,
        Method::GET,
        &format!("/api/v1/sign-in/link?token={}", token.token),
        Auth::None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(page.status, StatusCode::OK);
    assert!(page.text.contains("Kestrel Digital"));
    assert!(page.text.contains(r#"method="post""#));
    // Worded as a sign-up, not a sign-in.
    assert!(page.text.contains("Create organization</button>"));
    // Opening the page twice, as a mail scanner would, uses nothing up.
    let again = send(
        &app,
        Method::GET,
        &format!("/api/v1/sign-in/link?token={}", token.token),
        Auth::None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(again.status, StatusCode::OK);

    let used = post_form(
        &app,
        "/api/v1/sign-in/link",
        &format!("token={}", token.token),
    )
    .await
    .unwrap();
    assert_eq!(used.status, StatusCode::SEE_OTHER);
    // On to the first steps for what the founder manages.
    assert_eq!(used.headers[header::LOCATION], "/?first_steps=people");
    let set_cookie = used.headers[header::SET_COOKIE].to_str().unwrap();
    assert!(set_cookie.contains("HttpOnly"));
    assert!(set_cookie.contains("SameSite=Lax"));
    let session = cookie_token(&used.headers).unwrap();

    let me = send(
        &app,
        Method::GET,
        "/api/v1/me",
        Auth::Cookie(&session),
        None,
    )
    .await
    .unwrap();
    assert_eq!(me.status, StatusCode::OK);
    assert_eq!(me.json["person"]["email"], "n.berger@kestrel.example");
    assert_eq!(me.json["person"]["name"], "Nora Berger");
    assert_eq!(me.json["person"]["role"], "org_manager");
    assert_eq!(me.json["person"]["status"], "joined");
    assert_eq!(me.json["organization"]["name"], "Kestrel Digital");
    assert_eq!(me.json["organization"]["slug"], "kestrel-digital");
    // The answers to the sign-up questions came with the link.
    let settings = send(
        &app,
        Method::GET,
        "/api/v1/organization",
        Auth::Cookie(&session),
        None,
    )
    .await
    .unwrap();
    assert_eq!(settings.json["reporting_currency"], "EUR");
    assert_eq!(settings.json["approval"], false);

    // The link works once.
    let twice = post_form(
        &app,
        "/api/v1/sign-in/link",
        &format!("token={}", token.token),
    )
    .await
    .unwrap();
    assert_eq!(twice.status, StatusCode::GONE);

    let out = send(
        &app,
        Method::POST,
        "/api/v1/sign-out",
        Auth::Cookie(&session),
        None,
    )
    .await
    .unwrap();
    assert_eq!(out.status, StatusCode::NO_CONTENT);
    let gone = send(
        &app,
        Method::GET,
        "/api/v1/me",
        Auth::Cookie(&session),
        None,
    )
    .await
    .unwrap();
    assert_eq!(gone.status, StatusCode::UNAUTHORIZED);
}

/// Gives the newest link for `email` a token the test knows: the mail
/// that carries the real one is not readable here.
async fn readable_link(pool: &PgPool, email: &str) -> TestResult<String> {
    let token = secret::generate(secret::LINK, key())?;
    sqlx::query(
        "UPDATE sign_in_links SET token_hash = $1
         WHERE token_hash = (SELECT token_hash FROM sign_in_links WHERE email = $2
                             ORDER BY created_at DESC LIMIT 1)",
    )
    .bind(&token.hash)
    .bind(email)
    .execute(pool)
    .await?;
    Ok(token.token)
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_no_account_mail_creates_the_organization_without_a_second_mail(pool: PgPool) {
    let app = app(&pool);
    let sent = send(
        &app,
        Method::POST,
        "/api/v1/sign-in",
        Auth::None,
        Some(json!({"email": "Jan@Example.com"})),
    )
    .await
    .unwrap();
    assert_eq!(sent.status, StatusCode::ACCEPTED);
    let token = readable_link(&pool, "jan@example.com").await.unwrap();

    // The web app's sign-up form reads the address; that uses nothing up.
    for _ in 0..2 {
        let link = send(
            &app,
            Method::GET,
            &format!("/api/v1/sign-up/link?token={token}"),
            Auth::None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(link.status, StatusCode::OK);
        assert_eq!(link.json, json!({"email": "jan@example.com"}));
    }

    let bad_currency = send(
        &app,
        Method::POST,
        "/api/v1/sign-up/link",
        Auth::None,
        Some(json!({"token": token, "organization_name": "Kestrel", "reporting_currency": "EURO"})),
    )
    .await
    .unwrap();
    assert_eq!(bad_currency.status, StatusCode::BAD_REQUEST);

    let created = send(
        &app,
        Method::POST,
        "/api/v1/sign-up/link",
        Auth::None,
        Some(json!({
            "token": token,
            "organization_name": "Kestrel Digital",
            "name": "Jan",
            "cost_source": "usage_at_api_rates",
            "reporting_currency": "EUR",
            "manages": "agents",
        })),
    )
    .await
    .unwrap();
    assert_eq!(created.status, StatusCode::NO_CONTENT);
    let session = cookie_token(&created.headers).unwrap();
    let me = send(
        &app,
        Method::GET,
        "/api/v1/me",
        Auth::Cookie(&session),
        None,
    )
    .await
    .unwrap();
    assert_eq!(me.status, StatusCode::OK);
    assert_eq!(me.json["person"]["email"], "jan@example.com");
    assert_eq!(me.json["person"]["name"], "Jan");
    assert_eq!(me.json["person"]["role"], "org_manager");
    assert_eq!(me.json["organization"]["name"], "Kestrel Digital");
    let settings = send(
        &app,
        Method::GET,
        "/api/v1/organization",
        Auth::Cookie(&session),
        None,
    )
    .await
    .unwrap();
    assert_eq!(settings.json["reporting_currency"], "EUR");
    // One link, one mail: nothing else was asked for.
    let links: i64 =
        sqlx::query_scalar("SELECT count(*) FROM sign_in_links WHERE email = 'jan@example.com'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(links, 1);

    // The link works once, for the form as for the page.
    let twice = send(
        &app,
        Method::POST,
        "/api/v1/sign-up/link",
        Auth::None,
        Some(json!({"token": token, "organization_name": "Kestrel Two"})),
    )
    .await
    .unwrap();
    assert_eq!(twice.status, StatusCode::GONE);
    let read = send(
        &app,
        Method::GET,
        &format!("/api/v1/sign-up/link?token={token}"),
        Auth::None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(read.status, StatusCode::GONE);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn only_a_no_account_link_opens_the_sign_up_form(pool: PgPool) {
    let app = app(&pool);
    let read = async |token: &str| {
        send(
            &app,
            Method::GET,
            &format!("/api/v1/sign-up/link?token={token}"),
            Auth::None,
            None,
        )
        .await
        .unwrap()
        .status
    };
    let create = async |token: &str| {
        send(
            &app,
            Method::POST,
            "/api/v1/sign-up/link",
            Auth::None,
            Some(json!({"token": token, "organization_name": "Elsewhere"})),
        )
        .await
        .unwrap()
        .status
    };

    // A member's sign-in link creates nothing, and still signs them in.
    organization(&pool, "Kestrel Digital", "n.berger@kestrel.example")
        .await
        .unwrap();
    let member = link_for(&pool, "n.berger@kestrel.example").await.unwrap();
    assert_eq!(read(&member).await, StatusCode::GONE);
    assert_eq!(create(&member).await, StatusCode::GONE);
    let used = post_form(&app, "/api/v1/sign-in/link", &format!("token={member}"))
        .await
        .unwrap();
    assert_eq!(used.status, StatusCode::SEE_OTHER);
    assert_eq!(used.headers[header::LOCATION], "/");

    // A sign-up link from the form already names its organization.
    send(
        &app,
        Method::POST,
        "/api/v1/sign-up",
        Auth::None,
        Some(json!({"email": "founder@acme.example", "organization_name": "Acme"})),
    )
    .await
    .unwrap();
    let named = readable_link(&pool, "founder@acme.example").await.unwrap();
    assert_eq!(read(&named).await, StatusCode::GONE);
    assert_eq!(create(&named).await, StatusCode::GONE);

    // A local tool's link: whoever started `tc connect` would collect a
    // session in the organization the person creates.
    let tool = send(
        &app,
        Method::POST,
        "/api/v1/sign-in",
        Auth::None,
        Some(json!({"email": "victim@example.com", "local_tool": true})),
    )
    .await
    .unwrap();
    let poll_token = tool.json["poll_token"].as_str().unwrap().to_owned();
    let tool_link = readable_link(&pool, "victim@example.com").await.unwrap();
    assert_eq!(read(&tool_link).await, StatusCode::GONE);
    assert_eq!(create(&tool_link).await, StatusCode::GONE);
    let poll = send(
        &app,
        Method::POST,
        "/api/v1/sign-in/poll",
        Auth::None,
        Some(json!({"poll_token": poll_token})),
    )
    .await
    .unwrap();
    assert_eq!(poll.status, StatusCode::ACCEPTED);

    // An expired link from the no-account mail.
    send(
        &app,
        Method::POST,
        "/api/v1/sign-in",
        Auth::None,
        Some(json!({"email": "late@example.com"})),
    )
    .await
    .unwrap();
    let late = readable_link(&pool, "late@example.com").await.unwrap();
    sqlx::query("UPDATE sign_in_links SET expires_at = now() - interval '1 second' WHERE email = 'late@example.com'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(read(&late).await, StatusCode::GONE);
    assert_eq!(create(&late).await, StatusCode::GONE);
    assert_eq!(read("tc_link_made_up").await, StatusCode::GONE);
    let organizations: i64 = sqlx::query_scalar("SELECT count(*) FROM organizations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(organizations, 1);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn sign_in_never_tells_whether_an_address_is_a_member(pool: PgPool) {
    let app = app(&pool);
    organization(&pool, "Kestrel Digital", "n.berger@kestrel.example")
        .await
        .unwrap();
    for email in ["n.berger@kestrel.example", "nobody@kestrel.example"] {
        let sent = send(
            &app,
            Method::POST,
            "/api/v1/sign-in",
            Auth::None,
            Some(json!({"email": email})),
        )
        .await
        .unwrap();
        assert_eq!(sent.status, StatusCode::ACCEPTED, "{email}");
        assert_eq!(sent.json, json!({}));
    }
    let bad = send(
        &app,
        Method::POST,
        "/api/v1/sign-in",
        Auth::None,
        Some(json!({"email": "not-an-address"})),
    )
    .await
    .unwrap();
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn too_many_links_for_one_address_are_refused(pool: PgPool) {
    let app = app(&pool);
    let body = json!({"email": "mara@kestrel.example"});
    for _ in 0..5 {
        let sent = send(
            &app,
            Method::POST,
            "/api/v1/sign-in",
            Auth::None,
            Some(body.clone()),
        )
        .await
        .unwrap();
        assert_eq!(sent.status, StatusCode::ACCEPTED);
    }
    let sixth = send(
        &app,
        Method::POST,
        "/api/v1/sign-in",
        Auth::None,
        Some(body),
    )
    .await
    .unwrap();
    assert_eq!(sixth.status, StatusCode::TOO_MANY_REQUESTS);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn an_expired_link_no_longer_works(pool: PgPool) {
    let app = app(&pool);
    organization(&pool, "Kestrel Digital", "n.berger@kestrel.example")
        .await
        .unwrap();
    let token = link_for(&pool, "n.berger@kestrel.example").await.unwrap();
    sqlx::query("UPDATE sign_in_links SET expires_at = now() - interval '1 second'")
        .execute(&pool)
        .await
        .unwrap();
    let page = send(
        &app,
        Method::GET,
        &format!("/api/v1/sign-in/link?token={token}"),
        Auth::None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(page.status, StatusCode::GONE);
    let used = post_form(&app, "/api/v1/sign-in/link", &format!("token={token}"))
        .await
        .unwrap();
    assert_eq!(used.status, StatusCode::GONE);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_local_tool_collects_its_session_once(pool: PgPool) {
    let app = app(&pool);
    let (_, browser) = organization(&pool, "Kestrel Digital", "n.berger@kestrel.example")
        .await
        .unwrap();
    let sent = send(
        &app,
        Method::POST,
        "/api/v1/sign-in",
        Auth::None,
        Some(json!({"email": "n.berger@kestrel.example", "local_tool": true})),
    )
    .await
    .unwrap();
    let poll_token = sent.json["poll_token"].as_str().unwrap().to_owned();
    assert!(poll_token.starts_with("tc_poll_"));

    let poll = json!({"poll_token": poll_token});
    let waiting = send(
        &app,
        Method::POST,
        "/api/v1/sign-in/poll",
        Auth::None,
        Some(poll.clone()),
    )
    .await
    .unwrap();
    assert_eq!(waiting.status, StatusCode::ACCEPTED);
    assert_eq!(waiting.json["status"], "waiting");

    // The person opens the mail's link in a browser.
    let link = secret::generate(secret::LINK, key()).unwrap();
    sqlx::query("UPDATE sign_in_links SET token_hash = $1 WHERE poll_hash IS NOT NULL")
        .bind(&link.hash)
        .execute(&pool)
        .await
        .unwrap();
    let used = post_form(
        &app,
        "/api/v1/sign-in/link",
        &format!("token={}", link.token),
    )
    .await
    .unwrap();
    assert_eq!(used.status, StatusCode::OK);
    assert!(used.text.contains("terminal"));
    assert!(used.headers.get(header::SET_COOKIE).is_none());

    let signed_in = send(
        &app,
        Method::POST,
        "/api/v1/sign-in/poll",
        Auth::None,
        Some(poll.clone()),
    )
    .await
    .unwrap();
    assert_eq!(signed_in.status, StatusCode::OK);
    let session = signed_in.json["session_token"].as_str().unwrap().to_owned();
    assert!(session.starts_with("tc_cli_"));

    let me = send(
        &app,
        Method::GET,
        "/api/v1/me",
        Auth::Bearer(&session),
        None,
    )
    .await
    .unwrap();
    assert_eq!(me.status, StatusCode::OK);
    assert_eq!(me.json["credential"], "session");
    // A local tool's token is not a cookie.
    let as_cookie = send(
        &app,
        Method::GET,
        "/api/v1/me",
        Auth::Cookie(&session),
        None,
    )
    .await
    .unwrap();
    assert_eq!(as_cookie.status, StatusCode::UNAUTHORIZED);
    // A CLI token edits its person's rules, never the organization.
    let settings = send(
        &app,
        Method::PATCH,
        "/api/v1/organization",
        Auth::Bearer(&session),
        Some(json!({"approval": true})),
    )
    .await
    .unwrap();
    assert_eq!(settings.status, StatusCode::FORBIDDEN);
    let token = send(
        &app,
        Method::POST,
        "/api/v1/api-tokens",
        Auth::Bearer(&session),
        Some(json!({"kind": "read"})),
    )
    .await
    .unwrap();
    assert_eq!(token.status, StatusCode::FORBIDDEN);
    // What `tc` does when connected: record, split and submit its person's
    // own messages, and keep their rules (docs: API > Tokens).
    let rules = send(
        &app,
        Method::PUT,
        "/api/v1/me/rules",
        Auth::Bearer(&session),
        Some(json!([{"name": "mine", "signal": "branch", "pattern": "(?P<key>KD-\\d+)", "tag_name": "feature"}])),
    )
    .await
    .unwrap();
    assert_eq!(rules.status, StatusCode::OK, "{}", rules.text);
    assert_eq!(rules.json[0]["name"], "mine");
    let recorded = send(
        &app,
        Method::POST,
        "/api/v1/token-entries",
        Auth::Bearer(&session),
        Some(json!({
            "agent_harness": "claude-code",
            "model": "claude-opus-5",
            "provider": "anthropic",
            "started_at": "2026-10-06T09:00:00Z",
            "ended_at": "2026-10-06T09:30:00Z",
            "tokens": {"input": 12000, "output": 3000, "cache_read": 0, "cache_write": 0},
            "session_id": "laptop-session-1",
            "local_id": "laptop-local-1"
        })),
    )
    .await
    .unwrap();
    assert_eq!(recorded.status, StatusCode::CREATED, "{}", recorded.text);
    assert_eq!(recorded.json["session_id"], "laptop-session-1");
    let split = send(
        &app,
        Method::POST,
        "/api/v1/splits",
        Auth::Bearer(&session),
        Some(json!({"session_id": "laptop-session-1", "parts": [{"tags": {"feature": "KD-1"}}]})),
    )
    .await
    .unwrap();
    assert_eq!(split.status, StatusCode::CREATED, "{}", split.text);
    let submitted = send(
        &app,
        Method::POST,
        "/api/v1/submissions",
        Auth::Bearer(&session),
        Some(json!({})),
    )
    .await
    .unwrap();
    assert_eq!(submitted.status, StatusCode::CREATED, "{}", submitted.text);
    assert_eq!(submitted.json["status"], "submitted");
    // Deciding on a submission stays with the browser.
    let decided = send(
        &app,
        Method::POST,
        &format!(
            "/api/v1/submissions/{}/approve",
            submitted.json["id"].as_str().unwrap()
        ),
        Auth::Bearer(&session),
        None,
    )
    .await
    .unwrap();
    assert_eq!(decided.status, StatusCode::FORBIDDEN);

    let again = send(
        &app,
        Method::POST,
        "/api/v1/sign-in/poll",
        Auth::None,
        Some(poll),
    )
    .await
    .unwrap();
    assert_eq!(again.status, StatusCode::GONE);

    // Under Settings the CLI token sits next to the API tokens, and
    // revoking it signs that machine out.
    let tokens = send(
        &app,
        Method::GET,
        "/api/v1/api-tokens",
        Auth::Cookie(&browser),
        None,
    )
    .await
    .unwrap();
    let listed = tokens.json.as_array().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["kind"], "cli");
    let shown = listed[0]["shown_prefix"].as_str().unwrap();
    assert!(session.starts_with(shown) && shown.len() == "tc_cli_xxxx".len());
    let made = send(
        &app,
        Method::POST,
        "/api/v1/api-tokens",
        Auth::Cookie(&browser),
        Some(json!({"kind": "cli"})),
    )
    .await
    .unwrap();
    assert_eq!(made.status, StatusCode::BAD_REQUEST);
    let id = listed[0]["id"].as_str().unwrap();
    let revoked = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/api-tokens/{id}"),
        Auth::Cookie(&browser),
        None,
    )
    .await
    .unwrap();
    assert_eq!(revoked.status, StatusCode::NO_CONTENT);
    let me = send(
        &app,
        Method::GET,
        "/api/v1/me",
        Auth::Bearer(&session),
        None,
    )
    .await
    .unwrap();
    assert_eq!(me.status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn an_org_manager_invites_and_the_invitee_joins(pool: PgPool) {
    let app = app(&pool);
    let (_, org_manager) = organization(&pool, "Kestrel Digital", "n.berger@kestrel.example")
        .await
        .unwrap();
    // `team` is a built-in attribute; its value `web` goes on record first.
    let web = send(
        &app,
        Method::POST,
        "/api/v1/attributes/team/values",
        Auth::Cookie(&org_manager),
        Some(json!({"key": "web", "title": "Web team"})),
    )
    .await
    .unwrap();
    assert_eq!(web.status, StatusCode::CREATED, "{}", web.text);

    let invited = send(
        &app,
        Method::POST,
        "/api/v1/members",
        Auth::Cookie(&org_manager),
        Some(json!({"email": "m.ortner@kestrel.example", "name": "Mara Ortner", "attributes": {"team": "web"}})),
    )
    .await
    .unwrap();
    assert_eq!(invited.status, StatusCode::CREATED, "{}", invited.text);
    assert_eq!(invited.json["member"]["role"], "agent_manager");
    assert_eq!(invited.json["member"]["status"], "invited");
    let url = invited.json["invite_url"].as_str().unwrap();
    let code = url.strip_prefix("http://localhost:8080/i/").unwrap();
    assert!(code.starts_with("kestrel-digital-"));

    let invite = send(
        &app,
        Method::GET,
        &format!("/api/v1/invites/{code}"),
        Auth::None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(invite.status, StatusCode::OK);
    assert_eq!(
        invite.json,
        json!({"organization_name": "Kestrel Digital", "email": "m.ortner@kestrel.example"})
    );

    let twice = send(
        &app,
        Method::POST,
        "/api/v1/members",
        Auth::Cookie(&org_manager),
        Some(json!({"email": "m.ortner@kestrel.example"})),
    )
    .await
    .unwrap();
    assert_eq!(twice.status, StatusCode::CONFLICT);

    let unknown_tag = send(
        &app,
        Method::POST,
        "/api/v1/members",
        Auth::Cookie(&org_manager),
        Some(json!({"email": "j.weber@kestrel.example", "attributes": {"guild": "rust"}})),
    )
    .await
    .unwrap();
    assert_eq!(unknown_tag.status, StatusCode::BAD_REQUEST);
    let unknown_value = send(
        &app,
        Method::POST,
        "/api/v1/members",
        Auth::Cookie(&org_manager),
        Some(json!({"email": "j.weber@kestrel.example", "attributes": {"team": "data"}})),
    )
    .await
    .unwrap();
    assert_eq!(unknown_value.status, StatusCode::BAD_REQUEST);

    let token = link_for(&pool, "m.ortner@kestrel.example").await.unwrap();
    let used = post_form(&app, "/api/v1/sign-in/link", &format!("token={token}"))
        .await
        .unwrap();
    let mara = cookie_token(&used.headers).unwrap();
    let me = send(&app, Method::GET, "/api/v1/me", Auth::Cookie(&mara), None)
        .await
        .unwrap();
    assert_eq!(me.json["person"]["status"], "joined");
    assert_eq!(me.json["person"]["attributes"], json!({"team": "web"}));
    // A new person records their own messages and views their own figures.
    let grants = send(
        &app,
        Method::GET,
        "/api/v1/grants",
        Auth::Cookie(&mara),
        None,
    )
    .await
    .unwrap();
    assert_eq!(grants.status, StatusCode::OK, "{}", grants.text);
    let mine: Vec<&str> = grants
        .json
        .as_array()
        .unwrap()
        .iter()
        .filter(|grant| grant["person"] == "m.ortner@kestrel.example")
        .map(|grant| grant["capability"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(mine, ["view"], "{}", grants.text);
    assert_eq!(
        grants.json[0]["filter"],
        json!({"person": "m.ortner@kestrel.example"})
    );

    // An agent manager cannot invite or change settings.
    let refused = send(
        &app,
        Method::POST,
        "/api/v1/members",
        Auth::Cookie(&mara),
        Some(json!({"email": "j.weber@kestrel.example"})),
    )
    .await
    .unwrap();
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    let settings = send(
        &app,
        Method::PATCH,
        "/api/v1/organization",
        Auth::Cookie(&mara),
        Some(json!({"approval": true})),
    )
    .await
    .unwrap();
    assert_eq!(settings.status, StatusCode::FORBIDDEN);

    let members = send(
        &app,
        Method::GET,
        "/api/v1/members",
        Auth::Cookie(&mara),
        None,
    )
    .await
    .unwrap();
    assert_eq!(members.json.as_array().unwrap().len(), 2);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn an_org_manager_changes_a_members_email(pool: PgPool) {
    let app = app(&pool);
    let (_, org_manager) = organization(&pool, "Kestrel Digital", "n.berger@kestrel.example")
        .await
        .unwrap();
    organization(&pool, "Osprey Labs", "o.lund@osprey.example")
        .await
        .unwrap();
    let me = send(
        &app,
        Method::GET,
        "/api/v1/me",
        Auth::Cookie(&org_manager),
        None,
    )
    .await
    .unwrap();
    let id = me.json["person"]["id"].as_str().unwrap().to_owned();
    let path = format!("/api/v1/members/{id}");

    for (email, status) in [
        ("not an address", StatusCode::BAD_REQUEST),
        ("o.lund@osprey.example", StatusCode::CONFLICT),
    ] {
        let refused = send(
            &app,
            Method::PATCH,
            &path,
            Auth::Cookie(&org_manager),
            Some(json!({"email": email})),
        )
        .await
        .unwrap();
        assert_eq!(refused.status, status, "{email}");
    }

    let changed = send(
        &app,
        Method::PATCH,
        &path,
        Auth::Cookie(&org_manager),
        Some(json!({"email": "  N.Berger+tc@Kestrel.example "})),
    )
    .await
    .unwrap();
    assert_eq!(changed.status, StatusCode::OK);
    assert_eq!(changed.json["email"], "n.berger+tc@kestrel.example");
    assert_eq!(changed.json["name"], me.json["person"]["name"]);

    let same = send(
        &app,
        Method::PATCH,
        &path,
        Auth::Cookie(&org_manager),
        Some(json!({"email": "n.berger+tc@kestrel.example"})),
    )
    .await
    .unwrap();
    assert_eq!(same.status, StatusCode::OK);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_last_org_manager_stays(pool: PgPool) {
    let app = app(&pool);
    let (organization_id, org_manager) =
        organization(&pool, "Kestrel Digital", "n.berger@kestrel.example")
            .await
            .unwrap();
    let me = send(
        &app,
        Method::GET,
        "/api/v1/me",
        Auth::Cookie(&org_manager),
        None,
    )
    .await
    .unwrap();
    let id = me.json["person"]["id"].as_str().unwrap().to_owned();

    let demote = send(
        &app,
        Method::PATCH,
        &format!("/api/v1/members/{id}"),
        Auth::Cookie(&org_manager),
        Some(json!({"role": "agent_manager"})),
    )
    .await
    .unwrap();
    assert_eq!(demote.status, StatusCode::CONFLICT);
    let remove = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/members/{id}"),
        Auth::Cookie(&org_manager),
        None,
    )
    .await
    .unwrap();
    assert_eq!(remove.status, StatusCode::CONFLICT);

    // With a second org manager, the first can leave, and their session ends.
    let second: Uuid = sqlx::query_scalar(
        "INSERT INTO people (organization_id, email, role, joined_at) VALUES ($1, 'p.shah@kestrel.example', 'org_manager', now()) RETURNING id",
    )
    .bind(organization_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let priya = session_for(&pool, second, "browser").await.unwrap();
    let remove = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/members/{id}"),
        Auth::Cookie(&priya),
        None,
    )
    .await
    .unwrap();
    assert_eq!(remove.status, StatusCode::NO_CONTENT);
    let gone = send(
        &app,
        Method::GET,
        "/api/v1/me",
        Auth::Cookie(&org_manager),
        None,
    )
    .await
    .unwrap();
    assert_eq!(gone.status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn organization_settings_change_and_stay_apart(pool: PgPool) {
    let app = app(&pool);
    let (_, kestrel) = organization(&pool, "Kestrel Digital", "n.berger@kestrel.example")
        .await
        .unwrap();
    let (_, other) = organization(&pool, "Kestrel Digital", "boss@other.example")
        .await
        .unwrap();

    let changed = send(
        &app,
        Method::PATCH,
        "/api/v1/organization",
        Auth::Cookie(&kestrel),
        Some(json!({"reporting_currency": "eur", "approval": true, "sign_in_domains": ["kestrel.example"]})),
    )
    .await
    .unwrap();
    assert_eq!(changed.status, StatusCode::OK, "{}", changed.text);
    assert_eq!(changed.json["reporting_currency"], "EUR");
    assert_eq!(changed.json["approval"], true);
    assert_eq!(changed.json["sign_in_domains"], json!(["kestrel.example"]));

    let cleared = send(
        &app,
        Method::PATCH,
        "/api/v1/organization",
        Auth::Cookie(&kestrel),
        Some(json!({"name": "Kestrel Digital GmbH"})),
    )
    .await
    .unwrap();
    assert_eq!(cleared.json["name"], "Kestrel Digital GmbH");
    assert_eq!(cleared.json["approval"], true);
    assert_eq!(cleared.json["reporting_currency"], "EUR");

    // Same name, another organization: its own short name and settings.
    let theirs = send(
        &app,
        Method::GET,
        "/api/v1/organization",
        Auth::Cookie(&other),
        None,
    )
    .await
    .unwrap();
    assert_ne!(theirs.json["slug"], "kestrel-digital");
    assert_eq!(theirs.json["approval"], false);

    let outside = send(
        &app,
        Method::POST,
        "/api/v1/members",
        Auth::Cookie(&kestrel),
        Some(json!({"email": "someone@gmail.example"})),
    )
    .await
    .unwrap();
    assert_eq!(outside.status, StatusCode::BAD_REQUEST);
    let member_elsewhere = send(
        &app,
        Method::POST,
        "/api/v1/members",
        Auth::Cookie(&kestrel),
        Some(json!({"email": "boss@kestrel.example"})),
    )
    .await
    .unwrap();
    assert_eq!(member_elsewhere.status, StatusCode::CREATED);
    // Members of another organization are out of reach.
    let id = member_elsewhere.json["member"]["id"].as_str().unwrap();
    let reach = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/members/{id}"),
        Auth::Cookie(&other),
        None,
    )
    .await
    .unwrap();
    assert_eq!(reach.status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn requests_from_another_site_are_refused(pool: PgPool) {
    let app = app(&pool);
    let (_, org_manager) = organization(&pool, "Kestrel Digital", "n.berger@kestrel.example")
        .await
        .unwrap();
    let request = Request::patch("/api/v1/organization")
        .header(header::COOKIE, format!("tc_session={org_manager}"))
        .header(header::ORIGIN, "https://evil.example")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"approval": true}"#))
        .unwrap();
    assert_eq!(
        answer(&app, request).await.unwrap().status,
        StatusCode::FORBIDDEN
    );
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn api_tokens_do_what_their_kind_allows(pool: PgPool) {
    let app = app(&pool);
    let (organization_id, org_manager) =
        organization(&pool, "Kestrel Digital", "n.berger@kestrel.example")
            .await
            .unwrap();
    let create = |kind: &'static str, session: String| {
        let app = app.clone();
        async move {
            send(
                &app,
                Method::POST,
                "/api/v1/api-tokens",
                Auth::Cookie(&session),
                Some(json!({"kind": kind, "name": "review bot"})),
            )
            .await
            .unwrap()
        }
    };
    let read = create("read", org_manager.clone()).await;
    assert_eq!(read.status, StatusCode::CREATED);
    let read_token = read.json["token"].as_str().unwrap().to_owned();
    assert!(read_token.starts_with("tc_read_"));
    assert_eq!(
        read.json["shown_prefix"].as_str().unwrap().len(),
        "tc_read_xxxx".len()
    );

    // A read token reads, and only reads.
    let me = send(
        &app,
        Method::GET,
        "/api/v1/me",
        Auth::Bearer(&read_token),
        None,
    )
    .await
    .unwrap();
    assert_eq!(me.json["credential"], "read");
    let write = send(
        &app,
        Method::POST,
        "/api/v1/tags",
        Auth::Bearer(&read_token),
        Some(json!({"name": "team"})),
    )
    .await
    .unwrap();
    assert_eq!(write.status, StatusCode::UNAUTHORIZED);

    // A service token reports token entries; it reads nothing here.
    let service = create("service", org_manager.clone()).await;
    let service_token = service.json["token"].as_str().unwrap().to_owned();
    let refused = send(
        &app,
        Method::GET,
        "/api/v1/me",
        Auth::Bearer(&service_token),
        None,
    )
    .await
    .unwrap();
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED);

    // Only a org_manager makes the organization token.
    let agent: Uuid = sqlx::query_scalar(
        "INSERT INTO people (organization_id, email, joined_at) VALUES ($1, 'm.ortner@kestrel.example', now()) RETURNING id",
    )
    .bind(organization_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let mara = session_for(&pool, agent, "browser").await.unwrap();
    assert_eq!(
        create("organization", mara.clone()).await.status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        create("organization", org_manager.clone()).await.status,
        StatusCode::CREATED
    );

    // Mara sees her tokens, the org_manager sees all.
    assert_eq!(
        create("service", mara.clone()).await.status,
        StatusCode::CREATED
    );
    let hers = send(
        &app,
        Method::GET,
        "/api/v1/api-tokens",
        Auth::Cookie(&mara),
        None,
    )
    .await
    .unwrap();
    assert_eq!(hers.json.as_array().unwrap().len(), 1);
    let all = send(
        &app,
        Method::GET,
        "/api/v1/api-tokens",
        Auth::Cookie(&org_manager),
        None,
    )
    .await
    .unwrap();
    assert_eq!(all.json.as_array().unwrap().len(), 4);
    assert!(all.json.to_string().find("tc_read_").is_some());
    assert!(!all.json.to_string().contains(&read_token));

    // The service token moves to Mara, then is revoked.
    let id = service.json["id"].as_str().unwrap();
    let moved = send(
        &app,
        Method::POST,
        &format!("/api/v1/api-tokens/{id}/hand-over"),
        Auth::Cookie(&org_manager),
        Some(json!({"person_id": agent})),
    )
    .await
    .unwrap();
    assert_eq!(moved.status, StatusCode::NO_CONTENT);
    let hers = send(
        &app,
        Method::GET,
        "/api/v1/api-tokens",
        Auth::Cookie(&mara),
        None,
    )
    .await
    .unwrap();
    assert_eq!(hers.json.as_array().unwrap().len(), 2);
    let revoked = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/api-tokens/{id}"),
        Auth::Cookie(&mara),
        None,
    )
    .await
    .unwrap();
    assert_eq!(revoked.status, StatusCode::NO_CONTENT);
    let read_revoked = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/api-tokens/{}", read.json["id"].as_str().unwrap()),
        Auth::Cookie(&org_manager),
        None,
    )
    .await
    .unwrap();
    assert_eq!(read_revoked.status, StatusCode::NO_CONTENT);
    let me = send(
        &app,
        Method::GET,
        "/api/v1/me",
        Auth::Bearer(&read_token),
        None,
    )
    .await
    .unwrap();
    assert_eq!(me.status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn tag_names_and_values(pool: PgPool) {
    let app = app(&pool);
    let (_, org_manager) = organization(&pool, "Kestrel Digital", "n.berger@kestrel.example")
        .await
        .unwrap();
    let auth = Auth::Cookie(&org_manager);
    let added = send(
        &app,
        Method::POST,
        "/api/v1/tags",
        auth,
        Some(json!({"name": "epic"})),
    )
    .await
    .unwrap();
    assert_eq!(added.status, StatusCode::CREATED, "{}", added.text);
    // A name there already changes nothing; an attribute's name is taken.
    for (taken, status) in [
        ("epic", StatusCode::OK),
        ("project", StatusCode::OK),
        ("team", StatusCode::CONFLICT),
    ] {
        let duplicate = send(
            &app,
            Method::POST,
            "/api/v1/tags",
            auth,
            Some(json!({"name": taken})),
        )
        .await
        .unwrap();
        assert_eq!(duplicate.status, status, "{taken}");
    }
    let bad = send(
        &app,
        Method::POST,
        "/api/v1/tags",
        auth,
        Some(json!({"name": "Team Name"})),
    )
    .await
    .unwrap();
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);

    let names = send(&app, Method::GET, "/api/v1/tags", auth, None)
        .await
        .unwrap();
    let names: Vec<&str> = names
        .json
        .as_array()
        .unwrap()
        .iter()
        .map(|tag| tag["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["client", "feature", "project", "epic"]);

    // A value's defaults must be values on record.
    let no_client = send(
        &app,
        Method::POST,
        "/api/v1/tags/project/values",
        auth,
        Some(json!({"key": "shop", "defaults": {"client": "acme"}})),
    )
    .await
    .unwrap();
    assert_eq!(
        no_client.status,
        StatusCode::BAD_REQUEST,
        "{}",
        no_client.text
    );
    let acme = send(
        &app,
        Method::POST,
        "/api/v1/tags/client/values",
        auth,
        Some(json!({"key": "acme", "title": "Acme Corp"})),
    )
    .await
    .unwrap();
    assert_eq!(acme.status, StatusCode::CREATED, "{}", acme.text);
    let shop = send(
        &app,
        Method::POST,
        "/api/v1/tags/project/values",
        auth,
        Some(json!({"key": "shop", "defaults": {"client": "acme"}})),
    )
    .await
    .unwrap();
    assert_eq!(shop.status, StatusCode::CREATED, "{}", shop.text);
    assert_eq!(shop.json["defaults"], json!({"client": "acme"}));
    // The same key again supersedes the record; one value stays.
    let renamed = send(
        &app,
        Method::POST,
        "/api/v1/tags/project/values",
        auth,
        Some(json!({"key": "shop", "title": "Web shop", "defaults": {"client": "acme"}})),
    )
    .await
    .unwrap();
    assert_eq!(renamed.status, StatusCode::CREATED, "{}", renamed.text);
    let listed = send(&app, Method::GET, "/api/v1/tags", auth, None)
        .await
        .unwrap();
    let projects = listed
        .json
        .as_array()
        .unwrap()
        .iter()
        .find(|tag| tag["name"] == "project")
        .unwrap();
    assert_eq!(projects["values"].as_array().unwrap().len(), 1);
    assert_eq!(projects["values"][0]["title"], "Web shop");
    let unknown_name = send(
        &app,
        Method::POST,
        "/api/v1/tags/guild/values",
        auth,
        Some(json!({"key": "rust"})),
    )
    .await
    .unwrap();
    assert_eq!(unknown_name.status, StatusCode::NOT_FOUND);

    // Reversing the newer record brings the earlier one back; reversing
    // that too retires the value.
    let project_values = async || {
        let listed = send(&app, Method::GET, "/api/v1/tags", auth, None)
            .await
            .unwrap();
        listed
            .json
            .as_array()
            .unwrap()
            .iter()
            .find(|tag| tag["name"] == "project")
            .unwrap()["values"]
            .clone()
    };
    let reversed = send(
        &app,
        Method::POST,
        "/api/v1/reversals",
        auth,
        Some(json!({"event_id": renamed.json["id"]})),
    )
    .await
    .unwrap();
    assert_eq!(reversed.status, StatusCode::CREATED, "{}", reversed.text);
    let values = project_values().await;
    assert_eq!(values.as_array().unwrap().len(), 1);
    assert_eq!(values[0]["title"], Value::Null);
    assert_eq!(values[0]["id"], shop.json["id"]);
    let reversed = send(
        &app,
        Method::POST,
        "/api/v1/reversals",
        auth,
        Some(json!({"event_id": shop.json["id"]})),
    )
    .await
    .unwrap();
    assert_eq!(reversed.status, StatusCode::CREATED, "{}", reversed.text);
    assert_eq!(project_values().await, json!([]));

    let built_in = send(&app, Method::DELETE, "/api/v1/tags/project", auth, None)
        .await
        .unwrap();
    assert_eq!(built_in.status, StatusCode::CONFLICT);
    let removed = send(&app, Method::DELETE, "/api/v1/tags/epic", auth, None)
        .await
        .unwrap();
    assert_eq!(removed.status, StatusCode::NO_CONTENT);
    let gone = send(&app, Method::DELETE, "/api/v1/tags/epic", auth, None)
        .await
        .unwrap();
    assert_eq!(gone.status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn attributes_and_their_defaults_follow_people(pool: PgPool) {
    let app = app(&pool);
    let (_, org_manager) = organization(&pool, "Kestrel Digital", "n.berger@kestrel.example")
        .await
        .unwrap();
    let auth = Auth::Cookie(&org_manager);
    let names = send(&app, Method::GET, "/api/v1/attributes", auth, None)
        .await
        .unwrap();
    let names: Vec<&str> = names
        .json
        .as_array()
        .unwrap()
        .iter()
        .map(|name| name["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["department", "location", "team"]);

    let no_department = send(
        &app,
        Method::POST,
        "/api/v1/attributes/team/values",
        auth,
        Some(json!({"key": "web", "defaults": {"department": "engineering"}})),
    )
    .await
    .unwrap();
    assert_eq!(no_department.status, StatusCode::BAD_REQUEST);
    let engineering = send(
        &app,
        Method::POST,
        "/api/v1/attributes/department/values",
        auth,
        Some(json!({"key": "engineering"})),
    )
    .await
    .unwrap();
    assert_eq!(
        engineering.status,
        StatusCode::CREATED,
        "{}",
        engineering.text
    );
    let web = send(
        &app,
        Method::POST,
        "/api/v1/attributes/team/values",
        auth,
        Some(json!({"key": "web", "defaults": {"department": "engineering"}})),
    )
    .await
    .unwrap();
    assert_eq!(web.status, StatusCode::CREATED, "{}", web.text);
    assert_eq!(web.json["defaults"], json!({"department": "engineering"}));

    let me = send(&app, Method::GET, "/api/v1/me", auth, None)
        .await
        .unwrap();
    let id = me.json["person"]["id"].as_str().unwrap().to_owned();
    let unknown = send(
        &app,
        Method::POST,
        &format!("/api/v1/people/{id}/attributes"),
        auth,
        Some(json!({"name": "team", "key": "data"})),
    )
    .await
    .unwrap();
    assert_eq!(unknown.status, StatusCode::BAD_REQUEST);
    let set = send(
        &app,
        Method::POST,
        &format!("/api/v1/people/{id}/attributes"),
        auth,
        Some(json!({"name": "team", "key": "web", "from": "2026-03-01"})),
    )
    .await
    .unwrap();
    assert_eq!(set.status, StatusCode::CREATED, "{}", set.text);
    let members = send(&app, Method::GET, "/api/v1/members", auth, None)
        .await
        .unwrap();
    assert_eq!(
        members.json[0]["attributes"]["team"], "web",
        "{}",
        members.text
    );
    let nobody = send(
        &app,
        Method::POST,
        &format!("/api/v1/people/{}/attributes", Uuid::new_v4()),
        auth,
        Some(json!({"name": "team", "key": "web"})),
    )
    .await
    .unwrap();
    assert_eq!(nobody.status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_status_endpoint_shows_each_part(pool: PgPool) {
    let app = app(&pool);
    let fresh = send(&app, Method::GET, "/api/v1/status", Auth::None, None)
        .await
        .unwrap();
    assert_eq!(fresh.status, StatusCode::OK);
    assert_eq!(fresh.json["status"], "operational");
    let ids: Vec<&str> = fresh.json["components"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        [
            "service",
            "bill-pulls",
            "ticket-sync",
            "submit-reminders",
            "price-table",
            "exchange-rates",
            "sign-in-email"
        ]
    );
    assert_eq!(fresh.json["components"][0]["state"], "operational");
    assert_eq!(fresh.json["components"][1]["state"], "not_running");
    // Without TC_SMTP_URL no sign-in mail goes out.
    assert_eq!(fresh.json["components"][6]["state"], "not_running");

    let mut with_mail = config();
    with_mail.smtp_url = Some("smtp://localhost:2525".to_owned());
    let mail_app = router(AppState::new(
        pool.clone(),
        with_mail,
        Arc::new(NoExtension),
    ));
    let mail_state = async |app: &Router| {
        let answer = send(app, Method::GET, "/api/v1/status", Auth::None, None)
            .await
            .unwrap();
        (
            answer.json["components"][6]["state"].clone(),
            answer.json["status"].clone(),
        )
    };
    assert_eq!(
        mail_state(&mail_app).await,
        (json!("operational"), json!("operational"))
    );
    let deliver = async |kind: &str, sent: bool| {
        sqlx::query("INSERT INTO mail_deliveries (kind, sent) VALUES ($1, $2)")
            .bind(kind)
            .bind(sent)
            .execute(&pool)
            .await
            .unwrap();
    };
    deliver("sign_in", true).await;
    deliver("invite", true).await;
    deliver("sign_in", false).await;
    // Mail of other kinds does not count.
    deliver("budget_report", false).await;
    assert_eq!(
        mail_state(&mail_app).await,
        (json!("degraded"), json!("degraded"))
    );
    deliver("no_account", false).await;
    assert_eq!(mail_state(&mail_app).await, (json!("down"), json!("down")));
    // Mail older than 15 minutes no longer counts.
    sqlx::query("UPDATE mail_deliveries SET created_at = now() - interval '16 minutes'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        mail_state(&mail_app).await,
        (json!("operational"), json!("operational"))
    );
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_mcp_server_reads_the_organization(pool: PgPool) {
    let app = app(&pool);
    tokencontroller_server::seed::load(&pool).await.unwrap();
    let nadia = session_for(&pool, tokencontroller_server::seed::ids::NADIA, "browser")
        .await
        .unwrap();
    let read = send(
        &app,
        Method::POST,
        "/api/v1/api-tokens",
        Auth::Cookie(&nadia),
        Some(json!({"kind": "read"})),
    )
    .await
    .unwrap();
    let token = read.json["token"].as_str().unwrap().to_owned();
    let rpc = |method: &str, params: Value| json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});

    let init = send(
        &app,
        Method::POST,
        "/api/v1/mcp",
        Auth::Bearer(&token),
        Some(rpc("initialize", json!({"protocolVersion": "2025-06-18"}))),
    )
    .await
    .unwrap();
    assert_eq!(
        init.json["result"]["serverInfo"]["name"],
        "token-controller"
    );
    let note = send(
        &app,
        Method::POST,
        "/api/v1/mcp",
        Auth::Bearer(&token),
        Some(json!({"jsonrpc": "2.0", "method": "notifications/initialized"})),
    )
    .await
    .unwrap();
    assert_eq!(note.status, StatusCode::ACCEPTED);
    let list = send(
        &app,
        Method::POST,
        "/api/v1/mcp",
        Auth::Bearer(&token),
        Some(rpc("tools/list", json!({}))),
    )
    .await
    .unwrap();
    let tools: Vec<&str> = list.json["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        tools,
        [
            "organization",
            "tag_values",
            "attributes",
            "people",
            "plans",
            "report"
        ]
    );

    let features = send(
        &app,
        Method::POST,
        "/api/v1/mcp",
        Auth::Bearer(&token),
        Some(rpc(
            "tools/call",
            json!({"name": "tag_values", "arguments": {"name": "feature", "query": "KD-2"}}),
        )),
    )
    .await
    .unwrap();
    let result = &features.json["result"]["structuredContent"]["result"];
    assert_eq!(result.as_array().unwrap().len(), 1, "{}", features.text);
    assert_eq!(result[0]["key"], "KD-2");
    assert_eq!(result[0]["defaults"]["project"], "shop");
    assert!(
        features.json["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Returns portal")
    );
    let report = send(
        &app,
        Method::POST,
        "/api/v1/mcp",
        Auth::Bearer(&token),
        Some(rpc(
            "tools/call",
            json!({"name": "report", "arguments": {"from": "2026-03-01", "to": "2026-03-31", "by": "feature"}}),
        )),
    )
    .await
    .unwrap();
    let rows = &report.json["result"]["structuredContent"]["result"]["rows"];
    assert!(
        rows.as_array()
            .is_some_and(|rows| rows.iter().any(|row| row["key"] == "KD-1")),
        "{}",
        report.text
    );

    let unknown = send(
        &app,
        Method::POST,
        "/api/v1/mcp",
        Auth::Bearer(&token),
        Some(rpc("tools/call", json!({"name": "delete_everything"}))),
    )
    .await
    .unwrap();
    assert_eq!(unknown.json["error"]["code"], -32602);
    let no_method = send(
        &app,
        Method::POST,
        "/api/v1/mcp",
        Auth::Bearer(&token),
        Some(rpc("resources/list", json!({}))),
    )
    .await
    .unwrap();
    assert_eq!(no_method.json["error"]["code"], -32601);
    let anonymous = send(
        &app,
        Method::POST,
        "/api/v1/mcp",
        Auth::None,
        Some(rpc("tools/list", json!({}))),
    )
    .await
    .unwrap();
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);
}

/// An Enterprise plan build as the tests see it: it keeps every audit
/// event, has an org manager notice and offers one sign-in provider.
#[derive(Default)]
struct Recorder {
    events: std::sync::Mutex<Vec<tokencontroller_core::extension::AuditEvent>>,
}

impl tokencontroller_core::Extension for Recorder {
    fn edition(&self) -> &'static str {
        "enterprise"
    }

    fn sign_in_providers(&self) -> Vec<tokencontroller_core::extension::SignInProvider> {
        vec![tokencontroller_core::extension::SignInProvider {
            id: "oidc".to_owned(),
            label: "Sign in with Okta".to_owned(),
        }]
    }

    fn record_audit_event(&self, event: &tokencontroller_core::extension::AuditEvent) {
        if let Ok(mut events) = self.events.lock() {
            events.push(event.clone());
        }
    }

    fn org_manager_notice(&self) -> Option<String> {
        Some("The Enterprise plan renews in 30 days.".to_owned())
    }
}

impl Recorder {
    /// Each event as `action actor target`, with `-` for none.
    fn log(&self) -> Vec<String> {
        self.events
            .lock()
            .map(|events| {
                events
                    .iter()
                    .map(|event| {
                        format!(
                            "{} {} {}",
                            event.action,
                            event.actor_id.as_deref().unwrap_or("-"),
                            event.target_id.as_deref().unwrap_or("-")
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn identity_providers_add_remove_and_sign_in_members(pool: PgPool) {
    use tokencontroller_server::organizations::{
        add_member, member_by_email, only_organization, remove_member,
    };
    use tokencontroller_server::seed::ids;

    assert_eq!(only_organization(&pool).await.unwrap(), None);
    tokencontroller_server::seed::load(&pool).await.unwrap();
    assert_eq!(
        only_organization(&pool).await.unwrap(),
        Some(ids::ORGANIZATION)
    );
    let recorder = Arc::new(Recorder::default());
    let state = AppState::new(pool.clone(), config(), recorder.clone());
    let app = router(state.clone());
    let org = ids::ORGANIZATION;

    assert_eq!(
        member_by_email(&pool, org, " M.Ortner@kestrel.example ")
            .await
            .unwrap(),
        Some(ids::MARA)
    );
    assert_eq!(
        member_by_email(&pool, org, "nobody@kestrel.example")
            .await
            .unwrap(),
        None
    );

    // SCIM adds a person; adding them again changes nothing but the name.
    let ana = add_member(&state, org, "a.weiss@kestrel.example", Some("Ana Weiss"))
        .await
        .unwrap();
    assert_eq!(
        add_member(&state, org, "A.Weiss@kestrel.example", None)
            .await
            .unwrap(),
        ana
    );
    let member = tokencontroller_server::organizations::member(&state, org, ana)
        .await
        .unwrap();
    assert_eq!(member.name, "Ana Weiss");
    assert_eq!(member.role.as_str(), "agent_manager");

    // One account belongs to one organization.
    let (_, _) = organization(&pool, "Globex", "g.boss@globex.example")
        .await
        .unwrap();
    assert_eq!(only_organization(&pool).await.unwrap(), None);
    assert!(
        add_member(&state, org, "g.boss@globex.example", None)
            .await
            .is_err()
    );

    // Removed by the identity provider, then brought back with their role.
    remove_member(&state, org, ana, None).await.unwrap();
    assert_eq!(
        member_by_email(&pool, org, "a.weiss@kestrel.example")
            .await
            .unwrap(),
        None
    );
    assert!(remove_member(&state, org, ana, None).await.is_err());
    assert_eq!(
        add_member(&state, org, "a.weiss@kestrel.example", None)
            .await
            .unwrap(),
        ana
    );
    // Nadia is the only org_manager.
    assert!(remove_member(&state, org, ids::NADIA, None).await.is_err());

    // OIDC signs Mara in with the same answer a magic link gives.
    let signed_in = tokencontroller_server::sign_in::start_session(&state, ids::MARA)
        .await
        .unwrap();
    assert_eq!(signed_in.status(), StatusCode::SEE_OTHER);
    let session = cookie_token(signed_in.headers()).unwrap();
    let me = send(
        &app,
        Method::GET,
        "/api/v1/me",
        Auth::Cookie(&session),
        None,
    )
    .await
    .unwrap();
    assert_eq!(me.json["person"]["email"], "m.ortner@kestrel.example");
    // The notice is for controllers only.
    assert_eq!(me.json["org_manager_notice"], Value::Null);
    assert!(
        tokencontroller_server::sign_in::start_session(&state, Uuid::nil())
            .await
            .is_err()
    );

    // Who a request is signed in as, for the enterprise settings.
    let mut headers = HeaderMap::new();
    assert!(
        tokencontroller_server::sign_in::signed_in(&state, &headers)
            .await
            .unwrap()
            .is_none()
    );
    headers.insert(
        header::COOKIE,
        format!("tc_session={session}").parse().unwrap(),
    );
    let caller = tokencontroller_server::sign_in::signed_in(&state, &headers)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(caller.person_id, ids::MARA);
    headers.insert(header::ORIGIN, "https://evil.example".parse().unwrap());
    assert!(
        tokencontroller_server::sign_in::signed_in(&state, &headers)
            .await
            .unwrap()
            .is_none()
    );

    let nadia = session_for(&pool, ids::NADIA, "browser").await.unwrap();
    let me = send(&app, Method::GET, "/api/v1/me", Auth::Cookie(&nadia), None)
        .await
        .unwrap();
    assert_eq!(
        me.json["org_manager_notice"],
        "The Enterprise plan renews in 30 days."
    );
    let removed = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/members/{ana}"),
        Auth::Cookie(&nadia),
        None,
    )
    .await
    .unwrap();
    assert_eq!(removed.status, StatusCode::NO_CONTENT);

    let providers = send(
        &app,
        Method::GET,
        "/api/v1/sign-in/providers",
        Auth::None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        providers.json,
        json!([{"id": "oidc", "label": "Sign in with Okta", "url": "/api/v1/sign-in/providers/oidc"}])
    );
    let open = send(
        &self::app(&pool),
        Method::GET,
        "/api/v1/sign-in/providers",
        Auth::None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(open.json, json!([]));

    assert_eq!(
        recorder.log(),
        [
            format!("member.added - {ana}"),
            format!("member.removed - {ana}"),
            format!("member.restored - {ana}"),
            format!("member.signed_in {} {}", ids::MARA, ids::MARA),
            format!("member.removed {} {ana}", ids::NADIA),
        ]
    );
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn every_change_reaches_the_audit_log(pool: PgPool) {
    let recorder = Arc::new(Recorder::default());
    let app = router(AppState::new(pool.clone(), config(), recorder.clone()));
    let (_, nadia) = organization(&pool, "Kestrel Digital", "n.berger@kestrel.example")
        .await
        .unwrap();
    let call = async |method: Method, path: &str, body: Value| {
        let answer = send(&app, method, path, Auth::Cookie(&nadia), Some(body))
            .await
            .unwrap();
        assert!(answer.status.is_success(), "{path}: {}", answer.text);
        answer.json
    };
    call(
        Method::PATCH,
        "/api/v1/organization",
        json!({"approval": true}),
    )
    .await;
    call(Method::POST, "/api/v1/tags", json!({"name": "epic"})).await;
    call(
        Method::POST,
        "/api/v1/tags/client/values",
        json!({"key": "acme"}),
    )
    .await;
    call(
        Method::POST,
        "/api/v1/attributes/team/values",
        json!({"key": "web"}),
    )
    .await;
    let invited = call(
        Method::POST,
        "/api/v1/members",
        json!({"email": "m.ortner@kestrel.example", "name": "Mara Ortner", "role": "agent_manager"}),
    )
    .await;
    let mara = invited["member"]["id"].as_str().unwrap().to_owned();
    call(
        Method::PATCH,
        &format!("/api/v1/members/{mara}"),
        json!({"role": "people_manager"}),
    )
    .await;
    call(
        Method::PUT,
        &format!("/api/v1/members/{mara}/filters"),
        json!([{"team": "web"}]),
    )
    .await;
    let token = call(
        Method::POST,
        "/api/v1/api-tokens",
        json!({"kind": "service", "name": "CI"}),
    )
    .await;
    let token_id = token["id"].as_str().unwrap().to_owned();
    call(
        Method::POST,
        &format!("/api/v1/api-tokens/{token_id}/hand-over"),
        json!({"person_id": mara}),
    )
    .await;
    call(
        Method::POST,
        "/api/v1/rules",
        json!({"name": "feature key in branch", "signal": "branch", "pattern": "(?P<key>KD-\\d+)", "tag_name": "feature"}),
    )
    .await;
    let revoked = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/api-tokens/{token_id}"),
        Auth::Cookie(&nadia),
        None,
    )
    .await
    .unwrap();
    assert_eq!(revoked.status, StatusCode::NO_CONTENT);
    let removed_tag = send(
        &app,
        Method::DELETE,
        "/api/v1/tags/epic",
        Auth::Cookie(&nadia),
        None,
    )
    .await
    .unwrap();
    assert_eq!(removed_tag.status, StatusCode::NO_CONTENT);

    let actions: Vec<String> = recorder
        .log()
        .iter()
        .map(|line| line.split(' ').next().unwrap_or_default().to_owned())
        .collect();
    assert_eq!(
        actions,
        [
            "organization.settings_changed",
            "tag_name.added",
            "tag_value.recorded",
            "member.invited",
            "member.changed",
            "member.filters_changed",
            "api_token.created",
            "api_token.handed_over",
            "rule.added",
            "api_token.revoked",
        ]
    );
    // A new organization: its first sign-in creates it.
    let link = link_for(&pool, "founder@acme.example").await.unwrap();
    sqlx::query("UPDATE sign_in_links SET sign_up_organization_name = 'Acme' WHERE email = 'founder@acme.example'")
        .execute(&pool)
        .await
        .unwrap();
    let used = post_form(&app, "/api/v1/sign-in/link", &format!("token={link}"))
        .await
        .unwrap();
    assert_eq!(used.status, StatusCode::SEE_OTHER);
    let log = recorder.log();
    let last: Vec<&str> = log
        .iter()
        .rev()
        .take(2)
        .map(|line| line.split(' ').next().unwrap_or_default())
        .collect();
    assert_eq!(last, ["member.signed_in", "organization.created"]);
}

/// Hooks a build could add: a one-member limit.
struct TestHooks;

impl tokencontroller_server::hooks::ServiceHooks for TestHooks {
    fn member_limit<'a>(
        &'a self,
        _state: &'a AppState,
        _organization_id: Uuid,
    ) -> tokencontroller_server::hooks::HookFuture<
        'a,
        Option<tokencontroller_server::hooks::MemberLimit>,
    > {
        Box::pin(async {
            Ok(Some(tokencontroller_server::hooks::MemberLimit {
                max: 1,
                message: "This plan is for one person.".to_owned(),
            }))
        })
    }
}

fn hooked_app(pool: &PgPool) -> Router {
    router(
        AppState::new(pool.clone(), config(), Arc::new(NoExtension))
            .with_hooks(Arc::new(TestHooks)),
    )
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_hook_sets_the_member_limit(pool: PgPool) {
    let (organization_id, org_manager) =
        organization(&pool, "Kestrel Digital", "n.berger@kestrel.example")
            .await
            .unwrap();
    let invite = json!({"email": "m.ortner@kestrel.example", "name": "Mara Ortner"});

    let refused = send(
        &hooked_app(&pool),
        Method::POST,
        "/api/v1/members",
        Auth::Cookie(&org_manager),
        Some(invite.clone()),
    )
    .await
    .unwrap();
    assert_eq!(refused.status, StatusCode::CONFLICT);
    assert_eq!(refused.json["message"], "This plan is for one person.");

    // Without hooks the open build has no member limit.
    let state = AppState::new(pool.clone(), config(), Arc::new(NoExtension));
    let limit = tokencontroller_server::organizations::member_limit(&state, organization_id)
        .await
        .unwrap();
    assert_eq!(limit, None);
    let invited = send(
        &app(&pool),
        Method::POST,
        "/api/v1/members",
        Auth::Cookie(&org_manager),
        Some(invite),
    )
    .await
    .unwrap();
    assert_eq!(invited.status, StatusCode::CREATED, "{}", invited.text);
}

async fn member_limit_of(app: &Router, auth: Auth<'_>) -> TestResult<Answer> {
    send(
        app,
        Method::GET,
        "/api/v1/organization/member-limit",
        auth,
        None,
    )
    .await
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn every_member_reads_the_member_limit(pool: PgPool) {
    let (_, org_manager) = organization(&pool, "Kestrel Digital", "n.berger@kestrel.example")
        .await
        .unwrap();

    // The open build has no member limit.
    let open = member_limit_of(&app(&pool), Auth::Cookie(&org_manager))
        .await
        .unwrap();
    assert_eq!(open.status, StatusCode::OK, "{}", open.text);
    assert_eq!(
        open.json,
        json!({"members": 1, "member_limit": null, "message": null})
    );

    // Invited people count before they join. An agent manager reads it, and
    // so does a read token.
    let invited = send(
        &app(&pool),
        Method::POST,
        "/api/v1/members",
        Auth::Cookie(&org_manager),
        Some(json!({"email": "m.ortner@kestrel.example", "name": "Mara Ortner"})),
    )
    .await
    .unwrap();
    assert_eq!(invited.status, StatusCode::CREATED, "{}", invited.text);
    let mara = Uuid::parse_str(invited.json["member"]["id"].as_str().unwrap()).unwrap();
    let mara = session_for(&pool, mara, "browser").await.unwrap();
    let agent_manager = member_limit_of(&app(&pool), Auth::Cookie(&mara))
        .await
        .unwrap();
    assert_eq!(agent_manager.json["members"], 2);
    let read = send(
        &app(&pool),
        Method::POST,
        "/api/v1/api-tokens",
        Auth::Cookie(&org_manager),
        Some(json!({"kind": "read", "name": "Power BI"})),
    )
    .await
    .unwrap();
    let read = read.json["token"].as_str().unwrap();
    let by_token = member_limit_of(&app(&pool), Auth::Bearer(read))
        .await
        .unwrap();
    assert_eq!(by_token.json["members"], 2);
    let signed_out = member_limit_of(&app(&pool), Auth::None).await.unwrap();
    assert_eq!(signed_out.status, StatusCode::UNAUTHORIZED);

    // A build's hook sets a limit.
    let hooked = member_limit_of(&hooked_app(&pool), Auth::Cookie(&org_manager))
        .await
        .unwrap();
    assert_eq!(
        hooked.json,
        json!({"members": 2, "member_limit": 1, "message": "This plan is for one person."})
    );
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_build_adds_api_routes_with_their_own_state(pool: PgPool) {
    #[derive(Clone)]
    struct Added {
        app: AppState,
        greeting: &'static str,
    }
    impl axum::extract::FromRef<Added> for AppState {
        fn from_ref(added: &Added) -> Self {
            added.app.clone()
        }
    }
    /// Says hello to a signed-in member.
    #[utoipa::path(get, path = "/hello", responses((status = OK, body = String)))]
    async fn hello(
        axum::extract::State(added): axum::extract::State<Added>,
        member: tokencontroller_server::sign_in::Member,
    ) -> String {
        format!("{} {}", added.greeting, member.organization_id)
    }

    let (organization_id, org_manager) =
        organization(&pool, "Kestrel Digital", "n.berger@kestrel.example")
            .await
            .unwrap();
    let state = AppState::new(pool.clone(), config(), Arc::new(NoExtension));
    let added = Added {
        app: state.clone(),
        greeting: "hello",
    };
    let routes =
        || utoipa_axum::router::OpenApiRouter::<Added>::new().routes(utoipa_axum::routes!(hello));
    let committed = tokencontroller_server::openapi_json_with(
        utoipa_axum::router::OpenApiRouter::<Added>::new()
            .nest("/api/v1", routes())
            .into_openapi(),
    )
    .unwrap();
    assert!(committed.contains("\"/api/v1/hello\""));
    assert!(committed.contains("\"/api/v1/members\""));
    let extra = routes().with_state(added);
    let app = tokencontroller_server::router_with(state, extra);

    let answer = send(
        &app,
        Method::GET,
        "/api/v1/hello",
        Auth::Cookie(&org_manager),
        None,
    )
    .await
    .unwrap();
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.text, format!("hello {organization_id}"));
    assert!(answer.headers.contains_key("x-request-id"));
    let signed_out = send(&app, Method::GET, "/api/v1/hello", Auth::None, None)
        .await
        .unwrap();
    assert_eq!(signed_out.status, StatusCode::UNAUTHORIZED);
    let document = send(&app, Method::GET, "/api/v1/openapi.json", Auth::None, None)
        .await
        .unwrap();
    assert!(document.json["paths"]["/api/v1/hello"]["get"].is_object());
    assert!(document.json["paths"]["/api/v1/members"].is_object());
}

/// A database that ran migrations which have since left this repo, such as
/// the hosted service's billing table (ADR 0020), still starts.
#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn migrations_that_left_the_repo_do_not_stop_the_service(pool: PgPool) {
    sqlx::query(
        "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) \
         VALUES (20261012010500, 'billing subscriptions', true, '\\x00', 0)",
    )
    .execute(&pool)
    .await
    .unwrap();

    tokencontroller_server::MIGRATOR.run(&pool).await.unwrap();

    // Without passing over them, the migrator would refuse the database.
    let strict = sqlx::migrate!("./migrations");
    let refused = strict.run(&pool).await.unwrap_err();
    assert!(
        matches!(
            refused,
            sqlx::migrate::MigrateError::VersionMissing(20_261_012_010_500)
        ),
        "{refused}"
    );
}
