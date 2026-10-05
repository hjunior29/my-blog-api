use std::collections::HashMap;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, Response, StatusCode, header},
};
use my_blog_api::{
    auth::dto::{LoginRequest, ResendTwoFactorRequest, VerifyTwoFactorRequest},
    config::Config,
    database,
    http::router_with_config,
    users::service as user_service,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
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
        "TWO_FACTOR_ENABLED" => Ok(Some("true".into())),
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

fn hash_code(code: &str) -> String {
    use std::fmt::Write;
    let hash = Sha256::digest(code.as_bytes());
    let mut hex = String::with_capacity(64);
    for b in hash {
        let _ = write!(&mut hex, "{:02x}", b);
    }
    hex
}

#[tokio::test]
async fn two_factor_login_flow_and_verification() {
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

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let cookies = extract_set_cookies(&res);
    assert!(!cookies.contains_key("blog_access"));
    assert!(!cookies.contains_key("blog_refresh"));

    let body = json_body(res).await;
    assert_eq!(body["requires_2fa"], true);
    assert_eq!(body["email_masked"], "o***r@example.com");

    let challenge_token = body["challenge_token"].as_str().unwrap().to_string();

    let wrong_payload = serde_json::to_vec(&VerifyTwoFactorRequest {
        challenge_token: challenge_token.clone(),
        code: "000000".into(),
        remember_me: None,
    })
    .unwrap();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/2fa/verify")
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(wrong_payload))
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let body = json_body(res).await;
    assert_eq!(body["error"]["code"], "invalid_code");

    let resend_payload = serde_json::to_vec(&ResendTwoFactorRequest {
        challenge_token: challenge_token.clone(),
    })
    .unwrap();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/2fa/resend")
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(resend_payload))
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
    let body = json_body(res).await;
    assert_eq!(body["error"]["code"], "cooldown_active");

    let known_code = "654321";
    let known_hash = hash_code(known_code);
    sqlx::query("UPDATE auth_two_factor_challenges SET code_hash = ? WHERE id = ?")
        .bind(&known_hash)
        .bind(&challenge_token)
        .execute(&pool)
        .await
        .unwrap();

    let valid_payload = serde_json::to_vec(&VerifyTwoFactorRequest {
        challenge_token: challenge_token.clone(),
        code: known_code.into(),
        remember_me: None,
    })
    .unwrap();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/2fa/verify")
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(valid_payload))
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let cookies = extract_set_cookies(&res);
    assert!(cookies.contains_key("blog_access"));
    assert!(cookies.contains_key("blog_refresh"));

    let body = json_body(res).await;
    assert_eq!(body["user"]["email"], "owner@example.com");

    let replay_payload = serde_json::to_vec(&VerifyTwoFactorRequest {
        challenge_token: challenge_token.clone(),
        code: known_code.into(),
        remember_me: None,
    })
    .unwrap();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/2fa/verify")
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(replay_payload))
        .unwrap();

    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let body = json_body(res).await;
    assert_eq!(body["error"]["code"], "invalid_challenge");
}

#[tokio::test]
async fn two_factor_lockout_after_max_attempts() {
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

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = json_body(res).await;
    let challenge_token = body["challenge_token"].as_str().unwrap().to_string();

    for _ in 0..4 {
        let wrong_payload = serde_json::to_vec(&VerifyTwoFactorRequest {
            challenge_token: challenge_token.clone(),
            code: "999999".into(),
            remember_me: None,
        })
        .unwrap();

        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/auth/2fa/verify")
            .header(header::ORIGIN, "http://localhost:3000")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(wrong_payload))
            .unwrap();

        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let body = json_body(res).await;
        assert_eq!(body["error"]["code"], "invalid_code");
    }

    let fifth_payload = serde_json::to_vec(&VerifyTwoFactorRequest {
        challenge_token: challenge_token.clone(),
        code: "999999".into(),
        remember_me: None,
    })
    .unwrap();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/2fa/verify")
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(fifth_payload))
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    let sixth_payload = serde_json::to_vec(&VerifyTwoFactorRequest {
        challenge_token: challenge_token.clone(),
        code: "999999".into(),
        remember_me: None,
    })
    .unwrap();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/2fa/verify")
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(sixth_payload))
        .unwrap();

    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let body = json_body(res).await;
    assert_eq!(body["error"]["code"], "too_many_attempts");
}

#[tokio::test]
async fn two_factor_login_cooldown_prevents_duplicate_challenges() {
    let (pool, _, app, _dir) = test_app().await;
    user_service::seed_owner(&pool, "cooldown@example.com", "Owner", "ValidOwnerPass123!")
        .await
        .unwrap();

    let payload = serde_json::to_vec(&LoginRequest {
        email: "cooldown@example.com".into(),
        password: "ValidOwnerPass123!".into(),
        remember_me: None,
    })
    .unwrap();

    let req1 = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload.clone()))
        .unwrap();

    let res1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(res1.status(), StatusCode::OK);

    let req2 = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload))
        .unwrap();

    let res2 = app.oneshot(req2).await.unwrap();
    assert_eq!(res2.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(res2.headers().contains_key(header::RETRY_AFTER));
    let body = json_body(res2).await;
    assert_eq!(body["error"]["code"], "cooldown_active");
}

#[tokio::test]
async fn two_factor_locked_challenge_still_enforces_cooldown() {
    let (pool, _, app, _dir) = test_app().await;
    user_service::seed_owner(&pool, "locked_cooldown@example.com", "Owner", "ValidOwnerPass123!")
        .await
        .unwrap();

    let payload = serde_json::to_vec(&LoginRequest {
        email: "locked_cooldown@example.com".into(),
        password: "ValidOwnerPass123!".into(),
        remember_me: None,
    })
    .unwrap();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload.clone()))
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = json_body(res).await;
    let challenge_token = body["challenge_token"].as_str().unwrap().to_string();

    for _ in 0..5 {
        let wrong_payload = serde_json::to_vec(&VerifyTwoFactorRequest {
            challenge_token: challenge_token.clone(),
            code: "000000".into(),
            remember_me: None,
        })
        .unwrap();

        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/auth/2fa/verify")
            .header(header::ORIGIN, "http://localhost:3000")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(wrong_payload))
            .unwrap();

        let _ = app.clone().oneshot(req).await.unwrap();
    }

    let req_after_lockout = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload))
        .unwrap();

    let res_locked = app.oneshot(req_after_lockout).await.unwrap();
    assert_eq!(res_locked.status(), StatusCode::TOO_MANY_REQUESTS);
    let body = json_body(res_locked).await;
    assert_eq!(body["error"]["code"], "cooldown_active");
}
