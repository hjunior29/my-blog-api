use axum::{
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use sqlx::SqlitePool;

use super::{
    dto::{PaginationQuery, PostResponse, SearchQuery},
    model::PostStatus,
    service::{self, PostServiceError},
};
use crate::http::{error::ApiError, json::Json};

pub async fn list_posts(
    State(pool): State<SqlitePool>,
    Query(pagination): Query<PaginationQuery>,
) -> Result<Response, ApiError> {
    let result = service::list_published_posts(&pool, pagination.limit, pagination.offset)
        .await
        .map_err(|_| ApiError::internal())?;

    let mut response = (StatusCode::OK, Json(result)).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("public, max-age=60"),
    );
    Ok(response)
}

pub async fn search_posts(
    State(pool): State<SqlitePool>,
    Query(query): Query<SearchQuery>,
) -> Result<Response, ApiError> {
    let result = service::search_published_posts(&pool, &query.q, query.limit, query.offset)
        .await
        .map_err(|_| ApiError::internal())?;

    let mut response = (StatusCode::OK, Json(result)).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("public, max-age=60"),
    );
    Ok(response)
}

pub async fn get_post_by_slug(
    State(pool): State<SqlitePool>,
    Path(slug): Path<String>,
) -> Result<Response, ApiError> {
    let post_with_tags = service::get_post_by_slug(&pool, &slug)
        .await
        .map_err(|err| match err {
            PostServiceError::PostNotFound => ApiError::not_found("Post not found"),
            _ => ApiError::internal(),
        })?
        .ok_or_else(|| ApiError::not_found("Post not found"))?;

    if post_with_tags.post.status != PostStatus::Published {
        return Err(ApiError::not_found("Post not found"));
    }

    let response_body = PostResponse::from(&post_with_tags);
    let mut response = (StatusCode::OK, Json(response_body)).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("public, max-age=60"),
    );
    Ok(response)
}

pub async fn list_tags(State(pool): State<SqlitePool>) -> Result<Response, ApiError> {
    let tags = service::list_published_tags(&pool)
        .await
        .map_err(|_| ApiError::internal())?;

    let mut response = (StatusCode::OK, Json(tags)).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("public, max-age=300"),
    );
    Ok(response)
}
