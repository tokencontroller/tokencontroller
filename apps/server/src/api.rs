//! The HTTP API under `/api/v1`, and its OpenAPI document (ADR 0009: the web
//! app uses nothing else).
//!
//! Each area module returns its own routes from a `routes()` function; this
//! file only puts them together. To add an endpoint, add a handler with
//! `#[utoipa::path]` in its area module and register it there with
//! `routes!`, so the OpenAPI document always lists it.

use std::path::Path;
use std::sync::Arc;

use anyhow::Context as _;
use axum::routing::get;
use axum::{Json, Router};
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;

use crate::AppState;
use crate::sign_in::SecuritySchemes;

/// The path every API endpoint sits under.
pub const API_PREFIX: &str = "/api/v1";

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Token Controller API",
        description = "The versioned REST API of Token Controller. Everything the web app does goes through it. See docs.tokencontroller.com/reference/api.",
        license(name = "AGPL-3.0-only", identifier = "AGPL-3.0-only"),
    ),
    components(schemas(crate::error::ErrorBody)),
    modifiers(&SecuritySchemes)
)]
struct ApiDoc;

/// Every area's routes, relative to [`API_PREFIX`].
fn area_routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .merge(crate::status::routes())
        .merge(crate::organizations::routes())
        .merge(crate::sign_in::routes())
        .merge(crate::tags::routes())
        .merge(crate::mcp::routes())
        .merge(crate::ingest::routes())
        .merge(crate::messages::routes())
        .merge(crate::splits::routes())
        .merge(crate::events::routes())
        .merge(crate::grants::routes())
        .merge(crate::cutoff::routes())
        .merge(crate::submissions::routes())
        .merge(crate::plans::routes())
        .merge(crate::price_table::routes())
        .merge(crate::jobs::routes())
        .merge(crate::integrations::routes())
        .merge(crate::bills::routes())
        .merge(crate::reports::routes())
}

fn api(extra: OpenApiRouter<AppState>) -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(ApiDoc::openapi()).nest(API_PREFIX, area_routes().merge(extra))
}

/// The whole HTTP service, ready to serve: the API, and the web app on
/// every other path when the image carries it.
pub fn router(state: AppState) -> Router {
    router_with(state, OpenApiRouter::new())
}

/// Like [`router`], with a build's own API routes (`extra`, relative to
/// [`API_PREFIX`]) next to the service's, in the same OpenAPI document.
pub fn router_with(state: AppState, extra: OpenApiRouter<AppState>) -> Router {
    let (api_router, openapi) = api(extra).split_for_parts();
    let openapi = Arc::new(openapi);
    let router = api_router.route(
        "/api/v1/openapi.json",
        get(move || {
            let openapi = Arc::clone(&openapi);
            async move { Json(openapi) }
        }),
    );
    let router = match crate::web::routes(Path::new(crate::web::WEB_DIR)) {
        Some(web) => router.merge(web),
        None => router,
    };
    router
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(TraceLayer::new_for_http().make_span_with(request_span))
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .with_state(state)
}

/// One log span per request, with its `x-request-id`, so the log lines of
/// one answer can be found. The path goes without its query, which can hold
/// a sign-in token.
fn request_span<B>(request: &axum::http::Request<B>) -> tracing::Span {
    let id = request
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    tracing::info_span!(
        "request",
        method = %request.method(),
        path = request.uri().path(),
        request_id = id
    )
}

/// The OpenAPI document as pretty JSON, as served at
/// `/api/v1/openapi.json` and committed as `apps/server/openapi.json`.
///
/// # Errors
///
/// Only if the document cannot be written as JSON.
pub fn openapi_json() -> anyhow::Result<String> {
    pretty(&api(OpenApiRouter::new()).into_openapi())
}

/// The OpenAPI document with a build's own paths and schemas (`extra`, with
/// its paths under [`API_PREFIX`]) merged in.
///
/// # Errors
///
/// Only if the document cannot be written as JSON.
pub fn openapi_json_with(extra: utoipa::openapi::OpenApi) -> anyhow::Result<String> {
    let mut document = api(OpenApiRouter::new()).into_openapi();
    document.merge(extra);
    pretty(&document)
}

fn pretty(document: &utoipa::openapi::OpenApi) -> anyhow::Result<String> {
    document
        .to_pretty_json()
        .context("could not write the OpenAPI document")
}
