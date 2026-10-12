//! Sending requests to a ticket system and reading its answers: sign-in,
//! error answers turned into [`SyncError`], and `Link` header paging.

use std::time::Duration;

use reqwest::header::{HeaderMap, LINK, RETRY_AFTER};
use reqwest::{RequestBuilder, StatusCode, Url};
use serde::de::DeserializeOwned;
use serde_json::Value;
use time::OffsetDateTime;

use super::{Connection, Credentials, SyncError, TicketSystem, text};

/// The longest provider message kept in an error, in characters.
const MAX_MESSAGE_CHARS: usize = 300;

/// A successful answer: its JSON body and its headers.
pub(super) struct Reply<T> {
    pub body: T,
    pub headers: HeaderMap,
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
    let response = signed_in(connection, request)?
        .send()
        .await
        .map_err(unreachable)?;
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.text().await.map_err(unreachable)?;
    // Azure DevOps answers a token it does not know with a sign-in page.
    if status == StatusCode::NON_AUTHORITATIVE_INFORMATION {
        return Err(not_accepted(system));
    }
    if !status.is_success() {
        let now = OffsetDateTime::now_utc();
        return Err(failure(system, status, &headers, &body, missing, now));
    }
    let body = serde_json::from_str(&body)
        .map_err(|error| SyncError::Format(format!("{}: {error}", system.name())))?;
    Ok(Reply { body, headers })
}

fn signed_in(
    connection: &Connection,
    request: RequestBuilder,
) -> Result<RequestBuilder, SyncError> {
    let credentials = &connection.credentials;
    Ok(match connection.system {
        TicketSystem::Jira => match credentials {
            Credentials::EmailAndToken { email, token } => request.basic_auth(email, Some(token)),
            Credentials::Token(_) => {
                return Err(SyncError::Unauthorized(
                    "Jira Cloud needs the account email together with the API token.".to_owned(),
                ));
            }
        },
        TicketSystem::AzureDevOps => request.basic_auth("", Some(credentials.token())),
        TicketSystem::GitHubIssues => request
            .bearer_auth(credentials.token())
            .header("X-GitHub-Api-Version", "2022-11-28"),
    })
}

fn not_accepted(system: TicketSystem) -> SyncError {
    let credential = match system {
        TicketSystem::Jira => "account email and API token",
        TicketSystem::AzureDevOps => "personal access token",
        TicketSystem::GitHubIssues => "token",
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

/// From `Retry-After` in seconds, else from `GitHub`'s `x-ratelimit-reset`
/// (seconds since 1970).
fn retry_after(headers: &HeaderMap, now: OffsetDateTime) -> Option<Duration> {
    if let Some(seconds) = header(headers, RETRY_AFTER.as_str()).and_then(|v| v.parse().ok()) {
        return Some(Duration::from_secs(seconds));
    }
    let reset: i64 = header(headers, "x-ratelimit-reset")?.parse().ok()?;
    let wait = reset.checked_sub(now.unix_timestamp())?;
    Some(Duration::from_secs(u64::try_from(wait).unwrap_or_default()))
}

/// The error text of an answer: `message` (`GitHub`, Azure DevOps),
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

/// The `rel="next"` URL of a `Link` header. A link to any other host than
/// the connection's is refused, so credentials never leave it.
pub(super) fn next_link(
    headers: &HeaderMap,
    connection: &Connection,
) -> Result<Option<Url>, SyncError> {
    let next = header(headers, LINK.as_str()).and_then(|value| {
        value.split(',').find_map(|link| {
            let (target, params) = link.split_once(';')?;
            let is_next = params
                .split(';')
                .any(|param| matches!(param.trim(), "rel=\"next\"" | "rel=next"));
            is_next.then(|| target.trim().trim_start_matches('<').trim_end_matches('>'))
        })
    });
    let Some(next) = next else {
        return Ok(None);
    };
    let unexpected = || {
        SyncError::Format(format!(
            "{} sent a next page link that is not on {}",
            connection.system.name(),
            connection.base_url
        ))
    };
    let next = Url::parse(next).map_err(|_| unexpected())?;
    let base = endpoint(connection, &[])?;
    if next.origin() != base.origin() {
        return Err(unexpected());
    }
    Ok(Some(next))
}

#[cfg(test)]
mod tests {
    use reqwest::header::HeaderValue;

    use super::*;

    fn connection(base_url: &str) -> Connection {
        Connection {
            system: TicketSystem::GitHubIssues,
            base_url: base_url.to_owned(),
            credentials: Credentials::Token("t".to_owned()),
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
        let ado = Connection {
            base_url: "https://dev.azure.com/acme/".to_owned(),
            ..connection("")
        };
        let url = endpoint(&ado, &["Fabrikam Fiber", "_apis", "wit", "wiql"]).unwrap();
        assert_eq!(
            url.as_str(),
            "https://dev.azure.com/acme/Fabrikam%20Fiber/_apis/wit/wiql"
        );
        let github = endpoint(
            &connection("https://api.github.com"),
            &["repos", "acme", "a/b"],
        )
        .unwrap();
        assert_eq!(github.as_str(), "https://api.github.com/repos/acme/a%2Fb");
        assert!(matches!(
            endpoint(&connection("api.github.com"), &[]),
            Err(SyncError::Network(_))
        ));
    }

    #[test]
    fn next_links_are_read_and_kept_on_the_same_host() {
        let base = connection("https://api.github.com");
        let link = r#"<https://api.github.com/repositories/1/issues?page=2>; rel="next", <https://api.github.com/repositories/1/issues?page=9>; rel="last""#;
        let next = next_link(&headers(&[("link", link)]).unwrap(), &base).unwrap();
        assert_eq!(
            next.map(String::from).as_deref(),
            Some("https://api.github.com/repositories/1/issues?page=2")
        );
        let last_only = r#"<https://api.github.com/repositories/1/issues?page=1>; rel="prev""#;
        assert_eq!(
            next_link(&headers(&[("link", last_only)]).unwrap(), &base),
            Ok(None)
        );
        assert_eq!(next_link(&HeaderMap::new(), &base), Ok(None));
        let elsewhere = r#"<https://collector.example/steal?page=2>; rel="next""#;
        assert!(matches!(
            next_link(&headers(&[("link", elsewhere)]).unwrap(), &base),
            Err(SyncError::Format(_))
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
                TicketSystem::GitHubIssues,
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
            TicketSystem::AzureDevOps,
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
