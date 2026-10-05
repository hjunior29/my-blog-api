use axum::{
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use sqlx::SqlitePool;

use super::{
    cookie,
    dto::{
        LoginResponse, ResendTwoFactorRequest, ResendTwoFactorResponse, VerifyTwoFactorRequest,
    },
    jwt, session,
    two_factor::{self, TwoFactorResendError, TwoFactorVerifyError},
};
use crate::{
    config::Config,
    email::EmailService,
    http::{error::ApiError, json::Json},
    users::{
        model::{UserProfile, UserRole, UserStatus},
        repository,
        service::current_unix_time,
    },
};

pub async fn verify_two_factor(
    State(pool): State<SqlitePool>,
    State(config): State<Config>,
    headers: HeaderMap,
    Json(payload): Json<VerifyTwoFactorRequest>,
) -> Result<Response, ApiError> {
    let now = current_unix_time();
    let mut conn = pool.acquire().await.map_err(|_| ApiError::internal())?;

    let verify_res = match two_factor::verify_challenge(
        &mut conn,
        &payload.challenge_token,
        &payload.code,
        now,
    )
    .await
    {
        Ok(res) => res,
        Err(TwoFactorVerifyError::InvalidChallenge) => {
            return Err(ApiError::bad_request(
                "invalid_challenge",
                "Challenge is invalid or has expired",
            ));
        }
        Err(TwoFactorVerifyError::Expired) => {
            return Err(ApiError::bad_request(
                "challenge_expired",
                "Verification code has expired. Please login again.",
            ));
        }
        Err(TwoFactorVerifyError::TooManyAttempts) => {
            return Err(ApiError::bad_request(
                "too_many_attempts",
                "Maximum verification attempts exceeded. Please login again.",
            ));
        }
        Err(TwoFactorVerifyError::InvalidCode { remaining_attempts }) => {
            return Err(ApiError::bad_request(
                "invalid_code",
                if remaining_attempts > 0 {
                    "Invalid verification code"
                } else {
                    "Invalid verification code. Maximum attempts reached."
                },
            ));
        }
        Err(TwoFactorVerifyError::Database) => return Err(ApiError::internal()),
    };

    let user = match repository::find_by_id(&pool, verify_res.user_id).await {
        Ok(Some(u)) => u,
        _ => return Err(ApiError::unauthorized("User not found")),
    };

    if user.updated_at > verify_res.created_at {
        return Err(ApiError::bad_request(
            "invalid_challenge",
            "Credentials were changed after challenge was issued. Please login again.",
        ));
    }

    if user.status != UserStatus::Active || (config.owner_only && user.role != UserRole::Owner) {
        return Err(ApiError::invalid_credentials());
    }

    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok());

    let remember_me = payload.remember_me.unwrap_or(verify_res.remember_me);

    let (session, raw_refresh_token) =
        session::create_session(&mut conn, user.id, user_agent, None, remember_me, now)
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

    let response_body = LoginResponse {
        user: UserProfile::from(&user),
        csrf_token: session.csrf_token,
    };

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

pub async fn resend_two_factor(
    State(pool): State<SqlitePool>,
    State(config): State<Config>,
    Json(payload): Json<ResendTwoFactorRequest>,
) -> Result<Response, ApiError> {
    let now = current_unix_time();

    let (challenge_token, new_code, user) = {
        let mut conn = pool.acquire().await.map_err(|_| ApiError::internal())?;
        let (token, code, user_id) =
            match two_factor::resend_challenge(&mut conn, &payload.challenge_token, now).await {
                Ok(res) => res,
                Err(TwoFactorResendError::InvalidChallenge) => {
                    return Err(ApiError::bad_request(
                        "invalid_challenge",
                        "Challenge is invalid or has expired",
                    ));
                }
                Err(TwoFactorResendError::CooldownActive { retry_after }) => {
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
                Err(TwoFactorResendError::MaxResendsExceeded) => {
                    return Err(ApiError::bad_request(
                        "max_resends_exceeded",
                        "Maximum resend limit reached. Please login again.",
                    ));
                }
                Err(TwoFactorResendError::Database) => return Err(ApiError::internal()),
            };

        let user = match repository::find_by_id(&pool, user_id).await {
            Ok(Some(u)) => u,
            _ => return Err(ApiError::unauthorized("User not found")),
        };

        (token, code, user)
    };

    let email_service = EmailService::new(config.clone());
    email_service
        .send_two_factor_code(&user.email, &new_code)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "failed to send resend 2FA email");
            ApiError::new(
                StatusCode::BAD_GATEWAY,
                "email_delivery_failed",
                "Failed to deliver verification email",
            )
        })?;

    let response_body = ResendTwoFactorResponse {
        challenge_token,
        email_masked: two_factor::mask_email(&user.email),
    };

    let mut response = (StatusCode::OK, Json(response_body)).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );

    Ok(response)
}
