pub mod error;
pub mod json;

use std::time::Duration;

use axum::{
    Router,
    extract::{DefaultBodyLimit, Request},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use sqlx::SqlitePool;
use tower_http::trace::TraceLayer;

use crate::health::handler;
use error::ApiError;

pub const MAX_BODY_BYTES: usize = 64 * 1024;

pub fn router(pool: SqlitePool) -> Router {
    Router::new()
        .route("/health", get(handler::live))
        .route("/ready", get(handler::ready))
        .fallback(|| async { ApiError::new(StatusCode::NOT_FOUND, "not_found", "Route not found") })
        .method_not_allowed_fallback(|| async {
            ApiError::new(
                StatusCode::METHOD_NOT_ALLOWED,
                "method_not_allowed",
                "Method not allowed",
            )
        })
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(middleware::from_fn(request_timeout))
        .layer(TraceLayer::new_for_http())
        .with_state(pool)
}

async fn request_timeout(request: Request, next: Next) -> Response {
    match tokio::time::timeout(Duration::from_secs(10), next.run(request)).await {
        Ok(response) => response,
        Err(_) => ApiError::new(
            StatusCode::REQUEST_TIMEOUT,
            "request_timeout",
            "Request timed out",
        )
        .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use tower::ServiceExt;

    #[tokio::test(start_paused = true)]
    async fn slow_handlers_return_a_json_timeout() {
        let app = Router::new()
            .route(
                "/",
                get(|| async {
                    tokio::time::sleep(Duration::from_secs(20)).await;
                    "late response"
                }),
            )
            .layer(middleware::from_fn(request_timeout));
        let response = app.oneshot(Request::new(Body::empty())).await.unwrap();
        assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
        let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"]["code"], "request_timeout");
    }
}
