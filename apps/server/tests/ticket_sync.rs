//! Ticket systems over HTTP and the ticket sync job, against a real Postgres
//! database and recorded Jira Cloud answers. Synced issues become feature
//! tag values in the event log, with the mapped project as their default.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use sqlx::PgPool;
use tokencontroller_core::NoExtension;
use tokencontroller_server::integrations::ticket_systems;
use tokencontroller_server::integrations::{secrets, tickets::TicketSystem};
use tokencontroller_server::jobs::{self, Attempt};
use tokencontroller_server::sign_in::secret;
use tokencontroller_server::{AppState, Config, router};
use tower::ServiceExt as _;
use uuid::Uuid;
use wiremock::matchers::{basic_auth, body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

const KEY: &str = "0123456789abcdef0123456789abcdef";
const SEARCH_PAGE_1: &str = include_str!("fixtures/tickets/jira/search_page_1.json");
const SEARCH_PAGE_2: &str = include_str!("fixtures/tickets/jira/search_page_2.json");

fn config() -> Config {
    Config {
        database_url: "postgres://unused".to_owned(),
        public_url: "http://localhost:8080".to_owned(),
        secret_key: KEY.to_owned(),
        smtp_url: None,
        smtp_from: None,
        price_table: None,
        rates_refresh: false,
        classifier: None,
    }
}

struct Kestrel {
    state: AppState,
    app: Router,
    organization_id: Uuid,
    org_manager: String,
    agent_manager: String,
}

async fn session(pool: &PgPool, person_id: Uuid) -> TestResult<String> {
    let session = secret::generate(secret::SESSION, KEY)?;
    sqlx::query("INSERT INTO sessions (token_hash, person_id, client, expires_at) VALUES ($1, $2, 'browser', now() + interval '1 day')")
        .bind(&session.hash)
        .bind(person_id)
        .execute(pool)
        .await?;
    Ok(session.token)
}

async fn kestrel(pool: &PgPool) -> TestResult<Kestrel> {
    let state = AppState::new(pool.clone(), config(), Arc::new(NoExtension));
    let app = router(state.clone());
    let mut conn = pool.acquire().await?;
    let created = tokencontroller_server::organizations::create(
        &mut conn,
        "Kestrel Digital",
        "ana.silva@acme.example",
    )
    .await
    .map_err(|error| error.to_string())?;
    let organization_id = created.organization_id;
    let mia: Uuid = sqlx::query_scalar(
        "INSERT INTO people (organization_id, email, name, joined_at) VALUES ($1, 'mia.krystof@acme.example', 'Mia Krystof', now()) RETURNING id",
    )
    .bind(organization_id)
    .fetch_one(pool)
    .await?;
    let kestrel = Kestrel {
        org_manager: session(pool, created.person_id).await?,
        agent_manager: session(pool, mia).await?,
        state,
        app,
        organization_id,
    };
    // The project the issues map to is a tag value on record.
    let project = send(
        &kestrel.app,
        Method::POST,
        "/api/v1/tags/project/values",
        &kestrel.org_manager,
        Some(json!({ "key": "storefront", "title": "Kestrel storefront" })),
    )
    .await?;
    assert_eq!(project.status, StatusCode::CREATED, "{}", project.json);
    Ok(kestrel)
}

/// The feature tag values on record, by key: title, description,
/// source status and the project default.
async fn features(
    kestrel: &Kestrel,
) -> TestResult<
    Vec<(
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
    )>,
> {
    let listed = send(
        kestrel.app(),
        Method::GET,
        "/api/v1/tags",
        &kestrel.org_manager,
        None,
    )
    .await?;
    if listed.status != StatusCode::OK {
        return Err(format!("GET /tags: {} {}", listed.status, listed.json).into());
    }
    let text = |value: &Value, pointer: &str| {
        value
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let mut rows: Vec<_> = listed
        .json
        .as_array()
        .into_iter()
        .flatten()
        .filter(|name| name.get("name").and_then(Value::as_str) == Some("feature"))
        .flat_map(|name| {
            name.get("values")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        })
        .map(|value| {
            (
                text(&value, "/key").unwrap_or_default(),
                text(&value, "/title"),
                text(&value, "/description"),
                text(&value, "/source_status"),
                text(&value, "/defaults/project").unwrap_or_default(),
            )
        })
        .collect();
    rows.sort();
    Ok(rows)
}

impl Kestrel {
    const fn app(&self) -> &Router {
        &self.app
    }
}

struct Answer {
    status: StatusCode,
    json: Value,
}

async fn send(
    app: &Router,
    method: Method,
    path: &str,
    session: &str,
    body: Option<Value>,
) -> TestResult<Answer> {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::COOKIE, format!("tc_session={session}"))
        .header(header::CONTENT_TYPE, "application/json");
    let body = body.map_or_else(Body::empty, |body| Body::from(body.to_string()));
    let response = app.clone().oneshot(request.body(body)?).await?;
    let status = response.status();
    let bytes = response.into_body().collect().await?.to_bytes();
    Ok(Answer {
        status,
        json: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    })
}

/// A Jira Cloud connection pointing at `server`, stored the way the
/// endpoint stores one, since the endpoint takes only Atlassian addresses.
async fn jira_at(kestrel: &Kestrel, server: &MockServer) -> TestResult<Uuid> {
    let sealed = secrets::seal(
        KEY,
        &json!({ "email": "dev@acme.example", "token": "ATATT3xFfGF0-test" }).to_string(),
    )?;
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO ticket_systems (organization_id, kind, site, sealed_credentials, key_hint)
         VALUES ($1, $2, $3, $4, '…test') RETURNING id",
    )
    .bind(kestrel.organization_id)
    .bind(TicketSystem::Jira.as_str())
    .bind(server.uri())
    .bind(sealed)
    .fetch_one(&kestrel.state.db)
    .await?;
    Ok(id)
}

async fn mock_search(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/rest/api/3/search/jql"))
        .and(basic_auth("dev@acme.example", "ATATT3xFfGF0-test"))
        .and(body_partial_json(json!({ "nextPageToken": "CAEaAggD" })))
        .respond_with(ResponseTemplate::new(200).set_body_raw(SEARCH_PAGE_2, "application/json"))
        .with_priority(1)
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/rest/api/3/search/jql"))
        .and(basic_auth("dev@acme.example", "ATATT3xFfGF0-test"))
        .and(body_partial_json(
            json!({ "jql": "project = \"KD\" ORDER BY updated ASC" }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_raw(SEARCH_PAGE_1, "application/json"))
        .expect(1)
        .mount(server)
        .await;
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn an_org_manager_connects_a_ticket_system(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    let connect = async |session: &str, body: Value| {
        send(
            &kestrel.app,
            Method::POST,
            "/api/v1/ticket-systems",
            session,
            Some(body),
        )
        .await
        .unwrap()
    };
    let jira = json!({ "kind": "jira", "site": "https://acme.atlassian.net/", "token": "ATATT3xFfGF0-secret", "email": "dev@acme.example" });

    let refused = connect(&kestrel.agent_manager, jira.clone()).await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    let elsewhere = connect(
        &kestrel.org_manager,
        json!({ "kind": "jira", "site": "https://evil.example", "token": "x", "email": "dev@acme.example" }),
    )
    .await;
    assert_eq!(elsewhere.status, StatusCode::BAD_REQUEST);
    assert_eq!(
        elsewhere.json["message"],
        "The address should be a Jira Cloud site, such as https://acme.atlassian.net."
    );
    let no_email = connect(
        &kestrel.org_manager,
        json!({ "kind": "jira", "site": "https://acme.atlassian.net", "token": "ATATT3xFfGF0-secret" }),
    )
    .await;
    assert_eq!(no_email.status, StatusCode::BAD_REQUEST);

    let connected = connect(&kestrel.org_manager, jira).await;
    assert_eq!(connected.status, StatusCode::CREATED, "{}", connected.json);
    assert_eq!(connected.json["kind"], "jira");
    assert_eq!(connected.json["site"], "https://acme.atlassian.net");
    assert_eq!(connected.json["key_hint"], "…cret");
    assert_eq!(connected.json["status"], "ok");
    assert!(
        !connected.json.to_string().contains("ATATT3xFfGF0-secret"),
        "the token is never sent back"
    );
    let stored: String = sqlx::query_scalar("SELECT sealed_credentials FROM ticket_systems")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        !stored.contains("ATATT3xFfGF0-secret"),
        "the token is sealed"
    );
    let asked: Option<time::OffsetDateTime> =
        sqlx::query_scalar("SELECT requested_at FROM job_runs WHERE job = 'ticket-sync'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(asked.is_some(), "the first sync is asked for at once");

    let id = connected.json["id"].as_str().unwrap().to_owned();
    let mapped = send(
        &kestrel.app,
        Method::PUT,
        &format!("/api/v1/ticket-systems/{id}/mappings"),
        &kestrel.org_manager,
        Some(json!([{ "external": "KD", "external_name": "Kestrel storefront", "project": "storefront" }])),
    )
    .await
    .unwrap();
    assert_eq!(mapped.status, StatusCode::OK, "{}", mapped.json);
    assert_eq!(mapped.json["mappings"][0]["external"], "KD");
    assert_eq!(
        mapped.json["mappings"][0]["external_name"],
        "Kestrel storefront"
    );
    assert_eq!(mapped.json["mappings"][0]["project"], "storefront");
    assert_eq!(
        mapped.json["mappings"][0]["fields"],
        json!([
            "title",
            "description",
            "type",
            "status",
            "parent",
            "assignee"
        ])
    );
    let unknown = send(
        &kestrel.app,
        Method::PUT,
        &format!("/api/v1/ticket-systems/{id}/mappings"),
        &kestrel.org_manager,
        Some(json!([{ "external": "OPS", "project": "operations" }])),
    )
    .await
    .unwrap();
    assert_eq!(unknown.status, StatusCode::BAD_REQUEST);

    let listed = send(
        &kestrel.app,
        Method::GET,
        "/api/v1/ticket-systems",
        &kestrel.agent_manager,
        None,
    )
    .await
    .unwrap();
    assert_eq!(listed.status, StatusCode::OK);
    assert_eq!(listed.json.as_array().map(Vec::len), Some(1));

    let sync = send(
        &kestrel.app,
        Method::POST,
        &format!("/api/v1/ticket-systems/{id}/sync"),
        &kestrel.org_manager,
        None,
    )
    .await
    .unwrap();
    assert_eq!(sync.status, StatusCode::ACCEPTED);
    let gone = send(
        &kestrel.app,
        Method::DELETE,
        &format!("/api/v1/ticket-systems/{id}"),
        &kestrel.org_manager,
        None,
    )
    .await
    .unwrap();
    assert_eq!(gone.status, StatusCode::NO_CONTENT);
    let again = send(
        &kestrel.app,
        Method::DELETE,
        &format!("/api/v1/ticket-systems/{id}"),
        &kestrel.org_manager,
        None,
    )
    .await
    .unwrap();
    assert_eq!(again.status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_sync_writes_features_with_parents_and_assignees(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    let server = MockServer::start().await;
    mock_search(&server).await;
    let id = jira_at(&kestrel, &server).await.unwrap();
    let mapped = send(
        &kestrel.app,
        Method::PUT,
        &format!("/api/v1/ticket-systems/{id}/mappings"),
        &kestrel.org_manager,
        Some(json!([{ "external": "KD", "project": "storefront" }])),
    )
    .await
    .unwrap();
    assert_eq!(mapped.status, StatusCode::OK, "{}", mapped.json);

    // A token entry names KD-215 before the issue synced: the tag counts,
    // and the answer says the value is not on record yet.
    let answer = send(
        &kestrel.app,
        Method::POST,
        "/api/v1/token-entries",
        &kestrel.org_manager,
        Some(json!({
            "agent_harness": "claude-code",
            "model": "claude-sonnet-5",
            "started_at": "2026-10-12T08:00:00Z",
            "ended_at": "2026-10-12T08:20:00Z",
            "tokens": { "input": 1000, "output": 100, "cache_read": 0, "cache_write": 0 },
            "feature": "KD-215",
            "signals": { "branch": "feature/KD-215-cart" }
        })),
    )
    .await
    .unwrap();
    assert_eq!(answer.status, StatusCode::CREATED, "{}", answer.json);
    assert_eq!(answer.json["unknown_tags"], json!(["feature: KD-215"]));

    let note = ticket_systems::sync_due(kestrel.state.clone())
        .await
        .unwrap();
    assert_eq!(note, "1 of 1 ticket systems synced, 3 features written");

    let synced = features(&kestrel).await.unwrap();
    assert_eq!(synced.len(), 3, "{synced:?}");
    assert_eq!(synced[0].0, "KD-214");
    assert_eq!(
        synced[0].1.as_deref(),
        Some("Guest checkout fails with saved cards")
    );
    // The story links to its epic and names the assignee.
    assert!(
        synced[0].2.as_deref().is_some_and(|text| {
            text.ends_with("Parent: KD-200\nAssignee: mia.krystof@acme.example")
        }),
        "{synced:?}"
    );
    assert_eq!(synced[0].3.as_deref(), Some("open"));
    assert_eq!(synced[0].4, "storefront");
    // Jira hides this assignee's email, so the name stands in.
    assert_eq!(synced[1].0, "KD-215");
    assert!(
        synced[1]
            .2
            .as_deref()
            .is_some_and(|text| text.contains("Parent: KD-200") && text.contains("Assignee: ")),
        "{synced:?}"
    );
    assert_eq!(synced[1].3.as_deref(), Some("open"));
    assert_eq!(synced[2].0, "KD-216");
    assert_eq!(synced[2].4, "storefront");
    let source_ids: Vec<Option<String>> = sqlx::query_scalar(
        "SELECT payload->'tag_value'->>'source_id' FROM events
         WHERE organization_id = $1 AND kind = 'tag value' AND payload->'tag_value'->>'name' = 'feature'
         ORDER BY id",
    )
    .bind(kestrel.organization_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(source_ids.len(), 3);
    assert!(
        source_ids
            .iter()
            .all(|id| id.as_deref().is_some_and(|id| id.contains("KD-"))),
        "{source_ids:?}"
    );

    let (last_sync, error, synced_until_lag): (Option<time::OffsetDateTime>, Option<String>, bool) =
        sqlx::query_as(
            "SELECT s.last_sync_at, s.last_error,
                    m.synced_until = s.last_sync_started_at - interval '5 minutes'
             FROM ticket_systems s JOIN ticket_system_projects m ON m.ticket_system_id = s.id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(last_sync.is_some());
    assert_eq!(error, None);
    assert!(
        synced_until_lag,
        "the next sync asks from five minutes before this one"
    );

    // The message's tag is now a value on record, so a report by feature
    // shows the project it brings.
    let report = send(
        &kestrel.app,
        Method::GET,
        "/api/v1/reports/cost?from=2026-10-01&to=2026-10-31&by=project",
        &kestrel.org_manager,
        None,
    )
    .await
    .unwrap();
    assert_eq!(report.status, StatusCode::OK, "{}", report.json);
    assert!(
        report.json["rows"]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|row| row["key"] == "storefront")),
        "{}",
        report.json
    );

    // Nothing is due again until the minutes pass.
    let quiet = ticket_systems::sync_due(kestrel.state.clone())
        .await
        .unwrap();
    assert_eq!(quiet, "0 of 0 ticket systems synced, 0 features written");
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_refused_token_shows_on_the_ticket_system_not_the_status_page(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/rest/api/3/search/jql"))
        .respond_with(ResponseTemplate::new(401).set_body_raw(
            r#"{"errorMessages":["Client must be authenticated to access this resource."],"errors":{}}"#,
            "application/json",
        ))
        .mount(&server)
        .await;
    let id = jira_at(&kestrel, &server).await.unwrap();
    sqlx::query(
        "INSERT INTO ticket_system_projects (organization_id, ticket_system_id, external_project, external_name, project)
         VALUES ($1, $2, 'KD', 'KD', 'storefront')",
    )
    .bind(kestrel.organization_id)
    .bind(id)
    .execute(&pool)
    .await
    .unwrap();

    let job = jobs::standard(&kestrel.state.config)
        .into_iter()
        .find(|job| job.name == ticket_systems::JOB)
        .unwrap();
    let attempt = jobs::run_if_due(&kestrel.state, &job).await.unwrap();
    assert_eq!(attempt, Attempt::Ran { succeeded: true });

    let listed = send(
        &kestrel.app,
        Method::GET,
        "/api/v1/ticket-systems",
        &kestrel.org_manager,
        None,
    )
    .await
    .unwrap();
    assert_eq!(listed.json[0]["status"], "failing");
    assert!(
        listed.json[0]["message"]
            .as_str()
            .is_some_and(|message| !message.is_empty()),
        "{}",
        listed.json
    );
    let status = send(&kestrel.app, Method::GET, "/api/v1/status", "", None)
        .await
        .unwrap();
    let ticket_sync = status.json["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|component| component["id"] == "ticket-sync")
        .unwrap()
        .clone();
    assert_eq!(ticket_sync["state"], "operational");
    let prices = status.json["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|component| component["id"] == "price-table")
        .unwrap()
        .clone();
    assert_eq!(
        prices["state"], "not_running",
        "TC_PRICE_TABLE is not set in this test"
    );
}
