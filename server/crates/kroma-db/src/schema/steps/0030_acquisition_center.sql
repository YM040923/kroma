-- The acquisition centre: TMDB follow entries, 115 share-link subscriptions,
-- and the one job ledger both of them (plus one-shot gets) write into.
-- Deliberately destination-scoped: `destination` is 'cloud_intake' today;
-- 'local_library' and 'local_intake' are reserved for the two modes that are
-- not implemented yet.

-- One title the operator follows by TMDB identity. `seasons` is
-- `[{"season": 1, "start_episode": 5}]`; a season absent from the list is not
-- followed. `source_order` is `["madreel", "download"]` today.
CREATE TABLE IF NOT EXISTS watch_titles (
    id           TEXT PRIMARY KEY,
    tmdb_id      INTEGER NOT NULL,
    kind         TEXT NOT NULL,
    title        TEXT NOT NULL DEFAULT '',
    year         INTEGER,
    poster_url   TEXT,
    seasons      TEXT NOT NULL DEFAULT '[]',
    source_order TEXT NOT NULL DEFAULT '["madreel","download"]',
    enabled      INTEGER NOT NULL DEFAULT 1,
    last_checked INTEGER,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_watch_titles_tmdb
    ON watch_titles(kind, tmdb_id);

-- One 115 share link the operator watches for new files. The same title may
-- have several links; a link is the unit that updates, not the title.
CREATE TABLE IF NOT EXISTS share_subscriptions (
    id           TEXT PRIMARY KEY,
    tmdb_id      INTEGER NOT NULL,
    kind         TEXT NOT NULL,
    title        TEXT NOT NULL DEFAULT '',
    poster_url   TEXT,
    share_code   TEXT NOT NULL,
    receive_code TEXT NOT NULL DEFAULT '',
    seasons      TEXT NOT NULL DEFAULT '[]',
    enabled      INTEGER NOT NULL DEFAULT 1,
    status       TEXT NOT NULL DEFAULT 'active',
    last_checked INTEGER,
    last_error   TEXT NOT NULL DEFAULT '',
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_share_subscriptions_link
    ON share_subscriptions(share_code, receive_code);

CREATE INDEX IF NOT EXISTS idx_share_subscriptions_title
    ON share_subscriptions(kind, tmdb_id);

-- Every file a share subscription has seen, so a re-listing transfers each new
-- file once. `state` is seen / transferred / in_intake / organized / skipped.
CREATE TABLE IF NOT EXISTS share_files (
    id              TEXT PRIMARY KEY,
    subscription_id TEXT NOT NULL REFERENCES share_subscriptions(id) ON DELETE CASCADE,
    file_id         TEXT NOT NULL,
    name            TEXT NOT NULL,
    size            INTEGER,
    sha             TEXT,
    season          INTEGER,
    episode         INTEGER,
    state           TEXT NOT NULL DEFAULT 'seen',
    remote_id       TEXT,
    error           TEXT NOT NULL DEFAULT '',
    first_seen      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_share_files_ident
    ON share_files(subscription_id, file_id);

-- One job for a one-shot get, a watch attempt, or a share transfer.
-- `destination` is 'cloud_intake' (only implemented value).
-- `status` drives the UI: queued / searching / candidate_ready /
-- waiting_confirm / transferring / downloading / uploading / verifying /
-- awaiting_organize / organized / failed / cancelled.
CREATE TABLE IF NOT EXISTS acquisition_jobs (
    id             TEXT PRIMARY KEY,
    kind           TEXT NOT NULL,
    unit_key       TEXT NOT NULL DEFAULT '',
    tmdb_id        INTEGER NOT NULL DEFAULT 0,
    title          TEXT NOT NULL DEFAULT '',
    media_kind     TEXT NOT NULL DEFAULT 'movie',
    season         INTEGER,
    episode        INTEGER,
    destination    TEXT NOT NULL DEFAULT 'cloud_intake',
    source         TEXT NOT NULL DEFAULT '',
    candidate      TEXT,
    status         TEXT NOT NULL DEFAULT 'queued',
    stage_detail   TEXT NOT NULL DEFAULT '',
    price_points   INTEGER,
    price_known    INTEGER NOT NULL DEFAULT 0,
    paid_confirmed INTEGER NOT NULL DEFAULT 0,
    remote_id      TEXT NOT NULL DEFAULT '',
    progress       REAL,
    error_code     TEXT NOT NULL DEFAULT '',
    error_message  TEXT NOT NULL DEFAULT '',
    created_at     INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL,
    finished_at    INTEGER
);

CREATE INDEX IF NOT EXISTS idx_acquisition_jobs_status
    ON acquisition_jobs(status, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_acquisition_jobs_title
    ON acquisition_jobs(kind, tmdb_id, season, episode);
