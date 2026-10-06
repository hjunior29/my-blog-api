use axum::{
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use sqlx::SqlitePool;

use super::{
    cookie::{self, access_cookie_name, refresh_cookie_name},
    dto::{
        CsrfResponse, LoginRequest, LoginResponse, LoginResultResponse, SessionItemResponse,
        TwoFactorChallengeResponse,
    },
    jwt,
    middleware::{AuthenticatedUser, verify_csrf},
    password, session, two_factor,
};
use crate::{
    config::Config,
    email::EmailService,
    http::{error::ApiError, json::Json},
    users::{
        model::{UserProfile, UserRole, UserStatus},
        repository,
        service::{current_unix_time, normalize_email},
    },
};

pub async fn login(
    State(pool): State<SqlitePool>,
    State(config): State<Config>,
    State(rate_limiter): State<super::rate_limit::LoginRateLimiter>,
    client_ip: super::rate_limit::ClientIp,
    headers: HeaderMap,
    Json(payload): Json<LoginRequest>,
) -> Result<Response, ApiError> {
    let now = current_unix_time();
    let ip = super::rate_limit::extract_client_ip(&headers, client_ip.0);

    if let Err(retry_after) = rate_limiter.check_and_record(&ip, now) {
        return Err(ApiError::too_many_requests(Some(retry_after as u64)));
    }

    let normalized = match normalize_email(&payload.email) {
        Ok(e) => e,
        Err(_) => return Err(ApiError::invalid_credentials()),
    };

    let user = match repository::find_by_normalized_email(&pool, &normalized).await {
        Ok(Some(u)) => u,
        Ok(None) => return Err(ApiError::invalid_credentials()),
        Err(_) => return Err(ApiError::internal()),
    };

    if user.status != UserStatus::Active || (config.owner_only && user.role != UserRole::Owner) {
        return Err(ApiError::invalid_credentials());
    }

    let _hash_guard = rate_limiter
        .acquire_active_hash(&ip)
        .map_err(|retry_after| ApiError::too_many_requests(Some(retry_after as u64)))?;

    let is_valid = match password::verify_password(payload.password, user.password_hash.clone()).await {
        Ok(valid) => valid,
        Err(password::PasswordError::Busy) => return Err(ApiError::too_many_requests(Some(5))),
        Err(_) => return Err(ApiError::internal()),
    };

    if !is_valid {
        let failures = rate_limiter.record_account_failure(&normalized, now);
        let delay_secs = failures.clamp(1, 3);
        tokio::time::sleep(std::time::Duration::from_secs(delay_secs as u64)).await;
        return Err(ApiError::invalid_credentials());
    }

    rate_limiter.record_account_success(&normalized);

    let now = current_unix_time();
    let remember_me = payload.remember_me.unwrap_or(false);

    if config.two_factor_enabled {
        let (challenge_token, code) = {
            let mut conn = pool.acquire().await.map_err(|_| ApiError::internal())?;
            match two_factor::create_challenge(&mut conn, user.id, remember_me, now).await {
                Ok(pair) => pair,
                Err(two_factor::TwoFactorError::CooldownActive { retry_after }) => {
                    return Err(ApiError::new(
                        StatusCode::TOO_MANY_REQUESTS,
                        "cooldown_active",
                        "Please wait before requesting a new verification code",
                    )
                    .with_header(
                        header::RETRY_AFTER,
                        header::HeaderValue::from(retry_after as u64),
                    ));
                }
                Err(two_factor::TwoFactorError::Database(_)) => return Err(ApiError::internal()),
            }
        };

        let email_service = EmailService::new(config.clone());
        if let Err(e) = email_service.send_two_factor_code(&user.email, &code).await {
            tracing::error!(error = %e, "failed to send 2FA email");
            let _ = sqlx::query("DELETE FROM auth_two_factor_challenges WHERE id = ?")
                .bind(&challenge_token)
                .execute(&pool)
                .await;
            return Err(ApiError::new(
                StatusCode::BAD_GATEWAY,
                "email_delivery_failed",
                "Failed to deliver verification email",
            ));
        }

        let response_body = LoginResultResponse::TwoFactorRequired(TwoFactorChallengeResponse {
            requires_2fa: true,
            challenge_token,
            email_masked: two_factor::mask_email(&user.email),
        });

        let mut response = (StatusCode::OK, Json(response_body)).into_response();
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("no-store"),
        );
        return Ok(response);
    }

    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok());

    let mut conn = pool.acquire().await.map_err(|_| ApiError::internal())?;
    let (session, raw_refresh_token) =
        session::create_session(&mut conn, user.id, user_agent, Some(&ip), remember_me, now)
            .await
            .map_err(|_| ApiError::internal())?;

    let access_token = jwt::create_access_token(&config, user.id, &session.id, session.expires_at)
        .map_err(|_| ApiError::internal())?;

    let (access_cookie, refresh_cookie) = cookie::build_auth_cookies(
        &config,
        &access_token,
        &raw_refresh_token,
        session.expires_at - now,
    );

    let response_body = LoginResultResponse::Success(LoginResponse {
        user: UserProfile::from(&user),
        csrf_token: session.csrf_token,
    });

    let mut response = (StatusCode::OK, Json(response_body)).into_response();
    response
        .headers_mut()
        .append(header::SET_COOKIE, access_cookie);
    response
        .headers_mut()
        .append(header::SET_COOKIE, refresh_cookie);
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );

    Ok(response)
}

pub async fn csrf(
    State(pool): State<SqlitePool>,
    State(config): State<Config>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let now = current_unix_time();
    let mut active = None;
    if let Some(token) = cookie::extract_cookie(&headers, access_cookie_name(config.secure_cookies))
        && let Ok(claims) = jwt::verify_access_token(&config, &token)
    {
        active = session::find_active_session(&pool, &claims.sid, now)
            .await
            .map_err(|_| ApiError::unavailable())?;
        if active
            .as_ref()
            .is_some_and(|session| claims.sub != session.user_id.to_string())
        {
            return Err(ApiError::unauthorized("Invalid session"));
        }
    }
    if active.is_none()
        && let Some(token) =
            cookie::extract_cookie(&headers, refresh_cookie_name(config.secure_cookies))
    {
        let session_id: Option<String> = sqlx::query_scalar(
            "SELECT session_id FROM refresh_tokens WHERE token_hash = ? AND consumed_at IS NULL AND expires_at > ?",
        ).bind(session::hash_token(&token)).bind(now).fetch_optional(&pool)
            .await.map_err(|_| ApiError::unavailable())?;
        if let Some(id) = session_id {
            active = session::find_active_session(&pool, &id, now)
                .await
                .map_err(|_| ApiError::unavailable())?;
        }
    }
    let active = active.ok_or_else(|| ApiError::unauthorized("No active session found"))?;
    let user = repository::find_by_id(&pool, active.user_id)
        .await
        .map_err(|_| ApiError::unavailable())?
        .ok_or_else(|| ApiError::unauthorized("Invalid session"))?;
    if user.status != UserStatus::Active || (config.owner_only && user.role != UserRole::Owner) {
        return Err(ApiError::unauthorized("Invalid session"));
    }
    let mut response = Json(CsrfResponse {
        csrf_token: active.csrf_token,
    })
    .into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}

pub async fn refresh(
    State(pool): State<SqlitePool>,
    State(config): State<Config>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let refresh_name = refresh_cookie_name(config.secure_cookies);
    let raw_refresh = cookie::extract_cookie(&headers, refresh_name)
        .ok_or_else(|| ApiError::unauthorized("Missing refresh token"))?;

    let csrf_header = headers.get("x-csrf-token").and_then(|h| h.to_str().ok());
    let now = current_unix_time();
    let (session, new_raw_token) =
        match session::rotate_refresh_token(&pool, &raw_refresh, csrf_header, now).await {
            Ok(res) => res,
            Err(session::SessionError::InvalidCsrf) => return Err(ApiError::invalid_csrf_token()),
            Err(session::SessionError::ReplayDetected) => {
                let (clear_access, clear_refresh) = cookie::build_clear_auth_cookies(&config);
                let mut response = ApiError::unauthorized("Invalid refresh token").into_response();
                response
                    .headers_mut()
                    .append(header::SET_COOKIE, clear_access);
                response
                    .headers_mut()
                    .append(header::SET_COOKIE, clear_refresh);
                return Ok(response);
            }
            Err(session::SessionError::Database(_)) => return Err(ApiError::unavailable()),
            Err(_) => return Err(ApiError::unauthorized("Invalid refresh token")),
        };

    let user = repository::find_by_id(&pool, session.user_id)
        .await
        .map_err(|_| ApiError::internal())?
        .ok_or_else(|| ApiError::unauthorized("User not found"))?;

    if user.status != UserStatus::Active || (config.owner_only && user.role != UserRole::Owner) {
        return Err(ApiError::unauthorized("User inactive"));
    }

    let access_token = jwt::create_access_token(&config, user.id, &session.id, session.expires_at)
        .map_err(|_| ApiError::internal())?;

    let (access_cookie, refresh_cookie) = cookie::build_auth_cookies(
        &config,
        &access_token,
        &new_raw_token,
        session.expires_at - now,
    );

    let mut response = (
        StatusCode::OK,
        Json(CsrfResponse {
            csrf_token: session.csrf_token,
        }),
    )
        .into_response();
    response
        .headers_mut()
        .append(header::SET_COOKIE, access_cookie);
    response
        .headers_mut()
        .append(header::SET_COOKIE, refresh_cookie);
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );

    Ok(response)
}

pub async fn logout(
    State(pool): State<SqlitePool>,
    State(config): State<Config>,
    auth: Result<AuthenticatedUser, ApiError>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let now = current_unix_time();
    let (clear_access, clear_refresh) = cookie::build_clear_auth_cookies(&config);

    if let Ok(auth_user) = auth {
        if let Err(csrf_err) = verify_csrf(&headers, &auth_user.session.csrf_token) {
            let mut response = csrf_err.into_response();
            response.headers_mut().append(header::SET_COOKIE, clear_access);
            response.headers_mut().append(header::SET_COOKIE, clear_refresh);
            return Ok(response);
        }
        session::revoke_session(&pool, &auth_user.session.id, now)
            .await
            .map_err(|_| ApiError::unavailable())?;
    } else {
        let refresh_name = refresh_cookie_name(config.secure_cookies);
        if let Some(raw_refresh) = cookie::extract_cookie(&headers, refresh_name) {
            let token_hash = session::hash_token(&raw_refresh);
            let record = sqlx::query_as::<_, session::RefreshTokenRow>(
                "SELECT token_hash, session_id, created_at, expires_at, consumed_at, replaced_by_hash
                 FROM refresh_tokens WHERE token_hash = ?",
            )
            .bind(&token_hash)
            .fetch_optional(&pool)
            .await
            .map_err(|_| ApiError::unavailable())?;

            if let Some(record) = record
                && let Some(s) = session::find_active_session(&pool, &record.session_id, now)
                    .await
                    .map_err(|_| ApiError::unavailable())?
            {
                if let Err(csrf_err) = verify_csrf(&headers, &s.csrf_token) {
                    let mut response = csrf_err.into_response();
                    response.headers_mut().append(header::SET_COOKIE, clear_access);
                    response.headers_mut().append(header::SET_COOKIE, clear_refresh);
                    return Ok(response);
                }
                session::revoke_session(&pool, &record.session_id, now)
                    .await
                    .map_err(|_| ApiError::unavailable())?;
            }
        }
    }

    let mut response = StatusCode::NO_CONTENT.into_response();
    response
        .headers_mut()
        .append(header::SET_COOKIE, clear_access);
    response
        .headers_mut()
        .append(header::SET_COOKIE, clear_refresh);
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );

    Ok(response)
}

pub async fn logout_all(
    State(pool): State<SqlitePool>,
    State(config): State<Config>,
    auth: AuthenticatedUser,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let (clear_access, clear_refresh) = cookie::build_clear_auth_cookies(&config);
    if let Err(csrf_err) = verify_csrf(&headers, &auth.session.csrf_token) {
        let mut response = csrf_err.into_response();
        response.headers_mut().append(header::SET_COOKIE, clear_access);
        response.headers_mut().append(header::SET_COOKIE, clear_refresh);
        return Ok(response);
    }
    let now = current_unix_time();

    session::revoke_all_user_sessions(&pool, auth.user.id, now)
        .await
        .map_err(|_| ApiError::unavailable())?;

    let mut response = StatusCode::NO_CONTENT.into_response();
    response
        .headers_mut()
        .append(header::SET_COOKIE, clear_access);
    response
        .headers_mut()
        .append(header::SET_COOKIE, clear_refresh);
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );

    Ok(response)
}

pub async fn list_sessions(
    State(pool): State<SqlitePool>,
    auth: AuthenticatedUser,
) -> Result<Response, ApiError> {
    let now = current_unix_time();
    let sessions = session::list_user_sessions(&pool, auth.user.id, now)
        .await
        .map_err(|_| ApiError::internal())?;

    let items: Vec<SessionItemResponse> = sessions
        .into_iter()
        .map(|s| SessionItemResponse {
            is_current: s.id == auth.session.id,
            id: s.id,
            user_agent: s.user_agent,
            ip_address: s.ip_address,
            created_at: s.created_at,
            expires_at: s.expires_at,
        })
        .collect();

    let mut response = (StatusCode::OK, Json(items)).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );

    Ok(response)
}

pub async fn delete_session(
    State(pool): State<SqlitePool>,
    State(config): State<Config>,
    auth: AuthenticatedUser,
    headers: HeaderMap,
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> Result<Response, ApiError> {
    verify_csrf(&headers, &auth.session.csrf_token)?;
    let now = current_unix_time();

    let was_revoked = session::revoke_user_session(&pool, auth.user.id, &session_id, now)
        .await
        .map_err(|_| ApiError::internal())?;

    if !was_revoked {
        return Err(ApiError::not_found("Session not found"));
    }

    let mut response = StatusCode::NO_CONTENT.into_response();
    if session_id == auth.session.id {
        let (clear_access, clear_refresh) = cookie::build_clear_auth_cookies(&config);
        response
            .headers_mut()
            .append(header::SET_COOKIE, clear_access);
        response
            .headers_mut()
            .append(header::SET_COOKIE, clear_refresh);
    }

    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );

    Ok(response)
}
