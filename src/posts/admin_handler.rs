use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use sqlx::SqlitePool;

use super::{
    dto::{
        AdminPostQuery, CreatePostDto, PostResponse, PreviewPostDto, PreviewPostResponse,
        UpdatePostDto,
    },
    service::{self, PostServiceError},
};
use crate::{
    auth::{AuthenticatedUser, middleware::verify_csrf},
    http::{error::ApiError, json::Json},
    users::model::UserRole,
};

fn parse_if_match(headers: &HeaderMap) -> Result<i64, ApiError> {
    let raw = headers
        .get(header::IF_MATCH)
        .ok_or_else(|| ApiError::precondition_required("If-Match header is required"))?
        .to_str()
        .map_err(|_| ApiError::precondition_required("Invalid If-Match header"))?;

    let trimmed = raw.trim();
    let unquoted = trimmed
        .strip_prefix("W/\"")
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| trimmed.strip_prefix('"').and_then(|s| s.strip_suffix('"')))
        .unwrap_or(trimmed);

    unquoted
        .parse::<i64>()
        .map_err(|_| ApiError::precondition_failed("Invalid If-Match version tag"))
}

fn etag_header(version: i64) -> (header::HeaderName, header::HeaderValue) {
    (
        header::ETAG,
        header::HeaderValue::from_str(&format!("\"{}\"", version)).unwrap(),
    )
}

fn map_service_error(err: PostServiceError) -> ApiError {
    match err {
        PostServiceError::InvalidTitle => ApiError::bad_request(
            "invalid_title",
            "Title must be between 1 and 160 characters",
        ),
        PostServiceError::InvalidSummary => {
            ApiError::bad_request("invalid_summary", "Summary must be at most 320 characters")
        }
        PostServiceError::ContentTooLarge => {
            ApiError::payload_too_large("Content exceeds maximum size of 256 KiB")
        }
        PostServiceError::EmptyContentWhenPublished => {
            ApiError::bad_request("empty_content", "Content cannot be empty when publishing")
        }
        PostServiceError::TooManyTags => {
            ApiError::bad_request("too_many_tags", "Post can have at most 10 tags")
        }
        PostServiceError::InvalidTagName => ApiError::bad_request(
            "invalid_tag_name",
            "Tag name must be between 1 and 40 characters",
        ),
        PostServiceError::PostNotFound => ApiError::not_found("Post not found"),
        PostServiceError::SlugCollision => {
            ApiError::conflict("slug_collision", "Could not allocate a unique slug")
        }
        PostServiceError::VersionConflict => {
            ApiError::precondition_failed("Resource version mismatch")
        }
        PostServiceError::Database(_) => ApiError::internal(),
    }
}

pub async fn list_posts(
    auth: AuthenticatedUser,
    State(pool): State<SqlitePool>,
    Query(query): Query<AdminPostQuery>,
) -> Result<Response, ApiError> {
    let author_id = if auth.user.role == UserRole::Owner {
        None
    } else {
        Some(auth.user.id)
    };

    let result =
        service::list_admin_posts(&pool, author_id, query.status, query.limit, query.offset)
            .await
            .map_err(|_| ApiError::internal())?;

    let mut response = (StatusCode::OK, Json(result)).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}

pub async fn create_post(
    auth: AuthenticatedUser,
    State(pool): State<SqlitePool>,
    headers: HeaderMap,
    Json(payload): Json<CreatePostDto>,
) -> Result<Response, ApiError> {
    verify_csrf(&headers, &auth.session.csrf_token)?;

    let post_with_tags = service::create_post(&pool, auth.user.id, payload)
        .await
        .map_err(map_service_error)?;

    let version = post_with_tags.post.version;
    let post_id = post_with_tags.post.id;
    let (etag_name, etag_val) = etag_header(version);

    let location = format!("/api/v1/admin/posts/{}", post_id);
    let mut response = (
        StatusCode::CREATED,
        Json(PostResponse::from(&post_with_tags)),
    )
        .into_response();

    response.headers_mut().insert(etag_name, etag_val);
    if let Ok(loc_val) = header::HeaderValue::from_str(&location) {
        response.headers_mut().insert(header::LOCATION, loc_val);
    }
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}

pub async fn get_post(
    auth: AuthenticatedUser,
    State(pool): State<SqlitePool>,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    let post_resp = service::get_admin_post_by_id(&pool, id)
        .await
        .map_err(|_| ApiError::internal())?
        .ok_or_else(|| ApiError::not_found("Post not found"))?;

    if auth.user.role != UserRole::Owner && post_resp.author_id != auth.user.id {
        return Err(ApiError::forbidden(
            "You do not have permission to view this post",
        ));
    }

    let (etag_name, etag_val) = etag_header(post_resp.version);
    let mut response = (StatusCode::OK, Json(post_resp)).into_response();
    response.headers_mut().insert(etag_name, etag_val);
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}

pub async fn update_post(
    auth: AuthenticatedUser,
    State(pool): State<SqlitePool>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(mut payload): Json<UpdatePostDto>,
) -> Result<Response, ApiError> {
    verify_csrf(&headers, &auth.session.csrf_token)?;
    let expected_version = parse_if_match(&headers)?;
    payload.version = expected_version;

    let existing = service::get_post_by_id(&pool, id)
        .await
        .map_err(|_| ApiError::internal())?
        .ok_or_else(|| ApiError::not_found("Post not found"))?;

    if auth.user.role != UserRole::Owner && existing.post.author_id != auth.user.id {
        return Err(ApiError::forbidden(
            "You do not have permission to edit this post",
        ));
    }

    let updated = service::update_post(&pool, id, payload)
        .await
        .map_err(map_service_error)?;

    let (etag_name, etag_val) = etag_header(updated.post.version);
    let mut response = (StatusCode::OK, Json(PostResponse::from(&updated))).into_response();
    response.headers_mut().insert(etag_name, etag_val);
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}

pub async fn delete_post(
    auth: AuthenticatedUser,
    State(pool): State<SqlitePool>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    verify_csrf(&headers, &auth.session.csrf_token)?;
    let expected_version = parse_if_match(&headers)?;

    let existing = service::get_post_by_id(&pool, id)
        .await
        .map_err(|_| ApiError::internal())?
        .ok_or_else(|| ApiError::not_found("Post not found"))?;

    if auth.user.role != UserRole::Owner && existing.post.author_id != auth.user.id {
        return Err(ApiError::forbidden(
            "You do not have permission to delete this post",
        ));
    }

    service::delete_post_versioned(&pool, id, expected_version)
        .await
        .map_err(map_service_error)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

pub async fn publish_post(
    auth: AuthenticatedUser,
    State(pool): State<SqlitePool>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    verify_csrf(&headers, &auth.session.csrf_token)?;
    let expected_version = parse_if_match(&headers)?;

    let existing = service::get_post_by_id(&pool, id)
        .await
        .map_err(|_| ApiError::internal())?
        .ok_or_else(|| ApiError::not_found("Post not found"))?;

    if auth.user.role != UserRole::Owner && existing.post.author_id != auth.user.id {
        return Err(ApiError::forbidden(
            "You do not have permission to publish this post",
        ));
    }

    let published = service::publish_post(&pool, id, expected_version)
        .await
        .map_err(map_service_error)?;

    let (etag_name, etag_val) = etag_header(published.post.version);
    let mut response = (StatusCode::OK, Json(PostResponse::from(&published))).into_response();
    response.headers_mut().insert(etag_name, etag_val);
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}

pub async fn unpublish_post(
    auth: AuthenticatedUser,
    State(pool): State<SqlitePool>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    verify_csrf(&headers, &auth.session.csrf_token)?;
    let expected_version = parse_if_match(&headers)?;

    let existing = service::get_post_by_id(&pool, id)
        .await
        .map_err(|_| ApiError::internal())?
        .ok_or_else(|| ApiError::not_found("Post not found"))?;

    if auth.user.role != UserRole::Owner && existing.post.author_id != auth.user.id {
        return Err(ApiError::forbidden(
            "You do not have permission to unpublish this post",
        ));
    }

    let unpublished = service::unpublish_post(&pool, id, expected_version)
        .await
        .map_err(map_service_error)?;

    let (etag_name, etag_val) = etag_header(unpublished.post.version);
    let mut response = (StatusCode::OK, Json(PostResponse::from(&unpublished))).into_response();
    response.headers_mut().insert(etag_name, etag_val);
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}

pub async fn archive_post(
    auth: AuthenticatedUser,
    State(pool): State<SqlitePool>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    verify_csrf(&headers, &auth.session.csrf_token)?;
    let expected_version = parse_if_match(&headers)?;

    let existing = service::get_post_by_id(&pool, id)
        .await
        .map_err(|_| ApiError::internal())?
        .ok_or_else(|| ApiError::not_found("Post not found"))?;

    if auth.user.role != UserRole::Owner && existing.post.author_id != auth.user.id {
        return Err(ApiError::forbidden("You do not have permission to archive this post"));
    }

    let archived = service::archive_post(&pool, id, expected_version)
        .await
        .map_err(map_service_error)?;

    let (etag_name, etag_val) = etag_header(archived.post.version);
    let mut response = (StatusCode::OK, Json(PostResponse::from(&archived))).into_response();
    response.headers_mut().insert(etag_name, etag_val);
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}

pub async fn unarchive_post(
    auth: AuthenticatedUser,
    State(pool): State<SqlitePool>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    verify_csrf(&headers, &auth.session.csrf_token)?;
    let expected_version = parse_if_match(&headers)?;

    let existing = service::get_post_by_id(&pool, id)
        .await
        .map_err(|_| ApiError::internal())?
        .ok_or_else(|| ApiError::not_found("Post not found"))?;

    if auth.user.role != UserRole::Owner && existing.post.author_id != auth.user.id {
        return Err(ApiError::forbidden("You do not have permission to unarchive this post"));
    }

    let unarchived = service::unarchive_post(&pool, id, expected_version)
        .await
        .map_err(map_service_error)?;

    let (etag_name, etag_val) = etag_header(unarchived.post.version);
    let mut response = (StatusCode::OK, Json(PostResponse::from(&unarchived))).into_response();
    response.headers_mut().insert(etag_name, etag_val);
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}

pub async fn preview_post(
    auth: AuthenticatedUser,
    headers: HeaderMap,
    Json(payload): Json<PreviewPostDto>,
) -> Result<Response, ApiError> {
    verify_csrf(&headers, &auth.session.csrf_token)?;

    let content_html = service::preview_markdown(&payload.content_md);
    let response_body = PreviewPostResponse { content_html };

    let mut response = (StatusCode::OK, Json(response_body)).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}

pub async fn save_post_draft(
    auth: AuthenticatedUser,
    State(pool): State<SqlitePool>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(payload): Json<UpdatePostDto>,
) -> Result<Response, ApiError> {
    verify_csrf(&headers, &auth.session.csrf_token)?;
    let existing = service::get_post_by_id(&pool, id)
        .await
        .map_err(|_| ApiError::internal())?
        .ok_or_else(|| ApiError::not_found("Post not found"))?;

    if auth.user.role != UserRole::Owner && existing.post.author_id != auth.user.id {
        return Err(ApiError::forbidden(
            "You do not have permission to modify this post",
        ));
    }

    let post_resp = service::save_post_draft(&pool, id, payload)
        .await
        .map_err(map_service_error)?;

    let (etag_name, etag_val) = etag_header(post_resp.version);
    let mut response = (StatusCode::OK, Json(post_resp)).into_response();
    response.headers_mut().insert(etag_name, etag_val);
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}

pub async fn discard_post_draft(
    auth: AuthenticatedUser,
    State(pool): State<SqlitePool>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    verify_csrf(&headers, &auth.session.csrf_token)?;
    let existing = service::get_post_by_id(&pool, id)
        .await
        .map_err(|_| ApiError::internal())?
        .ok_or_else(|| ApiError::not_found("Post not found"))?;

    if auth.user.role != UserRole::Owner && existing.post.author_id != auth.user.id {
        return Err(ApiError::forbidden(
            "You do not have permission to modify this post",
        ));
    }

    let post_resp = service::discard_post_draft(&pool, id)
        .await
        .map_err(map_service_error)?;

    let (etag_name, etag_val) = etag_header(post_resp.version);
    let mut response = (StatusCode::OK, Json(post_resp)).into_response();
    response.headers_mut().insert(etag_name, etag_val);
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}
