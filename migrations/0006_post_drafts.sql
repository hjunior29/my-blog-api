CREATE TABLE IF NOT EXISTS post_drafts (
    post_id INTEGER PRIMARY KEY REFERENCES posts(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    summary TEXT NOT NULL DEFAULT '',
    content_md TEXT NOT NULL,
    content_html TEXT NOT NULL,
    featured_image_media_id TEXT,
    book_color TEXT,
    tags TEXT NOT NULL DEFAULT '[]',
    updated_at INTEGER NOT NULL
);
