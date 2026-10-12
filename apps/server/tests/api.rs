//! The service's HTTP API against a real Postgres database.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt as _;
use sqlx::PgPool;
use tokencontroller_core::NoExtension;
use tokencontroller_server::{AppState, Config, router};
use tower::ServiceExt as _;

fn test_config() -> Config {
    Config {
        database_url: "postgres://unused".to_owned(),
        public_url: "http://localhost:8080".to_owned(),
        secret_key: "0123456789abcdef0123456789abcdef".to_owned(),
        smtp_url: None,
        smtp_from: None,
        prices_refresh: false,
        customer_relations: tokencontroller_server::customer_relations::Settings::default(),
        stripe: None,
    }
}

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

async fn get(pool: PgPool, path: &str) -> TestResult<(StatusCode, serde_json::Value)> {
    let app = router(AppState::new(pool, test_config(), Arc::new(NoExtension)));
    let response = app.oneshot(Request::get(path).body(Body::empty())?).await?;
    let status = response.status();
    let bytes = response.into_body().collect().await?.to_bytes();
    Ok((status, serde_json::from_slice(&bytes)?))
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn health_reports_a_working_database(pool: PgPool) {
    let (status, body) = get(pool, "/api/v1/health").await.unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
    assert_eq!(body["database"], "ok");
    assert_eq!(body["edition"], "open");
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn health_reports_a_failing_database(pool: PgPool) {
    let closed = pool.clone();
    closed.close().await;
    let (status, body) = get(closed, "/api/v1/health").await.unwrap();
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["status"], "failing");
    assert_eq!(body["database"], "failing");
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_openapi_document_lists_every_endpoint(pool: PgPool) {
    let (status, body) = get(pool, "/api/v1/openapi.json").await.unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["info"]["title"], "Token Controller API");
    assert!(body["paths"]["/api/v1/health"]["get"].is_object());
}

#[test]
fn the_committed_openapi_document_is_current() {
    let committed = include_str!("../openapi.json");
    let current = tokencontroller_server::openapi_json().unwrap();
    assert_eq!(
        committed.trim_end(),
        current.trim_end(),
        "apps/server/openapi.json is out of date: run `cargo run -p tokencontroller-server -- openapi > apps/server/openapi.json`"
    );
}

#[test]
fn every_operation_has_its_own_id() {
    // Clients generated from the document, such as the web app's, refuse
    // two operations with the same id.
    let document: serde_json::Value =
        serde_json::from_str(&tokencontroller_server::openapi_json().unwrap()).unwrap();
    let mut ids = Vec::new();
    for operations in document["paths"].as_object().unwrap().values() {
        for operation in operations.as_object().unwrap().values() {
            ids.push(operation["operationId"].as_str().unwrap().to_owned());
        }
    }
    let count = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), count, "two operations share an operationId");
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn seed_data_loads_twice_without_error(pool: PgPool) {
    tokencontroller_server::seed::load(&pool).await.unwrap();
    tokencontroller_server::seed::load(&pool).await.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_web_app_sits_behind_the_whole_api(pool: PgPool) {
    let dir = std::env::temp_dir().join(format!("tc-web-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("index.html"),
        "<!doctype html><title>Token Controller</title>",
    )
    .unwrap();
    let app = tokencontroller_server::web::with_web_app(
        router(AppState::new(pool, test_config(), Arc::new(NoExtension))),
        &dir,
    );
    let send = |path: &str| {
        let request = Request::get(path).body(Body::empty()).unwrap();
        app.clone().oneshot(request)
    };

    let health = send("/api/v1/health").await.unwrap();
    assert_eq!(health.status(), StatusCode::OK);
    // Every answer carries the request id that feedback reports quote.
    let id = health
        .headers()
        .get("x-request-id")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(uuid::Uuid::parse_str(id).is_ok(), "{id}");

    let page = send("/close/2026-09").await.unwrap();
    assert_eq!(page.status(), StatusCode::OK);
    let body = page.into_body().collect().await.unwrap().to_bytes();
    assert!(String::from_utf8_lossy(&body).contains("<title>Token Controller</title>"));

    let missing = send("/api/v1/nothing-here").await.unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    let body = missing.into_body().collect().await.unwrap().to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["error"], "not_found");
}
