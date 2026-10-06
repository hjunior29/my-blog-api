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
    users::service as user_service,
};
use serde_json::Value;
use sqlx::SqlitePool;
use tower::ServiceExt;

async fn test_app() -> (SqlitePool, Router, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("media_test.db");
    let url = format!("sqlite://{}", db_path.display());
    let pool = database::connect(&url, 4).await.unwrap();

    let uploads_dir = dir.path().join("uploads");
    std::fs::create_dir_all(&uploads_dir).unwrap();
    let uploads_str = uploads_dir.to_str().unwrap().to_string();

    let config = Config::from_lookup(move |key| match key {
        "OWNER_ONLY" => Ok(Some("false".into())),
        "APP_ENV" => Ok(Some("test".into())),
        "APP_ORIGIN" => Ok(Some("http://localhost:3000".into())),
        "SECURE_COOKIES" => Ok(Some("false".into())),
        "MEDIA_ROOT" => Ok(Some(uploads_str.clone())),
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
                .header(header::COOKIE, format!("blog_access={access_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = json_body(csrf_res).await;
    let csrf_token = body["csrf_token"].as_str().unwrap().to_string();

    (access_token, csrf_token)
}

fn create_multipart_body(
    boundary: &str,
    field_name: &str,
    filename: &str,
    content_type: &str,
    data: &[u8],
) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"{field_name}\"; filename=\"{filename}\"\r\n").as_bytes(),
    );
    body.extend_from_slice(format!("Content-Type: {content_type}\r\n\r\n").as_bytes());
    body.extend_from_slice(data);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

#[tokio::test]
async fn unauthenticated_media_upload_is_rejected() {
    let (_pool, app, _dir) = test_app().await;
    let boundary = "boundary777";
    let body = create_multipart_body(boundary, "file", "photo.png", "image/png", b"fake png");

    let req = Request::post("/api/v1/admin/media")
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .header(header::ORIGIN, "http://localhost:3000")
        .body(Body::from(body))
        .unwrap();

    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn upload_image_video_and_audio_succeeds() {
    let (pool, app, _dir) = test_app().await;

    user_service::seed_owner(&pool, "admin@blog.local", "Admin", "Admin Pass 1234!")
        .await
        .unwrap();

    let (access, csrf) = login_user(&app, "admin@blog.local", "Admin Pass 1234!").await;

    let boundary = "boundary123";
    let img_body = create_multipart_body(boundary, "file", "cover.png", "image/png", b"\x89PNG\r\n\x1a\nfakeimage");
    let req = Request::post("/api/v1/admin/media")
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .header(header::COOKIE, format!("blog_access={access}"))
        .header("x-csrf-token", &csrf)
        .header(header::ORIGIN, "http://localhost:3000")
        .body(Body::from(img_body))
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let body = json_body(res).await;
    let image_id = body["id"].as_str().unwrap().to_string();
    assert_eq!(body["media_kind"], "image");
    assert_eq!(body["content_type"], "image/png");
    assert_eq!(body["filename"], "cover.png");

    let get_req = Request::get(format!("/api/v1/media/{image_id}"))
        .body(Body::empty())
        .unwrap();
    let get_res = app.clone().oneshot(get_req).await.unwrap();
    assert_eq!(get_res.status(), StatusCode::OK);
    assert_eq!(
        get_res.headers().get(header::CONTENT_TYPE).unwrap(),
        "image/png"
    );
    let get_bytes = to_bytes(get_res.into_body(), 64 * 1024).await.unwrap();
    assert_eq!(&get_bytes[..], b"\x89PNG\r\n\x1a\nfakeimage");

    let get_with_name_req = Request::get(format!("/api/v1/media/{image_id}/cover.png"))
        .body(Body::empty())
        .unwrap();
    let get_with_name_res = app.clone().oneshot(get_with_name_req).await.unwrap();
    assert_eq!(get_with_name_res.status(), StatusCode::OK);
    assert_eq!(
        get_with_name_res.headers().get(header::CONTENT_TYPE).unwrap(),
        "image/png"
    );

    let vid_body = create_multipart_body(
        boundary,
        "file",
        "clip.mp4",
        "video/mp4",
        b"\x00\x00\x00\x18ftypmp42\x00\x00\x00\x00fake mp4 video bytes",
    );
    let vid_req = Request::post("/api/v1/admin/media")
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .header(header::COOKIE, format!("blog_access={access}"))
        .header("x-csrf-token", &csrf)
        .header(header::ORIGIN, "http://localhost:3000")
        .body(Body::from(vid_body))
        .unwrap();

    let vid_res = app.clone().oneshot(vid_req).await.unwrap();
    assert_eq!(vid_res.status(), StatusCode::CREATED);
    let vid_json = json_body(vid_res).await;
    assert_eq!(vid_json["media_kind"], "video");
    assert_eq!(vid_json["content_type"], "video/mp4");

    let audio_body = create_multipart_body(
        boundary,
        "file",
        "podcast.mp3",
        "audio/mpeg",
        b"ID3\x03\x00\x00\x00\x00\x00\x00fake mp3 audio bytes",
    );
    let audio_req = Request::post("/api/v1/admin/media")
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .header(header::COOKIE, format!("blog_access={access}"))
        .header("x-csrf-token", &csrf)
        .header(header::ORIGIN, "http://localhost:3000")
        .body(Body::from(audio_body))
        .unwrap();

    let audio_res = app.clone().oneshot(audio_req).await.unwrap();
    assert_eq!(audio_res.status(), StatusCode::CREATED);
    let audio_json = json_body(audio_res).await;
    assert_eq!(audio_json["media_kind"], "audio");
    assert_eq!(audio_json["content_type"], "audio/mpeg");

    let list_req = Request::get("/api/v1/admin/media")
        .header(header::COOKIE, format!("blog_access={access}"))
        .body(Body::empty())
        .unwrap();
    let list_res = app.clone().oneshot(list_req).await.unwrap();
    assert_eq!(list_res.status(), StatusCode::OK);
    let list_json = json_body(list_res).await;
    assert_eq!(list_json["total"], 3);

    let del_req = Request::delete(format!("/api/v1/admin/media/{image_id}"))
        .header(header::COOKIE, format!("blog_access={access}"))
        .header("x-csrf-token", &csrf)
        .header(header::ORIGIN, "http://localhost:3000")
        .body(Body::empty())
        .unwrap();
    let del_res = app.clone().oneshot(del_req).await.unwrap();
    assert_eq!(del_res.status(), StatusCode::NO_CONTENT);

    let check_del = Request::get(format!("/api/v1/media/{image_id}"))
        .body(Body::empty())
        .unwrap();
    let check_res = app.clone().oneshot(check_del).await.unwrap();
    assert_eq!(check_res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn rejects_unsupported_media_formats() {
    let (pool, app, _dir) = test_app().await;

    user_service::seed_owner(&pool, "owner@blog.local", "Owner", "Owner Pass 1234!")
        .await
        .unwrap();

    let (access, csrf) = login_user(&app, "owner@blog.local", "Owner Pass 1234!").await;

    let boundary = "boundary999";
    let exe_body = create_multipart_body(boundary, "file", "malware.exe", "application/x-msdownload", b"MZbadcode");
    let req = Request::post("/api/v1/admin/media")
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .header(header::COOKIE, format!("blog_access={access}"))
        .header("x-csrf-token", &csrf)
        .header(header::ORIGIN, "http://localhost:3000")
        .body(Body::from(exe_body))
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

    let spoofed_mp4_body = create_multipart_body(boundary, "file", "fake.mp4", "video/mp4", b"not a real mp4 video");
    let spoofed_mp4_req = Request::post("/api/v1/admin/media")
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .header(header::COOKIE, format!("blog_access={access}"))
        .header("x-csrf-token", &csrf)
        .header(header::ORIGIN, "http://localhost:3000")
        .body(Body::from(spoofed_mp4_body))
        .unwrap();

    let spoofed_mp4_res = app.oneshot(spoofed_mp4_req).await.unwrap();
    assert_eq!(spoofed_mp4_res.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[tokio::test]
async fn tigris_storage_redirects_media_downloads() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("tigris_media_test.db");
    let url = format!("sqlite://{}", db_path.display());
    let pool = database::connect(&url, 4).await.unwrap();

    let config = Config::from_lookup(|key| match key {
        "OWNER_ONLY" => Ok(Some("false".into())),
        "APP_ENV" => Ok(Some("test".into())),
        "APP_ORIGIN" => Ok(Some("http://localhost:3000".into())),
        "SECURE_COOKIES" => Ok(Some("false".into())),
        "MEDIA_ROOT" => Ok(Some("./uploads".into())),
        "JWT_SECRET" => Ok(Some("test-jwt-secret-key-must-be-at-least-32-bytes!".into())),
        "BUCKET_NAME" => Ok(Some("blog-assets".into())),
        "AWS_ACCESS_KEY_ID" => Ok(Some("mock-access-key".into())),
        "AWS_SECRET_ACCESS_KEY" => Ok(Some("mock-secret-key".into())),
        _ => Ok(None),
    })
    .unwrap();

    let uploader_id = match user_service::seed_owner(&pool, "owner@blog.local", "Owner", "Owner Pass 1234!")
        .await
        .unwrap()
    {
        user_service::SeedResult::Created(id) | user_service::SeedResult::AlreadyExists(id) => id,
    };

    let now = 1700000000i64;
    sqlx::query(
        "INSERT INTO media (id, filename, content_type, media_kind, size_bytes, storage_key, storage_backend, public_url, uploader_id, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind("tigris-vid-123")
    .bind("video.mp4")
    .bind("video/mp4")
    .bind("video")
    .bind(50_000_000i64)
    .bind("tigris-vid-123/video.mp4")
    .bind("tigris")
    .bind("https://fly.storage.tigris.dev/blog-assets/tigris-vid-123/video.mp4")
    .bind(uploader_id)
    .bind(now)
    .execute(&pool)
    .await
    .unwrap();

    let app = router_with_config(pool, config);
    let req = Request::get("/api/v1/media/tigris-vid-123")
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(
        res.headers().get(header::LOCATION).unwrap(),
        "https://fly.storage.tigris.dev/blog-assets/tigris-vid-123/video.mp4"
    );
    assert_eq!(
        res.headers().get(header::X_CONTENT_TYPE_OPTIONS).unwrap(),
        "nosniff"
    );
    assert_eq!(
        res.headers().get("content-security-policy").unwrap(),
        "default-src 'none'; sandbox"
    );
}
