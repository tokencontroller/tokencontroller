//! The worked month of the example organization (seed), read through the
//! API: the figures the docs show, each area's endpoints, and what each
//! role may see and do.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use sqlx::PgPool;
use tokencontroller_core::NoExtension;
use tokencontroller_server::seed::{emails, ids};
use tokencontroller_server::sign_in::secret;
use tokencontroller_server::{AppState, Config, router, seed};
use tower::ServiceExt as _;
use uuid::Uuid;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

/// The secret key of this test run: made once, never written down.
fn key() -> &'static str {
    static KEY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    KEY.get_or_init(|| uuid::Uuid::new_v4().simple().to_string())
}

fn config() -> Config {
    Config {
        database_url: "postgres://unused".to_owned(),
        public_url: "http://localhost:8080".to_owned(),
        secret_key: key().to_owned(),
        smtp_url: None,
        smtp_from: None,
        price_table: None,
        rates_refresh: false,
        classifier: None,
    }
}

/// The seeded organization with a browser session per person.
struct Kestrel {
    app: Router,
    nadia: String,
    priya: String,
    mara: String,
    tom: String,
}

impl Kestrel {
    async fn load(pool: &PgPool) -> TestResult<Self> {
        seed::load(pool).await?;
        Ok(Self {
            app: router(AppState::new(pool.clone(), config(), Arc::new(NoExtension))),
            nadia: session_for(pool, ids::NADIA).await?,
            priya: session_for(pool, ids::PRIYA).await?,
            mara: session_for(pool, ids::MARA).await?,
            tom: session_for(pool, ids::TOM).await?,
        })
    }

    async fn get(&self, who: &str, path: &str) -> TestResult<Answer> {
        self.send(who, Method::GET, path, None).await
    }

    async fn post(&self, who: &str, path: &str, body: Value) -> TestResult<Answer> {
        self.send(who, Method::POST, path, Some(body)).await
    }

    async fn send(
        &self,
        who: &str,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> TestResult<Answer> {
        let mut request = Request::builder()
            .method(method)
            .uri(format!("/api/v1{path}"))
            .header(header::COOKIE, format!("tc_session={who}"));
        if body.is_some() {
            request = request.header(header::CONTENT_TYPE, "application/json");
        }
        let request = match body {
            Some(body) => request.body(Body::from(serde_json::to_vec(&body)?))?,
            None => request.body(Body::empty())?,
        };
        let response = self.app.clone().oneshot(request).await?;
        let status = response.status();
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        let bytes = response.into_body().collect().await?.to_bytes();
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        Ok(Answer {
            status,
            content_type,
            json,
            text,
        })
    }
}

struct Answer {
    status: StatusCode,
    content_type: String,
    json: Value,
    text: String,
}

impl Answer {
    /// The answer's JSON, after checking the status.
    fn ok(self, expected: StatusCode) -> TestResult<Value> {
        if self.status != expected {
            return Err(format!("expected {expected}, got {}: {}", self.status, self.text).into());
        }
        Ok(self.json)
    }

    /// The error message of a refused request.
    fn refused(self, expected: StatusCode) -> TestResult<String> {
        let json = self.ok(expected)?;
        Ok(json
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned())
    }
}

async fn session_for(pool: &PgPool, person_id: Uuid) -> TestResult<String> {
    let session = secret::generate(secret::SESSION, key())?;
    sqlx::query("INSERT INTO sessions (token_hash, person_id, client, expires_at) VALUES ($1, $2, 'browser', now() + interval '1 day')")
        .bind(&session.hash)
        .bind(person_id)
        .execute(pool)
        .await?;
    Ok(session.token)
}

/// The row of a report whose key is `key`.
fn row<'a>(report: &'a Value, key: Option<&str>) -> Option<&'a Value> {
    report
        .get("rows")?
        .as_array()?
        .iter()
        .find(|row| row.get("key").and_then(Value::as_str) == key)
}

/// A string field of a JSON object.
fn text<'a>(value: &'a Value, field: &str) -> &'a str {
    value.get(field).and_then(Value::as_str).unwrap_or_default()
}

/// How many elements a JSON array has.
fn count(value: &Value) -> usize {
    value.as_array().map(Vec::len).unwrap_or_default()
}

const MARCH: &str = "from=2026-03-01&to=2026-03-31";

// ---- Cost report -------------------------------------------------------

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_month_costs_950(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let report = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(text(&report, "total"), "950.00");
        assert_eq!(text(&report, "currency"), "USD");
        assert_eq!(text(&report, "view"), "money");
        assert_eq!(report["count_errors"], 0);
        assert_eq!(count(&report["unknown_models"]), 0);
        let whole = row(&report, None).ok_or("no row")?;
        assert_eq!(whole["messages"], 4);
        assert_eq!(text(whole, "share"), "1");
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn by_feature_shows_the_split(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let report = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&by=feature"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(
            text(row(&report, Some("KD-1")).ok_or("KD-1")?, "amount"),
            "580.00"
        );
        assert_eq!(
            text(row(&report, Some("KD-2")).ok_or("KD-2")?, "amount"),
            "120.00"
        );
        assert_eq!(text(row(&report, None).ok_or("none")?, "amount"), "250.00");
        assert_eq!(row(&report, Some("KD-1")).ok_or("KD-1")?["messages"], 2);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn by_project_brings_the_features_along(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let report = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&by=project"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(
            text(row(&report, Some("shop")).ok_or("shop")?, "amount"),
            "800.00"
        );
        assert_eq!(row(&report, Some("shop")).ok_or("shop")?["messages"], 3);
        assert_eq!(text(row(&report, None).ok_or("none")?, "amount"), "150.00");
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn by_client_follows_the_project_default(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let report = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&by=client"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(
            text(row(&report, Some("acme")).ok_or("acme")?, "amount"),
            "800.00"
        );
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn by_person_splits_mara_and_tom(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let report = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&by=person"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(
            text(row(&report, Some(emails::MARA)).ok_or("mara")?, "amount"),
            "700.00"
        );
        assert_eq!(
            text(row(&report, Some(emails::TOM)).ok_or("tom")?, "amount"),
            "250.00"
        );
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn by_label_separates_direct_from_overhead(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let report = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&by=label"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(
            text(row(&report, Some("direct")).ok_or("direct")?, "amount"),
            "700.00"
        );
        assert_eq!(
            text(row(&report, Some("project overhead")).ok_or("p")?, "amount"),
            "100.00"
        );
        assert_eq!(
            text(
                row(&report, Some("organization overhead")).ok_or("o")?,
                "amount"
            ),
            "150.00"
        );
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn by_team_uses_the_persons_attribute(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let report = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&by=team"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&report["rows"]), 1);
        assert_eq!(
            text(row(&report, Some("web")).ok_or("web")?, "amount"),
            "950.00"
        );
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn by_department_follows_the_team_default(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let report = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&by=department"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(
            text(row(&report, Some("engineering")).ok_or("eng")?, "amount"),
            "950.00"
        );
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn by_model_and_agent_harness_group_the_messages(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let report = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&by=model"))
            .await?
            .ok(StatusCode::OK)?;
        let model = format!("{}/{}", seed::PROVIDER, seed::MODEL);
        assert_eq!(
            text(row(&report, Some(&model)).ok_or("model")?, "amount"),
            "950.00"
        );
        let report = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&by=agent_harness"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(
            text(
                row(&report, Some("claude-code")).ok_or("harness")?,
                "amount"
            ),
            "950.00"
        );
        let report = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&by=agent_name"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(
            text(row(&report, Some("review bot")).ok_or("bot")?, "amount"),
            "100.00"
        );
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn by_day_puts_each_session_on_its_day(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let report = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&by=day"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&report["rows"]), 4);
        assert_eq!(
            text(row(&report, Some("2026-03-04")).ok_or("day")?, "amount"),
            "400.00"
        );
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_share_view_answers_fractions(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let report = k
            .get(
                &k.nadia,
                &format!("/reports/cost?{MARCH}&by=feature&view=share"),
            )
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(text(&report, "view"), "share");
        assert_eq!(
            text(row(&report, Some("KD-1")).ok_or("KD-1")?, "amount"),
            "0.610526"
        );
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn an_unknown_view_is_refused(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let error = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&view=pie"))
            .await?
            .refused(StatusCode::BAD_REQUEST)?;
        assert!(error.contains("money or share"), "{error}");
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_filter_narrows_the_report(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let report = k
            .get(
                &k.nadia,
                &format!("/reports/cost?{MARCH}&filter=feature:KD-1"),
            )
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(text(&report, "total"), "580.00");
        assert_eq!(report["filter"]["feature"], "KD-1");
        let report = k
            .get(
                &k.nadia,
                &format!(
                    "/reports/cost?{MARCH}&filter=project:shop,person:{}",
                    emails::TOM
                ),
            )
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(text(&report, "total"), "100.00");
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_filter_on_an_attribute_works_like_a_tag(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let report = k
            .get(
                &k.nadia,
                &format!("/reports/cost?{MARCH}&filter=team:web&by=feature"),
            )
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(text(&report, "total"), "950.00");
        let report = k
            .get(
                &k.nadia,
                &format!("/reports/cost?{MARCH}&filter=team:mobile"),
            )
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(text(&report, "total"), "0.00");
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_broken_filter_is_refused(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let error = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&filter=feature"))
            .await?
            .refused(StatusCode::BAD_REQUEST)?;
        assert!(!error.is_empty());
        let error = k
            .get(
                &k.nadia,
                &format!("/reports/cost?{MARCH}&filter=Bad%20Name:x"),
            )
            .await?
            .refused(StatusCode::BAD_REQUEST)?;
        assert!(!error.is_empty());
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_report_as_of_before_the_month_is_empty(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let report = k
            .get(
                &k.nadia,
                &format!("/reports/cost?{MARCH}&as_of=2026-03-01T00:00:00Z"),
            )
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(text(&report, "total"), "0.00");
        assert_eq!(text(&report, "as_of"), "2026-03-01T00:00:00Z");
        let report = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&as_of=2026-02-28"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(text(&report, "as_of"), "2026-02-28T23:59:59.999999999Z");
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_report_as_of_mid_month_shows_what_was_there(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        // S1 and S2 are in, but S2's split is not yet: S2 is project overhead.
        let report = k
            .get(
                &k.nadia,
                &format!("/reports/cost?{MARCH}&as_of=2026-03-12T15:00:00Z&by=label"),
            )
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(text(&report, "total"), "700.00");
        assert_eq!(
            text(row(&report, Some("direct")).ok_or("direct")?, "amount"),
            "400.00"
        );
        assert_eq!(
            text(
                row(&report, Some("organization overhead")).ok_or("o")?,
                "amount"
            ),
            "300.00"
        );
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_report_as_of_a_day_means_its_end(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let report = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&as_of=2026-03-04"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(text(&report, "total"), "400.00");
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_broken_as_of_is_refused(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let error = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&as_of=yesterday"))
            .await?
            .refused(StatusCode::BAD_REQUEST)?;
        assert!(!error.is_empty());
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_window_outside_the_month_is_empty(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let report = k
            .get(&k.nadia, "/reports/cost?from=2026-04-01&to=2026-04-30")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(text(&report, "total"), "0.00");
        assert_eq!(count(&report["rows"]), 0);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_window_that_ends_before_it_starts_is_refused(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let error = k
            .get(&k.nadia, "/reports/cost?from=2026-03-31&to=2026-03-01")
            .await?
            .refused(StatusCode::BAD_REQUEST)?;
        assert!(!error.is_empty());
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_partial_window_takes_part_of_the_month(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let report = k
            .get(&k.nadia, "/reports/cost?from=2026-03-10&to=2026-03-20")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(text(&report, "total"), "400.00");
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_report_comes_as_csv(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let answer = k
            .get(
                &k.nadia,
                &format!("/reports/cost?{MARCH}&by=feature&format=csv"),
            )
            .await?;
        assert_eq!(answer.status, StatusCode::OK);
        assert!(
            answer.content_type.starts_with("text/csv"),
            "{}",
            answer.content_type
        );
        assert!(answer.text.contains("KD-1,580.00"), "{}", answer.text);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn any_other_format_is_json(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let answer = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&format=json"))
            .await?;
        assert_eq!(answer.status, StatusCode::OK);
        assert!(
            answer.content_type.starts_with("application/json"),
            "{}",
            answer.content_type
        );
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_currency_without_a_rate_is_a_conflict(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let error = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&currency=EUR"))
            .await?
            .refused(StatusCode::CONFLICT)?;
        assert!(error.contains("EUR"), "{error}");
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_reporting_currency_may_be_given_in_lower_case(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let report = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&currency=usd"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(text(&report, "currency"), "USD");
        let error = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&currency=dollars"))
            .await?
            .refused(StatusCode::BAD_REQUEST)?;
        assert!(!error.is_empty());
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn approved_only_shows_nothing_before_any_approval(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let report = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&approved=true"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(text(&report, "total"), "0.00");
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn an_agent_manager_reports_on_their_own_messages_only(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let report = k
            .get(&k.mara, &format!("/reports/cost?{MARCH}&by=feature"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(text(&report, "total"), "700.00");
        assert!(row(&report, None).is_none(), "Tom's untagged session shows");
        let report = k
            .get(&k.tom, &format!("/reports/cost?{MARCH}"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(text(&report, "total"), "250.00");
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_people_manager_reports_on_their_team(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let report = k
            .get(&k.priya, &format!("/reports/cost?{MARCH}&by=person"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(text(&report, "total"), "950.00");
        assert_eq!(count(&report["rows"]), 2);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_filter_outside_the_view_shows_nothing_of_it(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let report = k
            .get(
                &k.mara,
                &format!("/reports/cost?{MARCH}&filter=person:{}", emails::TOM),
            )
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(text(&report, "total"), "0.00");
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn reports_need_a_signed_in_caller(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let answer = k
            .get("not-a-session", &format!("/reports/cost?{MARCH}"))
            .await?;
        assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
        Ok(())
    }
    .await;
    result.unwrap();
}

// ---- Deviation and forecast ---------------------------------------------

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_deviation_report_compares_plan_and_cost(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let report = k
            .get(&k.nadia, &format!("/reports/deviation?{MARCH}&by=feature"))
            .await?
            .ok(StatusCode::OK)?;
        let kd1 = row(&report, Some("KD-1")).ok_or("KD-1")?;
        assert_eq!(text(kd1, "plan"), "500.00");
        assert_eq!(text(kd1, "cost"), "580.00");
        assert_eq!(text(kd1, "deviation"), "-80.00");
        assert_eq!(kd1["favourable"], false);
        assert_eq!(kd1["filter"]["feature"], "KD-1");
        let kd2 = row(&report, Some("KD-2")).ok_or("KD-2")?;
        assert!(kd2["plan"].is_null());
        assert!(kd2["deviation"].is_null());
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_plan_of_a_long_estimate_is_prorated_to_the_window(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let report = k
            .get(&k.nadia, &format!("/reports/deviation?{MARCH}&by=project"))
            .await?
            .ok(StatusCode::OK)?;
        let shop = row(&report, Some("shop")).ok_or("shop")?;
        assert_eq!(text(shop, "plan"), "232.50");
        assert_eq!(text(shop, "cost"), "800.00");
        assert_eq!(text(shop, "deviation"), "-567.50");
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_deviation_report_takes_a_filter(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let report = k
            .get(
                &k.nadia,
                &format!("/reports/deviation?{MARCH}&filter=feature:KD-1"),
            )
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&report["rows"]), 1);
        let only = row(&report, None).ok_or("row")?;
        assert_eq!(text(only, "plan"), "500.00");
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_deviation_report_comes_as_csv(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let answer = k
            .get(
                &k.nadia,
                &format!("/reports/deviation?{MARCH}&by=feature&format=csv"),
            )
            .await?;
        assert_eq!(answer.status, StatusCode::OK);
        assert!(answer.text.contains("KD-1"), "{}", answer.text);
        assert!(answer.text.contains("-80.00"), "{}", answer.text);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn an_agent_manager_sees_the_deviation_of_their_own_work(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let report = k
            .get(&k.mara, &format!("/reports/deviation?{MARCH}&by=feature"))
            .await?
            .ok(StatusCode::OK)?;
        let kd1 = row(&report, Some("KD-1")).ok_or("KD-1")?;
        assert_eq!(text(kd1, "cost"), "580.00");
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_forecast_report_answers_for_the_months_ahead(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let report = k
            .get(
                &k.nadia,
                "/reports/forecast?from=2026-04-01&to=2026-04-30&by=project",
            )
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(text(&report, "currency"), "USD");
        assert_eq!(text(&report, "by"), "project");
        assert!(report["rows"].is_array());
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_forecast_report_comes_as_csv(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let answer = k
            .get(
                &k.nadia,
                "/reports/forecast?from=2026-04-01&to=2026-04-30&format=csv",
            )
            .await?;
        assert_eq!(answer.status, StatusCode::OK);
        assert!(
            answer.content_type.starts_with("text/csv"),
            "{}",
            answer.content_type
        );
        Ok(())
    }
    .await;
    result.unwrap();
}

// ---- Saved reports -------------------------------------------------------

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_saved_report_keeps_its_parameters(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let saved = k
            .post(
                &k.nadia,
                "/reports/saved",
                json!({
                    "name": "Shop by feature",
                    "from": "2026-03-01",
                    "to": "2026-03-31",
                    "by": "feature",
                    "filter": {"project": "shop"}
                }),
            )
            .await?
            .ok(StatusCode::CREATED)?;
        assert_eq!(text(&saved, "name"), "Shop by feature");
        assert_eq!(text(&saved, "created_by"), emails::NADIA);
        assert_eq!(saved["filter"]["project"], "shop");
        let list = k
            .get(&k.nadia, "/reports/saved")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&list), 1);
        assert_eq!(list[0]["id"], saved["id"]);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_saved_report_needs_a_name(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let error = k
            .post(&k.nadia, "/reports/saved", json!({"name": "  "}))
            .await?
            .refused(StatusCode::BAD_REQUEST)?;
        assert!(!error.is_empty());
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn saved_reports_belong_to_the_organization(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let saved = k
            .post(
                &k.mara,
                "/reports/saved",
                json!({"name": "Mine", "by": "feature"}),
            )
            .await?
            .ok(StatusCode::CREATED)?;
        assert_eq!(text(&saved, "created_by"), emails::MARA);
        assert!(saved["from"].is_null());
        let mine = k.get(&k.mara, "/reports/saved").await?.ok(StatusCode::OK)?;
        assert_eq!(count(&mine), 1);
        let nadias = k
            .get(&k.nadia, "/reports/saved")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&nadias), 1);
        assert_eq!(text(&nadias[0], "by"), "feature");
        Ok(())
    }
    .await;
    result.unwrap();
}

// ---- Reconciliation ------------------------------------------------------

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn reconciliation_shows_what_no_message_explains(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let rows = k
            .get(&k.nadia, "/reconciliation")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&rows), 1);
        let bill = &rows[0];
        assert_eq!(text(bill, "bill_id"), "bil_seed_anthropic_2026_03");
        assert_eq!(text(bill, "provider"), "anthropic");
        assert_eq!(text(bill, "bill"), "1000.00");
        assert_eq!(text(bill, "messages"), "950.00");
        assert_eq!(text(bill, "unexplained"), "50.00");
        assert_eq!(bill["a_message_is_wrong"], false);
        assert_eq!(bill["final"], true);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn reconciliation_filters_by_provider(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let rows = k
            .get(&k.nadia, "/reconciliation?provider=openai")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&rows), 0);
        let rows = k
            .get(&k.nadia, "/reconciliation?provider=anthropic")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&rows), 1);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn reconciliation_as_of_before_the_bill_is_empty(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let rows = k
            .get(&k.nadia, "/reconciliation?as_of=2026-04-01")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&rows), 0);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn reconciliation_comes_as_csv(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let answer = k.get(&k.nadia, "/reconciliation?format=csv").await?;
        assert_eq!(answer.status, StatusCode::OK);
        assert!(
            answer.text.starts_with("provider,from,to"),
            "{}",
            answer.text
        );
        assert!(answer.text.contains("50.00"), "{}", answer.text);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn reconciliation_is_for_the_whole_organization_only(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let answer = k.get(&k.mara, "/reconciliation").await?;
        assert_eq!(answer.status, StatusCode::FORBIDDEN, "{}", answer.text);
        let answer = k.get(&k.priya, "/reconciliation").await?;
        assert_eq!(answer.status, StatusCode::FORBIDDEN, "{}", answer.text);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_bill_carries_its_reconciliation(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let bills = k
            .get(&k.nadia, &format!("/bills?{MARCH}"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&bills), 1);
        assert_eq!(bills[0]["reconciliation"]["unexplained"], "50.00");
        // One bill's history: the bill and what it superseded.
        let one = k
            .get(&k.nadia, "/bills/bil_seed_anthropic_2026_03")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&one), 1);
        assert_eq!(one[0]["amount"], "1000.00");
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_bill_in_another_range_is_not_listed(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let bills = k
            .get(&k.nadia, "/bills?from=2026-04-01&to=2026-04-30")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&bills), 0);
        let answer = k.get(&k.nadia, "/bills/bil_nobody").await?;
        assert_eq!(answer.status, StatusCode::NOT_FOUND);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_second_bill_for_the_range_supersedes_the_first(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let bill = k
            .post(
                &k.nadia,
                "/bills",
                json!({
                    "provider": "anthropic", "from": "2026-03-01", "to": "2026-03-31",
                    "amount": "960.00", "currency": "USD"
                }),
            )
            .await?
            .ok(StatusCode::CREATED)?;
        assert_eq!(text(&bill, "amount"), "960.00");
        let rows = k
            .get(&k.nadia, "/reconciliation")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&rows), 1);
        assert_eq!(text(&rows[0], "unexplained"), "10.00");
        let all = k
            .get(&k.nadia, &format!("/bills?{MARCH}&reversed=true"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&all), 2);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_bill_smaller_than_its_messages_says_a_message_is_wrong(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        k.post(
            &k.nadia,
            "/bills",
            json!({
                "provider": "anthropic", "from": "2026-03-01", "to": "2026-03-31",
                "amount": "900.00", "currency": "USD"
            }),
        )
        .await?
        .ok(StatusCode::CREATED)?;
        let rows = k
            .get(&k.nadia, "/reconciliation")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(text(&rows[0], "unexplained"), "-50.00");
        assert_eq!(rows[0]["a_message_is_wrong"], true);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn only_an_org_manager_enters_bills(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let answer = k
            .post(
                &k.priya,
                "/bills",
                json!({
                    "provider": "openai", "from": "2026-03-01", "to": "2026-03-31",
                    "amount": "10.00", "currency": "USD"
                }),
            )
            .await?;
        assert_eq!(answer.status, StatusCode::FORBIDDEN, "{}", answer.text);
        Ok(())
    }
    .await;
    result.unwrap();
}

// ---- Sessions and messages ----------------------------------------------

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn sessions_list_the_four_of_the_month(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let list = k
            .get(&k.nadia, &format!("/sessions?{MARCH}"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&list["sessions"]), 4);
        let ids: Vec<&str> = list["sessions"]
            .as_array()
            .ok_or("array")?
            .iter()
            .map(|s| text(s, "session_id"))
            .collect();
        assert_eq!(ids, ["seed-s4", "seed-s3", "seed-s2", "seed-s1"]);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_session_shows_its_parts_and_label(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let list = k
            .get(&k.nadia, &format!("/sessions?{MARCH}&session_id=seed-s2"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&list["sessions"]), 1);
        let s2 = &list["sessions"][0];
        assert_eq!(text(s2, "cost"), "300.00");
        assert_eq!(text(s2, "person"), emails::MARA);
        assert_eq!(count(&s2["parts"]), 2);
        assert_eq!(s2["parts"][0]["fraction"], "0.6");
        assert_eq!(s2["parts"][0]["amount"], "180.00");
        assert_eq!(s2["parts"][0]["label"], "direct");
        assert_eq!(s2["parts"][0]["tags"]["project"], "shop");
        assert_eq!(s2["parts"][0]["tags"]["client"], "acme");
        assert_eq!(s2["parts"][0]["tags"]["team"], "web");
        assert_eq!(s2["parts"][1]["assigned"]["feature"], "KD-2");
        assert_eq!(text(s2, "submitted"), "open");
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn an_untagged_session_is_organization_overhead(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let list = k
            .get(&k.nadia, &format!("/sessions?{MARCH}&session_id=seed-s4"))
            .await?
            .ok(StatusCode::OK)?;
        let s4 = &list["sessions"][0];
        assert_eq!(s4["parts"][0]["label"], "organization overhead");
        assert_eq!(text(s4, "assigned_by"), "default");
        assert_eq!(s4["facts"]["turns"], 12);
        assert_eq!(text(s4, "summary"), "Exploring a logging library");
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_session_with_a_project_and_no_feature_is_project_overhead(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let list = k
            .get(&k.nadia, &format!("/sessions?{MARCH}&session_id=seed-s3"))
            .await?
            .ok(StatusCode::OK)?;
        let s3 = &list["sessions"][0];
        assert_eq!(s3["parts"][0]["label"], "project overhead");
        assert_eq!(text(s3, "agent_name"), "review bot");
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn unassigned_sessions_are_the_ones_without_a_split(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let list = k
            .get(&k.nadia, &format!("/sessions?{MARCH}&unassigned=true"))
            .await?
            .ok(StatusCode::OK)?;
        let ids: Vec<&str> = list["sessions"]
            .as_array()
            .ok_or("array")?
            .iter()
            .map(|s| text(s, "session_id"))
            .collect();
        assert_eq!(ids, ["seed-s4"]);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn sessions_filter_by_tags(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let list = k
            .get(&k.nadia, &format!("/sessions?{MARCH}&filter=feature:KD-2"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&list["sessions"]), 1);
        assert_eq!(list["sessions"][0]["session_id"], "seed-s2");
        let list = k
            .get(&k.nadia, &format!("/sessions?{MARCH}&filter=project:shop"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&list["sessions"]), 3);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn sessions_take_a_limit(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let list = k
            .get(&k.nadia, &format!("/sessions?{MARCH}&limit=2"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&list["sessions"]), 2);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn an_agent_manager_lists_their_own_sessions(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let list = k
            .get(&k.tom, &format!("/sessions?{MARCH}"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&list["sessions"]), 2);
        for session in list["sessions"].as_array().ok_or("array")? {
            assert_eq!(text(session, "person"), emails::TOM);
        }
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_people_manager_lists_the_teams_sessions(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let list = k
            .get(&k.priya, &format!("/sessions?{MARCH}"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&list["sessions"]), 4);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn sessions_as_of_an_earlier_moment(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let list = k
            .get(&k.nadia, &format!("/sessions?{MARCH}&as_of=2026-03-15"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&list["sessions"]), 2);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn messages_need_a_from_date(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let error = k
            .get(&k.nadia, "/messages")
            .await?
            .refused(StatusCode::BAD_REQUEST)?;
        assert!(error.contains("from"), "{error}");
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn messages_show_tokens_and_cost(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let list = k
            .get(&k.nadia, &format!("/messages?{MARCH}&session_id=seed-s1"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&list["messages"]), 1);
        let m = &list["messages"][0];
        assert_eq!(m["tokens"]["input"], 40_000_000);
        assert_eq!(text(m, "cost"), "400.00");
        assert_eq!(text(m, "cost_label"), "priced");
        assert_eq!(text(m, "model"), seed::MODEL);
        assert_eq!(text(m, "provider"), "anthropic");
        assert_eq!(m["parts"][0]["tags"]["feature"], "KD-1");
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn messages_filter_and_limit(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let list = k
            .get(
                &k.nadia,
                &format!("/messages?{MARCH}&filter=person:{}", emails::TOM),
            )
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&list["messages"]), 2);
        let list = k
            .get(&k.nadia, &format!("/messages?{MARCH}&limit=1"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&list["messages"]), 1);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn an_agent_manager_reads_only_their_messages(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let list = k
            .get(&k.mara, &format!("/messages?{MARCH}"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&list["messages"]), 2);
        let list = k
            .get(&k.mara, &format!("/messages?{MARCH}&session_id=seed-s3"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&list["messages"]), 0);
        Ok(())
    }
    .await;
    result.unwrap();
}

// ---- Shares --------------------------------------------------------------

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn shares_list_maras_share_of_shop(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let shares = k.get(&k.nadia, "/shares").await?.ok(StatusCode::OK)?;
        assert_eq!(count(&shares), 1);
        assert_eq!(text(&shares[0], "person"), emails::MARA);
        assert_eq!(text(&shares[0], "project"), "shop");
        assert_eq!(text(&shares[0], "fraction"), "0.6");
        assert_eq!(text(&shares[0], "from"), "2026-03-01");
        assert!(shares[0]["to"].is_null());
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_share_needs_plan_on_the_project(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        // Priya plans for team web, not for project shop.
        let error = k
            .post(
                &k.priya,
                "/shares",
                json!({
                    "person": emails::TOM, "project": "shop", "fraction": "0.25", "from": "2026-04-01"
                }),
            )
            .await?
            .refused(StatusCode::FORBIDDEN)?;
        assert!(error.contains("plan"), "{error}");
        let share = k.post(&k.nadia, "/shares", json!({
            "person": emails::TOM, "project": "shop", "fraction": "0.25", "from": "2026-04-01", "to": "2026-04-30"
        })).await?.ok(StatusCode::CREATED)?;
        assert_eq!(text(&share, "fraction"), "0.25");
        assert_eq!(text(&share, "to"), "2026-04-30");
        assert!(share["over"].is_null());
        let shares = k.get(&k.nadia, "/shares").await?.ok(StatusCode::OK)?;
        assert_eq!(count(&shares), 2);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn an_agent_manager_sets_no_share(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let answer = k
            .post(
                &k.mara,
                "/shares",
                json!({
                    "person": emails::MARA, "project": "shop", "fraction": "1", "from": "2026-04-01"
                }),
            )
            .await?;
        assert_eq!(answer.status, StatusCode::FORBIDDEN, "{}", answer.text);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn shares_over_one_are_shown_never_corrected(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let share = k
            .post(
                &k.nadia,
                "/shares",
                json!({
                    "person": emails::TOM, "project": "shop", "fraction": "0.5", "from": "2026-04-01"
                }),
            )
            .await?
            .ok(StatusCode::CREATED)?;
        assert_eq!(text(&share, "over"), "1.1");
        let shares = k.get(&k.nadia, "/shares").await?.ok(StatusCode::OK)?;
        assert_eq!(count(&shares), 2);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_share_that_is_not_a_fraction_is_refused(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let error = k
            .post(
                &k.nadia,
                "/shares",
                json!({
                    "person": emails::TOM, "project": "shop", "fraction": "half", "from": "2026-04-01"
                }),
            )
            .await?
            .refused(StatusCode::BAD_REQUEST)?;
        assert!(!error.is_empty());
        let answer = k
            .post(
                &k.nadia,
                "/shares",
                json!({
                    "person": emails::TOM, "project": "shop", "fraction": "-0.5", "from": "2026-04-01"
                }),
            )
            .await?;
        assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{}", answer.text);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_share_of_an_unknown_project_is_refused(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let answer = k.post(&k.nadia, "/shares", json!({
            "person": emails::TOM, "project": "warehouse", "fraction": "0.5", "from": "2026-04-01"
        })).await?;
        assert!(
            matches!(
                answer.status,
                StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND
            ),
            "{}",
            answer.text
        );
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_share_of_an_unknown_person_is_refused(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let answer = k.post(&k.nadia, "/shares", json!({
            "person": "nobody@kestrel.example", "project": "shop", "fraction": "0.5", "from": "2026-04-01"
        })).await?;
        assert!(
            matches!(
                answer.status,
                StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND
            ),
            "{}",
            answer.text
        );
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn shares_as_of_before_the_share_are_empty(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let shares = k
            .get(&k.nadia, "/shares?as_of=2026-02-01")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&shares), 0);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn an_agent_manager_sees_their_own_shares(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let shares = k.get(&k.mara, "/shares").await?.ok(StatusCode::OK)?;
        assert_eq!(count(&shares), 1);
        let shares = k.get(&k.tom, "/shares").await?.ok(StatusCode::OK)?;
        assert_eq!(count(&shares), 0);
        Ok(())
    }
    .await;
    result.unwrap();
}

// ---- Grants --------------------------------------------------------------

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn grants_list_the_role_grants(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let grants = k.get(&k.nadia, "/grants").await?.ok(StatusCode::OK)?;
        assert_eq!(count(&grants), 15);
        let nadias: Vec<&str> = grants
            .as_array()
            .ok_or("array")?
            .iter()
            .filter(|g| text(g, "person") == emails::NADIA)
            .map(|g| text(g, "capability"))
            .collect();
        assert_eq!(
            nadias,
            [
                "view", "assign", "approve", "plan", "grant", "adjust", "cutoff"
            ]
        );
        assert!(
            grants
                .as_array()
                .ok_or("array")?
                .iter()
                .all(|g| g["from_role"] == true)
        );
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn grants_filter_by_person(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let grants = k
            .get(&k.nadia, &format!("/grants?person={}", emails::PRIYA))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&grants), 5);
        assert!(
            grants
                .as_array()
                .ok_or("array")?
                .iter()
                .all(|g| g["filter"]["team"] == "web")
        );
        let grants = k
            .get(&k.nadia, &format!("/grants?person={}", emails::MARA))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&grants), 1);
        assert_eq!(grants[0]["capability"], "view");
        assert_eq!(grants[0]["filter"]["person"], emails::MARA);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn an_org_manager_grants_anywhere(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let grant = k
            .post(
                &k.nadia,
                "/grants",
                json!({
                    "person": emails::TOM, "capability": "view", "filter": {"client": "acme"}
                }),
            )
            .await?
            .ok(StatusCode::CREATED)?;
        assert_eq!(grant["from_role"], false);
        assert_eq!(grant["filter"]["client"], "acme");
        // Acme's 800 and Tom's own untagged session.
        let report = k
            .get(&k.tom, &format!("/reports/cost?{MARCH}&by=person"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(text(&report, "total"), "950.00");
        let report = k
            .get(&k.tom, &format!("/reports/cost?{MARCH}&filter=client:acme"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(text(&report, "total"), "800.00");
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_people_manager_grants_inside_their_filter_only(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let error = k
            .post(
                &k.priya,
                "/grants",
                json!({
                    "person": emails::TOM, "capability": "view", "filter": {"client": "acme"}
                }),
            )
            .await?
            .refused(StatusCode::FORBIDDEN)?;
        assert!(!error.is_empty());
        let grant = k.post(&k.priya, "/grants", json!({
            "person": emails::TOM, "capability": "assign", "filter": {"team": "web", "project": "shop"}
        })).await?.ok(StatusCode::CREATED)?;
        assert_eq!(grant["capability"], "assign");
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn an_agent_manager_grants_nothing(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let answer = k
            .post(
                &k.mara,
                "/grants",
                json!({
                    "person": emails::TOM, "capability": "view", "filter": {"person": emails::MARA}
                }),
            )
            .await?;
        assert_eq!(answer.status, StatusCode::FORBIDDEN, "{}", answer.text);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_grant_on_a_value_not_on_record_is_refused(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let answer = k
            .post(
                &k.nadia,
                "/grants",
                json!({
                    "person": emails::TOM, "capability": "view", "filter": {"project": "warehouse"}
                }),
            )
            .await?;
        assert!(
            matches!(
                answer.status,
                StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND
            ),
            "{}",
            answer.text
        );
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_grant_for_an_unknown_capability_is_refused(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let answer = k
            .post(
                &k.nadia,
                "/grants",
                json!({
                    "person": emails::TOM, "capability": "rule", "filter": {}
                }),
            )
            .await?;
        assert_eq!(
            answer.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{}",
            answer.text
        );
        assert!(answer.text.contains("unknown variant"), "{}", answer.text);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_new_grant_supersedes_the_earlier_one(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        k.post(
            &k.nadia,
            "/grants",
            json!({
                "person": emails::TOM, "capability": "view", "filter": {"client": "acme"}
            }),
        )
        .await?
        .ok(StatusCode::CREATED)?;
        k.post(
            &k.nadia,
            "/grants",
            json!({
                "person": emails::TOM, "capability": "view", "filter": {"feature": "KD-2"}
            }),
        )
        .await?
        .ok(StatusCode::CREATED)?;
        let grants = k
            .get(&k.nadia, &format!("/grants?person={}", emails::TOM))
            .await?
            .ok(StatusCode::OK)?;
        // The role's own grant stays; the hand grant was superseded.
        let views: Vec<&Value> = grants
            .as_array()
            .ok_or("array")?
            .iter()
            .filter(|g| text(g, "capability") == "view")
            .collect();
        assert_eq!(views.len(), 2, "{grants}");
        assert_eq!(views[0]["from_role"], true);
        assert_eq!(views[1]["filter"]["feature"], "KD-2");
        let report = k
            .get(&k.tom, &format!("/reports/cost?{MARCH}&by=feature"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(text(&report, "total"), "370.00");
        assert!(row(&report, Some("KD-1")).is_none());
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_reversed_grant_is_gone(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let grant = k
            .post(
                &k.nadia,
                "/grants",
                json!({
                    "person": emails::TOM, "capability": "view", "filter": {}
                }),
            )
            .await?
            .ok(StatusCode::CREATED)?;
        let report = k
            .get(&k.tom, &format!("/reports/cost?{MARCH}"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(text(&report, "total"), "950.00");
        k.post(&k.nadia, "/reversals", json!({"event_id": grant["id"]}))
            .await?
            .ok(StatusCode::CREATED)?;
        let report = k
            .get(&k.tom, &format!("/reports/cost?{MARCH}"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(text(&report, "total"), "250.00");
        let grants = k
            .get(&k.nadia, &format!("/grants?person={}", emails::TOM))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&grants), 1);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn each_person_lists_their_own_grants(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let grants = k.get(&k.mara, "/grants").await?.ok(StatusCode::OK)?;
        assert_eq!(count(&grants), 1);
        let grants = k.get(&k.priya, "/grants").await?.ok(StatusCode::OK)?;
        assert!(count(&grants) >= 5);
        Ok(())
    }
    .await;
    result.unwrap();
}

// ---- Cutoff ---------------------------------------------------------------

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_cutoff_starts_unset(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let cutoff = k.get(&k.nadia, "/cutoff").await?.ok(StatusCode::OK)?;
        assert!(cutoff["cutoff"].is_null());
        assert!(cutoff["frozen_as_of"].is_null());
        let organization = k.get(&k.nadia, "/organization").await?.ok(StatusCode::OK)?;
        assert!(organization["cutoff"].is_null());
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn an_org_manager_moves_the_cutoff(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let moved = k
            .post(&k.nadia, "/cutoff", json!({"cutoff": "2026-03-31"}))
            .await?
            .ok(StatusCode::CREATED)?;
        assert_eq!(text(&moved, "cutoff"), "2026-03-31");
        assert_eq!(text(&moved, "moved_by"), emails::NADIA);
        assert_eq!(text(&moved, "frozen_as_of"), "2026-03-31T00:00:00Z");
        assert!(moved["event_id"].is_string());
        let cutoff = k.get(&k.mara, "/cutoff").await?.ok(StatusCode::OK)?;
        assert_eq!(text(&cutoff, "cutoff"), "2026-03-31");
        let context = k.get(&k.mara, "/me/context").await?.ok(StatusCode::OK)?;
        assert_eq!(text(&context, "cutoff"), "2026-03-31");
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn only_an_org_manager_moves_the_cutoff(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        for who in [&k.priya, &k.mara] {
            let answer = k
                .post(who, "/cutoff", json!({"cutoff": "2026-03-31"}))
                .await?;
            assert_eq!(answer.status, StatusCode::FORBIDDEN, "{}", answer.text);
        }
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_cutoff_moves_back_too(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        k.post(&k.nadia, "/cutoff", json!({"cutoff": "2026-03-31"}))
            .await?
            .ok(StatusCode::CREATED)?;
        let moved = k
            .post(&k.nadia, "/cutoff", json!({"cutoff": "2026-02-28"}))
            .await?
            .ok(StatusCode::CREATED)?;
        assert_eq!(text(&moved, "cutoff"), "2026-02-28");
        let events = k
            .get(&k.nadia, "/events?kind=cutoff")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&events), 2);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_broken_cutoff_date_is_refused(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let answer = k
            .post(&k.nadia, "/cutoff", json!({"cutoff": "March"}))
            .await?;
        assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{}", answer.text);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn events_before_the_cutoff_need_adjust(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        k.post(&k.nadia, "/cutoff", json!({"cutoff": "2026-03-31"}))
            .await?
            .ok(StatusCode::CREATED)?;
        // Priya may split Tom's sessions, but not dated before the cutoff.
        let error = k
            .post(
                &k.priya,
                "/splits",
                json!({
                    "session_id": "seed-s4", "dated": "2026-03-26T00:00:00Z",
                    "parts": [{"tags": {"feature": "KD-2"}}]
                }),
            )
            .await?
            .refused(StatusCode::FORBIDDEN)?;
        assert!(error.contains("adjust"), "{error}");
        // Dated now it is fine, and the frozen figures stay.
        k.post(
            &k.priya,
            "/splits",
            json!({
                "session_id": "seed-s4", "parts": [{"tags": {"feature": "KD-2"}}]
            }),
        )
        .await?
        .ok(StatusCode::CREATED)?;
        let frozen = k
            .get(
                &k.nadia,
                &format!("/reports/cost?{MARCH}&by=feature&as_of=2026-04-01T00:00:00Z"),
            )
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(
            text(row(&frozen, Some("KD-2")).ok_or("KD-2")?, "amount"),
            "120.00"
        );
        let live = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&by=feature"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(
            text(row(&live, Some("KD-2")).ok_or("KD-2")?, "amount"),
            "270.00"
        );
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn an_org_manager_adjusts_before_the_cutoff(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        k.post(&k.nadia, "/cutoff", json!({"cutoff": "2026-03-31"}))
            .await?
            .ok(StatusCode::CREATED)?;
        let split = k
            .post(
                &k.nadia,
                "/splits",
                json!({
                    "session_id": "seed-s4", "dated": "2026-03-26T00:00:00Z",
                    "parts": [{"tags": {"feature": "KD-2"}}]
                }),
            )
            .await?
            .ok(StatusCode::CREATED)?;
        assert_eq!(text(&split, "recorded_at"), "2026-03-26T00:00:00Z");
        let frozen = k
            .get(
                &k.nadia,
                &format!("/reports/cost?{MARCH}&by=feature&as_of=2026-04-01T00:00:00Z"),
            )
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(
            text(row(&frozen, Some("KD-2")).ok_or("KD-2")?, "amount"),
            "270.00"
        );
        Ok(())
    }
    .await;
    result.unwrap();
}

// ---- Splits ---------------------------------------------------------------

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_split_on_a_session_moves_its_cost(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let split = k.post(&k.nadia, "/splits", json!({
            "session_id": "seed-s4",
            "parts": [{"fraction": "0.5", "tags": {"feature": "KD-1"}}, {"fraction": "0.5", "tags": {"feature": "KD-2"}}]
        })).await?.ok(StatusCode::CREATED)?;
        assert_eq!(text(&split, "session_id"), "seed-s4");
        assert!(split["message_id"].is_null());
        assert!(split["supersedes"].is_null());
        assert_eq!(count(&split["parts"]), 2);
        assert_eq!(split["parts"][0]["fraction"], "0.5");
        assert_eq!(split["parts"][0]["tags"]["feature"], "KD-1");
        let report = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&by=feature"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(
            text(row(&report, Some("KD-1")).ok_or("KD-1")?, "amount"),
            "655.00"
        );
        assert!(row(&report, None).is_none_or(|r| text(r, "amount") == "100.00"));
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_second_split_supersedes_the_first(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let first = k
            .post(
                &k.nadia,
                "/splits",
                json!({
                    "session_id": "seed-s4", "parts": [{"tags": {"feature": "KD-1"}}]
                }),
            )
            .await?
            .ok(StatusCode::CREATED)?;
        let second = k
            .post(
                &k.nadia,
                "/splits",
                json!({
                    "session_id": "seed-s4", "parts": [{"tags": {"feature": "KD-2"}}]
                }),
            )
            .await?
            .ok(StatusCode::CREATED)?;
        assert_eq!(second["supersedes"], first["id"]);
        let report = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&by=feature"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(
            text(row(&report, Some("KD-2")).ok_or("KD-2")?, "amount"),
            "270.00"
        );
        assert_eq!(
            text(row(&report, Some("KD-1")).ok_or("KD-1")?, "amount"),
            "580.00"
        );
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_split_on_one_message_beats_the_sessions(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        k.post(
            &k.nadia,
            "/splits",
            json!({
                "session_id": "seed-s2", "message_id": "1", "parts": [{"tags": {"feature": "KD-2"}}]
            }),
        )
        .await?
        .ok(StatusCode::CREATED)?;
        let report = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&by=feature"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(
            text(row(&report, Some("KD-2")).ok_or("KD-2")?, "amount"),
            "300.00"
        );
        assert_eq!(
            text(row(&report, Some("KD-1")).ok_or("KD-1")?, "amount"),
            "400.00"
        );
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn fractions_must_add_up_to_one(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let error = k.post(&k.nadia, "/splits", json!({
            "session_id": "seed-s4",
            "parts": [{"fraction": "0.5", "tags": {"feature": "KD-1"}}, {"fraction": "0.3", "tags": {"feature": "KD-2"}}]
        })).await?.refused(StatusCode::BAD_REQUEST)?;
        assert!(!error.is_empty());
        let error = k
            .post(
                &k.nadia,
                "/splits",
                json!({
                    "session_id": "seed-s4", "parts": []
                }),
            )
            .await?
            .refused(StatusCode::BAD_REQUEST)?;
        assert!(!error.is_empty());
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_single_part_may_leave_its_fraction_out(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let split = k
            .post(
                &k.nadia,
                "/splits",
                json!({
                    "session_id": "seed-s4", "parts": [{"tags": {"feature": "KD-2"}}]
                }),
            )
            .await?
            .ok(StatusCode::CREATED)?;
        assert_eq!(split["parts"][0]["fraction"], "1");
        let error = k.post(&k.nadia, "/splits", json!({
            "session_id": "seed-s4",
            "parts": [{"fraction": "0.25", "tags": {"feature": "KD-1"}}, {"tags": {"feature": "KD-2"}}]
        })).await?.refused(StatusCode::BAD_REQUEST)?;
        assert!(error.contains("fraction"), "{error}");
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_split_may_name_a_key_not_yet_on_record(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        // As with ingest: the key counts; the value can be added later.
        k.post(
            &k.nadia,
            "/splits",
            json!({
                "session_id": "seed-s4", "parts": [{"tags": {"feature": "KD-9"}}]
            }),
        )
        .await?
        .ok(StatusCode::CREATED)?;
        let report = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&by=feature"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(
            text(row(&report, Some("KD-9")).ok_or("KD-9")?, "amount"),
            "150.00"
        );
        let report = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&by=project"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(text(row(&report, None).ok_or("none")?, "amount"), "150.00");
        k.post(
            &k.nadia,
            "/tags/feature/values",
            json!({"key": "KD-9", "defaults": {"project": "shop"}}),
        )
        .await?
        .ok(StatusCode::CREATED)?;
        let report = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&by=project"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(
            text(row(&report, Some("shop")).ok_or("shop")?, "amount"),
            "950.00"
        );
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_split_needs_a_session_on_record(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let answer = k
            .post(
                &k.nadia,
                "/splits",
                json!({
                    "session_id": "nobody-s9", "parts": [{"tags": {"feature": "KD-1"}}]
                }),
            )
            .await?;
        assert_eq!(answer.status, StatusCode::NOT_FOUND, "{}", answer.text);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_person_splits_their_own_sessions(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let split = k
            .post(
                &k.tom,
                "/splits",
                json!({
                    "session_id": "seed-s4", "parts": [{"tags": {"feature": "KD-1"}}]
                }),
            )
            .await?
            .ok(StatusCode::CREATED)?;
        assert_eq!(split["parts"][0]["tags"]["feature"], "KD-1");
        let report = k
            .get(&k.tom, &format!("/reports/cost?{MARCH}&by=feature"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(
            text(row(&report, Some("KD-1")).ok_or("KD-1")?, "amount"),
            "150.00"
        );
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn splitting_someone_elses_session_needs_assign(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let error = k
            .post(
                &k.mara,
                "/splits",
                json!({
                    "session_id": "seed-s4", "parts": [{"tags": {"feature": "KD-1"}}]
                }),
            )
            .await?
            .refused(StatusCode::FORBIDDEN)?;
        assert!(error.contains("assign"), "{error}");
        k.post(
            &k.priya,
            "/splits",
            json!({
                "session_id": "seed-s4", "parts": [{"tags": {"feature": "KD-1"}}]
            }),
        )
        .await?
        .ok(StatusCode::CREATED)?;
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_reversed_split_brings_the_default_back(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let split = k
            .post(
                &k.nadia,
                "/splits",
                json!({
                    "session_id": "seed-s4", "parts": [{"tags": {"feature": "KD-1"}}]
                }),
            )
            .await?
            .ok(StatusCode::CREATED)?;
        let reversed = k
            .post(&k.nadia, "/reversals", json!({"event_id": split["id"]}))
            .await?
            .ok(StatusCode::CREATED)?;
        assert_eq!(reversed["event_id"], split["id"]);
        let report = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&by=label"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(
            text(
                row(&report, Some("organization overhead")).ok_or("o")?,
                "amount"
            ),
            "150.00"
        );
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn reversing_the_second_split_brings_the_first_back(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        k.post(
            &k.nadia,
            "/splits",
            json!({
                "session_id": "seed-s4", "parts": [{"tags": {"feature": "KD-1"}}]
            }),
        )
        .await?
        .ok(StatusCode::CREATED)?;
        let second = k
            .post(
                &k.nadia,
                "/splits",
                json!({
                    "session_id": "seed-s4", "parts": [{"tags": {"feature": "KD-2"}}]
                }),
            )
            .await?
            .ok(StatusCode::CREATED)?;
        k.post(&k.nadia, "/reversals", json!({"event_id": second["id"]}))
            .await?
            .ok(StatusCode::CREATED)?;
        let report = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&by=feature"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(
            text(row(&report, Some("KD-1")).ok_or("KD-1")?, "amount"),
            "730.00"
        );
        Ok(())
    }
    .await;
    result.unwrap();
}

// ---- Reversals ----------------------------------------------------------

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_reversal_needs_an_event_on_record(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let answer = k
            .post(&k.nadia, "/reversals", json!({"event_id": "spl_nothing"}))
            .await?;
        assert_eq!(answer.status, StatusCode::NOT_FOUND, "{}", answer.text);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn an_event_is_reversed_once_and_a_reversal_is_not_reversed(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let split = k
            .post(
                &k.nadia,
                "/splits",
                json!({
                    "session_id": "seed-s4", "parts": [{"tags": {"feature": "KD-1"}}]
                }),
            )
            .await?
            .ok(StatusCode::CREATED)?;
        let reversed = k
            .post(&k.nadia, "/reversals", json!({"event_id": split["id"]}))
            .await?
            .ok(StatusCode::CREATED)?;
        let answer = k
            .post(&k.nadia, "/reversals", json!({"event_id": split["id"]}))
            .await?;
        assert_eq!(answer.status, StatusCode::CONFLICT, "{}", answer.text);
        // A reversal is not reversed; the event is recorded again instead.
        let error = k
            .post(&k.nadia, "/reversals", json!({"event_id": reversed["id"]}))
            .await?
            .refused(StatusCode::CONFLICT)?;
        assert!(error.contains("record the event again"), "{error}");
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_reversal_needs_the_capability_of_the_event(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let answer = k
            .post(&k.mara, "/reversals", json!({"event_id": "spl_seed_s2"}))
            .await?;
        assert_eq!(answer.status, StatusCode::CREATED, "{}", answer.text);
        let answer = k
            .post(
                &k.tom,
                "/reversals",
                json!({"event_id": "bil_seed_anthropic_2026_03"}),
            )
            .await?;
        assert_eq!(answer.status, StatusCode::FORBIDDEN, "{}", answer.text);
        let answer = k
            .post(
                &k.priya,
                "/reversals",
                json!({"event_id": "shr_seed_mara_shop"}),
            )
            .await?;
        assert_eq!(answer.status, StatusCode::FORBIDDEN, "{}", answer.text);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_reversed_bill_leaves_reconciliation(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        k.post(
            &k.nadia,
            "/reversals",
            json!({"event_id": "bil_seed_anthropic_2026_03"}),
        )
        .await?
        .ok(StatusCode::CREATED)?;
        let rows = k
            .get(&k.nadia, "/reconciliation")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&rows), 0);
        let bills = k.get(&k.nadia, "/bills").await?.ok(StatusCode::OK)?;
        assert_eq!(count(&bills), 0);
        let all = k
            .get(&k.nadia, "/bills?reversed=true")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&all), 1);
        assert!(all[0]["reversed_by"].is_string());
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_reversal_before_the_cutoff_needs_adjust(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        k.post(&k.nadia, "/cutoff", json!({"cutoff": "2026-03-31"}))
            .await?
            .ok(StatusCode::CREATED)?;
        let answer = k
            .post(
                &k.priya,
                "/reversals",
                json!({"event_id": "spl_seed_s2", "dated": "2026-03-13T00:00:00Z"}),
            )
            .await?;
        assert_eq!(answer.status, StatusCode::FORBIDDEN, "{}", answer.text);
        let answer = k
            .post(
                &k.nadia,
                "/reversals",
                json!({"event_id": "spl_seed_s2", "dated": "2026-03-13T00:00:00Z"}),
            )
            .await?;
        assert_eq!(answer.status, StatusCode::CREATED, "{}", answer.text);
        let frozen = k
            .get(
                &k.nadia,
                &format!("/reports/cost?{MARCH}&by=label&as_of=2026-04-01T00:00:00Z"),
            )
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(
            text(
                row(&frozen, Some("organization overhead")).ok_or("o")?,
                "amount"
            ),
            "450.00"
        );
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_reversed_tag_value_loses_its_defaults(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        k.post(
            &k.nadia,
            "/reversals",
            json!({"event_id": "tag_seed_feature_kd_2"}),
        )
        .await?
        .ok(StatusCode::CREATED)?;
        let tags = k.get(&k.nadia, "/tags").await?.ok(StatusCode::OK)?;
        let feature = tags
            .as_array()
            .ok_or("array")?
            .iter()
            .find(|t| text(t, "name") == "feature")
            .ok_or("feature")?;
        assert_eq!(count(&feature["values"]), 1);
        // The key stays on the split; the project it brought along is gone.
        let report = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&by=feature"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(
            text(row(&report, Some("KD-2")).ok_or("KD-2")?, "amount"),
            "120.00"
        );
        let report = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&by=project"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(
            text(row(&report, Some("shop")).ok_or("shop")?, "amount"),
            "680.00"
        );
        Ok(())
    }
    .await;
    result.unwrap();
}

// ---- Events ---------------------------------------------------------------

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn events_list_the_log_in_order_and_a_limit_keeps_the_latest(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let events = k
            .get(&k.nadia, "/events?limit=3")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&events), 3);
        assert_eq!(events[0]["id"], "msg_seed_s3");
        assert_eq!(events[2]["id"], "bil_seed_anthropic_2026_03");
        assert_eq!(events[2]["kind"], "bill");
        assert_eq!(events[2]["recorded_by"], emails::NADIA);
        let all = k.get(&k.nadia, "/events").await?.ok(StatusCode::OK)?;
        let moments: Vec<&str> = all
            .as_array()
            .ok_or("array")?
            .iter()
            .map(|e| text(e, "recorded_at"))
            .collect();
        assert!(
            moments.windows(2).all(|pair| pair[0] <= pair[1]),
            "{moments:?}"
        );
        assert!(all.as_array().ok_or("array")?.len() > 20);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn events_filter_by_kind(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let events = k
            .get(&k.nadia, "/events?kind=message")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&events), 4);
        let events = k
            .get(&k.nadia, "/events?kind=split")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&events), 1);
        assert_eq!(events[0]["record"]["split"]["parts"][0]["weight"], 60);
        let events = k
            .get(&k.nadia, "/events?kind=tag%20value")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&events), 4);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn events_as_of_a_moment(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let events = k
            .get(&k.nadia, "/events?as_of=2026-02-01T09:00:00Z&kind=message")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&events), 0);
        let events = k
            .get(&k.nadia, "/events?as_of=2026-03-31&kind=bill")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&events), 0);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn an_unknown_kind_lists_nothing(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let answer = k.get(&k.nadia, "/events?kind=receipt").await?;
        assert!(
            matches!(answer.status, StatusCode::OK | StatusCode::BAD_REQUEST),
            "{}",
            answer.text
        );
        if answer.status == StatusCode::OK {
            assert_eq!(count(&answer.json), 0);
        }
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_log_shows_each_person_what_they_may_view(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let events = k
            .get(&k.mara, "/events?kind=message")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&events), 2);
        assert!(
            events
                .as_array()
                .ok_or("array")?
                .iter()
                .all(|e| e["recorded_by"] == emails::MARA)
        );
        let events = k
            .get(&k.tom, "/events?kind=split")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&events), 0);
        let events = k
            .get(&k.priya, "/events?kind=message")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&events), 4);
        Ok(())
    }
    .await;
    result.unwrap();
}

// ---- Estimates ----------------------------------------------------------

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn an_estimate_is_read_back_with_its_cost_so_far(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let estimate = k
            .get(&k.nadia, "/estimates/est_seed_kd_1")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(text(&estimate, "median"), "500.00");
        assert_eq!(text(&estimate, "amount"), "500.00");
        assert_eq!(estimate["accepted"], true);
        assert_eq!(text(&estimate, "cost"), "580.00");
        assert_eq!(text(&estimate, "deviation"), "-80.00");
        assert_eq!(estimate["tags"]["feature"], "KD-1");
        assert_eq!(text(&estimate, "from"), "2026-03-01");
        assert_eq!(text(&estimate, "estimator"), "classifier");
        assert_eq!(estimate["markup_percent"], 0);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_typed_estimate_is_accepted_and_becomes_the_plan(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let estimate = k
            .post(
                &k.nadia,
                "/estimates",
                json!({
                    "work": "Returns portal", "tags": {"feature": "KD-2"}, "median": "200",
                    "from": "2026-03-01", "to": "2026-03-31"
                }),
            )
            .await?
            .ok(StatusCode::CREATED)?;
        assert_eq!(estimate["accepted"], false);
        assert_eq!(text(&estimate, "estimator"), emails::NADIA);
        let id = text(&estimate, "id").to_owned();
        let accepted = k
            .post(&k.nadia, &format!("/estimates/{id}/accept"), Value::Null)
            .await?
            .ok(StatusCode::CREATED)?;
        assert_eq!(accepted["estimate"]["accepted"], true);
        let report = k
            .get(&k.nadia, &format!("/reports/deviation?{MARCH}&by=feature"))
            .await?
            .ok(StatusCode::OK)?;
        let kd2 = row(&report, Some("KD-2")).ok_or("KD-2")?;
        assert_eq!(text(kd2, "plan"), "200.00");
        assert_eq!(text(kd2, "deviation"), "80.00");
        assert_eq!(kd2["favourable"], true);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn accepting_another_estimate_supersedes_the_plan(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let estimate = k
            .post(
                &k.nadia,
                "/estimates",
                json!({
                    "work": "Checkout redesign, revised", "tags": {"feature": "KD-1"}, "median": "600",
                    "from": "2026-03-01", "to": "2026-03-31"
                }),
            )
            .await?
            .ok(StatusCode::CREATED)?;
        let id = text(&estimate, "id").to_owned();
        k.post(&k.nadia, &format!("/estimates/{id}/accept"), Value::Null)
            .await?
            .ok(StatusCode::CREATED)?;
        let report = k
            .get(&k.nadia, &format!("/reports/deviation?{MARCH}&by=feature"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(
            text(row(&report, Some("KD-1")).ok_or("KD-1")?, "plan"),
            "600.00"
        );
        let earlier = k
            .get(
                &k.nadia,
                &format!("/reports/deviation?{MARCH}&by=feature&as_of=2026-03-31"),
            )
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(
            text(row(&earlier, Some("KD-1")).ok_or("KD-1")?, "plan"),
            "500.00"
        );
        let old = k
            .get(&k.nadia, "/estimates/est_seed_kd_1")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(old["accepted"], false);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_markup_raises_the_amount(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let estimate = k.post(&k.nadia, "/estimates", json!({
            "work": "Returns portal", "tags": {"feature": "KD-2"}, "median": "200", "markup_percent": 25,
            "from": "2026-03-01", "to": "2026-03-31"
        })).await?.ok(StatusCode::CREATED)?;
        assert_eq!(text(&estimate, "median"), "200.00");
        assert_eq!(text(&estimate, "amount"), "250.00");
        assert_eq!(estimate["markup_percent"], 25);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn an_estimate_with_bands_keeps_them(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let estimate = k.post(&k.nadia, "/estimates", json!({
            "work": "Returns portal", "tags": {"feature": "KD-2"}, "median": "200",
            "distribution": [{"p": "0.5", "up_to": "200"}, {"p": "0.3", "up_to": "400"}, {"p": "0.2"}],
            "from": "2026-03-01", "to": "2026-03-31"
        })).await?.ok(StatusCode::CREATED)?;
        assert_eq!(count(&estimate["distribution"]), 3);
        assert!(estimate["distribution"][2]["up_to"].is_null());
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn an_estimate_needs_plan_on_its_tags(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let answer = k
            .post(
                &k.mara,
                "/estimates",
                json!({
                    "work": "Returns portal", "tags": {"feature": "KD-2"}, "median": "200",
                    "from": "2026-03-01", "to": "2026-03-31"
                }),
            )
            .await?;
        assert_eq!(answer.status, StatusCode::FORBIDDEN, "{}", answer.text);
        let answer = k
            .post(
                &k.priya,
                "/estimates",
                json!({
                    "work": "Web team in April", "tags": {"team": "web"}, "median": "900",
                    "from": "2026-04-01", "to": "2026-04-30"
                }),
            )
            .await?;
        assert_eq!(answer.status, StatusCode::CREATED, "{}", answer.text);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn an_estimate_without_a_median_or_classifier_is_refused(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let answer = k.post(&k.nadia, "/estimates", json!({
            "work": "Something", "tags": {"feature": "KD-2"}, "from": "2026-03-01", "to": "2026-03-31"
        })).await?;
        assert!(
            matches!(
                answer.status,
                StatusCode::BAD_REQUEST | StatusCode::SERVICE_UNAVAILABLE | StatusCode::CONFLICT
            ),
            "{}",
            answer.text
        );
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn an_estimate_on_a_value_not_on_record_is_refused(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let answer = k.post(&k.nadia, "/estimates", json!({
            "work": "Something", "tags": {"feature": "KD-9"}, "median": "1", "from": "2026-03-01", "to": "2026-03-31"
        })).await?;
        assert!(
            matches!(
                answer.status,
                StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND
            ),
            "{}",
            answer.text
        );
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn an_unknown_estimate_is_not_found(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let answer = k.get(&k.nadia, "/estimates/est_nothing").await?;
        assert_eq!(answer.status, StatusCode::NOT_FOUND);
        let answer = k
            .post(&k.nadia, "/estimates/est_nothing/accept", Value::Null)
            .await?;
        assert_eq!(answer.status, StatusCode::NOT_FOUND);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn an_estimate_outside_the_view_is_not_found(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        // KD-1 is not inside `person: mara`, nor inside `team: web`.
        let answer = k.get(&k.mara, "/estimates/est_seed_kd_1").await?;
        assert_eq!(answer.status, StatusCode::NOT_FOUND, "{}", answer.text);
        let answer = k.get(&k.priya, "/estimates/est_seed_kd_1").await?;
        assert_eq!(answer.status, StatusCode::NOT_FOUND, "{}", answer.text);
        Ok(())
    }
    .await;
    result.unwrap();
}

// ---- Submissions ----------------------------------------------------------

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_person_submits_their_open_messages(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let submission = k
            .post(&k.mara, "/submissions", json!({"to": "2026-03-31"}))
            .await?
            .ok(StatusCode::CREATED)?;
        assert_eq!(text(&submission, "person"), emails::MARA);
        assert_eq!(submission["messages"], 2);
        assert_eq!(text(&submission, "cost"), "700.00");
        assert_eq!(text(&submission, "status"), "submitted");
        assert_eq!(text(&submission, "to"), "2026-03-31");
        let sessions = k
            .get(&k.mara, &format!("/sessions?{MARCH}"))
            .await?
            .ok(StatusCode::OK)?;
        assert!(
            sessions["sessions"]
                .as_array()
                .ok_or("array")?
                .iter()
                .all(|s| text(s, "submitted") == "submitted")
        );
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_second_submit_starts_where_the_last_ended(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let first = k
            .post(&k.tom, "/submissions", json!({"to": "2026-03-20"}))
            .await?
            .ok(StatusCode::CREATED)?;
        assert_eq!(first["messages"], 1);
        let second = k
            .post(&k.tom, "/submissions", json!({"to": "2026-03-31"}))
            .await?
            .ok(StatusCode::CREATED)?;
        assert_eq!(second["messages"], 1);
        assert_eq!(text(&second, "from"), "2026-03-21");
        let list = k.get(&k.tom, "/submissions").await?.ok(StatusCode::OK)?;
        assert_eq!(count(&list), 2);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn an_empty_submit_says_there_was_nothing(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let submission = k
            .post(&k.mara, "/submissions", json!({"to": "2026-02-28"}))
            .await?
            .ok(StatusCode::CREATED)?;
        assert_eq!(submission["messages"], 0);
        assert_eq!(text(&submission, "cost"), "0.00");
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_submission_is_approved_by_someone_with_approve(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let submission = k
            .post(&k.mara, "/submissions", json!({"to": "2026-03-31"}))
            .await?
            .ok(StatusCode::CREATED)?;
        let id = text(&submission, "id").to_owned();
        let approved = k
            .post(&k.priya, &format!("/submissions/{id}/approve"), Value::Null)
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(text(&approved, "status"), "approved");
        let report = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&approved=true"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(text(&report, "total"), "700.00");
        let sessions = k
            .get(&k.nadia, &format!("/sessions?{MARCH}&session_id=seed-s1"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(sessions["sessions"][0]["submitted"], "approved");
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_submission_is_rejected_with_a_reason(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let submission = k
            .post(&k.mara, "/submissions", json!({"to": "2026-03-31"}))
            .await?
            .ok(StatusCode::CREATED)?;
        let id = text(&submission, "id").to_owned();
        let answer = k
            .post(
                &k.nadia,
                &format!("/submissions/{id}/reject"),
                json!({"reason": ""}),
            )
            .await?;
        assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{}", answer.text);
        let rejected = k
            .post(
                &k.nadia,
                &format!("/submissions/{id}/reject"),
                json!({"reason": "S2 is all KD-2"}),
            )
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(text(&rejected, "status"), "rejected");
        assert_eq!(text(&rejected, "reason"), "S2 is all KD-2");
        let sessions = k
            .get(&k.mara, &format!("/sessions?{MARCH}&session_id=seed-s2"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(sessions["sessions"][0]["submitted"], "rejected");
        // Fixed and submitted again.
        let again = k
            .post(&k.mara, "/submissions", json!({"to": "2026-03-31"}))
            .await?
            .ok(StatusCode::CREATED)?;
        assert_eq!(again["messages"], 2);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn nobody_approves_their_own_submission(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let submission = k
            .post(&k.nadia, "/submissions", json!({"to": "2026-03-31"}))
            .await?
            .ok(StatusCode::CREATED)?;
        let id = text(&submission, "id").to_owned();
        let answer = k
            .post(&k.nadia, &format!("/submissions/{id}/approve"), Value::Null)
            .await?;
        assert_eq!(answer.status, StatusCode::FORBIDDEN, "{}", answer.text);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn approving_needs_approve_on_the_messages(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let submission = k
            .post(&k.mara, "/submissions", json!({"to": "2026-03-31"}))
            .await?
            .ok(StatusCode::CREATED)?;
        let id = text(&submission, "id").to_owned();
        let answer = k
            .post(&k.tom, &format!("/submissions/{id}/approve"), Value::Null)
            .await?;
        assert_eq!(answer.status, StatusCode::FORBIDDEN, "{}", answer.text);
        let answer = k
            .post(
                &k.tom,
                &format!("/submissions/{id}/reject"),
                json!({"reason": "no"}),
            )
            .await?;
        assert_eq!(answer.status, StatusCode::FORBIDDEN, "{}", answer.text);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_later_decision_supersedes_the_earlier_one(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let submission = k
            .post(&k.mara, "/submissions", json!({"to": "2026-03-31"}))
            .await?
            .ok(StatusCode::CREATED)?;
        let id = text(&submission, "id").to_owned();
        k.post(&k.nadia, &format!("/submissions/{id}/approve"), Value::Null)
            .await?
            .ok(StatusCode::OK)?;
        let rejected = k
            .post(
                &k.nadia,
                &format!("/submissions/{id}/reject"),
                json!({"reason": "late"}),
            )
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(text(&rejected, "status"), "rejected");
        let report = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&approved=true"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(text(&report, "total"), "0.00");
        let list = k
            .get(&k.nadia, &format!("/submissions?as_of={}", "2026-10-19"))
            .await?;
        assert_eq!(list.status, StatusCode::OK);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn an_unknown_submission_is_not_found(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let answer = k
            .post(&k.nadia, "/submissions/sub_nothing/approve", Value::Null)
            .await?;
        assert_eq!(answer.status, StatusCode::NOT_FOUND, "{}", answer.text);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn submissions_list_by_person_inside_the_view(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        k.post(&k.mara, "/submissions", json!({"to": "2026-03-31"}))
            .await?
            .ok(StatusCode::CREATED)?;
        k.post(&k.tom, "/submissions", json!({"to": "2026-03-31"}))
            .await?
            .ok(StatusCode::CREATED)?;
        let all = k.get(&k.priya, "/submissions").await?.ok(StatusCode::OK)?;
        assert_eq!(count(&all), 2);
        let maras = k
            .get(&k.priya, &format!("/submissions?person={}", emails::MARA))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&maras), 1);
        let own = k.get(&k.tom, "/submissions").await?.ok(StatusCode::OK)?;
        assert_eq!(count(&own), 1);
        assert_eq!(text(&own[0], "person"), emails::TOM);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_submit_request_asks_everyone(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let today = tokencontroller_server::dates::text(time::OffsetDateTime::now_utc().date());
        let request = k
            .post(
                &k.nadia,
                "/submit-requests",
                json!({"until": today, "message": "Month end."}),
            )
            .await?
            .ok(StatusCode::CREATED)?;
        assert_eq!(text(&request, "until"), today);
        assert_eq!(text(&request, "requested_by"), emails::NADIA);
        assert_eq!(text(&request, "message"), "Month end.");
        assert_eq!(request["done"], false);
        let seen = k
            .get(&k.mara, "/submit-requests")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&seen), 1);
        assert_eq!(seen[0]["done"], false);
        // A submit up to the day asked for is done; one that stops short is not.
        k.post(&k.mara, "/submissions", json!({"to": "2026-03-31"}))
            .await?
            .ok(StatusCode::CREATED)?;
        let seen = k
            .get(&k.mara, "/submit-requests")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(seen[0]["done"], false);
        k.post(&k.mara, "/submissions", json!({}))
            .await?
            .ok(StatusCode::CREATED)?;
        let seen = k
            .get(&k.mara, "/submit-requests")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(seen[0]["done"], true);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn only_an_org_manager_requests_submits(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let answer = k
            .post(&k.priya, "/submit-requests", json!({"until": "2099-03-31"}))
            .await?;
        assert_eq!(answer.status, StatusCode::FORBIDDEN, "{}", answer.text);
        let answer = k
            .post(&k.nadia, "/submit-requests", json!({"until": "soon"}))
            .await?;
        assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{}", answer.text);
        let long = "x".repeat(501);
        let answer = k
            .post(
                &k.nadia,
                "/submit-requests",
                json!({"until": "2099-03-31", "message": long}),
            )
            .await?;
        assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{}", answer.text);
        Ok(())
    }
    .await;
    result.unwrap();
}

// ---- Price table ----------------------------------------------------------

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_price_table_shows_the_organizations_row(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let table = k.get(&k.nadia, "/price-table").await?.ok(StatusCode::OK)?;
        assert_eq!(count(&table["rows"]), 1);
        let row = &table["rows"][0];
        assert_eq!(text(row, "source"), "organization");
        assert_eq!(text(row, "price_per_million"), "10");
        assert_eq!(text(row, "kind"), "input");
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_new_price_row_prices_from_its_day(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let row = k
            .post(
                &k.nadia,
                "/price-table/rows",
                json!({
                    "provider": "anthropic", "model": seed::MODEL, "kind": "input",
                    "price_per_million": "5", "valid_from": "2026-03-15"
                }),
            )
            .await?
            .ok(StatusCode::CREATED)?;
        assert_eq!(text(&row, "valid_from"), "2026-03-15");
        let report = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}"))
            .await?
            .ok(StatusCode::OK)?;
        // S1 400 + S2 300 at 10; S3 50 + S4 75 at 5.
        assert_eq!(text(&report, "total"), "825.00");
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn an_output_row_prices_what_was_unknown(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let table = k
            .get(
                &k.nadia,
                "/price-table?provider=anthropic&model=claude-opus-5",
            )
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&table["rows"]), 0);
        k.post(
            &k.nadia,
            "/price-table/rows",
            json!({
                "provider": "anthropic", "model": "claude-opus-5", "kind": "output",
                "price_per_million": "75", "valid_from": "2026-01-01"
            }),
        )
        .await?
        .ok(StatusCode::CREATED)?;
        let table = k
            .get(&k.nadia, "/price-table?model=claude-opus-5")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&table["rows"]), 1);
        let table = k
            .get(&k.nadia, "/price-table?provider=openai")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&table["rows"]), 0);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_price_row_needs_a_known_kind_and_a_price(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let answer = k
            .post(
                &k.nadia,
                "/price-table/rows",
                json!({
                    "provider": "anthropic", "model": seed::MODEL, "kind": "thinking",
                    "price_per_million": "5", "valid_from": "2026-03-15"
                }),
            )
            .await?;
        assert_eq!(
            answer.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{}",
            answer.text
        );
        let answer = k
            .post(
                &k.nadia,
                "/price-table/rows",
                json!({
                    "provider": "anthropic", "model": seed::MODEL, "kind": "input",
                    "price_per_million": "-5", "valid_from": "2026-03-15"
                }),
            )
            .await?;
        assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{}", answer.text);
        let answer = k
            .post(
                &k.nadia,
                "/price-table/rows",
                json!({
                    "provider": "", "model": seed::MODEL, "kind": "input",
                    "price_per_million": "5", "valid_from": "2026-03-15"
                }),
            )
            .await?;
        assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{}", answer.text);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn only_an_org_manager_adds_price_rows(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let answer = k
            .post(
                &k.priya,
                "/price-table/rows",
                json!({
                    "provider": "anthropic", "model": seed::MODEL, "kind": "input",
                    "price_per_million": "5", "valid_from": "2026-03-15"
                }),
            )
            .await?;
        assert_eq!(answer.status, StatusCode::FORBIDDEN, "{}", answer.text);
        let table = k.get(&k.mara, "/price-table").await?.ok(StatusCode::OK)?;
        assert_eq!(count(&table["rows"]), 1);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_price_table_as_of_before_its_row_is_empty(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let table = k
            .get(&k.nadia, "/price-table?as_of=2026-01-31")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&table["rows"]), 0);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_reversed_price_row_leaves_the_messages_unpriced(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        k.post(
            &k.nadia,
            "/reversals",
            json!({"event_id": "prc_seed_sonnet_input"}),
        )
        .await?
        .ok(StatusCode::CREATED)?;
        let report = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(text(&report, "total"), "0.00");
        assert_eq!(count(&report["unknown_models"]), 1);
        let rows = k
            .get(&k.nadia, "/reconciliation")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&rows[0]["unknown_models"]), 1);
        assert_eq!(text(&rows[0], "unexplained"), "1000.00");
        Ok(())
    }
    .await;
    result.unwrap();
}

// ---- Tags and attributes -----------------------------------------------

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn tags_list_the_built_in_names_and_their_values(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let tags = k.get(&k.nadia, "/tags").await?.ok(StatusCode::OK)?;
        let names: Vec<&str> = tags
            .as_array()
            .ok_or("array")?
            .iter()
            .map(|t| text(t, "name"))
            .collect();
        assert_eq!(names, ["client", "feature", "project"]);
        let feature = &tags[1];
        assert_eq!(feature["built_in"], true);
        assert_eq!(count(&feature["values"]), 2);
        assert_eq!(feature["values"][0]["key"], "KD-1");
        assert_eq!(feature["values"][0]["title"], "Checkout redesign");
        assert_eq!(feature["values"][0]["defaults"]["project"], "shop");
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_tag_value_with_defaults_moves_cost_in_reports(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let value = k
            .post(
                &k.nadia,
                "/tags/feature/values",
                json!({
                    "key": "KD-3", "title": "Wishlist", "defaults": {"project": "shop"}
                }),
            )
            .await?
            .ok(StatusCode::CREATED)?;
        assert_eq!(text(&value, "name"), "feature");
        assert_eq!(text(&value, "key"), "KD-3");
        assert_eq!(value["defaults"]["project"], "shop");
        k.post(
            &k.nadia,
            "/splits",
            json!({
                "session_id": "seed-s4", "parts": [{"tags": {"feature": "KD-3"}}]
            }),
        )
        .await?
        .ok(StatusCode::CREATED)?;
        let report = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&by=client"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(
            text(row(&report, Some("acme")).ok_or("acme")?, "amount"),
            "950.00"
        );
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_tag_value_recorded_again_supersedes_its_record(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let again = k
            .post(
                &k.nadia,
                "/tags/feature/values",
                json!({
                    "key": "KD-1", "title": "Checkout, second round", "defaults": {"project": "shop"}
                }),
            )
            .await?
            .ok(StatusCode::CREATED)?;
        assert_eq!(text(&again, "title"), "Checkout, second round");
        let tags = k.get(&k.nadia, "/tags").await?.ok(StatusCode::OK)?;
        let feature = &tags[1];
        assert_eq!(count(&feature["values"]), 2);
        assert_eq!(feature["values"][0]["title"], "Checkout, second round");
        let old = k
            .get(&k.nadia, "/tags?as_of=2026-03-01")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(old[1]["values"][0]["title"], "Checkout redesign");
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_dated_tag_value_is_on_record_from_its_date(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let value = k
            .post(
                &k.nadia,
                "/tags/project/values",
                json!({
                    "key": "warehouse", "dated": "2026-02-15T00:00:00Z"
                }),
            )
            .await?
            .ok(StatusCode::CREATED)?;
        assert_eq!(text(&value, "recorded_at"), "2026-02-15T00:00:00Z");
        let then = k
            .get(&k.nadia, "/tags?as_of=2026-02-14")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&then[2]["values"]), 1);
        let later = k
            .get(&k.nadia, "/tags?as_of=2026-02-16")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&later[2]["values"]), 2);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_tag_value_needs_a_key_and_known_defaults(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let answer = k
            .post(&k.nadia, "/tags/feature/values", json!({"key": " "}))
            .await?;
        assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{}", answer.text);
        let answer = k
            .post(
                &k.nadia,
                "/tags/feature/values",
                json!({"key": "KD-3", "defaults": {"project": "nowhere"}}),
            )
            .await?;
        assert!(
            matches!(
                answer.status,
                StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND
            ),
            "{}",
            answer.text
        );
        let answer = k
            .post(&k.nadia, "/tags/sprint/values", json!({"key": "12"}))
            .await?;
        assert_eq!(answer.status, StatusCode::NOT_FOUND, "{}", answer.text);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn an_agent_manager_records_no_tag_values(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let answer = k
            .post(&k.mara, "/tags/feature/values", json!({"key": "KD-3"}))
            .await?;
        assert_eq!(answer.status, StatusCode::FORBIDDEN, "{}", answer.text);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_people_manager_records_values_inside_their_filter(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let answer = k
            .post(
                &k.priya,
                "/tags/feature/values",
                json!({"key": "KD-3", "defaults": {"project": "shop"}}),
            )
            .await?;
        assert!(
            matches!(answer.status, StatusCode::CREATED | StatusCode::FORBIDDEN),
            "{}",
            answer.text
        );
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_new_tag_name_takes_values_and_groups_reports(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        k.post(&k.nadia, "/tags", json!({"name": "sprint"}))
            .await?
            .ok(StatusCode::CREATED)?;
        k.post(&k.nadia, "/tags/sprint/values", json!({"key": "12"}))
            .await?
            .ok(StatusCode::CREATED)?;
        k.post(
            &k.nadia,
            "/splits",
            json!({
                "session_id": "seed-s4", "parts": [{"tags": {"sprint": "12"}}]
            }),
        )
        .await?
        .ok(StatusCode::CREATED)?;
        let report = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&by=sprint"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(
            text(row(&report, Some("12")).ok_or("12")?, "amount"),
            "150.00"
        );
        let tags = k.get(&k.nadia, "/tags").await?.ok(StatusCode::OK)?;
        assert_eq!(count(&tags), 4);
        assert_eq!(tags[3]["built_in"], false);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_tag_name_is_taken_away_with_its_values(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        k.post(&k.nadia, "/tags", json!({"name": "sprint"}))
            .await?
            .ok(StatusCode::CREATED)?;
        let gone = k
            .send(&k.nadia, Method::DELETE, "/tags/sprint", None)
            .await?;
        assert_eq!(gone.status, StatusCode::NO_CONTENT, "{}", gone.text);
        let tags = k.get(&k.nadia, "/tags").await?.ok(StatusCode::OK)?;
        assert_eq!(count(&tags), 3);
        let answer = k
            .send(&k.nadia, Method::DELETE, "/tags/feature", None)
            .await?;
        assert_eq!(answer.status, StatusCode::CONFLICT, "{}", answer.text);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn attributes_list_teams_and_departments(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let attributes = k.get(&k.nadia, "/attributes").await?.ok(StatusCode::OK)?;
        let names: Vec<&str> = attributes
            .as_array()
            .ok_or("array")?
            .iter()
            .map(|a| text(a, "name"))
            .collect();
        assert_eq!(names, ["department", "location", "team"]);
        assert_eq!(attributes[2]["values"][0]["key"], "web");
        assert_eq!(
            attributes[2]["values"][0]["defaults"]["department"],
            "engineering"
        );
        assert_eq!(count(&attributes[1]["values"]), 0);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_person_moves_team_from_a_date(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        k.post(
            &k.nadia,
            "/attributes/team/values",
            json!({"key": "mobile", "defaults": {"department": "engineering"}}),
        )
        .await?
        .ok(StatusCode::CREATED)?;
        let moved = k
            .post(
                &k.nadia,
                &format!("/people/{}/attributes", ids::TOM),
                json!({
                    "name": "team", "key": "mobile", "from": "2026-03-20"
                }),
            )
            .await?
            .ok(StatusCode::CREATED)?;
        assert_eq!(text(&moved, "from"), "2026-03-20");
        assert_eq!(text(&moved, "person"), emails::TOM);
        let report = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&by=team"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(
            text(row(&report, Some("mobile")).ok_or("mobile")?, "amount"),
            "150.00"
        );
        assert_eq!(
            text(row(&report, Some("web")).ok_or("web")?, "amount"),
            "800.00"
        );
        let me = k.get(&k.tom, "/me").await?.ok(StatusCode::OK)?;
        assert_eq!(me["person"]["attributes"]["team"], "mobile");
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_person_attribute_needs_a_value_on_record(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let answer = k
            .post(
                &k.nadia,
                &format!("/people/{}/attributes", ids::TOM),
                json!({
                    "name": "team", "key": "mobile"
                }),
            )
            .await?;
        assert!(
            matches!(
                answer.status,
                StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND
            ),
            "{}",
            answer.text
        );
        let answer = k
            .post(
                &k.nadia,
                &format!("/people/{}/attributes", Uuid::nil()),
                json!({
                    "name": "team", "key": "web"
                }),
            )
            .await?;
        assert_eq!(answer.status, StatusCode::NOT_FOUND, "{}", answer.text);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_people_manager_moves_people_inside_their_filter(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let answer = k
            .post(
                &k.mara,
                &format!("/people/{}/attributes", ids::TOM),
                json!({"name": "team", "key": "web"}),
            )
            .await?;
        assert_eq!(answer.status, StatusCode::FORBIDDEN, "{}", answer.text);
        let answer = k
            .post(
                &k.priya,
                &format!("/people/{}/attributes", ids::TOM),
                json!({"name": "team", "key": "web"}),
            )
            .await?;
        assert!(
            matches!(answer.status, StatusCode::CREATED | StatusCode::FORBIDDEN),
            "{}",
            answer.text
        );
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_new_attribute_name_takes_values(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        k.post(&k.nadia, "/attributes", json!({"name": "office"}))
            .await?
            .ok(StatusCode::CREATED)?;
        let value = k
            .post(
                &k.nadia,
                "/attributes/office/values",
                json!({"key": "vienna", "title": "Vienna"}),
            )
            .await?
            .ok(StatusCode::CREATED)?;
        assert_eq!(text(&value, "title"), "Vienna");
        let attributes = k.get(&k.nadia, "/attributes").await?.ok(StatusCode::OK)?;
        assert_eq!(count(&attributes), 4);
        let answer = k
            .post(&k.nadia, "/attributes", json!({"name": "feature"}))
            .await?;
        assert_eq!(answer.status, StatusCode::CONFLICT, "{}", answer.text);
        Ok(())
    }
    .await;
    result.unwrap();
}

// ---- Rules ---------------------------------------------------------------

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn an_organization_rule_tags_arriving_messages(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let rule = k
            .post(
                &k.nadia,
                "/rules",
                json!({
                    "name": "feature key in branch", "signal": "branch",
                    "pattern": "(?<key>KD-\\d+)", "tag_name": "feature"
                }),
            )
            .await?
            .ok(StatusCode::CREATED)?;
        assert_eq!(text(&rule, "scope"), "organization");
        assert_eq!(rule["order"], 10);
        assert!(rule["key"].is_null());
        let rules = k.get(&k.nadia, "/rules").await?.ok(StatusCode::OK)?;
        assert_eq!(count(&rules), 1);
        let context = k.get(&k.mara, "/me/context").await?.ok(StatusCode::OK)?;
        assert_eq!(count(&context["rules"]), 1);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_rule_with_a_fixed_key_needs_the_value_on_record(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let answer = k
            .post(
                &k.nadia,
                "/rules",
                json!({
                    "name": "reviews are shop", "signal": "agent_name",
                    "pattern": "review", "tag_name": "project", "key": "warehouse"
                }),
            )
            .await?;
        assert!(
            matches!(
                answer.status,
                StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND
            ),
            "{}",
            answer.text
        );
        let rule = k
            .post(
                &k.nadia,
                "/rules",
                json!({
                    "name": "reviews are shop", "signal": "agent_name",
                    "pattern": "review", "tag_name": "project", "key": "shop"
                }),
            )
            .await?
            .ok(StatusCode::CREATED)?;
        assert_eq!(text(&rule, "key"), "shop");
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_rule_needs_a_signal_and_a_pattern(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let answer = k
            .post(
                &k.nadia,
                "/rules",
                json!({
                    "name": "x", "signal": "weather", "pattern": "a", "tag_name": "feature"
                }),
            )
            .await?;
        assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{}", answer.text);
        let answer = k
            .post(
                &k.nadia,
                "/rules",
                json!({
                    "name": "x", "signal": "branch", "pattern": "(", "tag_name": "feature"
                }),
            )
            .await?;
        assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{}", answer.text);
        let answer = k
            .post(
                &k.nadia,
                "/rules",
                json!({
                    "name": "x", "signal": "branch", "pattern": "a", "tag_name": "sprint"
                }),
            )
            .await?;
        assert!(
            matches!(
                answer.status,
                StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND
            ),
            "{}",
            answer.text
        );
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn organization_rules_need_assign(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let answer = k
            .post(
                &k.mara,
                "/rules",
                json!({
                    "name": "x", "signal": "branch", "pattern": "(?<key>KD-\\d+)", "tag_name": "feature"
                }),
            )
            .await?;
        assert_eq!(answer.status, StatusCode::FORBIDDEN, "{}", answer.text);
        let answer = k
            .post(
                &k.priya,
                "/rules",
                json!({
                    "name": "x", "signal": "branch", "pattern": "(?<key>KD-\\d+)", "tag_name": "feature"
                }),
            )
            .await?;
        assert_eq!(answer.status, StatusCode::CREATED, "{}", answer.text);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_person_keeps_their_own_rules(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let rules = k
            .send(
                &k.mara,
                Method::PUT,
                "/me/rules",
                Some(json!([{
                    "name": "my checkout branches", "signal": "branch",
                    "pattern": "checkout", "tag_name": "feature", "key": "KD-1"
                }])),
            )
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&rules), 1);
        assert_eq!(text(&rules[0], "scope"), emails::MARA);
        let mine = k.get(&k.mara, "/me/rules").await?.ok(StatusCode::OK)?;
        assert_eq!(count(&mine), 1);
        let toms = k.get(&k.tom, "/me/rules").await?.ok(StatusCode::OK)?;
        assert_eq!(count(&toms), 0);
        let organization = k.get(&k.nadia, "/rules").await?.ok(StatusCode::OK)?;
        assert_eq!(count(&organization), 0);
        let none = k
            .send(&k.mara, Method::PUT, "/me/rules", Some(json!([])))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&none), 0);
        Ok(())
    }
    .await;
    result.unwrap();
}

// ---- Me, context, organization and members -------------------------------

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_context_hands_a_client_everything_it_needs(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let context = k.get(&k.mara, "/me/context").await?.ok(StatusCode::OK)?;
        assert_eq!(text(&context, "person"), emails::MARA);
        assert_eq!(text(&context, "reporting_currency"), "USD");
        assert_eq!(context["organization"]["slug"], "kestrel");
        assert_eq!(context["attributes"]["team"], "web");
        assert_eq!(count(&context["names"]), 6);
        // Autocomplete offers only values inside the view: Mara views herself.
        assert_eq!(count(&context["tag_values"]), 0);
        assert_eq!(count(&context["price_table"]), 1);
        assert!(context["cutoff"].is_null());
        let context = k.get(&k.nadia, "/me/context").await?.ok(StatusCode::OK)?;
        assert_eq!(count(&context["tag_values"]), 4);
        assert_eq!(count(&context["attribute_values"]), 2);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn me_shows_the_role_and_its_filters(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let me = k.get(&k.priya, "/me").await?.ok(StatusCode::OK)?;
        assert_eq!(me["person"]["role"], "people_manager");
        assert_eq!(me["person"]["filters"][0]["team"], "web");
        assert_eq!(text(&me, "credential"), "session");
        assert!(me["org_manager_notice"].is_null());
        let me = k.get(&k.nadia, "/me").await?.ok(StatusCode::OK)?;
        assert_eq!(me["person"]["role"], "org_manager");
        assert_eq!(count(&me["person"]["filters"]), 0);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_organization_shows_its_settings(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let organization = k.get(&k.mara, "/organization").await?.ok(StatusCode::OK)?;
        assert_eq!(text(&organization, "name"), "Kestrel Digital");
        assert_eq!(text(&organization, "reporting_currency"), "USD");
        assert_eq!(organization["approval"], false);
        let changed = k
            .send(
                &k.nadia,
                Method::PATCH,
                "/organization",
                Some(json!({"approval": true})),
            )
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(changed["approval"], true);
        let answer = k
            .send(
                &k.mara,
                Method::PATCH,
                "/organization",
                Some(json!({"name": "Mine"})),
            )
            .await?;
        assert_eq!(answer.status, StatusCode::FORBIDDEN, "{}", answer.text);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn members_list_the_five_people(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let members = k.get(&k.nadia, "/members").await?.ok(StatusCode::OK)?;
        assert_eq!(count(&members), 5);
        let ola = members
            .as_array()
            .ok_or("array")?
            .iter()
            .find(|m| text(m, "email") == emails::OLA)
            .ok_or("ola")?;
        assert_eq!(text(ola, "status"), "invited");
        assert!(ola["joined_at"].is_null());
        let mara = members
            .as_array()
            .ok_or("array")?
            .iter()
            .find(|m| text(m, "email") == emails::MARA)
            .ok_or("mara")?;
        assert_eq!(mara["attributes"]["team"], "web");
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_new_member_gets_view_on_themselves(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let invited = k.post(&k.nadia, "/members", json!({
            "email": "e.novak@kestrel.example", "name": "Eva Novak", "role": "agent_manager", "attributes": {"team": "web"}
        })).await?.ok(StatusCode::CREATED)?;
        assert!(text(&invited, "invite_url").contains("/i/"));
        assert_eq!(invited["member"]["status"], "invited");
        let grants = k
            .get(&k.nadia, "/grants?person=e.novak@kestrel.example")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&grants), 1);
        assert_eq!(grants[0]["capability"], "view");
        assert_eq!(grants[0]["filter"]["person"], "e.novak@kestrel.example");
        let report = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&by=team"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(text(&report, "total"), "950.00");
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_people_managers_filters_become_grants(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let invited = k.post(&k.nadia, "/members", json!({
            "email": "a.kim@kestrel.example", "name": "Ana Kim", "role": "people_manager", "filters": [{"client": "acme"}]
        })).await?.ok(StatusCode::CREATED)?;
        assert_eq!(invited["member"]["filters"][0]["client"], "acme");
        let grants = k
            .get(&k.nadia, "/grants?person=a.kim@kestrel.example")
            .await?
            .ok(StatusCode::OK)?;
        let mut capabilities: Vec<&str> = grants
            .as_array()
            .ok_or("array")?
            .iter()
            .map(|g| text(g, "capability"))
            .collect();
        capabilities.sort_unstable();
        assert_eq!(capabilities, ["approve", "assign", "grant", "plan", "view"]);
        assert!(
            grants
                .as_array()
                .ok_or("array")?
                .iter()
                .all(|g| g["filter"]["client"] == "acme")
        );
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_role_change_regrants(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let changed = k
            .send(
                &k.nadia,
                Method::PATCH,
                &format!("/members/{}", ids::TOM),
                Some(json!({"role": "people_manager", "filters": [{"team": "web"}]})),
            )
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(changed["role"], "people_manager");
        let report = k
            .get(&k.tom, &format!("/reports/cost?{MARCH}"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(text(&report, "total"), "950.00");
        let back = k
            .send(
                &k.nadia,
                Method::PATCH,
                &format!("/members/{}", ids::TOM),
                Some(json!({"role": "agent_manager"})),
            )
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(back["role"], "agent_manager");
        let report = k
            .get(&k.tom, &format!("/reports/cost?{MARCH}"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(text(&report, "total"), "250.00");
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_removed_member_keeps_their_messages_in_reports(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let gone = k
            .send(
                &k.nadia,
                Method::DELETE,
                &format!("/members/{}", ids::TOM),
                None,
            )
            .await?;
        assert_eq!(gone.status, StatusCode::NO_CONTENT, "{}", gone.text);
        let answer = k.get(&k.tom, "/me").await?;
        assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{}", answer.text);
        let report = k
            .get(&k.nadia, &format!("/reports/cost?{MARCH}&by=person"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(
            text(row(&report, Some(emails::TOM)).ok_or("tom")?, "amount"),
            "250.00"
        );
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn only_org_managers_manage_members(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let answer = k
            .post(&k.priya, "/members", json!({"email": "x@kestrel.example"}))
            .await?;
        assert_eq!(answer.status, StatusCode::FORBIDDEN, "{}", answer.text);
        let answer = k
            .send(
                &k.mara,
                Method::DELETE,
                &format!("/members/{}", ids::TOM),
                None,
            )
            .await?;
        assert_eq!(answer.status, StatusCode::FORBIDDEN, "{}", answer.text);
        let members = k.get(&k.mara, "/members").await?.ok(StatusCode::OK)?;
        assert_eq!(count(&members), 5);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_invite_link_of_the_docs_belongs_to_ola(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let invite = k
            .get("", "/invites/kestrel-4f2a")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(text(&invite, "organization_name"), "Kestrel Digital");
        assert_eq!(text(&invite, "email"), emails::OLA);
        let answer = k.get("", "/invites/nobody-0000").await?;
        assert_eq!(answer.status, StatusCode::NOT_FOUND);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_read_token_reads_until_it_is_revoked(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let token = k
            .post(
                &k.nadia,
                "/api-tokens",
                json!({"name": "reports", "kind": "read"}),
            )
            .await?
            .ok(StatusCode::CREATED)?;
        let secret = text(&token, "token").to_owned();
        assert!(secret.starts_with("tc_read_"));
        let request = Request::get("/api/v1/reports/cost?from=2026-03-01&to=2026-03-31")
            .header(header::AUTHORIZATION, format!("Bearer {secret}"))
            .body(Body::empty())?;
        let response = k.app.clone().oneshot(request).await?;
        assert_eq!(response.status(), StatusCode::OK);
        let tokens = k.get(&k.nadia, "/api-tokens").await?.ok(StatusCode::OK)?;
        assert_eq!(count(&tokens), 1);
        let id = text(&tokens[0], "id").to_owned();
        let gone = k
            .send(&k.nadia, Method::DELETE, &format!("/api-tokens/{id}"), None)
            .await?;
        assert_eq!(gone.status, StatusCode::NO_CONTENT, "{}", gone.text);
        let request = Request::get("/api/v1/me")
            .header(header::AUTHORIZATION, format!("Bearer {secret}"))
            .body(Body::empty())?;
        let response = k.app.clone().oneshot(request).await?;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_status_page_lists_the_jobs(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let status = k.get("", "/status").await?.ok(StatusCode::OK)?;
        let ids: Vec<&str> = status["components"]
            .as_array()
            .ok_or("array")?
            .iter()
            .map(|c| text(c, "id"))
            .collect();
        assert_eq!(
            ids,
            [
                "service",
                "bill-pulls",
                "ticket-sync",
                "submit-reminders",
                "price-table",
                "exchange-rates",
                "sign-in-email"
            ]
        );
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_deviation_row_costs_its_own_lines(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let report = k
            .get(&k.nadia, &format!("/reports/deviation?{MARCH}&by=project"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(
            text(row(&report, Some("shop")).ok_or("shop")?, "cost"),
            "800.00"
        );
        let none = row(&report, None).ok_or("none")?;
        assert_eq!(text(none, "cost"), "150.00");
        assert!(none["plan"].is_null());
        assert!(none["deviation"].is_null());
        let report = k
            .get(&k.nadia, &format!("/reports/deviation?{MARCH}&by=feature"))
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(text(row(&report, None).ok_or("none")?, "cost"), "250.00");
        // Without a grouping the one row is the whole filter, plan included.
        let report = k
            .get(
                &k.nadia,
                &format!("/reports/deviation?{MARCH}&filter=project:shop"),
            )
            .await?
            .ok(StatusCode::OK)?;
        let only = row(&report, None).ok_or("row")?;
        assert_eq!(text(only, "cost"), "800.00");
        assert_eq!(text(only, "plan"), "232.50");
        Ok(())
    }
    .await;
    result.unwrap();
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn estimates_list_newest_first_inside_the_view(pool: PgPool) {
    let k = Kestrel::load(&pool).await.unwrap();
    let result: TestResult<()> = async {
        let estimates = k.get(&k.nadia, "/estimates").await?.ok(StatusCode::OK)?;
        assert_eq!(count(&estimates), 2);
        assert_eq!(estimates[0]["id"], "est_seed_kd_1");
        assert_eq!(estimates[0]["accepted"], true);
        assert_eq!(estimates[0]["cost"], "580.00");
        assert_eq!(estimates[1]["id"], "est_seed_shop");
        // A new estimate is first and not yet accepted.
        let added = k
            .post(
                &k.nadia,
                "/estimates",
                json!({"work": "Returns portal", "tags": {"feature": "KD-2"}, "median": "200", "from": "2026-03-01", "to": "2026-03-31"}),
            )
            .await?
            .ok(StatusCode::CREATED)?;
        let estimates = k.get(&k.nadia, "/estimates").await?.ok(StatusCode::OK)?;
        assert_eq!(count(&estimates), 3);
        assert_eq!(estimates[0]["id"], added["id"]);
        assert_eq!(estimates[0]["accepted"], false);
        // Mara views only herself: nothing planned is inside that.
        let mine = k.get(&k.mara, "/estimates").await?.ok(StatusCode::OK)?;
        assert_eq!(count(&mine), 0);
        let then = k
            .get(&k.nadia, "/estimates?as_of=2026-02-20T12:00:00Z")
            .await?
            .ok(StatusCode::OK)?;
        assert_eq!(count(&then), 1);
        Ok(())
    }
    .await;
    result.unwrap();
}
