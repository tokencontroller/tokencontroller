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
use tokencontroller_server::config::StripeConfig;
use tokencontroller_server::sign_in::secret;
use tokencontroller_server::{AppState, Config, router};
use tower::ServiceExt as _;
use uuid::Uuid;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

const KEY: &str = "0123456789abcdef0123456789abcdef";
const WEBHOOK_SECRET: &str = "whsec_test";

fn config(stripe: bool) -> Config {
    Config {
        database_url: "postgres://unused".to_owned(),
        public_url: "http://localhost:8080".to_owned(),
        secret_key: KEY.to_owned(),
        smtp_url: None,
        smtp_from: None,
        prices_refresh: false,
        customer_relations: tokencontroller_server::customer_relations::Settings::default(),
        stripe: stripe.then(|| StripeConfig {
            secret_key: "sk_test_unused".to_owned(),
            webhook_secret: WEBHOOK_SECRET.to_owned(),
        }),
    }
}

fn app(pool: &PgPool) -> Router {
    router(AppState::new(
        pool.clone(),
        config(false),
        Arc::new(NoExtension),
    ))
}

fn billing_app(pool: &PgPool) -> Router {
    router(AppState::new(
        pool.clone(),
        config(true),
        Arc::new(NoExtension),
    ))
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
    let session = secret::generate(secret::SESSION, KEY)?;
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
        Some(json!({"email": "N.Berger@Kestrel.Example", "organization_name": "Kestrel Digital"})),
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
    organization(&pool, "Kestrel Digital", "n.berger@kestrel.example")
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
    assert!(session.starts_with("tc_ses_"));

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
async fn the_status_endpoint_shows_job_heartbeats(pool: PgPool) {
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
    assert_eq!(fresh.json["components"][1]["state"], "not_running");

    let hour = std::time::Duration::from_hours(1);
    tokencontroller_server::status::record_job_run(&pool, "bill-pulls", hour, Ok(()))
        .await
        .unwrap();
    tokencontroller_server::status::record_job_run(
        &pool,
        "ticket-sync",
        hour,
        Err("Jira answered 401"),
    )
    .await
    .unwrap();
    let after = send(&app, Method::GET, "/api/v1/status", Auth::None, None)
        .await
        .unwrap();
    assert_eq!(after.json["components"][1]["state"], "operational");
    assert!(after.json["components"][1]["last_success_at"].is_string());
    assert_eq!(after.json["components"][2]["state"], "degraded");
    assert_eq!(after.json["status"], "degraded");
    // The public answer carries no error text.
    assert!(!after.text.contains("Jira answered"));
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
    let app = app(&pool);
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
