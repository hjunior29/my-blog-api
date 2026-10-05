use std::collections::HashMap;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, Response, StatusCode, header},
};
use my_blog_api::{
    auth::dto::LoginRequest, config::Config, database, http::router_with_config,
    users::service as user_service,
};
use serde_json::Value;
use sqlx::SqlitePool;
use tower::ServiceExt;

async fn test_app() -> (SqlitePool, Config, Router, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let url = format!("sqlite://{}", db_path.display());
    let pool = database::connect(&url, 4).await.unwrap();

    let config = Config::from_lookup(|key| match key {
        "OWNER_ONLY" => Ok(Some("false".into())),
        "APP_ENV" => Ok(Some("test".into())),
        "APP_ORIGIN" => Ok(Some("http://localhost:3000".into())),
        "SECURE_COOKIES" => Ok(Some("false".into())),
        "JWT_SECRET" => Ok(Some(
            "test-jwt-secret-key-must-be-at-least-32-bytes!".into(),
        )),
        _ => Ok(None),
    })
    .unwrap();

    let app = router_with_config(pool.clone(), config.clone());
    (pool, config, app, dir)
}

fn extract_set_cookies(response: &Response<Body>) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for header in response.headers().get_all(header::SET_COOKIE) {
        if let Ok(cookie_str) = header.to_str()
            && let Some(first_part) = cookie_str.split(';').next()
            && let Some((k, v)) = first_part.split_once('=')
        {
            map.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    map
}

async fn json_body(response: Response<Body>) -> Value {
    let bytes = to_bytes(response.into_body(), 16 * 1024).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn login_success_returns_user_and_sets_cookies() {
    let (pool, _, app, _dir) = test_app().await;
    user_service::seed_owner(&pool, "owner@example.com", "Owner", "ValidOwnerPass123!")
        .await
        .unwrap();

    let payload = serde_json::to_vec(&LoginRequest {
        email: "owner@example.com".into(),
        password: "ValidOwnerPass123!".into(),
        remember_me: None,
    })
    .unwrap();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload))
        .unwrap();

    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers().get(header::CACHE_CONTROL).unwrap(),
        "no-store"
    );

    let cookies = extract_set_cookies(&res);
    assert!(cookies.contains_key("blog_access"));
    assert!(cookies.contains_key("blog_refresh"));

    let body = json_body(res).await;
    assert_eq!(body["user"]["email"], "owner@example.com");
    assert_eq!(body["user"]["role"], "owner");
    assert_eq!(body["user"]["status"], "active");
    assert!(body["user"].get("password_hash").is_none());
    assert!(!body["csrf_token"].as_str().unwrap().is_empty());
}

#[tokio::test]
async fn login_failure_does_not_enumerate_users() {
    let (pool, _, app, _dir) = test_app().await;
    user_service::seed_owner(&pool, "owner@example.com", "Owner", "ValidOwnerPass123!")
        .await
        .unwrap();

    let non_existent_payload = serde_json::to_vec(&LoginRequest {
        email: "missing@example.com".into(),
        password: "WrongPassword123!".into(),
        remember_me: None,
    })
    .unwrap();

    let req1 = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(non_existent_payload))
        .unwrap();

    let res1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(res1.status(), StatusCode::UNAUTHORIZED);
    let body1 = json_body(res1).await;
    assert_eq!(body1["error"]["code"], "invalid_credentials");

    let wrong_pw_payload = serde_json::to_vec(&LoginRequest {
        email: "owner@example.com".into(),
        password: "WrongPassword123!".into(),
        remember_me: None,
    })
    .unwrap();

    let req2 = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(wrong_pw_payload))
        .unwrap();

    let res2 = app.oneshot(req2).await.unwrap();
    assert_eq!(res2.status(), StatusCode::UNAUTHORIZED);
    let body2 = json_body(res2).await;
    assert_eq!(body2["error"]["code"], "invalid_credentials");
    assert_eq!(body1["error"]["message"], body2["error"]["message"]);
}

#[tokio::test]
async fn state_mutations_reject_untrusted_origins() {
    let (_, _, app, _dir) = test_app().await;

    let payload = serde_json::to_vec(&LoginRequest {
        email: "any@example.com".into(),
        password: "AnyPass123!".into(),
        remember_me: None,
    })
    .unwrap();

    let no_origin = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload.clone()))
        .unwrap();

    let res1 = app.clone().oneshot(no_origin).await.unwrap();
    assert_eq!(res1.status(), StatusCode::FORBIDDEN);

    let evil_origin = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header(header::ORIGIN, "https://malicious-site.com")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload.clone()))
        .unwrap();

    let res2 = app.clone().oneshot(evil_origin).await.unwrap();
    assert_eq!(res2.status(), StatusCode::FORBIDDEN);

    let cross_site = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header(header::ORIGIN, "http://localhost:3000")
        .header("sec-fetch-site", "cross-site")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload))
        .unwrap();

    let res3 = app.oneshot(cross_site).await.unwrap();
    assert_eq!(res3.status(), StatusCode::FORBIDDEN);
}
