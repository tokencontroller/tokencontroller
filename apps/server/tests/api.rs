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
        embeddings: None,
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

#[test]
fn path_parameters_are_in_their_path() {
    // A query parameter declared `in: path` makes generated clients put it
    // in the URL path, where the service never reads it.
    let document: serde_json::Value =
        serde_json::from_str(&tokencontroller_server::openapi_json().unwrap()).unwrap();
    for (path, operations) in document["paths"].as_object().unwrap() {
        for operation in operations.as_object().unwrap().values() {
            let Some(parameters) = operation["parameters"].as_array() else {
                continue;
            };
            for parameter in parameters {
                let name = parameter["name"].as_str().unwrap();
                if parameter["in"] == "path" {
                    assert!(
                        path.contains(&format!("{{{name}}}")),
                        "{path} declares `{name}` in the path"
                    );
                }
            }
        }
    }
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn seed_data_loads_twice_without_error(pool: PgPool) {
    tokencontroller_server::seed::load(&pool).await.unwrap();
    tokencontroller_server::seed::load(&pool).await.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn every_answer_carries_a_request_id(pool: PgPool) {
    let app = router(AppState::new(pool, test_config(), Arc::new(NoExtension)));
    let request = Request::get("/api/v1/health").body(Body::empty()).unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    // Feedback reports quote this id (docs: Security and privacy).
    let id = response
        .headers()
        .get("x-request-id")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(uuid::Uuid::parse_str(id).is_ok(), "{id}");

    // An id the caller already sent is kept.
    let request = Request::get("/api/v1/health")
        .header("x-request-id", "abc-123")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.headers().get("x-request-id").unwrap(), "abc-123");
}
