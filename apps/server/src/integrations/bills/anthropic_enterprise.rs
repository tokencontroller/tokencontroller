//! Bills from the Claude Enterprise Analytics API, for organizations on
//! Anthropic's Enterprise plan billed for usage at API rates: the cost
//! report at list price, with the discount as its own figure.

use serde::Deserialize;
use time::{Date, OffsetDateTime, UtcOffset};
use tokencontroller_core::{Currency, Money};

use super::bill::{BillLine, BillOrigin, BillProvider, PulledBill};
use super::pull::{
    MAX_PAGES, PullError, get_json, money_from_cents, period_end, period_start, rfc3339,
};
use tokencontroller_core::{CostSource, Period};

/// Where the Enterprise Analytics API lives.
pub const BASE_URL: &str = "https://api.anthropic.com";

/// The bill's source API, as [`PulledBill::source_api`] gives it.
pub const SOURCE_API: &str = "anthropic_enterprise";

/// Days after a period ends until its figures count as final.
pub const FINAL_AFTER_DAYS: i64 = 30;

const API_VERSION: &str = "2023-06-01";

/// Rows Anthropic reports per day at most; any more are dropped without a
/// remainder row, so a full day means the bill would be incomplete.
const GROUP_CAP: usize = 100;

const REFUSED: &str = "Anthropic did not accept the Enterprise Analytics API key. The \
     organization's primary owner creates it in claude.ai under Organization settings, API, \
     with the read:analytics scope.";

#[derive(Deserialize)]
struct CostReport {
    data: Vec<Bucket>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    data_refreshed_at: Option<OffsetDateTime>,
    #[serde(default)]
    has_more: bool,
    next_page: Option<String>,
    organization_id: String,
}

#[derive(Deserialize)]
struct Bucket {
    #[serde(with = "time::serde::rfc3339")]
    starting_at: OffsetDateTime,
    results: Vec<CostRow>,
}

#[derive(Deserialize)]
struct CostRow {
    /// After discount, before credit, in cents.
    amount: String,
    /// Before discount, in cents.
    list_amount: String,
    currency: String,
    product: Option<String>,
    model: Option<String>,
}

/// What one pass over the pages read.
struct Report {
    organization_id: String,
    refreshed_at: Option<OffsetDateTime>,
    /// At list price.
    lines: Vec<BillLine>,
    /// What each line cost after discount.
    charged: Vec<Money>,
}

/// Pulls one period's bill, one line per day, product and model at list
/// price. Gross is the list price, net what was charged, and the discount
/// the difference. Anthropic may revise the figures for a while, so the bill
/// is final only [`FINAL_AFTER_DAYS`] after the period ends, counted from
/// `today`.
///
/// # Errors
///
/// [`PullError::Unauthorized`] if the key is not an Enterprise Analytics API
/// key of an Enterprise organization, [`PullError::RateLimited`] if
/// Anthropic asks to wait, and the other [`PullError`]s if Anthropic fails
/// or answers with something that is not a cost report.
pub async fn pull(
    client: &reqwest::Client,
    base_url: &str,
    key: &str,
    period: Period,
    today: Date,
) -> Result<PulledBill, PullError> {
    // A cursor expires when Anthropic refreshes its data (410 Gone); the
    // pages are then read again from the first.
    let report = match read(client, base_url, key, period, today).await {
        Err(PullError::Provider { status: 410, .. }) => {
            read(client, base_url, key, period, today).await?
        }
        other => other?,
    };

    let end = period_end(period)?;
    let currency = report
        .lines
        .first()
        .map_or(Currency::USD, |line| line.amount.currency());
    let gross = Money::sum(report.lines.iter().map(|line| line.amount), currency)?;
    let net = Money::sum(report.charged, currency)?;
    let settled = end
        .date()
        .checked_add(time::Duration::days(FINAL_AFTER_DAYS))
        .is_some_and(|day| day <= today);
    let refreshed = report.refreshed_at.is_some_and(|at| at >= end);
    let origin = BillOrigin {
        provider: BillProvider::Anthropic,
        cost_source: CostSource::UsageAtApiRates,
        source_api: SOURCE_API,
        scope: report.organization_id,
    };
    Ok(PulledBill::from_lines(
        origin,
        period,
        report.lines,
        gross.checked_sub(net)?,
        Money::zero(currency),
        settled && refreshed,
    )?)
}

async fn read(
    client: &reqwest::Client,
    base_url: &str,
    key: &str,
    period: Period,
    today: Date,
) -> Result<Report, PullError> {
    let url = format!(
        "{}/v1/organizations/analytics/cost_report",
        base_url.trim_end_matches('/')
    );
    let end = period_end(period)?;
    let mut query = vec![
        ("starting_at", rfc3339(period_start(period)?)?),
        ("bucket_width", "1d".to_owned()),
        ("group_by[]", "product".to_owned()),
        ("group_by[]", "model".to_owned()),
        ("limit", "31".to_owned()),
    ];
    // For the open period, leaving the end out reads up to now.
    if end.date() <= today {
        query.push(("ending_at", rfc3339(end)?));
    }

    let mut lines = Vec::new();
    let mut charged = Vec::new();
    let mut page: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let mut request = client
            .get(&url)
            .header("x-api-key", key)
            .header("anthropic-version", API_VERSION)
            .query(&query);
        if let Some(cursor) = &page {
            request = request.query(&[("page", cursor)]);
        }
        let report: CostReport = get_json(request, REFUSED).await?;
        for bucket in report.data {
            if bucket.results.len() >= GROUP_CAP {
                return Err(PullError::Format(format!(
                    "Anthropic reports {GROUP_CAP} or more products and models on one day, \
                     so its cost report for {period} may be incomplete"
                )));
            }
            if !period.contains(bucket.starting_at) {
                continue;
            }
            for row in bucket.results {
                let (line, amount) = line(&bucket.starting_at, row)?;
                lines.push(line);
                charged.push(amount);
            }
        }
        page = match (report.has_more, report.next_page) {
            (false, _) => {
                return Ok(Report {
                    organization_id: report.organization_id,
                    refreshed_at: report.data_refreshed_at,
                    lines,
                    charged,
                });
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

/// A row as a line at list price, with what it cost after discount.
fn line(day: &OffsetDateTime, row: CostRow) -> Result<(BillLine, Money), PullError> {
    let currency = Currency::new(&row.currency.to_ascii_uppercase())
        .map_err(|error| PullError::Format(error.to_string()))?;
    let line = BillLine {
        date: day.to_offset(UtcOffset::UTC).date(),
        workspace: None,
        api_key_id: None,
        model: row.model,
        description: row
            .product
            .unwrap_or_else(|| "not attributed to a product".to_owned()),
        amount: money_from_cents(&row.list_amount, currency)?,
    };
    Ok((line, money_from_cents(&row.amount, currency)?))
}

// Recorded responses follow the Claude Enterprise Analytics API reference,
// Get Cost Over Time:
// https://platform.claude.com/docs/en/api/beta/organization/analytics
// and https://platform.claude.com/docs/en/manage-claude/analytics-api
#[cfg(test)]
mod tests {
    use std::fmt::Write as _;
    use std::time::Duration;

    use super::*;
    use time::macros::date;
    use wiremock::matchers::{header, method, path, query_param, query_param_is_missing};
    use wiremock::{Mock, MockBuilder, MockServer, ResponseTemplate};

    const KEY: &str = "sk-ant-api01-analytics-test";
    const PAGE_1: &str =
        include_str!("../../../tests/fixtures/bills/anthropic_enterprise_cost_report_page_1.json");
    const PAGE_2: &str =
        include_str!("../../../tests/fixtures/bills/anthropic_enterprise_cost_report_page_2.json");
    const UNAUTHORIZED: &str =
        include_str!("../../../tests/fixtures/bills/anthropic_error_401.json");
    const RATE_LIMITED: &str =
        include_str!("../../../tests/fixtures/bills/anthropic_error_429.json");
    const CUT_OFF: &str =
        include_str!("../../../tests/fixtures/bills/anthropic_cost_report_cut_off.json");
    const CURSOR: &str = "page_eyJvZmZzZXQiOjJ9";

    fn json(body: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_raw(body.to_owned(), "application/json")
    }

    fn report() -> MockBuilder {
        Mock::given(method("GET"))
            .and(path("/v1/organizations/analytics/cost_report"))
            .and(header("x-api-key", KEY))
            .and(header("anthropic-version", "2023-06-01"))
    }

    async fn serve_two_pages(server: &MockServer) {
        report()
            .and(query_param("starting_at", "2026-09-01T00:00:00Z"))
            .and(query_param("ending_at", "2026-10-01T00:00:00Z"))
            .and(query_param("bucket_width", "1d"))
            .and(query_param("group_by[]", "product"))
            .and(query_param("group_by[]", "model"))
            .and(query_param("limit", "31"))
            .and(query_param_is_missing("page"))
            .respond_with(json(PAGE_1))
            .mount(server)
            .await;
        report()
            .and(query_param("page", CURSOR))
            .respond_with(json(PAGE_2))
            .mount(server)
            .await;
    }

    fn september() -> Period {
        Period::parse("2026-09").unwrap()
    }

    #[tokio::test]
    async fn list_price_is_gross_and_the_discount_its_own_figure() {
        let server = MockServer::start().await;
        serve_two_pages(&server).await;
        let client = crate::integrations::http::client().unwrap();
        let bill = pull(
            &client,
            &server.uri(),
            KEY,
            september(),
            date!(2026 - 10 - 12),
        )
        .await
        .unwrap();

        assert_eq!(bill.provider(), BillProvider::Anthropic);
        assert_eq!(bill.cost_source(), CostSource::UsageAtApiRates);
        assert_eq!(
            bill.source_id(),
            "anthropic_enterprise:org_013FP9SaFPBg7Kw7fetjn6cF:2026-09"
        );
        // List 54150.5 cents, charged 43320.4 cents.
        assert_eq!(bill.gross().nanos(), 541_505_000_000);
        assert_eq!(bill.net().nanos(), 433_204_000_000);
        assert_eq!(bill.discount().nanos(), 108_301_000_000);
        assert!(bill.credit().is_zero());
        assert_eq!(bill.currency(), Currency::USD);
        // Only 11 days after the period ended.
        assert!(!bill.is_final());

        let lines = bill.lines();
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0].date, date!(2026 - 09 - 01));
        assert_eq!(lines[0].description, "claude_code");
        assert_eq!(lines[0].model.as_deref(), Some("claude-opus-4-6"));
        assert_eq!(lines[0].amount.nanos(), 516_000_000_000);
        assert_eq!(lines[1].description, "chat");
        assert_eq!(lines[2].date, date!(2026 - 09 - 02));
        assert_eq!(lines[3].description, "not attributed to a product");
        assert_eq!(lines[3].model, None);
    }

    #[tokio::test]
    async fn figures_are_final_thirty_days_after_the_period() {
        let server = MockServer::start().await;
        serve_two_pages(&server).await;
        let client = crate::integrations::http::client().unwrap();
        let day_before = pull(
            &client,
            &server.uri(),
            KEY,
            september(),
            date!(2026 - 10 - 30),
        )
        .await
        .unwrap();
        assert!(!day_before.is_final());
        let on_the_day = pull(
            &client,
            &server.uri(),
            KEY,
            september(),
            date!(2026 - 10 - 31),
        )
        .await
        .unwrap();
        assert!(on_the_day.is_final());
    }

    #[tokio::test]
    async fn figures_from_before_the_period_ended_are_not_final() {
        let server = MockServer::start().await;
        let stale = PAGE_2.replace("2026-10-12T04:00:00Z", "2026-09-30T20:00:00Z");
        report().respond_with(json(&stale)).mount(&server).await;
        let client = crate::integrations::http::client().unwrap();
        let bill = pull(
            &client,
            &server.uri(),
            KEY,
            september(),
            date!(2026 - 12 - 01),
        )
        .await
        .unwrap();
        assert!(!bill.is_final());
        let unrefreshed = PAGE_2.replace("\"2026-10-12T04:00:00Z\"", "null");
        server.reset().await;
        report()
            .respond_with(json(&unrefreshed))
            .mount(&server)
            .await;
        let bill = pull(
            &client,
            &server.uri(),
            KEY,
            september(),
            date!(2026 - 12 - 01),
        )
        .await
        .unwrap();
        assert!(!bill.is_final());
    }

    #[tokio::test]
    async fn the_open_period_is_read_up_to_now() {
        let server = MockServer::start().await;
        report()
            .and(query_param("starting_at", "2026-10-01T00:00:00Z"))
            .and(query_param_is_missing("ending_at"))
            .respond_with(json(
                r#"{"data": [], "data_refreshed_at": null, "has_more": false,
                    "next_page": null, "organization_id": "org_1"}"#,
            ))
            .expect(1)
            .mount(&server)
            .await;
        let client = crate::integrations::http::client().unwrap();
        let october = Period::parse("2026-10").unwrap();
        let bill = pull(&client, &server.uri(), KEY, october, date!(2026 - 10 - 12))
            .await
            .unwrap();
        assert!(bill.gross().is_zero());
        assert!(!bill.is_final());
    }

    #[tokio::test]
    async fn an_expired_cursor_starts_the_pages_again() {
        let server = MockServer::start().await;
        report()
            .and(query_param_is_missing("page"))
            .respond_with(json(PAGE_1))
            .expect(2)
            .mount(&server)
            .await;
        report()
            .and(query_param("page", CURSOR))
            .respond_with(ResponseTemplate::new(410).set_body_raw(
                r#"{"type":"error","error":{"type":"not_found_error","message":"cursor expired"}}"#,
                "application/json",
            ))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        report()
            .and(query_param("page", CURSOR))
            .respond_with(json(PAGE_2))
            .mount(&server)
            .await;
        let client = crate::integrations::http::client().unwrap();
        let bill = pull(
            &client,
            &server.uri(),
            KEY,
            september(),
            date!(2026 - 10 - 12),
        )
        .await
        .unwrap();
        assert_eq!(bill.lines().len(), 4);
        assert_eq!(bill.gross().nanos(), 541_505_000_000);
    }

    #[tokio::test]
    async fn a_day_at_the_group_cap_is_refused_as_incomplete() {
        let mut rows = String::new();
        for model in 0..100 {
            if model > 0 {
                rows.push(',');
            }
            write!(
                rows,
                r#"{{"amount": "1", "list_amount": "1", "currency": "USD",
                    "product": "chat", "model": "model-{model}"}}"#
            )
            .unwrap();
        }
        let body = format!(
            r#"{{"data": [{{"starting_at": "2026-09-01T00:00:00Z",
                "ending_at": "2026-09-02T00:00:00Z", "results": [{rows}]}}],
                "data_refreshed_at": null, "has_more": false, "next_page": null,
                "organization_id": "org_1"}}"#
        );
        let server = MockServer::start().await;
        report().respond_with(json(&body)).mount(&server).await;
        let client = crate::integrations::http::client().unwrap();
        let error = pull(
            &client,
            &server.uri(),
            KEY,
            september(),
            date!(2026 - 10 - 12),
        )
        .await
        .unwrap_err();
        assert!(matches!(error, PullError::Format(_)), "{error:?}");
    }

    #[tokio::test]
    async fn a_wrong_key_is_unauthorized_in_plain_words() {
        let server = MockServer::start().await;
        Mock::given(path("/v1/organizations/analytics/cost_report"))
            .respond_with(ResponseTemplate::new(401).set_body_raw(UNAUTHORIZED, "application/json"))
            .mount(&server)
            .await;
        let client = crate::integrations::http::client().unwrap();
        let error = pull(
            &client,
            &server.uri(),
            "wrong",
            september(),
            date!(2026 - 10 - 12),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(
                &error,
                PullError::Unauthorized(message)
                    if message.starts_with("Anthropic did not accept the Enterprise Analytics API key.")
            ),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn a_rate_limit_says_how_long_to_wait() {
        let server = MockServer::start().await;
        report()
            .respond_with(
                ResponseTemplate::new(429)
                    .insert_header("retry-after", "45")
                    .set_body_raw(RATE_LIMITED, "application/json"),
            )
            .expect(1)
            .mount(&server)
            .await;
        let client = crate::integrations::http::client().unwrap();
        let error = pull(
            &client,
            &server.uri(),
            KEY,
            september(),
            date!(2026 - 10 - 12),
        )
        .await
        .unwrap_err();
        assert_eq!(
            error,
            PullError::RateLimited {
                retry_after: Some(Duration::from_secs(45))
            }
        );
    }

    #[tokio::test]
    async fn a_malformed_body_is_a_format_error() {
        let server = MockServer::start().await;
        report().respond_with(json(CUT_OFF)).mount(&server).await;
        let client = crate::integrations::http::client().unwrap();
        let error = pull(
            &client,
            &server.uri(),
            KEY,
            september(),
            date!(2026 - 10 - 12),
        )
        .await
        .unwrap_err();
        assert!(matches!(error, PullError::Format(_)), "{error:?}");
    }
}
