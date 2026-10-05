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

use crate::{config::Config, health::handler};
use error::ApiError;

pub const MAX_BODY_BYTES: usize = 64 * 1024;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub config: Config,
    pub storage: crate::media::StorageBackend,
}

impl axum::extract::FromRef<AppState> for SqlitePool {
    fn from_ref(state: &AppState) -> Self {
        state.pool.clone()
    }
}

impl axum::extract::FromRef<AppState> for Config {
    fn from_ref(state: &AppState) -> Self {
        state.config.clone()
    }
}

impl axum::extract::FromRef<AppState> for crate::media::StorageBackend {
    fn from_ref(state: &AppState) -> Self {
        state.storage.clone()
    }
}

pub const ROOT_BODY_LIMIT_BYTES: usize = 384 * 1024;

pub fn router_with_config(pool: SqlitePool, config: Config) -> Router {
    let storage = crate::media::init_storage(&config);
    let state = AppState {
        pool,
        config: config.clone(),
        storage,
    };

    let origin_check_layer =
        middleware::from_fn_with_state(config.clone(), crate::auth::check_origin);

    let auth_routes = crate::auth::router().layer(origin_check_layer.clone());
    let users_routes = crate::users::router().layer(origin_check_layer.clone());
    let admin_routes = crate::admin::router().layer(origin_check_layer);
    let posts_routes = crate::posts::router();
    let media_routes = crate::media::public_routes();

    let api_routes = Router::new()
        .nest("/auth", auth_routes)
        .nest("/users", users_routes)
        .nest("/admin", admin_routes)
        .merge(posts_routes)
        .merge(media_routes);

    Router::new()
        .route("/health", get(handler::live))
        .route("/ready", get(handler::ready))
        .nest("/api/v1", api_routes)
        .fallback(|| async { ApiError::not_found("Route not found") })
        .method_not_allowed_fallback(|| async {
            ApiError::new(
                StatusCode::METHOD_NOT_ALLOWED,
                "method_not_allowed",
                "Method not allowed",
            )
        })
        .layer(DefaultBodyLimit::max(ROOT_BODY_LIMIT_BYTES))
        .layer(middleware::from_fn(request_timeout))
        .layer(middleware::from_fn(security_headers))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

pub fn router(pool: SqlitePool) -> Router {
    let config = Config::from_lookup(|_| Ok(None)).expect("default test config");
    router_with_config(pool, config)
}

async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        axum::http::header::X_CONTENT_TYPE_OPTIONS,
        axum::http::HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        axum::http::header::X_FRAME_OPTIONS,
        axum::http::HeaderValue::from_static("DENY"),
    );
    headers.insert(
        axum::http::header::REFERRER_POLICY,
        axum::http::HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    response
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
