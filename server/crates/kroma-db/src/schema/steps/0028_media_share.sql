CREATE TABLE media_share (
    id                TEXT PRIMARY KEY,
    slug              TEXT NOT NULL UNIQUE,
    item_id           TEXT NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    created_by        TEXT NOT NULL,
    password_hash     TEXT,
    password_version  INTEGER NOT NULL DEFAULT 1,
    access_key        TEXT NOT NULL,
    expires_at        INTEGER NOT NULL,
    revoked_at        INTEGER,
    view_count        INTEGER NOT NULL DEFAULT 0,
    last_accessed_at  INTEGER,
    created_at        INTEGER NOT NULL
);
CREATE INDEX idx_media_share_item ON media_share(item_id);
