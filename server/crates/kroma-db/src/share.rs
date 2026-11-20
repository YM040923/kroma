//! One public link to one title: the share row. Only the server's own actions
//! decide whether a row is live; the read side always asks "active now".

use rusqlite::{params, OptionalExtension};

use super::{Pool, Result};

#[derive(Debug, Clone)]
pub struct Share {
    pub id: String,
    pub slug: String,
    pub item_id: String,
    pub created_by: String,
    pub password_hash: Option<String>,
    pub password_version: i64,
    /// The cookie value an unlocked visitor carries; changing it revokes every
    /// browser that had unlocked.
    pub access_key: String,
    pub expires_at: i64,
    pub revoked_at: Option<i64>,
    pub view_count: i64,
    pub last_accessed_at: Option<i64>,
    pub created_at: i64,
}

const COLS: &str = "id,slug,item_id,created_by,password_hash,password_version,access_key,\
                    expires_at,revoked_at,view_count,last_accessed_at,created_at";

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Share> {
    Ok(Share {
        id: r.get(0)?,
        slug: r.get(1)?,
        item_id: r.get(2)?,
        created_by: r.get(3)?,
        password_hash: r.get(4)?,
        password_version: r.get(5)?,
        access_key: r.get(6)?,
        expires_at: r.get(7)?,
        revoked_at: r.get(8)?,
        view_count: r.get(9)?,
        last_accessed_at: r.get(10)?,
        created_at: r.get(11)?,
    })
}

impl Share {
    pub fn is_live(&self, now: i64) -> bool {
        self.revoked_at.is_none() && self.expires_at > now
    }
}

pub fn by_slug(pool: &Pool, slug: &str) -> Result<Option<Share>> {
    let conn = pool.get()?;
    Ok(conn
        .query_row(
            &format!("SELECT {COLS} FROM media_share WHERE slug = ?1"),
            params![slug],
            row,
        )
        .optional()?)
}

/// The item's live share, if it has one. An expired or revoked row does not
/// count: a title may be shared again right after its link lapsed.
pub fn active_for_item(pool: &Pool, item_id: &str, now: i64) -> Result<Option<Share>> {
    let conn = pool.get()?;
    Ok(conn
        .query_row(
            &format!(
                "SELECT {COLS} FROM media_share \
                 WHERE item_id = ?1 AND revoked_at IS NULL AND expires_at > ?2 \
                 ORDER BY created_at DESC LIMIT 1"
            ),
            params![item_id, now],
            row,
        )
        .optional()?)
}

pub fn insert(pool: &Pool, share: &Share) -> Result<()> {
    let conn = pool.get()?;
    conn.execute(
        "INSERT INTO media_share \
         (id,slug,item_id,created_by,password_hash,password_version,access_key,\
          expires_at,revoked_at,view_count,last_accessed_at,created_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,NULL,0,NULL,?9)",
        params![
            share.id,
            share.slug,
            share.item_id,
            share.created_by,
            share.password_hash,
            share.password_version,
            share.access_key,
            share.expires_at,
            share.created_at,
        ],
    )?;
    Ok(())
}

pub fn revoke(pool: &Pool, slug: &str, now: i64) -> Result<bool> {
    let conn = pool.get()?;
    Ok(conn.execute(
        "UPDATE media_share SET revoked_at = ?2 \
         WHERE slug = ?1 AND revoked_at IS NULL",
        params![slug, now],
    )? > 0)
}

pub fn touch(pool: &Pool, slug: &str, now: i64) -> Result<()> {
    let conn = pool.get()?;
    conn.execute(
        "UPDATE media_share SET view_count = view_count + 1, last_accessed_at = ?2 \
         WHERE slug = ?1",
        params![slug, now],
    )?;
    Ok(())
}

/// Every live share, for the management list.
pub fn live(pool: &Pool, now: i64) -> Result<Vec<Share>> {
    let conn = pool.get()?;
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM media_share \
         WHERE revoked_at IS NULL AND expires_at > ?1 ORDER BY created_at DESC"
    ))?;
    let rows = stmt.query_map([now], row)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}
