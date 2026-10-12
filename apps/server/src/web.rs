//! The web app (ADR 0009). The image puts its built files in [`WEB_DIR`];
//! the service serves them on every path outside `/api/`. The app routes in
//! the browser, so a path that is not a file, such as `/close/2026-09`,
//! gets `index.html`.

use std::path::Path;

use axum::Router;
use axum::extract::Request;
use axum::http::{HeaderValue, header};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::any;
use tower_http::services::{ServeDir, ServeFile};

use crate::error::ApiError;

/// Where the image keeps the built web app (`Dockerfile`, stage `web`).
pub const WEB_DIR: &str = "/usr/share/tokencontroller/web";

/// Files under `/assets/` have a hash in their name, so they never change.
const IMMUTABLE: HeaderValue = HeaderValue::from_static("public, max-age=31536000, immutable");
/// Everything else, `index.html` above all, is asked for again each time.
const NO_CACHE: HeaderValue = HeaderValue::from_static("no-cache");

/// `app` with the web app behind it, when `dir` holds one. Without it (a
/// local `cargo run`), only the API answers. Unknown `/api/` paths answer
/// the API's JSON 404, never the web app.
pub fn with_web_app(app: Router, dir: &Path) -> Router {
    let index = dir.join("index.html");
    let api_not_found = Router::new().route("/api/{*rest}", any(|| async { ApiError::NotFound }));
    if !index.is_file() {
        tracing::info!(dir = %dir.display(), "no web app to serve");
        return app.merge(api_not_found);
    }
    let files = Router::new()
        .nest_service("/assets", ServeDir::new(dir.join("assets")))
        .fallback_service(ServeDir::new(dir).fallback(ServeFile::new(index)))
        .layer(middleware::from_fn(cache_control));
    app.merge(api_not_found).fallback_service(files)
}

async fn cache_control(request: Request, next: Next) -> Response {
    let assets = request.uri().path().starts_with("/assets/");
    let mut response = next.run(request).await;
    if response.status().is_success() {
        let value = if assets { IMMUTABLE } else { NO_CACHE };
        response.headers_mut().insert(header::CACHE_CONTROL, value);
    }
    response
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::routing::get;
    use http_body_util::BodyExt as _;
    use tower::ServiceExt as _;

    use super::*;

    /// A folder like the image's: `index.html` and one hashed asset.
    fn web_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tc-web-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(dir.join("assets")).unwrap();
        fs::write(
            dir.join("index.html"),
            "<!doctype html><title>Token Controller</title>",
        )
        .unwrap();
        fs::write(dir.join("assets/index-3f9a.js"), "console.log(1)").unwrap();
        fs::write(dir.join("logo-light.svg"), "<svg/>").unwrap();
        dir
    }

    async fn get_path(app: Router, path: &str) -> (StatusCode, Option<String>, String) {
        let response = app
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let cache = response
            .headers()
            .get(header::CACHE_CONTROL)
            .map(|v| v.to_str().unwrap().to_owned());
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, cache, String::from_utf8_lossy(&body).into_owned())
    }

    fn app(dir: &Path) -> Router {
        let api = Router::new().route("/api/v1/health", get(|| async { "ok" }));
        with_web_app(api, dir)
    }

    #[tokio::test]
    async fn pages_of_the_app_get_index_html() {
        let dir = web_dir();
        for path in ["/", "/close/2026-09", "/i/kestrel-4f2a"] {
            let (status, cache, body) = get_path(app(&dir), path).await;
            assert_eq!(status, StatusCode::OK, "{path}");
            assert_eq!(cache.as_deref(), Some("no-cache"), "{path}");
            assert!(body.contains("<title>Token Controller</title>"), "{path}");
        }
    }

    #[tokio::test]
    async fn files_are_served_and_assets_cached_for_good() {
        let dir = web_dir();
        let (status, cache, body) = get_path(app(&dir), "/assets/index-3f9a.js").await;
        assert_eq!(
            (status, cache.as_deref(), body.as_str()),
            (
                StatusCode::OK,
                Some("public, max-age=31536000, immutable"),
                "console.log(1)"
            )
        );
        let (status, cache, _) = get_path(app(&dir), "/logo-light.svg").await;
        assert_eq!(
            (status, cache.as_deref()),
            (StatusCode::OK, Some("no-cache"))
        );
        // A missing asset is a 404, not the app's HTML.
        let (status, _, _) = get_path(app(&dir), "/assets/gone-0000.js").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn the_api_keeps_its_own_answers() {
        let dir = web_dir();
        let (status, _, body) = get_path(app(&dir), "/api/v1/health").await;
        assert_eq!((status, body.as_str()), (StatusCode::OK, "ok"));
        let (status, _, body) = get_path(app(&dir), "/api/v1/nothing-here").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(body.contains("\"not_found\""), "{body}");
    }

    #[tokio::test]
    async fn without_a_web_app_only_the_api_answers() {
        let dir = std::env::temp_dir().join(format!("tc-no-web-{}", uuid::Uuid::new_v4()));
        let (status, _, _) = get_path(app(&dir), "/close/2026-09").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _, body) = get_path(app(&dir), "/api/v1/health").await;
        assert_eq!((status, body.as_str()), (StatusCode::OK, "ok"));
    }
}
