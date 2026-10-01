use axum::{
    extract::{FromRef, FromRequestParts, Request, State},
    http::{HeaderMap, Method, header, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
};
use sqlx::SqlitePool;

use super::{
    cookie::{self, access_cookie_name},
    jwt,
    session::{self, AuthSession, constant_time_compare},
};
use crate::{
    config::Config,
    http::error::ApiError,
    users::{
        model::{User, UserRole, UserStatus},
        repository,
        service::current_unix_time,
    },
};

pub async fn check_origin(State(config): State<Config>, request: Request, next: Next) -> Response {
    let method = request.method();
    if matches!(
        *method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    ) {
        if let Some(sec_fetch_site) = request
            .headers()
            .get("sec-fetch-site")
            .and_then(|v| v.to_str().ok())
            && sec_fetch_site == "cross-site"
        {
            return ApiError::forbidden("Cross-site request rejected").into_response();
        }

        let origin = request
            .headers()
            .get(header::ORIGIN)
            .and_then(|v| v.to_str().ok());

        match origin {
            Some(o) if o == config.app_origin => (),
            _ => {
                return ApiError::forbidden("Origin not allowed").into_response();
            }
        }
    }

    next.run(request).await
}

#[derive(Clone)]
pub struct AuthenticatedUser {
    pub user: User,
    pub session: AuthSession,
}

impl<S> FromRequestParts<S> for AuthenticatedUser
where
    SqlitePool: FromRef<S>,
    Config: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let pool = SqlitePool::from_ref(state);
        let config = Config::from_ref(state);

        let cookie_name = access_cookie_name(config.secure_cookies);
        let token = cookie::extract_cookie(&parts.headers, cookie_name)
            .ok_or_else(|| ApiError::unauthorized("Authentication required"))?;

        let claims = jwt::verify_access_token(&config, &token)
            .map_err(|_| ApiError::unauthorized("Invalid access token"))?;

        let user_id = claims
            .sub
            .parse::<i64>()
            .map_err(|_| ApiError::unauthorized("Invalid token subject"))?;

        let now = current_unix_time();
        let session = session::find_active_session(&pool, &claims.sid, now)
            .await
            .map_err(|_| ApiError::internal())?
            .ok_or_else(|| ApiError::unauthorized("Session expired or revoked"))?;

        if session.user_id != user_id {
            return Err(ApiError::unauthorized("Session user mismatch"));
        }

        let user = repository::find_by_id(&pool, user_id)
            .await
            .map_err(|_| ApiError::internal())?
            .ok_or_else(|| ApiError::unauthorized("User not found"))?;

        if user.status != UserStatus::Active || (config.owner_only && user.role != UserRole::Owner)
        {
            return Err(ApiError::unauthorized("User account is inactive"));
        }

        Ok(Self { user, session })
    }
}

#[derive(Clone)]
pub struct RequireOwner(pub AuthenticatedUser);

impl<S> FromRequestParts<S> for RequireOwner
where
    SqlitePool: FromRef<S>,
    Config: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let auth = AuthenticatedUser::from_request_parts(parts, state).await?;
        if auth.user.role != UserRole::Owner {
            return Err(ApiError::forbidden("Insufficient permissions"));
        }
        Ok(Self(auth))
    }
}

pub fn verify_csrf(headers: &HeaderMap, expected_csrf: &str) -> Result<(), ApiError> {
    let header_token = headers
        .get("x-csrf-token")
        .and_then(|h| h.to_str().ok())
        .ok_or_else(ApiError::invalid_csrf_token)?;

    if !constant_time_compare(header_token, expected_csrf) {
        return Err(ApiError::invalid_csrf_token());
    }

    Ok(())
}
