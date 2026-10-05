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
async fn list_and_delete_active_sessions() {
    let (pool, _, app, _dir) = test_app().await;
    user_service::seed_owner(&pool, "owner@example.com", "Owner", "ValidOwnerPass123!")
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
                .header(header::USER_AGENT, "Device-A")
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
                .header(header::USER_AGENT, "Device-B")
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

    let cookies2 = extract_set_cookies(&login2);
    let body2 = json_body(login2).await;
    let access2 = cookies2.get("blog_access").unwrap();
    let csrf2 = body2["csrf_token"].as_str().unwrap();

    let list_req = Request::builder()
        .method("GET")
        .uri("/api/v1/auth/sessions")
        .header(header::COOKIE, format!("blog_access={access2}"))
        .body(Body::empty())
        .unwrap();

    let list_res = app.clone().oneshot(list_req).await.unwrap();
    assert_eq!(list_res.status(), StatusCode::OK);
    let sessions_val = json_body(list_res).await;
    let sessions_arr = sessions_val.as_array().unwrap();
    assert_eq!(sessions_arr.len(), 2);

    let current_session = sessions_arr
        .iter()
        .find(|s| s["is_current"] == true)
        .unwrap();
    let other_session = sessions_arr
        .iter()
        .find(|s| s["is_current"] == false)
        .unwrap();
    let other_id = other_session["id"].as_str().unwrap();
    let current_id = current_session["id"].as_str().unwrap();

    assert!(current_session.get("csrf_token").is_none());
    assert!(other_session.get("csrf_token").is_none());

    let del_other_req = Request::builder()
        .method("DELETE")
        .uri(format!("/api/v1/auth/sessions/{other_id}"))
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::COOKIE, format!("blog_access={access2}"))
        .header("x-csrf-token", csrf2)
        .body(Body::empty())
        .unwrap();

    let del_other_res = app.clone().oneshot(del_other_req).await.unwrap();
    assert_eq!(del_other_res.status(), StatusCode::NO_CONTENT);

    let refresh_deleted_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/refresh")
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::COOKIE, format!("blog_refresh={refresh1}"))
        .header("x-csrf-token", csrf2)
        .body(Body::empty())
        .unwrap();

    let refresh_deleted_res = app.clone().oneshot(refresh_deleted_req).await.unwrap();
    assert_eq!(refresh_deleted_res.status(), StatusCode::UNAUTHORIZED);

    let del_current_req = Request::builder()
        .method("DELETE")
        .uri(format!("/api/v1/auth/sessions/{current_id}"))
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::COOKIE, format!("blog_access={access2}"))
        .header("x-csrf-token", csrf2)
        .body(Body::empty())
        .unwrap();

    let del_current_res = app.oneshot(del_current_req).await.unwrap();
    assert_eq!(del_current_res.status(), StatusCode::NO_CONTENT);
    let clear_cookies = extract_set_cookies(&del_current_res);
    assert_eq!(
        clear_cookies.get("blog_access").map(String::as_str),
        Some("")
    );
    assert_eq!(
        clear_cookies.get("blog_refresh").map(String::as_str),
        Some("")
    );
}
