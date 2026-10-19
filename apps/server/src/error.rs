//! The one error type API handlers return. Every error reaches the client as
//! JSON with a stable `error` code and a plain-English `message`.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use utoipa::ToSchema;

/// An error an API handler returns.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("{0}")]
    BadRequest(String),
    #[error("sign in first")]
    Unauthorized,
    #[error("your role cannot do this")]
    Forbidden,
    /// A capability the caller's grants do not cover; the message names it.
    #[error("{0}")]
    NoCapability(String),
    #[error("not found")]
    NotFound,
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Gone(String),
    #[error("{0}")]
    TooManyRequests(String),
    #[error("the database is not available")]
    Database(#[from] sqlx::Error),
    #[error("something went wrong on our side")]
    Internal(#[from] anyhow::Error),
}

/// The body of every error response.
#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorBody {
    /// A stable code, such as `not_found`.
    pub error: &'static str,
    /// What went wrong, in plain English.
    pub message: String,
}

impl ApiError {
    #[must_use]
    pub fn status(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden | Self::NoCapability(_) => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::Gone(_) => StatusCode::GONE,
            Self::TooManyRequests(_) => StatusCode::TOO_MANY_REQUESTS,
            Self::Database(_) => StatusCode::SERVICE_UNAVAILABLE,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::BadRequest(_) => "bad_request",
            Self::Unauthorized => "unauthorized",
            Self::Forbidden | Self::NoCapability(_) => "forbidden",
            Self::NotFound => "not_found",
            Self::Conflict(_) => "conflict",
            Self::Gone(_) => "gone",
            Self::TooManyRequests(_) => "too_many_requests",
            Self::Database(_) => "database_unavailable",
            Self::Internal(_) => "internal",
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match &self {
            Self::Database(error) => tracing::error!(%error, "database error"),
            Self::Internal(error) => tracing::error!("{error:#}"),
            _ => {}
        }
        let body = ErrorBody {
            error: self.code(),
            message: self.to_string(),
        };
        (self.status(), Json(body)).into_response()
    }
}
