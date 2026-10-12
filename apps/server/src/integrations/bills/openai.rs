//! Bills from OpenAI's Costs API, for organizations that pay OpenAI for API
//! usage: costs per day, project, API key and line item.

use serde::Deserialize;
use time::OffsetDateTime;
use tokencontroller_core::{Currency, Money};

use super::bill::{BillLine, BillOrigin, BillProvider, PulledBill};
use super::pull::{
    MAX_PAGES, PullError, get_json_and_headers, has_ended, period_end, period_start,
};
use crate::integrations::decimal::money_from_json_number;
use tokencontroller_core::{CostSource, Period};

/// Where the OpenAI API lives.
pub const BASE_URL: &str = "https://api.openai.com";

/// The bill's source API, as [`PulledBill::source_api`] gives it.
pub const SOURCE_API: &str = "openai_costs";

/// The scope of a bill when OpenAI does not name the organization.
const UNNAMED_ORGANIZATION: &str = "default";

const REFUSED: &str = "OpenAI did not accept the admin key. Use an Admin key of the \
     organization (it starts with sk-admin-), created by an organization owner under \
     Organization settings, Admin keys.";

#[derive(Deserialize)]
struct CostsPage {
    data: Vec<Bucket>,
    #[serde(default)]
    has_more: bool,
    next_page: Option<String>,
}

#[derive(Deserialize)]
struct Bucket {
    start_time: i64,
    #[serde(alias = "result")]
    results: Vec<CostsResult>,
}

#[derive(Deserialize)]
struct CostsResult {
    amount: Option<Amount>,
    line_item: Option<String>,
    project_id: Option<String>,
    api_key_id: Option<String>,
}

#[derive(Deserialize)]
struct Amount {
    value: Option<serde_json::Number>,
    currency: Option<String>,
}

/// Pulls one period's bill from the Costs API, one line per day, project,
/// API key and line item. The bill is final once the period has ended.
///
/// # Errors
///
/// [`PullError::Unauthorized`] if the key is not an admin key of the
/// organization, [`PullError::RateLimited`] if OpenAI asks to wait, and the
/// other [`PullError`]s if OpenAI fails or answers with something that is
/// not a costs page.
pub async fn pull(
    client: &reqwest::Client,
    base_url: &str,
    admin_key: &str,
    period: Period,
) -> Result<PulledBill, PullError> {
    let url = format!("{}/v1/organization/costs", base_url.trim_end_matches('/'));
    let query = [
        (
            "start_time",
            period_start(period)?.unix_timestamp().to_string(),
        ),
        ("end_time", period_end(period)?.unix_timestamp().to_string()),
        ("bucket_width", "1d".to_owned()),
        ("group_by", "project_id".to_owned()),
        ("group_by", "line_item".to_owned()),
        ("group_by", "api_key_id".to_owned()),
        ("limit", "31".to_owned()),
    ];
    let mut lines = Vec::new();
    let mut organization: Option<String> = None;
    let mut page: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let mut request = client.get(&url).bearer_auth(admin_key).query(&query);
        if let Some(cursor) = &page {
            request = request.query(&[("page", cursor)]);
        }
        let (costs, headers): (CostsPage, _) = get_json_and_headers(request, REFUSED).await?;
        if organization.is_none() {
            organization = headers
                .get("openai-organization")
                .and_then(|value| value.to_str().ok())
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_owned);
        }
        for bucket in costs.data {
            let day = OffsetDateTime::from_unix_timestamp(bucket.start_time)
                .map_err(|error| PullError::Format(error.to_string()))?;
            if !period.contains(day) {
                continue;
            }
            for result in bucket.results {
                if let Some(line) = line(day, result)? {
                    lines.push(line);
                }
            }
        }
        page = match (costs.has_more, costs.next_page) {
            (false, _) => {
                let currency = lines
                    .first()
                    .map_or(Currency::USD, |line: &BillLine| line.amount.currency());
                let origin = BillOrigin {
                    provider: BillProvider::OpenAi,
                    cost_source: CostSource::ApiKey,
                    source_api: SOURCE_API,
                    scope: organization.unwrap_or_else(|| UNNAMED_ORGANIZATION.to_owned()),
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
                    "OpenAI said there are more pages but gave no next page".to_owned(),
                ));
            }
        };
    }
    Err(PullError::Format(format!(
        "OpenAI's costs for {period} have more than {MAX_PAGES} pages"
    )))
}

/// A result as a bill line, or `None` if it carries no amount.
fn line(day: OffsetDateTime, result: CostsResult) -> Result<Option<BillLine>, PullError> {
    let Some(Amount {
        value: Some(value),
        currency,
    }) = result.amount
    else {
        return Ok(None);
    };
    let code = currency.unwrap_or_else(|| "usd".to_owned());
    let currency = Currency::new(&code.to_ascii_uppercase())
        .map_err(|error| PullError::Format(error.to_string()))?;
    let amount = money_from_json_number(&value, currency)
        .map_err(|error| PullError::Format(format!("OpenAI sent an amount that {error}")))?;
    // Line items name the model first, such as "gpt-5-2025-08-07, input".
    let model = result
        .line_item
        .as_deref()
        .and_then(|item| item.split_once(", "))
        .map(|(model, _)| model.to_owned());
    Ok(Some(BillLine {
        date: day.date(),
        workspace: result.project_id,
        api_key_id: result.api_key_id,
        model,
        description: result
            .line_item
            .unwrap_or_else(|| "not attributed to a line item".to_owned()),
        amount,
    }))
}

// Recorded responses follow the OpenAI API reference, Costs:
// https://platform.openai.com/docs/api-reference/usage/costs
#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use time::macros::date;
    use wiremock::matchers::{header, method, path, query_param, query_param_is_missing};
    use wiremock::{Mock, MockBuilder, MockServer, ResponseTemplate};

    const KEY: &str = "sk-admin-test";
    const PAGE_1: &str = include_str!("../../../tests/fixtures/bills/openai_costs_page_1.json");
    const PAGE_2: &str = include_str!("../../../tests/fixtures/bills/openai_costs_page_2.json");
    const UNAUTHORIZED: &str = include_str!("../../../tests/fixtures/bills/openai_error_401.json");
    const RATE_LIMITED: &str = include_str!("../../../tests/fixtures/bills/openai_error_429.json");

    fn json(body: &str) -> ResponseTemplate {
        ResponseTemplate::new(200)
            .insert_header("openai-organization", "org-Wn3xJ8kPq2RtYv5LmB7cZd1A")
            .set_body_raw(body.to_owned(), "application/json")
    }

    fn costs() -> MockBuilder {
        Mock::given(method("GET"))
            .and(path("/v1/organization/costs"))
            .and(header("authorization", "Bearer sk-admin-test"))
    }

    fn september() -> Period {
        Period::parse("2026-09").unwrap()
    }

    #[tokio::test]
    async fn a_period_is_read_across_two_pages() {
        let server = MockServer::start().await;
        let query = || {
            costs()
                .and(query_param("start_time", "1788220800"))
                .and(query_param("end_time", "1790812800"))
                .and(query_param("bucket_width", "1d"))
                .and(query_param("group_by", "project_id"))
                .and(query_param("group_by", "line_item"))
                .and(query_param("group_by", "api_key_id"))
                .and(query_param("limit", "31"))
        };
        query()
            .and(query_param_is_missing("page"))
            .respond_with(json(PAGE_1))
            .expect(1)
            .mount(&server)
            .await;
        query()
            .and(query_param("page", "page_AAAAAGdGxdEiJdKOAAAAAGcqsYA="))
            .respond_with(json(PAGE_2))
            .expect(1)
            .mount(&server)
            .await;
        let client = crate::integrations::http::client().unwrap();
        let bill = pull(&client, &server.uri(), KEY, september())
            .await
            .unwrap();

        assert_eq!(bill.provider(), BillProvider::OpenAi);
        assert_eq!(bill.cost_source(), CostSource::ApiKey);
        assert_eq!(
            bill.source_id(),
            "openai_costs:org-Wn3xJ8kPq2RtYv5LmB7cZd1A:2026-09"
        );
        assert_eq!(bill.currency(), Currency::USD);
        assert_eq!(bill.gross().nanos(), 58_269_023_000);
        assert_eq!(bill.net(), bill.gross());
        assert!(bill.discount().is_zero());
        assert!(bill.credit().is_zero());
        assert!(bill.is_final());

        let lines = bill.lines();
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0].date, date!(2026 - 09 - 01));
        assert_eq!(
            lines[0].workspace.as_deref(),
            Some("proj_4Hn1r8XbqMvRk2LmTzE7aWcQ")
        );
        assert_eq!(lines[0].api_key_id.as_deref(), Some("key_7Gx2Pq9LmN3vRtYb"));
        assert_eq!(lines[0].model.as_deref(), Some("gpt-5-2025-08-07"));
        assert_eq!(lines[0].description, "gpt-5-2025-08-07, input");
        assert_eq!(lines[0].amount.nanos(), 12_340_000_000);
        assert_eq!(lines[2].date, date!(2026 - 09 - 02));
        assert_eq!(lines[2].model, None);
        assert_eq!(lines[2].api_key_id, None);
        assert_eq!(lines[3].amount.nanos(), 123_000);
    }

    #[tokio::test]
    async fn without_an_organization_header_the_scope_is_default() {
        let server = MockServer::start().await;
        costs()
            .respond_with(ResponseTemplate::new(200).set_body_raw(PAGE_2, "application/json"))
            .mount(&server)
            .await;
        let client = crate::integrations::http::client().unwrap();
        let bill = pull(&client, &server.uri(), KEY, september())
            .await
            .unwrap();
        assert_eq!(bill.source_id(), "openai_costs:default:2026-09");
    }

    #[tokio::test]
    async fn results_without_an_amount_are_left_out() {
        let server = MockServer::start().await;
        let body = r#"{"object": "page", "data": [{"object": "bucket",
            "start_time": 1788220800, "end_time": 1788307200, "result": [
            {"object": "organization.costs.result", "amount": null, "line_item": null},
            {"object": "organization.costs.result", "amount": {"value": 2, "currency": "eur"},
             "line_item": null, "project_id": null}]}],
            "has_more": false, "next_page": null}"#;
        costs().respond_with(json(body)).mount(&server).await;
        let client = crate::integrations::http::client().unwrap();
        let bill = pull(&client, &server.uri(), KEY, september())
            .await
            .unwrap();
        assert_eq!(bill.lines().len(), 1);
        assert_eq!(bill.currency(), Currency::EUR);
        assert_eq!(bill.gross().nanos(), 2_000_000_000);
        assert_eq!(bill.lines()[0].description, "not attributed to a line item");
    }

    #[tokio::test]
    async fn a_wrong_key_is_unauthorized_in_plain_words() {
        let server = MockServer::start().await;
        Mock::given(path("/v1/organization/costs"))
            .respond_with(ResponseTemplate::new(401).set_body_raw(UNAUTHORIZED, "application/json"))
            .mount(&server)
            .await;
        let client = crate::integrations::http::client().unwrap();
        let error = pull(&client, &server.uri(), "sk-proj-wrong", september())
            .await
            .unwrap_err();
        assert!(
            matches!(
                &error,
                PullError::Unauthorized(message)
                    if message.starts_with("OpenAI did not accept the admin key.")
                        && message.contains("Incorrect API key provided")
            ),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn a_key_without_rights_is_unauthorized_too() {
        let server = MockServer::start().await;
        Mock::given(path("/v1/organization/costs"))
            .respond_with(ResponseTemplate::new(403).set_body_raw(
                r#"{"error": {"message": "You have insufficient permissions for this operation.",
                    "type": "invalid_request_error", "param": null, "code": null}}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        let client = crate::integrations::http::client().unwrap();
        let error = pull(&client, &server.uri(), KEY, september())
            .await
            .unwrap_err();
        assert!(matches!(error, PullError::Unauthorized(_)), "{error:?}");
    }

    #[tokio::test]
    async fn a_rate_limit_says_how_long_to_wait() {
        let server = MockServer::start().await;
        costs()
            .respond_with(
                ResponseTemplate::new(429)
                    .insert_header("retry-after", "20")
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
                retry_after: Some(Duration::from_secs(20))
            }
        );
    }

    #[tokio::test]
    async fn a_malformed_body_is_a_format_error() {
        let server = MockServer::start().await;
        let body = PAGE_2.replace("\"value\": 0.000123", "\"value\": \"0.000123\"");
        costs().respond_with(json(&body)).mount(&server).await;
        let client = crate::integrations::http::client().unwrap();
        let error = pull(&client, &server.uri(), KEY, september())
            .await
            .unwrap_err();
        assert!(matches!(error, PullError::Format(_)), "{error:?}");
    }
}
