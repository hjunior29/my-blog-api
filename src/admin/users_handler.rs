use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use sqlx::SqlitePool;

use crate::{
    auth::{RequireOwner, middleware::verify_csrf},
    http::{error::ApiError, json::Json},
    users::{
        dto::{AdminUpdateUserRequest, AdminUserResponse, PaginationQuery, UserListResponse},
        service::{self, UserServiceError},
    },
};

pub async fn list_users(
    _owner: RequireOwner,
    State(pool): State<SqlitePool>,
    Query(pagination): Query<PaginationQuery>,
) -> Result<Response, ApiError> {
    let limit = pagination.limit.unwrap_or(20).clamp(1, 50);
    let offset = pagination.offset.unwrap_or(0).max(0);

    let (users, total) = service::list_users_paged(&pool, Some(limit), Some(offset))
        .await
        .map_err(|_| ApiError::internal())?;

    let items: Vec<AdminUserResponse> = users.iter().map(AdminUserResponse::from).collect();
    let body = UserListResponse {
        items,
        total,
        limit,
        offset,
    };

    let mut response = (StatusCode::OK, Json(body)).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}

pub async fn update_user(
    owner: RequireOwner,
    State(pool): State<SqlitePool>,
    headers: HeaderMap,
    Path(user_id): Path<i64>,
    Json(payload): Json<AdminUpdateUserRequest>,
) -> Result<Response, ApiError> {
    verify_csrf(&headers, &owner.0.session.csrf_token)?;

    let updated = service::admin_update_user(&pool, user_id, payload.role, payload.status)
        .await
        .map_err(|err| match err {
            UserServiceError::UserNotFound => ApiError::not_found("User not found"),
            UserServiceError::LastOwnerProtection => ApiError::unprocessable_entity(
                "last_owner_protection",
                "Cannot disable or demote the last active owner",
            ),
            _ => ApiError::internal(),
        })?;

    let response_body = AdminUserResponse::from(&updated);
    let mut response = (StatusCode::OK, Json(response_body)).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}
