//! Bills, the monthly close and reports against a real Postgres database
//! (docs: Account > Bills, Monthly close, Approval; Report > The cost sheet,
//! Reading reports, Raw export): a bill and its reconciliation, the close
//! and its locks, adjustments, the cost sheet and its link for finance, the
//! cost report with its share view and CSV, and the raw export.

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

const KEY: &str = "0123456789abcdef0123456789abcdef";

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

/// Kestrel Digital in USD: Nina the controller, Priya a people manager over
/// `team: web`, and Ana in `team: web` with two September sessions sent from
/// the CLI: 0.42 on KD-214 and 1.08 on OPS-7.
struct Kestrel {
    app: Router,
    organization_id: Uuid,
    controller: String,
    people_manager: String,
    ana: String,
}

/// An answer: its status, content type and body.
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
    sqlx::query("INSERT INTO tickets (organization_id, key, project_id, title) VALUES ($1, 'KD-214', $2, 'Cart totals'), ($1, 'OPS-7', $2, 'Deploy script')")
        .bind(org)
        .bind(project)
        .execute(pool)
        .await?;
    let kestrel = Kestrel {
        app,
        organization_id: org,
        controller: session(pool, created.person_id, secret::SESSION, "browser").await?,
        people_manager: session(pool, people_manager, secret::SESSION, "browser").await?,
        ana: session(pool, ana_id, secret::SESSION, "browser").await?,
    };
    let ana_cli = session(pool, ana_id, secret::CLI, "local_tool").await?;
    let sent = call(
        &kestrel.app,
        Method::POST,
        "/api/v1/token-sheets/submit",
        &ana_cli,
        Some(json!({"token_entries": [
            september("claude-code:one", "2026-09-10T09:00:00Z", "0.42", "KD-214"),
            september("claude-code:two", "2026-09-24T14:00:00Z", "1.08", "OPS-7"),
        ]})),
    )
    .await?;
    if sent.status != StatusCode::OK {
        return Err(format!("submit failed: {}", sent.text()).into());
    }
    Ok(kestrel)
}

/// A Claude Code session in September 2026, as `tc submit` sends it.
fn september(local_id: &str, started_at: &str, cost: &str, ticket: &str) -> Value {
    json!({
        "agent_harness": "claude-code",
        "model": "claude-opus-5",
        "started_at": started_at,
        "ended_at": started_at.replace("00:00Z", "30:00Z"),
        "tokens": {"input": 12000, "output": 3000},
        "reported_cost": cost,
        "summary": format!("Work on {ticket}"),
        "ticket": ticket,
        "local_id": local_id
    })
}

async fn call(
    app: &Router,
    method: Method,
    path: &str,
    token: &str,
    body: Option<Value>,
) -> TestResult<Answer> {
    let mut request = Request::builder().method(method).uri(path);
    if token.starts_with(secret::SESSION) {
        request = request.header(header::COOKIE, format!("tc_session={token}"));
    } else if !token.is_empty() {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let request = match body {
        Some(body) => request
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&body)?))?,
        None => request.body(Body::empty())?,
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

async fn post_bill(kestrel: &Kestrel, net: &str) -> TestResult<Answer> {
    call(
        &kestrel.app,
        Method::POST,
        "/api/v1/bills",
        &kestrel.controller,
        Some(json!({
            "period": "2026-09",
            "provider": "anthropic",
            "cost_source": "usage_at_api_rates",
            "net": net,
            "currency": "USD"
        })),
    )
    .await
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_bill_is_reconciled_against_its_token_entries(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    let first = post_bill(&kestrel, "10.00").await.unwrap();
    assert_eq!(first.status, StatusCode::CREATED, "{}", first.text());
    // The same figures again change nothing; new figures are a new revision.
    let same = post_bill(&kestrel, "10.00").await.unwrap();
    assert_eq!(same.status, StatusCode::OK, "{}", same.text());
    let revised = post_bill(&kestrel, "12.50").await.unwrap();
    assert_eq!(revised.status, StatusCode::CREATED, "{}", revised.text());
    let revisions: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM bill_revisions r JOIN bills b ON b.id = r.bill_id WHERE b.organization_id = $1",
    )
    .bind(kestrel.organization_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(revisions, 2);

    let reconciliation = call(
        &kestrel.app,
        Method::GET,
        "/api/v1/reconciliation?period=2026-09",
        &kestrel.controller,
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        reconciliation.status,
        StatusCode::OK,
        "{}",
        reconciliation.text()
    );
    let reconciliation = reconciliation.json();
    let bill = &reconciliation["bills"][0];
    assert_eq!(bill["net"], "12.50", "{reconciliation}");
    assert_eq!(bill["token_entries"], "1.50", "{reconciliation}");
    assert_eq!(bill["residual"], "11.00", "{reconciliation}");

    // Bills are for controllers only.
    let refused = call(
        &kestrel.app,
        Method::GET,
        "/api/v1/reconciliation?period=2026-09",
        &kestrel.ana,
        None,
    )
    .await
    .unwrap();
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn closing_a_month_locks_it_and_keeps_its_cost_sheet(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    let bill = post_bill(&kestrel, "10.00").await.unwrap();
    assert_eq!(bill.status, StatusCode::CREATED, "{}", bill.text());

    // Only a controller closes, and only a month that has ended.
    let not_yours = call(
        &kestrel.app,
        Method::POST,
        "/api/v1/periods/2026-09/close",
        &kestrel.people_manager,
        None,
    )
    .await
    .unwrap();
    assert_eq!(not_yours.status, StatusCode::FORBIDDEN);
    let running = time::OffsetDateTime::now_utc();
    let running = format!(
        "/api/v1/periods/{}-{:02}/close",
        running.year(),
        u8::from(running.month())
    );
    let too_early = call(
        &kestrel.app,
        Method::POST,
        &running,
        &kestrel.controller,
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        too_early.status,
        StatusCode::CONFLICT,
        "{}",
        too_early.text()
    );

    let closed = call(
        &kestrel.app,
        Method::POST,
        "/api/v1/periods/2026-09/close",
        &kestrel.controller,
        None,
    )
    .await
    .unwrap();
    assert_eq!(closed.status, StatusCode::OK, "{}", closed.text());
    let sheet = closed.json();
    assert_eq!(sheet["closed"], true, "{sheet}");
    assert_eq!(sheet["total"], "10.00", "{sheet}");
    let again = call(
        &kestrel.app,
        Method::POST,
        "/api/v1/periods/2026-09/close",
        &kestrel.controller,
        None,
    )
    .await
    .unwrap();
    assert_eq!(again.status, StatusCode::CONFLICT, "{}", again.text());

    // Every counted token entry is locked, and the database refuses changes.
    let unlocked: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM token_entries WHERE organization_id = $1 AND status <> 'locked'",
    )
    .bind(kestrel.organization_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(unlocked, 0);
    let changed =
        sqlx::query("UPDATE token_entries SET summary = 'changed' WHERE organization_id = $1")
            .bind(kestrel.organization_id)
            .execute(&pool)
            .await;
    assert!(changed.is_err(), "a locked token entry changed");
    let retagged = sqlx::query(
        "UPDATE split_tags SET value = 'other' FROM splits s JOIN token_entries e ON e.id = s.token_entry_id
         WHERE split_tags.split_id = s.id AND e.organization_id = $1",
    )
    .bind(kestrel.organization_id)
    .execute(&pool)
    .await;
    assert!(retagged.is_err(), "a locked split's tags changed");

    // The cost sheet reads the same afterwards, as JSON and as CSV.
    let stored = call(
        &kestrel.app,
        Method::GET,
        "/api/v1/periods/2026-09/cost-sheet",
        &kestrel.controller,
        None,
    )
    .await
    .unwrap();
    assert_eq!(stored.status, StatusCode::OK, "{}", stored.text());
    assert_eq!(stored.json()["total"], "10.00");
    let csv = call(
        &kestrel.app,
        Method::GET,
        "/api/v1/periods/2026-09/cost-sheet?format=csv",
        &kestrel.controller,
        None,
    )
    .await
    .unwrap();
    assert_eq!(csv.status, StatusCode::OK, "{}", csv.text());
    assert!(
        csv.content_type.starts_with("text/csv"),
        "{}",
        csv.content_type
    );
    assert!(csv.text().lines().count() > 1, "{}", csv.text());
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn an_adjustment_goes_to_a_closed_month_only(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    post_bill(&kestrel, "10.00").await.unwrap();
    let adjustment = json!({"amount": "-1.25", "reason": "Provider credit for an outage", "tags": {"team": "web"}});
    let open = call(
        &kestrel.app,
        Method::POST,
        "/api/v1/periods/2026-09/adjustments",
        &kestrel.controller,
        Some(adjustment.clone()),
    )
    .await
    .unwrap();
    assert_eq!(open.status, StatusCode::CONFLICT, "{}", open.text());
    call(
        &kestrel.app,
        Method::POST,
        "/api/v1/periods/2026-09/close",
        &kestrel.controller,
        None,
    )
    .await
    .unwrap();
    let posted = call(
        &kestrel.app,
        Method::POST,
        "/api/v1/periods/2026-09/adjustments",
        &kestrel.controller,
        Some(adjustment),
    )
    .await
    .unwrap();
    assert_eq!(posted.status, StatusCode::CREATED, "{}", posted.text());
    let sheet = posted.json();
    assert_eq!(sheet["total"], "8.75", "{sheet}");
    assert_eq!(
        sheet["adjustments"][0]["reason"], "Provider credit for an outage",
        "{sheet}"
    );
    // A reason is required, and an adjustment is never zero.
    for bad in [
        json!({"amount": "-1.00", "reason": ""}),
        json!({"amount": "0", "reason": "Nothing"}),
    ] {
        let refused = call(
            &kestrel.app,
            Method::POST,
            "/api/v1/periods/2026-09/adjustments",
            &kestrel.controller,
            Some(bad),
        )
        .await
        .unwrap();
        assert_eq!(
            refused.status,
            StatusCode::BAD_REQUEST,
            "{}",
            refused.text()
        );
    }
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn finance_reads_the_cost_sheet_through_a_link_until_it_is_revoked(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    post_bill(&kestrel, "10.00").await.unwrap();
    call(
        &kestrel.app,
        Method::POST,
        "/api/v1/periods/2026-09/close",
        &kestrel.controller,
        None,
    )
    .await
    .unwrap();
    let link = call(
        &kestrel.app,
        Method::POST,
        "/api/v1/periods/2026-09/cost-sheet/links",
        &kestrel.controller,
        None,
    )
    .await
    .unwrap();
    assert_eq!(link.status, StatusCode::CREATED, "{}", link.text());
    let link = link.json();
    // The address opens the web app's page, which reads the API with the
    // same secret.
    let url = link["url"].as_str().unwrap_or_default().to_owned();
    let secret = url
        .strip_prefix("http://localhost:8080/shared/cost-sheet/")
        .unwrap_or_default()
        .to_owned();
    assert_eq!(secret.len(), 64, "{link}");
    let path = format!("/api/v1/shared/cost-sheets/{secret}");
    let shared = call(&kestrel.app, Method::GET, &path, "", None)
        .await
        .unwrap();
    assert_eq!(shared.status, StatusCode::OK, "{}", shared.text());
    assert_eq!(shared.json()["total"], "10.00");
    assert_eq!(shared.json()["organization"], "Kestrel Digital");
    let csv = call(
        &kestrel.app,
        Method::GET,
        &format!("{path}?format=csv"),
        "",
        None,
    )
    .await
    .unwrap();
    assert!(
        csv.content_type.starts_with("text/csv"),
        "{}",
        csv.content_type
    );
    // The list never shows the address again.
    let listed = call(
        &kestrel.app,
        Method::GET,
        "/api/v1/periods/2026-09/cost-sheet/links",
        &kestrel.controller,
        None,
    )
    .await
    .unwrap();
    assert_eq!(listed.json()[0]["url"], Value::Null, "{}", listed.text());
    let id = link["id"].as_str().unwrap_or_default().to_owned();
    let revoked = call(
        &kestrel.app,
        Method::DELETE,
        &format!("/api/v1/cost-sheet-links/{id}"),
        &kestrel.controller,
        None,
    )
    .await
    .unwrap();
    assert_eq!(revoked.status, StatusCode::NO_CONTENT, "{}", revoked.text());
    let gone = call(&kestrel.app, Method::GET, &path, "", None)
        .await
        .unwrap();
    assert_eq!(gone.status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn with_approval_on_a_month_closes_only_once_everything_is_approved(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    sqlx::query("UPDATE organizations SET approval = true WHERE id = $1")
        .bind(kestrel.organization_id)
        .execute(&pool)
        .await
        .unwrap();
    post_bill(&kestrel, "10.00").await.unwrap();
    let waiting = call(
        &kestrel.app,
        Method::POST,
        "/api/v1/periods/2026-09/close",
        &kestrel.controller,
        None,
    )
    .await
    .unwrap();
    assert_eq!(waiting.status, StatusCode::CONFLICT, "{}", waiting.text());
    let approved = call(
        &kestrel.app,
        Method::POST,
        "/api/v1/token-entries/approve",
        &kestrel.people_manager,
        Some(json!({"period": "2026-09", "filter": "team:web"})),
    )
    .await
    .unwrap();
    assert_eq!(approved.status, StatusCode::OK, "{}", approved.text());
    let closed = call(
        &kestrel.app,
        Method::POST,
        "/api/v1/periods/2026-09/close",
        &kestrel.controller,
        None,
    )
    .await
    .unwrap();
    assert_eq!(closed.status, StatusCode::OK, "{}", closed.text());
    assert_eq!(closed.json()["approval"], true);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_cost_report_groups_by_a_tag_with_shares_and_csv(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    // Joe is in team data, outside Priya's filter, with 0.50 on OPS-7.
    let joe = sqlx::query_scalar::<_, Uuid>(
        r#"INSERT INTO people (organization_id, email, name, role, tags, joined_at)
           VALUES ($1, 'joe@kestrel.example', 'Joe', 'agent_manager', '{"team": "data"}', now()) RETURNING id"#,
    )
    .bind(kestrel.organization_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let joe_cli = session(&pool, joe, secret::CLI, "local_tool")
        .await
        .unwrap();
    let sent = call(
        &kestrel.app,
        Method::POST,
        "/api/v1/token-sheets/submit",
        &joe_cli,
        Some(json!({"token_entries": [september("claude-code:joe", "2026-09-15T10:00:00Z", "0.50", "OPS-7")]})),
    )
    .await
    .unwrap();
    assert_eq!(sent.status, StatusCode::OK, "{}", sent.text());

    let report = call(
        &kestrel.app,
        Method::GET,
        "/api/v1/reports/cost?period=2026-09&by=ticket",
        &kestrel.controller,
        None,
    )
    .await
    .unwrap();
    assert_eq!(report.status, StatusCode::OK, "{}", report.text());
    let report = report.json();
    assert_eq!(report["total"], "2.00", "{report}");
    assert_eq!(report["rows"][0]["value"], "OPS-7", "{report}");
    assert_eq!(report["rows"][0]["amount"], "1.58", "{report}");
    assert_eq!(report["rows"][0]["token_entries"], 2, "{report}");
    assert_eq!(report["rows"][1]["amount"], "0.42", "{report}");

    // The share view adds up to exactly 100 percent.
    let shares = call(
        &kestrel.app,
        Method::GET,
        "/api/v1/reports/cost?period=2026-09&by=ticket&view=share",
        &kestrel.controller,
        None,
    )
    .await
    .unwrap();
    let shares = shares.json();
    assert_eq!(shares["rows"][0]["share"], 7900, "{shares}");
    assert_eq!(shares["rows"][1]["share"], 2100, "{shares}");

    let csv = call(
        &kestrel.app,
        Method::GET,
        "/api/v1/reports/cost?period=2026-09&by=ticket&format=csv",
        &kestrel.controller,
        None,
    )
    .await
    .unwrap();
    assert!(
        csv.content_type.starts_with("text/csv"),
        "{}",
        csv.content_type
    );
    let text = csv.text();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines.first(),
        Some(&"period,ticket,amount,currency,share_percent,token_entries,unknown_model,adjustment")
    );
    assert_eq!(lines.get(1), Some(&"2026-09,OPS-7,1.58,USD,,2,0,no"));
    assert_eq!(lines.last(), Some(&"2026-09,total,2.00,USD,,,,"));

    // Each role sees the cost it covers: Priya team web, Joe his own.
    for (token, total) in [
        (&kestrel.people_manager, "1.50"),
        (&kestrel.ana, "1.50"),
        (&joe_cli, "0.50"),
    ] {
        let seen = call(
            &kestrel.app,
            Method::GET,
            "/api/v1/reports/cost?period=2026-09&by=team",
            token,
            None,
        )
        .await
        .unwrap();
        assert_eq!(seen.status, StatusCode::OK, "{}", seen.text());
        assert_eq!(seen.json()["total"], total, "{}", seen.text());
    }
    let by_team = call(
        &kestrel.app,
        Method::GET,
        "/api/v1/reports/cost?period=2026-09&by=team",
        &kestrel.controller,
        None,
    )
    .await
    .unwrap()
    .json();
    let teams: Vec<(&str, &str)> = by_team["rows"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .map(|row| {
                    (
                        row["value"].as_str().unwrap_or("none"),
                        row["amount"].as_str().unwrap_or_default(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(teams, vec![("web", "1.50"), ("data", "0.50")], "{by_team}");
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_raw_export_holds_everything_and_only_controllers_get_it(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    post_bill(&kestrel, "10.00").await.unwrap();
    call(
        &kestrel.app,
        Method::POST,
        "/api/v1/periods/2026-09/close",
        &kestrel.controller,
        None,
    )
    .await
    .unwrap();
    let link = call(
        &kestrel.app,
        Method::POST,
        "/api/v1/periods/2026-09/cost-sheet/links",
        &kestrel.controller,
        None,
    )
    .await
    .unwrap();
    assert_eq!(link.status, StatusCode::CREATED, "{}", link.text());
    let refused = call(
        &kestrel.app,
        Method::GET,
        "/api/v1/exports/raw",
        &kestrel.ana,
        None,
    )
    .await
    .unwrap();
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    let export = call(
        &kestrel.app,
        Method::GET,
        "/api/v1/exports/raw",
        &kestrel.controller,
        None,
    )
    .await
    .unwrap();
    assert_eq!(export.status, StatusCode::OK, "{}", export.text());
    assert_eq!(export.content_type, "application/zip");
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(export.body)).unwrap();
    let names: Vec<String> = zip
        .file_names()
        .filter_map(Result::ok)
        .map(std::borrow::Cow::into_owned)
        .collect();
    for name in [
        "manifest.json",
        "organization.json",
        "people.json",
        "bills.json",
        "periods.json",
        "tickets.json",
        "token_entries.json",
        "token_sheets.json",
        "adjustments.json",
    ] {
        assert!(
            names.iter().any(|file| file == name),
            "{name} missing from {names:?}"
        );
    }
    let mut entries = String::new();
    zip.by_name("token_entries.json")
        .unwrap()
        .read_to_string(&mut entries)
        .unwrap();
    let entries: Value = serde_json::from_str(&entries).unwrap();
    assert_eq!(entries.as_array().map(Vec::len), Some(2));
    assert_eq!(
        entries[0]["splits"][0]["tags"]["ticket"], "KD-214",
        "{entries:#}"
    );
    assert!(entries[0]["messages"].is_array(), "{entries:#}");
    let mut bills = String::new();
    zip.by_name("bills.json")
        .unwrap()
        .read_to_string(&mut bills)
        .unwrap();
    let bills: Value = serde_json::from_str(&bills).unwrap();
    assert_eq!(
        bills[0]["revisions"].as_array().map(Vec::len),
        Some(1),
        "{bills:#}"
    );
    // No secret leaves: the finance links keep their hash to themselves.
    let mut links = String::new();
    zip.by_name("cost_sheet_links.json")
        .unwrap()
        .read_to_string(&mut links)
        .unwrap();
    assert!(links.contains("created_at"), "{links}");
    assert!(!links.contains("secret_hash"), "{links}");
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn every_table_downloads_as_csv(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    let bill = post_bill(&kestrel, "10.00").await.unwrap().json();
    let bill_id = bill["id"].as_str().unwrap_or_default().to_owned();
    call(
        &kestrel.app,
        Method::POST,
        "/api/v1/periods/2026-09/close",
        &kestrel.controller,
        None,
    )
    .await
    .unwrap();
    call(
        &kestrel.app,
        Method::POST,
        "/api/v1/periods/2026-09/cost-sheet/links",
        &kestrel.controller,
        None,
    )
    .await
    .unwrap();
    let tables = [
        (
            "/api/v1/bills?period=2026-09&format=csv",
            "period,provider,cost_source,revision",
            "2026-09,anthropic,usage_at_api_rates,1,USD,10.00",
        ),
        (
            &format!("/api/v1/bills/{bill_id}?format=csv"),
            "period,provider",
            "2026-09,anthropic",
        ),
        (
            "/api/v1/reconciliation?period=2026-09&format=csv",
            "period,provider,cost_source,currency,gross,discount,credit,net,token_entries,residual",
            "2026-09,anthropic,usage_at_api_rates,USD,10.00,0.00,0.00,10.00,1.50,8.50",
        ),
        (
            "/api/v1/periods?format=csv",
            "period,ended,closed,closed_at,closed_by",
            "2026-09,yes,yes,",
        ),
        (
            "/api/v1/periods/2026-09/cost-sheet/links?format=csv",
            "period,created_by,created_at,revoked_at",
            "2026-09,,",
        ),
        (
            "/api/v1/token-sheets/late?format=csv",
            "person,email,period_start,period_end",
            "",
        ),
    ];
    for (path, header, first_row) in tables {
        let answer = call(&kestrel.app, Method::GET, path, &kestrel.controller, None)
            .await
            .unwrap();
        assert_eq!(answer.status, StatusCode::OK, "{path}: {}", answer.text());
        assert!(
            answer.content_type.starts_with("text/csv"),
            "{path}: {}",
            answer.content_type
        );
        let text = answer.text();
        let mut lines = text.lines();
        assert!(
            lines.next().is_some_and(|line| line.starts_with(header)),
            "{path}: {text}"
        );
        if !first_row.is_empty() {
            assert!(
                lines.any(|line| line.starts_with(first_row)),
                "{path}: {text}"
            );
        }
    }
    // The review list downloads its token entries.
    let review = call(
        &kestrel.app,
        Method::GET,
        "/api/v1/token-sheets/current?format=csv",
        &kestrel.ana,
        None,
    )
    .await
    .unwrap();
    assert_eq!(review.status, StatusCode::OK, "{}", review.text());
    assert!(
        review.text().starts_with("token_entry,started_at"),
        "{}",
        review.text()
    );
}

/// The September bill from Anthropic as a pull brings it: 10.00 USD, final
/// or not yet.
fn pulled(is_final: bool) -> TestResult<tokencontroller_server::bills::NewBill> {
    let usd = |cents| {
        tokencontroller_core::Money::from_minor_units(cents, tokencontroller_core::Currency::USD)
    };
    Ok(tokencontroller_server::bills::NewBill {
        period: tokencontroller_core::Period::parse("2026-09")?,
        provider: "anthropic".to_owned(),
        cost_source: tokencontroller_server::bills::CostSource::UsageAtApiRates,
        gross: usd(1000)?,
        discount: usd(0)?,
        credit: usd(0)?,
        net: usd(1000)?,
        exchange_rate: None,
        source: tokencontroller_server::bills::BillSource::Pulled,
        lines: Vec::new(),
        recorded_by: None,
        is_final,
    })
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_close_waits_for_a_bill_that_is_not_final(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    let early = tokencontroller_server::bills::record_bill(
        &pool,
        kestrel.organization_id,
        pulled(false).unwrap(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(early.changed);
    assert!(!early.bill.is_final);
    let view = call(
        &kestrel.app,
        Method::GET,
        "/api/v1/periods/2026-09",
        &kestrel.controller,
        None,
    )
    .await
    .unwrap()
    .json();
    assert_eq!(view["can_close"], false, "{view}");
    assert_eq!(
        view["open_items"]["bills_not_final"][0], "anthropic",
        "{view}"
    );
    let waiting = call(
        &kestrel.app,
        Method::POST,
        "/api/v1/periods/2026-09/close",
        &kestrel.controller,
        None,
    )
    .await
    .unwrap();
    assert_eq!(waiting.status, StatusCode::CONFLICT, "{}", waiting.text());
    assert!(waiting.text().contains("not final"), "{}", waiting.text());

    // The same figures, now final: no new revision, and the close goes ahead.
    let settled = tokencontroller_server::bills::record_bill(
        &pool,
        kestrel.organization_id,
        pulled(true).unwrap(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(settled.changed);
    assert!(settled.bill.is_final);
    assert_eq!(settled.bill.revision, 1);
    // A final bill never goes back.
    let again = tokencontroller_server::bills::record_bill(
        &pool,
        kestrel.organization_id,
        pulled(false).unwrap(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(!again.changed);
    assert!(again.bill.is_final);
    let closed = call(
        &kestrel.app,
        Method::POST,
        "/api/v1/periods/2026-09/close",
        &kestrel.controller,
        None,
    )
    .await
    .unwrap();
    assert_eq!(closed.status, StatusCode::OK, "{}", closed.text());
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_saved_report_says_who_can_remove_it(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    let save = |token: String, name: &str| {
        let app = kestrel.app.clone();
        let body = json!({"name": name, "filter": "team:web", "by": "ticket"});
        async move {
            call(
                &app,
                Method::POST,
                "/api/v1/reports/saved",
                &token,
                Some(body),
            )
            .await
        }
    };
    let ours = save(kestrel.controller.clone(), "Web by ticket")
        .await
        .unwrap();
    assert_eq!(ours.status, StatusCode::CREATED, "{}", ours.text());
    assert_eq!(ours.json()["can_remove"], true);
    let mine = save(kestrel.ana.clone(), "Mine").await.unwrap();
    assert_eq!(mine.status, StatusCode::CREATED, "{}", mine.text());
    let mine = mine.json();
    assert_eq!(mine["created_by"], "ana");
    assert_ne!(mine["created_by_id"], ours.json()["created_by_id"]);

    let list = |token: String| {
        let app = kestrel.app.clone();
        async move {
            call(&app, Method::GET, "/api/v1/reports/saved", &token, None)
                .await
                .map(|answer| answer.json())
        }
    };
    let can_remove = |list: &Value| -> Vec<(String, bool)> {
        list.as_array()
            .into_iter()
            .flatten()
            .map(|saved| {
                (
                    saved["name"].as_str().unwrap_or_default().to_owned(),
                    saved["can_remove"].as_bool().unwrap_or_default(),
                )
            })
            .collect()
    };
    let as_ana = list(kestrel.ana.clone()).await.unwrap();
    assert_eq!(
        can_remove(&as_ana),
        [
            ("Mine".to_owned(), true),
            ("Web by ticket".to_owned(), false)
        ]
    );
    assert_eq!(
        as_ana.pointer("/0/created_by_id"),
        Some(&mine["created_by_id"])
    );
    let as_controller = list(kestrel.controller.clone()).await.unwrap();
    assert_eq!(
        can_remove(&as_controller),
        [
            ("Mine".to_owned(), true),
            ("Web by ticket".to_owned(), true)
        ]
    );

    let remove = |token: &str, id: &Value| {
        let path = format!("/api/v1/reports/saved/{}", id.as_str().unwrap_or_default());
        let app = kestrel.app.clone();
        let token = token.to_owned();
        async move { call(&app, Method::DELETE, &path, &token, None).await }
    };
    let refused = remove(&kestrel.ana, &ours.json()["id"]).await.unwrap();
    assert_eq!(refused.status, StatusCode::NOT_FOUND);
    let removed = remove(&kestrel.ana, &mine["id"]).await.unwrap();
    assert_eq!(removed.status, StatusCode::NO_CONTENT);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn outliers_follow_the_status_and_currency_asked_for(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    // Three storefront tickets in August make the baseline: their median is
    // 0.50.
    sqlx::query(
        "INSERT INTO tickets (organization_id, key, project_id, title)
         SELECT $1, key, (SELECT id FROM projects WHERE organization_id = $1), key
         FROM unnest(ARRAY['KD-1', 'KD-2', 'KD-3']) AS key",
    )
    .bind(kestrel.organization_id)
    .execute(&pool)
    .await
    .unwrap();
    let ana: Uuid = sqlx::query_scalar("SELECT id FROM people WHERE email = 'ana@kestrel.example'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let ana_cli = session(&pool, ana, secret::CLI, "local_tool")
        .await
        .unwrap();
    let sent = call(
        &kestrel.app,
        Method::POST,
        "/api/v1/token-sheets/submit",
        &ana_cli,
        Some(json!({"token_entries": [
            september("claude-code:a1", "2026-08-03T09:00:00Z", "0.40", "KD-1"),
            september("claude-code:a2", "2026-08-04T09:00:00Z", "0.50", "KD-2"),
            september("claude-code:a3", "2026-08-05T09:00:00Z", "0.60", "KD-3"),
        ]})),
    )
    .await
    .unwrap();
    assert_eq!(sent.status, StatusCode::OK, "{}", sent.text());
    // 1.25 USD per euro on the last day of September: 1 USD is 0.80 EUR.
    sqlx::query(
        "INSERT INTO exchange_rates (day, currency, per_euro_nanos) VALUES ('2026-09-30', 'USD', 1250000000)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let outliers = |query: &str| {
        let path = format!("/api/v1/reports/outliers?period=2026-09{query}");
        let app = kestrel.app.clone();
        let token = kestrel.controller.clone();
        async move { call(&app, Method::GET, &path, &token, None).await }
    };
    let usd = outliers("").await.unwrap();
    assert_eq!(usd.status, StatusCode::OK, "{}", usd.text());
    let usd = usd.json();
    assert_eq!(usd["currency"], "USD");
    assert_eq!(usd["status"], "all");
    assert_eq!(usd["exchange_rate"], Value::Null);
    assert_eq!(usd["tickets"][0]["ticket"], "OPS-7");
    assert_eq!(usd["tickets"][0]["amount"], "1.08");
    assert_eq!(usd["tickets"][0]["baseline"], "0.50");
    assert_eq!(usd["tickets"][0]["ratio"], "2.2");
    assert_eq!(usd["tickets"][0]["similar_tickets"], 3);
    assert_eq!(usd["tickets"][1]["ticket"], "KD-214");
    assert_eq!(usd["tickets"][1]["direction"], "low");

    let eur = outliers("&currency=EUR").await.unwrap().json();
    assert_eq!(eur["currency"], "EUR");
    assert_eq!(eur["exchange_rate"], "0.8");
    assert_eq!(eur["tickets"][0]["amount"], "0.86");
    assert_eq!(eur["tickets"][0]["baseline"], "0.40");
    assert_eq!(eur["tickets"][0]["ratio"], "2.2");
    let csv = outliers("&currency=EUR&format=csv").await.unwrap().text();
    assert!(
        csv.contains("2026-09,OPS-7,storefront,0.86,0.40,EUR,2.2,high"),
        "{csv}"
    );

    // With approval asked for, nothing submitted counts, in the period or
    // in the baseline.
    let approved = outliers("&status=approved").await.unwrap().json();
    assert_eq!(approved["status"], "approved");
    assert_eq!(approved["tickets"], json!([]));
    assert_eq!(approved["without_baseline"], 0);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn everyone_lists_periods_and_open_items_name_their_currency(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    let periods = call(
        &kestrel.app,
        Method::GET,
        "/api/v1/periods",
        &kestrel.ana,
        None,
    )
    .await
    .unwrap();
    assert_eq!(periods.status, StatusCode::OK, "{}", periods.text());
    assert!(
        periods
            .json()
            .as_array()
            .is_some_and(|all| all.iter().any(|period| period["period"] == "2026-09"))
    );
    let september = call(
        &kestrel.app,
        Method::GET,
        "/api/v1/periods/2026-09",
        &kestrel.controller,
        None,
    )
    .await
    .unwrap()
    .json();
    assert_eq!(september["open_items"]["currency"], "USD");
    assert_eq!(september["open_items"]["organization_overhead"], "0.00");
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_bill_in_a_currency_that_cannot_be_read_says_so(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    let answer = call(
        &kestrel.app,
        Method::POST,
        "/api/v1/bills",
        &kestrel.controller,
        Some(json!({
            "period": "2026-09",
            "provider": "anthropic",
            "cost_source": "usage_at_api_rates",
            "net": "1.50",
            "currency": "dollars"
        })),
    )
    .await
    .unwrap();
    assert_eq!(answer.status, StatusCode::BAD_REQUEST);
    assert_eq!(
        answer.json()["message"],
        "\"dollars\" is not a currency code: use three capital letters, such as EUR"
    );
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_token_entries_table_lists_one_person(pool: PgPool) {
    let kestrel = kestrel(&pool).await.unwrap();
    let ana: Uuid = sqlx::query_scalar("SELECT id FROM people WHERE email = 'ana@kestrel.example'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let list = |person: Uuid| {
        let path = format!("/api/v1/reports/token-entries?period=2026-09&person={person}");
        let app = kestrel.app.clone();
        let token = kestrel.controller.clone();
        async move { call(&app, Method::GET, &path, &token, None).await }
    };
    let hers = list(ana).await.unwrap().json();
    assert_eq!(hers["total"], 2);
    assert_eq!(
        hers["token_entries"][0]["agent_manager_id"],
        ana.to_string()
    );
    assert_eq!(hers["token_entries"][0]["agent_manager"], "ana");
    let nobody = list(Uuid::nil()).await.unwrap().json();
    assert_eq!(nobody["total"], 0);
}
