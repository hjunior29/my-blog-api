use sqlx::SqlitePool;
use thiserror::Error;

use super::{
    dto::{MediaListResponse, MediaResponse},
    model::{Media, MediaKind},
    repository,
    storage::{StorageBackend, StorageError},
};
use crate::users::model::{User, UserRole};

pub const MAX_IMAGE_SIZE: usize = 20 * 1024 * 1024;
pub const MAX_AUDIO_SIZE: usize = 50 * 1024 * 1024;
pub const MAX_VIDEO_SIZE: usize = 100 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum MediaServiceError {
    #[error("unsupported media type: {0}")]
    UnsupportedMediaType(String),
    #[error("file size {0} exceeds limit of {1} bytes")]
    PayloadTooLarge(usize, usize),
    #[error("media file is empty")]
    EmptyFile,
    #[error("media not found")]
    NotFound,
    #[error("forbidden: you do not have permission to delete this media")]
    Forbidden,
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

pub fn sanitize_filename(filename: &str) -> String {
    let clean = filename
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' })
        .collect::<String>();
    let clean = clean.trim_matches('.').trim_matches('_');
    if clean.is_empty() {
        "file".to_string()
    } else {
        clean.to_string()
    }
}

const ALLOWED_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "webp", "gif", "mp4", "webm", "mov", "mp3", "ogg", "wav", "m4a",
];

fn validate_magic_bytes(data: &[u8], content_type: &str) -> bool {
    match content_type {
        "image/png" => data.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]),
        "image/jpeg" => data.starts_with(&[0xFF, 0xD8, 0xFF]),
        "image/gif" => data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a"),
        "image/webp" => data.len() >= 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP",
        _ => true,
    }
}

pub fn detect_media_kind(filename: &str, content_type: &str) -> Result<(MediaKind, String), MediaServiceError> {
    let lower = filename.to_ascii_lowercase();
    let lower_ct = content_type.to_ascii_lowercase();
    if lower.ends_with(".svg") || lower_ct == "image/svg+xml" || lower_ct.contains("svg") {
        return Err(MediaServiceError::UnsupportedMediaType("SVG images are not permitted".to_string()));
    }

    let ext = lower.rsplit('.').next().unwrap_or("");
    if !ALLOWED_EXTENSIONS.contains(&ext) {
        return Err(MediaServiceError::UnsupportedMediaType("File extension is not permitted".to_string()));
    }

    if let Some(kind) = MediaKind::from_content_type(content_type) {
        return Ok((kind, content_type.to_string()));
    }

    if lower.ends_with(".png") {
        Ok((MediaKind::Image, "image/png".to_string()))
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        Ok((MediaKind::Image, "image/jpeg".to_string()))
    } else if lower.ends_with(".webp") {
        Ok((MediaKind::Image, "image/webp".to_string()))
    } else if lower.ends_with(".gif") {
        Ok((MediaKind::Image, "image/gif".to_string()))
    } else if lower.ends_with(".mp4") {
        Ok((MediaKind::Video, "video/mp4".to_string()))
    } else if lower.ends_with(".webm") {
        Ok((MediaKind::Video, "video/webm".to_string()))
    } else if lower.ends_with(".mov") {
        Ok((MediaKind::Video, "video/quicktime".to_string()))
    } else if lower.ends_with(".mp3") {
        Ok((MediaKind::Audio, "audio/mpeg".to_string()))
    } else if lower.ends_with(".ogg") {
        Ok((MediaKind::Audio, "audio/ogg".to_string()))
    } else if lower.ends_with(".wav") {
        Ok((MediaKind::Audio, "audio/wav".to_string()))
    } else if lower.ends_with(".m4a") {
        Ok((MediaKind::Audio, "audio/mp4".to_string()))
    } else {
        Err(MediaServiceError::UnsupportedMediaType(content_type.to_string()))
    }
}

pub async fn upload_media(
    storage: &StorageBackend,
    pool: &SqlitePool,
    uploader_id: i64,
    filename: &str,
    content_type: &str,
    data: &[u8],
) -> Result<MediaResponse, MediaServiceError> {
    if data.len() < 4 {
        return Err(MediaServiceError::EmptyFile);
    }

    let (kind, resolved_content_type) = detect_media_kind(filename, content_type)?;

    if !validate_magic_bytes(data, &resolved_content_type) {
        return Err(MediaServiceError::UnsupportedMediaType("File signature does not match declared type".to_string()));
    }

    let check_len = data.len().min(1024);
    let lower_preview = String::from_utf8_lossy(&data[..check_len]).to_ascii_lowercase();
    if lower_preview.contains("<svg") || lower_preview.contains("<?xml") || lower_preview.contains("<script") {
        return Err(MediaServiceError::UnsupportedMediaType("SVG or script payloads are not allowed".to_string()));
    }

    let max_size = match kind {
        MediaKind::Image => MAX_IMAGE_SIZE,
        MediaKind::Audio => MAX_AUDIO_SIZE,
        MediaKind::Video => MAX_VIDEO_SIZE,
    };

    if data.len() > max_size {
        return Err(MediaServiceError::PayloadTooLarge(data.len(), max_size));
    }

    let id = uuid::Uuid::new_v4().to_string();
    let safe_filename = sanitize_filename(filename);
    let storage_key = format!("{}/{}", id, safe_filename);

    let public_url = storage
        .put_object(&storage_key, &resolved_content_type, data)
        .await?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let media = Media {
        id,
        filename: safe_filename,
        content_type: resolved_content_type,
        media_kind: kind.as_str().to_string(),
        size_bytes: data.len() as i64,
        storage_key,
        storage_backend: storage.name().to_string(),
        public_url,
        uploader_id,
        created_at: now,
    };

    repository::insert_media(pool, &media).await?;
    Ok(MediaResponse::from(&media))
}

pub async fn get_media_by_id(
    pool: &SqlitePool,
    id: &str,
) -> Result<Media, MediaServiceError> {
    repository::find_media_by_id(pool, id)
        .await?
        .ok_or(MediaServiceError::NotFound)
}

pub async fn list_media(
    pool: &SqlitePool,
    kind: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<MediaListResponse, MediaServiceError> {
    let limit = limit.clamp(1, 100);
    let offset = offset.max(0);
    let items = repository::list_media(pool, kind, limit, offset).await?;
    let total = repository::count_media(pool, kind).await?;
    Ok(MediaListResponse {
        items: items.iter().map(MediaResponse::from).collect(),
        total,
        limit,
        offset,
    })
}

pub async fn delete_media(
    storage: &StorageBackend,
    pool: &SqlitePool,
    id: &str,
    user: &User,
) -> Result<(), MediaServiceError> {
    let media = repository::find_media_by_id(pool, id)
        .await?
        .ok_or(MediaServiceError::NotFound)?;

    if user.role != UserRole::Owner && media.uploader_id != user.id {
        return Err(MediaServiceError::Forbidden);
    }

    let _ = storage.delete_object(&media.storage_key).await;
    repository::delete_media(pool, id).await?;
    Ok(())
}
