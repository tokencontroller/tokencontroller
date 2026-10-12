//! Bills from Anthropic's Usage and Cost Admin API, for organizations that
//! pay per token through the Console or API keys: the cost report per
//! workspace and day.

use serde::Deserialize;
use time::{OffsetDateTime, UtcOffset};
use tokencontroller_core::{Currency, Money};

use super::bill::{BillLine, BillOrigin, BillProvider, PulledBill};
use super::pull::{
    MAX_PAGES, PullError, get_json, has_ended, money_from_cents, period_end, period_start, rfc3339,
};
use tokencontroller_core::{CostSource, Period};

/// Where the Admin API lives.
pub const BASE_URL: &str = "https://api.anthropic.com";

/// The bill's source API, as [`PulledBill::source_api`] gives it.
pub const SOURCE_API: &str = "anthropic_console";

const API_VERSION: &str = "2023-06-01";

const REFUSED: &str = "Anthropic did not accept the admin key. Use an Admin API key \
     (it starts with sk-ant-admin) of the organization, created by an organization admin \
     in the Claude Console.";

#[derive(Deserialize)]
struct Organization {
    id: String,
}

#[derive(Deserialize)]
struct CostReport {
    data: Vec<Bucket>,
    #[serde(default)]
    has_more: bool,
    next_page: Option<String>,
}

#[derive(Deserialize)]
struct Bucket {
    #[serde(with = "time::serde::rfc3339")]
    starting_at: OffsetDateTime,
    results: Vec<CostItem>,
}

#[derive(Deserialize)]
struct CostItem {
    /// In cents, as a decimal string.
    amount: String,
    currency: String,
    description: Option<String>,
    cost_type: Option<String>,
    model: Option<String>,
    workspace_id: Option<String>,
}

/// Pulls one period's bill from the cost report, one line per workspace,
/// day and kind of cost. Priority Tier usage is not in the cost report, so
/// it shows as residual. The bill is final once the period has ended.
///
/// # Errors
///
/// [`PullError::Unauthorized`] if the key is not an admin key of the
/// organization, [`PullError::RateLimited`] if Anthropic asks to wait,
/// and the other [`PullError`]s if Anthropic fails or answers with
/// something that is not a cost report.
pub async fn pull(
    client: &reqwest::Client,
    base_url: &str,
    admin_key: &str,
    period: Period,
) -> Result<PulledBill, PullError> {
    let base_url = base_url.trim_end_matches('/');
    let get = |path: &str| {
        client
            .get(format!("{base_url}{path}"))
            .header("x-api-key", admin_key)
            .header("anthropic-version", API_VERSION)
    };
    let organization: Organization = get_json(get("/v1/organizations/me"), REFUSED).await?;

    let starting_at = rfc3339(period_start(period)?)?;
    let ending_at = rfc3339(period_end(period)?)?;
    let query = [
        ("starting_at", starting_at.as_str()),
        ("ending_at", ending_at.as_str()),
        ("bucket_width", "1d"),
        ("group_by[]", "workspace_id"),
        ("group_by[]", "description"),
        ("limit", "31"),
    ];
    let mut lines = Vec::new();
    let mut page: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let mut request = get("/v1/organizations/cost_report").query(&query);
        if let Some(cursor) = &page {
            request = request.query(&[("page", cursor)]);
        }
        let report: CostReport = get_json(request, REFUSED).await?;
        for bucket in report.data {
            if period.contains(bucket.starting_at) {
                for item in bucket.results {
                    lines.push(line(&bucket.starting_at, item)?);
                }
            }
        }
        page = match (report.has_more, report.next_page) {
            (false, _) => {
                let currency = lines
                    .first()
                    .map_or(Currency::USD, |line: &BillLine| line.amount.currency());
                let origin = BillOrigin {
                    provider: BillProvider::Anthropic,
                    cost_source: CostSource::ApiKey,
                    source_api: SOURCE_API,
                    scope: organization.id,
                };
                let is_final = has_ended(period, OffsetDateTime::now_utc())?;
                let zero = Money::zero(currency);
                return Ok(PulledBill::from_lines(
                    origin, period, lines, zero, zero, is_final,
                )?);
            }
            (true, Some(next)) => Some(next),
            (true, None) => {
                return Err(PullError::Format(
                    "Anthropic said there are more pages but gave no next page".to_owned(),
                ));
            }
        };
    }
    Err(PullError::Format(format!(
        "Anthropic's cost report for {period} has more than {MAX_PAGES} pages"
    )))
}

fn line(day: &OffsetDateTime, item: CostItem) -> Result<BillLine, PullError> {
    let currency = Currency::new(&item.currency.to_ascii_uppercase())
        .map_err(|error| PullError::Format(error.to_string()))?;
    Ok(BillLine {
        date: day.to_offset(UtcOffset::UTC).date(),
        workspace: item.workspace_id,
        api_key_id: None,
        model: item.model,
        description: item
            .description
            .or(item.cost_type)
            .unwrap_or_else(|| "Usage".to_owned()),
        amount: money_from_cents(&item.amount, currency)?,
    })
}

// Recorded responses follow the Admin API reference:
// https://platform.claude.com/docs/en/api/admin-api/usage-cost/get-cost-report
// and https://platform.claude.com/docs/en/api/beta/organization (Get Current
// Organization).
#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use time::macros::date;
    use wiremock::matchers::{header, method, path, query_param, query_param_is_missing};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const KEY: &str = "sk-ant-admin01-test";
    const ORGANIZATION: &str =
        include_str!("../../../tests/fixtures/bills/anthropic_console_organization.json");
    const PAGE_1: &str =
        include_str!("../../../tests/fixtures/bills/anthropic_console_cost_report_page_1.json");
    const PAGE_2: &str =
        include_str!("../../../tests/fixtures/bills/anthropic_console_cost_report_page_2.json");
    const UNAUTHORIZED: &str =
        include_str!("../../../tests/fixtures/bills/anthropic_error_401.json");
    const RATE_LIMITED: &str =
        include_str!("../../../tests/fixtures/bills/anthropic_error_429.json");
    const CUT_OFF: &str =
        include_str!("../../../tests/fixtures/bills/anthropic_cost_report_cut_off.json");

    fn json(body: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_raw(body.to_owned(), "application/json")
    }

    async fn serve_organization(server: &MockServer) {
        Mock::given(method("GET"))
            .and(path("/v1/organizations/me"))
            .and(header("x-api-key", KEY))
            .and(header("anthropic-version", "2023-06-01"))
            .respond_with(json(ORGANIZATION))
            .mount(server)
            .await;
    }

    fn september() -> Period {
        Period::parse("2026-09").unwrap()
    }

    #[tokio::test]
    async fn a_period_is_read_across_two_pages() {
        let server = MockServer::start().await;
        serve_organization(&server).await;
        let report = || {
            Mock::given(method("GET"))
                .and(path("/v1/organizations/cost_report"))
                .and(header("x-api-key", KEY))
                .and(header("anthropic-version", "2023-06-01"))
                .and(query_param("starting_at", "2026-09-01T00:00:00Z"))
                .and(query_param("ending_at", "2026-10-01T00:00:00Z"))
                .and(query_param("bucket_width", "1d"))
                .and(query_param("group_by[]", "workspace_id"))
                .and(query_param("group_by[]", "description"))
                .and(query_param("limit", "31"))
        };
        report()
            .and(query_param_is_missing("page"))
            .respond_with(json(PAGE_1))
            .expect(1)
            .mount(&server)
            .await;
        report()
            .and(query_param("page", "page_MjAyNi0wOS0wM1QwMDowMDowMFo="))
            .respond_with(json(PAGE_2))
            .expect(1)
            .mount(&server)
            .await;

        let client = crate::integrations::http::client().unwrap();
        let bill = pull(&client, &server.uri(), KEY, september())
            .await
            .unwrap();

        assert_eq!(bill.provider(), BillProvider::Anthropic);
        assert_eq!(bill.cost_source(), CostSource::ApiKey);
        assert_eq!(bill.source_api(), "anthropic_console");
        assert_eq!(
            bill.source_id(),
            "anthropic_console:12345678-1234-5678-1234-567812345678:2026-09"
        );
        assert_eq!(bill.currency(), Currency::USD);
        // 4830.873456 cents in all.
        assert_eq!(bill.gross().nanos(), 48_308_734_560);
        assert!(bill.discount().is_zero());
        assert!(bill.credit().is_zero());
        assert_eq!(bill.net(), bill.gross());
        assert!(bill.is_final());

        let lines = bill.lines();
        assert_eq!(lines.len(), 5);
        let first = &lines[0];
        assert_eq!(first.date, date!(2026 - 09 - 01));
        assert_eq!(
            first.workspace.as_deref(),
            Some("wrkspc_01JwQvzr7rXLA5AGx3HKfFUJ")
        );
        assert_eq!(first.model.as_deref(), Some("claude-sonnet-4-5"));
        assert_eq!(first.description, "Claude Sonnet 4.5 Usage - Input Tokens");
        assert_eq!(first.amount.nanos(), 12_505_000_000);
        assert_eq!(first.api_key_id, None);
        let web_search = &lines[2];
        assert_eq!(web_search.workspace, None);
        assert_eq!(web_search.model, None);
        assert_eq!(web_search.description, "Web Search Usage");
        assert_eq!(web_search.amount.nanos(), 1_000_000_000);
        assert_eq!(lines[3].date, date!(2026 - 09 - 02));
        assert_eq!(lines[4].date, date!(2026 - 09 - 03));
        assert_eq!(lines[4].description, "Code Execution Usage");
    }

    #[tokio::test]
    async fn an_open_period_is_not_final() {
        let server = MockServer::start().await;
        serve_organization(&server).await;
        Mock::given(path("/v1/organizations/cost_report"))
            .respond_with(json(
                r#"{"data": [], "has_more": false, "next_page": null}"#,
            ))
            .mount(&server)
            .await;
        let client = crate::integrations::http::client().unwrap();
        let later = Period::parse("2999-01").unwrap();
        let bill = pull(&client, &server.uri(), KEY, later).await.unwrap();
        assert!(!bill.is_final());
        assert!(bill.gross().is_zero());
        assert_eq!(bill.currency(), Currency::USD);
        assert!(bill.lines().is_empty());
    }

    #[tokio::test]
    async fn a_wrong_key_is_unauthorized_in_plain_words() {
        let server = MockServer::start().await;
        Mock::given(path("/v1/organizations/me"))
            .respond_with(ResponseTemplate::new(401).set_body_raw(UNAUTHORIZED, "application/json"))
            .mount(&server)
            .await;
        let client = crate::integrations::http::client().unwrap();
        let error = pull(&client, &server.uri(), "sk-ant-api03-wrong", september())
            .await
            .unwrap_err();
        assert!(
            matches!(
                &error,
                PullError::Unauthorized(message)
                    if message.starts_with("Anthropic did not accept the admin key.")
                        && message.ends_with("The provider said: invalid x-api-key")
            ),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn a_rate_limit_says_how_long_to_wait() {
        let server = MockServer::start().await;
        serve_organization(&server).await;
        Mock::given(path("/v1/organizations/cost_report"))
            .respond_with(
                ResponseTemplate::new(429)
                    .insert_header("retry-after", "42")
                    .set_body_raw(RATE_LIMITED, "application/json"),
            )
            .expect(1)
            .mount(&server)
            .await;
        let client = crate::integrations::http::client().unwrap();
        let error = pull(&client, &server.uri(), KEY, september())
            .await
            .unwrap_err();
        assert_eq!(
            error,
            PullError::RateLimited {
                retry_after: Some(Duration::from_secs(42))
            }
        );
    }

    #[tokio::test]
    async fn a_malformed_body_is_a_format_error() {
        let server = MockServer::start().await;
        serve_organization(&server).await;
        Mock::given(path("/v1/organizations/cost_report"))
            .respond_with(json(CUT_OFF))
            .mount(&server)
            .await;
        let client = crate::integrations::http::client().unwrap();
        let error = pull(&client, &server.uri(), KEY, september())
            .await
            .unwrap_err();
        assert!(matches!(error, PullError::Format(_)), "{error:?}");
    }

    #[tokio::test]
    async fn an_amount_that_is_no_number_is_a_format_error() {
        let server = MockServer::start().await;
        serve_organization(&server).await;
        let body = PAGE_2.replace("\"amount\": \"5\"", "\"amount\": \"five\"");
        Mock::given(path("/v1/organizations/cost_report"))
            .respond_with(json(&body))
            .mount(&server)
            .await;
        let client = crate::integrations::http::client().unwrap();
        let error = pull(&client, &server.uri(), KEY, september())
            .await
            .unwrap_err();
        assert_eq!(
            error,
            PullError::Format("\"five\" is not an amount in cents".to_owned())
        );
    }

    #[tokio::test]
    async fn more_pages_without_a_cursor_is_a_format_error() {
        let server = MockServer::start().await;
        serve_organization(&server).await;
        Mock::given(path("/v1/organizations/cost_report"))
            .respond_with(json(r#"{"data": [], "has_more": true, "next_page": null}"#))
            .mount(&server)
            .await;
        let client = crate::integrations::http::client().unwrap();
        let error = pull(&client, &server.uri(), KEY, september())
            .await
            .unwrap_err();
        assert!(matches!(error, PullError::Format(_)), "{error:?}");
    }

    #[tokio::test]
    async fn a_server_error_is_passed_on() {
        let server = MockServer::start().await;
        serve_organization(&server).await;
        Mock::given(path("/v1/organizations/cost_report"))
            .respond_with(ResponseTemplate::new(529).set_body_raw(
                r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        let client = crate::integrations::http::client().unwrap();
        let error = pull(&client, &server.uri(), KEY, september())
            .await
            .unwrap_err();
        assert_eq!(
            error,
            PullError::Provider {
                status: 529,
                message: "Overloaded".to_owned()
            }
        );
    }

    #[tokio::test]
    async fn an_unreachable_provider_is_a_network_error() {
        let client = crate::integrations::http::client().unwrap();
        let error = pull(&client, "http://127.0.0.1:9", KEY, september())
            .await
            .unwrap_err();
        assert!(matches!(error, PullError::Network(_)), "{error:?}");
    }
}
