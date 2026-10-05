use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, Response, StatusCode, header},
};
use my_blog_api::{
    config::Config,
    database,
    http::router_with_config,
    posts::{PostStatus, dto::CreatePostDto, service as post_service},
    users::{model::UserRole, service as user_service},
};
use serde_json::Value;
use sqlx::SqlitePool;
use tower::ServiceExt;

async fn test_app() -> (SqlitePool, Router, i64, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("public_posts_test.db");
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

    let author_id = user_service::create_user(
        &pool,
        "author@example.com",
        "Author",
        "SecurePassword123!",
        UserRole::Author,
    )
    .await
    .unwrap();

    let app = router_with_config(pool.clone(), config);
    (pool, app, author_id, dir)
}

async fn json_body(response: Response<Body>) -> Value {
    let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn list_posts_only_returns_published_items_with_cache_header() {
    let (pool, app, author_id, _dir) = test_app().await;

    post_service::create_post(
        &pool,
        author_id,
        CreatePostDto {
            title: "Draft Post".to_string(),
            summary: Some("Draft summary".to_string()),
            content_md: "Draft body".to_string(),
            featured_image_media_id: None,
            status: Some(PostStatus::Draft),
            tags: Some(vec!["DraftTag".to_string()]),
            scheduled_for: None,
            book_color: None,
        },
    )
    .await
    .unwrap();

    post_service::create_post(
        &pool,
        author_id,
        CreatePostDto {
            title: "Published Post 1".to_string(),
            summary: Some("Summary 1".to_string()),
            content_md: "Body 1".to_string(),
            featured_image_media_id: None,
            status: Some(PostStatus::Published),
            tags: Some(vec!["Rust".to_string()]),
            scheduled_for: None,
            book_color: None,
        },
    )
    .await
    .unwrap();

    let request = Request::builder()
        .uri("/api/v1/posts")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CACHE_CONTROL).unwrap(),
        "no-store"
    );

    let body = json_body(response).await;
    assert_eq!(body["total"], 1);
    assert_eq!(body["items"].as_array().unwrap().len(), 1);
    assert_eq!(body["items"][0]["title"], "Published Post 1");
    assert_eq!(body["items"][0]["tags"][0]["name"], "Rust");
}

#[tokio::test]
async fn get_post_by_slug_returns_published_post_and_404_for_draft() {
    let (pool, app, author_id, _dir) = test_app().await;

    let draft = post_service::create_post(
        &pool,
        author_id,
        CreatePostDto {
            title: "Secret Draft".to_string(),
            summary: None,
            content_md: "Secret text".to_string(),
            featured_image_media_id: None,
            status: Some(PostStatus::Draft),
            tags: None,
            scheduled_for: None,
            book_color: None,
        },
    )
    .await
    .unwrap();

    let published = post_service::create_post(
        &pool,
        author_id,
        CreatePostDto {
            title: "Public Article".to_string(),
            summary: Some("Public summary".to_string()),
            content_md: "# Heading\n\nPublic [link](https://example.com).".to_string(),
            featured_image_media_id: None,
            status: Some(PostStatus::Published),
            tags: Some(vec!["Public".to_string()]),
            scheduled_for: None,
            book_color: None,
        },
    )
    .await
    .unwrap();

    let req_draft = Request::builder()
        .uri(format!("/api/v1/posts/{}", draft.post.slug))
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let res_draft = app.clone().oneshot(req_draft).await.unwrap();
    assert_eq!(res_draft.status(), StatusCode::NOT_FOUND);

    let req_pub = Request::builder()
        .uri(format!("/api/v1/posts/{}", published.post.slug))
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let res_pub = app.clone().oneshot(req_pub).await.unwrap();
    assert_eq!(res_pub.status(), StatusCode::OK);
    let pub_body = json_body(res_pub).await;
    assert_eq!(pub_body["title"], "Public Article");
    assert!(
        pub_body["content_html"]
            .as_str()
            .unwrap()
            .contains("<h1>Heading</h1>")
    );
    assert_eq!(pub_body["tags"][0]["name"], "Public");

    let req_nonexistent = Request::builder()
        .uri("/api/v1/posts/non-existent-slug")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let res_nonexistent = app.oneshot(req_nonexistent).await.unwrap();
    assert_eq!(res_nonexistent.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn search_posts_finds_published_content_and_safely_escapes_query() {
    let (pool, app, author_id, _dir) = test_app().await;

    post_service::create_post(
        &pool,
        author_id,
        CreatePostDto {
            title: "Deep Dive into Axum Web Framework".to_string(),
            summary: Some("Complete guide to routing and handlers".to_string()),
            content_md: "Axum is built on top of Tower and Hyper.".to_string(),
            featured_image_media_id: None,
            status: Some(PostStatus::Published),
            tags: Some(vec!["Rust".to_string(), "Axum".to_string()]),
            scheduled_for: None,
            book_color: None,
        },
    )
    .await
    .unwrap();

    let req = Request::builder()
        .uri("/api/v1/posts/search?q=framework")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = json_body(res).await;
    assert_eq!(body["total"], 1);
    assert_eq!(
        body["items"][0]["title"],
        "Deep Dive into Axum Web Framework"
    );

    let symbol_req = Request::builder()
        .uri("/api/v1/posts/search?q=%22%27%28%29---%2A%2A%2Aaxum%2A%2A%2A%29%29")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let symbol_res = app.oneshot(symbol_req).await.unwrap();
    assert_eq!(symbol_res.status(), StatusCode::OK);
    let symbol_body = json_body(symbol_res).await;
    assert_eq!(symbol_body["total"], 1);
}

#[tokio::test]
async fn list_tags_only_returns_tags_from_published_posts() {
    let (pool, app, author_id, _dir) = test_app().await;

    post_service::create_post(
        &pool,
        author_id,
        CreatePostDto {
            title: "Draft".to_string(),
            summary: None,
            content_md: "Body".to_string(),
            featured_image_media_id: None,
            status: Some(PostStatus::Draft),
            tags: Some(vec!["OnlyDraftTag".to_string()]),
            scheduled_for: None,
            book_color: None,
        },
    )
    .await
    .unwrap();

    post_service::create_post(
        &pool,
        author_id,
        CreatePostDto {
            title: "Published".to_string(),
            summary: None,
            content_md: "Body".to_string(),
            featured_image_media_id: None,
            status: Some(PostStatus::Published),
            tags: Some(vec!["PublicTag".to_string()]),
            scheduled_for: None,
            book_color: None,
        },
    )
    .await
    .unwrap();

    let req = Request::builder()
        .uri("/api/v1/tags")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = json_body(res).await;
    let tags = body.as_array().unwrap();
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0]["name"], "PublicTag");
    assert_eq!(tags[0]["post_count"], 1);
}

#[tokio::test]
async fn search_posts_rejects_query_exceeding_100_characters() {
    let (_pool, app, _author_id, _dir) = test_app().await;

    let long_query = "a".repeat(101);
    let req = Request::builder()
        .uri(format!("/api/v1/posts/search?q={}", long_query))
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let body = json_body(res).await;
    assert_eq!(body["error"]["code"], "invalid_search_query");
}
