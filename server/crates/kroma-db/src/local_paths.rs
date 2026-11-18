//! The local files a title occupies, for sidecar writers (NFO).
//!
//! A streamed library keeps the play URL in `files.abs_path` and the scanned
//! `.strm` in `rel_path`; both are returned so the caller can fall back to the
//! library folder for a file that is not a local path.

use rusqlite::params;

use crate::{Pool, Result};

/// One scanned file: its stored path (possibly a stream URL) and its path
/// relative to the library folder.
pub struct LocalFile {
    pub abs_path: String,
    pub rel_path: String,
}

fn push(out: &mut Vec<LocalFile>, abs_path: String, rel_path: String) {
    if !abs_path.trim().is_empty() || !rel_path.trim().is_empty() {
        out.push(LocalFile { abs_path, rel_path });
    }
}

/// Every scanned file of one item, in scan order. For a `.strm` this is the
/// `.strm` file itself, which is where an NFO belongs.
pub fn item_file_paths(pool: &Pool, item_id: &str) -> Result<Vec<LocalFile>> {
    let conn = pool.get()?;
    let mut stmt = conn
        .prepare("SELECT abs_path, rel_path FROM files WHERE item_id = ?1 ORDER BY id")?;
    let rows = stmt.query_map(params![item_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (abs, rel) = row?;
        push(&mut out, abs, rel);
    }
    Ok(out)
}

/// Every file belonging to a show's episodes.
pub fn show_file_paths(pool: &Pool, show_id: &str) -> Result<Vec<LocalFile>> {
    let conn = pool.get()?;
    let mut stmt = conn.prepare(
        "SELECT f.abs_path, f.rel_path FROM files f JOIN items i ON i.id = f.item_id \
         WHERE i.show_id = ?1 ORDER BY i.season, i.episode, f.id",
    )?;
    let rows = stmt.query_map(params![show_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (abs, rel) = row?;
        push(&mut out, abs, rel);
    }
    Ok(out)
}
