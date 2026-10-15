//! Tenant isolation against the real service and a real Postgres database.
//!
//! Two organizations, Alpha Works and Bravo Holdings, each get data in every
//! kind of object the API has: members, tags, projects, tickets, token
//! entries, bills, a closed period with an adjustment and a cost sheet link,
//! cadences, token budgets, a token forecast, saved reports, API tokens,
//! bill connections and ticket systems. Then every route in the service's
//! OpenAPI document is called as Alpha's members with Bravo's IDs, names and
//! keys. Each call must be refused, or answer with nothing of Bravo's, and
//! Bravo's data must be exactly as it was afterwards.
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
use tokencontroller_server::budgets::embeddings::{EmbedError, Embedder};
use tokencontroller_server::budgets::forecast;
use tokencontroller_server::budgets::similarity::Embedding;
use tokencontroller_server::config::EmbeddingsConfig;
use tokencontroller_server::reports::scope::Scope;
use tokencontroller_server::sign_in::secret;
use tokencontroller_server::{AppState, Config, router};
use tower::ServiceExt as _;
use uuid::Uuid;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

const KEY: &str = "0123456789abcdef0123456789abcdef";

fn config() -> Config {
    Config {
        database_url: "postgres://unused".to_owned(),
        public_url: "http://localhost:8080".to_owned(),
        secret_key: KEY.to_owned(),
        smtp_url: None,
        smtp_from: None,
        prices_refresh: false,
        // A model nobody answers at: a token forecast for another
        // organization's ticket must be refused before the model is asked.
        embeddings: Some(EmbeddingsConfig {
            url: "http://127.0.0.1:9".to_owned(),
            key: None,
            model: "isolation-test".to_owned(),
        }),
    }
}

/// A stand-in for the embeddings model that Bravo's token forecast is made
/// with: every text means the same.
struct Constant;

impl Embedder for Constant {
    fn model(&self) -> &'static str {
        "constant"
    }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Embedding>, EmbedError> {
        texts
            .iter()
            .map(|_| {
                Embedding::from_millionths(vec![1_000_000, 0, 0])
                    .map_err(|error| EmbedError::Unreadable(error.to_string()))
            })
            .collect()
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
    let session = secret::generate(prefix, KEY)?;
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

/// A Claude Code session as `tc` sends it.
fn session_entry(
    local_id: &str,
    session_id: &str,
    started_at: &str,
    ticket: &str,
    summary: &str,
) -> Value {
    json!({
        "agent_harness": "claude-code",
        "model": "claude-opus-5",
        "started_at": started_at,
        "ended_at": started_at.replace("00:00Z", "30:00Z"),
        "tokens": {"input": 12000, "output": 3000},
        "reported_cost": "0.42",
        "summary": summary,
        "ticket": ticket,
        "session_id": session_id,
        "local_id": local_id
    })
}

// ---------------------------------------------------------------------------
// Alpha: the organization whose members call

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cred {
    /// Alice's browser session; she is Alpha's controller.
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
    ticket_id: Uuid,
    service_token_id: Uuid,
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
    must(
        app,
        Method::POST,
        "/api/v1/tags",
        &session,
        Payload::Json(json!({"name": "client"})),
    )
    .await?;
    must(
        app,
        Method::POST,
        "/api/v1/projects",
        &session,
        Payload::Json(json!({"name": "alpha-app", "ticket_key_pattern": "^ALPHA-\\d+"})),
    )
    .await?;
    let ticket = must(
        app,
        Method::POST,
        "/api/v1/tickets",
        &session,
        Payload::Json(json!({"key": "ALPHA-1", "title": "Alpha onboarding"})),
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
        ticket_id: id_at(&ticket, "/id")?,
        service_token_id: id_at(&service, "/id")?,
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

/// What Alpha's members must never see, and the IDs they try.
#[derive(Default)]
struct Bravo {
    organization_id: Uuid,
    boss: String,
    bruno_browser: String,
    bruno_cli: String,
    bruno: Uuid,
    petra: Uuid,
    project: Uuid,
    epic: Uuid,
    ticket: Uuid,
    invite_code: String,
    bill: Uuid,
    link: Uuid,
    link_secret: String,
    rejected_entry: Uuid,
    draft_entry: Uuid,
    submitted_entry: Uuid,
    locked_entry: Uuid,
    read_token: Uuid,
    service_token: Uuid,
    cli_session: Uuid,
    cadence: Uuid,
    budget: Uuid,
    saved_report: Uuid,
    bill_connection: Uuid,
    ticket_system: Uuid,
    forecast: Uuid,
    /// Every name, key, text and ID of Bravo's.
    secrets: Vec<String>,
}

fn bravo_rule() -> Value {
    json!({
        "name": "Bravo branch rule",
        "when": [{"signal": "branch", "test": {"starts_with": "bravo/"}}],
        "then": {"set_tags": {"bravozone": "bravo-team"}}
    })
}

async fn bravo_people(app: &Router, pool: &PgPool, bravo: &mut Bravo) -> TestResult<()> {
    let (organization_id, boss_id) =
        organization(pool, "Bravo Holdings", "boss@bravo.example").await?;
    bravo.organization_id = organization_id;
    bravo.boss = session_for(pool, boss_id, secret::SESSION).await?;
    let boss = bravo.boss.clone();
    for name in ["bravozone", "client"] {
        must(
            app,
            Method::POST,
            "/api/v1/tags",
            &boss,
            Payload::Json(json!({"name": name})),
        )
        .await?;
    }
    must(
        app,
        Method::PUT,
        "/api/v1/tag-map",
        &boss,
        Payload::Json(json!({"cost_center": "bravozone", "ladder": ["ticket", "project"]})),
    )
    .await?;
    let bruno = must(
        app,
        Method::POST,
        "/api/v1/members",
        &boss,
        Payload::Json(json!({"email": "bruno@bravo.example", "name": "Bruno Bravo", "tags": {"bravozone": "bravo-team"}})),
    )
    .await?;
    bravo.bruno = id_at(&bruno, "/member/id")?;
    sqlx::query("UPDATE people SET joined_at = now() WHERE id = $1")
        .bind(bravo.bruno)
        .execute(pool)
        .await?;
    let petra = must(
        app,
        Method::POST,
        "/api/v1/members",
        &boss,
        Payload::Json(json!({"email": "petra@bravo.example", "name": "Petra Bravo", "role": "people_manager"})),
    )
    .await?;
    bravo.petra = id_at(&petra, "/member/id")?;
    let invite_url = text_at(&petra, "/invite_url")?;
    invite_url
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .clone_into(&mut bravo.invite_code);
    must(
        app,
        Method::PUT,
        &format!("/api/v1/members/{}/filters", bravo.petra),
        &boss,
        Payload::Json(json!([{"bravozone": "bravo-team"}])),
    )
    .await?;
    must(
        app,
        Method::PUT,
        "/api/v1/organization/rules",
        &boss,
        Payload::Json(json!({"rules": [bravo_rule()]})),
    )
    .await?;
    bravo.bruno_browser = session_for(pool, bravo.bruno, secret::SESSION).await?;
    must(
        app,
        Method::PUT,
        "/api/v1/me/rules",
        &bravo.bruno_browser,
        Payload::Json(json!({"rules": [bravo_rule()]})),
    )
    .await?;
    Ok(())
}

async fn bravo_catalog(app: &Router, bravo: &mut Bravo) -> TestResult<()> {
    let boss = bravo.boss.clone();
    let project = must(
        app,
        Method::POST,
        "/api/v1/projects",
        &boss,
        Payload::Json(json!({
            "name": "bravo-ledger",
            "ticket_key_pattern": "^BRAVO-\\d+",
            "tags": {"client": "bravo-client", "bravozone": "bravo-team"},
            "repositories": ["bravo/ledger"]
        })),
    )
    .await?;
    bravo.project = id_at(&project, "/id")?;
    let epic = must(
        app,
        Method::POST,
        "/api/v1/tickets",
        &boss,
        Payload::Json(json!({"key": "BRAVO-1", "title": "Bravo epic"})),
    )
    .await?;
    bravo.epic = id_at(&epic, "/id")?;
    let ticket = must(
        app,
        Method::POST,
        "/api/v1/tickets",
        &boss,
        Payload::Json(json!({
            "key": "BRAVO-7",
            "title": "Bravo payroll migration",
            "description": "Bravo secret description",
            "parent_id": bravo.epic,
            "assignee_id": bravo.bruno
        })),
    )
    .await?;
    bravo.ticket = id_at(&ticket, "/id")?;
    Ok(())
}

/// September is closed with a bill, an adjustment and a cost sheet link;
/// October has a rejected, a draft and a submitted token entry.
async fn bravo_work(app: &Router, pool: &PgPool, bravo: &mut Bravo) -> TestResult<()> {
    let boss = bravo.boss.clone();
    bravo.bruno_cli = session_for(pool, bravo.bruno, secret::CLI).await?;
    let cli = bravo.bruno_cli.clone();
    let september = must(
        app,
        Method::POST,
        "/api/v1/token-sheets/submit",
        &cli,
        Payload::Json(json!({"token_entries": [session_entry("bravo-local-1", "bravo-session-1", "2026-09-10T09:00:00Z", "BRAVO-7", "Bravo secret summary")]})),
    )
    .await?;
    bravo.locked_entry = id_at(&september, "/token_entries/0/id")?;
    let bill = must(
        app,
        Method::POST,
        "/api/v1/bills",
        &boss,
        Payload::Json(json!({"period": "2026-09", "provider": "anthropic", "cost_source": "usage_at_api_rates", "net": "777.77", "currency": "USD"})),
    )
    .await?;
    bravo.bill = id_at(&bill, "/id")?;
    must(
        app,
        Method::POST,
        "/api/v1/periods/2026-09/close",
        &boss,
        Payload::Empty,
    )
    .await?;
    must(
        app,
        Method::POST,
        "/api/v1/periods/2026-09/adjustments",
        &boss,
        Payload::Json(json!({"amount": "-12.34", "reason": "Bravo credit note", "tags": {"project": "bravo-ledger"}})),
    )
    .await?;
    let link = must(
        app,
        Method::POST,
        "/api/v1/periods/2026-09/cost-sheet/links",
        &boss,
        Payload::Empty,
    )
    .await?;
    bravo.link = id_at(&link, "/id")?;
    text_at(&link, "/url")?
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .clone_into(&mut bravo.link_secret);
    must(
        app,
        Method::PATCH,
        "/api/v1/organization",
        &boss,
        Payload::Json(json!({"approval": true})),
    )
    .await?;
    Ok(())
}

async fn bravo_october(app: &Router, pool: &PgPool, bravo: &mut Bravo) -> TestResult<()> {
    let boss = bravo.boss.clone();
    let cli = bravo.bruno_cli.clone();
    let october = must(
        app,
        Method::POST,
        "/api/v1/token-sheets/submit",
        &cli,
        Payload::Json(json!({"token_entries": [session_entry("bravo-local-2", "bravo-session-2", "2026-10-05T09:00:00Z", "BRAVO-7", "Bravo rejected summary")]})),
    )
    .await?;
    bravo.rejected_entry = id_at(&october, "/token_entries/0/id")?;
    let draft = must(
        app,
        Method::POST,
        "/api/v1/token-entries",
        &cli,
        Payload::Json(session_entry(
            "bravo-local-3",
            "bravo-session-3",
            "2026-10-06T09:00:00Z",
            "BRAVO-7",
            "Bravo draft summary",
        )),
    )
    .await?;
    bravo.draft_entry = id_at(&draft, "/id")?;
    let service = must(
        app,
        Method::POST,
        "/api/v1/api-tokens",
        &boss,
        Payload::Json(json!({"kind": "service", "name": "Bravo bot"})),
    )
    .await?;
    bravo.service_token = id_at(&service, "/id")?;
    let submitted = must(
        app,
        Method::POST,
        "/api/v1/token-entries",
        &text_at(&service, "/token")?,
        Payload::Json(session_entry(
            "bravo-local-4",
            "bravo-session-4",
            "2026-10-07T09:00:00Z",
            "BRAVO-7",
            "Bravo submitted summary",
        )),
    )
    .await?;
    bravo.submitted_entry = id_at(&submitted, "/id")?;
    must(
        app,
        Method::POST,
        &format!("/api/v1/token-entries/{}/reject", bravo.rejected_entry),
        &boss,
        Payload::Json(json!({"reason": "Bravo rejection reason"})),
    )
    .await?;
    let read = must(
        app,
        Method::POST,
        "/api/v1/api-tokens",
        &boss,
        Payload::Json(json!({"kind": "read", "name": "Bravo reader"})),
    )
    .await?;
    bravo.read_token = id_at(&read, "/id")?;
    bravo.cli_session = sqlx::query_scalar(
        "SELECT id FROM sessions WHERE person_id = $1 AND client = 'local_tool'",
    )
    .bind(bravo.bruno)
    .fetch_one(pool)
    .await?;
    Ok(())
}

async fn bravo_settings(app: &Router, pool: &PgPool, bravo: &mut Bravo) -> TestResult<()> {
    let boss = bravo.boss.clone();
    let post = |path: &'static str, body: Value| {
        let boss = boss.clone();
        async move { must(app, Method::POST, path, &boss, Payload::Json(body)).await }
    };
    let cadence = must(
        app,
        Method::PUT,
        "/api/v1/cadences",
        &boss,
        Payload::Json(json!({"project": "bravo-ledger", "kind": "weekly"})),
    )
    .await?;
    bravo.cadence = id_at(&cadence, "/id")?;
    let budget = post(
        "/api/v1/token-budgets",
        json!({"name": "Bravo budget", "filter": {"project": "bravo-ledger"}, "every_month": true, "period": "2026-09", "amount": "999.00"}),
    )
    .await?;
    bravo.budget = id_at(&budget, "/id")?;
    let saved = post(
        "/api/v1/reports/saved",
        json!({"name": "Bravo saved report", "filter": "project:bravo-ledger", "by": "ticket"}),
    )
    .await?;
    bravo.saved_report = id_at(&saved, "/id")?;
    let connection = post(
        "/api/v1/bill-connections",
        json!({"provider": "openai", "key": "sk-bravo-secret-QZ7X"}),
    )
    .await?;
    bravo.bill_connection = id_at(&connection, "/id")?;
    let system = post(
        "/api/v1/ticket-systems",
        json!({"kind": "jira", "site": "https://bravo-holdings.atlassian.net", "token": "bravo-jira-token-WK3P", "email": "boss@bravo.example"}),
    )
    .await?;
    bravo.ticket_system = id_at(&system, "/id")?;
    must(
        app,
        Method::PUT,
        &format!("/api/v1/ticket-systems/{}/mappings", bravo.ticket_system),
        &boss,
        Payload::Json(json!([{"external": "BRAVOJ", "external_name": "Bravo Jira project", "project": "bravo-ledger"}])),
    )
    .await?;
    post("/api/v1/token-sheets/remind", json!({"period": "2026-10"})).await?;
    must(
        app,
        Method::PUT,
        "/api/v1/token-forecasts/settings",
        &boss,
        Payload::Json(json!({"enabled": true})),
    )
    .await?;
    let boss_id: Uuid =
        sqlx::query_scalar("SELECT id FROM people WHERE email = 'boss@bravo.example'")
            .fetch_one(pool)
            .await?;
    let made = forecast::create(
        pool,
        &Constant,
        bravo.organization_id,
        boss_id,
        &Scope::everything(),
        "BRAVO-7",
    )
    .await
    .map_err(|error| error.to_string())?;
    bravo.forecast = made.id;
    Ok(())
}

/// Every ID in Bravo's rows, read from the database itself.
async fn bravo_ids(pool: &PgPool, organization_id: Uuid) -> TestResult<Vec<String>> {
    let mut ids = vec![organization_id.to_string()];
    for table in [
        "people",
        "projects",
        "tickets",
        "token_entries",
        "api_tokens",
        "bills",
        "cadences",
        "token_budgets",
        "saved_reports",
        "cost_sheet_links",
        "bill_connections",
        "ticket_systems",
        "token_forecasts",
        "token_sheets",
        "adjustments",
    ] {
        let found: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT id::text FROM {table} WHERE organization_id = $1"
        )))
        .bind(organization_id)
        .fetch_all(pool)
        .await?;
        ids.extend(found);
    }
    for query in [
        "SELECT s.id::text FROM splits s JOIN token_entries e ON e.id = s.token_entry_id WHERE e.organization_id = $1",
        "SELECT s.id::text FROM sessions s JOIN people p ON p.id = s.person_id WHERE p.organization_id = $1",
        "SELECT r.id::text FROM bill_revisions r JOIN bills b ON b.id = r.bill_id WHERE b.organization_id = $1",
        "SELECT code FROM invites WHERE organization_id = $1",
        "SELECT slug FROM organizations WHERE id = $1",
    ] {
        let found: Vec<String> = sqlx::query_scalar(query)
            .bind(organization_id)
            .fetch_all(pool)
            .await?;
        ids.extend(found);
    }
    Ok(ids)
}

async fn bravo(app: &Router, pool: &PgPool) -> TestResult<Bravo> {
    let mut bravo = Bravo::default();
    bravo_people(app, pool, &mut bravo).await?;
    bravo_catalog(app, &mut bravo).await?;
    bravo_work(app, pool, &mut bravo).await?;
    bravo_october(app, pool, &mut bravo).await?;
    bravo_settings(app, pool, &mut bravo).await?;
    let mut secrets: Vec<String> = [
        "Bravo Holdings",
        "bravo.example",
        "Bruno Bravo",
        "Petra Bravo",
        "bravozone",
        "bravo-team",
        "bravo-client",
        "bravo-ledger",
        "bravo/ledger",
        "BRAVO-1",
        "BRAVO-7",
        "Bravo epic",
        "Bravo payroll migration",
        "Bravo secret description",
        "Bravo secret summary",
        "Bravo rejected summary",
        "Bravo draft summary",
        "Bravo submitted summary",
        "Bravo rejection reason",
        "Bravo credit note",
        "Bravo branch rule",
        "bravo-local-1",
        "bravo-local-2",
        "bravo-local-3",
        "bravo-local-4",
        "bravo-session-1",
        "bravo-session-2",
        "bravo-session-3",
        "bravo-session-4",
        "777.77",
        "12.34",
        "999.00",
        "Bravo budget",
        "Bravo saved report",
        "Bravo bot",
        "Bravo reader",
        "QZ7X",
        "WK3P",
        "bravo-holdings.atlassian.net",
        "BRAVOJ",
        "Bravo Jira project",
    ]
    .map(str::to_owned)
    .to_vec();
    secrets.push(bravo.link_secret.clone());
    secrets.extend(bravo_ids(pool, bravo.organization_id).await?);
    bravo.secrets = secrets;
    Ok(bravo)
}

// ---------------------------------------------------------------------------
// The cases: every route, called by Alpha with Bravo's IDs

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
    /// The checks in place of [`Self::checks`] once Alpha has written: for
    /// Alpha's own lists, which its writes fill.
    after: Option<Vec<(&'static str, Value)>>,
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

    fn after(mut self, pointer: &'static str, value: Value) -> Self {
        self.after
            .get_or_insert_with(Vec::new)
            .push((pointer, value));
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
        after: None,
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

/// Bravo's names Alpha stored as its own with its writes: its project's
/// repository, its token entries' tickets, sender IDs and sessions, its
/// filters. Only Alpha's raw export holds them all.
const ALPHA_STORED: [&str; 10] = [
    "bravo-ledger",
    "bravo/ledger",
    "BRAVO-7",
    "bravo-local-1",
    "bravo-local-3",
    "bravo-local-4",
    "bravo-session-1",
    "bravo-session-2",
    "bravo-session-3",
    "bravo-session-4",
];

const OK: &[u16] = &[200];
const CREATED: &[u16] = &[201];
const NO_CONTENT: &[u16] = &[204];
const BAD_REQUEST: &[u16] = &[400];
const NOT_FOUND: &[u16] = &[404];
const CONFLICT: &[u16] = &[409];
const ACCEPTED: &[u16] = &[202];

fn people_cases(alpha: &Alpha, b: &Bravo) -> Vec<Case> {
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
                    .expect(BAD_REQUEST),
            ],
        ),
    ]
}

fn member_cases(alpha: &Alpha, b: &Bravo) -> Vec<Case> {
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
            ],
        ),
        case(
            "PATCH",
            "/api/v1/members/{id}",
            vec![
                patch(format!("/api/v1/members/{}", b.bruno))
                    .json(json!({"name": "Taken over", "role": "controller"}))
                    .expect(NOT_FOUND),
                patch(format!("/api/v1/members/{}", alpha.person_id))
                    .json(json!({"tags": {"bravozone": "bravo-team"}}))
                    .expect(BAD_REQUEST),
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
                    .json(json!([{"client": "x"}]))
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
                // An invite code is the invitee's key; a wrong one opens nothing.
                get("/api/v1/invites/alpha-works-000000000000")
                    .by(Cred::Nobody)
                    .expect(NOT_FOUND),
                get(format!("/api/v1/invites/{}x", b.invite_code)).expect(NOT_FOUND),
            ],
        ),
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
                get("/api/v1/me/context")
                    .expect(OK)
                    .allow(&["bravo/ledger"]),
                get("/api/v1/me/context")
                    .by(Cred::Cli)
                    .expect(OK)
                    .allow(&["bravo/ledger"]),
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
                    .json(json!({"rules": [bravo_rule()]}))
                    .expect(BAD_REQUEST),
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
                    .json(json!({"approval": true}))
                    .expect(OK),
            ],
        ),
        case(
            "GET",
            "/api/v1/organization/member-limit",
            vec![
                get("/api/v1/organization/member-limit")
                    .expect(OK)
                    .check("/members", json!(1)),
            ],
        ),
        case(
            "GET",
            "/api/v1/organization/rules",
            vec![get("/api/v1/organization/rules").expect(OK)],
        ),
        case(
            "PUT",
            "/api/v1/organization/rules",
            vec![
                put("/api/v1/organization/rules")
                    .json(json!({"rules": [bravo_rule()]}))
                    .expect(BAD_REQUEST),
            ],
        ),
    ]
}

fn tag_cases() -> Vec<Case> {
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
            "GET",
            "/api/v1/tags/{name}/values",
            vec![
                get("/api/v1/tags/bravozone/values")
                    .expect(OK)
                    .check("", json!([])),
                get("/api/v1/tags/client/values")
                    .expect(OK)
                    .check("", json!([])),
                get("/api/v1/tags/person/values").expect(OK),
                get("/api/v1/tags/project/values").expect(OK),
                get("/api/v1/tags/ticket/values").expect(OK),
            ],
        ),
        case(
            "POST",
            "/api/v1/tags/{name}/merge",
            vec![
                post("/api/v1/tags/bravozone/merge")
                    .json(json!({"from": ["bravo-team"], "into": "alpha"}))
                    .expect(NOT_FOUND),
                // Alpha's own tag, with Bravo's values: nothing of Bravo's moves.
                post("/api/v1/tags/client/merge")
                    .json(json!({"from": ["bravo-client"], "into": "alpha-client"}))
                    .expect(OK)
                    .check("", json!([])),
            ],
        ),
        case(
            "DELETE",
            "/api/v1/tags/{name}",
            vec![
                delete("/api/v1/tags/bravozone").expect(NOT_FOUND),
                // Removing Alpha's own `client` leaves Bravo's `client` tags.
                delete("/api/v1/tags/client").expect(NO_CONTENT),
            ],
        ),
        case(
            "GET",
            "/api/v1/tag-map",
            vec![
                get("/api/v1/tag-map")
                    .expect(OK)
                    .check("/version", json!(1)),
            ],
        ),
        case(
            "PUT",
            "/api/v1/tag-map",
            vec![
                put("/api/v1/tag-map")
                    .json(json!({"cost_center": "bravozone", "ladder": []}))
                    .expect(BAD_REQUEST),
            ],
        ),
    ]
}

fn project_cases(b: &Bravo) -> Vec<Case> {
    vec![
        case(
            "GET",
            "/api/v1/projects",
            vec![
                // Alpha's project names Bravo's repository as its own.
                get("/api/v1/projects").expect(OK).allow(&["bravo/ledger"]),
                get("/api/v1/projects?archived=true")
                    .expect(OK)
                    .allow(&["bravo/ledger"]),
            ],
        ),
        case(
            "POST",
            "/api/v1/projects",
            vec![
                post("/api/v1/projects")
                    .json(json!({"name": "alpha-two", "repositories": ["bravo/ledger"]}))
                    .expect(CREATED),
            ],
        ),
        case(
            "PATCH",
            "/api/v1/projects/{id}",
            vec![
                patch(format!("/api/v1/projects/{}", b.project))
                    .json(json!({"name": "taken"}))
                    .expect(NOT_FOUND),
            ],
        ),
        case(
            "DELETE",
            "/api/v1/projects/{id}",
            vec![delete(format!("/api/v1/projects/{}", b.project)).expect(NOT_FOUND)],
        ),
        case(
            "GET",
            "/api/v1/clients",
            vec![get("/api/v1/clients").expect(OK).check("", json!([]))],
        ),
    ]
}

fn ticket_cases(alpha: &Alpha, b: &Bravo) -> Vec<Case> {
    let tickets = "/api/v1/tickets";
    vec![
        case(
            "GET",
            "/api/v1/tickets",
            vec![
                get(tickets).expect(OK),
                get(format!("{tickets}?project_id={}", b.project))
                    .expect(OK)
                    .check("", json!([])),
                get(format!("{tickets}?parent_id={}", b.epic))
                    .expect(OK)
                    .check("", json!([])),
                get(format!("{tickets}?q=BRAVO"))
                    .expect(OK)
                    .check("", json!([])),
            ],
        ),
        case(
            "POST",
            "/api/v1/tickets",
            vec![
                post(tickets)
                    .json(json!({"key": "ALPHA-2", "project_id": b.project}))
                    .expect(BAD_REQUEST),
                post(tickets)
                    .json(json!({"key": "ALPHA-3", "parent_id": b.ticket}))
                    .expect(BAD_REQUEST),
                post(tickets)
                    .json(json!({"key": "ALPHA-4", "assignee_id": b.bruno}))
                    .expect(BAD_REQUEST),
            ],
        ),
        case(
            "GET",
            "/api/v1/tickets/{id}",
            vec![get(format!("{tickets}/{}", b.ticket)).expect(NOT_FOUND)],
        ),
        case(
            "PATCH",
            "/api/v1/tickets/{id}",
            vec![
                patch(format!("{tickets}/{}", b.ticket))
                    .json(json!({"title": "taken"}))
                    .expect(NOT_FOUND),
                patch(format!("{tickets}/{}", alpha.ticket_id))
                    .json(json!({"parent_id": b.epic}))
                    .expect(BAD_REQUEST),
                patch(format!("{tickets}/{}", alpha.ticket_id))
                    .json(json!({"project_id": b.project}))
                    .expect(BAD_REQUEST),
                patch(format!("{tickets}/{}", alpha.ticket_id))
                    .json(json!({"assignee_id": b.bruno}))
                    .expect(BAD_REQUEST),
            ],
        ),
        case(
            "GET",
            "/api/v1/tickets/{key}/token-forecast",
            vec![get("/api/v1/tickets/BRAVO-7/token-forecast").expect(NOT_FOUND)],
        ),
        case(
            "POST",
            "/api/v1/tickets/{key}/token-forecast",
            vec![post("/api/v1/tickets/BRAVO-7/token-forecast").expect(NOT_FOUND)],
        ),
        case(
            "POST",
            "/api/v1/token-forecasts/{id}/accept",
            vec![
                post(format!("/api/v1/token-forecasts/{}/accept", b.forecast))
                    .json(json!({"amount": "30"}))
                    .expect(NOT_FOUND),
            ],
        ),
        case(
            "GET",
            "/api/v1/token-forecasts/settings",
            vec![get("/api/v1/token-forecasts/settings").expect(OK)],
        ),
        case(
            "PUT",
            "/api/v1/token-forecasts/settings",
            vec![
                put("/api/v1/token-forecasts/settings")
                    .json(json!({"enabled": true}))
                    .expect(OK),
            ],
        ),
    ]
}

fn entry_cases() -> Vec<Case> {
    let entries = "/api/v1/token-entries";
    let csv = "agent_harness,model,started_at,ended_at,tokens.input,tokens.output,reported_cost,summary,ticket\n\
               claude-code,claude-sonnet-5,2026-10-08T09:12:00Z,2026-10-08T09:40:00Z,41200,6100,0.42,Alpha CSV work,BRAVO-7\n";
    vec![
        case(
            "POST",
            "/api/v1/token-entries",
            vec![
                // Bravo's session and sender IDs, ticket and project name:
                // a new token entry of Alpha's, nothing of Bravo's touched.
                post(entries)
                    .by(Cred::Service)
                    .json(session_entry(
                        "bravo-local-4",
                        "bravo-session-4",
                        "2026-10-08T09:00:00Z",
                        "BRAVO-7",
                        "Alpha service work",
                    ))
                    .expect(CREATED),
                post(entries)
                    .json(session_entry(
                        "bravo-local-3",
                        "bravo-session-3",
                        "2026-10-08T11:00:00Z",
                        "BRAVO-7",
                        "Alpha manual work",
                    ))
                    .expect(CREATED),
            ],
        ),
        case(
            "POST",
            "/api/v1/token-entries/csv/preview",
            vec![
                post(format!("{entries}/csv/preview"))
                    .raw("text/csv", csv)
                    .expect(OK),
            ],
        ),
        case(
            "POST",
            "/api/v1/token-entries/csv",
            vec![
                post(format!("{entries}/csv"))
                    .raw("text/csv", csv)
                    .expect(CREATED),
            ],
        ),
    ]
}

fn decision_cases(b: &Bravo) -> Vec<Case> {
    let entries = "/api/v1/token-entries";
    let bravo_entries = [
        b.rejected_entry,
        b.draft_entry,
        b.submitted_entry,
        b.locked_entry,
    ];
    vec![
        case(
            "GET",
            "/api/v1/token-entries/approval",
            vec![
                // Alpha's token entries carry Bravo's ticket key as their own.
                get(format!("{entries}/approval"))
                    .expect(OK)
                    .allow(&["BRAVO-7"]),
                get(format!("{entries}/approval?format=csv"))
                    .expect(OK)
                    .allow(&["BRAVO-7"]),
            ],
        ),
        case(
            "POST",
            "/api/v1/token-entries/approve",
            vec![
                post(format!("{entries}/approve"))
                    .json(json!({"token_entry_ids": bravo_entries}))
                    .expect(OK)
                    .check("/approved", json!(0)),
            ],
        ),
        case(
            "POST",
            "/api/v1/token-entries/{id}/reject",
            bravo_entries
                .iter()
                .map(|id| {
                    post(format!("{entries}/{id}/reject"))
                        .json(json!({"reason": "not yours"}))
                        .expect(NOT_FOUND)
                })
                .collect(),
        ),
        case(
            "PUT",
            "/api/v1/token-entries/{id}/splits",
            bravo_entries
                .iter()
                .map(|id| {
                    put(format!("{entries}/{id}/splits"))
                        .json(json!({"splits": [{"share_percent": 50}, {"share_percent": 50}]}))
                        .expect(NOT_FOUND)
                })
                .collect(),
        ),
        case(
            "PATCH",
            "/api/v1/token-entries/{id}/tags",
            bravo_entries
                .iter()
                .map(|id| {
                    patch(format!("{entries}/{id}/tags"))
                        .json(json!({"tags": {"ticket": "ALPHA-1"}}))
                        .expect(NOT_FOUND)
                })
                .collect(),
        ),
    ]
}

fn telemetry_cases() -> Vec<Case> {
    let otlp =
        |signal: &str| post(format!("/api/v1/ingest/otlp/v1/{signal}")).by(Cred::Organization);
    vec![
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

fn sheet_cases(b: &Bravo) -> Vec<Case> {
    vec![
        case(
            "GET",
            "/api/v1/token-sheets/current",
            vec![
                get("/api/v1/token-sheets/current")
                    .expect(OK)
                    .allow(&["BRAVO-7"]),
                get("/api/v1/token-sheets/current")
                    .by(Cred::Cli)
                    .expect(OK)
                    .allow(&["BRAVO-7"]),
                get("/api/v1/token-sheets/current?format=csv")
                    .expect(OK)
                    .allow(&["BRAVO-7"]),
            ],
        ),
        case(
            "POST",
            "/api/v1/token-sheets/submit",
            vec![
                // The sender ID of Bravo's locked September token entry.
                post("/api/v1/token-sheets/submit")
                    .by(Cred::Cli)
                    .json(json!({"token_entries": [session_entry("bravo-local-1", "bravo-session-1", "2026-10-09T09:00:00Z", "BRAVO-7", "Alpha CLI work")]}))
                    .expect(OK),
            ],
        ),
        case(
            "GET",
            "/api/v1/token-sheets/late",
            vec![get("/api/v1/token-sheets/late").expect(OK), get("/api/v1/token-sheets/late?period=2026-10").expect(OK)],
        ),
        case("POST", "/api/v1/token-sheets/remind", vec![post("/api/v1/token-sheets/remind").json(json!({"period": "2026-10"})).expect(OK)]),
        case("GET", "/api/v1/cadences", vec![get("/api/v1/cadences").expect(OK).check("", json!([]))]),
        case(
            "PUT",
            "/api/v1/cadences",
            vec![put("/api/v1/cadences").json(json!({"project": "bravo-ledger", "kind": "weekly"})).expect(NOT_FOUND)],
        ),
        case("DELETE", "/api/v1/cadences/{id}", vec![delete(format!("/api/v1/cadences/{}", b.cadence)).expect(NOT_FOUND)]),
    ]
}

fn close_cases(b: &Bravo) -> Vec<Case> {
    let september = "/api/v1/periods/2026-09";
    vec![
        case(
            "GET",
            "/api/v1/periods",
            vec![
                get("/api/v1/periods").expect(OK),
                get("/api/v1/periods?format=csv").expect(OK),
            ],
        ),
        // Bravo closed September; Alpha's September is its own and open.
        case(
            "GET",
            "/api/v1/periods/{period}",
            vec![get(september).expect(OK).check("/closed", json!(false))],
        ),
        case(
            "POST",
            "/api/v1/periods/{period}/adjustments",
            vec![
                post(format!("{september}/adjustments"))
                    .json(json!({"amount": "1.00", "reason": "Alpha"}))
                    .expect(CONFLICT),
            ],
        ),
        case(
            "POST",
            "/api/v1/periods/{period}/close",
            vec![post("/api/v1/periods/2026-10/close").expect(CONFLICT)],
        ),
        case(
            "GET",
            "/api/v1/periods/{period}/cost-sheet",
            vec![
                get(format!("{september}/cost-sheet"))
                    .expect(OK)
                    .check("/closed", json!(false)),
                get(format!("{september}/cost-sheet?format=csv")).expect(OK),
            ],
        ),
        case(
            "GET",
            "/api/v1/periods/{period}/cost-sheet/links",
            vec![
                get(format!("{september}/cost-sheet/links"))
                    .expect(OK)
                    .check("", json!([])),
            ],
        ),
        case(
            "POST",
            "/api/v1/periods/{period}/cost-sheet/links",
            vec![post(format!("{september}/cost-sheet/links")).expect(CONFLICT)],
        ),
        case(
            "DELETE",
            "/api/v1/cost-sheet-links/{id}",
            vec![delete(format!("/api/v1/cost-sheet-links/{}", b.link)).expect(NOT_FOUND)],
        ),
        case(
            "GET",
            "/api/v1/shared/cost-sheets/{secret}",
            vec![
                // The link's secret is the key to it; a wrong one opens nothing.
                get(format!("/api/v1/shared/cost-sheets/{}", "0".repeat(64)))
                    .by(Cred::Nobody)
                    .expect(NOT_FOUND),
                get(format!("/api/v1/shared/cost-sheets/{}", "f".repeat(64))).expect(NOT_FOUND),
            ],
        ),
        case(
            "GET",
            "/api/v1/reconciliation",
            vec![
                get("/api/v1/reconciliation?period=2026-09").expect(OK),
                get("/api/v1/reconciliation?period=2026-09&format=csv").expect(OK),
            ],
        ),
    ]
}

fn bill_cases(b: &Bravo) -> Vec<Case> {
    let csv =
        "period,cost_source,gross,discount,credit,net,currency\n2026-10,api_key,1,0,0,1,USD\n";
    let bill = json!({"period": "2026-10", "provider": "anthropic", "cost_source": "usage_at_api_rates", "net": "1.00", "currency": "USD"});
    vec![
        case(
            "GET",
            "/api/v1/bills",
            vec![
                get("/api/v1/bills?period=2026-09")
                    .expect(OK)
                    .check("", json!([])),
                get("/api/v1/bills?format=csv").expect(OK),
            ],
        ),
        case(
            "POST",
            "/api/v1/bills",
            vec![
                post("/api/v1/bills").json(bill.clone()).expect(CREATED),
                post("/api/v1/bills")
                    .by(Cred::Service)
                    .json(bill)
                    .expect(OK),
            ],
        ),
        case(
            "GET",
            "/api/v1/bills/{id}",
            vec![
                get(format!("/api/v1/bills/{}", b.bill)).expect(NOT_FOUND),
                get(format!("/api/v1/bills/{}?format=csv", b.bill)).expect(NOT_FOUND),
            ],
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
            vec![
                post("/api/v1/bills/csv")
                    .raw("text/csv", csv)
                    .expect(CREATED),
            ],
        ),
        case(
            "GET",
            "/api/v1/bill-connections",
            vec![
                get("/api/v1/bill-connections")
                    .expect(OK)
                    .check("", json!([]))
                    .after("/0/key_hint", json!("…A1B2"))
                    .after("/1", Value::Null),
            ],
        ),
        case(
            "POST",
            "/api/v1/bill-connections",
            vec![
                post("/api/v1/bill-connections")
                    .json(json!({"provider": "openai", "key": "sk-alpha-key-A1B2"}))
                    .expect(CREATED),
            ],
        ),
        case(
            "DELETE",
            "/api/v1/bill-connections/{id}",
            vec![
                delete(format!("/api/v1/bill-connections/{}", b.bill_connection)).expect(NOT_FOUND),
            ],
        ),
        case(
            "POST",
            "/api/v1/bill-connections/{id}/pull",
            vec![
                post(format!(
                    "/api/v1/bill-connections/{}/pull",
                    b.bill_connection
                ))
                .expect(NOT_FOUND),
            ],
        ),
    ]
}

fn integration_cases(b: &Bravo) -> Vec<Case> {
    let system = format!("/api/v1/ticket-systems/{}", b.ticket_system);
    vec![
        case("GET", "/api/v1/ticket-systems", vec![get("/api/v1/ticket-systems").expect(OK).check("", json!([])).after("/0/site", json!("https://alpha-works.atlassian.net")).after("/1", Value::Null)]),
        case(
            "POST",
            "/api/v1/ticket-systems",
            vec![post("/api/v1/ticket-systems")
                .json(json!({"kind": "jira", "site": "https://alpha-works.atlassian.net", "token": "alpha-jira-C3D4", "email": "alice@alpha.example"}))
                .expect(CREATED)],
        ),
        case("DELETE", "/api/v1/ticket-systems/{id}", vec![delete(system.clone()).expect(NOT_FOUND)]),
        case(
            "PUT",
            "/api/v1/ticket-systems/{id}/mappings",
            vec![put(format!("{system}/mappings")).json(json!([{"external": "ALPHA", "project": "alpha-app"}])).expect(NOT_FOUND)],
        ),
        case("GET", "/api/v1/ticket-systems/{id}/projects", vec![get(format!("{system}/projects")).expect(NOT_FOUND)]),
        case("POST", "/api/v1/ticket-systems/{id}/sync", vec![post(format!("{system}/sync")).expect(NOT_FOUND)]),
    ]
}

fn report_cases(b: &Bravo) -> Vec<Case> {
    let saved = format!("/api/v1/reports/saved/{}", b.saved_report);
    let entries = "/api/v1/reports/token-entries";
    vec![
        case(
            "GET",
            "/api/v1/reports/cost",
            vec![
                get("/api/v1/reports/cost?period=2026-09&by=project").expect(OK),
                get("/api/v1/reports/cost?period=2026-10&by=ticket&filter=project:bravo-ledger").expect(OK),
                get("/api/v1/reports/cost?period=2026-10&by=bravozone&format=csv").expect(OK),
            ],
        ),
        case(
            "GET",
            "/api/v1/reports/agent-harness",
            vec![get("/api/v1/reports/agent-harness?period=2026-09").expect(OK), get("/api/v1/reports/agent-harness?period=2026-10").expect(OK)],
        ),
        case(
            "GET",
            "/api/v1/reports/outliers",
            vec![get("/api/v1/reports/outliers?period=2026-09").expect(OK), get("/api/v1/reports/outliers?period=2026-10").expect(OK)],
        ),
        case("GET", "/api/v1/reports/saved", vec![get("/api/v1/reports/saved").expect(OK).check("", json!([])).after("/0/name", json!("Alpha copy")).after("/1", Value::Null).allow(&["bravo-ledger"])]),
        case(
            "POST",
            "/api/v1/reports/saved",
            vec![post("/api/v1/reports/saved").json(json!({"name": "Alpha copy", "filter": "project:bravo-ledger", "by": "ticket"})).expect(CREATED)],
        ),
        case("GET", "/api/v1/reports/saved/{id}", vec![get(format!("{saved}?period=2026-09")).expect(NOT_FOUND)]),
        case("DELETE", "/api/v1/reports/saved/{id}", vec![delete(saved).expect(NOT_FOUND)]),
        case(
            "GET",
            "/api/v1/reports/token-entries",
            vec![
                get(format!("{entries}?person={}", b.bruno)).expect(OK).check("/total", json!(0)),
                get(format!("{entries}?filter=project:bravo-ledger")).expect(OK).check("/total", json!(0)),
                get(format!("{entries}?status=submitted,approved,rejected,locked")).expect(OK).allow(&["BRAVO-7"]),
                get(format!("{entries}?format=csv")).expect(OK).allow(&["BRAVO-7"]),
            ],
        ),
        case(
            "GET",
            "/api/v1/reports/token-budgets",
            vec![get("/api/v1/reports/token-budgets?period=2026-10").expect(OK).allow(&["bravo-ledger"]), get("/api/v1/reports/token-budgets?period=2026-09").expect(OK).allow(&["bravo-ledger"])],
        ),
        case(
            "GET",
            "/api/v1/reports/token-budgets/{id}",
            vec![get(format!("/api/v1/reports/token-budgets/{}?period=2026-10", b.budget)).expect(NOT_FOUND)],
        ),
        case("GET", "/api/v1/token-budgets", vec![get("/api/v1/token-budgets").expect(OK).check("", json!([])).after("/0/name", json!("Alpha watch")).after("/1", Value::Null).allow(&["bravo-ledger"]), get("/api/v1/token-budgets?format=csv").expect(OK).allow(&["bravo-ledger"])]),
        case(
            "POST",
            "/api/v1/token-budgets",
            vec![post("/api/v1/token-budgets")
                .json(json!({"name": "Alpha watch", "filter": {"project": "bravo-ledger"}, "every_month": true, "period": "2026-09", "amount": "5.00"}))
                .expect(CREATED)],
        ),
        case(
            "PUT",
            "/api/v1/token-budgets/{id}",
            vec![put(format!("/api/v1/token-budgets/{}", b.budget)).json(json!({"every_month": true, "period": "2026-09", "amount": "1.00"})).expect(NOT_FOUND)],
        ),
        case("DELETE", "/api/v1/token-budgets/{id}", vec![delete(format!("/api/v1/token-budgets/{}", b.budget)).expect(NOT_FOUND)]),
        case("GET", "/api/v1/exports/raw", vec![get("/api/v1/exports/raw").expect(OK).allow(&ALPHA_STORED)]),
    ]
}

/// The routes anyone may call, called with Bravo's addresses and names.
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

/// The MCP tools Alpha calls; `every_mcp_tool_is_called` keeps this list
/// complete.
const MCP_TOOLS: [&str; 5] = ["organization", "members", "projects", "clients", "tickets"];

fn mcp_cases() -> Vec<Case> {
    let mcp = "/api/v1/mcp";
    let mut asks = vec![
        post(mcp)
            .json(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}))
            .expect(OK),
        post(mcp)
            .json(json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}))
            .expect(OK),
        post(mcp)
            .json(mcp_call("tickets", &json!({"query": "BRAVO"})))
            .expect(OK)
            .check("/result/structuredContent/result", json!([])),
        post(mcp)
            .json(mcp_call("tickets", &json!({"project": "bravo-ledger"})))
            .expect(OK)
            .check("/error/code", json!(-32602)),
        post(mcp)
            .json(mcp_call("clients", &json!({})))
            .expect(OK)
            .check("/result/structuredContent/result", json!({})),
    ];
    for tool in MCP_TOOLS {
        // Alpha's project names Bravo's repository as its own.
        let stored: &[&str] = if tool == "projects" {
            &["bravo/ledger"]
        } else {
            &[]
        };
        asks.push(
            post(mcp)
                .json(mcp_call(tool, &json!({})))
                .expect(OK)
                .allow(stored),
        );
        asks.push(
            post(mcp)
                .by(Cred::Read)
                .json(mcp_call(tool, &json!({})))
                .expect(OK)
                .allow(stored),
        );
    }
    vec![case("POST", "/api/v1/mcp", asks)]
}

fn all_cases(alpha: &Alpha, bravo: &Bravo) -> Vec<Case> {
    let mut cases = people_cases(alpha, bravo);
    cases.extend(member_cases(alpha, bravo));
    cases.extend(organization_cases());
    cases.extend(tag_cases());
    cases.extend(project_cases(bravo));
    cases.extend(ticket_cases(alpha, bravo));
    cases.extend(close_cases(bravo));
    cases.extend(bill_cases(bravo));
    cases.extend(integration_cases(bravo));
    cases.extend(report_cases(bravo));
    cases.extend(mcp_cases());
    // Alpha's token entries reuse Bravo's sender IDs and ticket keys, which
    // then are Alpha's own too; reads before them see none of that.
    cases.extend(entry_cases());
    cases.extend(decision_cases(bravo));
    cases.extend(telemetry_cases());
    cases.extend(sheet_cases(bravo));
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

/// Bravo's names and IDs in `answer`, but for those `request` itself sent.
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
const ALPHA_AGGREGATES: [&str; 22] = [
    "/api/v1/reports/cost?period=2026-09&by=project",
    "/api/v1/reports/cost?period=2026-10&by=project",
    "/api/v1/reports/cost?period=2026-10&by=ticket",
    "/api/v1/reports/cost?period=2026-10&by=client",
    "/api/v1/reports/agent-harness?period=2026-09",
    "/api/v1/reports/agent-harness?period=2026-10",
    "/api/v1/reports/outliers?period=2026-10",
    "/api/v1/reports/token-budgets?period=2026-10",
    "/api/v1/reports/token-entries?status=submitted,approved,rejected,locked",
    "/api/v1/reconciliation?period=2026-09",
    "/api/v1/reconciliation?period=2026-10",
    "/api/v1/periods",
    "/api/v1/periods/2026-09/cost-sheet",
    "/api/v1/token-entries/approval",
    "/api/v1/token-sheets/current",
    "/api/v1/token-sheets/late?period=2026-10",
    "/api/v1/organization/member-limit",
    "/api/v1/me/context",
    "/api/v1/clients",
    "/api/v1/tags/client/values",
    "/api/v1/tickets/ALPHA-1/token-forecast",
    "/api/v1/exports/raw",
];

/// Alpha's counts and totals, read as Alpha.
async fn alpha_totals(app: &Router, alpha: &Alpha) -> TestResult<Vec<String>> {
    let mut totals = Vec::new();
    for path in ALPHA_AGGREGATES {
        let answer = call(app, Method::GET, path, &alpha.session, &Payload::Empty).await?;
        // The export's own time is no total.
        let readable = answer.readable()?;
        let kept: Vec<&str> = readable
            .lines()
            .filter(|line| !line.contains("\"exported_at\""))
            .collect();
        totals.push(format!("{path}: {} {}", answer.status, kept.join("\n")));
    }
    Ok(totals)
}

/// Bravo's own view of its data, read through the API as Bravo.
async fn bravo_view(app: &Router, bravo: &Bravo) -> TestResult<Vec<String>> {
    let boss = bravo.boss.as_str();
    let mut view = Vec::new();
    for path in [
        "/api/v1/organization".to_owned(),
        "/api/v1/organization/rules".to_owned(),
        "/api/v1/members".to_owned(),
        "/api/v1/api-tokens".to_owned(),
        "/api/v1/tags".to_owned(),
        "/api/v1/tag-map".to_owned(),
        "/api/v1/tags/bravozone/values".to_owned(),
        "/api/v1/tags/client/values".to_owned(),
        "/api/v1/projects?archived=true".to_owned(),
        "/api/v1/clients".to_owned(),
        "/api/v1/tickets?limit=500".to_owned(),
        format!("/api/v1/tickets/{}", bravo.ticket),
        "/api/v1/cadences".to_owned(),
        "/api/v1/token-budgets".to_owned(),
        format!(
            "/api/v1/reports/token-budgets/{}?period=2026-10",
            bravo.budget
        ),
        "/api/v1/reports/saved".to_owned(),
        "/api/v1/bill-connections".to_owned(),
        "/api/v1/ticket-systems".to_owned(),
        "/api/v1/bills".to_owned(),
        format!("/api/v1/bills/{}", bravo.bill),
        "/api/v1/periods".to_owned(),
        "/api/v1/periods/2026-09/cost-sheet".to_owned(),
        "/api/v1/periods/2026-09/cost-sheet/links".to_owned(),
        "/api/v1/reconciliation?period=2026-09".to_owned(),
        "/api/v1/reports/token-entries?status=submitted,approved,rejected,locked".to_owned(),
        "/api/v1/token-entries/approval".to_owned(),
        "/api/v1/tickets/BRAVO-7/token-forecast".to_owned(),
        "/api/v1/token-sheets/late".to_owned(),
    ] {
        let answer = call(app, Method::GET, &path, boss, &Payload::Empty).await?;
        view.push(format!("{path}: {} {}", answer.status, answer.text()));
    }
    for path in [
        "/api/v1/token-sheets/current",
        "/api/v1/me/rules",
        "/api/v1/me/context",
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
            answer.text()
        ));
    }
    let shared = format!("/api/v1/shared/cost-sheets/{}", bravo.link_secret);
    let answer = call(app, Method::GET, &shared, "", &Payload::Empty).await?;
    view.push(format!(
        "shared cost sheet: {} {}",
        answer.status,
        answer.text()
    ));
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
            "SELECT s::text FROM splits s JOIN token_entries e ON e.id = s.token_entry_id WHERE e.organization_id = $1 ORDER BY 1",
            "SELECT r::text FROM bill_revisions r JOIN bills b ON b.id = r.bill_id WHERE b.organization_id = $1 ORDER BY 1",
            "SELECT d::text FROM token_entry_decisions d JOIN token_entries e ON e.id = d.token_entry_id WHERE e.organization_id = $1 ORDER BY 1",
            "SELECT x::text FROM token_sheet_entries x JOIN token_sheets s ON s.id = x.token_sheet_id WHERE s.organization_id = $1 ORDER BY 1",
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
    // Every read once more, now that Alpha has written with Bravo's IDs.
    let reads: Vec<Case> = all_cases(&alpha, &bravo)
        .into_iter()
        .map(|mut case| {
            case.asks
                .retain(|ask| ask.method == Method::GET && ask.cred != Cred::Nobody);
            for ask in &mut case.asks {
                if let Some(after) = ask.after.take() {
                    ask.checks = after;
                }
            }
            case
        })
        .collect();
    run(&tenants, &reads, &mut findings).await.unwrap();
    assert!(findings.calls > 200, "only {} calls", findings.calls);
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
    let bravos_cost = "/api/v1/reports/cost?period=2026-10&by=project";
    let before = call(&app, Method::GET, bravos_cost, &bravo.boss, &Payload::Empty)
        .await
        .unwrap();
    bravo_grows(&app, &bravo).await.unwrap();
    let after = call(&app, Method::GET, bravos_cost, &bravo.boss, &Payload::Empty)
        .await
        .unwrap();
    assert_ne!(before.json(), after.json(), "Bravo's own cost did not grow");
    let changed = differences(&with_bravo, &alpha_totals(&app, &alpha).await.unwrap());
    assert!(
        changed.is_empty(),
        "Alpha's totals counted Bravo's usage:\n{}",
        changed.join("\n\n")
    );
}

/// One more approved token entry for Bravo, in October like Alpha's.
async fn bravo_grows(app: &Router, bravo: &Bravo) -> TestResult<()> {
    let service = must(
        app,
        Method::POST,
        "/api/v1/api-tokens",
        &bravo.boss,
        Payload::Json(json!({"kind": "service", "name": "Bravo second bot"})),
    )
    .await?;
    let entry = must(
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
    must(
        app,
        Method::POST,
        "/api/v1/token-entries/approve",
        &bravo.boss,
        Payload::Json(json!({"token_entry_ids": [id_at(&entry, "/id")?]})),
    )
    .await?;
    Ok(())
}

/// The model calls stored from telemetry with Bravo's session ID, and for
/// which organization.
async fn telemetry_with_bravos_session_id(pool: &PgPool) -> TestResult<Vec<(Uuid, String)>> {
    Ok(sqlx::query_as(
        "SELECT organization_id, model FROM messages
         WHERE session_id = 'bravo-session-2' AND from_telemetry ORDER BY model",
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
        "/api/v1/tickets".to_owned(),
        format!("/api/v1/tickets/{}", bravo.ticket),
        "/api/v1/reports/token-entries?status=submitted,approved,rejected,locked".to_owned(),
        "/api/v1/exports/raw".to_owned(),
        "/api/v1/members".to_owned(),
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
    assert_eq!(documented.len(), 111, "{documented:?}");
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
    // Alpha names Bravo's project in a token budget and a saved report.
    let budget = must(
        &app,
        Method::POST,
        "/api/v1/token-budgets",
        session,
        Payload::Json(
            json!({"filter": {"project": "bravo-ledger"}, "every_month": true, "period": "2026-09", "amount": "5.00"}),
        ),
    )
    .await
    .unwrap();
    let saved = must(
        &app,
        Method::POST,
        "/api/v1/reports/saved",
        session,
        Payload::Json(
            json!({"name": "Alpha copy", "filter": "project:bravo-ledger", "by": "ticket"}),
        ),
    )
    .await
    .unwrap();
    for period in ["2026-09", "2026-10"] {
        let detail = format!(
            "/api/v1/reports/token-budgets/{}?period={period}",
            budget["id"].as_str().unwrap()
        );
        let answer = call(&app, Method::GET, &detail, session, &Payload::Empty)
            .await
            .unwrap();
        assert_eq!(answer.status, StatusCode::OK, "{}", answer.text());
        assert_eq!(
            answer.json()["token_entries"],
            json!([]),
            "{}",
            answer.text()
        );
        let run = format!(
            "/api/v1/reports/saved/{}?period={period}",
            saved["id"].as_str().unwrap()
        );
        let answer = call(&app, Method::GET, &run, session, &Payload::Empty)
            .await
            .unwrap();
        assert_eq!(answer.status, StatusCode::OK, "{}", answer.text());
        for secret in ["BRAVO-7", "777.77", &bravo.ticket.to_string()] {
            assert!(
                !answer.text().contains(secret),
                "{secret} in {}",
                answer.text()
            );
        }
    }
}
