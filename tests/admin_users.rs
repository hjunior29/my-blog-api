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
        dto::AdminUpdateUserRequest,
        model::{UserRole, UserStatus},
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
async fn author_cannot_access_admin_endpoints() {
    let (pool, _, app, _dir) = test_app().await;
    user_service::seed_owner(&pool, "owner@example.com", "Owner", "ValidOwnerPass123!")
        .await
        .unwrap();

    let author_id = user_service::create_user(
        &pool,
        "author@example.com",
        "Author Name",
        "AuthorPassword123!",
        UserRole::Author,
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
                        email: "author@example.com".into(),
                        password: "AuthorPassword123!".into(),
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let cookies = extract_set_cookies(&login);
    let access = cookies.get("blog_access").unwrap();

    let admin_req = Request::builder()
        .method("GET")
        .uri("/api/v1/admin/users")
        .header(header::COOKIE, format!("blog_access={access}"))
        .body(Body::empty())
        .unwrap();

    let admin_res = app.oneshot(admin_req).await.unwrap();
    assert_eq!(admin_res.status(), StatusCode::FORBIDDEN);
    assert!(author_id > 0);
}

#[tokio::test]
async fn owner_can_list_users_and_update_role_and_status() {
    let (pool, _, app, _dir) = test_app().await;
    user_service::seed_owner(&pool, "owner@example.com", "Owner", "ValidOwnerPass123!")
        .await
        .unwrap();

    let author_id = user_service::create_user(
        &pool,
        "author@example.com",
        "Author Name",
        "AuthorPassword123!",
        UserRole::Author,
    )
    .await
    .unwrap();

    let owner_login = app
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
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let owner_cookies = extract_set_cookies(&owner_login);
    let owner_body = json_body(owner_login).await;
    let owner_access = owner_cookies.get("blog_access").unwrap();
    let owner_csrf = owner_body["csrf_token"].as_str().unwrap();

    let list_req = Request::builder()
        .method("GET")
        .uri("/api/v1/admin/users")
        .header(header::COOKIE, format!("blog_access={owner_access}"))
        .body(Body::empty())
        .unwrap();

    let list_res = app.clone().oneshot(list_req).await.unwrap();
    assert_eq!(list_res.status(), StatusCode::OK);
    let list_body = json_body(list_res).await;
    assert_eq!(list_body["total"], 2);

    let author_login = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header(header::ORIGIN, "http://localhost:3000")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&LoginRequest {
                        email: "author@example.com".into(),
                        password: "AuthorPassword123!".into(),
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let author_cookies = extract_set_cookies(&author_login);
    let author_refresh = author_cookies.get("blog_refresh").unwrap().clone();

    let disable_payload = serde_json::to_vec(&AdminUpdateUserRequest {
        role: None,
        status: Some(UserStatus::Inactive),
    })
    .unwrap();

    let disable_req = Request::builder()
        .method("PATCH")
        .uri(format!("/api/v1/admin/users/{author_id}"))
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::COOKIE, format!("blog_access={owner_access}"))
        .header("x-csrf-token", owner_csrf)
        .body(Body::from(disable_payload))
        .unwrap();

    let disable_res = app.clone().oneshot(disable_req).await.unwrap();
    assert_eq!(disable_res.status(), StatusCode::OK);
    let disabled_user = json_body(disable_res).await;
    assert_eq!(disabled_user["status"], "inactive");

    let author_refresh_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/refresh")
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::COOKIE, format!("blog_refresh={author_refresh}"))
        .header("x-csrf-token", owner_csrf)
        .body(Body::empty())
        .unwrap();

    let author_refresh_res = app.clone().oneshot(author_refresh_req).await.unwrap();
    assert_eq!(author_refresh_res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn last_owner_protection_prevents_disabling_or_demoting_only_owner() {
    let (pool, _, app, _dir) = test_app().await;
    let owner = user_service::seed_owner(&pool, "owner@example.com", "Owner", "ValidOwnerPass123!")
        .await
        .unwrap();
    let owner_id = match owner {
        user_service::SeedResult::Created(id) | user_service::SeedResult::AlreadyExists(id) => id,
    };

    let owner_login = app
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
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let owner_cookies = extract_set_cookies(&owner_login);
    let owner_body = json_body(owner_login).await;
    let owner_access = owner_cookies.get("blog_access").unwrap();
    let owner_csrf = owner_body["csrf_token"].as_str().unwrap();

    let demote_payload = serde_json::to_vec(&AdminUpdateUserRequest {
        role: Some(UserRole::Author),
        status: None,
    })
    .unwrap();

    let demote_req = Request::builder()
        .method("PATCH")
        .uri(format!("/api/v1/admin/users/{owner_id}"))
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::COOKIE, format!("blog_access={owner_access}"))
        .header("x-csrf-token", owner_csrf)
        .body(Body::from(demote_payload))
        .unwrap();

    let demote_res = app.clone().oneshot(demote_req).await.unwrap();
    assert_eq!(demote_res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let demote_body = json_body(demote_res).await;
    assert_eq!(demote_body["error"]["code"], "last_owner_protection");

    let disable_payload = serde_json::to_vec(&AdminUpdateUserRequest {
        role: None,
        status: Some(UserStatus::Inactive),
    })
    .unwrap();

    let disable_req = Request::builder()
        .method("PATCH")
        .uri(format!("/api/v1/admin/users/{owner_id}"))
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::COOKIE, format!("blog_access={owner_access}"))
        .header("x-csrf-token", owner_csrf)
        .body(Body::from(disable_payload))
        .unwrap();

    let disable_res = app.oneshot(disable_req).await.unwrap();
    assert_eq!(disable_res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let disable_body = json_body(disable_res).await;
    assert_eq!(disable_body["error"]["code"], "last_owner_protection");
}
