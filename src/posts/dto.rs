use serde::{Deserialize, Serialize};

use super::model::{PostStatus, PostWithTags, Tag};

#[derive(Debug, Serialize, Deserialize)]
pub struct CreatePostDto {
    pub title: String,
    pub summary: Option<String>,
    pub content_md: String,
    pub featured_image_media_id: Option<String>,
    pub status: Option<PostStatus>,
    pub tags: Option<Vec<String>>,
    pub scheduled_for: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdatePostDto {
    pub title: Option<String>,
    pub summary: Option<String>,
    pub content_md: Option<String>,
    pub featured_image_media_id: Option<String>,
    pub status: Option<PostStatus>,
    pub tags: Option<Vec<String>>,
    pub scheduled_for: Option<i64>,
    pub version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagDto {
    pub id: i64,
    pub name: String,
    pub slug: String,
}

impl From<&Tag> for TagDto {
    fn from(t: &Tag) -> Self {
        Self {
            id: t.id,
            name: t.name.clone(),
            slug: t.slug.clone(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PostResponse {
    pub id: i64,
    pub slug: String,
    pub title: String,
    pub summary: String,
    pub content_md: String,
    pub content_html: String,
    pub featured_image_media_id: Option<String>,
    pub status: PostStatus,
    pub author_id: i64,
    pub published_at: Option<i64>,
    pub scheduled_for: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
    pub version: i64,
    pub tags: Vec<TagDto>,
}

impl From<&PostWithTags> for PostResponse {
    fn from(pt: &PostWithTags) -> Self {
        Self {
            id: pt.post.id,
            slug: pt.post.slug.clone(),
            title: pt.post.title.clone(),
            summary: pt.post.summary.clone(),
            content_md: pt.post.content_md.clone(),
            content_html: pt.post.content_html.clone(),
            featured_image_media_id: pt.post.featured_image_media_id.clone(),
            status: pt.post.status,
            author_id: pt.post.author_id,
            published_at: pt.post.published_at,
            scheduled_for: pt.post.scheduled_for,
            created_at: pt.post.created_at,
            updated_at: pt.post.updated_at,
            version: pt.post.version,
            tags: pt.tags.iter().map(TagDto::from).collect(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PostSummaryResponse {
    pub id: i64,
    pub slug: String,
    pub title: String,
    pub summary: String,
    pub featured_image_media_id: Option<String>,
    pub status: PostStatus,
    pub author_id: i64,
    pub published_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
    pub version: i64,
    pub tags: Vec<TagDto>,
}

impl From<&PostWithTags> for PostSummaryResponse {
    fn from(pt: &PostWithTags) -> Self {
        Self {
            id: pt.post.id,
            slug: pt.post.slug.clone(),
            title: pt.post.title.clone(),
            summary: pt.post.summary.clone(),
            featured_image_media_id: pt.post.featured_image_media_id.clone(),
            status: pt.post.status,
            author_id: pt.post.author_id,
            published_at: pt.post.published_at,
            created_at: pt.post.created_at,
            updated_at: pt.post.updated_at,
            version: pt.post.version,
            tags: pt.tags.iter().map(TagDto::from).collect(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PostListResponse {
    pub items: Vec<PostSummaryResponse>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: String,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}
