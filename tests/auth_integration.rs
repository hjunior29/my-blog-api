use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use my_blog_api::{
    auth::{jwt, session},
    config::Config,
    database,
    http::router_with_config,
    users::{model::UserRole, service},
};
use tower::ServiceExt;

#[tokio::test]
async fn owner_only_rejects_author_login_and_existing_author_sessions() {
    let pool = database::connect("sqlite::memory:", 1).await.unwrap();
    let config = Config::from_lookup(|_| Ok(None)).unwrap();
    assert!(config.owner_only);
    let password = "Fictional integration password 123";
    let author = service::create_user(
        &pool,
        "author@example.test",
        "Author",
        password,
        UserRole::Author,
    )
    .await
    .unwrap();
    let app = router_with_config(pool.clone(), config.clone());
    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/auth/login")
                .header(header::ORIGIN, &config.app_origin)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({"email":"author@example.test","password":password})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let now = service::current_unix_time();
    let mut conn = pool.acquire().await.unwrap();
    let (session, _) = session::create_session(&mut conn, author, None, None, now)
        .await
        .unwrap();
    drop(conn);
    let token = jwt::create_access_token(&config, author, &session.id, session.expires_at).unwrap();
    for path in ["/api/v1/users/me", "/api/v1/admin/posts"] {
        let response = app
            .clone()
            .oneshot(
                Request::get(path)
                    .header(header::COOKIE, format!("blog_access={token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    pool.close().await;
}

#[tokio::test]
async fn database_outages_do_not_claim_refresh_or_logout_succeeded() {
    let pool = database::connect("sqlite::memory:", 1).await.unwrap();
    let config = Config::from_lookup(|_| Ok(None)).unwrap();
    let origin = config.app_origin.clone();
    let app = router_with_config(pool.clone(), config);
    pool.close().await;
    for path in ["/api/v1/auth/refresh", "/api/v1/auth/logout"] {
        let response = app
            .clone()
            .oneshot(
                Request::post(path)
                    .header(header::ORIGIN, &origin)
                    .header(header::COOKIE, "blog_refresh=unavailable-test-token")
                    .header("x-csrf-token", "test-csrf")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(!response.headers().contains_key(header::SET_COOKIE));
    }
}
