//! The web app (`apps/web`), which the image builds into [`WEB_DIR`]. Every
//! path outside `/api/` serves it. Files under `/assets/` have hashed names
//! and are cached for a year. Any other path that is not a file gets
//! `index.html`, because the app routes in the browser (`/review`,
//! `/close/2026-09`, `/i/<code>`…).
//!
//! Owner: the service platform work.

use std::path::Path;

use axum::Router;
use axum::body::Body;
use axum::extract::Request;
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use tower::ServiceExt as _;
use tower_http::services::{ServeDir, ServeFile};

use crate::error::ApiError;

/// Where the image puts the built web app.
pub const WEB_DIR: &str = "/usr/share/tokencontroller/web";

/// Hashed file names never change their content.
const IMMUTABLE: &str = "public, max-age=31536000, immutable";

/// Everything else is checked on every load, so a new version shows at once.
const NO_CACHE: &str = "no-cache";

/// The routes that serve the web app from `dir`, or `None` when `dir` has no
/// `index.html`, as with a local `cargo run`.
#[must_use]
pub fn routes<S: Clone + Send + Sync + 'static>(dir: &Path) -> Option<Router<S>> {
    let index = dir.join("index.html");
    if !index.is_file() {
        return None;
    }
    let files = ServeDir::new(dir);
    let pages = files.clone().fallback(ServeFile::new(index));
    Some(
        Router::new()
            .route(
                "/assets/{*file}",
                get(move |request: Request| asset(files.clone(), request)),
            )
            .fallback(move |request: Request| page(pages.clone(), request)),
    )
}

/// A file under `/assets/`; a missing one is a 404, never `index.html`.
async fn asset(files: ServeDir, request: Request) -> Response {
    let Ok(response) = files.oneshot(request).await;
    let mut response = response.map(Body::new);
    let cache = if response.status().is_success() {
        IMMUTABLE
    } else {
        NO_CACHE
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    response
}

/// A file, or `index.html` for a path the app routes. Unknown API paths
/// stay errors.
async fn page(pages: ServeDir<ServeFile>, request: Request) -> Response {
    if request.uri().path().starts_with("/api/") {
        return ApiError::NotFound.into_response();
    }
    let Ok(response) = pages.oneshot(request).await;
    let mut response = response.map(Body::new);
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(NO_CACHE));
    response
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use axum::http::StatusCode;

    use super::*;

    /// A built web app in a fresh folder.
    fn built_app() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tc-web-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("index.html"), "<!doctype html><title>app</title>").unwrap();
        std::fs::write(dir.join("favicon.svg"), "<svg/>").unwrap();
        std::fs::write(dir.join("assets/app-3f9a.js"), "console.log(1)").unwrap();
        dir
    }

    async fn get(router: &Router, path: &str) -> (StatusCode, String, String) {
        let response = router
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let cache = response
            .headers()
            .get(header::CACHE_CONTROL)
            .map(|value| value.to_str().unwrap().to_owned())
            .unwrap_or_default();
        let body = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        (status, cache, String::from_utf8(body.to_vec()).unwrap())
    }

    #[test]
    fn no_built_app_means_no_routes() {
        let dir = std::env::temp_dir().join(format!("tc-web-{}", uuid::Uuid::new_v4()));
        assert!(routes::<()>(&dir).is_none());
    }

    #[tokio::test]
    async fn pages_files_and_assets() {
        let dir = built_app();
        let router = routes(&dir).unwrap();

        for path in ["/", "/review", "/close/2026-09", "/i/kestrel-4f2a"] {
            let (status, cache, body) = get(&router, path).await;
            assert_eq!(status, StatusCode::OK, "{path}");
            assert_eq!(cache, "no-cache", "{path}");
            assert!(body.contains("<title>app</title>"), "{path}");
        }
        let (status, cache, body) = get(&router, "/favicon.svg").await;
        assert_eq!(
            (status, cache.as_str(), body.as_str()),
            (StatusCode::OK, "no-cache", "<svg/>")
        );

        let (status, cache, body) = get(&router, "/assets/app-3f9a.js").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(cache, "public, max-age=31536000, immutable");
        assert_eq!(body, "console.log(1)");
        let (status, cache, _) = get(&router, "/assets/app-0000.js").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(cache, "no-cache");

        let (status, _, body) = get(&router, "/api/v1/nothing-here").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(!body.contains("<title>app</title>"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
