//! Catalog reads: counts, movie/item listings and single-item fetches.
//!
//! Re-exported flat here so the public `db::<item>` paths resolve unchanged:
//! [`shows`] carries the show listing, [`show_detail`] one show with its
//! seasons, and [`splash`] the anonymous sign-in sample.

use rusqlite::OptionalExtension;

use super::{
    attach_files, attach_files_batch, items_by_ids_ordered, params, row_to_item, MediaItem, Pool,
    Result, ITEM_COLS,
};
use kroma_domain::{Season, Show, ShowDetail, SplashEntry};

mod art_owner;
mod person;
mod show_detail;
mod shows;
mod splash;

#[cfg(test)]
mod test_support;

pub use art_owner::*;
pub use person::*;
pub use show_detail::*;
pub use shows::*;
pub use splash::*;

/// (libraries, items, shows) counts for `/api/health`.
pub fn counts(pool: &Pool) -> Result<(usize, usize, usize)> {
    let conn = pool.get()?;
    let libs: i64 = conn.query_row("SELECT COUNT(*) FROM libraries", [], |r| r.get(0))?;
    let items: i64 = conn.query_row("SELECT COUNT(*) FROM items", [], |r| r.get(0))?;
    let shows: i64 = conn.query_row("SELECT COUNT(*) FROM shows", [], |r| r.get(0))?;
    Ok((libs as usize, items as usize, shows as usize))
}

/// How many of those items are not episodes, for a caller that wants a library's
/// size rather than its contents.
pub fn movie_count(pool: &Pool) -> Result<i64> {
    let conn = pool.get()?;
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM items WHERE kind IN ('movie','video')",
        [],
        |r| r.get(0),
    )?)
}

/// Movies (and loose videos) everything that isn't an episode.
pub fn list_movies(pool: &Pool, library: Option<&str>) -> Result<Vec<MediaItem>> {
    query_items(
        pool,
        &format!("SELECT {ITEM_COLS} FROM items WHERE kind IN ('movie','video')"),
        library,
        "ORDER BY title COLLATE NOCASE",
    )
}

/// Photos indexed in capture-date order.
pub fn list_photos(pool: &Pool, library: Option<&str>) -> Result<Vec<MediaItem>> {
    query_items(
        pool,
        &format!("SELECT {ITEM_COLS} FROM items WHERE kind = 'photo'"),
        library,
        "ORDER BY added_at DESC, id",
    )
}

/// Unclassified, playable items from Other libraries.
pub fn list_other(pool: &Pool, library: Option<&str>) -> Result<Vec<MediaItem>> {
    query_items(
        pool,
        &format!("SELECT {ITEM_COLS} FROM items WHERE kind = 'other'"),
        library,
        "ORDER BY added_at DESC, title COLLATE NOCASE",
    )
}

/// All playable items: movies + episodes.
pub fn list_items(pool: &Pool, library: Option<&str>) -> Result<Vec<MediaItem>> {
    query_items(
        pool,
        &format!("SELECT {ITEM_COLS} FROM items WHERE kind IN ('movie','video','episode')"),
        library,
        "ORDER BY title COLLATE NOCASE",
    )
}

/// The age rating that governs `id`: an episode inherits its show's, a movie
/// carries its own. `None` when neither mentions one.
pub fn item_certification(pool: &Pool, id: &str) -> Result<Option<String>> {
    let conn = pool.get()?;
    Ok(conn.query_row(
        "SELECT COALESCE( \
             NULLIF(json_extract(i.metadata,'$.certification'), ''), \
             NULLIF(json_extract(s.metadata,'$.certification'), '')) \
         FROM items i LEFT JOIN shows s ON s.id = i.show_id WHERE i.id = ?1",
        params![id],
        |r| r.get::<_, Option<String>>(0),
    )?)
}

pub fn show_certification(pool: &Pool, id: &str) -> Result<Option<String>> {
    let conn = pool.get()?;
    Ok(conn.query_row(
        "SELECT NULLIF(json_extract(metadata,'$.certification'), '') FROM shows WHERE id = ?1",
        params![id],
        |r| r.get::<_, Option<String>>(0),
    )?)
}

/// The ratings of `ids`, for filtering a batch of episodes by their show.
pub fn show_certifications(
    pool: &Pool,
    ids: &[String],
) -> Result<std::collections::HashMap<String, Option<String>>> {
    let mut out = std::collections::HashMap::new();
    if ids.is_empty() {
        return Ok(out);
    }
    let conn = pool.get()?;
    let placeholders = (1..=ids.len())
        .map(|at| format!("?{at}"))
        .collect::<Vec<_>>()
        .join(",");
    let mut stmt = conn.prepare(&format!(
        "SELECT id, NULLIF(json_extract(metadata,'$.certification'), '') \
         FROM shows WHERE id IN ({placeholders})"
    ))?;
    let params: Vec<&dyn rusqlite::ToSql> =
        ids.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
    let rows = stmt.query_map(params.as_slice(), |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
    })?;
    for row in rows {
        let (id, certification) = row?;
        out.insert(id, certification);
    }
    Ok(out)
}

/// The other films of the series `id` belongs to, in release order, with the
/// series' name. A title that names no series answers with none.
///
/// A provider id groups precisely; an NFO `<set>` carries only a name, so its
/// films group by that name.
pub fn collection_members(pool: &Pool, id: &str) -> Result<(Option<String>, Vec<MediaItem>)> {
    let conn = pool.get()?;
    let names: Option<(Option<String>, Option<i64>)> = conn
        .query_row(
            "SELECT json_extract(metadata,'$.collectionName'), \
                    json_extract(metadata,'$.collectionId') \
             FROM items WHERE id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((name, tmdb)) = names else {
        return Ok((None, Vec::new()));
    };
    let name = name
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty());
    if name.is_none() && tmdb.is_none() {
        return Ok((None, Vec::new()));
    }
    let mut stmt = conn.prepare(&format!(
        "SELECT {ITEM_COLS} FROM items \
         WHERE kind IN ('movie','video') AND id != ?1 \
           AND ((?2 IS NOT NULL AND json_extract(metadata,'$.collectionId') = ?2) \
             OR (?2 IS NULL AND json_extract(metadata,'$.collectionName') = ?3)) \
         ORDER BY COALESCE(NULLIF(json_extract(metadata,'$.releaseDate'), ''), \
                           CAST(year AS TEXT), ''), \
                  title COLLATE NOCASE"
    ))?;
    let items = stmt
        .query_map(params![id, tmdb, name.as_deref()], row_to_item)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok((name, items))
}

pub fn get_item(pool: &Pool, id: &str) -> Result<Option<MediaItem>> {
    let conn = pool.get()?;
    let mut stmt = conn.prepare(&format!("SELECT {ITEM_COLS} FROM items WHERE id = ?1"))?;
    let mut rows = stmt.query_map(params![id], row_to_item)?;
    match rows.next() {
        Some(item) => {
            let mut item = item?;
            attach_files(&conn, &mut item)?;
            Ok(Some(item))
        }
        None => Ok(None),
    }
}

/// Catalogue snapshot for the search index: no per-row file or
/// representative-video lookups, so a full reindex is two table scans.
pub fn index_snapshot(pool: &Pool) -> Result<(Vec<MediaItem>, Vec<Show>)> {
    let conn = pool.get()?;
    let mut stmt = conn.prepare(&format!(
        "SELECT {ITEM_COLS} FROM items WHERE kind IN ('movie','video','episode')"
    ))?;
    let items: Vec<MediaItem> = stmt
        .query_map([], row_to_item)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut stmt = conn.prepare("SELECT id,title,year,library,added_at,metadata FROM shows")?;
    let shows: Vec<Show> = stmt
        .query_map([], row_to_show_bare)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok((items, shows))
}

/// Order is unspecified the caller re-orders by relevance.
pub fn get_items_by_ids(pool: &Pool, ids: &[String]) -> Result<Vec<MediaItem>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let conn = pool.get()?;
    let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
    Ok(items_by_ids_ordered(&conn, &ids)?)
}

fn query_items(
    pool: &Pool,
    base: &str,
    library: Option<&str>,
    tail: &str,
) -> Result<Vec<MediaItem>> {
    let conn = pool.get()?;
    let mut items: Vec<MediaItem> = match library {
        Some(lib) => {
            let sql = format!(
                "{base} {} {tail}",
                if base.contains("WHERE") {
                    "AND library = ?1"
                } else {
                    "WHERE library = ?1"
                }
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params![lib], row_to_item)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        }
        None => {
            let sql = format!("{base} {tail}");
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map([], row_to_item)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        }
    };
    attach_files_batch(&conn, &mut items)?;
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::test_support::*;
    use kroma_domain::Kind;

    #[test]
    fn counts_reflects_seeded_rows() {
        let p = pool();
        {
            let conn = p.get().unwrap();
            conn.execute(
                "INSERT INTO libraries (id,name,kind,path,added_at) VALUES ('lib','L','movies','/x','t')",
                [],
            )
            .unwrap();
            seed_movie(&conn, "m1", "Dune", "lib");
            conn.execute(
                "INSERT INTO shows (id,library,title,added_at) VALUES ('s1','lib','Show','t')",
                [],
            )
            .unwrap();
        }
        assert_eq!(counts(&p).unwrap(), (1, 1, 1));
    }

    #[test]
    fn list_movies_and_items_ordering_and_episode_split() {
        let p = pool();
        {
            let conn = p.get().unwrap();
            conn.execute(
                "INSERT INTO libraries (id,name,kind,path,added_at) VALUES ('lib','L','movies','/x','t')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO libraries (id,name,kind,path,added_at) VALUES ('lib2','L2','movies','/y','t')",
                [],
            )
            .unwrap();
            seed_movie(&conn, "m1", "Dune", "lib");
            seed_movie(&conn, "m2", "Arrival", "lib");
            seed_movie(&conn, "mo", "Other", "lib2");
            conn.execute(
                "INSERT INTO shows (id,library,title,added_at) VALUES ('s1','lib','Show','t')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO items (id,kind,title,container,library,show_id,season,episode,added_at) \
                 VALUES ('e1','episode','Ep','mkv','lib','s1',1,1,'t')",
                [],
            )
            .unwrap();
        }
        let movies = list_movies(&p, None).unwrap();
        assert_eq!(
            movies.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
            ["m2", "m1", "mo"]
        );
        assert!(movies.iter().all(|i| i.kind != Kind::Episode));

        let lib_movies = list_movies(&p, Some("lib")).unwrap();
        assert_eq!(
            lib_movies.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
            ["m2", "m1"]
        );

        let items = list_items(&p, None).unwrap();
        assert!(items.iter().any(|i| i.id == "e1"));
        assert_eq!(items.len(), 4);
    }

    #[test]
    fn get_item_hydrates_representative_file() {
        let p = pool();
        {
            let conn = p.get().unwrap();
            conn.execute(
                "INSERT INTO libraries (id,name,kind,path,added_at) VALUES ('lib','L','movies','/x','t')",
                [],
            )
            .unwrap();
            seed_movie(&conn, "m1", "Dune", "lib");
            seed_probed_file(&conn, "f1", "m1", "/media/dune.mkv", 3840);
        }
        let item = get_item(&p, "m1").unwrap().unwrap();
        assert_eq!(item.default_file_id.as_deref(), Some("f1"));
        assert_eq!(item.abs_path.as_deref(), Some("/media/dune.mkv"));
        assert_eq!(item.container, "mkv");
        assert_eq!(item.duration_ms, Some(7_200_000));
        let video = item.video.expect("probed file yields a video stream");
        assert_eq!(video.codec, "hevc");
        assert_eq!(video.width, Some(3840));
        assert_eq!(item.files.len(), 1);

        assert!(get_item(&p, "missing").unwrap().is_none());
    }

    #[test]
    fn a_collection_groups_by_provider_id_in_release_order() {
        let p = pool();
        {
            let conn = p.get().unwrap();
            conn.execute(
                "INSERT INTO libraries (id,name,kind,path,added_at) VALUES ('lib','L','movies','/x','t')",
                [],
            )
            .unwrap();
            seed_movie(&conn, "m1", "Saga II", "lib");
            seed_movie(&conn, "m2", "Saga I", "lib");
            seed_movie(&conn, "m3", "Saga III", "lib");
            seed_movie(&conn, "m4", "Same Name, Other Series", "lib");
            // Release dates out of title order, so the order can only be the date's.
            let set = |id: &str, tmdb: Option<i64>, name: &str, date: &str| {
                let id_part = tmdb
                    .map(|t| format!(r#","collectionId":{t}"#))
                    .unwrap_or_default();
                let meta = format!(
                    r#"{{"tmdbId":1,"collectionName":"{name}","tmdbUrl":"x","releaseDate":"{date}"{id_part}}}"#
                );
                conn.execute(
                    "UPDATE items SET metadata=?2 WHERE id=?1",
                    params![id, meta],
                )
                .unwrap();
            };
            set("m1", Some(10), "Saga", "2001-05-01");
            set("m2", Some(10), "Saga", "1999-05-01");
            set("m3", Some(10), "Saga", "2019-12-20");
            // Same name, no provider id: a namesake, not this series.
            set("m4", None, "Saga", "1990-01-01");
        }
        let (name, items) = collection_members(&p, "m2").unwrap();
        assert_eq!(name.as_deref(), Some("Saga"));
        let ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["m1", "m3"]);
        assert!(collection_members(&p, "ghost").unwrap().1.is_empty());
    }

    #[test]
    fn an_nfo_named_set_without_an_id_groups_by_its_name() {
        let p = pool();
        {
            let conn = p.get().unwrap();
            conn.execute(
                "INSERT INTO libraries (id,name,kind,path,added_at) VALUES ('lib','L','movies','/x','t')",
                [],
            )
            .unwrap();
            seed_movie(&conn, "m1", "Local I", "lib");
            seed_movie(&conn, "m2", "Local II", "lib");
            seed_movie(&conn, "m3", "Elsewhere", "lib");
            let set = |id: &str, name: &str| {
                conn.execute(
                    "UPDATE items SET metadata=?2 WHERE id=?1",
                    params![
                        id,
                        format!(r#"{{"tmdbId":0,"collectionName":"{name}","tmdbUrl":"x"}}"#)
                    ],
                )
                .unwrap();
            };
            set("m1", "Local Set");
            set("m2", "Local Set");
            set("m3", "Other Set");
        }
        let (name, items) = collection_members(&p, "m1").unwrap();
        assert_eq!(name.as_deref(), Some("Local Set"));
        assert_eq!(
            items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
            ["m2"]
        );
    }

    #[test]
    fn by_ids_and_index_snapshot() {
        let p = pool();
        {
            let conn = p.get().unwrap();
            conn.execute(
                "INSERT INTO libraries (id,name,kind,path,added_at) VALUES ('lib','L','movies','/x','t')",
                [],
            )
            .unwrap();
            seed_movie(&conn, "m1", "Dune", "lib");
            seed_movie(&conn, "m2", "Arrival", "lib");
            conn.execute(
                "INSERT INTO shows (id,library,title,added_at) VALUES ('s1','lib','Show','t')",
                [],
            )
            .unwrap();
        }
        assert!(get_items_by_ids(&p, &[]).unwrap().is_empty());
        assert!(get_shows_by_ids(&p, &[]).unwrap().is_empty());

        let items = get_items_by_ids(&p, &["m2".into(), "m1".into(), "ghost".into()]).unwrap();
        let mut ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, ["m1", "m2"]);

        let shows = get_shows_by_ids(&p, &["s1".into()]).unwrap();
        assert_eq!(shows.len(), 1);
        assert_eq!(shows[0].id, "s1");

        let (items, shows) = index_snapshot(&p).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(shows.len(), 1);
    }

    const SQLITE_BIND_LIMIT: usize = 32_766;

    fn padded_past_the_bind_limit(real: &str) -> Vec<String> {
        let mut ids: Vec<String> = (0..=SQLITE_BIND_LIMIT)
            .map(|n| format!("absent{n}"))
            .collect();
        ids.push(real.to_string());
        ids
    }

    #[test]
    fn a_by_ids_read_longer_than_sqlite_can_bind_still_answers() {
        let p = pool();
        {
            let conn = p.get().unwrap();
            conn.execute(
                "INSERT INTO libraries (id,name,kind,path,added_at) VALUES ('lib','L','movies','/x','t')",
                [],
            )
            .unwrap();
            seed_movie(&conn, "m1", "Dune", "lib");
            conn.execute(
                "INSERT INTO shows (id,library,title,added_at) VALUES ('s1','lib','Show','t')",
                [],
            )
            .unwrap();
        }

        let items = get_items_by_ids(&p, &padded_past_the_bind_limit("m1")).unwrap();
        let shows = get_shows_by_ids(&p, &padded_past_the_bind_limit("s1")).unwrap();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "m1");
        assert_eq!(shows.len(), 1);
        assert_eq!(shows[0].id, "s1");
    }
}
