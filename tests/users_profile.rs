use std::collections::HashMap;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, Response, StatusCode, header},
};
use my_blog_api::{
    auth::dto::LoginRequest,
    config::Config,
    database,
    http::router_with_config,
    users::{
        dto::{ChangePasswordRequest, UpdateProfileRequest},
        service as user_service,
    },
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
async fn get_and_update_own_profile() {
    let (pool, _, app, _dir) = test_app().await;
    user_service::seed_owner(
        &pool,
        "owner@example.com",
        "Original Name",
        "ValidOwnerPass123!",
    )
    .await
    .unwrap();

    let login = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header(header::ORIGIN, "http://localhost:3000")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&LoginRequest {
                        email: "owner@example.com".into(),
                        password: "ValidOwnerPass123!".into(),
                        remember_me: None,
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let cookies = extract_set_cookies(&login);
    let body = json_body(login).await;
    let access = cookies.get("blog_access").unwrap();
    let csrf = body["csrf_token"].as_str().unwrap();

    let get_req = Request::builder()
        .method("GET")
        .uri("/api/v1/users/me")
        .header(header::COOKIE, format!("blog_access={access}"))
        .body(Body::empty())
        .unwrap();

    let get_res = app.clone().oneshot(get_req).await.unwrap();
    assert_eq!(get_res.status(), StatusCode::OK);
    let profile = json_body(get_res).await;
    assert_eq!(profile["email"], "owner@example.com");
    assert_eq!(profile["display_name"], "Original Name");
    assert!(profile.get("password_hash").is_none());

    let patch_payload = serde_json::to_vec(&UpdateProfileRequest {
        display_name: Some("Updated Name".into()),
        bio: Some("Updated bio description".into()),
        avatar_media_id: Some("media_123".into()),
    })
    .unwrap();

    let patch_req = Request::builder()
        .method("PATCH")
        .uri("/api/v1/users/me")
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::COOKIE, format!("blog_access={access}"))
        .header("x-csrf-token", csrf)
        .body(Body::from(patch_payload))
        .unwrap();

    let patch_res = app.oneshot(patch_req).await.unwrap();
    assert_eq!(patch_res.status(), StatusCode::OK);
    let updated = json_body(patch_res).await;
    assert_eq!(updated["display_name"], "Updated Name");
    assert_eq!(updated["bio"], "Updated bio description");
    assert_eq!(updated["avatar_media_id"], "media_123");
}

#[tokio::test]
async fn change_password_revokes_other_sessions_and_keeps_current() {
    let (pool, _, app, _dir) = test_app().await;
    user_service::seed_owner(&pool, "owner@example.com", "Owner", "InitialPassword123!")
        .await
        .unwrap();

    let login1 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header(header::ORIGIN, "http://localhost:3000")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&LoginRequest {
                        email: "owner@example.com".into(),
                        password: "InitialPassword123!".into(),
                        remember_me: None,
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let cookies1 = extract_set_cookies(&login1);
    let refresh1 = cookies1.get("blog_refresh").unwrap().clone();

    let login2 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header(header::ORIGIN, "http://localhost:3000")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&LoginRequest {
                        email: "owner@example.com".into(),
                        password: "InitialPassword123!".into(),
                        remember_me: None,
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let cookies2 = extract_set_cookies(&login2);
    let body2 = json_body(login2).await;
    let access2 = cookies2.get("blog_access").unwrap();
    let csrf2 = body2["csrf_token"].as_str().unwrap();

    let wrong_pw_payload = serde_json::to_vec(&ChangePasswordRequest {
        current_password: "WrongCurrentPassword!".into(),
        new_password: "NewBrandNewPassword123!".into(),
    })
    .unwrap();

    let wrong_req = Request::builder()
        .method("PUT")
        .uri("/api/v1/users/me/password")
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::COOKIE, format!("blog_access={access2}"))
        .header("x-csrf-token", csrf2)
        .body(Body::from(wrong_pw_payload))
        .unwrap();

    let wrong_res = app.clone().oneshot(wrong_req).await.unwrap();
    assert_eq!(wrong_res.status(), StatusCode::UNAUTHORIZED);

    let valid_pw_payload = serde_json::to_vec(&ChangePasswordRequest {
        current_password: "InitialPassword123!".into(),
        new_password: "BrandNewSecurePassword2026!".into(),
    })
    .unwrap();

    let change_req = Request::builder()
        .method("PUT")
        .uri("/api/v1/users/me/password")
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::COOKIE, format!("blog_access={access2}"))
        .header("x-csrf-token", csrf2)
        .body(Body::from(valid_pw_payload))
        .unwrap();

    let change_res = app.clone().oneshot(change_req).await.unwrap();
    assert_eq!(change_res.status(), StatusCode::NO_CONTENT);

    let check_current_session = Request::builder()
        .method("GET")
        .uri("/api/v1/users/me")
        .header(header::COOKIE, format!("blog_access={access2}"))
        .body(Body::empty())
        .unwrap();
    let check_res = app.clone().oneshot(check_current_session).await.unwrap();
    assert_eq!(check_res.status(), StatusCode::OK);

    let check_old_session = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/refresh")
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::COOKIE, format!("blog_refresh={refresh1}"))
        .header("x-csrf-token", csrf2)
        .body(Body::empty())
        .unwrap();
    let check_old_res = app.clone().oneshot(check_old_session).await.unwrap();
    assert_eq!(check_old_res.status(), StatusCode::UNAUTHORIZED);

    let old_login = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header(header::ORIGIN, "http://localhost:3000")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&LoginRequest {
                        email: "owner@example.com".into(),
                        password: "InitialPassword123!".into(),
                        remember_me: None,
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(old_login.status(), StatusCode::UNAUTHORIZED);

    let new_login = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header(header::ORIGIN, "http://localhost:3000")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&LoginRequest {
                        email: "owner@example.com".into(),
                        password: "BrandNewSecurePassword2026!".into(),
                        remember_me: None,
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(new_login.status(), StatusCode::OK);
}
