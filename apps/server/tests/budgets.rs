//! Token budgets, token budget vs actual and token forecasts against a real
//! Postgres database (docs: Account > Token budgets and forecasts; Report >
//! Token budget vs actual).

use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use sqlx::PgPool;
use time::OffsetDateTime;
use time::macros::{date, datetime};
use tokencontroller_core::NoExtension;
use tokencontroller_server::budgets::embeddings::{EmbedError, Embedder};
use tokencontroller_server::budgets::similarity::Embedding;
use tokencontroller_server::budgets::{embed_pending, forecast};
use tokencontroller_server::reports::scope::Scope;
use tokencontroller_server::sign_in::secret;
use tokencontroller_server::{AppState, Config, router};
use tower::ServiceExt as _;
use uuid::Uuid;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

const KEY: &str = "0123456789abcdef0123456789abcdef";

fn test_config() -> TestResult<Config> {
    let vars: BTreeMap<&str, &str> = [
        ("DATABASE_URL", "postgres://unused"),
        ("TC_PUBLIC_URL", "http://localhost:8080"),
        ("TC_SECRET_KEY", KEY),
    ]
    .into();
    Ok(Config::from_lookup(|name| {
        vars.get(name).map(|value| (*value).to_owned())
    })?)
}

/// Calls the API as `person` and returns the status and the body's text.
async fn call_text(
    pool: &PgPool,
    method: &str,
    path: &str,
    person: Uuid,
    body: Option<Value>,
) -> TestResult<(StatusCode, String)> {
    let app = router(AppState::new(
        pool.clone(),
        test_config()?,
        Arc::new(NoExtension),
    ));
    let session = secret::generate(secret::SESSION, KEY)?;
    sqlx::query("INSERT INTO sessions (token_hash, person_id, client, expires_at) VALUES ($1, $2, 'browser', now() + interval '1 day')")
        .bind(&session.hash)
        .bind(person)
        .execute(pool)
        .await?;
    let builder = Request::builder()
        .method(method)
        .uri(path)
        .header(header::COOKIE, format!("tc_session={}", session.token));
    let request = match body {
        Some(body) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&body)?))?,
        None => builder.body(Body::empty())?,
    };
    let response = app.oneshot(request).await?;
    let status = response.status();
    let bytes = response.into_body().collect().await?.to_bytes();
    Ok((status, String::from_utf8(bytes.to_vec())?))
}

/// Calls the API as `person` and reads the answer as JSON.
async fn call(
    pool: &PgPool,
    method: &str,
    path: &str,
    person: Uuid,
    body: Option<Value>,
) -> TestResult<(StatusCode, Value)> {
    let (status, text) = call_text(pool, method, path, person, body).await?;
    let value = if text.is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&text)?
    };
    Ok((status, value))
}

/// The people of the example organization.
struct Org {
    id: Uuid,
    controller: Uuid,
    /// A people manager over `team: web`.
    web_lead: Uuid,
    /// A people manager over `client: acme`.
    account_manager: Uuid,
    /// An agent manager in `team: web`.
    ana: Uuid,
    /// An agent manager in `team: data`.
    bo: Uuid,
}

async fn person(
    pool: &PgPool,
    org: Uuid,
    email: &str,
    role: &str,
    tags: Value,
) -> TestResult<Uuid> {
    Ok(sqlx::query_scalar(
        "INSERT INTO people (organization_id, email, name, role, tags) VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(org)
    .bind(email)
    .bind(email.split('@').next().unwrap_or(email))
    .bind(role)
    .bind(tags)
    .fetch_one(pool)
    .await?)
}

async fn organization(pool: &PgPool, currency: &str) -> TestResult<Org> {
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO organizations (name, slug, reporting_currency) VALUES ('Acme Agency', 'acme-agency', $1) RETURNING id",
    )
    .bind(currency)
    .fetch_one(pool)
    .await?;
    tokencontroller_server::tags::create_defaults(&mut *pool.acquire().await?, id).await?;
    let controller = person(pool, id, "cleo@example.com", "controller", json!({})).await?;
    let web_lead = person(
        pool,
        id,
        "wes@example.com",
        "people_manager",
        json!({"team": "web"}),
    )
    .await?;
    let account_manager = person(pool, id, "amy@example.com", "people_manager", json!({})).await?;
    let ana = person(
        pool,
        id,
        "ana@example.com",
        "agent_manager",
        json!({"team": "web"}),
    )
    .await?;
    let bo = person(
        pool,
        id,
        "bo@example.com",
        "agent_manager",
        json!({"team": "data"}),
    )
    .await?;
    for (manager, filter) in [
        (web_lead, json!({"team": "web"})),
        (account_manager, json!({"client": "acme"})),
    ] {
        sqlx::query(
            "INSERT INTO people_manager_filters (organization_id, person_id, filter) VALUES ($1, $2, $3)",
        )
        .bind(id)
        .bind(manager)
            .bind(filter)
            .execute(pool)
            .await?;
    }
    let project: Uuid = sqlx::query_scalar(
        r#"INSERT INTO projects (organization_id, name, tags) VALUES ($1, 'acme-shop', '{"client": "acme"}') RETURNING id"#,
    )
    .bind(id)
    .fetch_one(pool)
    .await?;
    for (key, title, description) in [
        (
            "KD-214",
            Some("Cart totals wrong with coupon"),
            Some("The cart shows the wrong total when a coupon is applied"),
        ),
        ("KD-180", Some("Coupon totals"), None),
        ("KD-181", Some("Coupon rounding"), None),
        ("KD-182", None, None),
        ("KD-190", Some("Onboarding emails"), None),
        ("KD-300", None, None),
    ] {
        sqlx::query("INSERT INTO tickets (organization_id, key, project_id, title, description) VALUES ($1, $2, $3, $4, $5)")
            .bind(id)
            .bind(key)
            .bind(project)
            .bind(title)
            .bind(description)
            .execute(pool)
            .await?;
    }
    Ok(Org {
        id,
        controller,
        web_lead,
        account_manager,
        ana,
        bo,
    })
}

/// One token entry with one split.
struct Entry<'a> {
    person: Uuid,
    started_at: OffsetDateTime,
    status: &'a str,
    /// In the minor unit of `currency`; `None` for an unknown model.
    cents: Option<i64>,
    currency: &'a str,
    summary: &'a str,
    tags: &'a [(&'a str, &'a str)],
}

impl Default for Entry<'_> {
    fn default() -> Self {
        Self {
            person: Uuid::nil(),
            started_at: datetime!(2026-09-10 09:00 UTC),
            status: "submitted",
            cents: Some(100),
            currency: "USD",
            summary: "",
            tags: &[],
        }
    }
}

async fn entry(pool: &PgPool, org: Uuid, entry: Entry<'_>) -> TestResult<Uuid> {
    let nanos = entry
        .cents
        .map(|cents| cents.checked_mul(10_000_000).ok_or("too much"))
        .transpose()?;
    let ended_at = entry
        .started_at
        .checked_add(time::Duration::minutes(30))
        .ok_or("no end")?;
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO token_entries
               (organization_id, agent_manager_id, agent_harness, model, provider, started_at, ended_at,
                summary, on_token_sheet, door, status, list_value_nanos, currency)
           VALUES ($1, $2, 'claude-code', $3, 'anthropic', $4, $5, $6, true, 'token_sheet', 'submitted', $7, $8)
           RETURNING id",
    )
    .bind(org)
    .bind(entry.person)
    .bind(if nanos.is_some() { "claude-opus-5" } else { "mystery-1" })
    .bind(entry.started_at)
    .bind(ended_at)
    .bind(entry.summary)
    .bind(nanos)
    .bind(entry.currency)
    .fetch_one(pool)
    .await?;
    let split: Uuid = sqlx::query_scalar(
        "INSERT INTO splits (token_entry_id, position, method, list_value_nanos, placed_by)
         VALUES ($1, 0, 'whole', $2, 'by hand') RETURNING id",
    )
    .bind(id)
    .bind(nanos)
    .fetch_one(pool)
    .await?;
    for (name, value) in entry.tags {
        sqlx::query(
            "INSERT INTO split_tags (split_id, organization_id, name, value, source) VALUES ($1, $2, $3, $4, 'sent')",
        )
        .bind(split)
        .bind(org)
            .bind(name)
            .bind(value)
            .execute(pool)
            .await?;
    }
    if entry.status != "submitted" {
        sqlx::query("UPDATE token_entries SET status = $2 WHERE id = $1")
            .bind(id)
            .bind(entry.status)
            .execute(pool)
            .await?;
    }
    Ok(id)
}

async fn set_budget(pool: &PgPool, as_person: Uuid, body: Value) -> TestResult<String> {
    let (status, budget) =
        call(pool, "POST", "/api/v1/token-budgets", as_person, Some(body)).await?;
    if status != StatusCode::CREATED {
        return Err(format!("setting a token budget answered {status}: {budget}").into());
    }
    Ok(budget
        .get("id")
        .and_then(Value::as_str)
        .ok_or("no id")?
        .to_owned())
}

fn row<'a>(report: &'a Value, id: &str) -> Option<&'a Value> {
    report
        .get("rows")?
        .as_array()?
        .iter()
        .find(|row| row.pointer("/token_budget/id").and_then(Value::as_str) == Some(id))
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn people_managers_set_token_budgets_within_their_filters(pool: PgPool) {
    let org = organization(&pool, "USD").await.unwrap();
    let web = json!({"filter": {"team": "web"}, "period": "2026-09", "amount": "1500"});
    let (status, budget) = call(
        &pool,
        "POST",
        "/api/v1/token-budgets",
        org.web_lead,
        Some(web),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::CREATED, "{budget}");
    assert_eq!(budget["amount"], "1500.00");
    assert_eq!(budget["currency"], "USD");
    assert_eq!(budget["applies"], "2026-09");
    assert_eq!(budget["set_by"], "wes");
    let web_id = budget["id"].as_str().unwrap().to_owned();

    // Ana is tagged team: web, so a token budget on her lies within it.
    let ana = json!({"name": "Ana", "filter": {"person": "ana@example.com"}, "period": "2026-09", "every_month": true, "amount": "20"});
    let ana_id = set_budget(&pool, org.web_lead, ana).await.unwrap();
    for outside in [
        json!({"filter": {"team": "data"}, "amount": "10"}),
        json!({"filter": {"person": "bo@example.com"}, "amount": "10"}),
        json!({"filter": {"ticket": "KD-214"}, "amount": "10"}),
        json!({"filter": {}, "amount": "10"}),
    ] {
        let (status, _) = call(
            &pool,
            "POST",
            "/api/v1/token-budgets",
            org.web_lead,
            Some(outside.clone()),
        )
        .await
        .unwrap();
        assert_eq!(status, StatusCode::FORBIDDEN, "{outside}");
    }
    // KD-214 is in project acme-shop, which is client: acme.
    let ticket = json!({"filter": {"ticket": "KD-214"}, "amount": "45.50"});
    let ticket_id = set_budget(&pool, org.account_manager, ticket)
        .await
        .unwrap();
    let (status, _) = call(
        &pool,
        "POST",
        "/api/v1/token-budgets",
        org.ana,
        Some(json!({"filter": {"person": "ana@example.com"}, "amount": "10"})),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::FORBIDDEN);
    let whole = set_budget(
        &pool,
        org.controller,
        json!({"filter": {}, "amount": "9000"}),
    )
    .await
    .unwrap();

    for (wrong, why) in [
        (json!({"filter": {"team": "web"}, "amount": "0"}), "zero"),
        (
            json!({"filter": {"team": "web"}, "amount": "-5"}),
            "negative",
        ),
        (
            json!({"filter": {"team": "web"}, "amount": "lots"}),
            "not a number",
        ),
        (
            json!({"filter": {"team": "web"}, "every_month": true, "amount": "5"}),
            "no period",
        ),
        (
            json!({"filter": {"team": "web"}, "period": "Sept", "amount": "5"}),
            "no period",
        ),
        (
            json!({"filter": {"team": ""}, "amount": "5"}),
            "empty value",
        ),
    ] {
        let (status, body) = call(
            &pool,
            "POST",
            "/api/v1/token-budgets",
            org.web_lead,
            Some(wrong),
        )
        .await
        .unwrap();
        assert_eq!(status, StatusCode::BAD_REQUEST, "{why}: {body}");
    }
    let (_, body) = call(
        &pool,
        "POST",
        "/api/v1/token-budgets",
        org.web_lead,
        Some(json!({"filter": {"team": "web"}, "amount": "0"})),
    )
    .await
    .unwrap();
    assert_eq!(body["message"], "a token budget needs an amount above zero");

    let ids = |list: &Value| -> Vec<String> {
        let mut ids: Vec<String> = list
            .as_array()
            .unwrap()
            .iter()
            .map(|budget| budget["id"].as_str().unwrap().to_owned())
            .collect();
        ids.sort();
        ids
    };
    let sorted = |mut list: Vec<&String>| -> Vec<String> {
        list.sort();
        list.into_iter().cloned().collect()
    };
    let (_, all) = call(&pool, "GET", "/api/v1/token-budgets", org.controller, None)
        .await
        .unwrap();
    assert_eq!(
        ids(&all),
        sorted(vec![&web_id, &ana_id, &ticket_id, &whole])
    );
    let (_, lead) = call(&pool, "GET", "/api/v1/token-budgets", org.web_lead, None)
        .await
        .unwrap();
    assert_eq!(ids(&lead), sorted(vec![&web_id, &ana_id]));
    let (_, account) = call(
        &pool,
        "GET",
        "/api/v1/token-budgets",
        org.account_manager,
        None,
    )
    .await
    .unwrap();
    assert_eq!(ids(&account), std::slice::from_ref(&ticket_id));
    let (_, own) = call(&pool, "GET", "/api/v1/token-budgets", org.ana, None)
        .await
        .unwrap();
    assert_eq!(ids(&own), std::slice::from_ref(&ana_id));
    let (_, none) = call(&pool, "GET", "/api/v1/token-budgets", org.bo, None)
        .await
        .unwrap();
    assert_eq!(ids(&none), Vec::<String>::new());

    let (status, csv) = call_text(
        &pool,
        "GET",
        "/api/v1/token-budgets?format=csv",
        org.ana,
        None,
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK);
    let mut lines = csv.lines();
    assert_eq!(
        lines.next(),
        Some("token_budget,name,filter,applies,amount,currency,token_forecast,set_by,set_at")
    );
    let line = lines.next().unwrap();
    assert!(
        line.starts_with(&format!(
            "{ana_id},Ana,person:ana@example.com,every month from 2026-09,20.00,USD,,wes,"
        )),
        "{line}"
    );
    assert_eq!(lines.next(), None);

    let path = format!("/api/v1/token-budgets/{web_id}");
    let change = json!({"name": "Web", "filter": {"team": "web", "client": "acme"}, "period": "2026-10", "amount": "1200.5"});
    let (status, changed) = call(&pool, "PUT", &path, org.web_lead, Some(change))
        .await
        .unwrap();
    assert_eq!(status, StatusCode::OK, "{changed}");
    assert_eq!(changed["amount"], "1200.50");
    assert_eq!(changed["period"], "2026-10");
    assert_eq!(changed["filter"], json!({"team": "web", "client": "acme"}));
    let away = json!({"filter": {"team": "data"}, "amount": "5"});
    let (status, _) = call(&pool, "PUT", &path, org.web_lead, Some(away))
        .await
        .unwrap();
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(
        &pool,
        "PUT",
        &path,
        org.bo,
        Some(json!({"filter": {}, "amount": "5"})),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::FORBIDDEN);
    let ticket_path = format!("/api/v1/token-budgets/{ticket_id}");
    let (status, _) = call(&pool, "DELETE", &ticket_path, org.web_lead, None)
        .await
        .unwrap();
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = call(&pool, "DELETE", &path, org.web_lead, None)
        .await
        .unwrap();
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = call(&pool, "DELETE", &path, org.web_lead, None)
        .await
        .unwrap();
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, lead) = call(&pool, "GET", "/api/v1/token-budgets", org.web_lead, None)
        .await
        .unwrap();
    assert_eq!(ids(&lead), [ana_id]);
    let removed: Option<OffsetDateTime> =
        sqlx::query_scalar("SELECT removed_at FROM token_budgets WHERE id = $1")
            .bind(Uuid::parse_str(&web_id).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(removed.is_some());
}

/// September: web 10.00 (6.00 on KD-180, 4.00 on KD-181), data 7.50, qa on
/// an unknown model, and web cost that does not count. August, closed: 3.00
/// on KD-180 and a credit of 1.00 on it.
async fn september(pool: &PgPool, org: &Org) -> TestResult<()> {
    let web_180 = [("team", "web"), ("ticket", "KD-180")];
    let web_181 = [("team", "web"), ("ticket", "KD-181")];
    let web = [("team", "web")];
    let data = [("team", "data")];
    let qa = [("team", "qa")];
    for (person, status, cents, tags) in [
        (org.ana, "submitted", Some(600), &web_180[..]),
        (org.ana, "approved", Some(400), &web_181[..]),
        (org.ana, "draft", Some(10_000), &web[..]),
        (org.ana, "rejected", Some(5_000), &web[..]),
        (org.bo, "submitted", Some(750), &data[..]),
        (org.bo, "submitted", None, &qa[..]),
    ] {
        entry(
            pool,
            org.id,
            Entry {
                person,
                status,
                cents,
                tags,
                ..Entry::default()
            },
        )
        .await?;
    }
    entry(
        pool,
        org.id,
        Entry {
            person: org.ana,
            started_at: datetime!(2026-08-20 09:00 UTC),
            status: "locked",
            cents: Some(300),
            tags: &web_180,
            ..Entry::default()
        },
    )
    .await?;
    sqlx::query(
        "INSERT INTO closed_periods (organization_id, period, closed_by, reporting_currency, approval, tag_map_version) VALUES ($1, '2026-08-01', $2, 'USD', false, 1)",
    )
    .bind(org.id)
    .bind(org.controller)
    .execute(pool)
    .await?;
    sqlx::query(
        r#"INSERT INTO adjustments (organization_id, period, amount_nanos, currency, tags, reason, posted_by)
           VALUES ($1, '2026-08-01', -1000000000, 'USD', '{"ticket": "KD-180"}', 'Provider credit', $2)"#,
    )
    .bind(org.id)
    .bind(org.controller)
    .execute(pool)
    .await?;
    Ok(())
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn token_budget_vs_actual_pins_overrun_underrun_and_exactly_at(pool: PgPool) {
    let org = organization(&pool, "USD").await.unwrap();
    september(&pool, &org).await.unwrap();
    let c = org.controller;
    let at = set_budget(
        &pool,
        c,
        json!({"filter": {"team": "web"}, "period": "2026-09", "amount": "10"}),
    )
    .await
    .unwrap();
    let over = set_budget(
        &pool,
        c,
        json!({"filter": {"team": "data"}, "period": "2026-09", "amount": "5"}),
    )
    .await
    .unwrap();
    let nothing = set_budget(
        &pool,
        c,
        json!({"filter": {"client": "globex"}, "period": "2026-09", "amount": "100"}),
    )
    .await
    .unwrap();
    let monthly = set_budget(
        &pool,
        c,
        json!({"filter": {"person": "ana@example.com"}, "period": "2026-08", "every_month": true, "amount": "10.01"}),
    )
    .await
    .unwrap();
    let august = set_budget(
        &pool,
        c,
        json!({"filter": {"team": "web"}, "period": "2026-08", "amount": "1"}),
    )
    .await
    .unwrap();
    let later = set_budget(
        &pool,
        c,
        json!({"filter": {"team": "web"}, "period": "2026-10", "every_month": true, "amount": "1"}),
    )
    .await
    .unwrap();
    let all_time = set_budget(
        &pool,
        c,
        json!({"filter": {"ticket": "KD-180"}, "amount": "8"}),
    )
    .await
    .unwrap();
    let unknown = set_budget(
        &pool,
        c,
        json!({"filter": {"team": "qa"}, "period": "2026-09", "amount": "1"}),
    )
    .await
    .unwrap();

    let (status, report) = call(
        &pool,
        "GET",
        "/api/v1/reports/token-budgets?period=2026-09",
        c,
        None,
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["period"], "2026-09");
    assert_eq!(report["currency"], "USD");
    assert!(row(&report, &august).is_none());
    assert!(row(&report, &later).is_none());
    assert_eq!(report["rows"].as_array().unwrap().len(), 6);

    let at_row = row(&report, &at).unwrap();
    assert_eq!(at_row["amount"], "10.00");
    assert_eq!(at_row["actual"], "10.00");
    assert_eq!(at_row["difference"], "0.00");
    assert_eq!(at_row["outcome"], "at_token_budget");
    assert_eq!(at_row["token_entries"], 2);
    assert_eq!(at_row["actual_period"], "2026-09");

    let over_row = row(&report, &over).unwrap();
    assert_eq!(over_row["actual"], "7.50");
    assert_eq!(over_row["difference"], "2.50");
    assert_eq!(over_row["outcome"], "overrun");

    let nothing_row = row(&report, &nothing).unwrap();
    assert_eq!(nothing_row["actual"], "0.00");
    assert_eq!(nothing_row["difference"], "-100.00");
    assert_eq!(nothing_row["outcome"], "underrun");
    assert_eq!(nothing_row["token_entries"], 0);

    // Every month: September's actual against one month's amount.
    let monthly_row = row(&report, &monthly).unwrap();
    assert_eq!(monthly_row["actual"], "10.00");
    assert_eq!(monthly_row["difference"], "-0.01");
    assert_eq!(monthly_row["outcome"], "underrun");

    // All time: August's 3.00 and its 1.00 credit, and September's 6.00.
    let all_time_row = row(&report, &all_time).unwrap();
    assert_eq!(all_time_row["actual_period"], Value::Null);
    assert_eq!(all_time_row["actual"], "8.00");
    assert_eq!(all_time_row["outcome"], "at_token_budget");
    assert_eq!(all_time_row["token_entries"], 2);

    // Unknown models are counted, never valued as zero.
    let unknown_row = row(&report, &unknown).unwrap();
    assert_eq!(unknown_row["actual"], "0.00");
    assert_eq!(unknown_row["unknown_model_entries"], 1);
    assert_eq!(unknown_row["token_entries"], 1);
    assert_eq!(unknown_row["outcome"], Value::Null);

    // The largest overrun first, the largest underrun last.
    let order: Vec<&str> = report["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["difference"].as_str().unwrap())
        .collect();
    assert_eq!(order.first(), Some(&"2.50"));
    assert_eq!(order.last(), Some(&"-100.00"));

    // Approved only: the submitted 6.00 on KD-180 no longer counts.
    let (_, approved) = call(
        &pool,
        "GET",
        "/api/v1/reports/token-budgets?period=2026-09&status=approved",
        c,
        None,
    )
    .await
    .unwrap();
    assert_eq!(approved["status"], "approved");
    assert_eq!(row(&approved, &at).unwrap()["actual"], "4.00");

    let (status, csv) = call_text(
        &pool,
        "GET",
        "/api/v1/reports/token-budgets?period=2026-09&format=csv",
        c,
        None,
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK);
    assert!(csv.starts_with(
        "token_budget,name,filter,applies,actual_period,amount,actual,difference,outcome,share_percent,token_entries,unknown_model_entries,currency\r\n"
    ));
    assert!(
        csv.contains(&format!(
            "{over},,team:data,2026-09,2026-09,5.00,7.50,2.50,overrun,,1,0,USD\r\n"
        )),
        "{csv}"
    );
    assert!(csv.contains(&format!(
        "{unknown},,team:qa,2026-09,2026-09,1.00,0.00,-1.00,unknown model,,1,1,USD\r\n"
    )));
    assert!(csv.contains(&format!(
        "{all_time},,ticket:KD-180,all time,all time,8.00,8.00,0.00,at_token_budget,,2,0,USD\r\n"
    )));

    let (status, _) = call(
        &pool,
        "GET",
        "/api/v1/reports/token-budgets?period=Sept",
        c,
        None,
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn each_row_opens_down_to_tickets_and_token_entries(pool: PgPool) {
    let org = organization(&pool, "USD").await.unwrap();
    september(&pool, &org).await.unwrap();
    let web = set_budget(
        &pool,
        org.web_lead,
        json!({"filter": {"team": "web"}, "period": "2026-09", "amount": "12"}),
    )
    .await
    .unwrap();
    let ticket = set_budget(
        &pool,
        org.controller,
        json!({"filter": {"ticket": "KD-180"}, "amount": "5"}),
    )
    .await
    .unwrap();

    let path = format!("/api/v1/reports/token-budgets/{web}?period=2026-09");
    let (status, detail) = call(&pool, "GET", &path, org.web_lead, None).await.unwrap();
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["row"]["actual"], "10.00");
    assert_eq!(detail["row"]["outcome"], "underrun");
    assert_eq!(
        detail["tickets"],
        json!([
            {"ticket": "KD-180", "title": "Coupon totals", "actual": "6.00", "token_entries": 1, "unknown_model_entries": 0},
            {"ticket": "KD-181", "title": "Coupon rounding", "actual": "4.00", "token_entries": 1, "unknown_model_entries": 0},
        ])
    );
    assert_eq!(detail["token_entries"].as_array().unwrap().len(), 2);
    assert_eq!(detail["token_entries"][0]["agent_manager"], "ana");

    let (status, csv) = call_text(
        &pool,
        "GET",
        &format!("{path}&format=csv"),
        org.web_lead,
        None,
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        csv,
        "ticket,title,actual,token_entries,unknown_model_entries,currency\r\n\
         KD-180,Coupon totals,6.00,1,0,USD\r\nKD-181,Coupon rounding,4.00,1,0,USD\r\n"
    );
    let (status, csv) = call_text(
        &pool,
        "GET",
        &format!("{path}&format=csv&table=token_entries"),
        org.web_lead,
        None,
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(csv.lines().count(), 3);
    let (status, _) = call_text(
        &pool,
        "GET",
        &format!("{path}&format=csv&table=people"),
        org.web_lead,
        None,
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // The all-time token budget counts the closed August with its credit.
    let path = format!("/api/v1/reports/token-budgets/{ticket}?period=2026-09");
    let (_, detail) = call(&pool, "GET", &path, org.controller, None)
        .await
        .unwrap();
    assert_eq!(detail["row"]["actual"], "8.00");
    assert_eq!(detail["row"]["difference"], "3.00");
    assert_eq!(detail["row"]["outcome"], "overrun");
    assert_eq!(detail["tickets"][0]["actual"], "8.00");
    assert_eq!(detail["adjustments"][0]["amount"], "-1.00");
    assert_eq!(detail["adjustments"][0]["reason"], "Provider credit");

    let (status, _) = call(
        &pool,
        "GET",
        &format!("/api/v1/reports/token-budgets/{web}?period=2026-08"),
        org.web_lead,
        None,
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = call(
        &pool,
        "GET",
        &format!("/api/v1/reports/token-budgets/{web}?period=2026-09"),
        org.bo,
        None,
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(
        &pool,
        "GET",
        &format!("/api/v1/reports/token-budgets/{}", Uuid::nil()),
        org.controller,
        None,
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn each_role_sees_its_own_rows_and_actual(pool: PgPool) {
    let org = organization(&pool, "USD").await.unwrap();
    september(&pool, &org).await.unwrap();
    let c = org.controller;
    let web = set_budget(
        &pool,
        c,
        json!({"filter": {"team": "web"}, "period": "2026-09", "amount": "10"}),
    )
    .await
    .unwrap();
    let data = set_budget(
        &pool,
        c,
        json!({"filter": {"team": "data"}, "period": "2026-09", "amount": "10"}),
    )
    .await
    .unwrap();
    let ana = set_budget(
        &pool,
        c,
        json!({"filter": {"person": "ana@example.com"}, "period": "2026-09", "amount": "10"}),
    )
    .await
    .unwrap();

    let path = "/api/v1/reports/token-budgets?period=2026-09";
    let (_, lead) = call(&pool, "GET", path, org.web_lead, None).await.unwrap();
    assert!(row(&lead, &web).is_some());
    assert!(row(&lead, &ana).is_some());
    assert!(row(&lead, &data).is_none());
    let (_, own) = call(&pool, "GET", path, org.ana, None).await.unwrap();
    assert_eq!(own["rows"].as_array().unwrap().len(), 1);
    assert_eq!(row(&own, &ana).unwrap()["actual"], "10.00");
    let (_, none) = call(&pool, "GET", path, org.bo, None).await.unwrap();
    assert_eq!(none["rows"], json!([]));
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn actual_is_in_the_reporting_currency(pool: PgPool) {
    let org = organization(&pool, "EUR").await.unwrap();
    // 1.25 USD per euro on the period's last day: 10.00 USD is 8.00 EUR.
    sqlx::query(
        "INSERT INTO exchange_rates (day, currency, per_euro_nanos) VALUES ($1, 'USD', 1250000000)",
    )
    .bind(date!(2026 - 09 - 30))
    .execute(&pool)
    .await
    .unwrap();
    entry(
        &pool,
        org.id,
        Entry {
            person: org.ana,
            cents: Some(1_000),
            tags: &[("team", "web")],
            ..Entry::default()
        },
    )
    .await
    .unwrap();
    let web = set_budget(
        &pool,
        org.controller,
        json!({"filter": {"team": "web"}, "period": "2026-09", "amount": "8"}),
    )
    .await
    .unwrap();
    let (_, report) = call(
        &pool,
        "GET",
        "/api/v1/reports/token-budgets?period=2026-09",
        org.controller,
        None,
    )
    .await
    .unwrap();
    assert_eq!(report["currency"], "EUR");
    assert_eq!(report["exchange_rate"], Value::Null);
    assert_eq!(report["label"], "list-price estimate");
    let web_row = row(&report, &web).unwrap();
    assert_eq!(web_row["token_budget"]["currency"], "EUR");
    assert_eq!(web_row["actual"], "8.00");
    assert_eq!(web_row["outcome"], "at_token_budget");
    assert_eq!(web_row["share"], Value::Null);

    // Shown in dollars and as shares of the period, as every report can be.
    let (status, shown) = call(
        &pool,
        "GET",
        "/api/v1/reports/token-budgets?period=2026-09&currency=USD&view=share",
        org.controller,
        None,
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK, "{shown}");
    assert_eq!(shown["currency"], "USD");
    assert_eq!(shown["exchange_rate"], "1.25");
    assert_eq!(shown["view"], "share");
    assert_eq!(shown["label"], "share of the period");
    let web_row = row(&shown, &web).unwrap();
    assert_eq!(web_row["amount"], "10.00");
    assert_eq!(web_row["actual"], "10.00");
    assert_eq!(web_row["difference"], "0.00");
    assert_eq!(web_row["outcome"], "at_token_budget");
    assert_eq!(web_row["share"], 10_000);
    let (_, csv) = call_text(
        &pool,
        "GET",
        "/api/v1/reports/token-budgets?period=2026-09&currency=USD&view=share&format=csv",
        org.controller,
        None,
    )
    .await
    .unwrap();
    assert!(
        csv.contains(",10.00,10.00,0.00,at_token_budget,100.00,1,0,USD"),
        "{csv}"
    );
    let (_, detail) = call(
        &pool,
        "GET",
        &format!("/api/v1/reports/token-budgets/{web}?period=2026-09&currency=USD&view=share"),
        org.controller,
        None,
    )
    .await
    .unwrap();
    assert_eq!(detail["currency"], "USD");
    assert_eq!(detail["exchange_rate"], "1.25");
    assert_eq!(detail["row"]["actual"], "10.00");
    assert_eq!(detail["row"]["share"], 10_000);
    assert_eq!(detail["tickets"][0]["actual"], "10.00");
    let (status, _) = call(
        &pool,
        "GET",
        "/api/v1/reports/token-budgets?period=2026-09&currency=XYZ",
        org.controller,
        None,
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = call(
        &pool,
        "GET",
        "/api/v1/reports/token-budgets?period=2026-09&currency=dollars",
        org.controller,
        None,
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// A deterministic stand-in for an embeddings model: words hashed into a
/// fixed number of counts.
struct BagOfWords;

const DIMENSIONS: u64 = 512;

impl Embedder for BagOfWords {
    fn model(&self) -> &'static str {
        "bag-of-words"
    }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Embedding>, EmbedError> {
        texts
            .iter()
            .map(|text| {
                let mut counts = vec![0_i32; usize::try_from(DIMENSIONS).unwrap_or(1)];
                for word in text
                    .to_lowercase()
                    .split(|c: char| !c.is_alphanumeric())
                    .filter(|word| word.len() > 2)
                {
                    let mut hasher = DefaultHasher::new();
                    word.hash(&mut hasher);
                    let slot = hasher.finish().checked_rem(DIMENSIONS).unwrap_or(0);
                    if let Some(count) = usize::try_from(slot)
                        .ok()
                        .and_then(|slot| counts.get_mut(slot))
                    {
                        *count = count.saturating_add(100_000);
                    }
                }
                Embedding::from_millionths(counts)
                    .map_err(|error| EmbedError::Unreadable(error.to_string()))
            })
            .collect()
    }
}

/// An embeddings model that always refuses.
struct Refusing;

impl Embedder for Refusing {
    fn model(&self) -> &'static str {
        "refusing"
    }

    async fn embed(&self, _texts: &[String]) -> Result<Vec<Embedding>, EmbedError> {
        Err(EmbedError::Refused {
            status: 401,
            message: "Incorrect API key provided".to_owned(),
        })
    }
}

/// Past work in a closed August: two cart coupon fixes with a known cost,
/// one on an unknown model, and unrelated onboarding work.
async fn past_work(pool: &PgPool, org: &Org) -> TestResult<()> {
    for (ticket, cents, summary) in [
        (
            "KD-180",
            Some(3_000),
            "Fixed the cart totals when a coupon is applied",
        ),
        (
            "KD-181",
            Some(5_000),
            "Fixed rounding of cart totals for coupon codes",
        ),
        (
            "KD-182",
            None,
            "Cart coupon totals were wrong; fixed the total",
        ),
        (
            "KD-190",
            Some(9_900),
            "Wrote onboarding emails for new customers",
        ),
        ("KD-214", Some(100), "Looked at the cart coupon total bug"),
    ] {
        entry(
            pool,
            org.id,
            Entry {
                person: org.ana,
                started_at: datetime!(2026-08-12 10:00 UTC),
                status: "locked",
                cents,
                summary,
                tags: &[("team", "web"), ("ticket", ticket)],
                ..Entry::default()
            },
        )
        .await?;
    }
    // Not locked yet: not past work.
    entry(
        pool,
        org.id,
        Entry {
            person: org.ana,
            summary: "Fixed the cart totals when a coupon is applied",
            tags: &[("team", "web"), ("ticket", "KD-300")],
            ..Entry::default()
        },
    )
    .await?;
    Ok(())
}

async fn embedded(pool: &PgPool) -> TestResult<i64> {
    Ok(
        sqlx::query_scalar("SELECT count(*) FROM summary_embeddings")
            .fetch_one(pool)
            .await?,
    )
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_token_forecast_proposes_the_median_of_similar_past_tickets(pool: PgPool) {
    let org = organization(&pool, "USD").await.unwrap();
    past_work(&pool, &org).await.unwrap();
    let latest = "/api/v1/tickets/KD-214/token-forecast";
    let (status, _) = call(&pool, "GET", latest, org.controller, None)
        .await
        .unwrap();
    assert_eq!(status, StatusCode::NOT_FOUND);

    let made = forecast::create(
        &pool,
        &BagOfWords,
        org.id,
        org.controller,
        &Scope::everything(),
        "KD-214",
    )
    .await
    .unwrap();
    assert_eq!(made.ticket, "KD-214");
    assert_eq!(made.title.as_deref(), Some("Cart totals wrong with coupon"));
    assert_eq!(made.model, "bag-of-words");
    assert_eq!(made.currency, "USD");
    let matched: Vec<&str> = made
        .matches
        .iter()
        .map(|found| found.ticket.as_str())
        .collect();
    let mut sorted = matched.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, ["KD-180", "KD-181", "KD-182"], "{:?}", made.matches);
    assert!(
        made.matches
            .windows(2)
            .all(|pair| pair.first().map(|m| m.confidence) >= pair.get(1).map(|m| m.confidence))
    );
    assert!(
        made.matches
            .iter()
            .all(|found| found.confidence >= forecast::MIN_CONFIDENCE)
    );
    let kd180 = made
        .matches
        .iter()
        .find(|found| found.ticket == "KD-180")
        .unwrap();
    assert_eq!(kd180.actual, "30.00");
    assert_eq!(kd180.title.as_deref(), Some("Coupon totals"));
    let kd182 = made
        .matches
        .iter()
        .find(|found| found.ticket == "KD-182")
        .unwrap();
    assert_eq!(kd182.unknown_model_entries, 1);
    // The median of 30.00 and 50.00; KD-182's cost is not known.
    assert_eq!(made.proposed.as_deref(), Some("40.00"));
    assert_eq!(embedded(&pool).await.unwrap(), 5);
    let stored: Option<i64> =
        sqlx::query_scalar("SELECT proposed_nanos FROM token_forecasts WHERE id = $1")
            .bind(made.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored, Some(40_000_000_000));

    let accept = format!("/api/v1/token-forecasts/{}/accept", made.id);
    let (status, _) = call(&pool, "POST", &accept, org.ana, Some(json!({})))
        .await
        .unwrap();
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(&pool, "POST", &accept, org.web_lead, Some(json!({})))
        .await
        .unwrap();
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, budget) = call(&pool, "POST", &accept, org.account_manager, Some(json!({})))
        .await
        .unwrap();
    assert_eq!(status, StatusCode::CREATED, "{budget}");
    assert_eq!(budget["amount"], "40.00");
    assert_eq!(budget["filter"], json!({"ticket": "KD-214"}));
    assert_eq!(budget["applies"], "all time");
    assert_eq!(budget["period"], Value::Null);
    assert_eq!(budget["token_forecast_id"], made.id.to_string());
    // After a reload the page reads the token forecast again, accepted.
    let (status, read) = call(&pool, "GET", latest, org.account_manager, None)
        .await
        .unwrap();
    assert_eq!(status, StatusCode::OK, "{read}");
    assert_eq!(read["id"], made.id.to_string());
    assert_eq!(read["proposed"], "40.00");
    assert_eq!(read["matches"].as_array().map(Vec::len), Some(3));
    assert_eq!(read["token_budget_id"], budget["id"]);
    let (status, _) = call(&pool, "GET", latest, org.ana, None).await.unwrap();
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(&pool, "GET", latest, org.web_lead, None)
        .await
        .unwrap();
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, again) = call(&pool, "POST", &accept, org.account_manager, Some(json!({})))
        .await
        .unwrap();
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        again["message"],
        "this token forecast is already accepted as a token budget"
    );

    // Pick matches, or type the amount.
    let second = forecast::create(
        &pool,
        &BagOfWords,
        org.id,
        org.controller,
        &Scope::everything(),
        "KD-214",
    )
    .await
    .unwrap();
    let accept = format!("/api/v1/token-forecasts/{}/accept", second.id);
    let (status, _) = call(
        &pool,
        "POST",
        &accept,
        org.controller,
        Some(json!({"tickets": ["KD-190"]})),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(
        &pool,
        "POST",
        &accept,
        org.controller,
        Some(json!({"tickets": ["KD-182"]})),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, budget) = call(
        &pool,
        "POST",
        &accept,
        org.controller,
        Some(json!({"tickets": ["KD-181", "KD-182"]})),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(budget["amount"], "50.00");

    let third = forecast::create(
        &pool,
        &BagOfWords,
        org.id,
        org.controller,
        &Scope::everything(),
        "KD-214",
    )
    .await
    .unwrap();
    let accept = format!("/api/v1/token-forecasts/{}/accept", third.id);
    let (_, budget) = call(
        &pool,
        "POST",
        &accept,
        org.controller,
        Some(json!({"amount": "12.5"})),
    )
    .await
    .unwrap();
    assert_eq!(budget["amount"], "12.50");
    let (_, read) = call(&pool, "GET", latest, org.controller, None)
        .await
        .unwrap();
    assert_eq!(read["id"], third.id.to_string());
    assert_eq!(read["token_budget_id"], budget["id"]);
    let (status, _) = call(
        &pool,
        "POST",
        &format!("/api/v1/token-forecasts/{}/accept", Uuid::nil()),
        org.controller,
        Some(json!({})),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::NOT_FOUND);

    // The accepted token budget shows in token budget vs actual for all time.
    let (_, report) = call(
        &pool,
        "GET",
        "/api/v1/reports/token-budgets?period=2026-08",
        org.controller,
        None,
    )
    .await
    .unwrap();
    let accepted = row(&report, budget["id"].as_str().unwrap()).unwrap();
    assert_eq!(accepted["actual"], "1.00");
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_token_forecast_with_no_similar_work_proposes_nothing(pool: PgPool) {
    let org = organization(&pool, "USD").await.unwrap();
    let made = forecast::create(
        &pool,
        &BagOfWords,
        org.id,
        org.controller,
        &Scope::everything(),
        "KD-214",
    )
    .await
    .unwrap();
    assert!(made.matches.is_empty());
    assert_eq!(made.proposed, None);
    let accept = format!("/api/v1/token-forecasts/{}/accept", made.id);
    let (status, body) = call(&pool, "POST", &accept, org.controller, Some(json!({})))
        .await
        .unwrap();
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    let (status, _) = call(
        &pool,
        "POST",
        &accept,
        org.controller,
        Some(json!({"amount": "30"})),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::CREATED);

    let no_text = forecast::create(
        &pool,
        &BagOfWords,
        org.id,
        org.controller,
        &Scope::everything(),
        "KD-182",
    )
    .await;
    assert!(matches!(
        no_text,
        Err(tokencontroller_server::error::ApiError::Conflict(_))
    ));
    let missing = forecast::create(
        &pool,
        &BagOfWords,
        org.id,
        org.controller,
        &Scope::everything(),
        "NOPE-1",
    )
    .await;
    assert!(matches!(
        missing,
        Err(tokencontroller_server::error::ApiError::NotFound)
    ));
    let web = Scope::of_person(
        org.web_lead,
        vec![tokencontroller_server::reports::filter::Filter::parse("team:web").unwrap()],
    );
    let outside = forecast::create(&pool, &BagOfWords, org.id, org.web_lead, &web, "KD-214").await;
    assert!(matches!(
        outside,
        Err(tokencontroller_server::error::ApiError::Forbidden)
    ));
    let refused = forecast::create(
        &pool,
        &Refusing,
        org.id,
        org.controller,
        &Scope::everything(),
        "KD-214",
    )
    .await;
    match refused {
        Err(tokencontroller_server::error::ApiError::Conflict(message)) => {
            assert!(message.contains("Incorrect API key provided"), "{message}");
        }
        other => panic!("expected a conflict, got {other:?}"),
    }
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn summaries_are_embedded_lazily_and_in_bounded_batches(pool: PgPool) {
    let org = organization(&pool, "USD").await.unwrap();
    past_work(&pool, &org).await.unwrap();
    assert_eq!(
        embed_pending(&pool, &BagOfWords, Some(org.id), 2)
            .await
            .unwrap(),
        2
    );
    assert_eq!(embedded(&pool).await.unwrap(), 2);
    assert_eq!(
        embed_pending(&pool, &BagOfWords, None, 100).await.unwrap(),
        3
    );
    assert_eq!(
        embed_pending(&pool, &BagOfWords, None, 100).await.unwrap(),
        0
    );
    assert_eq!(
        embed_pending(&pool, &BagOfWords, Some(Uuid::nil()), 100)
            .await
            .unwrap(),
        0
    );
    assert_eq!(embedded(&pool).await.unwrap(), 5);
    let millionths: Vec<i32> =
        sqlx::query_scalar("SELECT millionths FROM summary_embeddings LIMIT 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(millionths.len(), usize::try_from(DIMENSIONS).unwrap());
    assert!(embed_pending(&pool, &Refusing, None, 10).await.is_err());
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn token_forecasts_can_be_switched_off(pool: PgPool) {
    let org = organization(&pool, "USD").await.unwrap();
    past_work(&pool, &org).await.unwrap();
    let (status, settings) = call(
        &pool,
        "GET",
        "/api/v1/token-forecasts/settings",
        org.web_lead,
        None,
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(settings["enabled"], true);
    assert_eq!(settings["available"], false);
    assert!(
        settings["reason"]
            .as_str()
            .unwrap()
            .contains("no embeddings model configured")
    );

    // This instance has no embeddings model.
    let (status, body) = call(
        &pool,
        "POST",
        "/api/v1/tickets/KD-214/token-forecast",
        org.controller,
        None,
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["message"], forecast::NO_MODEL);
    let (status, _) = call(
        &pool,
        "POST",
        "/api/v1/tickets/KD-214/token-forecast",
        org.ana,
        None,
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::FORBIDDEN);

    let off = json!({"enabled": false});
    let (status, _) = call(
        &pool,
        "PUT",
        "/api/v1/token-forecasts/settings",
        org.web_lead,
        Some(off.clone()),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, settings) = call(
        &pool,
        "PUT",
        "/api/v1/token-forecasts/settings",
        org.controller,
        Some(off),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(settings["enabled"], false);
    let switched_off = forecast::create(
        &pool,
        &BagOfWords,
        org.id,
        org.controller,
        &Scope::everything(),
        "KD-214",
    )
    .await;
    match switched_off {
        Err(tokencontroller_server::error::ApiError::Conflict(message)) => {
            assert_eq!(message, forecast::SWITCHED_OFF);
        }
        other => panic!("expected a conflict, got {other:?}"),
    }
    // A switched-off organization's summaries are not embedded.
    assert_eq!(
        embed_pending(&pool, &BagOfWords, None, 100).await.unwrap(),
        0
    );

    let (_, settings) = call(
        &pool,
        "PUT",
        "/api/v1/token-forecasts/settings",
        org.controller,
        Some(json!({"enabled": true})),
    )
    .await
    .unwrap();
    assert_eq!(settings["enabled"], true);
    assert_eq!(
        embed_pending(&pool, &BagOfWords, None, 100).await.unwrap(),
        5
    );
}
