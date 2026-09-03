// @group ErrorHandling : API error type with automatic HTTP response conversion

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub message: String,
}

impl ApiError {
    pub fn not_found(msg: impl ToString) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: msg.to_string(),
        }
    }

    pub fn bad_request(msg: impl ToString) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: msg.to_string(),
        }
    }

    pub fn internal(msg: impl ToString) -> Self {
        let detail = msg.to_string();
        let reference = uuid::Uuid::new_v4();
        tracing::error!(%reference, %detail, "request failed with an internal error");
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("Internal server error (reference: {reference})"),
        }
    }

    pub fn unauthorized(msg: impl ToString) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: msg.to_string(),
        }
    }

    pub fn conflict(msg: impl ToString) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            message: msg.to_string(),
        }
    }

    pub fn unavailable(msg: impl ToString) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: msg.to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(json!({ "error": self.message }));
        (self.status, body).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        // Never infer an HTTP status from human-readable error text. Route
        // handlers must opt in to 4xx responses with the constructors above;
        // unexpected manager/I/O errors fail closed as 500 responses.
        ApiError::internal(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arbitrary_error_text_cannot_change_http_status() {
        let error = ApiError::from(anyhow::anyhow!("upstream metadata not found"));
        assert_eq!(error.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(!error.message.contains("upstream metadata"));
        assert!(error.message.contains("reference:"));
    }
}
