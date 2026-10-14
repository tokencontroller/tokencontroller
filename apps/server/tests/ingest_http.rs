//! The doors over HTTP, against a real Postgres database: the token entries
//! endpoint with each kind of token, CSV import and the OpenTelemetry
//! receiver with a recorded Claude Code export.

use std::io::Write as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use sqlx::PgPool;
use tokencontroller_core::Extension;
use tokencontroller_core::extension::TelemetryBatch;
use tokencontroller_server::sign_in::secret;
use tokencontroller_server::{AppState, Config, router};
use tower::ServiceExt as _;
use uuid::Uuid;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

const KEY: &str = "0123456789abcdef0123456789abcdef";
const CLAUDE_CODE_LOGS: &str = include_str!("fixtures/otlp/claude_code_logs.json");

/// Counts the telemetry batches handed to the extension.
#[derive(Debug, Default)]
struct Forwarder {
    batches: AtomicUsize,
}

impl Extension for Forwarder {
    fn forward_telemetry(&self, batch: &TelemetryBatch<'_>) {
        if !batch.body.is_empty() {
            self.batches.fetch_add(1, Ordering::SeqCst);
        }
    }
}

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

struct Kestrel {
    app: Router,
    forwarder: Arc<Forwarder>,
    organization_id: Uuid,
    session: String,
    service: String,
    organization_token: String,
}

async fn kestrel(pool: &PgPool) -> TestResult<Kestrel> {
    let forwarder = Arc::new(Forwarder::default());
    let app = router(AppState::new(pool.clone(), config(), forwarder.clone()));
    let mut conn = pool.acquire().await?;
    let created = tokencontroller_server::organizations::create(
        &mut conn,
        "Kestrel Digital",
        "ana.silva@acme.example",
    )
    .await
    .map_err(|error| error.to_string())?;
    let session = secret::generate(secret::SESSION, KEY)?;
    sqlx::query("INSERT INTO sessions (token_hash, person_id, client, expires_at) VALUES ($1, $2, 'browser', now() + interval '1 day')")
        .bind(&session.hash)
        .bind(created.person_id)
        .execute(pool)
        .await?;
    let token = |prefix: &str, kind: &str| -> TestResult<(String, Vec<u8>, String)> {
        let secret = secret::generate(prefix, KEY)?;
        Ok((secret.token.clone(), secret.hash.clone(), kind.to_owned()))
    };
    let mut tokens = Vec::new();
    for (prefix, kind) in [
        (secret::SERVICE, "service"),
        (secret::ORGANIZATION, "organization"),
    ] {
        let (plain, hash, kind) = token(prefix, kind)?;
        sqlx::query("INSERT INTO api_tokens (organization_id, kind, name, person_id, token_hash, shown_prefix) VALUES ($1, $2, 'test', $3, $4, 'tc_')")
            .bind(created.organization_id)
            .bind(&kind)
            .bind(created.person_id)
            .bind(&hash)
            .execute(pool)
            .await?;
        tokens.push(plain);
    }
    let organization_token = tokens.pop().ok_or("no organization token")?;
    let service = tokens.pop().ok_or("no service token")?;
    Ok(Kestrel {
        app,
        forwarder,
        organization_id: created.organization_id,
        session: session.token,
        service,
        organization_token,
    })
}

struct Answer {
    status: StatusCode,
    content_type: String,
    json: Value,
}

async fn send(
    app: &Router,
    path: &str,
    token: &str,
    content_type: &str,
    body: Vec<u8>,
    gzip: bool,
) -> TestResult<Answer> {
    let mut request = Request::builder()
        .method(Method::POST)
        .uri(path)
        .header(header::CONTENT_TYPE, content_type);
    request = if token.starts_with(secret::SESSION) {
        request.header(header::COOKIE, format!("tc_session={token}"))
    } else {
        request.header(header::AUTHORIZATION, format!("Bearer {token}"))
    };
    let body = if gzip {
        request = request.header(header::CONTENT_ENCODING, "gzip");
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(&body)?;
        encoder.finish()?
    } else {
        body
    };
    let response = app.clone().oneshot(request.body(Body::from(body))?).await?;
    let status = response.status();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let bytes = response.into_body().collect().await?.to_bytes();
    Ok(Answer {
        status,
        content_type,
        json: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    })
}

fn support_entry() -> Value {
    json!({
        "agent_harness": "support-desk",
        "agent_name": "email answers",
        "model": "claude-sonnet-5",
        "started_at": "2026-09-18T09:12:00Z",
        "ended_at": "2026-09-18T09:14:10Z",
        "tokens": { "input": 4120, "output": 610, "cache_read": 18200 },
        "reported_cost": 0.031,
        "summary": "Answered refund question for order 1182",
        "project": "support",
        "local_id": "support-desk:1182"
    })
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_service_token_reports_automatic_token_entries(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    let body = serde_json::to_vec(&support_entry()).unwrap();
    let answer = send(
        &kestrel.app,
        "/api/v1/token-entries",
        &kestrel.service,
        "application/json",
        body,
        false,
    )
    .await
    .unwrap();
    assert_eq!(answer.status, StatusCode::CREATED, "{}", answer.json);
    assert_eq!(answer.json["status"], "submitted");
    assert_eq!(answer.json["list_value"], "0.03");
    assert_eq!(answer.json["local_id"], "support-desk:1182");

    // A list gets a list back.
    let body = serde_json::to_vec(&json!([support_entry()])).unwrap();
    let again = send(
        &kestrel.app,
        "/api/v1/token-entries",
        &kestrel.service,
        "application/json",
        body,
        false,
    )
    .await
    .unwrap();
    assert_eq!(again.status, StatusCode::CREATED);
    assert_eq!(again.json[0]["id"], answer.json["id"]);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_signed_in_person_adds_drafts_and_the_organization_token_cannot(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    let body = serde_json::to_vec(&support_entry()).unwrap();
    let manual = send(
        &kestrel.app,
        "/api/v1/token-entries",
        &kestrel.session,
        "application/json",
        body.clone(),
        false,
    )
    .await
    .unwrap();
    assert_eq!(manual.status, StatusCode::CREATED, "{}", manual.json);
    assert_eq!(manual.json["status"], "draft");

    let refused = send(
        &kestrel.app,
        "/api/v1/token-entries",
        &kestrel.organization_token,
        "application/json",
        body,
        false,
    )
    .await
    .unwrap();
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_broken_token_entry_is_refused_with_its_problem(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    let mut entry = support_entry();
    entry["ended_at"] = json!("2026-09-18T09:00:00Z");
    let body = serde_json::to_vec(&entry).unwrap();
    let answer = send(
        &kestrel.app,
        "/api/v1/token-entries",
        &kestrel.service,
        "application/json",
        body,
        false,
    )
    .await
    .unwrap();
    assert_eq!(answer.status, StatusCode::BAD_REQUEST);
    assert!(
        answer.json["message"]
            .as_str()
            .unwrap()
            .contains("token entry 0")
    );
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn claude_code_telemetry_builds_a_token_entry_once(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    for gzip in [false, true] {
        let answer = send(
            &kestrel.app,
            "/api/v1/ingest/otlp/v1/logs",
            &kestrel.organization_token,
            "application/json",
            CLAUDE_CODE_LOGS.as_bytes().to_vec(),
            gzip,
        )
        .await
        .unwrap();
        assert_eq!(answer.status, StatusCode::OK);
        assert_eq!(answer.content_type, "application/json");
        // Two events carried prompt text and were dropped.
        assert_eq!(
            answer.json["partialSuccess"]["rejectedLogRecords"],
            json!(2)
        );
    }
    assert_eq!(kestrel.forwarder.batches.load(Ordering::SeqCst), 2);

    let rows: Vec<(String, i64, Option<Uuid>)> = sqlx::query_as(
        "SELECT door, (SELECT COUNT(*) FROM messages m WHERE m.token_entry_id = e.id), agent_manager_id
         FROM token_entries e WHERE organization_id = $1",
    )
    .bind(kestrel.organization_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].0, "telemetry");
    assert_eq!(rows[0].1, 2);
    assert!(rows[0].2.is_some(), "the email names the member");
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn gemini_cli_with_default_settings_is_counted_without_its_text(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    // Gemini CLI logs prompts by default, so its response event carries the
    // response text next to the token counts.
    let text = |key: &str, value: &str| json!({ "key": key, "value": { "stringValue": value } });
    let int = |key: &str, value: i64| json!({ "key": key, "value": { "intValue": value } });
    let export = json!({ "resourceLogs": [{
        "resource": { "attributes": [text("service.name", "gemini-cli")] },
        "scopeLogs": [{ "scope": { "name": "gemini-cli" }, "logRecords": [{
            "timeUnixNano": "1791199800250000000",
            "body": { "stringValue": "API response from gemini-2.5-pro. Status: 200. Duration: 2310ms." },
            "attributes": [
                text("event.name", "gemini_cli.api_response"),
                text("session.id", "c0ffee00-1111-4222-8333-444455556666"),
                text("model", "gemini-2.5-pro"),
                int("input_token_count", 9000),
                int("output_token_count", 420),
                text("response_text", "Fixed the build."),
            ],
        }] }],
    }] });
    let answer = send(
        &kestrel.app,
        "/api/v1/ingest/otlp/v1/logs",
        &kestrel.organization_token,
        "application/json",
        serde_json::to_vec(&export).unwrap(),
        false,
    )
    .await
    .unwrap();
    assert_eq!(answer.status, StatusCode::OK);
    // Nothing rejected, and a warning to turn prompt logging off.
    let partial = &answer.json["partialSuccess"];
    assert_eq!(partial["rejectedLogRecords"], json!(0));
    assert!(
        partial["errorMessage"]
            .as_str()
            .unwrap()
            .contains("1 model calls carried prompt text")
    );

    let rows: Vec<(i64, i64, String)> = sqlx::query_as(
        "SELECT m.input_tokens, m.output_tokens, e.telemetry_attributes::text
         FROM messages m JOIN token_entries e ON e.id = m.token_entry_id
         WHERE m.organization_id = $1",
    )
    .bind(kestrel.organization_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!((rows[0].0, rows[0].1), (9000, 420));
    assert!(!rows[0].2.contains("Fixed the build"), "{rows:?}");
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_receiver_takes_only_the_organization_token_and_known_formats(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    let body = CLAUDE_CODE_LOGS.as_bytes().to_vec();
    let service = send(
        &kestrel.app,
        "/api/v1/ingest/otlp/v1/logs",
        &kestrel.service,
        "application/json",
        body.clone(),
        false,
    )
    .await
    .unwrap();
    assert_eq!(service.status, StatusCode::FORBIDDEN);
    let unknown = send(
        &kestrel.app,
        "/api/v1/ingest/otlp/v1/logs",
        &kestrel.organization_token,
        "text/plain",
        body,
        false,
    )
    .await
    .unwrap();
    assert_eq!(unknown.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    let metrics = send(
        &kestrel.app,
        "/api/v1/ingest/otlp/v1/metrics",
        &kestrel.organization_token,
        "application/json",
        include_bytes!("fixtures/otlp/claude_code_metrics.json").to_vec(),
        false,
    )
    .await
    .unwrap();
    assert_eq!(metrics.status, StatusCode::OK);
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0, "metrics are not counted");
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_csv_file_is_previewed_then_imported_as_drafts(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    let good = "agent_harness,model,started_at,ended_at,tokens.input,tokens.output,reported_cost,summary,ticket\n\
                claude-code,claude-sonnet-5,2026-09-18T09:12:00Z,2026-09-18T09:40:00Z,41200,6100,0.42,Fixed the cart total,KD-214\n";
    let bad = format!(
        "{good}claude-code,claude-sonnet-5,2026-09-18T10:00:00Z,2026-09-18T09:00:00Z,1,1,,,\n"
    );

    let preview = send(
        &kestrel.app,
        "/api/v1/token-entries/csv/preview",
        &kestrel.session,
        "text/csv",
        bad.clone().into_bytes(),
        false,
    )
    .await
    .unwrap();
    assert_eq!(preview.status, StatusCode::OK, "{}", preview.json);
    assert_eq!(preview.json["ready"], 1);
    assert_eq!(preview.json["problems"][0]["line"], 3);

    let refused = send(
        &kestrel.app,
        "/api/v1/token-entries/csv",
        &kestrel.session,
        "text/csv",
        bad.into_bytes(),
        false,
    )
    .await
    .unwrap();
    assert_eq!(refused.status, StatusCode::BAD_REQUEST);

    let imported = send(
        &kestrel.app,
        "/api/v1/token-entries/csv",
        &kestrel.session,
        "text/csv",
        good.as_bytes().to_vec(),
        false,
    )
    .await
    .unwrap();
    assert_eq!(imported.status, StatusCode::CREATED, "{}", imported.json);
    assert_eq!(imported.json["token_entries"][0]["status"], "draft");
}
