CREATE TABLE IF NOT EXISTS art_overrides (
    subject_kind TEXT NOT NULL,          -- 'item' | 'show'
    subject_id   TEXT NOT NULL,
    art          TEXT NOT NULL,          -- 'poster' | 'backdrop' | 'logo'
    url          TEXT NOT NULL,
    source       TEXT NOT NULL DEFAULT 'tmdb',
    updated_at   INTEGER NOT NULL,
    PRIMARY KEY (subject_kind, subject_id, art)
);
