//! Token sheets against a real Postgres database (docs: Attribute > Review
//! and submit, Ticket key patterns; Account > Cadence and token sheets,
//! Approval): submit from the CLI and the web app, the review list and its
//! fixes, reject and resubmit, late token sheets, asking everyone to
//! submit, and ticket keys still unknown after a sync.

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::sync::Arc;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tokencontroller_core::NoExtension;
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
        customer_relations: tokencontroller_server::customer_relations::Settings::default(),
        stripe: None,
        embeddings: None,
    }
}

/// Kestrel Digital: a controller, a people manager over `team: web`, and
/// Ana, an agent manager in `team: web`, with a browser session each and
/// Ana's CLI token. The project storefront has the pattern `^(KD|OPS)-\d+`
/// and the ticket KD-214.
struct Kestrel {
    app: Router,
    organization_id: Uuid,
    controller: String,
    people_manager: String,
    ana_id: Uuid,
    ana: String,
    ana_cli: String,
}

async fn session(pool: &PgPool, person_id: Uuid, prefix: &str, client: &str) -> TestResult<String> {
    let session = secret::generate(prefix, KEY)?;
    sqlx::query("INSERT INTO sessions (token_hash, person_id, client, expires_at) VALUES ($1, $2, $3, now() + interval '1 day')")
        .bind(&session.hash)
        .bind(person_id)
        .bind(client)
        .execute(pool)
        .await?;
    Ok(session.token)
}

async fn person(pool: &PgPool, org: Uuid, email: &str, role: &str) -> TestResult<Uuid> {
    Ok(sqlx::query_scalar(
        r#"INSERT INTO people (organization_id, email, name, role, tags, joined_at)
           VALUES ($1, $2, $3, $4, '{"team": "web"}', now()) RETURNING id"#,
    )
    .bind(org)
    .bind(email)
    .bind(email.split('@').next().unwrap_or(email))
    .bind(role)
    .fetch_one(pool)
    .await?)
}

async fn kestrel(pool: &PgPool) -> TestResult<Kestrel> {
    let app = router(AppState::new(pool.clone(), config(), Arc::new(NoExtension)));
    let mut conn = pool.acquire().await?;
    let created = tokencontroller_server::organizations::create(
        &mut conn,
        "Kestrel Digital",
        "nina@kestrel.example",
    )
    .await
    .map_err(|error| error.to_string())?;
    let org = created.organization_id;
    sqlx::query(
        "INSERT INTO tag_names (organization_id, name) VALUES ($1, 'team') ON CONFLICT DO NOTHING",
    )
    .bind(org)
    .execute(pool)
    .await?;
    let people_manager = person(pool, org, "priya@kestrel.example", "people_manager").await?;
    sqlx::query("INSERT INTO people_manager_filters (organization_id, person_id, filter) VALUES ($1, $2, '{\"team\": \"web\"}')")
        .bind(org)
        .bind(people_manager)
        .execute(pool)
        .await?;
    let ana_id = person(pool, org, "ana@kestrel.example", "agent_manager").await?;
    let project: Uuid = sqlx::query_scalar(
        r"INSERT INTO projects (organization_id, name, ticket_key_pattern) VALUES ($1, 'storefront', '^(KD|OPS)-\d+') RETURNING id",
    )
    .bind(org)
    .fetch_one(pool)
    .await?;
    sqlx::query("INSERT INTO tickets (organization_id, key, project_id, title) VALUES ($1, 'KD-214', $2, 'Cart totals')")
        .bind(org)
        .bind(project)
        .execute(pool)
        .await?;
    Ok(Kestrel {
        app,
        organization_id: org,
        controller: session(pool, created.person_id, secret::SESSION, "browser").await?,
        people_manager: session(pool, people_manager, secret::SESSION, "browser").await?,
        ana_id,
        ana: session(pool, ana_id, secret::SESSION, "browser").await?,
        ana_cli: session(pool, ana_id, secret::CLI, "local_tool").await?,
    })
}

async fn call(
    app: &Router,
    method: Method,
    path: &str,
    token: &str,
    body: Option<Value>,
) -> TestResult<(StatusCode, Value)> {
    let mut request = Request::builder().method(method).uri(path);
    request = if token.starts_with(secret::SESSION) {
        request.header(header::COOKIE, format!("tc_session={token}"))
    } else {
        request.header(header::AUTHORIZATION, format!("Bearer {token}"))
    };
    let request = match body {
        Some(body) => request
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&body)?))?,
        None => request.body(Body::empty())?,
    };
    let response = app.clone().oneshot(request).await?;
    let status = response.status();
    let bytes = response.into_body().collect().await?.to_bytes();
    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)?
    };
    Ok((status, json))
}

fn rfc3339(at: OffsetDateTime) -> String {
    at.format(&Rfc3339).unwrap_or_default()
}

/// A Claude Code session of an hour ago, as `tc submit` sends it.
fn session_entry(local_id: &str, extra: &Value) -> Value {
    let now = OffsetDateTime::now_utc();
    let started = now.checked_sub(time::Duration::hours(1)).unwrap_or(now);
    let mut entry = json!({
        "agent_harness": "claude-code",
        "model": "claude-opus-5",
        "started_at": rfc3339(started),
        "ended_at": rfc3339(started.checked_add(time::Duration::minutes(20)).unwrap_or(now)),
        "tokens": {"input": 12000, "output": 3000},
        "reported_cost": "0.42",
        "summary": "Fixed the cart totals when a coupon is applied",
        "local_id": local_id
    });
    if let (Some(entry), Some(extra)) = (entry.as_object_mut(), extra.as_object()) {
        for (name, value) in extra {
            entry.insert(name.clone(), value.clone());
        }
    }
    entry
}

async fn count(pool: &PgPool, sql: &'static str, id: Uuid) -> TestResult<i64> {
    Ok(sqlx::query_scalar(sql).bind(id).fetch_one(pool).await?)
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_cli_submit_stores_token_entries_and_covers_the_cadence_period(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    let today = OffsetDateTime::now_utc().date();
    let monday = today
        .checked_sub(time::Duration::days(i64::from(
            today.weekday().number_days_from_monday(),
        )))
        .unwrap();
    sqlx::query("INSERT INTO cadences (organization_id, kind, starts_on, set_by) SELECT $1, 'weekly', $2, id FROM people WHERE email = 'nina@kestrel.example'")
        .bind(kestrel.organization_id)
        .bind(monday)
        .execute(&pool)
        .await
        .unwrap();

    let body =
        json!({"token_entries": [session_entry("claude-code:s1", &json!({"ticket": "KD-214"}))]});
    let (status, answer) = call(
        &kestrel.app,
        Method::POST,
        "/api/v1/token-sheets/submit",
        &kestrel.ana_cli,
        Some(body),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer["token_entries"][0]["local_id"], "claude-code:s1");
    assert_eq!(answer["token_entries"][0]["status"], "submitted");
    assert_eq!(answer["submitted"], 1);
    assert_eq!(answer["list_value"], "0.42");
    assert_eq!(answer["currency"], "USD");
    assert_eq!(answer["share_on_tickets_percent"], 100);
    assert_eq!(answer["token_sheets"][0]["starts_on"], monday.to_string());

    // Sending it again replaces it.
    let again = json!({"token_entries": [session_entry("claude-code:s1", &json!({"ticket": "KD-214", "summary": "Fixed coupon totals"}))]});
    let (status, _) = call(
        &kestrel.app,
        Method::POST,
        "/api/v1/token-sheets/submit",
        &kestrel.ana_cli,
        Some(again),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM token_entries WHERE organization_id = $1",
            kestrel.organization_id
        )
        .await
        .unwrap(),
        1
    );
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM token_sheets WHERE person_id = $1",
            kestrel.ana_id
        )
        .await
        .unwrap(),
        1
    );
    assert_eq!(
        count(&pool, "SELECT count(*) FROM token_sheet_entries tse JOIN token_sheets ts ON ts.id = tse.token_sheet_id WHERE ts.person_id = $1", kestrel.ana_id).await.unwrap(),
        1
    );

    let (status, context) = call(
        &kestrel.app,
        Method::GET,
        "/api/v1/me/context",
        &kestrel.ana_cli,
        None,
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(context["cadence"]["period_start"], monday.to_string());
    assert_eq!(context["cadence"]["late"], false);
    assert_eq!(context["rejected"], json!([]));
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_web_app_submits_drafts_and_an_empty_submit_reports_nothing(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    let draft = session_entry("manual:1", &json!({"ticket": "KD-214"}));
    let (status, stored) = call(
        &kestrel.app,
        Method::POST,
        "/api/v1/token-entries",
        &kestrel.ana,
        Some(draft),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::CREATED, "{stored}");
    assert_eq!(stored["status"], "draft");

    let (status, sheet) = call(
        &kestrel.app,
        Method::GET,
        "/api/v1/token-sheets/current",
        &kestrel.ana,
        None,
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK, "{sheet}");
    assert_eq!(sheet["person"], "ana");
    assert_eq!(sheet["cadence"], Value::Null);
    assert_eq!(sheet["token_entries"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        sheet["token_entries"][0]["splits"][0]["placed_by"],
        "by hand"
    );
    assert_eq!(sheet["list_value"], "0.42");
    assert_eq!(sheet["share_on_tickets"], 10_000);

    let (status, submitted) = call(
        &kestrel.app,
        Method::POST,
        "/api/v1/token-sheets/submit",
        &kestrel.ana,
        None,
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK, "{submitted}");
    assert_eq!(submitted["submitted"], 1);
    assert_eq!(submitted["token_entries"], json!([]));
    let (_, sheet) = call(
        &kestrel.app,
        Method::GET,
        "/api/v1/token-sheets/current",
        &kestrel.ana,
        None,
    )
    .await
    .unwrap();
    assert_eq!(sheet["token_entries"], json!([]));
    assert_eq!(sheet["token_sheets"].as_array().map(Vec::len), Some(1));

    // Nothing to report: the token sheet is there, with nothing on it.
    let (status, empty) = call(
        &kestrel.app,
        Method::POST,
        "/api/v1/token-sheets/submit",
        &kestrel.ana_cli,
        Some(json!({"token_entries": []})),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(empty["submitted"], 0);
    assert_eq!(empty["share_on_tickets_percent"], Value::Null);
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM token_sheets WHERE person_id = $1",
            kestrel.ana_id
        )
        .await
        .unwrap(),
        1
    );

    // Service tokens and other people cannot submit for Ana.
    let (status, _) = call(
        &kestrel.app,
        Method::POST,
        "/api/v1/token-sheets/submit",
        "tc_svc_nothing",
        None,
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_rejected_token_entry_comes_back_and_goes_again_once_fixed(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    let body = json!({"token_entries": [session_entry("claude-code:s2", &json!({}))]});
    let (_, answer) = call(
        &kestrel.app,
        Method::POST,
        "/api/v1/token-sheets/submit",
        &kestrel.ana_cli,
        Some(body),
    )
    .await
    .unwrap();
    let id = answer["token_entries"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let reject = json!({"reason": "This was the coupon bug: put it on KD-214"});
    let (status, rejected) = call(
        &kestrel.app,
        Method::POST,
        &format!("/api/v1/token-entries/{id}/reject"),
        &kestrel.people_manager,
        Some(reject),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK, "{rejected}");

    let (_, context) = call(
        &kestrel.app,
        Method::GET,
        "/api/v1/me/context",
        &kestrel.ana_cli,
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        context["rejected"],
        json!([{"local_id": "claude-code:s2", "reason": "This was the coupon bug: put it on KD-214"}])
    );
    let (_, sheet) = call(
        &kestrel.app,
        Method::GET,
        "/api/v1/token-sheets/current",
        &kestrel.ana,
        None,
    )
    .await
    .unwrap();
    assert_eq!(sheet["token_entries"][0]["status"], "rejected");
    assert_eq!(
        sheet["token_entries"][0]["rejected_reason"],
        "This was the coupon bug: put it on KD-214"
    );

    // Someone else cannot fix Ana's token entry.
    let tags = json!({"tags": {"ticket": "KD-214"}});
    let (status, _) = call(
        &kestrel.app,
        Method::PATCH,
        &format!("/api/v1/token-entries/{id}/tags"),
        &kestrel.people_manager,
        Some(tags.clone()),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, fixed) = call(
        &kestrel.app,
        Method::PATCH,
        &format!("/api/v1/token-entries/{id}/tags"),
        &kestrel.ana,
        Some(tags),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK, "{fixed}");
    assert_eq!(fixed["status"], "draft");
    assert_eq!(fixed["splits"][0]["tags"]["ticket"], "KD-214");
    assert_eq!(fixed["splits"][0]["tags"]["project"], "storefront");
    assert_eq!(fixed["splits"][0]["tags"]["team"], "web");
    assert_eq!(fixed["splits"][0]["placed_by"], "by hand");

    let bad = json!({"splits": [{"share_percent": 60, "tags": {"ticket": "KD-214"}}, {"share_percent": 39}]});
    let (status, _) = call(
        &kestrel.app,
        Method::PUT,
        &format!("/api/v1/token-entries/{id}/splits"),
        &kestrel.ana,
        Some(bad),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let split = json!({"splits": [{"share_percent": 60, "tags": {"ticket": "KD-214"}}, {"share_percent": 40, "tags": {"ticket": "OPS-7"}}]});
    let (status, split) = call(
        &kestrel.app,
        Method::PUT,
        &format!("/api/v1/token-entries/{id}/splits"),
        &kestrel.ana,
        Some(split),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK, "{split}");
    assert_eq!(split["splits"].as_array().map(Vec::len), Some(2));
    assert_eq!(split["list_value"], "0.42");

    let (_, submitted) = call(
        &kestrel.app,
        Method::POST,
        "/api/v1/token-sheets/submit",
        &kestrel.ana,
        None,
    )
    .await
    .unwrap();
    assert_eq!(submitted["submitted"], 1);
    let status: String = sqlx::query_scalar("SELECT status FROM token_entries WHERE id = $1::uuid")
        .bind(&id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "submitted");
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_late_token_sheet_is_posted_to_its_closed_month(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    sqlx::query("INSERT INTO closed_periods (organization_id, period, closed_by, reporting_currency, approval, tag_map_version) SELECT $1, '2026-08-01', id, 'USD', false, 1 FROM people WHERE email = 'nina@kestrel.example'")
        .bind(kestrel.organization_id)
        .execute(&pool)
        .await
        .unwrap();
    let mut late = session_entry("claude-code:august", &json!({"ticket": "KD-214"}));
    late["started_at"] = json!("2026-08-20T09:00:00Z");
    late["ended_at"] = json!("2026-08-20T09:30:00Z");
    let (status, answer) = call(
        &kestrel.app,
        Method::POST,
        "/api/v1/token-sheets/submit",
        &kestrel.ana_cli,
        Some(json!({"token_entries": [late]})),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer["late"], 1);
    assert_eq!(answer["token_entries"][0]["status"], "locked");
    let (amount, is_late): (i64, bool) =
        sqlx::query_as("SELECT amount_nanos, late FROM adjustments WHERE organization_id = $1")
            .bind(kestrel.organization_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(amount, 420_000_000);
    assert!(is_late);
    // Sending it again cannot change the closed month.
    let mut again = session_entry("claude-code:august", &json!({}));
    again["started_at"] = json!("2026-08-20T09:00:00Z");
    again["ended_at"] = json!("2026-08-20T09:30:00Z");
    let (status, _) = call(
        &kestrel.app,
        Method::POST,
        "/api/v1/token-sheets/submit",
        &kestrel.ana_cli,
        Some(json!({"token_entries": [again]})),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::CONFLICT);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_controller_asks_everyone_to_submit_before_the_close(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    let draft = session_entry("manual:2", &json!({}));
    let (status, _) = call(
        &kestrel.app,
        Method::POST,
        "/api/v1/token-entries",
        &kestrel.ana,
        Some(draft),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::CREATED);
    let month = OffsetDateTime::now_utc()
        .checked_sub(time::Duration::hours(1))
        .unwrap();
    let period = format!("{}-{:02}", month.year(), u8::from(month.month()));

    let (status, _) = call(
        &kestrel.app,
        Method::POST,
        "/api/v1/token-sheets/remind",
        &kestrel.people_manager,
        Some(json!({"period": period})),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, reminded) = call(
        &kestrel.app,
        Method::POST,
        "/api/v1/token-sheets/remind",
        &kestrel.controller,
        Some(json!({"period": period})),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK, "{reminded}");
    assert_eq!(reminded["reminded"], 1);

    let (_, sheet) = call(
        &kestrel.app,
        Method::GET,
        "/api/v1/token-sheets/current",
        &kestrel.ana,
        None,
    )
    .await
    .unwrap();
    assert_eq!(sheet["asked_to_submit"]["period"], period);
    assert_eq!(sheet["asked_to_submit"]["asked_by"], Value::Null);
    let (_, context) = call(
        &kestrel.app,
        Method::GET,
        "/api/v1/me/context",
        &kestrel.ana_cli,
        None,
    )
    .await
    .unwrap();
    assert_eq!(context["asked_to_submit"]["period"], period);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_ticket_key_still_unknown_after_a_sync_shows_as_unknown_ticket(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    let org = kestrel.organization_id;
    let branch = json!({"signals": {"branch": "KD-999-cart-fix"}});
    let body = json!({"token_entries": [session_entry("claude-code:s3", &branch)]});
    let (status, answer) = call(
        &kestrel.app,
        Method::POST,
        "/api/v1/token-sheets/submit",
        &kestrel.ana_cli,
        Some(body),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK, "{answer}");
    let placed: (String, bool) = sqlx::query_as("SELECT st.value, s.ticket_not_synced FROM splits s JOIN split_tags st ON st.split_id = s.id AND st.name = 'ticket'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(placed, ("KD-999".to_owned(), true));

    // A sync that covers the time the token entry arrived does not find it.
    let system: Uuid = sqlx::query_scalar("INSERT INTO ticket_systems (organization_id, kind, site, sealed_credentials, key_hint) VALUES ($1, 'jira', 'kestrel.atlassian.net', 'sealed', 'abcd') RETURNING id")
        .bind(org)
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO ticket_system_projects (organization_id, ticket_system_id, external_project, external_name, project_id, synced_until) SELECT $1, $2, 'KD', 'Storefront', id, now() + interval '1 minute' FROM projects WHERE organization_id = $1")
        .bind(org)
        .bind(system)
        .execute(&pool)
        .await
        .unwrap();
    tokencontroller_server::ingest::store::place_unsynced(&pool, org)
        .await
        .unwrap();
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM missing_tickets WHERE organization_id = $1 AND key = 'KD-999'",
            org
        )
        .await
        .unwrap(),
        1
    );

    let (_, sheet) = call(
        &kestrel.app,
        Method::GET,
        "/api/v1/token-sheets/current",
        &kestrel.ana,
        None,
    )
    .await
    .unwrap();
    let entries = sheet["token_entries"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert_eq!(entries.len(), 1, "{sheet}");
    let split = &entries[0]["splits"][0];
    assert_eq!(split["unknown_ticket"], "KD-999");
    assert_eq!(split["tags"].get("ticket"), None);
    assert_eq!(split["placed_by"], "no ticket");

    // Once the ticket syncs, the token entry is placed on it again.
    sqlx::query("INSERT INTO tickets (organization_id, key, project_id, title) SELECT $1, 'KD-999', id, 'Cart fix' FROM projects WHERE organization_id = $1")
        .bind(org)
        .execute(&pool)
        .await
        .unwrap();
    tokencontroller_server::ingest::store::place_unsynced(&pool, org)
        .await
        .unwrap();
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM missing_tickets WHERE organization_id = $1",
            org
        )
        .await
        .unwrap(),
        0
    );
    let after: (String, Option<String>) = sqlx::query_as("SELECT st.value, s.unknown_ticket FROM splits s JOIN split_tags st ON st.split_id = s.id AND st.name = 'ticket'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(after, ("KD-999".to_owned(), None));
}
