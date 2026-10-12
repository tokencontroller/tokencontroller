//! The service platform against a real Postgres database: sign-in,
//! organizations, members, API tokens, tags, projects, tickets, status, the
//! MCP server, billing and the seed data.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use sqlx::PgPool;
use tokencontroller_core::NoExtension;
use tokencontroller_server::billing::StripeConfig;
use tokencontroller_server::hosted::{self, Hosted};
use tokencontroller_server::sign_in::secret;
use tokencontroller_server::{AppState, Config, router};
use tower::ServiceExt as _;
use uuid::Uuid;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

const KEY: &str = "0123456789abcdef0123456789abcdef";
const WEBHOOK_SECRET: &str = "whsec_test";

fn config() -> Config {
    Config {
        database_url: "postgres://unused".to_owned(),
        public_url: "http://localhost:8080".to_owned(),
        secret_key: KEY.to_owned(),
        smtp_url: None,
        smtp_from: None,
        prices_refresh: false,
        embeddings: None,
    }
}

fn app(pool: &PgPool) -> Router {
    router(AppState::new(pool.clone(), config(), Arc::new(NoExtension)))
}

/// The hosted service with Team plan billing on.
fn billing_app(pool: &PgPool) -> Router {
    let settings = hosted::Settings {
        stripe: Some(StripeConfig {
            secret_key: "sk_test_unused".to_owned(),
            webhook_secret: WEBHOOK_SECRET.to_owned(),
        }),
        ..hosted::Settings::default()
    };
    let app = AppState::new(pool.clone(), config(), Arc::new(NoExtension));
    hosted::router(Hosted::new(app, Arc::new(settings)))
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

/// A new organization with a signed-in controller; returns the session.
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
    let session = secret::generate(prefix, KEY)?;
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
    let link = secret::generate(secret::LINK, KEY)?;
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
        })),
    )
    .await
    .unwrap();
    assert_eq!(sent.status, StatusCode::ACCEPTED);
    assert!(sent.json.get("poll_token").is_none());

    // The mail is not readable here, so make a fresh link for the same
    // pending sign-up by reading the organization name from the stored link.
    let token = secret::generate(secret::LINK, KEY).unwrap();
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
    assert_eq!(used.headers[header::LOCATION], "/");
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
    assert_eq!(me.json["person"]["role"], "controller");
    assert_eq!(me.json["person"]["status"], "joined");
    assert_eq!(me.json["organization"]["name"], "Kestrel Digital");
    assert_eq!(me.json["organization"]["slug"], "kestrel-digital");

    let map = send(
        &app,
        Method::GET,
        "/api/v1/tag-map",
        Auth::Cookie(&session),
        None,
    )
    .await
    .unwrap();
    assert_eq!(map.json["cost_center"], "person");
    assert_eq!(map.json["ladder"], json!(["ticket", "project"]));

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
    let link = secret::generate(secret::LINK, KEY).unwrap();
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
async fn a_controller_invites_and_the_invitee_joins(pool: PgPool) {
    let app = app(&pool);
    let (_, controller) = organization(&pool, "Kestrel Digital", "n.berger@kestrel.example")
        .await
        .unwrap();
    let tag = send(
        &app,
        Method::POST,
        "/api/v1/tags",
        Auth::Cookie(&controller),
        Some(json!({"name": "team"})),
    )
    .await
    .unwrap();
    assert_eq!(tag.status, StatusCode::CREATED);

    let invited = send(
        &app,
        Method::POST,
        "/api/v1/members",
        Auth::Cookie(&controller),
        Some(json!({"email": "m.ortner@kestrel.example", "name": "Mara Ortner", "tags": {"team": "web"}})),
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
        Auth::Cookie(&controller),
        Some(json!({"email": "m.ortner@kestrel.example"})),
    )
    .await
    .unwrap();
    assert_eq!(twice.status, StatusCode::CONFLICT);

    let unknown_tag = send(
        &app,
        Method::POST,
        "/api/v1/members",
        Auth::Cookie(&controller),
        Some(json!({"email": "j.weber@kestrel.example", "tags": {"guild": "rust"}})),
    )
    .await
    .unwrap();
    assert_eq!(unknown_tag.status, StatusCode::BAD_REQUEST);

    let token = link_for(&pool, "m.ortner@kestrel.example").await.unwrap();
    let used = post_form(&app, "/api/v1/sign-in/link", &format!("token={token}"))
        .await
        .unwrap();
    let mara = cookie_token(&used.headers).unwrap();
    let me = send(&app, Method::GET, "/api/v1/me", Auth::Cookie(&mara), None)
        .await
        .unwrap();
    assert_eq!(me.json["person"]["status"], "joined");
    assert_eq!(me.json["person"]["tags"], json!({"team": "web"}));

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
async fn the_last_controller_stays(pool: PgPool) {
    let app = app(&pool);
    let (organization_id, controller) =
        organization(&pool, "Kestrel Digital", "n.berger@kestrel.example")
            .await
            .unwrap();
    let me = send(
        &app,
        Method::GET,
        "/api/v1/me",
        Auth::Cookie(&controller),
        None,
    )
    .await
    .unwrap();
    let id = me.json["person"]["id"].as_str().unwrap().to_owned();

    let demote = send(
        &app,
        Method::PATCH,
        &format!("/api/v1/members/{id}"),
        Auth::Cookie(&controller),
        Some(json!({"role": "agent_manager"})),
    )
    .await
    .unwrap();
    assert_eq!(demote.status, StatusCode::CONFLICT);
    let remove = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/members/{id}"),
        Auth::Cookie(&controller),
        None,
    )
    .await
    .unwrap();
    assert_eq!(remove.status, StatusCode::CONFLICT);

    // With a second controller, the first can leave, and their session ends.
    let second: Uuid = sqlx::query_scalar(
        "INSERT INTO people (organization_id, email, role, joined_at) VALUES ($1, 'p.shah@kestrel.example', 'controller', now()) RETURNING id",
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
        Auth::Cookie(&controller),
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
        Some(json!({"cost_source": "seat", "reporting_currency": "eur", "approval": true, "sign_in_domains": ["kestrel.example"]})),
    )
    .await
    .unwrap();
    assert_eq!(changed.status, StatusCode::OK, "{}", changed.text);
    assert_eq!(changed.json["cost_source"], "seat");
    assert_eq!(changed.json["reporting_currency"], "EUR");
    assert_eq!(changed.json["approval"], true);
    assert_eq!(changed.json["sign_in_domains"], json!(["kestrel.example"]));

    let cleared = send(
        &app,
        Method::PATCH,
        "/api/v1/organization",
        Auth::Cookie(&kestrel),
        Some(json!({"cost_source": null})),
    )
    .await
    .unwrap();
    assert_eq!(cleared.json["cost_source"], Value::Null);
    assert_eq!(cleared.json["approval"], true);

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
    let (_, controller) = organization(&pool, "Kestrel Digital", "n.berger@kestrel.example")
        .await
        .unwrap();
    let request = Request::patch("/api/v1/organization")
        .header(header::COOKIE, format!("tc_session={controller}"))
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
    let (organization_id, controller) =
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
    let read = create("read", controller.clone()).await;
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
    let service = create("service", controller.clone()).await;
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

    // Only a controller makes the organization token.
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
        create("organization", controller.clone()).await.status,
        StatusCode::CREATED
    );

    // Mara sees her tokens, the controller sees all.
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
        Auth::Cookie(&controller),
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
        Auth::Cookie(&controller),
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
        Auth::Cookie(&controller),
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
async fn tags_and_the_tag_map(pool: PgPool) {
    let app = app(&pool);
    let (_, controller) = organization(&pool, "Kestrel Digital", "n.berger@kestrel.example")
        .await
        .unwrap();
    let auth = Auth::Cookie(&controller);
    for name in ["team", "client", "epic"] {
        let added = send(
            &app,
            Method::POST,
            "/api/v1/tags",
            auth,
            Some(json!({"name": name})),
        )
        .await
        .unwrap();
        assert_eq!(added.status, StatusCode::CREATED);
    }
    let duplicate = send(
        &app,
        Method::POST,
        "/api/v1/tags",
        auth,
        Some(json!({"name": "team"})),
    )
    .await
    .unwrap();
    assert_eq!(duplicate.status, StatusCode::CONFLICT);
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
    assert_eq!(
        names,
        ["person", "project", "ticket", "client", "epic", "team"]
    );

    let first = send(&app, Method::GET, "/api/v1/tag-map", auth, None)
        .await
        .unwrap();
    let changed = send(
        &app,
        Method::PUT,
        "/api/v1/tag-map",
        auth,
        Some(json!({"cost_center": "team", "ladder": ["ticket", "epic", "project", "client"]})),
    )
    .await
    .unwrap();
    assert_eq!(changed.status, StatusCode::OK);
    assert_eq!(changed.json["cost_center"], "team");
    assert_eq!(first.json["version"], 1);
    assert_eq!(changed.json["version"], 2);
    let current = send(&app, Method::GET, "/api/v1/tag-map", auth, None)
        .await
        .unwrap();
    assert_eq!(current.json["version"], 2);

    let unknown = send(
        &app,
        Method::PUT,
        "/api/v1/tag-map",
        auth,
        Some(json!({"cost_center": "guild", "ladder": []})),
    )
    .await
    .unwrap();
    assert_eq!(unknown.status, StatusCode::BAD_REQUEST);

    let in_use = send(&app, Method::DELETE, "/api/v1/tags/epic", auth, None)
        .await
        .unwrap();
    assert_eq!(in_use.status, StatusCode::CONFLICT);
    let built_in = send(&app, Method::DELETE, "/api/v1/tags/ticket", auth, None)
        .await
        .unwrap();
    assert_eq!(built_in.status, StatusCode::CONFLICT);
    send(
        &app,
        Method::PUT,
        "/api/v1/tag-map",
        auth,
        Some(json!({"cost_center": "person", "ladder": ["ticket", "project"]})),
    )
    .await
    .unwrap();
    let removed = send(&app, Method::DELETE, "/api/v1/tags/epic", auth, None)
        .await
        .unwrap();
    assert_eq!(removed.status, StatusCode::NO_CONTENT);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn projects_repositories_clients_and_tickets(pool: PgPool) {
    let app = app(&pool);
    let (_, controller) = organization(&pool, "Kestrel Digital", "n.berger@kestrel.example")
        .await
        .unwrap();
    let auth = Auth::Cookie(&controller);
    send(
        &app,
        Method::POST,
        "/api/v1/tags",
        auth,
        Some(json!({"name": "client"})),
    )
    .await
    .unwrap();

    let storefront = send(
        &app,
        Method::POST,
        "/api/v1/projects",
        auth,
        Some(json!({"name": "storefront", "ticket_key_pattern": "^KD-\\d+", "tags": {"client": "acme"}, "repositories": ["Kestrel/Storefront"]})),
    )
    .await
    .unwrap();
    assert_eq!(
        storefront.status,
        StatusCode::CREATED,
        "{}",
        storefront.text
    );
    assert_eq!(
        storefront.json["repositories"],
        json!(["kestrel/storefront"])
    );
    let storefront_id = storefront.json["id"].as_str().unwrap().to_owned();

    let taken = send(
        &app,
        Method::POST,
        "/api/v1/projects",
        auth,
        Some(json!({"name": "shop-v2", "repositories": ["kestrel/storefront"]})),
    )
    .await
    .unwrap();
    assert_eq!(taken.status, StatusCode::CONFLICT);
    assert!(
        taken.json["message"]
            .as_str()
            .unwrap()
            .contains("storefront")
    );
    // Nothing of the refused project stays behind.
    let projects = send(&app, Method::GET, "/api/v1/projects", auth, None)
        .await
        .unwrap();
    assert_eq!(projects.json.as_array().unwrap().len(), 1);

    let bad_pattern = send(
        &app,
        Method::POST,
        "/api/v1/projects",
        auth,
        Some(json!({"name": "x", "ticket_key_pattern": "KD-("})),
    )
    .await
    .unwrap();
    assert_eq!(bad_pattern.status, StatusCode::BAD_REQUEST);

    let claims = send(
        &app,
        Method::POST,
        "/api/v1/projects",
        auth,
        Some(json!({"name": "claims-portal", "tags": {"client": "globex"}})),
    )
    .await
    .unwrap();
    let claims_id = claims.json["id"].as_str().unwrap().to_owned();
    let clients = send(&app, Method::GET, "/api/v1/clients", auth, None)
        .await
        .unwrap();
    assert_eq!(
        clients.json,
        json!([{"name": "acme", "projects": ["storefront"]}, {"name": "globex", "projects": ["claims-portal"]}])
    );

    // A ticket lands on the project whose pattern matches its key.
    let epic = send(
        &app,
        Method::POST,
        "/api/v1/tickets",
        auth,
        Some(json!({"key": "KD-200", "title": "Checkout rebuild"})),
    )
    .await
    .unwrap();
    assert_eq!(epic.status, StatusCode::CREATED);
    assert_eq!(epic.json["project_id"], storefront_id.as_str());
    assert_eq!(epic.json["source"], "manual");
    let epic_id = epic.json["id"].as_str().unwrap().to_owned();
    let story = send(
        &app,
        Method::POST,
        "/api/v1/tickets",
        auth,
        Some(
            json!({"key": "KD-214", "title": " Fix checkout redirect loop ", "parent_id": epic_id}),
        ),
    )
    .await
    .unwrap();
    assert_eq!(story.json["title"], "Fix checkout redirect loop");
    let story_id = story.json["id"].as_str().unwrap().to_owned();
    let duplicate = send(
        &app,
        Method::POST,
        "/api/v1/tickets",
        auth,
        Some(json!({"key": "KD-214"})),
    )
    .await
    .unwrap();
    assert_eq!(duplicate.status, StatusCode::CONFLICT);

    // A ticket never sits inside itself, however deep.
    let cycle = send(
        &app,
        Method::PATCH,
        &format!("/api/v1/tickets/{epic_id}"),
        auth,
        Some(json!({"parent_id": story_id})),
    )
    .await
    .unwrap();
    assert_eq!(cycle.status, StatusCode::BAD_REQUEST);

    let moved = send(
        &app,
        Method::PATCH,
        &format!("/api/v1/tickets/{story_id}"),
        auth,
        Some(json!({"project_id": claims_id, "title": null})),
    )
    .await
    .unwrap();
    assert_eq!(moved.json["project_id"], claims_id.as_str());
    assert_eq!(moved.json["title"], Value::Null);
    assert_eq!(moved.json["parent_id"], epic_id.as_str());

    let found = send(&app, Method::GET, "/api/v1/tickets?q=checkout", auth, None)
        .await
        .unwrap();
    assert_eq!(found.json.as_array().unwrap().len(), 1);
    let children = send(
        &app,
        Method::GET,
        &format!("/api/v1/tickets?parent_id={epic_id}"),
        auth,
        None,
    )
    .await
    .unwrap();
    assert_eq!(children.json.as_array().unwrap().len(), 1);

    // Archiving frees the repository.
    let archived = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/projects/{storefront_id}"),
        auth,
        None,
    )
    .await
    .unwrap();
    assert_eq!(archived.status, StatusCode::NO_CONTENT);
    let reused = send(
        &app,
        Method::PATCH,
        &format!("/api/v1/projects/{claims_id}"),
        auth,
        Some(json!({"repositories": ["kestrel/storefront"], "ticket_key_pattern": "^CP-\\d+"})),
    )
    .await
    .unwrap();
    assert_eq!(reused.status, StatusCode::OK, "{}", reused.text);
    assert_eq!(reused.json["ticket_key_pattern"], "^CP-\\d+");
    let with_archived = send(
        &app,
        Method::GET,
        "/api/v1/projects?archived=true",
        auth,
        None,
    )
    .await
    .unwrap();
    assert_eq!(with_archived.json.as_array().unwrap().len(), 2);
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
            "price-tables",
            "sign-in-email"
        ]
    );
    assert_eq!(fresh.json["components"][0]["state"], "operational");
    assert_eq!(fresh.json["components"][1]["state"], "not_running");
    // Without TC_SMTP_URL no sign-in mail goes out.
    assert_eq!(fresh.json["components"][4]["state"], "not_running");

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
            answer.json["components"][4]["state"].clone(),
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
    let mara = session_for(&pool, tokencontroller_server::seed::ids::MARA, "browser")
        .await
        .unwrap();
    let read = send(
        &app,
        Method::POST,
        "/api/v1/api-tokens",
        Auth::Cookie(&mara),
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
    assert_eq!(list.json["result"]["tools"].as_array().unwrap().len(), 5);

    let tickets = send(
        &app,
        Method::POST,
        "/api/v1/mcp",
        Auth::Bearer(&token),
        Some(rpc(
            "tools/call",
            json!({"name": "tickets", "arguments": {"project": "storefront", "query": "KD-21"}}),
        )),
    )
    .await
    .unwrap();
    let result = &tickets.json["result"]["structuredContent"]["result"];
    assert_eq!(result.as_array().unwrap().len(), 1);
    assert_eq!(result[0]["key"], "KD-214");
    assert!(
        tickets.json["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Fix checkout redirect loop")
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

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_seed_is_the_docs_example_organization(pool: PgPool) {
    tokencontroller_server::seed::load(&pool).await.unwrap();
    tokencontroller_server::seed::load(&pool).await.unwrap();
    let app = app(&pool);
    let invite = send(
        &app,
        Method::GET,
        "/api/v1/invites/kestrel-4f2a",
        Auth::None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(invite.json["organization_name"], "Kestrel Digital");

    // The core crate sees the same organization.
    let directory = tokencontroller_server::projects::directory(
        &pool,
        tokencontroller_server::seed::ids::ORGANIZATION,
    )
    .await
    .unwrap();
    assert_eq!(directory.people.len(), 9);
    let mara = tokencontroller_core::TagValue::new("m.ortner@kestrel.example").unwrap();
    let ticket = tokencontroller_core::TagValue::new("KD-214").unwrap();
    assert_eq!(
        directory.project_of_ticket(&ticket).unwrap().as_str(),
        "storefront"
    );
    assert_eq!(
        directory
            .project_of_repository("kestrel/storefront")
            .unwrap()
            .as_str(),
        "storefront"
    );
    let storefront = tokencontroller_core::TagValue::new("storefront").unwrap();
    let inherited = directory.inherited_tags(Some(&mara), Some(&storefront), Some(&ticket));
    assert_eq!(inherited.get_str("team").unwrap().as_str(), "web");
    assert_eq!(inherited.get_str("client").unwrap().as_str(), "acme");
    let found = directory.find_ticket_keys("feature/KD-231-checkout");
    assert_eq!(found.first().unwrap().key.as_str(), "KD-231");

    let nadia = session_for(&pool, tokencontroller_server::seed::ids::NADIA, "browser")
        .await
        .unwrap();
    let members = send(
        &app,
        Method::GET,
        "/api/v1/members",
        Auth::Cookie(&nadia),
        None,
    )
    .await
    .unwrap();
    assert_eq!(members.json.as_array().unwrap().len(), 9);
    let priya = members
        .json
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["name"] == "Priya Shah")
        .unwrap();
    assert_eq!(priya["filters"], json!([{"team": "web"}]));
    let projects = send(
        &app,
        Method::GET,
        "/api/v1/projects",
        Auth::Cookie(&nadia),
        None,
    )
    .await
    .unwrap();
    assert_eq!(projects.json.as_array().unwrap().len(), 4);
    let values = send(
        &app,
        Method::GET,
        "/api/v1/tags/team/values",
        Auth::Cookie(&nadia),
        None,
    )
    .await
    .unwrap();
    assert_eq!(values.json[0], json!({"value": "data", "records": 2}));
}

fn stripe_signature(body: &str, timestamp: i64) -> TestResult<String> {
    use hmac::{Hmac, KeyInit as _, Mac as _};
    let mut mac = Hmac::<sha2::Sha256>::new_from_slice(WEBHOOK_SECRET.as_bytes())?;
    mac.update(format!("{timestamp}.{body}").as_bytes());
    Ok(format!(
        "t={timestamp},v1={}",
        hex::encode(mac.finalize().into_bytes())
    ))
}

async fn stripe_event(app: &Router, event: &Value) -> TestResult<StatusCode> {
    let body = event.to_string();
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let request = Request::post("/api/v1/billing/stripe-webhook")
        .header(header::CONTENT_TYPE, "application/json")
        .header("stripe-signature", stripe_signature(&body, now)?)
        .body(Body::from(body))?;
    Ok(answer(app, request).await?.status)
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn solo_is_one_person_until_the_team_plan_is_paid(pool: PgPool) {
    let app = billing_app(&pool);
    let (organization_id, controller) =
        organization(&pool, "Kestrel Digital", "n.berger@kestrel.example")
            .await
            .unwrap();
    let auth = Auth::Cookie(&controller);
    let billing = send(&app, Method::GET, "/api/v1/billing", auth, None)
        .await
        .unwrap();
    assert_eq!(billing.json["plan"], "solo");
    assert_eq!(billing.json["member_limit"], 1);
    // The People page reads the same limit from the public answer.
    let limit = member_limit_of(&app, auth).await.unwrap();
    assert_eq!(
        limit.json,
        json!({"members": 1, "member_limit": 1, "message": "The Solo plan is for one person. Choose the Team plan under Billing to invite people."})
    );

    let invite = json!({"email": "m.ortner@kestrel.example"});
    let refused = send(
        &app,
        Method::POST,
        "/api/v1/members",
        auth,
        Some(invite.clone()),
    )
    .await
    .unwrap();
    assert_eq!(refused.status, StatusCode::CONFLICT);
    assert!(
        refused.json["message"]
            .as_str()
            .unwrap()
            .contains("Solo plan")
    );

    let completed = json!({
        "type": "checkout.session.completed", "created": 1_760_000_000,
        "data": {"object": {"client_reference_id": organization_id, "customer": "cus_1", "subscription": "sub_1"}}
    });
    assert_eq!(
        stripe_event(&app, &completed).await.unwrap(),
        StatusCode::NO_CONTENT
    );
    let active = json!({
        "type": "customer.subscription.created", "created": 1_760_000_001,
        "data": {"object": {
            "id": "sub_1", "customer": "cus_1", "status": "active",
            "metadata": {"organization_id": organization_id},
            "items": {"data": [{"current_period_end": 1_762_600_000, "price": {"recurring": {"interval": "month"}}}]}
        }}
    });
    assert_eq!(
        stripe_event(&app, &active).await.unwrap(),
        StatusCode::NO_CONTENT
    );
    let billing = send(&app, Method::GET, "/api/v1/billing", auth, None)
        .await
        .unwrap();
    assert_eq!(billing.json["plan"], "team");
    assert_eq!(billing.json["interval"], "month");
    assert_eq!(billing.json["member_limit"], 150);
    let limit = member_limit_of(&app, auth).await.unwrap();
    assert_eq!(limit.json["member_limit"], 150);
    let invited = send(&app, Method::POST, "/api/v1/members", auth, Some(invite))
        .await
        .unwrap();
    assert_eq!(invited.status, StatusCode::CREATED);

    // A late event older than what was applied changes nothing.
    let late = json!({
        "type": "customer.subscription.updated", "created": 1_759_000_000,
        "data": {"object": {
            "id": "sub_1", "customer": "cus_1", "status": "canceled",
            "items": {"data": [{"price": {"recurring": {"interval": "month"}}}]}
        }}
    });
    assert_eq!(
        stripe_event(&app, &late).await.unwrap(),
        StatusCode::NO_CONTENT
    );
    let billing = send(&app, Method::GET, "/api/v1/billing", auth, None)
        .await
        .unwrap();
    assert_eq!(billing.json["plan"], "team");

    let ended = json!({
        "type": "customer.subscription.deleted", "created": 1_763_000_000,
        "data": {"object": {
            "id": "sub_1", "customer": "cus_1", "status": "canceled",
            "items": {"data": [{"price": {"recurring": {"interval": "month"}}}]}
        }}
    });
    assert_eq!(
        stripe_event(&app, &ended).await.unwrap(),
        StatusCode::NO_CONTENT
    );
    let billing = send(&app, Method::GET, "/api/v1/billing", auth, None)
        .await
        .unwrap();
    assert_eq!(billing.json["plan"], "solo");

    let unsigned = Request::post("/api/v1/billing/stripe-webhook")
        .header("stripe-signature", "t=1,v1=00")
        .body(Body::from(ended.to_string()))
        .unwrap();
    assert_eq!(
        answer(&app, unsigned).await.unwrap().status,
        StatusCode::BAD_REQUEST
    );
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn billing_is_off_without_stripe_settings(pool: PgPool) {
    let app = hosted::router(Hosted::new(
        AppState::new(pool.clone(), config(), Arc::new(NoExtension)),
        Arc::new(hosted::Settings::default()),
    ));
    let (_, controller) = organization(&pool, "Kestrel Digital", "n.berger@kestrel.example")
        .await
        .unwrap();
    let billing = send(
        &app,
        Method::GET,
        "/api/v1/billing",
        Auth::Cookie(&controller),
        None,
    )
    .await
    .unwrap();
    assert_eq!(billing.status, StatusCode::NOT_FOUND);
    let webhook = send(
        &app,
        Method::POST,
        "/api/v1/billing/stripe-webhook",
        Auth::None,
        Some(json!({})),
    )
    .await
    .unwrap();
    assert_eq!(webhook.status, StatusCode::NOT_FOUND);
    // A self-hosted organization invites without a plan.
    let invited = send(
        &app,
        Method::POST,
        "/api/v1/members",
        Auth::Cookie(&controller),
        Some(json!({"email": "m.ortner@kestrel.example"})),
    )
    .await
    .unwrap();
    assert_eq!(invited.status, StatusCode::CREATED);
}

/// An Enterprise plan build as the tests see it: it keeps every audit
/// event, has a controller notice and offers one sign-in provider.
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

    fn controller_notice(&self) -> Option<String> {
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
    let member = tokencontroller_server::organizations::member(&pool, org, ana)
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
    // Nadia is the only controller.
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
    assert_eq!(me.json["controller_notice"], Value::Null);
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
        me.json["controller_notice"],
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
    call(Method::POST, "/api/v1/tags", json!({"name": "team"})).await;
    call(
        Method::PUT,
        "/api/v1/tag-map",
        json!({"cost_center": "team", "ladder": ["ticket", "project"]}),
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
        Method::PUT,
        "/api/v1/organization/rules",
        json!({"several_keys": "first_named"}),
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
        "/api/v1/tags/team",
        Auth::Cookie(&nadia),
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        removed_tag.status,
        StatusCode::CONFLICT,
        "the tag map uses team"
    );

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
            "tag_map.changed",
            "member.invited",
            "member.changed",
            "member.filters_changed",
            "api_token.created",
            "api_token.handed_over",
            "organization.rules_changed",
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

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn rules_and_the_context_local_tools_read(pool: PgPool) {
    use tokencontroller_server::seed::ids;
    tokencontroller_server::seed::load(&pool).await.unwrap();
    let app = app(&pool);
    let mara = session_for(&pool, ids::MARA, "local_tool").await.unwrap();
    let nadia = session_for(&pool, ids::NADIA, "browser").await.unwrap();

    let context = send(
        &app,
        Method::GET,
        "/api/v1/me/context",
        Auth::Bearer(&mara),
        None,
    )
    .await
    .unwrap();
    assert_eq!(context.status, StatusCode::OK, "{}", context.text);
    let organization = &context.json["organization"];
    assert_eq!(organization["name"], "Kestrel Digital");
    assert_eq!(organization["currency"], "USD");
    assert_eq!(organization["several_keys"], "most_often");
    assert!(
        organization["tag_names"]
            .as_array()
            .unwrap()
            .contains(&json!("team"))
    );
    assert_eq!(organization["rules"][0]["name"], "ticket key in branch");
    assert_eq!(
        organization["rules"][0]["then"],
        json!({"ticket_key_in": "branch"})
    );
    assert_eq!(organization["rules"][3]["then"], "repository_project");
    assert_eq!(context.json["person"]["email"], "m.ortner@kestrel.example");
    assert_eq!(context.json["person"]["tags"]["team"], "web");
    assert_eq!(context.json["rules"], json!([]));
    let storefront = context.json["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|project| project["name"] == "storefront")
        .unwrap();
    assert_eq!(storefront["ticket_key_pattern"], "^KD-\\d+");
    assert_eq!(storefront["repositories"], json!(["kestrel/storefront"]));
    assert_eq!(context.json["cadence"], Value::Null);
    assert_eq!(context.json["rejected"], json!([]));

    // Personal rules in the core shape.
    let review = json!({
        "name": "review branches",
        "when": [{"signal": "branch", "test": {"starts_with": "review/"}}],
        "then": {"set_tags": {"purpose": "code-review"}}
    });
    let put = |rules: Value| {
        let app = app.clone();
        let mara = mara.clone();
        async move {
            send(
                &app,
                Method::PUT,
                "/api/v1/me/rules",
                Auth::Bearer(&mara),
                Some(json!({"rules": rules})),
            )
            .await
            .unwrap()
        }
    };
    let saved = put(json!([review])).await;
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.text);
    assert_eq!(saved.json["rules"], json!([review]));
    let unknown_tag =
        put(json!([{"name": "colour", "then": {"set_tags": {"colour": "red"}}}])).await;
    assert_eq!(unknown_tag.status, StatusCode::BAD_REQUEST);
    let person =
        put(json!([{"name": "me", "then": {"set_tags": {"person": "k.novak@kestrel.example"}}}]))
            .await;
    assert_eq!(person.status, StatusCode::BAD_REQUEST);
    let not_a_rule = put(json!([{"name": "x", "then": "guess"}])).await;
    assert!(not_a_rule.status.is_client_error());

    let directory = tokencontroller_server::projects::directory(&pool, ids::ORGANIZATION)
        .await
        .unwrap();
    let mara_rules = &directory.people
        [&tokencontroller_core::TagValue::new("m.ortner@kestrel.example").unwrap()]
        .rules;
    assert_eq!(mara_rules.len(), 1);
    assert_eq!(mara_rules[0].name, "review branches");

    // Organization rules: controllers change them; an empty list switches
    // every rule off.
    let refused = send(
        &app,
        Method::PUT,
        "/api/v1/organization/rules",
        Auth::Bearer(&mara),
        Some(json!({"rules": []})),
    )
    .await
    .unwrap();
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    let changed = send(
        &app,
        Method::PUT,
        "/api/v1/organization/rules",
        Auth::Cookie(&nadia),
        Some(json!({"rules": [], "several_keys": "equal_split", "summary_match": false})),
    )
    .await
    .unwrap();
    assert_eq!(changed.status, StatusCode::OK, "{}", changed.text);
    let read = send(
        &app,
        Method::GET,
        "/api/v1/organization/rules",
        Auth::Bearer(&mara),
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        read.json,
        json!({"rules": [], "several_keys": "equal_split", "summary_match": false})
    );
    let rules = tokencontroller_server::organizations::organization_rules(&pool, ids::ORGANIZATION)
        .await
        .unwrap();
    assert!(rules.rules.is_empty());
    assert!(!rules.summary_match);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn merging_tag_values_reaches_every_record_and_rule(pool: PgPool) {
    use tokencontroller_server::seed::ids;
    tokencontroller_server::seed::load(&pool).await.unwrap();
    let app = app(&pool);
    let mara = session_for(&pool, ids::MARA, "local_tool").await.unwrap();
    let nadia = session_for(&pool, ids::NADIA, "browser").await.unwrap();
    let call = |method: Method, path: String, auth: &str, body: Option<Value>| {
        let app = app.clone();
        let auth = auth.to_owned();
        async move {
            let auth = if auth.starts_with(secret::CLI) {
                Auth::Bearer(&auth)
            } else {
                Auth::Cookie(&auth)
            };
            send(&app, method, &path, auth, body).await.unwrap()
        }
    };
    let set_tags = |value: &str| json!({"set_tags": {"client": value}});

    // Hand-typed drift: "Acme Corp" next to the seed's "acme".
    let mine = call(
        Method::PUT,
        "/api/v1/me/rules".to_owned(),
        &mara,
        Some(json!({"rules": [{"name": "acme work", "then": set_tags("Acme Corp")}]})),
    )
    .await;
    assert_eq!(mine.status, StatusCode::OK, "{}", mine.text);
    let theirs = call(
        Method::PUT,
        "/api/v1/organization/rules".to_owned(),
        &nadia,
        Some(json!({"rules": [{"name": "acme repo", "then": set_tags("Acme Corp")}]})),
    )
    .await;
    assert_eq!(theirs.status, StatusCode::OK, "{}", theirs.text);
    let tagged = call(
        Method::PATCH,
        format!("/api/v1/members/{}", ids::MARA),
        &nadia,
        Some(json!({"tags": {"team": "web", "client": "Acme Corp"}})),
    )
    .await;
    assert_eq!(tagged.status, StatusCode::OK, "{}", tagged.text);
    let filters = call(
        Method::PUT,
        format!("/api/v1/members/{}/filters", ids::KAI),
        &nadia,
        Some(json!([{"client": "acme"}, {"client": "Acme Corp"}])),
    )
    .await;
    assert_eq!(filters.status, StatusCode::OK, "{}", filters.text);
    // A token entry that carries the drifted value.
    let split: Uuid = sqlx::query_scalar(
        "WITH entry AS (
             INSERT INTO token_entries (organization_id, agent_harness, model, provider, started_at,
                                        ended_at, status, on_token_sheet, door)
             VALUES ($1, 'claude_code', 'claude-sonnet', 'anthropic', now(), now(), 'submitted',
                     false, 'endpoint')
             RETURNING id)
         INSERT INTO splits (token_entry_id, position, method, placed_by)
         SELECT id, 0, 'whole', 'by hand' FROM entry RETURNING id",
    )
    .bind(ids::ORGANIZATION)
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO split_tags (split_id, organization_id, name, value, source)
         VALUES ($1, $2, 'client', 'Acme Corp', 'sent')",
    )
    .bind(split)
    .bind(ids::ORGANIZATION)
    .execute(&pool)
    .await
    .unwrap();

    let merge = |name: &str, body: Value, auth: &str| {
        call(
            Method::POST,
            format!("/api/v1/tags/{name}/merge"),
            auth,
            Some(body),
        )
    };
    let body = json!({"from": ["Acme Corp"], "into": "acme"});
    assert_eq!(
        merge("client", body.clone(), &mara).await.status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        merge("ticket", body.clone(), &nadia).await.status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        merge("colour", body.clone(), &nadia).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        merge("client", json!({"from": [], "into": "acme"}), &nadia)
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
    let merged = merge("client", body, &nadia).await;
    assert_eq!(merged.status, StatusCode::OK, "{}", merged.text);
    let values: Vec<&str> = merged
        .json
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value["value"].as_str().unwrap())
        .collect();
    assert!(values.contains(&"acme"));
    assert!(!values.contains(&"Acme Corp"));
    let entry_client: String =
        sqlx::query_scalar("SELECT value FROM split_tags WHERE split_id = $1 AND name = 'client'")
            .bind(split)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(entry_client, "acme");

    let me = call(Method::GET, "/api/v1/me/rules".to_owned(), &mara, None).await;
    assert_eq!(me.status, StatusCode::OK, "{}", me.text);
    assert_eq!(me.json["rules"][0]["then"], set_tags("acme"));
    let organization = call(
        Method::GET,
        "/api/v1/organization/rules".to_owned(),
        &nadia,
        None,
    )
    .await;
    assert_eq!(organization.json["rules"][0]["then"], set_tags("acme"));
    let members = call(Method::GET, "/api/v1/members".to_owned(), &nadia, None).await;
    let member = |id: Uuid| {
        members
            .json
            .as_array()
            .unwrap()
            .iter()
            .find(|member| member["id"] == id.to_string())
            .unwrap()
            .clone()
    };
    assert_eq!(member(ids::MARA)["tags"]["client"], "acme");
    // Kai's two filters became one.
    assert_eq!(member(ids::KAI)["filters"], json!([{"client": "acme"}]));

    // Removing the tag name takes it out of the rules too.
    let removed = call(
        Method::DELETE,
        "/api/v1/tags/client".to_owned(),
        &nadia,
        None,
    )
    .await;
    assert_eq!(removed.status, StatusCode::NO_CONTENT, "{}", removed.text);
    let me = call(Method::GET, "/api/v1/me/rules".to_owned(), &mara, None).await;
    assert_eq!(me.json["rules"][0]["then"], json!({"set_tags": {}}));
}

/// Hooks a build could add: a one-member limit, and one more MCP tool.
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

    fn mcp_tools<'a>(
        &'a self,
        _state: &'a AppState,
        _caller: &'a tokencontroller_server::sign_in::Caller,
    ) -> tokencontroller_server::hooks::HookFuture<'a, Vec<Value>> {
        Box::pin(async {
            Ok(vec![json!({
                "name": "plan",
                "description": "The organization's plan.",
                "inputSchema": {"type": "object", "properties": {}}
            })])
        })
    }

    fn call_mcp_tool<'a>(
        &'a self,
        _state: &'a AppState,
        caller: &'a tokencontroller_server::sign_in::Caller,
        name: &'a str,
        _arguments: &'a Value,
    ) -> tokencontroller_server::hooks::HookFuture<'a, Option<Value>> {
        Box::pin(async move {
            Ok((name == "plan")
                .then(|| json!({"organization": caller.organization_id, "plan": "one"})))
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
    let (organization_id, controller) =
        organization(&pool, "Kestrel Digital", "n.berger@kestrel.example")
            .await
            .unwrap();
    let invite = json!({"email": "m.ortner@kestrel.example", "name": "Mara Ortner"});

    let refused = send(
        &hooked_app(&pool),
        Method::POST,
        "/api/v1/members",
        Auth::Cookie(&controller),
        Some(invite.clone()),
    )
    .await
    .unwrap();
    assert_eq!(refused.status, StatusCode::CONFLICT);
    assert_eq!(refused.json["message"], "This plan is for one person.");

    // Without hooks the instance's own limit holds.
    let state = AppState::new(pool.clone(), config(), Arc::new(NoExtension));
    let limit = tokencontroller_server::organizations::member_limit(&state, organization_id)
        .await
        .unwrap();
    assert_eq!(
        limit.max,
        i64::from(tokencontroller_server::organizations::DEFAULT_MEMBER_LIMIT)
    );
    let invited = send(
        &app(&pool),
        Method::POST,
        "/api/v1/members",
        Auth::Cookie(&controller),
        Some(invite),
    )
    .await
    .unwrap();
    assert_eq!(invited.status, StatusCode::CREATED, "{}", invited.text);
}

/// An Enterprise plan license for 250 members.
struct Licensed;

impl tokencontroller_core::Extension for Licensed {
    fn member_limit(&self) -> Option<u32> {
        Some(250)
    }
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
    let (_, controller) = organization(&pool, "Kestrel Digital", "n.berger@kestrel.example")
        .await
        .unwrap();

    // The open build: the instance's own limit, and what an invite past it
    // is refused with.
    let open = member_limit_of(&app(&pool), Auth::Cookie(&controller))
        .await
        .unwrap();
    assert_eq!(open.status, StatusCode::OK, "{}", open.text);
    assert_eq!(
        open.json,
        json!({"members": 1, "member_limit": 150, "message": "This instance allows no more members."})
    );

    // Invited people count before they join. An agent manager reads it, and
    // so does a read token.
    let invited = send(
        &app(&pool),
        Method::POST,
        "/api/v1/members",
        Auth::Cookie(&controller),
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
        Auth::Cookie(&controller),
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

    // An Enterprise plan license sets the limit, and a build's hook comes
    // before it.
    let licensed = router(AppState::new(pool.clone(), config(), Arc::new(Licensed)));
    let licensed = member_limit_of(&licensed, Auth::Cookie(&controller))
        .await
        .unwrap();
    assert_eq!(licensed.json["member_limit"], 250);
    let hooked = member_limit_of(&hooked_app(&pool), Auth::Cookie(&controller))
        .await
        .unwrap();
    assert_eq!(
        hooked.json,
        json!({"members": 2, "member_limit": 1, "message": "This plan is for one person."})
    );
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_hook_adds_mcp_tools(pool: PgPool) {
    let (organization_id, controller) =
        organization(&pool, "Kestrel Digital", "n.berger@kestrel.example")
            .await
            .unwrap();
    let rpc = |method: &str, params: Value| json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
    let mcp = |app: Router, body: Value| {
        let controller = controller.clone();
        async move {
            send(
                &app,
                Method::POST,
                "/api/v1/mcp",
                Auth::Cookie(&controller),
                Some(body),
            )
            .await
            .unwrap()
        }
    };

    let open = mcp(app(&pool), rpc("tools/list", json!({}))).await;
    let hooked = mcp(hooked_app(&pool), rpc("tools/list", json!({}))).await;
    let open_tools = open.json["result"]["tools"].as_array().unwrap().len();
    let hooked_tools = hooked.json["result"]["tools"].as_array().unwrap();
    assert_eq!(hooked_tools.len(), open_tools.checked_add(1).unwrap());
    assert_eq!(hooked_tools.last().unwrap()["name"], "plan");

    let called = mcp(
        hooked_app(&pool),
        rpc("tools/call", json!({"name": "plan", "arguments": {}})),
    )
    .await;
    assert_eq!(
        called.json["result"]["structuredContent"]["result"]["organization"],
        organization_id.to_string()
    );
    let unknown = mcp(app(&pool), rpc("tools/call", json!({"name": "plan"}))).await;
    assert_eq!(unknown.json["error"]["code"], -32602);
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

    let (organization_id, controller) =
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
        Auth::Cookie(&controller),
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
