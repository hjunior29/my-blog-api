use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Image,
    Video,
    Audio,
}

impl MediaKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Video => "video",
            Self::Audio => "audio",
        }
    }

    pub fn from_content_type(content_type: &str) -> Option<Self> {
        let ct = content_type.to_ascii_lowercase();
        if ct == "image/svg+xml" || ct.contains("svg") {
            return None;
        }
        if ct == "image/png" || ct == "image/jpeg" || ct == "image/jpg" || ct == "image/webp" || ct == "image/gif" || ct == "image/avif" {
            Some(Self::Image)
        } else if ct.starts_with("video/") {
            Some(Self::Video)
        } else if ct.starts_with("audio/") {
            Some(Self::Audio)
        } else {
            None
        }
    }
}

impl TryFrom<&str> for MediaKind {
    type Error = &'static str;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s.to_ascii_lowercase().as_str() {
            "image" => Ok(Self::Image),
            "video" => Ok(Self::Video),
            "audio" => Ok(Self::Audio),
            _ => Err("invalid media kind"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Media {
    pub id: String,
    pub filename: String,
    pub content_type: String,
    pub media_kind: String,
    pub size_bytes: i64,
    pub storage_key: String,
    pub storage_backend: String,
    pub public_url: String,
    pub uploader_id: i64,
    pub created_at: i64,
}
