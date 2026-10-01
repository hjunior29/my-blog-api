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
    posts::dto::{CreatePostDto, PreviewPostDto, UpdatePostDto},
    users::{model::UserRole, service as user_service},
};
use serde_json::Value;
use sqlx::SqlitePool;
use tower::ServiceExt;

async fn test_app() -> (SqlitePool, Router, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("admin_posts_crud_test.db");
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
async fn unauthenticated_admin_posts_access_is_rejected() {
    let (_pool, app, _dir) = test_app().await;

    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/admin/posts")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn author_creates_post_with_csrf_origin_and_etag() {
    let (pool, app, _dir) = test_app().await;

    user_service::create_user(
        &pool,
        "author1@example.com",
        "Author One",
        "ValidPassword123!",
        UserRole::Author,
    )
    .await
    .unwrap();

    let (access, csrf) = login_user(&app, "author1@example.com", "ValidPassword123!").await;

    let no_origin = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/admin/posts")
                .header(header::COOKIE, format!("blog_access={}", access))
                .header("x-csrf-token", &csrf)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&CreatePostDto {
                        title: "No Origin".into(),
                        summary: None,
                        content_md: "Text".into(),
                        featured_image_media_id: None,
                        status: None,
                        tags: None,
                        scheduled_for: None,
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(no_origin.status(), StatusCode::FORBIDDEN);

    let no_csrf = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/admin/posts")
                .header(header::ORIGIN, "http://localhost:3000")
                .header(header::COOKIE, format!("blog_access={}", access))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&CreatePostDto {
                        title: "No CSRF".into(),
                        summary: None,
                        content_md: "Text".into(),
                        featured_image_media_id: None,
                        status: None,
                        tags: None,
                        scheduled_for: None,
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(no_csrf.status(), StatusCode::FORBIDDEN);

    let valid_create = app
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
                        title: "Novo Artigo Editorial".into(),
                        summary: Some("Resumo do artigo".into()),
                        content_md: "# Editorial\n\nConteúdo com [link](https://ex.com)".into(),
                        featured_image_media_id: None,
                        status: None,
                        tags: Some(vec!["Editorial".into()]),
                        scheduled_for: None,
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(valid_create.status(), StatusCode::CREATED);
    assert_eq!(valid_create.headers().get(header::ETAG).unwrap(), "\"1\"");
    assert!(
        valid_create
            .headers()
            .get(header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("/api/v1/admin/posts/")
    );

    let created_body = json_body(valid_create).await;
    assert_eq!(created_body["title"], "Novo Artigo Editorial");
    assert_eq!(created_body["version"], 1);
}

#[tokio::test]
async fn author_isolation_and_owner_access_privileges() {
    let (pool, app, _dir) = test_app().await;

    user_service::seed_owner(&pool, "owner@example.com", "Owner", "ValidOwnerPass123!")
        .await
        .unwrap();
    user_service::create_user(
        &pool,
        "author_a@example.com",
        "Author A",
        "ValidPassword123!",
        UserRole::Author,
    )
    .await
    .unwrap();
    user_service::create_user(
        &pool,
        "author_b@example.com",
        "Author B",
        "ValidPassword123!",
        UserRole::Author,
    )
    .await
    .unwrap();

    let (access_a, csrf_a) = login_user(&app, "author_a@example.com", "ValidPassword123!").await;
    let (access_b, csrf_b) = login_user(&app, "author_b@example.com", "ValidPassword123!").await;
    let (access_owner, csrf_owner) =
        login_user(&app, "owner@example.com", "ValidOwnerPass123!").await;

    let create_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/admin/posts")
                .header(header::ORIGIN, "http://localhost:3000")
                .header(header::COOKIE, format!("blog_access={}", access_a))
                .header("x-csrf-token", &csrf_a)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&CreatePostDto {
                        title: "Post Author A".into(),
                        summary: None,
                        content_md: "Markdown text".into(),
                        featured_image_media_id: None,
                        status: None,
                        tags: None,
                        scheduled_for: None,
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let post_a = json_body(create_res).await;
    let post_a_id = post_a["id"].as_i64().unwrap();

    let author_b_read = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/admin/posts/{}", post_a_id))
                .header(header::COOKIE, format!("blog_access={}", access_b))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(author_b_read.status(), StatusCode::FORBIDDEN);

    let author_b_edit = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/api/v1/admin/posts/{}", post_a_id))
                .header(header::ORIGIN, "http://localhost:3000")
                .header(header::COOKIE, format!("blog_access={}", access_b))
                .header("x-csrf-token", &csrf_b)
                .header(header::IF_MATCH, "\"1\"")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&UpdatePostDto {
                        title: Some("Hacked by B".into()),
                        summary: None,
                        content_md: None,
                        featured_image_media_id: None,
                        status: None,
                        tags: None,
                        scheduled_for: None,
                        version: 1,
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(author_b_edit.status(), StatusCode::FORBIDDEN);

    let owner_edit = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/api/v1/admin/posts/{}", post_a_id))
                .header(header::ORIGIN, "http://localhost:3000")
                .header(header::COOKIE, format!("blog_access={}", access_owner))
                .header("x-csrf-token", &csrf_owner)
                .header(header::IF_MATCH, "\"1\"")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&UpdatePostDto {
                        title: Some("Edited by Owner".into()),
                        summary: None,
                        content_md: None,
                        featured_image_media_id: None,
                        status: None,
                        tags: None,
                        scheduled_for: None,
                        version: 1,
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(owner_edit.status(), StatusCode::OK);
}

#[tokio::test]
async fn preview_sanitizes_html_and_does_not_persist_data() {
    let (pool, app, _dir) = test_app().await;

    user_service::create_user(
        &pool,
        "author_p@example.com",
        "Author Preview",
        "ValidPassword123!",
        UserRole::Author,
    )
    .await
    .unwrap();

    let (access, csrf) = login_user(&app, "author_p@example.com", "ValidPassword123!").await;

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/admin/posts/preview")
                .header(header::ORIGIN, "http://localhost:3000")
                .header(header::COOKIE, format!("blog_access={}", access))
                .header("x-csrf-token", &csrf)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&PreviewPostDto {
                        content_md:
                            "# Title\n\n[Link](https://rust-lang.org) <script>alert(1)</script>"
                                .into(),
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = json_body(res).await;
    let html = body["content_html"].as_str().unwrap();
    assert!(html.contains("<h1>Title</h1>"));
    assert!(html.contains("rel=\"noopener noreferrer\""));
    assert!(!html.contains("<script>"));
    assert!(!html.contains("alert(1)"));
}
