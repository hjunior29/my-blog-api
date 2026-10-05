CREATE TABLE IF NOT EXISTS media (
    id TEXT PRIMARY KEY NOT NULL,
    filename TEXT NOT NULL,
    content_type TEXT NOT NULL,
    media_kind TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    storage_key TEXT NOT NULL,
    storage_backend TEXT NOT NULL,
    public_url TEXT NOT NULL,
    uploader_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_media_uploader ON media (uploader_id);
CREATE INDEX IF NOT EXISTS idx_media_kind ON media (media_kind);
CREATE INDEX IF NOT EXISTS idx_media_created ON media (created_at DESC);
