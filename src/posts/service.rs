use std::time::{SystemTime, UNIX_EPOCH};

use sqlx::SqlitePool;
use thiserror::Error;

use super::{
    dto::{CreatePostDto, PostListResponse, PostSummaryResponse, UpdatePostDto},
    markdown,
    model::{PostStatus, PostWithTags},
    repository::{self, NewPostRecord, UpdatePostRecord},
    slug,
};

#[derive(Debug, Error)]
pub enum PostServiceError {
    #[error("title must be between 1 and 160 characters")]
    InvalidTitle,
    #[error("summary must be at most 320 characters")]
    InvalidSummary,
    #[error("content exceeds maximum allowed size of 256 KiB")]
    ContentTooLarge,
    #[error("content cannot be empty when publishing")]
    EmptyContentWhenPublished,
    #[error("post can have at most 10 tags")]
    TooManyTags,
    #[error("tag name must be between 1 and 40 characters")]
    InvalidTagName,
    #[error("post not found")]
    PostNotFound,
    #[error("slug collision: could not allocate unique slug")]
    SlugCollision,
    #[error("version conflict: post was modified by another request")]
    VersionConflict,
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

pub fn current_unix_time() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub fn validate_title(title: &str) -> Result<&str, PostServiceError> {
    let trimmed = title.trim();
    let char_count = trimmed.chars().count();
    if char_count == 0 || char_count > 160 {
        return Err(PostServiceError::InvalidTitle);
    }
    Ok(trimmed)
}

pub fn validate_summary(summary: &str) -> Result<&str, PostServiceError> {
    let trimmed = summary.trim();
    if trimmed.chars().count() > 320 {
        return Err(PostServiceError::InvalidSummary);
    }
    Ok(trimmed)
}

pub fn validate_content(content: &str, status: PostStatus) -> Result<(), PostServiceError> {
    if content.len() > 256 * 1024 {
        return Err(PostServiceError::ContentTooLarge);
    }
    if status == PostStatus::Published && content.trim().is_empty() {
        return Err(PostServiceError::EmptyContentWhenPublished);
    }
    Ok(())
}

pub fn normalize_tags(raw_tags: &[String]) -> Result<Vec<String>, PostServiceError> {
    let mut normalized = Vec::new();
    for raw in raw_tags {
        let trimmed = raw.trim();
        let char_count = trimmed.chars().count();
        if char_count == 0 || char_count > 40 {
            return Err(PostServiceError::InvalidTagName);
        }
        let lower = trimmed.to_lowercase();
        if !normalized
            .iter()
            .any(|t: &String| t.to_lowercase() == lower)
        {
            normalized.push(trimmed.to_string());
        }
    }
    if normalized.len() > 10 {
        return Err(PostServiceError::TooManyTags);
    }
    Ok(normalized)
}

pub fn sanitize_fts_query(raw: &str) -> String {
    let words: Vec<String> = raw
        .split_whitespace()
        .filter_map(|w| {
            let sanitized: String = w
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                .collect();
            if sanitized.is_empty() {
                None
            } else {
                Some(format!("\"{}\"", sanitized))
            }
        })
        .collect();
    words.join(" ")
}

pub async fn create_post(
    pool: &SqlitePool,
    author_id: i64,
    dto: CreatePostDto,
) -> Result<PostWithTags, PostServiceError> {
    let title = validate_title(&dto.title)?;
    let summary = dto.summary.as_deref().unwrap_or("");
    let summary = validate_summary(summary)?;
    let status = dto.status.unwrap_or(PostStatus::Draft);
    validate_content(&dto.content_md, status)?;

    let clean_tags = match dto.tags {
        Some(ref tags) => normalize_tags(tags)?,
        None => Vec::new(),
    };

    let base_slug = slug::generate_slug(title);
    let content_html = markdown::render_and_sanitize(&dto.content_md);
    let now = current_unix_time();
    let published_at = if status == PostStatus::Published {
        Some(now)
    } else {
        None
    };

    let mut attempt = 0;
    let mut current_slug = base_slug.clone();

    loop {
        if attempt > 0 {
            current_slug = format!("{}-{}", base_slug, attempt + 1);
        }
        attempt += 1;
        if attempt > 100 {
            return Err(PostServiceError::SlugCollision);
        }

        let exists = repository::slug_exists(pool, &current_slug).await?;
        if exists {
            continue;
        }

        let mut tx = pool.begin().await?;
        let record = NewPostRecord {
            slug: &current_slug,
            title,
            summary,
            content_md: &dto.content_md,
            content_html: &content_html,
            featured_image_media_id: dto.featured_image_media_id.as_deref(),
            status,
            author_id,
            published_at,
            scheduled_for: dto.scheduled_for,
            now,
        };

        match repository::insert_post(&mut tx, &record).await {
            Ok(post_id) => {
                let mut tag_ids = Vec::with_capacity(clean_tags.len());
                for tag_name in &clean_tags {
                    let tag_slug = slug::generate_slug(tag_name);
                    let tag =
                        repository::find_or_create_tag(&mut tx, tag_name, &tag_slug, now).await?;
                    tag_ids.push(tag.id);
                }
                tag_ids.sort_unstable();
                tag_ids.dedup();
                repository::set_post_tags(&mut tx, post_id, &tag_ids).await?;
                tx.commit().await?;

                let post = repository::find_post_by_id(pool, post_id)
                    .await?
                    .ok_or(PostServiceError::PostNotFound)?;
                let tags = repository::get_tags_for_post(pool, post_id).await?;
                return Ok(PostWithTags { post, tags });
            }
            Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                continue;
            }
            Err(err) => return Err(PostServiceError::Database(err)),
        }
    }
}

pub async fn update_post(
    pool: &SqlitePool,
    post_id: i64,
    dto: UpdatePostDto,
) -> Result<PostWithTags, PostServiceError> {
    let existing = repository::find_post_by_id(pool, post_id)
        .await?
        .ok_or(PostServiceError::PostNotFound)?;

    let title = match dto.title {
        Some(ref t) => validate_title(t)?,
        None => &existing.title,
    };

    let summary = match dto.summary {
        Some(ref s) => validate_summary(s)?,
        None => &existing.summary,
    };

    let status = dto.status.unwrap_or(existing.status);
    let content_md = dto.content_md.as_ref().unwrap_or(&existing.content_md);
    validate_content(content_md, status)?;

    let clean_tags = match dto.tags {
        Some(ref tags) => Some(normalize_tags(tags)?),
        None => None,
    };

    let content_html = match dto.content_md {
        Some(ref md) => markdown::render_and_sanitize(md),
        None => existing.content_html.clone(),
    };

    let now = current_unix_time();
    let published_at = match status {
        PostStatus::Published => existing.published_at.or(Some(now)),
        PostStatus::Archived => existing.published_at,
        PostStatus::Draft | PostStatus::Scheduled => None,
    };

    let featured_image = dto
        .featured_image_media_id
        .as_deref()
        .or(existing.featured_image_media_id.as_deref());

    let mut tx = pool.begin().await?;
    let record = UpdatePostRecord {
        id: post_id,
        title,
        summary,
        content_md,
        content_html: &content_html,
        featured_image_media_id: featured_image,
        status,
        published_at,
        scheduled_for: dto.scheduled_for.or(existing.scheduled_for),
        now,
        expected_version: dto.version,
    };

    let updated = repository::update_post(&mut tx, &record).await?;
    if !updated {
        return Err(PostServiceError::VersionConflict);
    }

    if let Some(tags) = clean_tags {
        let mut tag_ids = Vec::with_capacity(tags.len());
        for tag_name in &tags {
            let tag_slug = slug::generate_slug(tag_name);
            let tag = repository::find_or_create_tag(&mut tx, tag_name, &tag_slug, now).await?;
            tag_ids.push(tag.id);
        }
        tag_ids.sort_unstable();
        tag_ids.dedup();
        repository::set_post_tags(&mut tx, post_id, &tag_ids).await?;
    }

    tx.commit().await?;

    let post = repository::find_post_by_id(pool, post_id)
        .await?
        .ok_or(PostServiceError::PostNotFound)?;
    let tags = repository::get_tags_for_post(pool, post_id).await?;
    Ok(PostWithTags { post, tags })
}

pub async fn get_post_by_id(
    pool: &SqlitePool,
    id: i64,
) -> Result<Option<PostWithTags>, PostServiceError> {
    let post = match repository::find_post_by_id(pool, id).await? {
        Some(p) => p,
        None => return Ok(None),
    };
    let tags = repository::get_tags_for_post(pool, id).await?;
    Ok(Some(PostWithTags { post, tags }))
}

pub async fn get_post_by_slug(
    pool: &SqlitePool,
    slug: &str,
) -> Result<Option<PostWithTags>, PostServiceError> {
    let post = match repository::find_post_by_slug(pool, slug).await? {
        Some(p) => p,
        None => return Ok(None),
    };
    let tags = repository::get_tags_for_post(pool, post.id).await?;
    Ok(Some(PostWithTags { post, tags }))
}

pub async fn delete_post(pool: &SqlitePool, id: i64) -> Result<bool, PostServiceError> {
    let deleted = repository::delete_post(pool, id).await?;
    Ok(deleted)
}

pub async fn list_published_posts(
    pool: &SqlitePool,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<PostListResponse, PostServiceError> {
    let limit = limit.unwrap_or(20).clamp(1, 50);
    let offset = offset.unwrap_or(0).max(0);

    let total = repository::count_published(pool).await?;
    let posts = repository::list_published(pool, limit, offset).await?;

    let mut items = Vec::with_capacity(posts.len());
    for post in posts {
        let tags = repository::get_tags_for_post(pool, post.id).await?;
        let pwt = PostWithTags { post, tags };
        items.push(PostSummaryResponse::from(&pwt));
    }

    Ok(PostListResponse {
        items,
        total,
        limit,
        offset,
    })
}

pub async fn search_published_posts(
    pool: &SqlitePool,
    raw_query: &str,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<PostListResponse, PostServiceError> {
    let limit = limit.unwrap_or(20).clamp(1, 50);
    let offset = offset.unwrap_or(0).max(0);

    let sanitized = sanitize_fts_query(raw_query);
    if sanitized.is_empty() {
        return Ok(PostListResponse {
            items: Vec::new(),
            total: 0,
            limit,
            offset,
        });
    }

    let total = repository::count_search_published_fts(pool, &sanitized).await?;
    let posts = repository::search_published_fts(pool, &sanitized, limit, offset).await?;

    let mut items = Vec::with_capacity(posts.len());
    for post in posts {
        let tags = repository::get_tags_for_post(pool, post.id).await?;
        let pwt = PostWithTags { post, tags };
        items.push(PostSummaryResponse::from(&pwt));
    }

    Ok(PostListResponse {
        items,
        total,
        limit,
        offset,
    })
}

pub fn preview_markdown(markdown: &str) -> String {
    markdown::render_and_sanitize(markdown)
}

pub async fn list_published_tags(
    pool: &SqlitePool,
) -> Result<Vec<super::dto::TagWithCountDto>, PostServiceError> {
    let tags = repository::list_tags_with_published_count(pool).await?;
    Ok(tags
        .into_iter()
        .map(|t| super::dto::TagWithCountDto {
            id: t.id,
            name: t.name,
            slug: t.slug,
            post_count: t.post_count,
        })
        .collect())
}

pub async fn list_admin_posts(
    pool: &SqlitePool,
    author_id: Option<i64>,
    status: Option<PostStatus>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<PostListResponse, PostServiceError> {
    let limit = limit.unwrap_or(20).clamp(1, 50);
    let offset = offset.unwrap_or(0).max(0);

    let total = repository::count_posts_admin(pool, author_id, status).await?;
    let posts = repository::list_posts_admin(pool, author_id, status, limit, offset).await?;

    let mut items = Vec::with_capacity(posts.len());
    for post in posts {
        let tags = repository::get_tags_for_post(pool, post.id).await?;
        let pwt = PostWithTags { post, tags };
        items.push(PostSummaryResponse::from(&pwt));
    }

    Ok(PostListResponse {
        items,
        total,
        limit,
        offset,
    })
}

pub async fn publish_post(
    pool: &SqlitePool,
    id: i64,
    expected_version: i64,
) -> Result<PostWithTags, PostServiceError> {
    update_post(
        pool,
        id,
        UpdatePostDto {
            title: None,
            summary: None,
            content_md: None,
            featured_image_media_id: None,
            status: Some(PostStatus::Published),
            tags: None,
            scheduled_for: None,
            version: expected_version,
        },
    )
    .await
}

pub async fn unpublish_post(
    pool: &SqlitePool,
    id: i64,
    expected_version: i64,
) -> Result<PostWithTags, PostServiceError> {
    update_post(
        pool,
        id,
        UpdatePostDto {
            title: None,
            summary: None,
            content_md: None,
            featured_image_media_id: None,
            status: Some(PostStatus::Draft),
            tags: None,
            scheduled_for: None,
            version: expected_version,
        },
    )
    .await
}

pub async fn delete_post_versioned(
    pool: &SqlitePool,
    id: i64,
    expected_version: i64,
) -> Result<(), PostServiceError> {
    let post = repository::find_post_by_id(pool, id)
        .await?
        .ok_or(PostServiceError::PostNotFound)?;

    if post.version != expected_version {
        return Err(PostServiceError::VersionConflict);
    }

    let deleted = repository::delete_post_with_version(pool, id, expected_version).await?;
    if !deleted {
        return Err(PostServiceError::VersionConflict);
    }
    Ok(())
}
