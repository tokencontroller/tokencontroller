//! Health and status: `/api/v1/health`, read by Docker, Azure, the uptime
//! check and status.tokencontroller.com.
//!
//! Owner: the service platform. The status signals (token entries arriving,
//! attribution rate, reconciliation residual, job heartbeats) go here too.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Serialize;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::AppState;

#[must_use]
pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(health))
}

/// Whether one part of the service works.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Check {
    Ok,
    Failing,
}

/// The answer of the health endpoint.
#[derive(Debug, Serialize, ToSchema)]
pub struct Health {
    /// `ok` when every check is `ok`.
    pub status: Check,
    /// The running version, such as `1.0.0`.
    pub version: &'static str,
    /// `open`, or the edition the Enterprise plan build reports.
    pub edition: &'static str,
    /// Whether Postgres answers.
    pub database: Check,
}

/// Is the service up and can it reach its database?
#[utoipa::path(
    get,
    path = "/health",
    tag = "status",
    responses(
        (status = OK, description = "The service works", body = Health),
        (status = SERVICE_UNAVAILABLE, description = "A check is failing", body = Health),
    ),
)]
pub async fn health(State(state): State<AppState>) -> (StatusCode, Json<Health>) {
    let database = match sqlx::query_scalar!(r#"SELECT 1 AS "one!""#)
        .fetch_one(&state.db)
        .await
    {
        Ok(_) => Check::Ok,
        Err(error) => {
            tracing::warn!(%error, "health check: Postgres does not answer");
            Check::Failing
        }
    };
    let (code, status) = if database == Check::Ok {
        (StatusCode::OK, Check::Ok)
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, Check::Failing)
    };
    (
        code,
        Json(Health {
            status,
            version: env!("CARGO_PKG_VERSION"),
            edition: state.extension.edition(),
            database,
        }),
    )
}
