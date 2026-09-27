use sqlx::{SqliteConnection, SqlitePool};

use super::model::{Post, PostStatus, Tag};

pub struct NewPostRecord<'a> {
    pub slug: &'a str,
    pub title: &'a str,
    pub summary: &'a str,
    pub content_md: &'a str,
    pub content_html: &'a str,
    pub featured_image_media_id: Option<&'a str>,
    pub status: PostStatus,
    pub author_id: i64,
    pub published_at: Option<i64>,
    pub scheduled_for: Option<i64>,
    pub now: i64,
}

pub struct UpdatePostRecord<'a> {
    pub id: i64,
    pub title: &'a str,
    pub summary: &'a str,
    pub content_md: &'a str,
    pub content_html: &'a str,
    pub featured_image_media_id: Option<&'a str>,
    pub status: PostStatus,
    pub published_at: Option<i64>,
    pub scheduled_for: Option<i64>,
    pub now: i64,
    pub expected_version: i64,
}

pub async fn find_post_by_id(pool: &SqlitePool, id: i64) -> Result<Option<Post>, sqlx::Error> {
    sqlx::query_as::<_, Post>(
        "SELECT id, slug, title, summary, content_md, content_html,
                featured_image_media_id, status, author_id, published_at,
                scheduled_for, created_at, updated_at, version
         FROM posts WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn find_post_by_slug(pool: &SqlitePool, slug: &str) -> Result<Option<Post>, sqlx::Error> {
    sqlx::query_as::<_, Post>(
        "SELECT id, slug, title, summary, content_md, content_html,
                featured_image_media_id, status, author_id, published_at,
                scheduled_for, created_at, updated_at, version
         FROM posts WHERE slug = ?",
    )
    .bind(slug)
    .fetch_optional(pool)
    .await
}

pub async fn slug_exists(pool: &SqlitePool, slug: &str) -> Result<bool, sqlx::Error> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM posts WHERE slug = ?")
        .bind(slug)
        .fetch_one(pool)
        .await?;
    Ok(count > 0)
}

pub async fn insert_post(
    conn: &mut SqliteConnection,
    record: &NewPostRecord<'_>,
) -> Result<i64, sqlx::Error> {
    let res = sqlx::query(
        "INSERT INTO posts (
            slug, title, summary, content_md, content_html,
            featured_image_media_id, status, author_id, published_at,
            scheduled_for, created_at, updated_at, version
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)",
    )
    .bind(record.slug)
    .bind(record.title)
    .bind(record.summary)
    .bind(record.content_md)
    .bind(record.content_html)
    .bind(record.featured_image_media_id)
    .bind(record.status.to_string())
    .bind(record.author_id)
    .bind(record.published_at)
    .bind(record.scheduled_for)
    .bind(record.now)
    .bind(record.now)
    .execute(conn)
    .await?;

    Ok(res.last_insert_rowid())
}

pub async fn update_post(
    conn: &mut SqliteConnection,
    record: &UpdatePostRecord<'_>,
) -> Result<bool, sqlx::Error> {
    let res = sqlx::query(
        "UPDATE posts SET
            title = ?, summary = ?, content_md = ?, content_html = ?,
            featured_image_media_id = ?, status = ?, published_at = ?,
            scheduled_for = ?, updated_at = ?, version = version + 1
         WHERE id = ? AND version = ?",
    )
    .bind(record.title)
    .bind(record.summary)
    .bind(record.content_md)
    .bind(record.content_html)
    .bind(record.featured_image_media_id)
    .bind(record.status.to_string())
    .bind(record.published_at)
    .bind(record.scheduled_for)
    .bind(record.now)
    .bind(record.id)
    .bind(record.expected_version)
    .execute(conn)
    .await?;

    Ok(res.rows_affected() > 0)
}

pub async fn delete_post(pool: &SqlitePool, id: i64) -> Result<bool, sqlx::Error> {
    let res = sqlx::query("DELETE FROM posts WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

pub async fn find_or_create_tag(
    conn: &mut SqliteConnection,
    name: &str,
    slug: &str,
    now: i64,
) -> Result<Tag, sqlx::Error> {
    sqlx::query("INSERT OR IGNORE INTO tags (name, slug, created_at) VALUES (?, ?, ?)")
        .bind(name)
        .bind(slug)
        .bind(now)
        .execute(&mut *conn)
        .await?;

    sqlx::query_as::<_, Tag>("SELECT id, name, slug, created_at FROM tags WHERE slug = ?")
        .bind(slug)
        .fetch_one(&mut *conn)
        .await
}

pub async fn set_post_tags(
    conn: &mut SqliteConnection,
    post_id: i64,
    tag_ids: &[i64],
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM post_tags WHERE post_id = ?")
        .bind(post_id)
        .execute(&mut *conn)
        .await?;

    for tag_id in tag_ids {
        sqlx::query("INSERT OR IGNORE INTO post_tags (post_id, tag_id) VALUES (?, ?)")
            .bind(post_id)
            .bind(tag_id)
            .execute(&mut *conn)
            .await?;
    }

    Ok(())
}

pub async fn get_tags_for_post(pool: &SqlitePool, post_id: i64) -> Result<Vec<Tag>, sqlx::Error> {
    sqlx::query_as::<_, Tag>(
        "SELECT t.id, t.name, t.slug, t.created_at
         FROM tags t
         JOIN post_tags pt ON pt.tag_id = t.id
         WHERE pt.post_id = ?
         ORDER BY t.name ASC",
    )
    .bind(post_id)
    .fetch_all(pool)
    .await
}

pub async fn list_published(
    pool: &SqlitePool,
    limit: i64,
    offset: i64,
) -> Result<Vec<Post>, sqlx::Error> {
    sqlx::query_as::<_, Post>(
        "SELECT id, slug, title, summary, content_md, content_html,
                featured_image_media_id, status, author_id, published_at,
                scheduled_for, created_at, updated_at, version
         FROM posts
         WHERE status = 'published'
         ORDER BY published_at DESC, created_at DESC
         LIMIT ? OFFSET ?",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
}

pub async fn count_published(pool: &SqlitePool) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT COUNT(*) FROM posts WHERE status = 'published'")
        .fetch_one(pool)
        .await
}

pub async fn search_published_fts(
    pool: &SqlitePool,
    query: &str,
    limit: i64,
    offset: i64,
) -> Result<Vec<Post>, sqlx::Error> {
    sqlx::query_as::<_, Post>(
        "SELECT p.id, p.slug, p.title, p.summary, p.content_md, p.content_html,
                p.featured_image_media_id, p.status, p.author_id, p.published_at,
                p.scheduled_for, p.created_at, p.updated_at, p.version
         FROM posts_fts f
         JOIN posts p ON p.id = f.rowid
         WHERE posts_fts MATCH ? AND p.status = 'published'
         ORDER BY rank
         LIMIT ? OFFSET ?",
    )
    .bind(query)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
}

pub async fn count_search_published_fts(
    pool: &SqlitePool,
    query: &str,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT COUNT(*)
         FROM posts_fts f
         JOIN posts p ON p.id = f.rowid
         WHERE posts_fts MATCH ? AND p.status = 'published'",
    )
    .bind(query)
    .fetch_one(pool)
    .await
}
