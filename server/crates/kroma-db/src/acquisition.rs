//! The acquisition ledger: logical units, their byte locations, and provider
//! attempts. Requests and wanted rows remain the demand layer; this module is
//! the shared identity/placement layer every provider writes against.

use rusqlite::OptionalExtension;

use super::{params, Connection, Pool, Result, Row};
use kroma_domain::{
    AcquisitionAttempt, AttemptStatus, LocationState, MediaLocation, MediaUnit, MediaUnitRef,
};

fn unit_from_row(row: &Row<'_>) -> rusqlite::Result<MediaUnit> {
    let kind = row
        .get::<_, String>(1)
        .ok()
        .and_then(|value| kroma_domain::MediaUnitKind::parse(&value))
        .unwrap_or(kroma_domain::MediaUnitKind::Movie);
    Ok(MediaUnit {
        unit_key: row.get(0)?,
        kind,
        tmdb_id: row.get::<_, i64>(2)? as u64,
        season: row.get::<_, i64>(3)? as u32,
        episode: row.get::<_, i64>(4)? as u32,
        title: row.get(5)?,
        year: row.get::<_, Option<i64>>(6)?.map(|value| value as u32),
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

fn location_from_row(row: &Row<'_>) -> rusqlite::Result<MediaLocation> {
    let state = row
        .get::<_, String>(7)
        .ok()
        .and_then(|value| LocationState::parse(&value))
        .unwrap_or(LocationState::Pending);
    Ok(MediaLocation {
        id: row.get(0)?,
        unit_key: row.get(1)?,
        provider: row.get(2)?,
        account_id: row.get(3)?,
        path: row.get(4)?,
        remote_id: row.get(5)?,
        origin: row.get(6)?,
        state,
        is_primary: row.get::<_, i64>(8)? != 0,
        size: row.get(9)?,
        fingerprint: row.get(10)?,
        quality: row
            .get::<_, Option<String>>(11)?
            .and_then(|value| serde_json::from_str(&value).ok()),
        verified_at: row.get(12)?,
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
    })
}

fn attempt_from_row(row: &Row<'_>) -> rusqlite::Result<AcquisitionAttempt> {
    let status = row
        .get::<_, String>(4)
        .ok()
        .and_then(|value| AttemptStatus::parse(&value))
        .unwrap_or(AttemptStatus::Queued);
    Ok(AcquisitionAttempt {
        id: row.get(0)?,
        unit_key: row.get(1)?,
        request_id: row.get(2)?,
        provider: row.get(3)?,
        status,
        error_code: row.get(5)?,
        error_message: row.get(6)?,
        candidate: row
            .get::<_, Option<String>>(7)?
            .and_then(|value| serde_json::from_str(&value).ok()),
        quote: row
            .get::<_, Option<String>>(8)?
            .and_then(|value| serde_json::from_str(&value).ok()),
        job_id: row.get(9)?,
        cost_points: row.get::<_, i64>(10)?.max(0) as u32,
        unlock_key: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
        finished_at: row.get(14)?,
    })
}

const UNIT_COLS: &str =
    "unit_key, kind, tmdb_id, season, episode, title, year, created_at, updated_at";
const LOCATION_COLS: &str = "id, unit_key, provider, account_id, path, remote_id, origin, state, \
    is_primary, size, fingerprint, quality, verified_at, created_at, updated_at";
const ATTEMPT_COLS: &str =
    "id, unit_key, request_id, provider, status, error_code, error_message, \
    candidate, quote, job_id, cost_points, unlock_key, created_at, updated_at, finished_at";

/// Create the unit once and fill in metadata on later calls. A blank title
/// never erases a title already stored.
pub fn upsert_media_unit(
    conn: &Connection,
    unit: &MediaUnitRef,
    title: &str,
    year: Option<u32>,
    now_ms: i64,
) -> Result<MediaUnit> {
    if !unit.valid() {
        anyhow::bail!("invalid media unit identity");
    }
    let key = unit.key();
    conn.execute(
        "INSERT INTO media_units (unit_key, kind, tmdb_id, season, episode, title, year, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8) \
         ON CONFLICT(unit_key) DO UPDATE SET \
             title = CASE WHEN excluded.title <> '' THEN excluded.title ELSE media_units.title END, \
             year = COALESCE(excluded.year, media_units.year), \
             updated_at = excluded.updated_at",
        params![
            key,
            unit.kind.as_str(),
            unit.tmdb_id as i64,
            unit.season as i64,
            unit.episode as i64,
            title.trim(),
            year.map(|value| value as i64),
            now_ms,
        ],
    )?;
    connection_unit(conn, &key)?.ok_or_else(|| anyhow::anyhow!("media unit vanished after upsert"))
}

fn connection_unit(conn: &Connection, key: &str) -> Result<Option<MediaUnit>> {
    let sql = format!("SELECT {UNIT_COLS} FROM media_units WHERE unit_key = ?1");
    conn.query_row(&sql, params![key], unit_from_row)
        .optional()
        .map_err(Into::into)
}

pub fn get_media_unit(conn: &Connection, key: &str) -> Result<Option<MediaUnit>> {
    connection_unit(conn, key)
}

/// Insert or update one location by its id. The caller owns id generation;
/// this keeps location edits idempotent.
pub fn upsert_media_location(conn: &Connection, location: &MediaLocation) -> Result<()> {
    if location.id.trim().is_empty() {
        anyhow::bail!("a media location needs an id");
    }
    let quality = location
        .quality
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    conn.execute(
        "INSERT INTO media_locations \
            (id, unit_key, provider, account_id, path, remote_id, origin, state, is_primary, \
             size, fingerprint, quality, verified_at, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15) \
         ON CONFLICT(id) DO UPDATE SET \
             unit_key = excluded.unit_key, provider = excluded.provider, \
             account_id = excluded.account_id, path = excluded.path, \
             remote_id = excluded.remote_id, origin = excluded.origin, \
             state = excluded.state, is_primary = excluded.is_primary, \
             size = excluded.size, fingerprint = excluded.fingerprint, \
             quality = excluded.quality, verified_at = excluded.verified_at, \
             updated_at = excluded.updated_at",
        params![
            location.id,
            location.unit_key,
            location.provider,
            location.account_id,
            location.path,
            location.remote_id,
            location.origin,
            location.state.as_str(),
            i64::from(location.is_primary),
            location.size,
            location.fingerprint,
            quality,
            location.verified_at,
            location.created_at,
            location.updated_at,
        ],
    )?;
    Ok(())
}

pub fn list_media_locations(conn: &Connection, unit_key: &str) -> Result<Vec<MediaLocation>> {
    let sql = format!(
        "SELECT {LOCATION_COLS} FROM media_locations WHERE unit_key = ?1 ORDER BY is_primary DESC, created_at"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![unit_key], location_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

pub fn set_location_state(
    pool: &Pool,
    id: &str,
    state: LocationState,
    verified_at: Option<i64>,
    now_ms: i64,
) -> Result<()> {
    let conn = pool.get()?;
    conn.execute(
        "UPDATE media_locations SET state = ?2, verified_at = ?3, updated_at = ?4 WHERE id = ?1",
        params![id, state.as_str(), verified_at, now_ms],
    )?;
    Ok(())
}

/// Insert one provider attempt. The partial unique index rejects a second live
/// attempt for the same unit; callers should read [`active_attempt`] first to
/// turn that into a friendly conflict.
pub fn create_attempt(pool: &Pool, attempt: &AcquisitionAttempt) -> Result<()> {
    let conn = pool.get()?;
    let candidate = attempt
        .candidate
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    let quote = attempt
        .quote
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    conn.execute(
        "INSERT INTO acquisition_attempts \
            (id, unit_key, request_id, provider, status, error_code, error_message, candidate, \
             quote, job_id, cost_points, unlock_key, created_at, updated_at, finished_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            attempt.id,
            attempt.unit_key,
            attempt.request_id,
            attempt.provider,
            attempt.status.as_str(),
            attempt.error_code,
            attempt.error_message,
            candidate,
            quote,
            attempt.job_id,
            attempt.cost_points as i64,
            attempt.unlock_key,
            attempt.created_at,
            attempt.updated_at,
            attempt.finished_at,
        ],
    )?;
    Ok(())
}

pub fn active_attempt(conn: &Connection, unit_key: &str) -> Result<Option<AcquisitionAttempt>> {
    let sql = format!(
        "SELECT {ATTEMPT_COLS} FROM acquisition_attempts \
         WHERE unit_key = ?1 AND status IN (\
            'queued','searching','candidate_ready','waiting_confirm',\
            'transferring','downloading','importing','verifying') \
         ORDER BY created_at DESC LIMIT 1"
    );
    conn.query_row(&sql, params![unit_key], attempt_from_row)
        .optional()
        .map_err(Into::into)
}

pub fn attempts_for_unit(conn: &Connection, unit_key: &str) -> Result<Vec<AcquisitionAttempt>> {
    let sql = format!(
        "SELECT {ATTEMPT_COLS} FROM acquisition_attempts WHERE unit_key = ?1 ORDER BY created_at DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![unit_key], attempt_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

pub fn attempts_by_provider(conn: &Connection, provider: &str) -> Result<Vec<AcquisitionAttempt>> {
    let sql = format!(
        "SELECT {ATTEMPT_COLS} FROM acquisition_attempts WHERE provider = ?1 ORDER BY created_at"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![provider], attempt_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

pub fn set_attempt_job(pool: &Pool, id: &str, job_id: &str, now_ms: i64) -> Result<()> {
    let conn = pool.get()?;
    conn.execute(
        "UPDATE acquisition_attempts SET job_id = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, job_id, now_ms],
    )?;
    Ok(())
}

pub fn set_attempt_status(
    pool: &Pool,
    id: &str,
    status: AttemptStatus,
    error_code: &str,
    error_message: &str,
    finished_at: Option<i64>,
    now_ms: i64,
) -> Result<()> {
    let conn = pool.get()?;
    conn.execute(
        "UPDATE acquisition_attempts SET status = ?2, error_code = ?3, error_message = ?4, \
            finished_at = ?5, updated_at = ?6 WHERE id = ?1",
        params![
            id,
            status.as_str(),
            error_code,
            error_message,
            finished_at,
            now_ms
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kroma_domain::PROVIDER_CLOUD115;

    fn unit() -> MediaUnitRef {
        MediaUnitRef::movie(603)
    }

    #[test]
    fn a_live_attempt_blocks_a_second_one_for_the_same_unit() {
        let pool = crate::testing::temp_pool("acquisition-attempt");
        let conn = pool.get().unwrap();
        let unit = upsert_media_unit(&conn, &unit(), "Film", Some(1999), 1).unwrap();
        drop(conn);
        let first = AcquisitionAttempt {
            id: "a1".into(),
            unit_key: unit.unit_key.clone(),
            request_id: None,
            provider: "bt".into(),
            status: AttemptStatus::Searching,
            error_code: String::new(),
            error_message: String::new(),
            candidate: None,
            quote: None,
            job_id: String::new(),
            cost_points: 0,
            unlock_key: None,
            created_at: 1,
            updated_at: 1,
            finished_at: None,
        };
        create_attempt(&pool, &first).unwrap();
        let mut second = first.clone();
        second.id = "a2".into();
        assert!(create_attempt(&pool, &second).is_err());
        let conn = pool.get().unwrap();
        assert_eq!(
            active_attempt(&conn, &unit.unit_key).unwrap().unwrap().id,
            "a1"
        );
    }

    #[test]
    fn present_cloud_location_is_recorded_and_read_back() {
        let pool = crate::testing::temp_pool("acquisition-location");
        let conn = pool.get().unwrap();
        let unit = upsert_media_unit(&conn, &unit(), "Film", None, 1).unwrap();
        let location = MediaLocation {
            id: "l1".into(),
            unit_key: unit.unit_key.clone(),
            provider: PROVIDER_CLOUD115.into(),
            account_id: "acct".into(),
            path: "/电影/Film.mkv".into(),
            remote_id: "pc1".into(),
            origin: "imported".into(),
            state: LocationState::Present,
            is_primary: false,
            size: Some(42),
            fingerprint: None,
            quality: None,
            verified_at: Some(10),
            created_at: 1,
            updated_at: 1,
        };
        upsert_media_location(&conn, &location).unwrap();
        assert_eq!(
            list_media_locations(&conn, &unit.unit_key).unwrap(),
            vec![location]
        );
    }
}
