use axum::{
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use sqlx::SqlitePool;

use super::{
    dto::{ChangePasswordRequest, UpdateProfileRequest},
    model::UserProfile,
    service::{self, UserServiceError},
};
use crate::{
    auth::{AuthenticatedUser, middleware::verify_csrf, password::PasswordError},
    http::{error::ApiError, json::Json},
};

pub async fn me(auth: AuthenticatedUser) -> Response {
    let profile = UserProfile::from(&auth.user);
    let mut response = (StatusCode::OK, Json(profile)).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    response
}

pub async fn update_me(
    State(pool): State<SqlitePool>,
    auth: AuthenticatedUser,
    headers: HeaderMap,
    Json(payload): Json<UpdateProfileRequest>,
) -> Result<Response, ApiError> {
    verify_csrf(&headers, &auth.session.csrf_token)?;

    let updated = service::update_profile(
        &pool,
        auth.user.id,
        payload.display_name,
        payload.bio,
        payload.avatar_media_id,
    )
    .await
    .map_err(|err| match err {
        UserServiceError::InvalidDisplayName => ApiError::unprocessable_entity(
            "invalid_display_name",
            "Display name must be between 1 and 80 characters",
        ),
        UserServiceError::InvalidBio => {
            ApiError::unprocessable_entity("invalid_bio", "Bio must be at most 500 characters")
        }
        _ => ApiError::internal(),
    })?;

    let profile = UserProfile::from(&updated);
    let mut response = (StatusCode::OK, Json(profile)).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}

pub async fn change_password(
    State(pool): State<SqlitePool>,
    auth: AuthenticatedUser,
    headers: HeaderMap,
    Json(payload): Json<ChangePasswordRequest>,
) -> Result<Response, ApiError> {
    verify_csrf(&headers, &auth.session.csrf_token)?;

    service::change_password(
        &pool,
        auth.user.id,
        &payload.current_password,
        &payload.new_password,
        &auth.session.id,
    )
    .await
    .map_err(|err| match err {
        UserServiceError::InvalidCredentials => ApiError::invalid_credentials(),
        UserServiceError::Password(PasswordError::InvalidPolicy) => ApiError::unprocessable_entity(
            "invalid_password",
            "Password must be between 15 and 128 characters and at most 512 bytes",
        ),
        _ => ApiError::internal(),
    })?;

    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}
