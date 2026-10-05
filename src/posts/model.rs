use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum PostStatus {
    Draft,
    Published,
    Archived,
}

impl fmt::Display for PostStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Draft => write!(f, "draft"),
            Self::Published => write!(f, "published"),
            Self::Archived => write!(f, "archived"),
        }
    }
}

impl std::str::FromStr for PostStatus {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "draft" => Ok(Self::Draft),
            "published" => Ok(Self::Published),
            "archived" => Ok(Self::Archived),
            _ => Err("invalid post status"),
        }
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Post {
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
    pub book_color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Tag {
    pub id: i64,
    pub name: String,
    pub slug: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PostTag {
    pub post_id: i64,
    pub tag_id: i64,
}

#[derive(Debug, Clone)]
pub struct PostWithTags {
    pub post: Post,
    pub tags: Vec<Tag>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PostDraft {
    pub post_id: i64,
    pub title: String,
    pub summary: String,
    pub content_md: String,
    pub content_html: String,
    pub featured_image_media_id: Option<String>,
    pub book_color: Option<String>,
    pub tags: String,
    pub updated_at: i64,
}
