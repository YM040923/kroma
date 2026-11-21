-- One logical thing a viewer wants: a movie, or one episode of a show.
-- Requests and wanted rows stay the demand layer; this table is the stable
-- identity that locations and provider attempts hang off.
CREATE TABLE IF NOT EXISTS media_units (
    unit_key   TEXT PRIMARY KEY,
    kind       TEXT NOT NULL,
    tmdb_id    INTEGER NOT NULL,
    season     INTEGER NOT NULL DEFAULT 0,
    episode    INTEGER NOT NULL DEFAULT 0,
    title      TEXT NOT NULL DEFAULT '',
    year       INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_media_units_ident
    ON media_units(kind, tmdb_id, season, episode);

-- Where the bytes for a unit actually live. A unit may have a local file, a
-- 115 replica, or both. `state` is the verification state of this location,
-- not the acquisition status.
CREATE TABLE IF NOT EXISTS media_locations (
    id          TEXT PRIMARY KEY,
    unit_key    TEXT NOT NULL REFERENCES media_units(unit_key) ON DELETE CASCADE,
    provider    TEXT NOT NULL,
    account_id  TEXT NOT NULL DEFAULT '',
    path        TEXT NOT NULL DEFAULT '',
    remote_id   TEXT NOT NULL DEFAULT '',
    origin      TEXT NOT NULL DEFAULT '',
    state       TEXT NOT NULL DEFAULT 'pending',
    is_primary  INTEGER NOT NULL DEFAULT 0,
    size        INTEGER,
    fingerprint TEXT,
    quality     TEXT,
    verified_at INTEGER,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_media_locations_unit
    ON media_locations(unit_key, provider, state);

-- One attempt by one provider to acquire a unit. History is kept; the partial
-- unique index allows only one live attempt per unit, so BT and 115 cannot run
-- against the same unit at once.
CREATE TABLE IF NOT EXISTS acquisition_attempts (
    id            TEXT PRIMARY KEY,
    unit_key      TEXT NOT NULL REFERENCES media_units(unit_key) ON DELETE CASCADE,
    request_id    TEXT REFERENCES requests(id) ON DELETE SET NULL,
    provider      TEXT NOT NULL,
    status        TEXT NOT NULL DEFAULT 'queued',
    error_code    TEXT NOT NULL DEFAULT '',
    error_message TEXT NOT NULL DEFAULT '',
    candidate     TEXT,
    quote         TEXT,
    job_id        TEXT NOT NULL DEFAULT '',
    cost_points   INTEGER NOT NULL DEFAULT 0,
    unlock_key    TEXT,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL,
    finished_at   INTEGER
);

CREATE INDEX IF NOT EXISTS idx_acquisition_attempts_unit
    ON acquisition_attempts(unit_key, created_at DESC);

CREATE UNIQUE INDEX IF NOT EXISTS idx_acquisition_attempts_active
    ON acquisition_attempts(unit_key)
    WHERE status IN (
        'queued', 'searching', 'candidate_ready', 'waiting_confirm',
        'transferring', 'downloading', 'importing', 'verifying'
    );