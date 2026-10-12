//! What the public website's pages need from the service: answers to
//! cross-origin requests from `TC_SITE_URL`, and limits per visitor so the
//! open endpoints cannot be used to run up costs or send mail in bulk.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::Json;
use axum::http::header::{
    ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN,
    ACCESS_CONTROL_MAX_AGE, ORIGIN, VARY,
};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};

use super::settings::Settings;
use crate::error::{ApiError, ErrorBody};

/// Why a request from the website was refused.
#[derive(Debug)]
pub enum Refusal {
    Api(ApiError),
    /// Too many requests from this visitor, or from everyone today.
    Busy(&'static str),
    /// An outside service is down.
    Unavailable(&'static str),
}

impl From<ApiError> for Refusal {
    fn from(error: ApiError) -> Self {
        Self::Api(error)
    }
}

impl From<sqlx::Error> for Refusal {
    fn from(error: sqlx::Error) -> Self {
        Self::Api(error.into())
    }
}

impl IntoResponse for Refusal {
    fn into_response(self) -> Response {
        let (status, error, message) = match self {
            Self::Api(error) => return error.into_response(),
            Self::Busy(message) => (StatusCode::TOO_MANY_REQUESTS, "too_many_requests", message),
            Self::Unavailable(message) => (StatusCode::SERVICE_UNAVAILABLE, "unavailable", message),
        };
        (
            status,
            Json(ErrorBody {
                error,
                message: message.to_owned(),
            }),
        )
            .into_response()
    }
}

/// Adds the cross-origin headers when the request comes from the website.
#[must_use]
pub fn cors(settings: &Settings, request: &HeaderMap, mut response: Response) -> Response {
    let headers = response.headers_mut();
    headers.insert(VARY, HeaderValue::from_static("Origin"));
    let origin = request.get(ORIGIN).and_then(|origin| origin.to_str().ok());
    if let (Some(origin), Some(site)) = (origin, settings.site_url.as_deref())
        && origin == site
        && let Ok(value) = HeaderValue::from_str(origin)
    {
        headers.insert(ACCESS_CONTROL_ALLOW_ORIGIN, value);
        headers.insert(
            ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("POST, OPTIONS"),
        );
        headers.insert(
            ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_static("content-type"),
        );
        headers.insert(ACCESS_CONTROL_MAX_AGE, HeaderValue::from_static("86400"));
    }
    response
}

/// The answer to a browser's preflight request.
#[must_use]
pub fn preflight(settings: &Settings, request: &HeaderMap) -> Response {
    cors(settings, request, StatusCode::NO_CONTENT.into_response())
}

/// Who is asking, as far as the service can tell: the address the load
/// balancer saw, which it appends last to `X-Forwarded-For`. Kept in memory
/// only, never stored.
#[must_use]
pub fn visitor(request: &HeaderMap) -> String {
    request
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.rsplit(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown")
        .to_owned()
}

/// At most `max` uses per visitor within `window`, counted in memory.
pub struct Limit {
    max: usize,
    window: Duration,
    uses: Mutex<HashMap<String, VecDeque<Instant>>>,
}

/// Memory guard: forget everyone when this many visitors are tracked.
const MAX_VISITORS: usize = 10_000;

impl Limit {
    #[must_use]
    pub fn new(max: usize, window: Duration) -> Self {
        Self {
            max,
            window,
            uses: Mutex::new(HashMap::new()),
        }
    }

    /// Counts one use and says whether it is allowed.
    #[must_use]
    pub fn allow(&self, visitor: &str) -> bool {
        let now = Instant::now();
        let Ok(mut uses) = self.uses.lock() else {
            return true;
        };
        if uses.len() >= MAX_VISITORS && !uses.contains_key(visitor) {
            uses.clear();
        }
        let times = uses.entry(visitor.to_owned()).or_default();
        while times
            .front()
            .is_some_and(|first| now.duration_since(*first) >= self.window)
        {
            times.pop_front();
        }
        if times.len() >= self.max {
            return false;
        }
        times.push_back(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site() -> Settings {
        Settings {
            site_url: Some("https://example.com".to_owned()),
            ..Settings::default()
        }
    }

    fn origin(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(ORIGIN, HeaderValue::from_str(value).unwrap());
        headers
    }

    #[test]
    fn only_the_website_may_call_across_origins() {
        let allowed = preflight(&site(), &origin("https://example.com"));
        assert_eq!(
            allowed.headers()[ACCESS_CONTROL_ALLOW_ORIGIN],
            "https://example.com"
        );
        let other = preflight(&site(), &origin("https://evil.example"));
        assert!(other.headers().get(ACCESS_CONTROL_ALLOW_ORIGIN).is_none());
        let off = preflight(&Settings::default(), &origin("https://example.com"));
        assert!(off.headers().get(ACCESS_CONTROL_ALLOW_ORIGIN).is_none());
    }

    #[test]
    fn the_visitor_is_the_last_forwarded_address() {
        let mut headers = HeaderMap::new();
        assert_eq!(visitor(&headers), "unknown");
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("10.0.0.1, 203.0.113.9"),
        );
        assert_eq!(visitor(&headers), "203.0.113.9");
    }

    #[test]
    fn a_limit_counts_per_visitor_and_window() {
        let limit = Limit::new(2, Duration::from_hours(1));
        assert!(limit.allow("a"));
        assert!(limit.allow("a"));
        assert!(!limit.allow("a"));
        assert!(limit.allow("b"));

        let short = Limit::new(1, Duration::ZERO);
        assert!(short.allow("a"));
        assert!(short.allow("a"));
    }
}
