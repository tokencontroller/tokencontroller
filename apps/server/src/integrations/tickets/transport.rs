//! Sending requests to a ticket system and reading its answers: sign-in,
//! error answers turned into [`SyncError`], and `Link` header paging.

use std::time::Duration;

use reqwest::header::{HeaderMap, RETRY_AFTER};
use reqwest::{RequestBuilder, StatusCode, Url};
use serde::de::DeserializeOwned;
use serde_json::Value;
use time::OffsetDateTime;

use super::{Connection, Credentials, SyncError, TicketSystem, text};

/// The longest provider message kept in an error, in characters.
const MAX_MESSAGE_CHARS: usize = 300;

/// A successful answer: its JSON body.
pub(super) struct Reply<T> {
    pub body: T,
}

/// The URL of `segments` below the connection's base URL, each segment
/// percent-encoded.
pub(super) fn endpoint(connection: &Connection, segments: &[&str]) -> Result<Url, SyncError> {
    let invalid = || SyncError::Network(format!("{} is not a web address", connection.base_url));
    let mut url = Url::parse(connection.base_url.trim_end_matches('/')).map_err(|_| invalid())?;
    url.path_segments_mut()
        .map_err(|()| invalid())?
        .pop_if_empty()
        .extend(segments);
    Ok(url)
}

/// Signs the request in, sends it and reads its JSON answer. `missing`
/// says what a 404 means, such as "Jira Cloud project KD was not found".
pub(super) async fn send<T: DeserializeOwned>(
    connection: &Connection,
    request: RequestBuilder,
    missing: &str,
) -> Result<Reply<T>, SyncError> {
    let system = connection.system;
    let unreachable =
        |error: reqwest::Error| SyncError::Network(format!("{}: {error}", system.name()));
    let response = signed_in(connection, request)
        .send()
        .await
        .map_err(unreachable)?;
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.text().await.map_err(unreachable)?;
    // Some systems answer a token they do not know with a sign-in page.
    if status == StatusCode::NON_AUTHORITATIVE_INFORMATION {
        return Err(not_accepted(system));
    }
    if !status.is_success() {
        let now = OffsetDateTime::now_utc();
        return Err(failure(system, status, &headers, &body, missing, now));
    }
    let body = serde_json::from_str(&body)
        .map_err(|error| SyncError::Format(format!("{}: {error}", system.name())))?;
    Ok(Reply { body })
}

fn signed_in(connection: &Connection, request: RequestBuilder) -> RequestBuilder {
    let credentials = &connection.credentials;
    match connection.system {
        TicketSystem::Jira => match credentials {
            Credentials::EmailAndToken { email, token } => request.basic_auth(email, Some(token)),
        },
    }
}

fn not_accepted(system: TicketSystem) -> SyncError {
    let credential = match system {
        TicketSystem::Jira => "account email and API token",
    };
    SyncError::Unauthorized(format!(
        "{} did not accept the {credential}. Check it under Settings, Ticket systems.",
        system.name()
    ))
}

fn failure(
    system: TicketSystem,
    status: StatusCode,
    headers: &HeaderMap,
    body: &str,
    missing: &str,
    now: OffsetDateTime,
) -> SyncError {
    let remaining = header(headers, "x-ratelimit-remaining");
    let limited = status == StatusCode::TOO_MANY_REQUESTS
        || (status == StatusCode::FORBIDDEN
            && (remaining == Some("0") || headers.contains_key(RETRY_AFTER)));
    if limited {
        return SyncError::RateLimited {
            retry_after: retry_after(headers, now),
        };
    }
    match status {
        StatusCode::UNAUTHORIZED => not_accepted(system),
        StatusCode::FORBIDDEN => SyncError::Unauthorized(format!(
            "{} refused access with these credentials: {}",
            system.name(),
            provider_message(body)
        )),
        StatusCode::NOT_FOUND | StatusCode::GONE => SyncError::NotFound(missing.to_owned()),
        _ => SyncError::Provider {
            status: status.as_u16(),
            message: provider_message(body),
        },
    }
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
}

/// From `Retry-After` in seconds, else from `x-ratelimit-reset`
/// (seconds since 1970).
fn retry_after(headers: &HeaderMap, now: OffsetDateTime) -> Option<Duration> {
    if let Some(seconds) = header(headers, RETRY_AFTER.as_str()).and_then(|v| v.parse().ok()) {
        return Some(Duration::from_secs(seconds));
    }
    let reset: i64 = header(headers, "x-ratelimit-reset")?.parse().ok()?;
    let wait = reset.checked_sub(now.unix_timestamp())?;
    Some(Duration::from_secs(u64::try_from(wait).unwrap_or_default()))
}

/// The error text of an answer: `message`,
/// `errorMessages` and `errors` (Jira Cloud), or the body as plain text.
fn provider_message(body: &str) -> String {
    let json: Option<Value> = serde_json::from_str(body).ok();
    let from_json = json.as_ref().and_then(|json| {
        if let Some(message) = json.get("message").and_then(Value::as_str) {
            return Some(message.to_owned());
        }
        let listed = json
            .get("errorMessages")
            .and_then(Value::as_array)
            .into_iter()
            .flatten();
        let by_field = json
            .get("errors")
            .and_then(Value::as_object)
            .into_iter()
            .flat_map(serde_json::Map::values);
        let parts: Vec<&str> = listed.chain(by_field).filter_map(Value::as_str).collect();
        (!parts.is_empty()).then(|| parts.join("; "))
    });
    let message = from_json.unwrap_or_else(|| text::from_html(body));
    if message.trim().is_empty() {
        return "no details given".to_owned();
    }
    message.chars().take(MAX_MESSAGE_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use reqwest::header::HeaderValue;

    use super::*;

    fn connection(base_url: &str) -> Connection {
        Connection {
            system: TicketSystem::Jira,
            base_url: base_url.to_owned(),
            credentials: Credentials::EmailAndToken {
                email: "dev@acme.example".to_owned(),
                token: "t".to_owned(),
            },
        }
    }

    fn headers(pairs: &[(&'static str, &str)]) -> Result<HeaderMap, Box<dyn std::error::Error>> {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(*name, HeaderValue::from_str(value)?);
        }
        Ok(map)
    }

    #[test]
    fn endpoints_encode_each_segment() {
        let spaced = Connection {
            base_url: "https://acme.atlassian.net/".to_owned(),
            ..connection("")
        };
        let url = endpoint(&spaced, &["rest", "api", "3", "Fabrikam Fiber"]).unwrap();
        assert_eq!(
            url.as_str(),
            "https://acme.atlassian.net/rest/api/3/Fabrikam%20Fiber"
        );
        let escaped =
            endpoint(&connection("https://acme.atlassian.net"), &["repos", "a/b"]).unwrap();
        assert_eq!(escaped.as_str(), "https://acme.atlassian.net/repos/a%2Fb");
        assert!(matches!(
            endpoint(&connection("acme.atlassian.net"), &[]),
            Err(SyncError::Network(_))
        ));
    }

    #[test]
    fn rate_limits_say_when_to_retry() {
        let now = OffsetDateTime::from_unix_timestamp(1_760_000_000).unwrap();
        let seconds = headers(&[("retry-after", "30")]).unwrap();
        assert_eq!(retry_after(&seconds, now), Some(Duration::from_secs(30)));
        let reset = headers(&[("x-ratelimit-reset", "1760000090")]).unwrap();
        assert_eq!(retry_after(&reset, now), Some(Duration::from_secs(90)));
        let past = headers(&[("x-ratelimit-reset", "1759999990")]).unwrap();
        assert_eq!(retry_after(&past, now), Some(Duration::ZERO));
        let date = headers(&[("retry-after", "Wed, 21 Oct 2026 07:28:00 GMT")]).unwrap();
        assert_eq!(retry_after(&date, now), None);
        let limited = headers(&[
            ("x-ratelimit-remaining", "0"),
            ("x-ratelimit-reset", "1760000060"),
        ])
        .unwrap();
        assert_eq!(
            failure(
                TicketSystem::Jira,
                StatusCode::FORBIDDEN,
                &limited,
                "{}",
                "",
                now
            ),
            SyncError::RateLimited {
                retry_after: Some(Duration::from_mins(1))
            }
        );
    }

    #[test]
    fn error_answers_keep_the_providers_message() {
        assert_eq!(
            provider_message(r#"{"message":"Bad credentials","status":"401"}"#),
            "Bad credentials"
        );
        assert_eq!(
            provider_message(
                r#"{"errorMessages":["Field 'x' does not exist."],"errors":{"jql":"Bad JQL"}}"#
            ),
            "Field 'x' does not exist.; Bad JQL"
        );
        assert_eq!(
            provider_message("<html><body><h1>Bad gateway</h1></body></html>"),
            "Bad gateway"
        );
        assert_eq!(provider_message(""), "no details given");
        let long = "x".repeat(1_000);
        assert_eq!(provider_message(&long).chars().count(), MAX_MESSAGE_CHARS);
        let server_error = failure(
            TicketSystem::Jira,
            StatusCode::INTERNAL_SERVER_ERROR,
            &HeaderMap::new(),
            r#"{"$id":"1","message":"TF400898: An internal error occurred."}"#,
            "",
            OffsetDateTime::UNIX_EPOCH,
        );
        assert_eq!(
            server_error,
            SyncError::Provider {
                status: 500,
                message: "TF400898: An internal error occurred.".to_owned()
            }
        );
    }
}
