//! Operator-chosen artwork, kept apart from the scraped metadata.
//!
//! Enrichment rewrites `items.metadata` / `shows.metadata` wholesale, so an
//! override written there would be lost on the next pass. This table is read
//! last, after the provider art, and is never touched by enrichment; clearing a
//! row restores the scraped image.

use std::collections::HashMap;

use rusqlite::params;

use crate::{Pool, Result};

pub const ITEM_KIND: &str = "item";
pub const SHOW_KIND: &str = "show";
pub const POSTER: &str = "poster";
pub const BACKDROP: &str = "backdrop";
pub const LOGO: &str = "logo";

pub fn get(pool: &Pool, kind: &str, id: &str) -> Result<HashMap<String, String>> {
    let conn = pool.get()?;
    let mut stmt = conn.prepare(
        "SELECT art, url FROM art_overrides WHERE subject_kind = ?1 AND subject_id = ?2",
    )?;
    let rows = stmt.query_map(params![kind, id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut out = HashMap::new();
    for row in rows {
        let (art, url) = row?;
        out.insert(art, url);
    }
    Ok(out)
}

/// Every override of one subject kind, keyed by subject id. The table only ever
/// holds hand-picked rows, so loading it whole keeps list paths from asking per
/// card.
pub fn all(pool: &Pool, kind: &str) -> Result<HashMap<String, HashMap<String, String>>> {
    let conn = pool.get()?;
    let mut stmt = conn.prepare(
        "SELECT subject_id, art, url FROM art_overrides WHERE subject_kind = ?1",
    )?;
    let rows = stmt.query_map(params![kind], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let mut out: HashMap<String, HashMap<String, String>> = HashMap::new();
    for row in rows {
        let (id, art, url) = row?;
        out.entry(id).or_default().insert(art, url);
    }
    Ok(out)
}

pub fn set(pool: &Pool, kind: &str, id: &str, art: &str, url: &str, source: &str) -> Result<()> {
    let conn = pool.get()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    conn.execute(
        "INSERT INTO art_overrides (subject_kind, subject_id, art, url, source, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
         ON CONFLICT(subject_kind, subject_id, art) DO UPDATE SET \
             url = excluded.url, source = excluded.source, updated_at = excluded.updated_at",
        params![kind, id, art, url, source, now],
    )?;
    Ok(())
}

pub fn clear(pool: &Pool, kind: &str, id: &str, art: &str) -> Result<()> {
    let conn = pool.get()?;
    conn.execute(
        "DELETE FROM art_overrides WHERE subject_kind = ?1 AND subject_id = ?2 AND art = ?3",
        params![kind, id, art],
    )?;
    Ok(())
}

/// Put the stored override over whichever provider art the metadata already
/// carries. Missing rows leave the scraped image in place.
pub fn apply(meta: &mut kroma_domain::Metadata, overrides: &HashMap<String, String>) {
    if let Some(url) = overrides.get(POSTER) {
        meta.poster_url = Some(url.clone());
    }
    if let Some(url) = overrides.get(BACKDROP) {
        meta.backdrop_url = Some(url.clone());
    }
    if let Some(url) = overrides.get(LOGO) {
        meta.logo_url = Some(url.clone());
    }
}