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
async fn csrf_endpoint_returns_token_for_valid_access_cookie() {
    let (pool, _, app, _dir) = test_app().await;
    user_service::seed_owner(&pool, "owner@example.com", "Owner", "ValidOwnerPass123!")
        .await
        .unwrap();

    let login_payload = serde_json::to_vec(&LoginRequest {
        email: "owner@example.com".into(),
        password: "ValidOwnerPass123!".into(),
    })
    .unwrap();

    let login_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(login_payload))
        .unwrap();

    let login_res = app.clone().oneshot(login_req).await.unwrap();
    let cookies = extract_set_cookies(&login_res);
    let login_body = json_body(login_res).await;
    let expected_csrf = login_body["csrf_token"].as_str().unwrap();

    let csrf_req = Request::builder()
        .method("GET")
        .uri("/api/v1/auth/csrf")
        .header(
            header::COOKIE,
            format!("blog_access={}", cookies["blog_access"]),
        )
        .body(Body::empty())
        .unwrap();

    let csrf_res = app.clone().oneshot(csrf_req).await.unwrap();
    assert_eq!(csrf_res.status(), StatusCode::OK);
    let csrf_body = json_body(csrf_res).await;
    assert_eq!(csrf_body["csrf_token"], expected_csrf);

    let unauth_req = Request::builder()
        .method("GET")
        .uri("/api/v1/auth/csrf")
        .body(Body::empty())
        .unwrap();
    let unauth_res = app.oneshot(unauth_req).await.unwrap();
    assert_eq!(unauth_res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn refresh_token_rotation_and_csrf_validation() {
    let (pool, _, app, _dir) = test_app().await;
    user_service::seed_owner(&pool, "owner@example.com", "Owner", "ValidOwnerPass123!")
        .await
        .unwrap();

    let login_payload = serde_json::to_vec(&LoginRequest {
        email: "owner@example.com".into(),
        password: "ValidOwnerPass123!".into(),
    })
    .unwrap();

    let login_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header(header::ORIGIN, "http://localhost:3000")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(login_payload))
                .unwrap(),
        )
        .await
        .unwrap();

    let cookies = extract_set_cookies(&login_res);
    let login_body = json_body(login_res).await;
    let csrf_token = login_body["csrf_token"].as_str().unwrap();
    let refresh_token = cookies.get("blog_refresh").unwrap().clone();

    let no_csrf_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/refresh")
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::COOKIE, format!("blog_refresh={refresh_token}"))
        .body(Body::empty())
        .unwrap();

    let no_csrf_res = app.clone().oneshot(no_csrf_req).await.unwrap();
    assert_eq!(no_csrf_res.status(), StatusCode::FORBIDDEN);

    let refresh_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/refresh")
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::COOKIE, format!("blog_refresh={refresh_token}"))
        .header("x-csrf-token", csrf_token)
        .body(Body::empty())
        .unwrap();

    let refresh_res = app.clone().oneshot(refresh_req).await.unwrap();
    assert_eq!(refresh_res.status(), StatusCode::OK);

    let new_cookies = extract_set_cookies(&refresh_res);
    let new_refresh_token = new_cookies.get("blog_refresh").unwrap().clone();
    assert_ne!(refresh_token, new_refresh_token);

    let replay_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/refresh")
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::COOKIE, format!("blog_refresh={refresh_token}"))
        .header("x-csrf-token", csrf_token)
        .body(Body::empty())
        .unwrap();

    let replay_res = app.clone().oneshot(replay_req).await.unwrap();
    assert_eq!(replay_res.status(), StatusCode::UNAUTHORIZED);

    let replay_cookies = extract_set_cookies(&replay_res);
    assert_eq!(
        replay_cookies.get("blog_access").map(String::as_str),
        Some("")
    );
    assert_eq!(
        replay_cookies.get("blog_refresh").map(String::as_str),
        Some("")
    );

    let second_refresh_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/refresh")
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::COOKIE, format!("blog_refresh={new_refresh_token}"))
        .header("x-csrf-token", csrf_token)
        .body(Body::empty())
        .unwrap();

    let second_res = app.oneshot(second_refresh_req).await.unwrap();
    assert_eq!(second_res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn logout_revokes_session_and_clears_cookies() {
    let (pool, _, app, _dir) = test_app().await;
    user_service::seed_owner(&pool, "owner@example.com", "Owner", "ValidOwnerPass123!")
        .await
        .unwrap();

    let login_payload = serde_json::to_vec(&LoginRequest {
        email: "owner@example.com".into(),
        password: "ValidOwnerPass123!".into(),
    })
    .unwrap();

    let login_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header(header::ORIGIN, "http://localhost:3000")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(login_payload))
                .unwrap(),
        )
        .await
        .unwrap();

    let cookies = extract_set_cookies(&login_res);
    let body = json_body(login_res).await;
    let csrf_token = body["csrf_token"].as_str().unwrap();
    let access_token = cookies.get("blog_access").unwrap();
    let refresh_token = cookies.get("blog_refresh").unwrap();

    let logout_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/logout")
        .header(header::ORIGIN, "http://localhost:3000")
        .header(
            header::COOKIE,
            format!("blog_access={access_token}; blog_refresh={refresh_token}"),
        )
        .header("x-csrf-token", csrf_token)
        .body(Body::empty())
        .unwrap();

    let logout_res = app.clone().oneshot(logout_req).await.unwrap();
    assert_eq!(logout_res.status(), StatusCode::NO_CONTENT);

    let clear_cookies = extract_set_cookies(&logout_res);
    assert_eq!(
        clear_cookies.get("blog_access").map(String::as_str),
        Some("")
    );
    assert_eq!(
        clear_cookies.get("blog_refresh").map(String::as_str),
        Some("")
    );

    let refresh_after_logout = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/refresh")
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::COOKIE, format!("blog_refresh={refresh_token}"))
        .header("x-csrf-token", csrf_token)
        .body(Body::empty())
        .unwrap();

    let refresh_res = app.oneshot(refresh_after_logout).await.unwrap();
    assert_eq!(refresh_res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn logout_all_revokes_every_session_of_user() {
    let (pool, _, app, _dir) = test_app().await;
    user_service::seed_owner(&pool, "owner@example.com", "Owner", "ValidOwnerPass123!")
        .await
        .unwrap();

    let login1_payload = serde_json::to_vec(&LoginRequest {
        email: "owner@example.com".into(),
        password: "ValidOwnerPass123!".into(),
    })
    .unwrap();

    let login1_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header(header::ORIGIN, "http://localhost:3000")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(login1_payload.clone()))
                .unwrap(),
        )
        .await
        .unwrap();

    let cookies1 = extract_set_cookies(&login1_res);
    let refresh1 = cookies1.get("blog_refresh").unwrap().clone();

    let login2_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header(header::ORIGIN, "http://localhost:3000")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(login1_payload))
                .unwrap(),
        )
        .await
        .unwrap();

    let cookies2 = extract_set_cookies(&login2_res);
    let body2 = json_body(login2_res).await;
    let access2 = cookies2.get("blog_access").unwrap();
    let refresh2 = cookies2.get("blog_refresh").unwrap().clone();
    let csrf2 = body2["csrf_token"].as_str().unwrap();

    let logout_all_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/logout-all")
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::COOKIE, format!("blog_access={access2}"))
        .header("x-csrf-token", csrf2)
        .body(Body::empty())
        .unwrap();

    let logout_all_res = app.clone().oneshot(logout_all_req).await.unwrap();
    assert_eq!(logout_all_res.status(), StatusCode::NO_CONTENT);

    let refresh1_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/refresh")
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::COOKIE, format!("blog_refresh={refresh1}"))
        .header("x-csrf-token", csrf2)
        .body(Body::empty())
        .unwrap();

    let refresh1_res = app.clone().oneshot(refresh1_req).await.unwrap();
    assert_eq!(refresh1_res.status(), StatusCode::UNAUTHORIZED);

    let refresh2_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/refresh")
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::COOKIE, format!("blog_refresh={refresh2}"))
        .header("x-csrf-token", csrf2)
        .body(Body::empty())
        .unwrap();

    let refresh2_res = app.oneshot(refresh2_req).await.unwrap();
    assert_eq!(refresh2_res.status(), StatusCode::UNAUTHORIZED);
}
