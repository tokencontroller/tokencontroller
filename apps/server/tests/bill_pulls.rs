//! Bill connections over HTTP, the bill pull job and CSV bill uploads,
//! against a real Postgres database and recorded Anthropic and OpenAI
//! answers.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use sqlx::PgPool;
use time::macros::datetime;
use tokencontroller_core::NoExtension;
use tokencontroller_server::integrations::bill_connections::{self, BaseUrls};
use tokencontroller_server::integrations::secrets;
use tokencontroller_server::sign_in::secret;
use tokencontroller_server::{AppState, Config, router};
use tower::ServiceExt as _;
use uuid::Uuid;
use wiremock::matchers::{header as has_header, method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

const KEY: &str = "0123456789abcdef0123456789abcdef";
const OPENAI_KEY: &str = "sk-admin-test";
const CONSOLE_KEY: &str = "sk-ant-admin01-test";
const OPENAI_PAGE_1: &str = include_str!("fixtures/bills/openai_costs_page_1.json");
const OPENAI_PAGE_2: &str = include_str!("fixtures/bills/openai_costs_page_2.json");
const OPENAI_401: &str = include_str!("fixtures/bills/openai_error_401.json");
const OPENAI_429: &str = include_str!("fixtures/bills/openai_error_429.json");
const CONSOLE_ORGANIZATION: &str =
    include_str!("fixtures/bills/anthropic_console_organization.json");
const CONSOLE_PAGE_1: &str =
    include_str!("fixtures/bills/anthropic_console_cost_report_page_1.json");
const CONSOLE_PAGE_2: &str =
    include_str!("fixtures/bills/anthropic_console_cost_report_page_2.json");
const BILLS_CSV: &str = include_str!("fixtures/bills/bills.csv");
const BEDROCK_CSV: &str = include_str!("fixtures/bills/bedrock_cur2.csv");

/// October 2026, so the job pulls September and October.
const NOW: time::OffsetDateTime = datetime!(2026-10-12 06:00 UTC);

fn config() -> Config {
    Config {
        database_url: "postgres://unused".to_owned(),
        public_url: "http://localhost:8080".to_owned(),
        secret_key: KEY.to_owned(),
        smtp_url: None,
        smtp_from: None,
        prices_refresh: false,
        customer_relations: tokencontroller_server::customer_relations::Settings::default(),
        stripe: None,
        embeddings: None,
    }
}

struct Kestrel {
    state: AppState,
    app: Router,
    organization_id: Uuid,
    controller: String,
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
    let mia: Uuid = sqlx::query_scalar(
        "INSERT INTO people (organization_id, email, name, joined_at) VALUES ($1, 'mia.krystof@acme.example', 'Mia Krystof', now()) RETURNING id",
    )
    .bind(created.organization_id)
    .fetch_one(pool)
    .await?;
    Ok(Kestrel {
        controller: session(pool, created.person_id).await?,
        agent_manager: session(pool, mia).await?,
        state,
        app,
        organization_id: created.organization_id,
    })
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
    body: Option<(&str, String)>,
) -> TestResult<Answer> {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::COOKIE, format!("tc_session={session}"));
    let body = match body {
        Some((content_type, body)) => {
            request = request.header(header::CONTENT_TYPE, content_type);
            Body::from(body)
        }
        None => Body::empty(),
    };
    let response = app.clone().oneshot(request.body(body)?).await?;
    let status = response.status();
    let bytes = response.into_body().collect().await?.to_bytes();
    Ok(Answer {
        status,
        json: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    })
}

fn json_body(value: &Value) -> (&'static str, String) {
    ("application/json", value.to_string())
}

fn csv_body(text: &str) -> (&'static str, String) {
    ("text/csv", text.to_owned())
}

/// A bill connection stored the way the endpoint stores one.
async fn connection(kestrel: &Kestrel, provider: &str, key: &str) -> TestResult<Uuid> {
    let sealed = secrets::seal(KEY, key)?;
    let id = sqlx::query_scalar(
        "INSERT INTO bill_connections (organization_id, provider, sealed_key, key_hint)
         VALUES ($1, $2, $3, '…test') RETURNING id",
    )
    .bind(kestrel.organization_id)
    .bind(provider)
    .bind(sealed)
    .fetch_one(&kestrel.state.db)
    .await?;
    Ok(id)
}

fn page(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200)
        .insert_header("openai-organization", "org-Wn3xJ8kPq2RtYv5LmB7cZd1A")
        .set_body_raw(body.to_owned(), "application/json")
}

/// OpenAI's Costs API: September across two pages, nothing yet in October.
async fn mock_openai(server: &MockServer) {
    let costs = || {
        Mock::given(method("GET"))
            .and(path("/v1/organization/costs"))
            .and(has_header(
                "authorization",
                format!("Bearer {OPENAI_KEY}").as_str(),
            ))
    };
    costs()
        .and(query_param("start_time", "1790812800"))
        .respond_with(page(
            r#"{"object": "page", "data": [], "has_more": false, "next_page": null}"#,
        ))
        .with_priority(1)
        .mount(server)
        .await;
    costs()
        .and(query_param("start_time", "1788220800"))
        .and(query_param_is_missing("page"))
        .respond_with(page(OPENAI_PAGE_1))
        .mount(server)
        .await;
    costs()
        .and(query_param("start_time", "1788220800"))
        .and(query_param("page", "page_AAAAAGdGxdEiJdKOAAAAAGcqsYA="))
        .respond_with(page(OPENAI_PAGE_2))
        .mount(server)
        .await;
}

/// Anthropic's Usage and Cost Admin API, with September across two pages.
async fn mock_console(server: &MockServer) {
    let anthropic = |at: &'static str| {
        Mock::given(method("GET"))
            .and(path(at))
            .and(has_header("x-api-key", CONSOLE_KEY))
            .and(has_header("anthropic-version", "2023-06-01"))
    };
    anthropic("/v1/organizations/me")
        .respond_with(page(CONSOLE_ORGANIZATION))
        .mount(server)
        .await;
    anthropic("/v1/organizations/cost_report")
        .and(query_param_is_missing("page"))
        .respond_with(page(CONSOLE_PAGE_1))
        .mount(server)
        .await;
    anthropic("/v1/organizations/cost_report")
        .and(query_param("page", "page_MjAyNi0wOS0wM1QwMDowMDowMFo="))
        .respond_with(page(CONSOLE_PAGE_2))
        .mount(server)
        .await;
}

async fn last_error(pool: &PgPool, id: Uuid) -> TestResult<Option<String>> {
    Ok(
        sqlx::query_scalar("SELECT last_error FROM bill_connections WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await?,
    )
}

async fn bills(kestrel: &Kestrel, period: &str) -> TestResult<Vec<Value>> {
    let answer = send(
        &kestrel.app,
        Method::GET,
        &format!("/api/v1/bills?period={period}"),
        &kestrel.controller,
        None,
    )
    .await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.json);
    Ok(answer.json.as_array().cloned().unwrap_or_default())
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_controller_connects_a_bill_source_and_the_key_never_comes_back(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    let openai = json!({ "provider": "openai", "key": "sk-admin-Wn3xJ8kPq2Rt" });

    let refused = send(
        &kestrel.app,
        Method::POST,
        "/api/v1/bill-connections",
        &kestrel.agent_manager,
        Some(json_body(&openai)),
    )
    .await
    .unwrap();
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    let empty = send(
        &kestrel.app,
        Method::POST,
        "/api/v1/bill-connections",
        &kestrel.controller,
        Some(json_body(
            &json!({ "provider": "anthropic_console", "key": "  " }),
        )),
    )
    .await
    .unwrap();
    assert_eq!(empty.status, StatusCode::BAD_REQUEST);
    assert_eq!(
        empty.json["message"],
        "Add an admin key from Anthropic Console."
    );

    let connected = send(
        &kestrel.app,
        Method::POST,
        "/api/v1/bill-connections",
        &kestrel.controller,
        Some(json_body(&openai)),
    )
    .await
    .unwrap();
    assert_eq!(connected.status, StatusCode::CREATED, "{}", connected.json);
    assert_eq!(connected.json["provider"], "openai");
    assert_eq!(connected.json["key_hint"], "…q2Rt");
    assert_eq!(connected.json["status"], "ok");
    assert_eq!(connected.json["last_pull_at"], Value::Null);
    assert!(
        !connected.json.to_string().contains("sk-admin-Wn3xJ8kPq2Rt"),
        "the key is never sent back"
    );
    let stored: String = sqlx::query_scalar("SELECT sealed_key FROM bill_connections")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        !stored.contains("sk-admin-Wn3xJ8kPq2Rt"),
        "the key is sealed"
    );
    let asked: Option<time::OffsetDateTime> =
        sqlx::query_scalar("SELECT requested_at FROM job_runs WHERE job = 'bill-pulls'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(asked.is_some(), "the first pull is asked for at once");

    let listed = send(
        &kestrel.app,
        Method::GET,
        "/api/v1/bill-connections",
        &kestrel.controller,
        None,
    )
    .await
    .unwrap();
    assert_eq!(listed.status, StatusCode::OK);
    assert_eq!(listed.json.as_array().map(Vec::len), Some(1));
    let hidden = send(
        &kestrel.app,
        Method::GET,
        "/api/v1/bill-connections",
        &kestrel.agent_manager,
        None,
    )
    .await
    .unwrap();
    assert_eq!(hidden.status, StatusCode::FORBIDDEN);

    let id = connected.json["id"].as_str().unwrap().to_owned();
    let pull = send(
        &kestrel.app,
        Method::POST,
        &format!("/api/v1/bill-connections/{id}/pull"),
        &kestrel.controller,
        None,
    )
    .await
    .unwrap();
    assert_eq!(pull.status, StatusCode::ACCEPTED);
    let unknown = send(
        &kestrel.app,
        Method::POST,
        &format!("/api/v1/bill-connections/{}/pull", Uuid::new_v4()),
        &kestrel.controller,
        None,
    )
    .await
    .unwrap();
    assert_eq!(unknown.status, StatusCode::NOT_FOUND);

    let removed = send(
        &kestrel.app,
        Method::DELETE,
        &format!("/api/v1/bill-connections/{id}"),
        &kestrel.controller,
        None,
    )
    .await
    .unwrap();
    assert_eq!(removed.status, StatusCode::NO_CONTENT);
    let again = send(
        &kestrel.app,
        Method::DELETE,
        &format!("/api/v1/bill-connections/{id}"),
        &kestrel.controller,
        None,
    )
    .await
    .unwrap();
    assert_eq!(again.status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_job_pulls_this_and_last_period_and_a_second_pull_changes_nothing(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    let server = MockServer::start().await;
    mock_openai(&server).await;
    mock_console(&server).await;
    let openai = connection(&kestrel, "openai", OPENAI_KEY).await.unwrap();
    let console = connection(&kestrel, "anthropic_console", CONSOLE_KEY)
        .await
        .unwrap();

    let note =
        bill_connections::pull_due(kestrel.state.clone(), BaseUrls::all_at(&server.uri()), NOW)
            .await
            .unwrap();
    assert_eq!(note, "2 of 2 bill sources pulled, 4 bills changed");
    assert_eq!(last_error(&pool, openai).await.unwrap(), None);
    assert_eq!(last_error(&pool, console).await.unwrap(), None);

    let september = bills(&kestrel, "2026-09").await.unwrap();
    let figures: Vec<(String, String, String, String)> = september
        .iter()
        .map(|bill| {
            (
                bill["provider"].as_str().unwrap_or_default().to_owned(),
                bill["cost_source"].as_str().unwrap_or_default().to_owned(),
                bill["gross"].as_str().unwrap_or_default().to_owned(),
                bill["source"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    assert!(figures.contains(&(
        "openai".to_owned(),
        "api_key".to_owned(),
        "58.27".to_owned(),
        "pulled".to_owned()
    )));
    assert!(figures.contains(&(
        "anthropic".to_owned(),
        "api_key".to_owned(),
        "48.31".to_owned(),
        "pulled".to_owned()
    )));
    let openai_bill = september
        .iter()
        .find(|bill| bill["provider"] == "openai")
        .unwrap();
    assert_eq!(openai_bill["lines"].as_array().map(Vec::len), Some(4));
    assert_eq!(openai_bill["lines"][3]["amount"], "0.000123");
    let october = bills(&kestrel, "2026-10").await.unwrap();
    assert_eq!(october.len(), 2, "this month's bills so far");

    // Not due again until tomorrow.
    let quiet =
        bill_connections::pull_due(kestrel.state.clone(), BaseUrls::all_at(&server.uri()), NOW)
            .await
            .unwrap();
    assert_eq!(quiet, "0 of 0 bill sources pulled, 0 bills changed");

    // Asked for again, the same figures store no new revision.
    send(
        &kestrel.app,
        Method::POST,
        &format!("/api/v1/bill-connections/{openai}/pull"),
        &kestrel.controller,
        None,
    )
    .await
    .unwrap();
    let again =
        bill_connections::pull_due(kestrel.state.clone(), BaseUrls::all_at(&server.uri()), NOW)
            .await
            .unwrap();
    assert_eq!(again, "1 of 1 bill sources pulled, 0 bills changed");
    let revisions: i64 = sqlx::query_scalar("SELECT count(*) FROM bill_revisions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(revisions, 4);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_refused_key_is_that_connections_problem_and_an_outage_is_the_jobs(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    let server = MockServer::start().await;
    mock_console(&server).await;
    Mock::given(path("/v1/organization/costs"))
        .respond_with(ResponseTemplate::new(401).set_body_raw(OPENAI_401, "application/json"))
        .mount(&server)
        .await;
    let openai = connection(&kestrel, "openai", "sk-admin-wrong")
        .await
        .unwrap();
    connection(&kestrel, "anthropic_console", CONSOLE_KEY)
        .await
        .unwrap();

    let note =
        bill_connections::pull_due(kestrel.state.clone(), BaseUrls::all_at(&server.uri()), NOW)
            .await
            .unwrap();
    assert_eq!(note, "1 of 2 bill sources pulled, 2 bills changed");
    let refused = last_error(&pool, openai).await.unwrap().unwrap_or_default();
    assert!(
        refused.starts_with(
            "OpenAI did not accept the admin key. Use an Admin key of the organization (it \
             starts with sk-admin-), created by an organization owner under Organization \
             settings, Admin keys. The provider said: Incorrect API key provided"
        ),
        "{refused}"
    );
    let listed = send(
        &kestrel.app,
        Method::GET,
        "/api/v1/bill-connections",
        &kestrel.controller,
        None,
    )
    .await
    .unwrap();
    let failing = listed
        .json
        .as_array()
        .and_then(|all| all.iter().find(|row| row["provider"] == "openai"))
        .cloned()
        .unwrap_or_default();
    assert_eq!(failing["status"], "failing");

    // Nothing answers: every source fails, so the job is down.
    sqlx::query("UPDATE bill_connections SET pull_requested_at = now()")
        .execute(&pool)
        .await
        .unwrap();
    let down = bill_connections::pull_due(
        kestrel.state.clone(),
        BaseUrls::all_at("http://127.0.0.1:9"),
        NOW,
    )
    .await;
    let message = down
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
    assert!(
        message.starts_with("no bill source could be pulled: "),
        "{message}"
    );
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_source_that_asks_to_wait_is_tried_again_at_the_next_run(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    let server = MockServer::start().await;
    Mock::given(path("/v1/organization/costs"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "20")
                .set_body_raw(OPENAI_429, "application/json"),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;
    mock_openai(&server).await;
    let openai = connection(&kestrel, "openai", OPENAI_KEY).await.unwrap();

    let limited =
        bill_connections::pull_due(kestrel.state.clone(), BaseUrls::all_at(&server.uri()), NOW)
            .await
            .unwrap();
    assert_eq!(limited, "0 of 1 bill sources pulled, 0 bills changed");
    assert!(last_error(&pool, openai).await.unwrap().is_some());

    let next =
        bill_connections::pull_due(kestrel.state.clone(), BaseUrls::all_at(&server.uri()), NOW)
            .await
            .unwrap();
    assert_eq!(next, "1 of 1 bill sources pulled, 2 bills changed");
    assert_eq!(last_error(&pool, openai).await.unwrap(), None);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_csv_bill_file_is_previewed_then_uploaded(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();

    let preview = send(
        &kestrel.app,
        Method::POST,
        "/api/v1/bills/csv/preview",
        &kestrel.controller,
        Some(csv_body(BILLS_CSV)),
    )
    .await
    .unwrap();
    assert_eq!(preview.status, StatusCode::OK, "{}", preview.json);
    assert_eq!(preview.json["problem"], Value::Null);
    let shown = preview.json["bills"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert_eq!(shown.len(), 3);
    assert_eq!(shown[0]["provider"], "openai");
    assert_eq!(shown[0]["net"], "1234.56");
    assert_eq!(shown[0]["exchange_rate"], "0.92");
    assert_eq!(shown[2]["provider"], "other");
    assert_eq!(shown[2]["currency"], "EUR");
    let saved: i64 = sqlx::query_scalar("SELECT count(*) FROM bills")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(saved, 0, "a preview saves nothing");

    let broken = "period,cost_source,gross,discount,credit,net,currency\n\
                  2026-09,api_key,10,0,0,10,USD\n\
                  2026-09,seat,30,0,0,30,USD\n";
    let problem = send(
        &kestrel.app,
        Method::POST,
        "/api/v1/bills/csv/preview",
        &kestrel.controller,
        Some(csv_body(broken)),
    )
    .await
    .unwrap();
    assert_eq!(problem.json["bills"], json!([]));
    assert_eq!(problem.json["problem"]["row"], 3);
    assert_eq!(problem.json["problem"]["column"], "cost_source");
    let refused = send(
        &kestrel.app,
        Method::POST,
        "/api/v1/bills/csv",
        &kestrel.controller,
        Some(csv_body(broken)),
    )
    .await
    .unwrap();
    assert_eq!(refused.status, StatusCode::BAD_REQUEST);
    assert_eq!(
        refused.json["message"],
        "Row 3, column cost_source: Seats and subscriptions are flat fees and have no bill. \
         Leave this row out; finance handles them as before. Nothing was saved."
    );
    // One good row and one the bills area would refuse: the preview names
    // it, and the upload stores neither.
    let half_good = "period,cost_source,gross,discount,credit,net,currency\n\
                     2026-09,api_key,10,0,0,10,USD\n\
                     2026-08,api_key,10,-5,0,15,USD\n";
    let negative = send(
        &kestrel.app,
        Method::POST,
        "/api/v1/bills/csv/preview",
        &kestrel.controller,
        Some(csv_body(half_good)),
    )
    .await
    .unwrap();
    assert_eq!(negative.json["problem"]["row"], 3);
    assert_eq!(negative.json["problem"]["column"], "discount");
    let partial = send(
        &kestrel.app,
        Method::POST,
        "/api/v1/bills/csv",
        &kestrel.controller,
        Some(csv_body(half_good)),
    )
    .await
    .unwrap();
    assert_eq!(partial.status, StatusCode::BAD_REQUEST);
    let saved: i64 = sqlx::query_scalar("SELECT count(*) FROM bills")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(saved, 0, "a file with a problem saves nothing");

    let not_a_controller = send(
        &kestrel.app,
        Method::POST,
        "/api/v1/bills/csv",
        &kestrel.agent_manager,
        Some(csv_body(BILLS_CSV)),
    )
    .await
    .unwrap();
    assert_eq!(not_a_controller.status, StatusCode::FORBIDDEN);

    let uploaded = send(
        &kestrel.app,
        Method::POST,
        "/api/v1/bills/csv",
        &kestrel.controller,
        Some(csv_body(BILLS_CSV)),
    )
    .await
    .unwrap();
    assert_eq!(uploaded.status, StatusCode::CREATED, "{}", uploaded.json);
    assert_eq!(uploaded.json["changed"], 3);
    assert_eq!(uploaded.json["bills"][0]["source"], "csv");
    let twice = send(
        &kestrel.app,
        Method::POST,
        "/api/v1/bills/csv",
        &kestrel.controller,
        Some(csv_body(BILLS_CSV)),
    )
    .await
    .unwrap();
    assert_eq!(
        twice.json["changed"], 0,
        "the same file again changes nothing"
    );
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_clouds_cost_export_is_one_periods_bill(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    let without_period = send(
        &kestrel.app,
        Method::POST,
        "/api/v1/bills/csv/preview?cloud=bedrock",
        &kestrel.controller,
        Some(csv_body(BEDROCK_CSV)),
    )
    .await
    .unwrap();
    assert_eq!(
        without_period.json["problem"]["message"],
        "Give the period the export is the bill for, such as 2026-09."
    );

    let uploaded = send(
        &kestrel.app,
        Method::POST,
        "/api/v1/bills/csv?cloud=bedrock&period=2026-09",
        &kestrel.controller,
        Some(csv_body(BEDROCK_CSV)),
    )
    .await
    .unwrap();
    assert_eq!(uploaded.status, StatusCode::CREATED, "{}", uploaded.json);
    let bill = &uploaded.json["bills"][0];
    assert_eq!(bill["provider"], "aws_bedrock");
    assert_eq!(bill["cost_source"], "cloud_marketplace");
    assert_eq!(bill["period"], "2026-09");
    assert!(
        bill["lines"]
            .as_array()
            .is_some_and(|lines| !lines.is_empty())
    );
}
