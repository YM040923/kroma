use std::collections::{HashMap, HashSet};

use crate::domain::naming::Parsed;
use crate::model::{Kind, MediaFile, MediaItem, Show};

use super::ids::{episode_logical_id, movie_logical_id, show_key};

pub(super) struct Indexed {
    pub item: String,
    pub show: Option<String>,
}

/// Fold one parsed file into the shared item / show maps. `item_kind` keeps
/// Photos and Other libraries from being mistaken for TMDB movies.
#[allow(clippy::too_many_arguments)]
pub(super) fn index_parsed(
    parsed: Parsed,
    lib_id: &str,
    added_at: &str,
    file: MediaFile,
    mtime: Option<i64>,
    item_kind: Option<Kind>,
    items: &mut HashMap<String, MediaItem>,
    shows: &mut HashMap<String, Show>,
    mtimes: &mut HashMap<String, Option<i64>>,
    lib_item_ids: &mut HashSet<String>,
    movie_seen: &mut bool,
    episode_seen: &mut bool,
    photo_seen: &mut bool,
    other_seen: &mut bool,
) -> Indexed {
    match parsed {
        Parsed::Movie { title, year } => {
            let kind = item_kind.clone().unwrap_or(Kind::Movie);
            match kind {
                Kind::Movie | Kind::Video => *movie_seen = true,
                Kind::Photo => *photo_seen = true,
                Kind::Other => *other_seen = true,
                Kind::Episode => {}
            }
            let title = if title.is_empty() {
                "Untitled".into()
            } else {
                title
            };
            let logical = match item_kind {
                Some(Kind::Photo | Kind::Other) => file.id.clone(),
                Some(Kind::Movie | Kind::Episode | Kind::Video) | None => {
                    movie_logical_id(lib_id, &title, year)
                }
            };
            lib_item_ids.insert(logical.clone());
            let item = items.entry(logical.clone()).or_insert_with(|| MediaItem {
                id: logical.clone(),
                title: title.clone(),
                kind: kind.clone(),
                year,
                duration_ms: None,
                container: if kind == Kind::Photo {
                    file.container.clone()
                } else {
                    String::new()
                },
                video: None,
                audio: None,
                audio_tracks: Vec::new(),
                subtitles: Vec::new(),
                library: lib_id.to_string(),
                show_id: None,
                show_title: None,
                season: None,
                episode: None,
                episode_end: None,
                episode_title: None,
                rel_path: file.rel_path.clone(),
                added_at: added_at.to_string(),
                metadata: None,
                abs_path: None,
                files: Vec::new(),
                editions: Vec::new(),
                default_file_id: None,
                markers: Vec::new(),
                audio_analysis: None,
            });
            mtimes.insert(file.id.clone(), mtime);
            item.files.push(file);
            Indexed {
                item: logical,
                show: None,
            }
        }
        Parsed::Episode {
            show_title,
            show_year,
            season,
            episode,
            episode_end,
            episode_title,
        } => {
            *episode_seen = true;
            let show_id = show_key(lib_id, &show_title);
            shows
                .entry(show_id.clone())
                .and_modify(|s| {
                    if s.year.is_none() {
                        s.year = show_year;
                    }
                })
                .or_insert_with(|| Show {
                    id: show_id.clone(),
                    title: show_title.clone(),
                    year: show_year,
                    library: lib_id.to_string(),
                    season_count: 0,
                    episode_count: 0,
                    video: None,
                    added_at: added_at.to_string(),
                    metadata: None,
                    progress: None,
                });

            let logical = episode_logical_id(&show_id, season, episode);
            lib_item_ids.insert(logical.clone());
            let title = episode_title
                .clone()
                .unwrap_or_else(|| format!("S{season:02}E{episode:02}"));
            let item = items.entry(logical.clone()).or_insert_with(|| MediaItem {
                id: logical.clone(),
                title: title.clone(),
                kind: Kind::Episode,
                year: show_year,
                duration_ms: None,
                container: String::new(),
                video: None,
                audio: None,
                audio_tracks: Vec::new(),
                subtitles: Vec::new(),
                library: lib_id.to_string(),
                show_id: Some(show_id.clone()),
                show_title: Some(show_title.clone()),
                season: Some(season),
                episode: Some(episode),
                episode_end,
                episode_title: episode_title.clone(),
                rel_path: None,
                added_at: added_at.to_string(),
                metadata: None,
                abs_path: None,
                files: Vec::new(),
                editions: Vec::new(),
                default_file_id: None,
                markers: Vec::new(),
                audio_analysis: None,
            });
            mtimes.insert(file.id.clone(), mtime);
            item.files.push(file);
            Indexed {
                item: logical,
                show: Some(show_id),
            }
        }
    }
}
