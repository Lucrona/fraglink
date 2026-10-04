use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

/// API error: HTTP status + stable machine code + human message.
#[derive(Debug)]
pub struct AppError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
}

pub type ApiResult<T> = Result<T, AppError>;

impl AppError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        AppError {
            status,
            code,
            message: message.into(),
        }
    }
    pub fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message)
    }
    pub fn unauthorized() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "unauthorized", "Sign in first.")
    }
    pub fn not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", "Not found.")
    }
    /// 503 with Retry-After: the server is at capacity, try again later.
    pub fn overloaded() -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "overloaded",
            "The server is busy. Trying again shortly.",
        )
    }
    pub fn internal(e: impl std::fmt::Display) -> Self {
        tracing::error!("internal error: {e}");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "Something went wrong on the server.",
        )
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        if matches!(e, sqlx::Error::PoolTimedOut) {
            tracing::warn!("database pool exhausted (503)");
            return AppError::overloaded();
        }
        AppError::internal(e)
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        AppError::internal(e)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let mut r = (
            self.status,
            Json(json!({ "error": self.code, "message": self.message })),
        )
            .into_response();
        if self.status == StatusCode::SERVICE_UNAVAILABLE {
            // Spread retries out: 15-45 s.
            let secs = 15 + rand::random::<u8>() % 31;
            r.headers_mut().insert(
                axum::http::header::RETRY_AFTER,
                axum::http::HeaderValue::from(secs as u32),
            );
        }
        r
    }
}
