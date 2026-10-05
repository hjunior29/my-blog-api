use sqlx::SqlitePool;

use super::{
    dto::{PostResponse, TagDto, UpdatePostDto},
    markdown,
    model::{PostDraft, PostStatus, PostWithTags},
    repository::{self, UpdatePostRecord},
    slug,
    validation::{
        current_unix_time, normalize_tags, validate_content, validate_summary, validate_title,
        PostServiceError,
    },
};

pub async fn get_admin_post_by_id(
    pool: &SqlitePool,
    id: i64,
) -> Result<Option<PostResponse>, PostServiceError> {
    let pwt = match super::service::get_post_by_id(pool, id).await? {
        Some(p) => p,
        None => return Ok(None),
    };
    let mut resp = PostResponse::from(&pwt);
    if let Some(draft) = repository::find_post_draft(pool, id).await? {
        let tags: Vec<String> = serde_json::from_str(&draft.tags).unwrap_or_default();
        resp.title = draft.title;
        resp.summary = draft.summary;
        resp.content_md = draft.content_md;
        resp.content_html = draft.content_html;
        resp.featured_image_media_id = draft.featured_image_media_id;
        resp.book_color = draft.book_color;
        resp.tags = tags
            .into_iter()
            .map(|name| {
                let slug = slug::generate_slug(&name);
                TagDto { id: 0, name, slug }
            })
            .collect();
        resp.has_draft = true;
    }
    Ok(Some(resp))
}

pub async fn save_post_draft(
    pool: &SqlitePool,
    id: i64,
    dto: UpdatePostDto,
) -> Result<PostResponse, PostServiceError> {
    let existing = repository::find_post_by_id(pool, id)
        .await?
        .ok_or(PostServiceError::PostNotFound)?;

    let title = match dto.title {
        Some(ref t) => validate_title(t)?.to_string(),
        None => existing.title.clone(),
    };
    let summary = match dto.summary {
        Some(ref s) => validate_summary(s)?.to_string(),
        None => existing.summary.clone(),
    };
    let content_md = dto.content_md.unwrap_or(existing.content_md);
    let content_html = markdown::render_and_sanitize(&content_md);
    let featured_image = dto
        .featured_image_media_id
        .or(existing.featured_image_media_id);
    let book_color = match dto.book_color {
        Some(c) if !c.trim().is_empty() => Some(c.trim().to_string()),
        _ => existing.book_color,
    };
    let tags_vec = match dto.tags {
        Some(ref raw) => normalize_tags(raw)?,
        None => repository::get_tags_for_post(pool, id)
            .await?
            .into_iter()
            .map(|t| t.name)
            .collect(),
    };
    let tags_json = serde_json::to_string(&tags_vec).unwrap_or_else(|_| "[]".to_string());
    let now = current_unix_time();

    let draft = PostDraft {
        post_id: id,
        title: title.clone(),
        summary: summary.clone(),
        content_md: content_md.clone(),
        content_html: content_html.clone(),
        featured_image_media_id: featured_image.clone(),
        book_color: book_color.clone(),
        tags: tags_json,
        updated_at: now,
    };

    let mut conn = pool.acquire().await?;
    repository::upsert_post_draft(&mut conn, &draft).await?;

    let tag_dtos: Vec<TagDto> = tags_vec
        .into_iter()
        .map(|name| {
            let slug = slug::generate_slug(&name);
            TagDto { id: 0, name, slug }
        })
        .collect();

    Ok(PostResponse {
        id,
        slug: existing.slug,
        title,
        summary,
        content_md,
        content_html,
        featured_image_media_id: featured_image,
        status: existing.status,
        author_id: existing.author_id,
        published_at: existing.published_at,
        scheduled_for: existing.scheduled_for,
        created_at: existing.created_at,
        updated_at: now,
        version: existing.version,
        book_color,
        tags: tag_dtos,
        has_draft: true,
    })
}

pub async fn discard_post_draft(
    pool: &SqlitePool,
    id: i64,
) -> Result<PostResponse, PostServiceError> {
    let mut conn = pool.acquire().await?;
    repository::delete_post_draft(&mut conn, id).await?;
    let pwt = super::service::get_post_by_id(pool, id)
        .await?
        .ok_or(PostServiceError::PostNotFound)?;
    let mut resp = PostResponse::from(&pwt);
    resp.has_draft = false;
    Ok(resp)
}

pub async fn try_publish_draft(
    pool: &SqlitePool,
    id: i64,
    version: i64,
) -> Result<Option<PostWithTags>, PostServiceError> {
    let draft = match repository::find_post_draft(pool, id).await? {
        Some(d) => d,
        None => return Ok(None),
    };
    let existing = repository::find_post_by_id(pool, id)
        .await?
        .ok_or(PostServiceError::PostNotFound)?;
    if existing.version != version {
        return Err(PostServiceError::VersionConflict);
    }
    validate_title(&draft.title)?;
    validate_summary(&draft.summary)?;
    validate_content(&draft.content_md, PostStatus::Published)?;
    let now = current_unix_time();
    let tags: Vec<String> = serde_json::from_str(&draft.tags).unwrap_or_default();
    let clean_tags = normalize_tags(&tags)?;
    let mut tx = pool.begin().await?;
    let record = UpdatePostRecord {
        id,
        title: &draft.title,
        summary: &draft.summary,
        content_md: &draft.content_md,
        content_html: &draft.content_html,
        featured_image_media_id: draft.featured_image_media_id.as_deref(),
        status: PostStatus::Published,
        published_at: existing.published_at.or(Some(now)),
        scheduled_for: existing.scheduled_for,
        now,
        expected_version: version,
        book_color: draft.book_color.as_deref(),
    };
    let updated = repository::update_post(&mut tx, &record).await?;
    if !updated {
        return Err(PostServiceError::VersionConflict);
    }
    let mut tag_ids = Vec::with_capacity(clean_tags.len());
    for tag_name in &clean_tags {
        let tag_slug = slug::generate_slug(tag_name);
        let tag = repository::find_or_create_tag(&mut tx, tag_name, &tag_slug, now).await?;
        tag_ids.push(tag.id);
    }
    tag_ids.sort_unstable();
    tag_ids.dedup();
    repository::set_post_tags(&mut tx, id, &tag_ids).await?;
    repository::delete_post_draft(&mut tx, id).await?;
    tx.commit().await?;

    let post = repository::find_post_by_id(pool, id)
        .await?
        .ok_or(PostServiceError::PostNotFound)?;
    let tags = repository::get_tags_for_post(pool, id).await?;
    Ok(Some(PostWithTags { post, tags }))
}
