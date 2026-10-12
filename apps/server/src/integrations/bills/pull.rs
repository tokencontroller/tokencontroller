//! What the bill pulls share: their errors, reading a provider's answer,
//! amounts in cents and the moments a period starts and ends.

use std::str::FromStr as _;
use std::time::Duration;

use reqwest::StatusCode;
use reqwest::header::HeaderMap;
use rust_decimal::Decimal;
use serde::de::DeserializeOwned;
use time::format_description::well_known::{Rfc2822, Rfc3339};
use time::{Date, OffsetDateTime};
use tokencontroller_core::{Currency, Money, MoneyError};

use super::bill::BillError;
use crate::integrations::decimal::money_from_decimal;
use tokencontroller_core::Period;

/// Pages a pull follows before it gives up on a provider that never stops.
pub(super) const MAX_PAGES: usize = 200;

/// Why a bill pull failed. The clients never retry: the job runner decides
/// when to try again.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PullError {
    /// The key is wrong, revoked or lacks the rights to read costs.
    #[error("{0}")]
    Unauthorized(String),
    /// The provider asks to slow down, for `retry_after` if it says how long.
    #[error("the provider is limiting requests; try again later")]
    RateLimited { retry_after: Option<Duration> },
    /// The provider answered with an error of its own.
    #[error("the provider answered {status}: {message}")]
    Provider { status: u16, message: String },
    /// The provider's answer is not the cost report it should be.
    #[error("{0}")]
    Format(String),
    /// The provider could not be reached or the answer was cut off.
    #[error("could not reach the provider: {0}")]
    Network(String),
}

impl From<BillError> for PullError {
    fn from(error: BillError) -> Self {
        Self::Format(format!(
            "the provider's cost report does not add up: {error}"
        ))
    }
}

impl From<MoneyError> for PullError {
    fn from(error: MoneyError) -> Self {
        Self::from(BillError::Money(error))
    }
}

/// Sends a request and reads the JSON answer. `refused` is the plain message
/// for a key the provider does not accept.
pub(super) async fn get_json<T: DeserializeOwned>(
    request: reqwest::RequestBuilder,
    refused: &str,
) -> Result<T, PullError> {
    get_json_and_headers(request, refused)
        .await
        .map(|(body, _)| body)
}

/// [`get_json`], also giving the answer's headers.
pub(super) async fn get_json_and_headers<T: DeserializeOwned>(
    request: reqwest::RequestBuilder,
    refused: &str,
) -> Result<(T, HeaderMap), PullError> {
    let response = request
        .send()
        .await
        .map_err(|error| PullError::Network(error.to_string()))?;
    let status = response.status();
    let headers = response.headers().clone();
    let retry_after = retry_after(&headers, OffsetDateTime::now_utc());
    let body = response
        .bytes()
        .await
        .map_err(|error| PullError::Network(error.to_string()))?;
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return Err(PullError::Unauthorized(match error_message(&body) {
            Some(said) => format!("{refused} The provider said: {said}"),
            None => refused.to_owned(),
        }));
    }
    if status == StatusCode::TOO_MANY_REQUESTS {
        return Err(PullError::RateLimited { retry_after });
    }
    if !status.is_success() {
        return Err(PullError::Provider {
            status: status.as_u16(),
            message: error_message(&body).unwrap_or_else(|| status.to_string()),
        });
    }
    let body = serde_json::from_slice(&body).map_err(|error| {
        PullError::Format(format!(
            "the provider's answer is not a cost report Token Controller can read: {error}"
        ))
    })?;
    Ok((body, headers))
}

/// The provider's own error message: `error.message` in the JSON errors of
/// Anthropic and OpenAI, otherwise the start of the body.
fn error_message(body: &[u8]) -> Option<String> {
    let json = serde_json::from_slice::<serde_json::Value>(body).ok();
    let said = json.as_ref().and_then(|json| {
        json.pointer("/error/message")
            .or_else(|| json.pointer("/message"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    });
    said.or_else(|| {
        let text: String = String::from_utf8_lossy(body).chars().take(300).collect();
        let text = text.trim();
        (!text.is_empty()).then(|| text.to_owned())
    })
}

/// How long a rate-limited client should wait: `Retry-After` in seconds or
/// as an HTTP date, or OpenAI's `retry-after-ms`.
fn retry_after(headers: &HeaderMap, now: OffsetDateTime) -> Option<Duration> {
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
    };
    if let Some(millis) = header("retry-after-ms").and_then(|value| value.parse::<u64>().ok()) {
        return Some(Duration::from_millis(millis));
    }
    let value = header("retry-after")?;
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let at = OffsetDateTime::parse(value, &Rfc2822).ok()?;
    let seconds = at.unix_timestamp().checked_sub(now.unix_timestamp())?;
    Some(Duration::from_secs(u64::try_from(seconds).unwrap_or(0)))
}

/// Reads an amount given as a decimal string in the currency's minor unit,
/// such as `"123.45"` cents for 1.2345 USD, as Anthropic reports costs.
pub(super) fn money_from_cents(text: &str, currency: Currency) -> Result<Money, PullError> {
    let invalid = || PullError::Format(format!("{text:?} is not an amount in cents"));
    let trimmed = text.trim();
    let minor_units = Decimal::from_str(trimmed)
        .or_else(|_| Decimal::from_scientific(trimmed))
        .map_err(|_| invalid())?;
    let units = minor_units
        .checked_mul(Decimal::new(1, currency.minor_unit_digits()))
        .ok_or_else(invalid)?;
    money_from_decimal(&units.to_string(), currency).map_err(|_| invalid())
}

/// The first moment of a period, midnight UTC on its first day.
pub(super) fn period_start(period: Period) -> Result<OffsetDateTime, PullError> {
    Date::from_calendar_date(period.year(), period.month(), 1)
        .map(|day| day.midnight().assume_utc())
        .map_err(|error| PullError::Format(format!("{period} has no first day: {error}")))
}

/// The first moment after a period.
pub(super) fn period_end(period: Period) -> Result<OffsetDateTime, PullError> {
    let next = period
        .next()
        .ok_or_else(|| PullError::Format(format!("{period} is the last period there is")))?;
    period_start(next)
}

/// A moment as an RFC 3339 query value, such as `2026-09-01T00:00:00Z`.
pub(super) fn rfc3339(moment: OffsetDateTime) -> Result<String, PullError> {
    moment
        .format(&Rfc3339)
        .map_err(|error| PullError::Format(error.to_string()))
}

/// Whether the period is over at `now`.
pub(super) fn has_ended(period: Period, now: OffsetDateTime) -> Result<bool, PullError> {
    Ok(period_end(period)? <= now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderValue;
    use time::macros::datetime;

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(*name, HeaderValue::from_static(value));
        }
        map
    }

    #[test]
    fn retry_after_reads_seconds_dates_and_milliseconds() {
        let now = datetime!(2026-10-12 08:00:00 UTC);
        assert_eq!(
            retry_after(&headers(&[("retry-after", "30")]), now),
            Some(Duration::from_secs(30))
        );
        assert_eq!(
            retry_after(
                &headers(&[("retry-after", "Mon, 12 Oct 2026 08:01:30 GMT")]),
                now
            ),
            Some(Duration::from_secs(90))
        );
        assert_eq!(
            retry_after(
                &headers(&[("retry-after", "Mon, 12 Oct 2026 07:00:00 GMT")]),
                now
            ),
            Some(Duration::ZERO)
        );
        assert_eq!(
            retry_after(
                &headers(&[("retry-after", "1"), ("retry-after-ms", "1500")]),
                now
            ),
            Some(Duration::from_millis(1500))
        );
        assert_eq!(retry_after(&headers(&[("retry-after", "soon")]), now), None);
        assert_eq!(retry_after(&HeaderMap::new(), now), None);
    }

    #[test]
    fn cents_become_money_without_floats() {
        let money = money_from_cents("123.78912", Currency::USD).unwrap();
        assert_eq!(money.nanos(), 1_237_891_200);
        assert_eq!(
            money_from_cents("41280.000000", Currency::USD)
                .unwrap()
                .nanos(),
            412_800_000_000
        );
        assert_eq!(
            money_from_cents("-50", Currency::USD).unwrap().nanos(),
            -500_000_000
        );
        assert_eq!(
            money_from_cents("1.5e2", Currency::USD).unwrap().nanos(),
            1_500_000_000
        );
        let yen = Currency::new("JPY").unwrap();
        assert_eq!(money_from_cents("12", yen).unwrap().nanos(), 12_000_000_000);
        assert!(matches!(
            money_from_cents("twelve", Currency::USD),
            Err(PullError::Format(_))
        ));
        assert!(matches!(
            money_from_cents("1e40", Currency::USD),
            Err(PullError::Format(_))
        ));
    }

    #[test]
    fn periods_start_and_end_at_midnight_utc() {
        let september = Period::parse("2026-09").unwrap();
        assert_eq!(
            period_start(september).unwrap(),
            datetime!(2026-09-01 00:00 UTC)
        );
        assert_eq!(
            period_end(september).unwrap(),
            datetime!(2026-10-01 00:00 UTC)
        );
        let december = Period::parse("2026-12").unwrap();
        assert_eq!(
            period_end(december).unwrap(),
            datetime!(2027-01-01 00:00 UTC)
        );
        assert!(period_end(Period::parse("9999-12").unwrap()).is_err());
        assert!(!has_ended(september, datetime!(2026-09-30 23:59:59 UTC)).unwrap());
        assert!(has_ended(september, datetime!(2026-10-01 00:00 UTC)).unwrap());
    }

    #[test]
    fn error_messages_come_from_the_body() {
        let anthropic = br#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#;
        assert_eq!(
            error_message(anthropic).as_deref(),
            Some("invalid x-api-key")
        );
        assert_eq!(
            error_message(b"  Bad Gateway  ").as_deref(),
            Some("Bad Gateway")
        );
        assert_eq!(error_message(b""), None);
    }
}
