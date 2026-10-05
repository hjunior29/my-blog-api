use serde::{Deserialize, Serialize};

use super::model::{Media, MediaKind};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaResponse {
    pub id: String,
    pub filename: String,
    pub content_type: String,
    pub media_kind: MediaKind,
    pub size_bytes: i64,
    pub public_url: String,
    pub uploader_id: i64,
    pub created_at: i64,
}

impl From<&Media> for MediaResponse {
    fn from(m: &Media) -> Self {
        let media_kind = MediaKind::try_from(m.media_kind.as_str()).unwrap_or(MediaKind::Image);
        Self {
            id: m.id.clone(),
            filename: m.filename.clone(),
            content_type: m.content_type.clone(),
            media_kind,
            size_bytes: m.size_bytes,
            public_url: m.public_url.clone(),
            uploader_id: m.uploader_id,
            created_at: m.created_at,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MediaListResponse {
    pub items: Vec<MediaResponse>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Deserialize)]
pub struct ListMediaQuery {
    pub kind: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}
