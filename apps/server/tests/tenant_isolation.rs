//! Tenant isolation against the real service and a real Postgres database.
//!
//! Two organizations, Alpha Works and Bravo Holdings, each get data in every
//! kind of record the API has: members, tag and attribute values, rules,
//! grants, messages, splits, estimates, shares, submissions, a cutoff,
//! bills, price table rows, saved reports, API tokens, bill connections and
//! ticket systems. Then every route in the service's OpenAPI document is
//! called as Alpha's members with Bravo's ids, names and keys. Each call
//! must be refused, or answer with nothing of Bravo's, and Bravo's data must
//! be exactly as it was afterwards.
//!
//! A route without a case here fails `every_route_is_called_across_tenants`,
//! so a new endpoint needs its isolation case.

use std::collections::BTreeSet;
use std::io::Read as _;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use sqlx::PgPool;
use tokencontroller_core::NoExtension;
use tokencontroller_server::sign_in::secret;
use tokencontroller_server::{AppState, Config, router};
use tower::ServiceExt as _;
use uuid::Uuid;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

/// The server's secret key for this test run: 32 hex characters, new each run.
static KEY: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| Uuid::new_v4().simple().to_string());

fn config() -> Config {
    Config {
        database_url: "postgres://unused".to_owned(),
        public_url: "http://localhost:8080".to_owned(),
        secret_key: KEY.clone(),
        smtp_url: None,
        smtp_from: None,
        price_table: None,
        rates_refresh: false,
        classifier: None,
    }
}

// ---------------------------------------------------------------------------
// Calling the service

enum Payload {
    Empty,
    Json(Value),
    Raw(&'static str, Vec<u8>),
}

impl Payload {
    fn text(&self) -> String {
        match self {
            Self::Empty => String::new(),
            Self::Json(value) => value.to_string(),
            Self::Raw(_, bytes) => String::from_utf8_lossy(bytes).into_owned(),
        }
    }
}

struct Answer {
    status: StatusCode,
    content_type: String,
    body: Vec<u8>,
}

impl Answer {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// Everything the answer says, with a ZIP file's files unpacked.
    fn readable(&self) -> TestResult<String> {
        if !self.content_type.starts_with("application/zip") {
            return Ok(self.text());
        }
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(self.body.clone()))?;
        let mut all = String::new();
        for index in 0..zip.len() {
            let mut file = zip.by_index(index)?;
            all.push_str(&file.name()?);
            all.push('\n');
            file.read_to_string(&mut all)?;
            all.push('\n');
        }
        Ok(all)
    }
}

async fn call(
    app: &Router,
    method: Method,
    path: &str,
    token: &str,
    payload: &Payload,
) -> TestResult<Answer> {
    let mut request = Request::builder().method(method).uri(path);
    if token.starts_with(secret::SESSION) {
        request = request.header(header::COOKIE, format!("tc_session={token}"));
    } else if !token.is_empty() {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let request = match payload {
        Payload::Empty => request.body(Body::empty())?,
        Payload::Json(value) => request
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(value)?))?,
        Payload::Raw(content_type, bytes) => request
            .header(header::CONTENT_TYPE, *content_type)
            .body(Body::from(bytes.clone()))?,
    };
    let response = app.clone().oneshot(request).await?;
    let status = response.status();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let body = response.into_body().collect().await?.to_bytes().to_vec();
    Ok(Answer {
        status,
        content_type,
        body,
    })
}

/// A call made while setting up; anything but success stops the test.
async fn must(
    app: &Router,
    method: Method,
    path: &str,
    token: &str,
    payload: Payload,
) -> TestResult<Value> {
    let answer = call(app, method.clone(), path, token, &payload).await?;
    if answer.status.is_success() {
        Ok(answer.json())
    } else {
        Err(format!("{method} {path}: {} {}", answer.status, answer.text()).into())
    }
}

fn id_at(value: &Value, pointer: &str) -> TestResult<Uuid> {
    let text = value
        .pointer(pointer)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("no {pointer} in {value}"))?;
    Ok(Uuid::parse_str(text)?)
}

fn text_at(value: &Value, pointer: &str) -> TestResult<String> {
    Ok(value
        .pointer(pointer)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("no {pointer} in {value}"))?
        .to_owned())
}

async fn session_for(pool: &PgPool, person_id: Uuid, prefix: &str) -> TestResult<String> {
    let client = if prefix == secret::CLI {
        "local_tool"
    } else {
        "browser"
    };
    let session = secret::generate(prefix, &KEY)?;
    sqlx::query("INSERT INTO sessions (token_hash, person_id, client, expires_at) VALUES ($1, $2, $3, now() + interval '1 day')")
        .bind(&session.hash)
        .bind(person_id)
        .bind(client)
        .execute(pool)
        .await?;
    Ok(session.token)
}

async fn organization(pool: &PgPool, name: &str, email: &str) -> TestResult<(Uuid, Uuid)> {
    let mut conn = pool.acquire().await?;
    let created = tokencontroller_server::organizations::create(&mut conn, name, email)
        .await
        .map_err(|error| error.to_string())?;
    Ok((created.organization_id, created.person_id))
}

/// A Claude Code session record as `tc` sends it.
fn session_entry(
    local_id: &str,
    session_id: &str,
    started_at: &str,
    feature: &str,
    summary: &str,
) -> Value {
    json!({
        "agent_harness": "claude-code",
        "model": "claude-opus-5",
        "provider": "anthropic",
        "started_at": started_at,
        "ended_at": started_at.replace("00:00Z", "30:00Z"),
        "tokens": {"input": 12000, "output": 3000, "cache_read": 0, "cache_write": 0},
        "reported_cost": "0.42",
        "summary": summary,
        "feature": feature,
        "session_id": session_id,
        "local_id": local_id
    })
}

async fn tag_value(
    app: &Router,
    session: &str,
    name: &str,
    key: &str,
    defaults: Value,
) -> TestResult<Value> {
    must(
        app,
        Method::POST,
        &format!("/api/v1/tags/{name}/values"),
        session,
        Payload::Json(json!({"key": key, "defaults": defaults})),
    )
    .await
}

// ---------------------------------------------------------------------------
// Alpha: the organization whose members call

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cred {
    /// Alice's browser session; she is Alpha's org manager.
    Session,
    /// Alice's read token.
    Read,
    /// Alice's service token.
    Service,
    /// Alpha's organization token.
    Organization,
    /// Alice's `tc connect` token.
    Cli,
    /// A browser session kept for signing out.
    SignOut,
    /// No credential at all.
    Nobody,
}

struct Alpha {
    organization_id: Uuid,
    person_id: Uuid,
    service_token_id: Uuid,
    estimate: String,
    session: String,
    read: String,
    service: String,
    organization: String,
    cli: String,
    sign_out: String,
}

impl Alpha {
    fn token(&self, cred: Cred) -> &str {
        match cred {
            Cred::Session => &self.session,
            Cred::Read => &self.read,
            Cred::Service => &self.service,
            Cred::Organization => &self.organization,
            Cred::Cli => &self.cli,
            Cred::SignOut => &self.sign_out,
            Cred::Nobody => "",
        }
    }
}

async fn alpha(app: &Router, pool: &PgPool) -> TestResult<Alpha> {
    let (organization_id, person_id) =
        organization(pool, "Alpha Works", "alice@alpha.example").await?;
    let session = session_for(pool, person_id, secret::SESSION).await?;
    let token = |kind: &'static str| {
        let session = session.clone();
        async move {
            must(
                app,
                Method::POST,
                "/api/v1/api-tokens",
                &session,
                Payload::Json(json!({"kind": kind, "name": "alpha"})),
            )
            .await
        }
    };
    let read = token("read").await?;
    let service = token("service").await?;
    let organization = token("organization").await?;
    must(
        app,
        Method::PATCH,
        "/api/v1/organization",
        &session,
        Payload::Json(json!({"approval": true})),
    )
    .await?;
    tag_value(app, &session, "project", "alpha-app", json!({})).await?;
    tag_value(
        app,
        &session,
        "feature",
        "ALPHA-1",
        json!({"project": "alpha-app"}),
    )
    .await?;
    let estimate = must(
        app,
        Method::POST,
        "/api/v1/estimates",
        &session,
        Payload::Json(json!({
            "work": "Alpha onboarding",
            "tags": {"project": "alpha-app"},
            "median": "100",
            "from": "2026-10-01",
            "to": "2026-10-31"
        })),
    )
    .await?;
    let service_token = text_at(&service, "/token")?;
    must(
        app,
        Method::POST,
        "/api/v1/token-entries",
        &service_token,
        Payload::Json(session_entry(
            "alpha-local-1",
            "alpha-session-1",
            "2026-10-06T09:00:00Z",
            "ALPHA-1",
            "Alpha onboarding work",
        )),
    )
    .await?;
    Ok(Alpha {
        organization_id,
        person_id,
        service_token_id: id_at(&service, "/id")?,
        estimate: text_at(&estimate, "/id")?,
        session: session.clone(),
        read: text_at(&read, "/token")?,
        service: service_token,
        organization: text_at(&organization, "/token")?,
        cli: session_for(pool, person_id, secret::CLI).await?,
        sign_out: session_for(pool, person_id, secret::SESSION).await?,
    })
}

// ---------------------------------------------------------------------------
// Bravo: the organization whose data must stay its own

/// What Alpha's members must never see, and the ids they try.
#[derive(Default)]
struct Bravo {
    organization_id: Uuid,
    boss: String,
    bruno_browser: String,
    bruno: Uuid,
    petra: Uuid,
    invite_code: String,
    estimate: String,
    tag_value: String,
    submission: String,
    bill: String,
    read_token: Uuid,
    service_token: Uuid,
    cli_session: Uuid,
    saved_report: String,
    bill_connection: Uuid,
    ticket_system: Uuid,
    /// Every name, key, text and id of Bravo's.
    secrets: Vec<String>,
}

async fn bravo(app: &Router, pool: &PgPool) -> TestResult<Bravo> {
    let mut b = Bravo::default();
    let (organization_id, boss_id) =
        organization(pool, "Bravo Holdings", "boss@bravo.example").await?;
    b.organization_id = organization_id;
    b.boss = session_for(pool, boss_id, secret::SESSION).await?;
    let boss = b.boss.clone();
    let boss = boss.as_str();

    // People: Bruno joined, Petra is invited.
    let bruno = must(
        app,
        Method::POST,
        "/api/v1/members",
        boss,
        Payload::Json(json!({"email": "bruno@bravo.example", "name": "Bruno Bravo"})),
    )
    .await?;
    b.bruno = id_at(&bruno, "/member/id")?;
    sqlx::query("UPDATE people SET joined_at = now() WHERE id = $1")
        .bind(b.bruno)
        .execute(pool)
        .await?;
    b.bruno_browser = session_for(pool, b.bruno, secret::SESSION).await?;
    let cli = session_for(pool, b.bruno, secret::CLI).await?;
    let petra = must(
        app,
        Method::POST,
        "/api/v1/members",
        boss,
        Payload::Json(json!({"email": "petra@bravo.example", "name": "Petra Bravo", "role": "people_manager"})),
    )
    .await?;
    b.petra = id_at(&petra, "/member/id")?;
    text_at(&petra, "/invite_url")?
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .clone_into(&mut b.invite_code);

    bravo_names(app, boss, &mut b).await?;
    bravo_work(app, boss, &mut b).await?;
    bravo_books(app, boss, &mut b).await?;
    drop(cli);
    b.secrets = bravo_ids(pool, organization_id).await?;
    b.secrets.extend(
        [
            "Bravo Holdings",
            "bravo-holdings",
            "bruno@bravo.example",
            "petra@bravo.example",
            "Bruno Bravo",
            "Petra Bravo",
            "bravozone",
            "bravo-zone-1",
            "bravo-client",
            "bravo-ledger",
            "BRAVO-7",
            "bravo-team",
            "Bravo team",
            "Bravo branch rule",
            "Bravo ledger rebuild",
            "777.77",
            "bravo-session-1",
            "bravo-session-2",
            "bravo-session-3",
            "bravo-local-1",
            "bravo-local-2",
            "bravo-local-3",
            "Bravo ledger import",
            "Bravo ledger export",
            "Bravo September work",
            "Bravo closes the year",
            "Bravo quarterly",
            "sk-admin-bravo-secret",
            "bravo.atlassian.net",
            "Bravo ledger board",
            "ATATT3xFfGF0-bravo",
        ]
        .map(str::to_owned),
    );
    b.secrets.push(b.invite_code.clone());
    b.secrets.push(b.estimate.clone());
    b.secrets.push(b.tag_value.clone());
    b.secrets.push(b.submission.clone());
    b.secrets.push(b.bill.clone());
    b.secrets.push(b.saved_report.clone());
    b.secrets.sort();
    b.secrets.dedup();
    Ok(b)
}

/// Bravo's names and values, its rule, Bruno's grant and its price rows.
async fn bravo_names(app: &Router, boss: &str, b: &mut Bravo) -> TestResult<()> {
    // Names and values.
    must(
        app,
        Method::POST,
        "/api/v1/tags",
        boss,
        Payload::Json(json!({"name": "bravozone"})),
    )
    .await?;
    tag_value(app, boss, "client", "bravo-client", json!({})).await?;
    let ledger = tag_value(
        app,
        boss,
        "project",
        "bravo-ledger",
        json!({"client": "bravo-client"}),
    )
    .await?;
    b.tag_value = text_at(&ledger, "/id")?;
    tag_value(
        app,
        boss,
        "feature",
        "BRAVO-7",
        json!({"project": "bravo-ledger"}),
    )
    .await?;
    tag_value(app, boss, "bravozone", "bravo-zone-1", json!({})).await?;
    must(
        app,
        Method::POST,
        "/api/v1/attributes/team/values",
        boss,
        Payload::Json(json!({"key": "bravo-team", "title": "Bravo team"})),
    )
    .await?;
    must(
        app,
        Method::POST,
        &format!("/api/v1/people/{}/attributes", b.bruno),
        boss,
        Payload::Json(json!({"name": "team", "key": "bravo-team", "from": "2026-01-01"})),
    )
    .await?;
    must(
        app,
        Method::PUT,
        &format!("/api/v1/members/{}/filters", b.petra),
        boss,
        Payload::Json(json!([{"team": "bravo-team"}])),
    )
    .await?;
    must(
        app,
        Method::POST,
        "/api/v1/rules",
        boss,
        Payload::Json(json!({"name": "Bravo branch rule", "signal": "branch", "pattern": "(?P<key>BRAVO-\\d+)", "tag_name": "feature"})),
    )
    .await?;
    must(
        app,
        Method::POST,
        "/api/v1/grants",
        boss,
        Payload::Json(json!({"person": "bruno@bravo.example", "capability": "view", "filter": {"project": "bravo-ledger"}})),
    )
    .await?;
    must(
        app,
        Method::POST,
        "/api/v1/price-table/rows",
        boss,
        // Bravo prices the model both organizations use; Alpha's figures
        // must not move with it.
        Payload::Json(json!({"provider": "anthropic", "model": "claude-opus-5", "kind": "input", "price_per_million": "7", "valid_from": "2026-01-01"})),
    )
    .await?;
    for kind in ["output", "cache_read", "cache_write"] {
        must(
            app,
            Method::POST,
            "/api/v1/price-table/rows",
            boss,
            Payload::Json(json!({"provider": "anthropic", "model": "claude-opus-5", "kind": kind, "price_per_million": "7", "valid_from": "2026-01-01"})),
        )
        .await?;
    }

    Ok(())
}

/// Bravo's plan, tokens, messages and split.
async fn bravo_work(app: &Router, boss: &str, b: &mut Bravo) -> TestResult<()> {
    // Plans.
    let estimate = must(
        app,
        Method::POST,
        "/api/v1/estimates",
        boss,
        Payload::Json(json!({
            "work": "Bravo ledger rebuild",
            "tags": {"project": "bravo-ledger"},
            "median": "777.77",
            "from": "2026-10-01",
            "to": "2026-12-31"
        })),
    )
    .await?;
    b.estimate = text_at(&estimate, "/id")?;
    must(
        app,
        Method::POST,
        &format!("/api/v1/estimates/{}/accept", b.estimate),
        boss,
        Payload::Empty,
    )
    .await?;
    must(
        app,
        Method::POST,
        "/api/v1/shares",
        boss,
        Payload::Json(json!({"person": "bruno@bravo.example", "project": "bravo-ledger", "fraction": "0.5", "from": "2026-10-01"})),
    )
    .await?;

    // Tokens and messages.
    let token = |kind: &'static str| async move {
        must(
            app,
            Method::POST,
            "/api/v1/api-tokens",
            boss,
            Payload::Json(json!({"kind": kind, "name": "bravo bot"})),
        )
        .await
    };
    let read = token("read").await?;
    let service = token("service").await?;
    b.read_token = id_at(&read, "/id")?;
    b.service_token = id_at(&service, "/id")?;
    let service_token = text_at(&service, "/token")?;
    for (local, session, day, summary) in [
        (
            "bravo-local-1",
            "bravo-session-1",
            "2026-10-05T09:00:00Z",
            "Bravo ledger import",
        ),
        (
            "bravo-local-2",
            "bravo-session-2",
            "2026-10-06T09:00:00Z",
            "Bravo ledger export",
        ),
        (
            "bravo-local-3",
            "bravo-session-3",
            "2026-09-20T09:00:00Z",
            "Bravo September work",
        ),
    ] {
        must(
            app,
            Method::POST,
            "/api/v1/token-entries",
            &service_token,
            Payload::Json(session_entry(local, session, day, "BRAVO-7", summary)),
        )
        .await?;
    }
    must(
        app,
        Method::POST,
        "/api/v1/splits",
        boss,
        Payload::Json(json!({"session_id": "bravo-session-1", "parts": [
            {"fraction": "0.5", "tags": {"feature": "BRAVO-7"}},
            {"fraction": "0.5", "tags": {"project": "bravo-ledger"}}
        ]})),
    )
    .await?;
    let tokens = must(app, Method::GET, "/api/v1/api-tokens", boss, Payload::Empty).await?;
    b.cli_session = tokens
        .as_array()
        .into_iter()
        .flatten()
        .find(|token| token["kind"] == "cli")
        .and_then(|token| token["id"].as_str())
        .and_then(|id| Uuid::parse_str(id).ok())
        .unwrap_or_default();

    Ok(())
}

/// Bravo's submissions, cutoff, bill, saved report and integrations.
async fn bravo_books(app: &Router, boss: &str, b: &mut Bravo) -> TestResult<()> {
    // Submissions and the cutoff.
    let submission = must(
        app,
        Method::POST,
        "/api/v1/submissions",
        &b.bruno_browser,
        Payload::Json(json!({"to": "2026-09-30"})),
    )
    .await?;
    b.submission = text_at(&submission, "/id")?;
    must(
        app,
        Method::POST,
        "/api/v1/submit-requests",
        boss,
        Payload::Json(json!({"until": "2099-12-31", "message": "Bravo closes the year"})),
    )
    .await?;
    must(
        app,
        Method::POST,
        "/api/v1/cutoff",
        boss,
        Payload::Json(json!({"cutoff": "2026-08-31"})),
    )
    .await?;

    // Bills, reports and integrations.
    let bill = must(
        app,
        Method::POST,
        "/api/v1/bills",
        boss,
        Payload::Json(json!({"provider": "anthropic", "from": "2026-10-01", "to": "2026-10-31", "amount": "777.77", "currency": "USD", "typed_in": true, "lines": []})),
    )
    .await?;
    b.bill = text_at(&bill, "/bill/id").or_else(|_| text_at(&bill, "/id"))?;
    let saved = must(
        app,
        Method::POST,
        "/api/v1/reports/saved",
        boss,
        Payload::Json(json!({"name": "Bravo quarterly", "filter": {"project": "bravo-ledger"}, "by": "feature"})),
    )
    .await?;
    b.saved_report = text_at(&saved, "/id")?;
    let connection = must(
        app,
        Method::POST,
        "/api/v1/bill-connections",
        boss,
        Payload::Json(json!({"provider": "openai", "key": "sk-admin-bravo-secret"})),
    )
    .await?;
    b.bill_connection = id_at(&connection, "/id")?;
    let ticket_system = must(
        app,
        Method::POST,
        "/api/v1/ticket-systems",
        boss,
        Payload::Json(json!({"kind": "jira", "site": "https://bravo.atlassian.net", "token": "ATATT3xFfGF0-bravo", "email": "bruno@bravo.example"})),
    )
    .await?;
    b.ticket_system = id_at(&ticket_system, "/id")?;
    must(
        app,
        Method::PUT,
        &format!("/api/v1/ticket-systems/{}/mappings", b.ticket_system),
        boss,
        Payload::Json(json!([{"external": "BRV", "external_name": "Bravo ledger board", "project": "bravo-ledger"}])),
    )
    .await?;

    Ok(())
}

/// Every uuid and event id of Bravo's in the database.
async fn bravo_ids(pool: &PgPool, organization_id: Uuid) -> TestResult<Vec<String>> {
    let mut ids: Vec<String> = Vec::new();
    ids.push(organization_id.to_string());
    let people: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM people WHERE organization_id = $1")
        .bind(organization_id)
        .fetch_all(pool)
        .await?;
    ids.extend(people.iter().map(ToString::to_string));
    let events: Vec<String> =
        sqlx::query_scalar("SELECT id FROM events WHERE organization_id = $1")
            .bind(organization_id)
            .fetch_all(pool)
            .await?;
    // Role grants carry the person's uuid; their ids are covered by it.
    ids.extend(events.into_iter().filter(|id| !id.starts_with("grt_role_")));
    for table in [
        "api_tokens",
        "bill_connections",
        "ticket_systems",
        "saved_reports",
        "submit_requests",
    ] {
        let rows: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT id::text FROM {table} WHERE organization_id = $1"
        )))
        .bind(organization_id)
        .fetch_all(pool)
        .await?;
        ids.extend(rows);
    }
    Ok(ids)
}

// ---------------------------------------------------------------------------
// The cases: every route, called by Alpha with Bravo's ids

/// One call Alpha makes.
struct Ask {
    method: Method,
    path: String,
    cred: Cred,
    payload: Payload,
    /// The statuses that count as refused or answered; empty takes any
    /// status below 500.
    expect: &'static [u16],
    /// JSON pointers into the answer and the values they must have.
    checks: Vec<(&'static str, Value)>,
    /// Bravo's names this answer may hold because Alpha stored them as its
    /// own: each counts only while Alpha's rows hold it.
    allow: Vec<&'static str>,
}

impl Ask {
    fn json(mut self, value: Value) -> Self {
        self.payload = Payload::Json(value);
        self
    }

    fn raw(mut self, content_type: &'static str, text: &str) -> Self {
        self.payload = Payload::Raw(content_type, text.as_bytes().to_vec());
        self
    }

    const fn by(mut self, cred: Cred) -> Self {
        self.cred = cred;
        self
    }

    const fn expect(mut self, statuses: &'static [u16]) -> Self {
        self.expect = statuses;
        self
    }

    fn check(mut self, pointer: &'static str, value: Value) -> Self {
        self.checks.push((pointer, value));
        self
    }

    fn allow(mut self, names: &[&'static str]) -> Self {
        self.allow.extend_from_slice(names);
        self
    }
}

fn ask(method: Method, path: impl Into<String>) -> Ask {
    Ask {
        method,
        path: path.into(),
        cred: Cred::Session,
        payload: Payload::Empty,
        expect: &[],
        checks: Vec::new(),
        allow: Vec::new(),
    }
}

fn get(path: impl Into<String>) -> Ask {
    ask(Method::GET, path)
}

fn post(path: impl Into<String>) -> Ask {
    ask(Method::POST, path)
}

fn put(path: impl Into<String>) -> Ask {
    ask(Method::PUT, path)
}

fn patch(path: impl Into<String>) -> Ask {
    ask(Method::PATCH, path)
}

fn delete(path: impl Into<String>) -> Ask {
    ask(Method::DELETE, path)
}

/// The calls for one route of the OpenAPI document.
struct Case {
    method: &'static str,
    template: &'static str,
    asks: Vec<Ask>,
}

fn case(method: &'static str, template: &'static str, asks: Vec<Ask>) -> Case {
    Case {
        method,
        template,
        asks,
    }
}

/// Bravo's names Alpha stored as its own with its writes: the session ids
/// and feature key of the messages it sent, the member Bravo's telemetry
/// named, and the project its saved report filters on. Only Alpha's own
/// records hold them.
const ALPHA_STORED: [&str; 5] = [
    "bravo-session-1",
    "bravo-session-2",
    "BRAVO-7",
    "bruno@bravo.example",
    "bravo-ledger",
];

const OK: &[u16] = &[200];
const CREATED: &[u16] = &[201];
const BAD_REQUEST: &[u16] = &[400];
const NOT_FOUND: &[u16] = &[404];
const CONFLICT: &[u16] = &[409];
const ACCEPTED: &[u16] = &[202];
const REFUSED: &[u16] = &[400, 404];

fn token_cases(alpha: &Alpha, b: &Bravo) -> Vec<Case> {
    let tokens = "/api/v1/api-tokens";
    vec![
        case("GET", "/api/v1/api-tokens", vec![get(tokens).expect(OK)]),
        case(
            "POST",
            "/api/v1/api-tokens",
            vec![
                post(tokens)
                    .json(json!({"kind": "read", "name": "alpha extra"}))
                    .expect(CREATED),
            ],
        ),
        case(
            "DELETE",
            "/api/v1/api-tokens/{id}",
            vec![
                delete(format!("{tokens}/{}", b.read_token)).expect(NOT_FOUND),
                delete(format!("{tokens}/{}", b.service_token)).expect(NOT_FOUND),
                delete(format!("{tokens}/{}", b.cli_session)).expect(NOT_FOUND),
            ],
        ),
        case(
            "POST",
            "/api/v1/api-tokens/{id}/hand-over",
            vec![
                post(format!("{tokens}/{}/hand-over", b.service_token))
                    .json(json!({"person_id": alpha.person_id}))
                    .expect(NOT_FOUND),
                post(format!("{tokens}/{}/hand-over", alpha.service_token_id))
                    .json(json!({"person_id": b.bruno}))
                    .expect(REFUSED),
            ],
        ),
    ]
}

fn member_cases(b: &Bravo) -> Vec<Case> {
    vec![
        case(
            "GET",
            "/api/v1/members",
            vec![get("/api/v1/members").expect(OK)],
        ),
        case(
            "POST",
            "/api/v1/members",
            vec![
                post("/api/v1/members")
                    .json(json!({"email": "bruno@bravo.example"}))
                    .expect(CONFLICT),
                post("/api/v1/members")
                    .json(json!({"email": "second@alpha.example", "attributes": {"team": "bravo-team"}}))
                    .expect(BAD_REQUEST),
            ],
        ),
        case(
            "PATCH",
            "/api/v1/members/{id}",
            vec![
                patch(format!("/api/v1/members/{}", b.bruno))
                    .json(json!({"name": "Taken over", "role": "org_manager"}))
                    .expect(NOT_FOUND),
            ],
        ),
        case(
            "DELETE",
            "/api/v1/members/{id}",
            vec![delete(format!("/api/v1/members/{}", b.bruno)).expect(NOT_FOUND)],
        ),
        case(
            "PUT",
            "/api/v1/members/{id}/filters",
            vec![
                put(format!("/api/v1/members/{}/filters", b.petra))
                    .json(json!([{"project": "alpha-app"}]))
                    .expect(NOT_FOUND),
            ],
        ),
        case(
            "POST",
            "/api/v1/members/{id}/invite",
            vec![post(format!("/api/v1/members/{}/invite", b.petra)).expect(NOT_FOUND)],
        ),
        case(
            "GET",
            "/api/v1/invites/{code}",
            vec![
                get(format!("/api/v1/invites/{}x", b.invite_code))
                    .by(Cred::Nobody)
                    .expect(NOT_FOUND),
            ],
        ),
        case(
            "POST",
            "/api/v1/people/{id}/attributes",
            vec![
                post(format!("/api/v1/people/{}/attributes", b.bruno))
                    .json(json!({"name": "team", "key": "bravo-team"}))
                    .expect(REFUSED),
            ],
        ),
    ]
}

fn me_cases() -> Vec<Case> {
    vec![
        case(
            "GET",
            "/api/v1/me",
            vec![
                get("/api/v1/me")
                    .expect(OK)
                    .check("/organization/name", json!("Alpha Works")),
            ],
        ),
        case(
            "GET",
            "/api/v1/me/context",
            vec![
                get("/api/v1/me/context").expect(OK).allow(&ALPHA_STORED),
                get("/api/v1/me/context")
                    .by(Cred::Cli)
                    .expect(OK)
                    .allow(&ALPHA_STORED),
            ],
        ),
        case(
            "GET",
            "/api/v1/me/rules",
            vec![get("/api/v1/me/rules").expect(OK)],
        ),
        case(
            "PUT",
            "/api/v1/me/rules",
            vec![
                put("/api/v1/me/rules")
                    .json(json!([{"name": "mine", "signal": "branch", "pattern": "(?P<key>ALPHA-\\d+)", "tag_name": "feature"}]))
                    .expect(OK),
                put("/api/v1/me/rules")
                    .json(json!([{"name": "theirs", "signal": "branch", "pattern": "x", "tag_name": "bravozone"}]))
                    .expect(BAD_REQUEST),
            ],
        ),
        case(
            "GET",
            "/api/v1/grants",
            vec![
                get("/api/v1/grants").expect(OK),
                get("/api/v1/grants?person=bruno@bravo.example")
                    .expect(OK)
                    .check("", json!([])),
            ],
        ),
        case(
            "POST",
            "/api/v1/grants",
            vec![
                post("/api/v1/grants")
                    .json(json!({"person": "bruno@bravo.example", "capability": "view", "filter": {}}))
                    .expect(REFUSED),
                post("/api/v1/grants")
                    .json(json!({"person": "alice@alpha.example", "capability": "view", "filter": {"project": "bravo-ledger"}}))
                    .expect(REFUSED),
            ],
        ),
    ]
}

fn organization_cases() -> Vec<Case> {
    vec![
        case(
            "GET",
            "/api/v1/organization",
            vec![
                get("/api/v1/organization")
                    .expect(OK)
                    .check("/name", json!("Alpha Works")),
            ],
        ),
        case(
            "PATCH",
            "/api/v1/organization",
            vec![
                patch("/api/v1/organization")
                    .json(json!({"name": "Alpha Works", "reporting_currency": "USD"}))
                    .expect(OK),
            ],
        ),
        case(
            "GET",
            "/api/v1/organization/member-limit",
            vec![get("/api/v1/organization/member-limit").expect(OK)],
        ),
        case(
            "GET",
            "/api/v1/rules",
            vec![get("/api/v1/rules").expect(OK)],
        ),
        case(
            "POST",
            "/api/v1/rules",
            vec![
                post("/api/v1/rules")
                    .json(json!({"name": "Alpha rule", "signal": "branch", "pattern": "(?P<key>ALPHA-\\d+)", "tag_name": "feature"}))
                    .expect(CREATED),
                post("/api/v1/rules")
                    .json(json!({"name": "Bravo copy", "signal": "branch", "pattern": "x", "tag_name": "feature", "key": "BRAVO-7"}))
                    .expect(BAD_REQUEST),
            ],
        ),
        case(
            "GET",
            "/api/v1/cutoff",
            vec![get("/api/v1/cutoff").expect(OK)],
        ),
        case(
            "GET",
            "/api/v1/price-table",
            vec![
                get("/api/v1/price-table").expect(OK),
                get("/api/v1/price-table?model=bravo-model")
                    .expect(OK)
                    .check("/rows", json!([])),
            ],
        ),
        case(
            "POST",
            "/api/v1/price-table/rows",
            vec![
                post("/api/v1/price-table/rows")
                    .json(json!({"provider": "anthropic", "model": "alpha-model", "kind": "input", "price_per_million": "3", "valid_from": "2026-01-01"}))
                    .expect(CREATED),
            ],
        ),
    ]
}

fn tag_cases(b: &Bravo) -> Vec<Case> {
    vec![
        case("GET", "/api/v1/tags", vec![get("/api/v1/tags").expect(OK)]),
        case(
            "POST",
            "/api/v1/tags",
            vec![
                post("/api/v1/tags")
                    .json(json!({"name": "alphazone"}))
                    .expect(CREATED),
            ],
        ),
        case(
            "DELETE",
            "/api/v1/tags/{name}",
            vec![delete("/api/v1/tags/bravozone").expect(NOT_FOUND)],
        ),
        case(
            "POST",
            "/api/v1/tags/{name}/values",
            vec![
                post("/api/v1/tags/bravozone/values")
                    .json(json!({"key": "alpha-zone"}))
                    .expect(NOT_FOUND),
                post("/api/v1/tags/feature/values")
                    .json(json!({"key": "ALPHA-2", "defaults": {"project": "bravo-ledger"}}))
                    .expect(BAD_REQUEST),
            ],
        ),
        case(
            "GET",
            "/api/v1/attributes",
            vec![get("/api/v1/attributes").expect(OK)],
        ),
        case(
            "POST",
            "/api/v1/attributes",
            vec![
                post("/api/v1/attributes")
                    .json(json!({"name": "alphafloor"}))
                    .expect(CREATED),
            ],
        ),
        case(
            "POST",
            "/api/v1/attributes/{name}/values",
            vec![
                post("/api/v1/attributes/team/values")
                    .json(json!({"key": "alpha-team", "defaults": {"department": "bravo-dept"}}))
                    .expect(BAD_REQUEST),
            ],
        ),
        case(
            "POST",
            "/api/v1/reversals",
            vec![
                post("/api/v1/reversals")
                    .json(json!({"event_id": b.tag_value}))
                    .expect(NOT_FOUND),
                post("/api/v1/reversals")
                    .json(json!({"event_id": b.estimate}))
                    .expect(NOT_FOUND),
            ],
        ),
        case(
            "GET",
            "/api/v1/events",
            vec![
                get("/api/v1/events").expect(OK).allow(&ALPHA_STORED),
                get("/api/v1/events?kind=estimate").expect(OK),
            ],
        ),
        case(
            "GET",
            "/api/v1/exports/raw",
            vec![get("/api/v1/exports/raw").expect(OK).allow(&ALPHA_STORED)],
        ),
    ]
}

fn plan_cases(alpha: &Alpha, b: &Bravo) -> Vec<Case> {
    vec![
        case(
            "POST",
            "/api/v1/estimates",
            vec![
                post("/api/v1/estimates")
                    .json(json!({"work": "Alpha copy", "tags": {"project": "bravo-ledger"}, "median": "1", "from": "2026-10-01", "to": "2026-10-31"}))
                    .expect(REFUSED),
            ],
        ),
        case(
            "GET",
            "/api/v1/estimates/{id}",
            vec![
                get(format!("/api/v1/estimates/{}", b.estimate)).expect(NOT_FOUND),
                get(format!("/api/v1/estimates/{}", alpha.estimate)).expect(OK),
            ],
        ),
        case(
            "POST",
            "/api/v1/estimates/{id}/accept",
            vec![post(format!("/api/v1/estimates/{}/accept", b.estimate)).expect(NOT_FOUND)],
        ),
        case(
            "GET",
            "/api/v1/shares",
            vec![get("/api/v1/shares").expect(OK).check("", json!([]))],
        ),
        case(
            "POST",
            "/api/v1/shares",
            vec![
                post("/api/v1/shares")
                    .json(json!({"person": "bruno@bravo.example", "project": "alpha-app", "fraction": "0.5", "from": "2026-10-01"}))
                    .expect(REFUSED),
                post("/api/v1/shares")
                    .json(json!({"person": "alice@alpha.example", "project": "bravo-ledger", "fraction": "0.5", "from": "2026-10-01"}))
                    .expect(REFUSED),
            ],
        ),
    ]
}

fn message_cases(b: &Bravo) -> Vec<Case> {
    vec![
        case(
            "GET",
            "/api/v1/messages",
            vec![
                get("/api/v1/messages?from=2026-09-01&to=2026-10-31")
                    .expect(OK)
                    .allow(&ALPHA_STORED),
                get("/api/v1/messages?from=2026-09-01&to=2026-10-31&session_id=bravo-session-3")
                    .expect(OK)
                    .check("/messages", json!([])),
                get("/api/v1/messages?from=2026-09-01&to=2026-10-31&filter=project:bravo-ledger")
                    .expect(OK)
                    .check("/messages", json!([])),
            ],
        ),
        case(
            "GET",
            "/api/v1/sessions",
            vec![
                get("/api/v1/sessions?from=2026-09-01&to=2026-10-31")
                    .expect(OK)
                    .allow(&ALPHA_STORED),
            ],
        ),
        case(
            "POST",
            "/api/v1/splits",
            vec![
                post("/api/v1/splits")
                    .json(json!({"session_id": "bravo-session-3", "parts": [{"tags": {"feature": "ALPHA-1"}}]}))
                    .expect(REFUSED),
            ],
        ),
        case(
            "GET",
            "/api/v1/submissions",
            vec![
                get("/api/v1/submissions").expect(OK),
                get("/api/v1/submissions?person=bruno@bravo.example")
                    .expect(OK)
                    .check("", json!([])),
            ],
        ),
        case(
            "POST",
            "/api/v1/submissions",
            vec![
                post("/api/v1/submissions")
                    .json(json!({}))
                    .expect(CREATED),
            ],
        ),
        case(
            "POST",
            "/api/v1/submissions/{id}/approve",
            vec![post(format!("/api/v1/submissions/{}/approve", b.submission)).expect(NOT_FOUND)],
        ),
        case(
            "POST",
            "/api/v1/submissions/{id}/reject",
            vec![
                post(format!("/api/v1/submissions/{}/reject", b.submission))
                    .json(json!({"reason": "not ours"}))
                    .expect(NOT_FOUND),
            ],
        ),
        case(
            "GET",
            "/api/v1/submit-requests",
            vec![get("/api/v1/submit-requests").expect(OK)],
        ),
        case(
            "POST",
            "/api/v1/submit-requests",
            vec![
                post("/api/v1/submit-requests")
                    .json(json!({"until": "2099-12-31"}))
                    .expect(CREATED),
            ],
        ),
    ]
}

fn ingest_cases() -> Vec<Case> {
    let csv = "agent_harness,model,started_at,ended_at,tokens.input,tokens.output,tokens.cache_read,tokens.cache_write,feature\n\
               claude-code,claude-opus-5,2026-10-07T09:00:00Z,2026-10-07T09:30:00Z,100,10,0,0,ALPHA-1\n";
    let otlp =
        |signal: &str| post(format!("/api/v1/ingest/otlp/v1/{signal}")).by(Cred::Organization);
    vec![
        case(
            "POST",
            "/api/v1/token-entries",
            vec![
                post("/api/v1/token-entries")
                    .by(Cred::Service)
                    .json(session_entry(
                        "alpha-local-2",
                        "bravo-session-1",
                        "2026-10-07T09:00:00Z",
                        "BRAVO-7",
                        "Alpha work under Bravo's names",
                    ))
                    .expect(CREATED)
                    .check("/unknown_tags", json!(["feature: BRAVO-7"])),
            ],
        ),
        case(
            "POST",
            "/api/v1/token-entries/csv/preview",
            vec![
                post("/api/v1/token-entries/csv/preview")
                    .raw("text/csv", csv)
                    .expect(OK),
            ],
        ),
        case(
            "POST",
            "/api/v1/token-entries/csv",
            vec![
                post("/api/v1/token-entries/csv")
                    .raw("text/csv", csv)
                    .expect(CREATED),
            ],
        ),
        case(
            "POST",
            "/api/v1/ingest/otlp/v1/logs",
            vec![
                otlp("logs")
                    .json(telemetry_with_bravos_session())
                    .expect(OK),
            ],
        ),
        case(
            "POST",
            "/api/v1/ingest/otlp/v1/traces",
            vec![otlp("traces").json(trace_with_bravos_session()).expect(OK)],
        ),
        case(
            "POST",
            "/api/v1/ingest/otlp/v1/metrics",
            vec![
                otlp("metrics")
                    .json(json!({"resourceMetrics": []}))
                    .expect(OK),
            ],
        ),
    ]
}

/// Telemetry that names Bravo's session and Bravo's member.
fn telemetry_with_bravos_session() -> Value {
    let text = |key: &str, value: &str| json!({ "key": key, "value": { "stringValue": value } });
    let int = |key: &str, value: i64| json!({ "key": key, "value": { "intValue": value } });
    json!({ "resourceLogs": [{
        "resource": { "attributes": [text("service.name", "gemini-cli")] },
        "scopeLogs": [{ "scope": { "name": "gemini-cli" }, "logRecords": [{
            "timeUnixNano": "1791199800250000000",
            "attributes": [
                text("event.name", "gemini_cli.api_response"),
                text("session.id", "bravo-session-2"),
                text("user.email", "bruno@bravo.example"),
                text("model", "gemini-2.5-pro"),
                int("input_token_count", 9000),
                int("output_token_count", 420),
            ],
        }] }],
    }] })
}

/// A Copilot CLI `chat` span that names Bravo's session and Bravo's member.
fn trace_with_bravos_session() -> Value {
    let text = |key: &str, value: &str| json!({ "key": key, "value": { "stringValue": value } });
    let int = |key: &str, value: i64| json!({ "key": key, "value": { "intValue": value } });
    json!({ "resourceSpans": [{
        "resource": { "attributes": [
            text("service.name", "github-copilot"),
            text("user.email", "bruno@bravo.example"),
        ] },
        "scopeSpans": [{ "scope": { "name": "github.copilot" }, "spans": [{
            "traceId": "4bf92f3577b34da6a3ce929d0e0e4736",
            "spanId": "00f067aa0ba902b7",
            "name": "chat claude-sonnet-5",
            "kind": 3,
            "startTimeUnixNano": "1791199900000000000",
            "endTimeUnixNano": "1791199907000000000",
            "attributes": [
                text("gen_ai.operation.name", "chat"),
                text("gen_ai.request.model", "claude-sonnet-5"),
                text("gen_ai.conversation.id", "bravo-session-2"),
                text("gen_ai.response.id", "alpha-span-1"),
                int("gen_ai.usage.input_tokens", 5200),
                int("gen_ai.usage.output_tokens", 140),
            ],
        }] }],
    }] })
}

fn bill_cases(b: &Bravo) -> Vec<Case> {
    let csv = "provider,from,to,amount,currency\nopenai,2026-10-01,2026-10-31,5.00,USD\n";
    vec![
        case(
            "GET",
            "/api/v1/bills",
            vec![
                get("/api/v1/bills").expect(OK),
                get("/api/v1/bills?from=2026-09-01&to=2026-09-30&provider=anthropic&reversed=true")
                    .expect(OK)
                    .check("", json!([])),
            ],
        ),
        case(
            "POST",
            "/api/v1/bills",
            vec![
                post("/api/v1/bills")
                    .json(json!({"provider": "anthropic", "from": "2026-10-01", "to": "2026-10-31", "amount": "5.00", "currency": "USD", "typed_in": true, "lines": []}))
                    .expect(CREATED),
            ],
        ),
        case(
            "GET",
            "/api/v1/bills/{id}",
            vec![get(format!("/api/v1/bills/{}", b.bill)).expect(NOT_FOUND)],
        ),
        case(
            "POST",
            "/api/v1/bills/csv/preview",
            vec![
                post("/api/v1/bills/csv/preview")
                    .raw("text/csv", csv)
                    .expect(OK),
            ],
        ),
        case(
            "POST",
            "/api/v1/bills/csv",
            vec![post("/api/v1/bills/csv").raw("text/csv", csv).expect(CREATED)],
        ),
        case(
            "GET",
            "/api/v1/reconciliation",
            vec![get("/api/v1/reconciliation").expect(OK)],
        ),
        case(
            "GET",
            "/api/v1/bill-connections",
            vec![get("/api/v1/bill-connections").expect(OK)],
        ),
        case(
            "POST",
            "/api/v1/bill-connections",
            vec![
                post("/api/v1/bill-connections")
                    .json(json!({"provider": "openai", "key": "sk-admin-alpha-secret"}))
                    .expect(CREATED),
            ],
        ),
        case(
            "DELETE",
            "/api/v1/bill-connections/{id}",
            vec![delete(format!("/api/v1/bill-connections/{}", b.bill_connection)).expect(NOT_FOUND)],
        ),
        case(
            "POST",
            "/api/v1/bill-connections/{id}/pull",
            vec![post(format!("/api/v1/bill-connections/{}/pull", b.bill_connection)).expect(NOT_FOUND)],
        ),
    ]
}

fn ticket_system_cases(b: &Bravo) -> Vec<Case> {
    vec![
        case(
            "GET",
            "/api/v1/ticket-systems",
            vec![get("/api/v1/ticket-systems").expect(OK)],
        ),
        case(
            "POST",
            "/api/v1/ticket-systems",
            vec![
                post("/api/v1/ticket-systems")
                    .json(json!({"kind": "jira", "site": "https://alpha.atlassian.net", "token": "ATATT3xFfGF0-alpha", "email": "alice@alpha.example"}))
                    .expect(CREATED),
            ],
        ),
        case(
            "DELETE",
            "/api/v1/ticket-systems/{id}",
            vec![delete(format!("/api/v1/ticket-systems/{}", b.ticket_system)).expect(NOT_FOUND)],
        ),
        case(
            "PUT",
            "/api/v1/ticket-systems/{id}/mappings",
            vec![
                put(format!("/api/v1/ticket-systems/{}/mappings", b.ticket_system))
                    .json(json!([{"external": "BRV", "project": "alpha-app"}]))
                    .expect(NOT_FOUND),
            ],
        ),
        case(
            "GET",
            "/api/v1/ticket-systems/{id}/projects",
            vec![get(format!("/api/v1/ticket-systems/{}/projects", b.ticket_system)).expect(NOT_FOUND)],
        ),
        case(
            "POST",
            "/api/v1/ticket-systems/{id}/sync",
            vec![post(format!("/api/v1/ticket-systems/{}/sync", b.ticket_system)).expect(NOT_FOUND)],
        ),
    ]
}

fn report_cases() -> Vec<Case> {
    let window = "from=2026-10-01&to=2026-10-31";
    vec![
        case(
            "GET",
            "/api/v1/reports/cost",
            vec![
                get(format!("/api/v1/reports/cost?{window}&by=project"))
                    .expect(OK)
                    .allow(&ALPHA_STORED),
                get(format!("/api/v1/reports/cost?{window}&by=feature&filter=project:bravo-ledger"))
                    .expect(OK)
                    .check("/rows", json!([])),
                get(format!("/api/v1/reports/cost?{window}&by=person&format=csv"))
                    .expect(OK)
                    .allow(&ALPHA_STORED),
            ],
        ),
        case(
            "GET",
            "/api/v1/reports/deviation",
            vec![
                get(format!("/api/v1/reports/deviation?{window}")).expect(OK),
                get(format!("/api/v1/reports/deviation?{window}&filter=project:bravo-ledger"))
                    .expect(OK)
                    .check("/rows/0/cost", json!("0.00"))
                    .check("/rows/0/plan", Value::Null),
            ],
        ),
        case(
            "GET",
            "/api/v1/reports/forecast",
            vec![
                get(format!("/api/v1/reports/forecast?{window}&by=project"))
                    .expect(OK)
                    .allow(&ALPHA_STORED),
                get(format!("/api/v1/reports/forecast?{window}&filter=project:bravo-ledger"))
                    .expect(OK)
                    .check("/rows/0/forecast", json!("0.00")),
            ],
        ),
        case(
            "GET",
            "/api/v1/reports/saved",
            vec![get("/api/v1/reports/saved").expect(OK).allow(&ALPHA_STORED)],
        ),
        case(
            "POST",
            "/api/v1/reports/saved",
            vec![
                post("/api/v1/reports/saved")
                    .json(json!({"name": "Alpha copy", "filter": {"project": "bravo-ledger"}, "by": "feature"}))
                    .expect(CREATED),
            ],
        ),
    ]
}

fn cutoff_cases() -> Vec<Case> {
    vec![case(
        "POST",
        "/api/v1/cutoff",
        vec![
            post("/api/v1/cutoff")
                .json(json!({"cutoff": "2026-09-30"}))
                .expect(CREATED),
        ],
    )]
}

fn public_cases() -> Vec<Case> {
    let bruno = json!({"email": "bruno@bravo.example"});
    vec![
        case(
            "GET",
            "/api/v1/health",
            vec![get("/api/v1/health").by(Cred::Nobody).expect(OK)],
        ),
        case(
            "GET",
            "/api/v1/status",
            vec![get("/api/v1/status").by(Cred::Nobody).expect(OK)],
        ),
        case(
            "POST",
            "/api/v1/sign-in",
            vec![
                post("/api/v1/sign-in")
                    .by(Cred::Nobody)
                    .json(bruno.clone())
                    .expect(ACCEPTED),
            ],
        ),
        case(
            "GET",
            "/api/v1/sign-in/link",
            vec![get("/api/v1/sign-in/link?token=tc_link_0000").by(Cred::Nobody)],
        ),
        case(
            "POST",
            "/api/v1/sign-in/link",
            vec![
                post("/api/v1/sign-in/link")
                    .by(Cred::Nobody)
                    .raw("application/x-www-form-urlencoded", "token=tc_link_0000"),
            ],
        ),
        case(
            "POST",
            "/api/v1/sign-in/poll",
            vec![
                post("/api/v1/sign-in/poll")
                    .by(Cred::Nobody)
                    .json(json!({"poll_token": "tc_poll_0000"})),
            ],
        ),
        case(
            "GET",
            "/api/v1/sign-in/providers",
            vec![get("/api/v1/sign-in/providers").by(Cred::Nobody).expect(OK)],
        ),
        case(
            "POST",
            "/api/v1/sign-up",
            vec![post("/api/v1/sign-up")
                .by(Cred::Nobody)
                .json(json!({"email": "boss@bravo.example", "organization_name": "Bravo Holdings"}))
                .expect(ACCEPTED)],
        ),
        case(
            "GET",
            "/api/v1/sign-up/link",
            vec![get("/api/v1/sign-up/link?token=tc_link_0000").by(Cred::Nobody)],
        ),
        case(
            "POST",
            "/api/v1/sign-up/link",
            vec![
                post("/api/v1/sign-up/link")
                    .by(Cred::Nobody)
                    .json(json!({"token": "tc_link_0000", "organization_name": "Bravo Holdings"})),
            ],
        ),
        case(
            "POST",
            "/api/v1/sign-out",
            vec![post("/api/v1/sign-out").by(Cred::SignOut)],
        ),
    ]
}

fn mcp_call(tool: &str, arguments: &Value) -> Value {
    json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": tool, "arguments": arguments}})
}

const MCP_TOOLS: [&str; 6] = [
    "organization",
    "tag_values",
    "attributes",
    "people",
    "plans",
    "report",
];

fn mcp_cases() -> Vec<Case> {
    let mcp = "/api/v1/mcp";
    let mut asks = vec![
        post(mcp)
            .json(json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}))
            .expect(OK),
        post(mcp)
            .json(mcp_call("tag_values", &json!({"query": "bravo"})))
            .expect(OK)
            .check("/result/structuredContent/result", json!([])),
        post(mcp)
            .json(mcp_call("tag_values", &json!({"name": "bravozone"})))
            .expect(OK)
            .check("/result/structuredContent/result", json!([])),
        post(mcp)
            .json(mcp_call("report", &json!({"from": "2026-10-01", "to": "2026-10-31", "by": "feature", "filter": "project:bravo-ledger"})))
            .expect(OK)
            .check("/result/structuredContent/result/rows", json!([])),
        post(mcp)
            .json(mcp_call("report", &json!({"from": "2026-10-01", "to": "2026-10-31", "by": "project"})))
            .expect(OK)
            .allow(&ALPHA_STORED),
    ];
    for tool in ["organization", "attributes", "people", "plans"] {
        asks.push(post(mcp).json(mcp_call(tool, &json!({}))).expect(OK));
    }
    // The read token too.
    asks.push(
        post(mcp)
            .by(Cred::Read)
            .json(mcp_call("people", &json!({})))
            .expect(OK),
    );
    vec![case("POST", "/api/v1/mcp", asks)]
}

fn all_cases(alpha: &Alpha, bravo: &Bravo) -> Vec<Case> {
    let mut cases = token_cases(alpha, bravo);
    cases.extend(member_cases(bravo));
    cases.extend(me_cases());
    cases.extend(organization_cases());
    cases.extend(tag_cases(bravo));
    cases.extend(plan_cases(alpha, bravo));
    cases.extend(message_cases(bravo));
    cases.extend(bill_cases(bravo));
    cases.extend(ticket_system_cases(bravo));
    cases.extend(report_cases());
    cases.extend(mcp_cases());
    // Alpha's messages reuse Bravo's session ids and feature key, which
    // then are Alpha's own too; reads before them see none of that.
    cases.extend(ingest_cases());
    // The cutoff freezes Alpha's earlier figures, so it goes after the
    // writes that are dated before it.
    cases.extend(cutoff_cases());
    // Signing out ends a session, so it goes last.
    cases.extend(public_cases());
    cases
}

// ---------------------------------------------------------------------------
// Running the cases

/// Whether `path` (without its query) fits the OpenAPI path `template`.
fn fits(template: &str, path: &str) -> bool {
    let path = path.split('?').next().unwrap_or_default();
    let template: Vec<&str> = template.split('/').collect();
    let path: Vec<&str> = path.split('/').collect();
    template.len() == path.len()
        && template
            .iter()
            .zip(&path)
            .all(|(want, got)| want == got || (want.starts_with('{') && !got.is_empty()))
}

/// Every method and path in the OpenAPI document the router serves.
fn documented_routes() -> TestResult<BTreeSet<(String, String)>> {
    let document: Value = serde_json::from_str(&tokencontroller_server::openapi_json()?)?;
    let paths = document
        .get("paths")
        .and_then(Value::as_object)
        .ok_or("the document has no paths")?;
    let mut routes = BTreeSet::new();
    for (path, operations) in paths {
        for method in operations
            .as_object()
            .ok_or("an operation is no object")?
            .keys()
        {
            routes.insert((method.to_uppercase(), path.clone()));
        }
    }
    Ok(routes)
}

/// The two organizations and the service they share.
struct Tenants<'a> {
    app: &'a Router,
    pool: &'a PgPool,
    alpha: &'a Alpha,
    bravo: &'a Bravo,
}

/// What Alpha's calls found: one line per problem.
#[derive(Default)]
struct Findings {
    problems: Vec<String>,
    calls: usize,
}

/// Bravo's names and ids in `answer`, but for those `request` itself sent.
fn leaks(bravo: &Bravo, request: &str, answer: &str) -> Vec<String> {
    bravo
        .secrets
        .iter()
        .filter(|secret| !secret.is_empty())
        .filter(|secret| !request.contains(secret.as_str()))
        .filter(|secret| answer.contains(secret.as_str()))
        .cloned()
        .collect()
}

/// Whether the status and the check of `ask` hold for `answer`.
fn expectations(ask: &Ask, label: &str, answer: &Answer, findings: &mut Findings) {
    let status = answer.status.as_u16();
    if !ask.expect.is_empty() && !ask.expect.contains(&status) {
        findings.problems.push(format!(
            "{label}: {status}, expected {:?}: {}",
            ask.expect,
            answer.text()
        ));
    }
    for (pointer, want) in &ask.checks {
        let got = answer
            .json()
            .pointer(pointer)
            .cloned()
            .unwrap_or(Value::Null);
        if got != *want {
            findings
                .problems
                .push(format!("{label}: {pointer:?} is {got}, expected {want}"));
        }
    }
}

async fn run_ask(
    tenants: &Tenants<'_>,
    ask: &Ask,
    cred: Cred,
    findings: &mut Findings,
) -> TestResult<()> {
    let answer = call(
        tenants.app,
        ask.method.clone(),
        &ask.path,
        tenants.alpha.token(cred),
        &ask.payload,
    )
    .await?;
    findings.calls = findings.calls.saturating_add(1);
    let label = format!("{} {} as {cred:?}", ask.method, ask.path);
    let request = format!("{} {}", ask.path, ask.payload.text());
    if answer.status.is_server_error() {
        findings
            .problems
            .push(format!("{label}: {} {}", answer.status, answer.text()));
    }
    // A read token may be refused by design; what it is answered must hold
    // as for the session.
    let refused_by_design = cred != ask.cred
        && matches!(
            answer.status,
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        );
    if !refused_by_design {
        expectations(ask, &label, &answer, findings);
    }
    let mut leaked = leaks(tenants.bravo, &request, &answer.readable()?);
    if leaked
        .iter()
        .any(|secret| ask.allow.contains(&secret.as_str()))
    {
        let alphas = organization_rows(tenants.pool, tenants.alpha.organization_id)
            .await?
            .join("\n");
        leaked.retain(|secret| {
            !(ask.allow.contains(&secret.as_str()) && alphas.contains(secret.as_str()))
        });
    }
    if !leaked.is_empty() {
        findings.problems.push(format!(
            "{label} answers with Bravo's {leaked:?}: {}",
            answer.text()
        ));
    }
    Ok(())
}

async fn run(tenants: &Tenants<'_>, cases: &[Case], findings: &mut Findings) -> TestResult<()> {
    for case in cases {
        for ask in &case.asks {
            if ask.method.as_str() != case.method || !fits(case.template, &ask.path) {
                findings.problems.push(format!(
                    "{} {} is filed under {} {}",
                    ask.method, ask.path, case.method, case.template
                ));
            }
            run_ask(tenants, ask, ask.cred, findings).await?;
            // Every read again with Alpha's read token.
            if ask.method == Method::GET && ask.cred == Cred::Session {
                run_ask(tenants, ask, Cred::Read, findings).await?;
            }
        }
    }
    Ok(())
}

/// Alpha's reads that count and total its usage: an organization lost from
/// one of their queries would take in Bravo's without naming any of it.
const ALPHA_AGGREGATES: [&str; 12] = [
    "/api/v1/reports/cost?from=2026-09-01&to=2026-09-30&by=project",
    "/api/v1/reports/cost?from=2026-10-01&to=2026-10-31&by=project",
    "/api/v1/reports/cost?from=2026-10-01&to=2026-10-31&by=feature",
    "/api/v1/reports/cost?from=2026-10-01&to=2026-10-31&by=person",
    "/api/v1/reports/deviation?from=2026-10-01&to=2026-10-31",
    "/api/v1/reports/forecast?from=2026-10-01&to=2026-10-31&by=project",
    "/api/v1/reconciliation",
    "/api/v1/messages?from=2026-09-01&to=2026-10-31",
    "/api/v1/sessions?from=2026-09-01&to=2026-10-31",
    "/api/v1/submissions",
    "/api/v1/price-table",
    "/api/v1/exports/raw",
];

/// Alpha's counts and totals, read as Alpha.
async fn alpha_totals(app: &Router, alpha: &Alpha) -> TestResult<Vec<String>> {
    let mut totals = Vec::new();
    for path in ALPHA_AGGREGATES {
        let answer = call(app, Method::GET, path, &alpha.session, &Payload::Empty).await?;
        // The export's and the reports' own time is no total.
        let readable = answer.readable()?;
        let kept: Vec<&str> = readable
            .lines()
            .filter(|line| !line.contains("\"exported_at\"") && !line.contains("\"as_of\""))
            .collect();
        let kept = kept.join("\n");
        let kept = strip_as_of(&kept);
        totals.push(format!("{path}: {} {kept}", answer.status));
    }
    Ok(totals)
}

/// A JSON answer without its `as_of` moments, which are the time of the
/// call.
fn strip_as_of(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some((before, after)) = rest.split_once("\"as_of\":\"") {
        out.push_str(before);
        let Some((_, tail)) = after.split_once('"') else {
            return out;
        };
        rest = tail.strip_prefix(',').unwrap_or(tail);
    }
    out.push_str(rest);
    out
}

/// Bravo's own view of its data, read through the API as Bravo.
async fn bravo_view(app: &Router, bravo: &Bravo) -> TestResult<Vec<String>> {
    let boss = bravo.boss.as_str();
    let mut view = Vec::new();
    for path in [
        "/api/v1/organization".to_owned(),
        "/api/v1/members".to_owned(),
        "/api/v1/api-tokens".to_owned(),
        "/api/v1/tags".to_owned(),
        "/api/v1/attributes".to_owned(),
        "/api/v1/rules".to_owned(),
        "/api/v1/grants".to_owned(),
        "/api/v1/cutoff".to_owned(),
        "/api/v1/price-table".to_owned(),
        format!("/api/v1/estimates/{}", bravo.estimate),
        "/api/v1/shares".to_owned(),
        "/api/v1/messages?from=2026-09-01&to=2026-10-31".to_owned(),
        "/api/v1/sessions?from=2026-09-01&to=2026-10-31".to_owned(),
        "/api/v1/submissions".to_owned(),
        "/api/v1/submit-requests".to_owned(),
        "/api/v1/bills".to_owned(),
        format!("/api/v1/bills/{}", bravo.bill),
        "/api/v1/reconciliation".to_owned(),
        "/api/v1/reports/saved".to_owned(),
        "/api/v1/bill-connections".to_owned(),
        "/api/v1/ticket-systems".to_owned(),
        "/api/v1/reports/cost?from=2026-10-01&to=2026-10-31&by=feature".to_owned(),
        "/api/v1/reports/deviation?from=2026-10-01&to=2026-10-31".to_owned(),
        "/api/v1/events".to_owned(),
    ] {
        let answer = call(app, Method::GET, &path, boss, &Payload::Empty).await?;
        view.push(format!(
            "{path}: {} {}",
            answer.status,
            strip_as_of(&answer.text())
        ));
    }
    for path in [
        "/api/v1/me/rules",
        "/api/v1/me/context",
        "/api/v1/submissions",
    ] {
        let answer = call(
            app,
            Method::GET,
            path,
            &bravo.bruno_browser,
            &Payload::Empty,
        )
        .await?;
        view.push(format!(
            "{path} for Bruno: {} {}",
            answer.status,
            strip_as_of(&answer.text())
        ));
    }
    Ok(view)
}

/// An organization's rows in every table that has one, and in the tables
/// that hang off them, as text.
async fn organization_rows(pool: &PgPool, organization_id: Uuid) -> TestResult<Vec<String>> {
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT table_name::text FROM information_schema.columns
         WHERE table_schema = 'public' AND column_name = 'organization_id' ORDER BY 1",
    )
    .fetch_all(pool)
    .await?;
    let mut queries: Vec<String> = tables
        .iter()
        .map(|table| format!("SELECT t::text FROM {table} t WHERE organization_id = $1 ORDER BY 1"))
        .collect();
    queries.extend(
        [
            "SELECT o::text FROM organizations o WHERE id = $1",
            "SELECT r::text FROM submit_reminders r JOIN submit_requests q ON q.id = r.request_id WHERE q.organization_id = $1 ORDER BY 1",
            "SELECT count(*)::text FROM sessions s JOIN people p ON p.id = s.person_id WHERE p.organization_id = $1",
        ]
        .map(str::to_owned),
    );
    let mut rows = Vec::new();
    for query in queries {
        let found: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(query.clone()))
            .bind(organization_id)
            .fetch_all(pool)
            .await?;
        rows.push(format!("{query}\n{}", found.join("\n")));
    }
    Ok(rows)
}

fn differences(before: &[String], after: &[String]) -> Vec<String> {
    before
        .iter()
        .zip(after)
        .filter(|(was, is)| was != is)
        .map(|(was, is)| format!("was: {was}\nis:  {is}"))
        .collect()
}

// ---------------------------------------------------------------------------
// The tests

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn every_route_refuses_another_organizations_data(pool: PgPool) {
    let app = router(AppState::new(pool.clone(), config(), Arc::new(NoExtension)));
    let alpha = alpha(&app, &pool).await.unwrap();
    let alone = alpha_totals(&app, &alpha).await.unwrap();
    let bravo = bravo(&app, &pool).await.unwrap();
    let changed = differences(&alone, &alpha_totals(&app, &alpha).await.unwrap());
    assert!(
        changed.is_empty(),
        "Alpha's totals moved with Bravo's usage:\n{}",
        changed.join("\n\n")
    );
    let tenants = Tenants {
        app: &app,
        pool: &pool,
        alpha: &alpha,
        bravo: &bravo,
    };

    let rows_before = organization_rows(&pool, bravo.organization_id)
        .await
        .unwrap();
    let view_before = bravo_view(&app, &bravo).await.unwrap();

    let mut findings = Findings::default();
    run(&tenants, &all_cases(&alpha, &bravo), &mut findings)
        .await
        .unwrap();
    // Every read once more, now that Alpha has written with Bravo's ids.
    let reads: Vec<Case> = all_cases(&alpha, &bravo)
        .into_iter()
        .map(|mut case| {
            case.asks
                .retain(|ask| ask.method == Method::GET && ask.cred != Cred::Nobody);
            case
        })
        .collect();
    run(&tenants, &reads, &mut findings).await.unwrap();
    assert!(findings.calls > 150, "only {} calls", findings.calls);
    assert!(
        findings.problems.is_empty(),
        "{} problems:\n{}",
        findings.problems.len(),
        findings.problems.join("\n\n")
    );

    let changed = differences(&view_before, &bravo_view(&app, &bravo).await.unwrap());
    assert!(
        changed.is_empty(),
        "Bravo's view changed:\n{}",
        changed.join("\n\n")
    );
    let changed = differences(
        &rows_before,
        &organization_rows(&pool, bravo.organization_id)
            .await
            .unwrap(),
    );
    assert!(
        changed.is_empty(),
        "Bravo's rows changed:\n{}",
        changed.join("\n\n")
    );
    // The telemetry Alpha sent with Bravo's session and member is Alpha's
    // alone: both model calls are stored for Alpha and none for Bravo.
    assert_eq!(
        telemetry_with_bravos_session_id(&pool).await.unwrap(),
        vec![
            (alpha.organization_id, "claude-sonnet-5".to_owned()),
            (alpha.organization_id, "gemini-2.5-pro".to_owned()),
        ]
    );

    // Bravo's usage grows; Alpha's totals stay.
    let with_bravo = alpha_totals(&app, &alpha).await.unwrap();
    let bravos_cost = "/api/v1/reports/cost?from=2026-10-01&to=2026-10-31&by=project";
    let before = call(&app, Method::GET, bravos_cost, &bravo.boss, &Payload::Empty)
        .await
        .unwrap();
    bravo_grows(&app, &bravo).await.unwrap();
    let after = call(&app, Method::GET, bravos_cost, &bravo.boss, &Payload::Empty)
        .await
        .unwrap();

    assert_ne!(
        before.json()["total"],
        after.json()["total"],
        "Bravo's own cost did not grow"
    );
    let changed = differences(&with_bravo, &alpha_totals(&app, &alpha).await.unwrap());
    assert!(
        changed.is_empty(),
        "Alpha's totals counted Bravo's usage:\n{}",
        changed.join("\n\n")
    );
}

/// One more message for Bravo, in October like Alpha's.
async fn bravo_grows(app: &Router, bravo: &Bravo) -> TestResult<()> {
    let service = must(
        app,
        Method::POST,
        "/api/v1/api-tokens",
        &bravo.boss,
        Payload::Json(json!({"kind": "service", "name": "Bravo second bot"})),
    )
    .await?;
    must(
        app,
        Method::POST,
        "/api/v1/token-entries",
        &text_at(&service, "/token")?,
        Payload::Json(session_entry(
            "bravo-local-5",
            "bravo-session-5",
            "2026-10-08T09:00:00Z",
            "BRAVO-7",
            "Bravo later summary",
        )),
    )
    .await?;
    Ok(())
}

/// The model calls stored from telemetry with Bravo's session id, and for
/// which organization.
async fn telemetry_with_bravos_session_id(pool: &PgPool) -> TestResult<Vec<(Uuid, String)>> {
    Ok(sqlx::query_as(
        "SELECT organization_id, payload->'message'->'model'->>'model' FROM events
         WHERE kind = 'message' AND payload->'message'->>'session_id' = 'bravo-session-2'
           AND payload->'message'->>'agent_harness' <> 'claude-code'
         ORDER BY 2",
    )
    .fetch_all(pool)
    .await?)
}

/// The leak check sees Bravo's data where it is: the reads Alpha makes
/// show it to Bravo.
#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_leak_check_sees_bravos_data(pool: PgPool) {
    let app = router(AppState::new(pool.clone(), config(), Arc::new(NoExtension)));
    let bravo = bravo(&app, &pool).await.unwrap();
    for path in [
        "/api/v1/tags".to_owned(),
        "/api/v1/members".to_owned(),
        format!("/api/v1/estimates/{}", bravo.estimate),
        "/api/v1/messages?from=2026-09-01&to=2026-10-31".to_owned(),
        "/api/v1/reports/cost?from=2026-10-01&to=2026-10-31&by=feature".to_owned(),
        "/api/v1/exports/raw".to_owned(),
        "/api/v1/events".to_owned(),
    ] {
        let answer = call(&app, Method::GET, &path, &bravo.boss, &Payload::Empty)
            .await
            .unwrap();
        let found = leaks(&bravo, "", &answer.readable().unwrap());
        assert!(
            !found.is_empty(),
            "{path} as Bravo shows nothing the check knows ({})",
            answer.status
        );
    }
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn every_route_is_called_across_tenants(pool: PgPool) {
    let app = router(AppState::new(pool.clone(), config(), Arc::new(NoExtension)));
    let alpha = alpha(&app, &pool).await.unwrap();
    let bravo = bravo(&app, &pool).await.unwrap();
    let covered: BTreeSet<(String, String)> = all_cases(&alpha, &bravo)
        .iter()
        .filter(|case| !case.asks.is_empty())
        .map(|case| (case.method.to_owned(), case.template.to_owned()))
        .collect();
    let documented = documented_routes().unwrap();
    let missing: Vec<_> = documented.difference(&covered).collect();
    let unknown: Vec<_> = covered.difference(&documented).collect();
    assert!(
        missing.is_empty(),
        "routes without a tenant isolation case: {missing:?}"
    );
    assert!(
        unknown.is_empty(),
        "cases for routes the API does not have: {unknown:?}"
    );
    // The committed OpenAPI file is the same document (api.rs checks it).
    assert_eq!(documented.len(), 90, "{documented:?}");
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn every_mcp_tool_is_called_across_tenants(pool: PgPool) {
    let app = router(AppState::new(pool.clone(), config(), Arc::new(NoExtension)));
    let alpha = alpha(&app, &pool).await.unwrap();
    let list = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"});
    let answer = call(
        &app,
        Method::POST,
        "/api/v1/mcp",
        &alpha.session,
        &Payload::Json(list),
    )
    .await
    .unwrap();
    let tools: BTreeSet<String> = answer.json()["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_owned())
        .collect();
    let called: BTreeSet<String> = MCP_TOOLS.map(str::to_owned).into_iter().collect();
    assert_eq!(tools, called, "an MCP tool has no tenant isolation case");
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_filter_on_another_organizations_names_counts_nothing(pool: PgPool) {
    let app = router(AppState::new(pool.clone(), config(), Arc::new(NoExtension)));
    let alpha = alpha(&app, &pool).await.unwrap();
    let bravo = bravo(&app, &pool).await.unwrap();
    let session = alpha.session.as_str();
    // Alpha names Bravo's project in a saved report and in reports.
    let saved = must(
        &app,
        Method::POST,
        "/api/v1/reports/saved",
        session,
        Payload::Json(
            json!({"name": "Alpha copy", "filter": {"project": "bravo-ledger"}, "by": "feature"}),
        ),
    )
    .await
    .unwrap();
    assert_eq!(saved["filter"]["project"], "bravo-ledger");
    for report in ["cost", "deviation", "forecast"] {
        let path = format!(
            "/api/v1/reports/{report}?from=2026-10-01&to=2026-10-31&by=feature&filter=project:bravo-ledger"
        );
        let answer = call(&app, Method::GET, &path, session, &Payload::Empty)
            .await
            .unwrap();
        assert_eq!(answer.status, StatusCode::OK, "{}", answer.text());
        let rows = answer.json()["rows"].clone();
        assert!(
            rows.as_array()
                .is_some_and(|rows| rows.iter().all(|row| row["key"].is_null())),
            "{}",
            answer.text()
        );
        for marker in ["BRAVO-7", "777.77", &bravo.estimate] {
            assert!(!answer.text().contains(marker), "{marker} in {path}");
        }
    }
}
