use axum::{
    extract::{Multipart, Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
};
use tokio_util::io::ReaderStream;
use sqlx::SqlitePool;

use super::{
    dto::ListMediaQuery,
    service::{self, MediaServiceError},
    storage::StorageBackend,
};
use crate::{
    auth::{AuthenticatedUser, middleware::verify_csrf},
    http::{error::ApiError, json::Json},
};

pub const UPLOAD_LIMIT_BYTES: usize = 105 * 1024 * 1024;

fn map_service_error(err: MediaServiceError) -> ApiError {
    match err {
        MediaServiceError::UnsupportedMediaType(_) => {
            ApiError::unsupported_media_type("Unsupported media format")
        }
        MediaServiceError::PayloadTooLarge(_, _) => {
            ApiError::payload_too_large("File exceeds maximum allowed size")
        }
        MediaServiceError::EmptyFile => {
            ApiError::bad_request("empty_file", "Upload payload contains no file data")
        }
        MediaServiceError::NotFound => ApiError::not_found("Media not found"),
        MediaServiceError::Forbidden => ApiError::forbidden("Permission denied"),
        MediaServiceError::Storage(e) => {
            tracing::error!("storage error: {:?}", e);
            ApiError::internal()
        }
        MediaServiceError::Database(e) => {
            tracing::error!("database error: {:?}", e);
            ApiError::internal()
        }
    }
}

pub async fn upload_media_handler(
    auth: AuthenticatedUser,
    State(pool): State<SqlitePool>,
    State(storage): State<StorageBackend>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Response, ApiError> {
    verify_csrf(&headers, &auth.session.csrf_token)?;

    let mut file_data: Option<Vec<u8>> = None;
    let mut filename = String::from("upload");
    let mut content_type = String::from("application/octet-stream");

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| ApiError::bad_request("invalid_multipart", "Failed to parse multipart body"))?
    {
        if let Some(name) = field.file_name() {
            filename = name.to_string();
        }
        if let Some(ct) = field.content_type() {
            content_type = ct.to_string();
        }
        let bytes = field
            .bytes()
            .await
            .map_err(|_| ApiError::bad_request("read_error", "Failed to read file bytes"))?;
        if !bytes.is_empty() {
            file_data = Some(bytes.to_vec());
            break;
        }
    }

    let data = file_data.ok_or_else(|| {
        ApiError::bad_request("empty_file", "No file was provided in the multipart request")
    })?;

    let media_resp = service::upload_media(
        &storage,
        &pool,
        auth.user.id,
        &filename,
        &content_type,
        &data,
    )
    .await
    .map_err(map_service_error)?;

    Ok((StatusCode::CREATED, Json(media_resp)).into_response())
}

pub async fn list_media_handler(
    _auth: AuthenticatedUser,
    State(pool): State<SqlitePool>,
    Query(query): Query<ListMediaQuery>,
) -> Result<Response, ApiError> {
    let limit = query.limit.unwrap_or(20);
    let offset = query.offset.unwrap_or(0);
    let resp = service::list_media(&pool, query.kind.as_deref(), limit, offset)
        .await
        .map_err(map_service_error)?;

    Ok((StatusCode::OK, Json(resp)).into_response())
}

pub async fn delete_media_handler(
    auth: AuthenticatedUser,
    State(pool): State<SqlitePool>,
    State(storage): State<StorageBackend>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    verify_csrf(&headers, &auth.session.csrf_token)?;

    service::delete_media(&storage, &pool, &id, &auth.user)
        .await
        .map_err(map_service_error)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

pub async fn get_media_handler(
    State(pool): State<SqlitePool>,
    State(storage): State<StorageBackend>,
    Path(id_or_key): Path<String>,
) -> Result<Response, ApiError> {
    get_media_by_id_internal(&pool, &storage, &id_or_key).await
}

pub async fn get_media_with_filename_handler(
    State(pool): State<SqlitePool>,
    State(storage): State<StorageBackend>,
    Path((id, _filename)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    get_media_by_id_internal(&pool, &storage, &id).await
}

async fn get_media_by_id_internal(
    pool: &SqlitePool,
    storage: &StorageBackend,
    id: &str,
) -> Result<Response, ApiError> {
    if id.is_empty() || id.contains('/') || id.contains('\\') || id.contains("..") {
        return Err(ApiError::not_found("Media not found"));
    }

    let media = match service::get_media_by_id(pool, id).await {
        Ok(m) => m,
        Err(MediaServiceError::NotFound) => return Err(ApiError::not_found("Media not found")),
        Err(e) => {
            tracing::error!("error querying media by id: {:?}", e);
            return Err(ApiError::internal());
        }
    };

    let (content_type, body) = match storage {
        StorageBackend::Tigris(_) => {
            return Ok(Redirect::temporary(&media.public_url).into_response());
        }
        StorageBackend::Local(local) => {
            let (content_type, file) = local
                .open_file(&media.storage_key)
                .await
                .map_err(|_| ApiError::not_found("Media object not found"))?;
            let stream = ReaderStream::new(file);
            (content_type, axum::body::Body::from_stream(stream))
        }
    };

    let mut response = (StatusCode::OK, body).into_response();
    if let Ok(val) = header::HeaderValue::from_str(&content_type) {
        response.headers_mut().insert(header::CONTENT_TYPE, val);
    }
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("public, max-age=31536000, immutable"),
    );
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        header::HeaderValue::from_static("nosniff"),
    );
    response.headers_mut().insert(
        header::HeaderName::from_static("content-security-policy"),
        header::HeaderValue::from_static("default-src 'none'; sandbox"),
    );

    Ok(response)
}
