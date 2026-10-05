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
    pub book_color: Option<&'a str>,
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
    pub book_color: Option<&'a str>,
}

pub async fn find_post_by_id(pool: &SqlitePool, id: i64) -> Result<Option<Post>, sqlx::Error> {
    sqlx::query_as::<_, Post>(
        "SELECT id, slug, title, summary, content_md, content_html,
                featured_image_media_id, status, author_id, published_at,
                scheduled_for, created_at, updated_at, version, book_color
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
                scheduled_for, created_at, updated_at, version, book_color
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
            scheduled_for, created_at, updated_at, version, book_color
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?)",
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
    .bind(record.book_color)
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
            scheduled_for = ?, updated_at = ?, version = version + 1,
            book_color = ?
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
    .bind(record.book_color)
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
                scheduled_for, created_at, updated_at, version, book_color
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
    like_query: &str,
    limit: i64,
    offset: i64,
) -> Result<Vec<Post>, sqlx::Error> {
    sqlx::query_as::<_, Post>(
        "SELECT p.id, p.slug, p.title, p.summary, p.content_md, p.content_html,
                p.featured_image_media_id, p.status, p.author_id, p.published_at,
                p.scheduled_for, p.created_at, p.updated_at, p.version, p.book_color
         FROM posts p
         WHERE p.status = 'published'
           AND (
               (p.id IN (SELECT rowid FROM posts_fts WHERE posts_fts MATCH ?))
               OR p.title LIKE ?
               OR p.summary LIKE ?
           )
         ORDER BY
           CASE
             WHEN p.title LIKE ? THEN 1
             WHEN p.summary LIKE ? THEN 2
             ELSE 3
           END,
           p.published_at DESC
         LIMIT ? OFFSET ?",
    )
    .bind(query)
    .bind(like_query)
    .bind(like_query)
    .bind(like_query)
    .bind(like_query)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
}

pub async fn count_search_published_fts(
    pool: &SqlitePool,
    query: &str,
    like_query: &str,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT COUNT(*)
         FROM posts p
         WHERE p.status = 'published'
           AND (
               (p.id IN (SELECT rowid FROM posts_fts WHERE posts_fts MATCH ?))
               OR p.title LIKE ?
               OR p.summary LIKE ?
           )",
    )
    .bind(query)
    .bind(like_query)
    .bind(like_query)
    .fetch_one(pool)
    .await
}

pub async fn list_posts_admin(
    pool: &SqlitePool,
    author_id: Option<i64>,
    status: Option<PostStatus>,
    limit: i64,
    offset: i64,
) -> Result<Vec<Post>, sqlx::Error> {
    let status_str = status.map(|s| s.to_string());
    sqlx::query_as::<_, Post>(
        "SELECT id, slug, title, summary, content_md, content_html,
                featured_image_media_id, status, author_id, published_at,
                scheduled_for, created_at, updated_at, version, book_color
         FROM posts
         WHERE (? IS NULL OR author_id = ?)
           AND (? IS NULL OR status = ?)
         ORDER BY updated_at DESC, id DESC
         LIMIT ? OFFSET ?",
    )
    .bind(author_id)
    .bind(author_id)
    .bind(status_str.as_deref())
    .bind(status_str.as_deref())
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
}

pub async fn count_posts_admin(
    pool: &SqlitePool,
    author_id: Option<i64>,
    status: Option<PostStatus>,
) -> Result<i64, sqlx::Error> {
    let status_str = status.map(|s| s.to_string());
    sqlx::query_scalar(
        "SELECT COUNT(*)
         FROM posts
         WHERE (? IS NULL OR author_id = ?)
           AND (? IS NULL OR status = ?)",
    )
    .bind(author_id)
    .bind(author_id)
    .bind(status_str.as_deref())
    .bind(status_str.as_deref())
    .fetch_one(pool)
    .await
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TagWithCount {
    pub id: i64,
    pub name: String,
    pub slug: String,
    pub post_count: i64,
}

pub async fn list_tags_with_published_count(
    pool: &SqlitePool,
) -> Result<Vec<TagWithCount>, sqlx::Error> {
    sqlx::query_as::<_, TagWithCount>(
        "SELECT t.id, t.name, t.slug, COUNT(p.id) AS post_count
         FROM tags t
         JOIN post_tags pt ON pt.tag_id = t.id
         JOIN posts p ON p.id = pt.post_id AND p.status = 'published'
         GROUP BY t.id, t.name, t.slug
         HAVING post_count > 0
         ORDER BY post_count DESC, t.name ASC",
    )
    .fetch_all(pool)
    .await
}

pub async fn delete_post_with_version(
    pool: &SqlitePool,
    id: i64,
    expected_version: i64,
) -> Result<bool, sqlx::Error> {
    let res = sqlx::query("DELETE FROM posts WHERE id = ? AND version = ?")
        .bind(id)
        .bind(expected_version)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

pub async fn find_post_draft(
    pool: &SqlitePool,
    post_id: i64,
) -> Result<Option<super::model::PostDraft>, sqlx::Error> {
    sqlx::query_as::<_, super::model::PostDraft>(
        "SELECT post_id, title, summary, content_md, content_html,
                featured_image_media_id, book_color, tags, updated_at
         FROM post_drafts WHERE post_id = ?",
    )
    .bind(post_id)
    .fetch_optional(pool)
    .await
}

pub async fn upsert_post_draft(
    conn: &mut SqliteConnection,
    draft: &super::model::PostDraft,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO post_drafts (
            post_id, title, summary, content_md, content_html,
            featured_image_media_id, book_color, tags, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(post_id) DO UPDATE SET
            title = excluded.title,
            summary = excluded.summary,
            content_md = excluded.content_md,
            content_html = excluded.content_html,
            featured_image_media_id = excluded.featured_image_media_id,
            book_color = excluded.book_color,
            tags = excluded.tags,
            updated_at = excluded.updated_at",
    )
    .bind(draft.post_id)
    .bind(&draft.title)
    .bind(&draft.summary)
    .bind(&draft.content_md)
    .bind(&draft.content_html)
    .bind(&draft.featured_image_media_id)
    .bind(&draft.book_color)
    .bind(&draft.tags)
    .bind(draft.updated_at)
    .execute(conn)
    .await?;

    Ok(())
}

pub async fn delete_post_draft(
    conn: &mut SqliteConnection,
    post_id: i64,
) -> Result<bool, sqlx::Error> {
    let res = sqlx::query("DELETE FROM post_drafts WHERE post_id = ?")
        .bind(post_id)
        .execute(conn)
        .await?;
    Ok(res.rows_affected() > 0)
}
