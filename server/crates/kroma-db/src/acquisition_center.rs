//! Read/write for the acquisition centre: watch titles, share subscriptions,
//! the files a share has listed, and the job ledger.

use kroma_domain::acquisition_center::{
    AcquisitionJob, Destination, SeasonStart, ShareFile, ShareSubscription, WatchTitle,
};

use super::{params, Connection, Pool, Result, Row};

fn season_starts(raw: Option<String>) -> Vec<SeasonStart> {
    raw.and_then(|value| serde_json::from_str(&value).ok())
        .unwrap_or_default()
}

fn watch_from_row(row: &Row<'_>) -> rusqlite::Result<WatchTitle> {
    Ok(WatchTitle {
        id: row.get(0)?,
        tmdb_id: row.get::<_, i64>(1)? as u64,
        kind: row.get(2)?,
        title: row.get(3)?,
        year: row.get::<_, Option<i64>>(4)?.map(|value| value as u32),
        poster_url: row.get(5)?,
        seasons: season_starts(row.get(6)?),
        source_order: row
            .get::<_, Option<String>>(7)?
            .and_then(|value| serde_json::from_str(&value).ok())
            .unwrap_or_default(),
        enabled: row.get::<_, i64>(8)? != 0,
        last_checked: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

fn share_subscription_from_row(row: &Row<'_>) -> rusqlite::Result<ShareSubscription> {
    Ok(ShareSubscription {
        id: row.get(0)?,
        tmdb_id: row.get::<_, i64>(1)? as u64,
        kind: row.get(2)?,
        title: row.get(3)?,
        poster_url: row.get(4)?,
        share_code: row.get(5)?,
        receive_code: row.get(6)?,
        seasons: season_starts(row.get(7)?),
        enabled: row.get::<_, i64>(8)? != 0,
        status: row.get(9)?,
        last_checked: row.get(10)?,
        last_error: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
    })
}

fn share_file_from_row(row: &Row<'_>) -> rusqlite::Result<ShareFile> {
    Ok(ShareFile {
        id: row.get(0)?,
        subscription_id: row.get(1)?,
        file_id: row.get(2)?,
        name: row.get(3)?,
        size: row.get(4)?,
        sha: row.get(5)?,
        season: row.get::<_, Option<i64>>(6)?.map(|value| value as u32),
        episode: row.get::<_, Option<i64>>(7)?.map(|value| value as u32),
        state: row.get(8)?,
        remote_id: row.get(9)?,
        error: row.get(10)?,
        first_seen: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

fn job_from_row(row: &Row<'_>) -> rusqlite::Result<AcquisitionJob> {
    let destination = row
        .get::<_, Option<String>>(8)?
        .and_then(|value| Destination::parse(&value))
        .unwrap_or_default();
    Ok(AcquisitionJob {
        id: row.get(0)?,
        kind: row.get(1)?,
        unit_key: row.get(2)?,
        tmdb_id: row.get::<_, i64>(3)? as u64,
        title: row.get(4)?,
        media_kind: row.get(5)?,
        season: row.get::<_, Option<i64>>(6)?.map(|value| value as u32),
        episode: row.get::<_, Option<i64>>(7)?.map(|value| value as u32),
        destination,
        source: row.get(9)?,
        candidate: row
            .get::<_, Option<String>>(10)?
            .and_then(|value| serde_json::from_str(&value).ok()),
        status: row.get(11)?,
        stage_detail: row.get(12)?,
        price_points: row
            .get::<_, Option<i64>>(13)?
            .map(|value| value.max(0) as u32),
        price_known: row.get::<_, i64>(14)? != 0,
        paid_confirmed: row.get::<_, i64>(15)? != 0,
        remote_id: row.get(16)?,
        progress: row.get(17)?,
        error_code: row.get(18)?,
        error_message: row.get(19)?,
        created_at: row.get(20)?,
        updated_at: row.get(21)?,
        finished_at: row.get(22)?,
    })
}

const WATCH_COLS: &str = "id, tmdb_id, kind, title, year, poster_url, seasons, source_order, \
    enabled, last_checked, created_at, updated_at";
const SHARE_COLS: &str = "id, tmdb_id, kind, title, poster_url, share_code, receive_code, \
    seasons, enabled, status, last_checked, last_error, created_at, updated_at";
const SHARE_FILE_COLS: &str = "id, subscription_id, file_id, name, size, sha, season, episode, \
    state, remote_id, error, first_seen, updated_at";
const JOB_COLS: &str = "id, kind, unit_key, tmdb_id, title, media_kind, season, episode, \
    destination, source, candidate, status, stage_detail, price_points, price_known, \
    paid_confirmed, remote_id, progress, error_code, error_message, created_at, updated_at, \
    finished_at";

pub fn upsert_watch_title(conn: &Connection, watch: &WatchTitle) -> Result<()> {
    let seasons = serde_json::to_string(&watch.seasons)?;
    let order = serde_json::to_string(&watch.source_order)?;
    conn.execute(
        "INSERT INTO watch_titles \
            (id, tmdb_id, kind, title, year, poster_url, seasons, source_order, enabled, \
             last_checked, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12) \
         ON CONFLICT(kind, tmdb_id) DO UPDATE SET \
             title = excluded.title, year = excluded.year, poster_url = excluded.poster_url, \
             seasons = excluded.seasons, source_order = excluded.source_order, \
             enabled = excluded.enabled, updated_at = excluded.updated_at",
        params![
            watch.id,
            watch.tmdb_id as i64,
            watch.kind,
            watch.title,
            watch.year.map(|value| value as i64),
            watch.poster_url,
            seasons,
            order,
            i64::from(watch.enabled),
            watch.last_checked,
            watch.created_at,
            watch.updated_at,
        ],
    )?;
    Ok(())
}

pub fn get_watch_title(conn: &Connection, kind: &str, tmdb_id: u64) -> Result<Option<WatchTitle>> {
    let sql = format!("SELECT {WATCH_COLS} FROM watch_titles WHERE kind = ?1 AND tmdb_id = ?2");
    conn.query_row(&sql, params![kind, tmdb_id as i64], watch_from_row)
        .optional()
        .map_err(Into::into)
}

pub fn list_watch_titles(conn: &Connection, enabled_only: bool) -> Result<Vec<WatchTitle>> {
    let sql = format!(
        "SELECT {WATCH_COLS} FROM watch_titles {} ORDER BY updated_at DESC",
        if enabled_only {
            "WHERE enabled = 1"
        } else {
            ""
        }
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], watch_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

pub fn set_watch_enabled(
    pool: &Pool,
    kind: &str,
    tmdb_id: u64,
    enabled: bool,
    now_ms: i64,
) -> Result<bool> {
    let conn = pool.get()?;
    let changed = conn.execute(
        "UPDATE watch_titles SET enabled = ?3, updated_at = ?4 WHERE kind = ?1 AND tmdb_id = ?2",
        params![kind, tmdb_id as i64, i64::from(enabled), now_ms],
    )?;
    Ok(changed > 0)
}

pub fn delete_watch_title(pool: &Pool, kind: &str, tmdb_id: u64) -> Result<bool> {
    let conn = pool.get()?;
    Ok(conn.execute(
        "DELETE FROM watch_titles WHERE kind = ?1 AND tmdb_id = ?2",
        params![kind, tmdb_id as i64],
    )? > 0)
}

pub fn upsert_share_subscription(conn: &Connection, sub: &ShareSubscription) -> Result<()> {
    let seasons = serde_json::to_string(&sub.seasons)?;
    conn.execute(
        "INSERT INTO share_subscriptions \
            (id, tmdb_id, kind, title, poster_url, share_code, receive_code, seasons, enabled, \
             status, last_checked, last_error, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14) \
         ON CONFLICT(id) DO UPDATE SET \
             title = excluded.title, poster_url = excluded.poster_url, \
             receive_code = excluded.receive_code, seasons = excluded.seasons, \
             enabled = excluded.enabled, status = excluded.status, \
             last_checked = excluded.last_checked, last_error = excluded.last_error, \
             updated_at = excluded.updated_at",
        params![
            sub.id,
            sub.tmdb_id as i64,
            sub.kind,
            sub.title,
            sub.poster_url,
            sub.share_code,
            sub.receive_code,
            seasons,
            i64::from(sub.enabled),
            sub.status,
            sub.last_checked,
            sub.last_error,
            sub.created_at,
            sub.updated_at,
        ],
    )?;
    Ok(())
}

pub fn get_share_subscription(conn: &Connection, id: &str) -> Result<Option<ShareSubscription>> {
    let sql = format!("SELECT {SHARE_COLS} FROM share_subscriptions WHERE id = ?1");
    conn.query_row(&sql, params![id], share_subscription_from_row)
        .optional()
        .map_err(Into::into)
}

pub fn list_share_subscriptions(
    conn: &Connection,
    enabled_only: bool,
) -> Result<Vec<ShareSubscription>> {
    let sql = format!(
        "SELECT {SHARE_COLS} FROM share_subscriptions {} ORDER BY updated_at DESC",
        if enabled_only {
            "WHERE enabled = 1 AND status = 'active'"
        } else {
            ""
        }
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], share_subscription_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

pub fn delete_share_subscription(pool: &Pool, id: &str) -> Result<bool> {
    let conn = pool.get()?;
    Ok(conn.execute("DELETE FROM share_subscriptions WHERE id = ?1", params![id])? > 0)
}

/// Record a file the share listed. Returns false when the pair is already known,
/// so the caller transfers only new files.
pub fn note_share_file(conn: &Connection, file: &ShareFile) -> Result<bool> {
    let changed = conn.execute(
        "INSERT OR IGNORE INTO share_files \
            (id, subscription_id, file_id, name, size, sha, season, episode, state, remote_id, \
             error, first_seen, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            file.id,
            file.subscription_id,
            file.file_id,
            file.name,
            file.size,
            file.sha,
            file.season.map(|value| value as i64),
            file.episode.map(|value| value as i64),
            file.state,
            file.remote_id,
            file.error,
            file.first_seen,
            file.updated_at,
        ],
    )?;
    Ok(changed > 0)
}

pub fn list_share_files(conn: &Connection, subscription_id: &str) -> Result<Vec<ShareFile>> {
    let sql = format!(
        "SELECT {SHARE_FILE_COLS} FROM share_files WHERE subscription_id = ?1 \
         ORDER BY season, episode, name"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![subscription_id], share_file_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

pub fn set_share_file_state(
    pool: &Pool,
    subscription_id: &str,
    file_id: &str,
    state: &str,
    remote_id: Option<&str>,
    error: &str,
    now_ms: i64,
) -> Result<()> {
    let conn = pool.get()?;
    conn.execute(
        "UPDATE share_files SET state = ?3, remote_id = COALESCE(?4, remote_id), error = ?5, \
         updated_at = ?6 WHERE subscription_id = ?1 AND file_id = ?2",
        params![subscription_id, file_id, state, remote_id, error, now_ms],
    )?;
    Ok(())
}

pub fn upsert_job(conn: &Connection, job: &AcquisitionJob) -> Result<()> {
    let candidate = job
        .candidate
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    conn.execute(
        "INSERT INTO acquisition_jobs \
            (id, kind, unit_key, tmdb_id, title, media_kind, season, episode, destination, source, \
             candidate, status, stage_detail, price_points, price_known, paid_confirmed, \
             remote_id, progress, error_code, error_message, created_at, updated_at, finished_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, \
             ?18, ?19, ?20, ?21, ?22, ?23) \
         ON CONFLICT(id) DO UPDATE SET \
             status = excluded.status, stage_detail = excluded.stage_detail, \
             price_points = excluded.price_points, price_known = excluded.price_known, \
             paid_confirmed = excluded.paid_confirmed, remote_id = excluded.remote_id, \
             progress = excluded.progress, error_code = excluded.error_code, \
             error_message = excluded.error_message, updated_at = excluded.updated_at, \
             finished_at = excluded.finished_at",
        params![
            job.id,
            job.kind,
            job.unit_key,
            job.tmdb_id as i64,
            job.title,
            job.media_kind,
            job.season.map(|value| value as i64),
            job.episode.map(|value| value as i64),
            job.destination.as_str(),
            job.source,
            candidate,
            job.status,
            job.stage_detail,
            job.price_points.map(|value| value as i64),
            i64::from(job.price_known),
            i64::from(job.paid_confirmed),
            job.remote_id,
            job.progress,
            job.error_code,
            job.error_message,
            job.created_at,
            job.updated_at,
            job.finished_at,
        ],
    )?;
    Ok(())
}

pub fn get_job(conn: &Connection, id: &str) -> Result<Option<AcquisitionJob>> {
    let sql = format!("SELECT {JOB_COLS} FROM acquisition_jobs WHERE id = ?1");
    conn.query_row(&sql, params![id], job_from_row)
        .optional()
        .map_err(Into::into)
}

pub fn list_jobs(conn: &Connection, limit: usize) -> Result<Vec<AcquisitionJob>> {
    let sql = format!("SELECT {JOB_COLS} FROM acquisition_jobs ORDER BY created_at DESC LIMIT ?1");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![limit as i64], job_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

/// Record a payment confirmation exactly once. A second call on the same job is
/// a no-op, so a double click cannot charge twice.
pub fn confirm_job_payment(pool: &Pool, id: &str, now_ms: i64) -> Result<bool> {
    let conn = pool.get()?;
    let changed = conn.execute(
        "UPDATE acquisition_jobs SET paid_confirmed = 1, updated_at = ?2 \
         WHERE id = ?1 AND paid_confirmed = 0 AND status = 'waiting_confirm'",
        params![id, now_ms],
    )?;
    Ok(changed > 0)
}

use rusqlite::OptionalExtension;

#[cfg(test)]
mod tests;
