use sqlx::SqlitePool;

use super::model::Media;

pub async fn insert_media(pool: &SqlitePool, media: &Media) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO media (id, filename, content_type, media_kind, size_bytes, storage_key, storage_backend, public_url, uploader_id, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&media.id)
    .bind(&media.filename)
    .bind(&media.content_type)
    .bind(&media.media_kind)
    .bind(media.size_bytes)
    .bind(&media.storage_key)
    .bind(&media.storage_backend)
    .bind(&media.public_url)
    .bind(media.uploader_id)
    .bind(media.created_at)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn find_media_by_id(pool: &SqlitePool, id: &str) -> Result<Option<Media>, sqlx::Error> {
    sqlx::query_as::<_, Media>(
        "SELECT id, filename, content_type, media_kind, size_bytes, storage_key, storage_backend, public_url, uploader_id, created_at
         FROM media WHERE id = ?"
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn list_media(
    pool: &SqlitePool,
    kind: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<Vec<Media>, sqlx::Error> {
    if let Some(k) = kind {
        sqlx::query_as::<_, Media>(
            "SELECT id, filename, content_type, media_kind, size_bytes, storage_key, storage_backend, public_url, uploader_id, created_at
             FROM media WHERE media_kind = ? ORDER BY created_at DESC LIMIT ? OFFSET ?"
        )
        .bind(k)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, Media>(
            "SELECT id, filename, content_type, media_kind, size_bytes, storage_key, storage_backend, public_url, uploader_id, created_at
             FROM media ORDER BY created_at DESC LIMIT ? OFFSET ?"
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
    }
}

pub async fn count_media(pool: &SqlitePool, kind: Option<&str>) -> Result<i64, sqlx::Error> {
    if let Some(k) = kind {
        sqlx::query_scalar("SELECT COUNT(*) FROM media WHERE media_kind = ?")
            .bind(k)
            .fetch_one(pool)
            .await
    } else {
        sqlx::query_scalar("SELECT COUNT(*) FROM media")
            .fetch_one(pool)
            .await
    }
}

pub async fn delete_media(pool: &SqlitePool, id: &str) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM media WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}
