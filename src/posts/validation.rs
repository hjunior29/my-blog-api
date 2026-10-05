use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

use super::model::PostStatus;

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
                Some(format!("\"{}\"*", sanitized))
            }
        })
        .collect();
    words.join(" ")
}
