use axum::{
    Json,
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: &'static str,
    headers: Option<Box<HeaderMap>>,
}

#[derive(Serialize)]
struct ErrorEnvelope {
    error: ErrorDetails,
}

#[derive(Serialize)]
struct ErrorDetails {
    code: &'static str,
    message: &'static str,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: &'static str) -> Self {
        Self {
            status,
            code,
            message,
            headers: None,
        }
    }

    pub fn with_header(mut self, name: HeaderName, value: HeaderValue) -> Self {
        let headers = self.headers.get_or_insert_with(Box::default);
        headers.insert(name, value);
        self
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }

    pub fn code(&self) -> &'static str {
        self.code
    }

    pub fn bad_request(code: &'static str, message: &'static str) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message)
    }

    pub fn unauthorized(message: &'static str) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "unauthorized", message)
    }

    pub fn invalid_credentials() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "invalid_credentials",
            "Invalid email or password",
        )
    }

    pub fn forbidden(message: &'static str) -> Self {
        Self::new(StatusCode::FORBIDDEN, "forbidden", message)
    }

    pub fn invalid_csrf_token() -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "invalid_csrf_token",
            "Invalid or missing CSRF token",
        )
    }

    pub fn not_found(message: &'static str) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", message)
    }

    pub fn conflict(code: &'static str, message: &'static str) -> Self {
        Self::new(StatusCode::CONFLICT, code, message)
    }

    pub fn precondition_failed(message: &'static str) -> Self {
        Self::new(
            StatusCode::PRECONDITION_FAILED,
            "precondition_failed",
            message,
        )
    }

    pub fn precondition_required(message: &'static str) -> Self {
        Self::new(
            StatusCode::PRECONDITION_REQUIRED,
            "precondition_required",
            message,
        )
    }

    pub fn payload_too_large(message: &'static str) -> Self {
        Self::new(StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large", message)
    }

    pub fn unsupported_media_type(message: &'static str) -> Self {
        Self::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_media_type",
            message,
        )
    }

    pub fn unprocessable_entity(code: &'static str, message: &'static str) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, code, message)
    }

    pub fn too_many_requests(retry_after_secs: Option<u64>) -> Self {
        let mut error = Self::new(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limit_exceeded",
            "Too many requests, please try again later",
        );
        if let Some(val) = retry_after_secs.map(HeaderValue::from) {
            error = error.with_header(axum::http::header::RETRY_AFTER, val);
        }
        error
    }

    pub fn internal() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "Internal server error",
        )
    }

    pub fn unavailable() -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "service_unavailable",
            "Service is unavailable",
        )
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut response = (
            self.status,
            Json(ErrorEnvelope {
                error: ErrorDetails {
                    code: self.code,
                    message: self.message,
                },
            }),
        )
            .into_response();
        if let Some(headers) = self.headers {
            for (name, value) in *headers {
                if let Some(name) = name {
                    response.headers_mut().insert(name, value);
                }
            }
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    #[tokio::test]
    async fn error_envelope_and_status_match() {
        let error = ApiError::invalid_credentials();
        assert_eq!(error.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(error.code(), "invalid_credentials");

        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let bytes = to_bytes(response.into_body(), 1024).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"]["code"], "invalid_credentials");
        assert_eq!(body["error"]["message"], "Invalid email or password");
    }

    #[tokio::test]
    async fn too_many_requests_sets_retry_after_header() {
        let error = ApiError::too_many_requests(Some(30));
        assert_eq!(error.status(), StatusCode::TOO_MANY_REQUESTS);

        let response = error.into_response();
        assert_eq!(
            response
                .headers()
                .get(axum::http::header::RETRY_AFTER)
                .unwrap(),
            "30"
        );
    }
}
