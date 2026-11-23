//! Flagging TMDB hits against this server: the catalogue row a title already
//! has in a library the viewer was granted.

use std::collections::HashMap;

use rusqlite::Connection;

use crate::db;
use crate::infra::metadata::discover;
use crate::model::{DiscoverDetail, DiscoverEntry, DiscoverSeason, RequestKind, User};

fn local_id_for(
    conn: &Connection,
    kind: RequestKind,
    tmdb_id: u64,
    viewer: &User,
) -> anyhow::Result<Option<String>> {
    Ok(db::titles_by_tmdb(conn, kind, tmdb_id)?
        .into_iter()
        .find(|(_, library)| viewer.sees_library(library))
        .map(|(id, _)| id))
}

pub fn flag_hits(
    conn: &Connection,
    hits: Vec<discover::DiscoverHit>,
    viewer: &User,
) -> anyhow::Result<Vec<DiscoverEntry>> {
    hits.into_iter()
        .map(|h| {
            let local_id = local_id_for(conn, h.kind, h.tmdb_id, viewer)?;
            Ok(DiscoverEntry {
                kind: h.kind,
                tmdb_id: h.tmdb_id,
                title: h.title,
                year: h.year,
                poster_url: h.poster_url,
                backdrop_url: h.backdrop_url,
                overview: h.overview,
                rating: h.rating,
                in_library: local_id.is_some(),
                local_id,
                request_id: None,
                request_status: None,
                request_progress: None,
            })
        })
        .collect()
}

pub fn flag_detail(
    conn: &Connection,
    d: discover::DiscoverRawDetail,
    viewer: &User,
) -> anyhow::Result<DiscoverDetail> {
    let local_id = local_id_for(conn, d.kind, d.tmdb_id, viewer)?;

    // Season flags: available = every listed episode is on disk.
    let mut seasons: Vec<DiscoverSeason> = Vec::with_capacity(d.seasons.len());
    if d.kind == RequestKind::Show && !d.seasons.is_empty() {
        let present = match &local_id {
            Some(show_id) => db::episodes_present(conn, show_id)?,
            None => Vec::new(),
        };
        let mut per_season: HashMap<u32, u32> = HashMap::new();
        for (s, _e) in present {
            *per_season.entry(s).or_default() += 1;
        }
        for s in &d.seasons {
            let have = per_season.get(&s.season).copied().unwrap_or(0);
            seasons.push(DiscoverSeason {
                season: s.season,
                name: s.name.clone(),
                episode_count: s.episode_count,
                air_date: s.air_date.clone(),
                available: s.episode_count > 0 && have >= s.episode_count,
                episodes_available: have,
                requested: false,
            });
        }
    }

    let air_status = d.status.clone();
    let next_air_date = match d.kind {
        RequestKind::Show => d.next_air.as_ref().map(|(dt, _, _)| dt.clone()),
        RequestKind::Movie => d.available_date.clone(),
    };

    Ok(DiscoverDetail {
        kind: d.kind,
        tmdb_id: d.tmdb_id,
        title: d.title,
        year: d.year,
        poster_url: d.poster_url,
        backdrop_url: d.backdrop_url,
        overview: d.overview,
        tagline: d.tagline,
        genres: d.genres,
        tmdb_genre_ids: d.tmdb_genre_ids,
        rating: d.rating,
        runtime_min: d.runtime_min,
        seasons,
        cast: d.cast,
        crew: d.crew,
        similar: flag_hits(conn, d.similar, viewer)?,
        in_library: local_id.is_some(),
        local_id,
        request_id: None,
        request_status: None,
        request_progress: None,
        air_status,
        next_air_date,
    })
}

#[cfg(test)]
mod tests;
