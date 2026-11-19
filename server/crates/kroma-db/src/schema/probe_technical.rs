//! Step 0026: technical probe fields and season/provider metadata.
//!
//! The fork shipped this as `0020_probe_technical` before upstream took that
//! number. The step now runs on databases that may already have some or all of
//! these columns (a database migrated by the fork build), so each column is
//! added only when it is missing.

use rusqlite::Connection;

pub(super) fn apply(conn: &Connection) -> rusqlite::Result<()> {
    add_column(conn, "files", "v_technical", "TEXT")?;
    add_column(conn, "files", "probe_version", "INTEGER")?;
    add_column(conn, "season_meta", "metadata", "TEXT")?;
    add_column(conn, "metadata_core", "studios", "TEXT NOT NULL DEFAULT '[]'")?;
    Ok(())
}

fn add_column(conn: &Connection, table: &str, column: &str, decl: &str) -> rusqlite::Result<()> {
    let sql = format!("SELECT EXISTS (SELECT 1 FROM pragma_table_info('{table}') WHERE name = ?1)");
    let exists: bool = conn.query_row(&sql, [column], |r| r.get(0))?;
    if !exists {
        conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {decl}"),
            [],
        )?;
    }
    Ok(())
}
