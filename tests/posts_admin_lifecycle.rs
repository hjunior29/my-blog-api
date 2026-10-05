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
    posts::dto::{CreatePostDto, UpdatePostDto},
    users::{model::UserRole, service as user_service},
};
use serde_json::Value;
use sqlx::SqlitePool;
use tower::ServiceExt;

async fn test_app() -> (SqlitePool, Router, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("admin_posts_lifecycle_test.db");
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

    let app = router_with_config(pool.clone(), config);
    (pool, app, dir)
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
    let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

async fn login_user(app: &Router, email: &str, password: &str) -> (String, String) {
    let login_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header(header::ORIGIN, "http://localhost:3000")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&LoginRequest {
                        email: email.into(),
                        password: password.into(),
                        remember_me: None,
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let cookies = extract_set_cookies(&login_res);
    let access_token = cookies.get("blog_access").unwrap().to_string();

    let csrf_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/auth/csrf")
                .header(header::COOKIE, format!("blog_access={}", access_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = json_body(csrf_res).await;
    let csrf_token = body["csrf_token"].as_str().unwrap().to_string();

    (access_token, csrf_token)
}

#[tokio::test]
async fn lifecycle_enforces_if_match_and_updates_version() {
    let (pool, app, _dir) = test_app().await;

    user_service::create_user(
        &pool,
        "author@example.com",
        "Author",
        "ValidPassword123!",
        UserRole::Author,
    )
    .await
    .unwrap();

    let (access, csrf) = login_user(&app, "author@example.com", "ValidPassword123!").await;

    let create_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/admin/posts")
                .header(header::ORIGIN, "http://localhost:3000")
                .header(header::COOKIE, format!("blog_access={}", access))
                .header("x-csrf-token", &csrf)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&CreatePostDto {
                        title: "Lifecycle Post".into(),
                        summary: None,
                        content_md: "Initial markdown text".into(),
                        featured_image_media_id: None,
                        status: None,
                        tags: None,
                        scheduled_for: None,
                        book_color: None,
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let post_json = json_body(create_res).await;
    let post_id = post_json["id"].as_i64().unwrap();

    let get_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/admin/posts/{}", post_id))
                .header(header::COOKIE, format!("blog_access={}", access))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_res.status(), StatusCode::OK);
    assert_eq!(get_res.headers().get(header::ETAG).unwrap(), "\"1\"");

    let patch_no_if_match = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/api/v1/admin/posts/{}", post_id))
                .header(header::ORIGIN, "http://localhost:3000")
                .header(header::COOKIE, format!("blog_access={}", access))
                .header("x-csrf-token", &csrf)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&UpdatePostDto {
                        title: Some("New Title".into()),
                        summary: None,
                        content_md: None,
                        featured_image_media_id: None,
                        status: None,
                        tags: None,
                        scheduled_for: None,
                        book_color: None,
                        version: 1,
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        patch_no_if_match.status(),
        StatusCode::PRECONDITION_REQUIRED
    );

    let patch_mismatch = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/api/v1/admin/posts/{}", post_id))
                .header(header::ORIGIN, "http://localhost:3000")
                .header(header::COOKIE, format!("blog_access={}", access))
                .header("x-csrf-token", &csrf)
                .header(header::IF_MATCH, "\"99\"")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&UpdatePostDto {
                        title: Some("New Title".into()),
                        summary: None,
                        content_md: None,
                        featured_image_media_id: None,
                        status: None,
                        tags: None,
                        scheduled_for: None,
                        book_color: None,
                        version: 99,
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(patch_mismatch.status(), StatusCode::PRECONDITION_FAILED);

    let patch_ok = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/api/v1/admin/posts/{}", post_id))
                .header(header::ORIGIN, "http://localhost:3000")
                .header(header::COOKIE, format!("blog_access={}", access))
                .header("x-csrf-token", &csrf)
                .header(header::IF_MATCH, "\"1\"")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "title": "Updated Title",
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(patch_ok.status(), StatusCode::OK);
    assert_eq!(patch_ok.headers().get(header::ETAG).unwrap(), "\"2\"");

    let publish_ok = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/admin/posts/{}/publish", post_id))
                .header(header::ORIGIN, "http://localhost:3000")
                .header(header::COOKIE, format!("blog_access={}", access))
                .header("x-csrf-token", &csrf)
                .header(header::IF_MATCH, "\"2\"")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(publish_ok.status(), StatusCode::OK);
    assert_eq!(publish_ok.headers().get(header::ETAG).unwrap(), "\"3\"");
    let pub_body = json_body(publish_ok).await;
    assert_eq!(pub_body["status"], "published");

    let unpublish_ok = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/admin/posts/{}/unpublish", post_id))
                .header(header::ORIGIN, "http://localhost:3000")
                .header(header::COOKIE, format!("blog_access={}", access))
                .header("x-csrf-token", &csrf)
                .header(header::IF_MATCH, "\"3\"")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unpublish_ok.status(), StatusCode::OK);
    assert_eq!(unpublish_ok.headers().get(header::ETAG).unwrap(), "\"4\"");

    let delete_ok = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/v1/admin/posts/{}", post_id))
                .header(header::ORIGIN, "http://localhost:3000")
                .header(header::COOKIE, format!("blog_access={}", access))
                .header("x-csrf-token", &csrf)
                .header(header::IF_MATCH, "\"4\"")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete_ok.status(), StatusCode::NO_CONTENT);
}
