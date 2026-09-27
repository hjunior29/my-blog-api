use axum::{
    Router,
    body::{Body, to_bytes},
    extract::DefaultBodyLimit,
    http::{Request, StatusCode},
    routing::post,
};
use my_blog_api::{
    database,
    http::{MAX_BODY_BYTES, json::Json},
    router,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tower::ServiceExt;

async fn request(app: Router, path: &str, method: &str) -> (StatusCode, Value) {
    let response = app
        .oneshot(
            Request::builder()
                .uri(path)
                .method(method)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    assert_eq!(response.headers()["content-type"], "application/json");
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap())
}

#[tokio::test]
async fn health_and_readiness_report_actual_database_availability() {
    let pool = database::connect("sqlite::memory:", 1).await.unwrap();
    let app = router(pool.clone());
    assert_eq!(
        request(app.clone(), "/health", "GET").await,
        (StatusCode::OK, json!({"status":"ok"}))
    );
    assert_eq!(
        request(app.clone(), "/ready", "GET").await,
        (StatusCode::OK, json!({"status":"ready"}))
    );
    pool.close().await;
    assert_eq!(
        request(app.clone(), "/health", "GET").await.0,
        StatusCode::OK
    );
    let (status, body) = request(app, "/ready", "GET").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        body,
        json!({"error":{"code":"service_unavailable", "message":"Service is unavailable"}})
    );
}

#[tokio::test]
async fn routing_errors_are_json() {
    let pool = database::connect("sqlite::memory:", 1).await.unwrap();
    let app = router(pool.clone());
    for (path, method, expected, code) in [
        ("/missing", "GET", StatusCode::NOT_FOUND, "not_found"),
        (
            "/health",
            "POST",
            StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
        ),
    ] {
        let (status, body) = request(app.clone(), path, method).await;
        assert_eq!(status, expected);
        assert_eq!(body["error"]["code"], code);
    }
    pool.close().await;
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    title: String,
}

fn json_app() -> Router {
    Router::new()
        .route(
            "/",
            post(|Json(payload): Json<Payload>| async { axum::Json(payload) }),
        )
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
}

#[tokio::test]
async fn json_parser_handles_unicode_and_sanitizes_rejections() {
    for (body, content_type, status, code) in [
        (
            r#"{"title":"Olá 🦀"}"#,
            "application/json",
            StatusCode::OK,
            None,
        ),
        (
            "{",
            "application/json",
            StatusCode::BAD_REQUEST,
            Some("invalid_json"),
        ),
        (
            r#"{"title":1}"#,
            "application/json",
            StatusCode::UNPROCESSABLE_ENTITY,
            Some("invalid_payload"),
        ),
        (
            r#"{"title":"test"}"#,
            "text/plain",
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Some("unsupported_media_type"),
        ),
    ] {
        let response = json_app()
            .oneshot(
                Request::post("/")
                    .header("content-type", content_type)
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
        let parsed: Value = serde_json::from_slice(&bytes).unwrap();
        if let Some(code) = code {
            assert_eq!(parsed["error"]["code"], code);
        } else {
            assert_eq!(parsed["title"], "Olá 🦀");
        }
    }
}

#[tokio::test]
async fn oversized_bodies_are_rejected_with_and_without_content_length() {
    for with_length in [false, true] {
        let body = format!(r#"{{"title":"{}"}}"#, "a".repeat(MAX_BODY_BYTES));
        let mut builder = Request::post("/").header("content-type", "application/json");
        if with_length {
            builder = builder.header("content-length", body.len());
        }
        let response = json_app()
            .oneshot(builder.body(Body::from(body)).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
        let parsed: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed["error"]["code"], "payload_too_large");
    }
}

#[tokio::test]
async fn readiness_requires_a_readable_migrated_schema() {
    let pool = database::connect("sqlite::memory:", 1).await.unwrap();
    sqlx::query("DROP TABLE _sqlx_migrations")
        .execute(&pool)
        .await
        .unwrap();
    let (status, body) = request(router(pool.clone()), "/ready", "GET").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["error"]["code"], "service_unavailable");
    pool.close().await;
}
