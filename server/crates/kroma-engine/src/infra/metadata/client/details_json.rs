//! Raw TMDB detail-endpoint JSON shapes, and the two picks that read nothing
//! but them. The fetch and the JSON->domain mapping live in [`super::details`].

use serde::Deserialize;

use super::super::common::{RawCreatedBy, RawCredits};
use super::certification::Certifications;
use super::MAX_KEYWORDS;

#[derive(Debug, Deserialize)]
pub(super) struct Details {
    pub(super) id: u64,
    #[serde(default)]
    pub(super) title: Option<String>, // movies
    #[serde(default)]
    pub(super) name: Option<String>, // shows
    #[serde(default)]
    pub(super) overview: Option<String>,
    #[serde(default)]
    pub(super) tagline: Option<String>,
    #[serde(default)]
    pub(super) release_date: Option<String>, // movies
    #[serde(default)]
    pub(super) first_air_date: Option<String>, // shows
    #[serde(default)]
    pub(super) vote_average: Option<f32>,
    #[serde(default)]
    pub(super) poster_path: Option<String>,
    #[serde(default)]
    pub(super) backdrop_path: Option<String>,
    #[serde(default)]
    pub(super) genres: Vec<Genre>,
    #[serde(default)]
    pub(super) production_companies: Vec<KeywordEntry>,
    #[serde(default)]
    pub(super) networks: Vec<KeywordEntry>,
    #[serde(default)]
    pub(super) imdb_id: Option<String>, // present on movie details
    #[serde(default)]
    pub(super) external_ids: Option<ExternalIds>, // appended (carries imdb_id for shows)
    #[serde(default)]
    pub(super) credits: Option<RawCredits>, // appended (cast + crew)
    #[serde(default)]
    pub(super) created_by: Vec<RawCreatedBy>, // TV series creators (top-level on show details)
    #[serde(default)]
    pub(super) images: Option<Images>, // appended (logos)
    #[serde(default)]
    pub(super) keywords: Option<Keywords>, // appended (thematic tags)
    #[serde(default)]
    pub(super) release_dates: Option<Certifications>, // appended (movie age ratings)
    #[serde(default)]
    pub(super) content_ratings: Option<Certifications>, // appended (show age ratings)
    #[serde(default)]
    pub(super) belongs_to_collection: Option<BelongsToCollection>, // movies only
}

#[derive(Debug, Deserialize)]
pub(super) struct BelongsToCollection {
    #[serde(default)]
    pub(super) id: u64,
    #[serde(default)]
    pub(super) name: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct Keywords {
    #[serde(default)]
    pub(super) keywords: Vec<KeywordEntry>,
    #[serde(default)]
    pub(super) results: Vec<KeywordEntry>,
}

#[derive(Debug, Deserialize)]
pub(super) struct KeywordEntry {
    #[serde(default)]
    pub(super) name: String,
}

pub(super) fn collect_keywords(k: Keywords) -> Vec<String> {
    k.keywords
        .into_iter()
        .chain(k.results)
        .map(|e| e.name)
        .filter(|n| !n.is_empty())
        .take(MAX_KEYWORDS)
        .collect()
}

#[derive(Debug, Deserialize)]
pub(super) struct Genre {
    pub(super) id: u32,
    pub(super) name: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct Images {
    #[serde(default)]
    pub(super) posters: Vec<ImageEntry>,
    #[serde(default)]
    pub(super) logos: Vec<ImageEntry>,
    #[serde(default)]
    pub(super) backdrops: Vec<ImageEntry>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ImageEntry {
    #[serde(default)]
    pub(super) file_path: Option<String>,
    #[serde(default)]
    pub(super) width: u32,
    #[serde(default)]
    pub(super) height: u32,
    #[serde(default, rename = "iso_639_1")]
    pub(super) lang: Option<String>,
    #[serde(default)]
    pub(super) vote_average: Option<f32>,
}

// Best poster and backdrop: preferred language first, then English, then
// language-neutral, with vote average and resolution breaking ties.
fn language_rank(image: &ImageEntry, lang2: &str) -> u8 {
    match image.lang.as_deref() {
        Some(language) if language == lang2 => 0,
        Some("en") => 1,
        None | Some("") => 2,
        _ => 3,
    }
}

pub(super) fn pick_poster(posters: &[ImageEntry], lang2: &str) -> Option<String> {
    posters
        .iter()
        .filter(|image| {
            image
                .file_path
                .as_deref()
                .is_some_and(|path| !path.is_empty())
        })
        .min_by(|a, b| {
            language_rank(a, lang2)
                .cmp(&language_rank(b, lang2))
                .then_with(|| {
                    b.vote_average
                        .unwrap_or(0.0)
                        .total_cmp(&a.vote_average.unwrap_or(0.0))
                })
                .then_with(|| (b.width * b.height).cmp(&(a.width * a.height)))
                .then_with(|| a.file_path.cmp(&b.file_path))
        })
        .and_then(|image| image.file_path.clone())
}

pub(super) fn pick_backdrop(backdrops: &[ImageEntry], lang2: &str) -> Option<String> {
    backdrops
        .iter()
        .filter(|image| {
            image
                .file_path
                .as_deref()
                .is_some_and(|path| !path.is_empty())
        })
        .min_by(|a, b| {
            language_rank(a, lang2)
                .cmp(&language_rank(b, lang2))
                .then_with(|| {
                    b.vote_average
                        .unwrap_or(0.0)
                        .total_cmp(&a.vote_average.unwrap_or(0.0))
                })
                .then_with(|| (b.width * b.height).cmp(&(a.width * a.height)))
                .then_with(|| a.file_path.cmp(&b.file_path))
        })
        .and_then(|image| image.file_path.clone())
}

// Best title logo `file_path`: PNG only, preferring the configured language,
// then English, then language-neutral; ties broken by TMDB vote.
pub(super) fn pick_logo(logos: &[ImageEntry], lang2: &str) -> Option<String> {
    let rank = |l: &ImageEntry| -> u8 {
        match l.lang.as_deref() {
            Some(x) if x == lang2 => 0,
            Some("en") => 1,
            None | Some("") => 2,
            _ => 3,
        }
    };
    logos
        .iter()
        .filter(|l| l.file_path.as_deref().is_some_and(|p| p.ends_with(".png")))
        .min_by(|a, b| {
            rank(a).cmp(&rank(b)).then(
                b.vote_average
                    .unwrap_or(0.0)
                    .partial_cmp(&a.vote_average.unwrap_or(0.0))
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
        })
        .and_then(|l| l.file_path.clone())
}

/// How many extra gallery images one title keeps, past its primary backdrop.
pub(super) const MAX_BACKDROPS: usize = 8;

// Gallery artwork `file_path`s: every entry with a path, best-voted first.
// The primary backdrop is not removed here — the caller knows which URL that
// is and drops it, so the gallery never repeats the hero image.
pub(super) fn pick_backdrops(backdrops: &[ImageEntry]) -> Vec<String> {
    let mut unique: Vec<&ImageEntry> = Vec::new();
    for entry in backdrops {
        let Some(path) = entry.file_path.as_deref().filter(|path| !path.is_empty()) else {
            continue;
        };
        match unique
            .iter_mut()
            .find(|seen| seen.file_path.as_deref() == Some(path))
        {
            Some(seen)
                if entry
                    .vote_average
                    .unwrap_or(0.0)
                    .total_cmp(&seen.vote_average.unwrap_or(0.0))
                    .then((entry.width * entry.height).cmp(&(seen.width * seen.height)))
                    .is_gt() =>
            {
                *seen = entry
            }
            Some(_) => {}
            None => unique.push(entry),
        }
    }
    unique.sort_by(|a, b| {
        b.vote_average
            .unwrap_or(0.0)
            .total_cmp(&a.vote_average.unwrap_or(0.0))
            .then((b.width * b.height).cmp(&(a.width * a.height)))
            .then(a.file_path.cmp(&b.file_path))
    });
    unique
        .into_iter()
        .take(MAX_BACKDROPS)
        .filter_map(|image| image.file_path.clone())
        .collect()
}

#[derive(Debug, Deserialize)]
pub(super) struct ExternalIds {
    #[serde(default)]
    pub(super) imdb_id: Option<String>,
    #[serde(default)]
    pub(super) tvdb_id: Option<u64>, // present on TV external_ids; absent for movies
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_movie_details() {
        let raw = r#"{
            "id": 542178,
            "title": "The French Dispatch",
            "tagline": "Read all about it.",
            "overview": "A love letter to journalists.",
            "release_date": "2021-10-21",
            "vote_average": 7.4,
            "poster_path": "/poster.jpg",
            "backdrop_path": "/back.jpg",
            "genres": [{"id": 35, "name": "Comedy"}, {"id": 18, "name": "Drama"}],
            "imdb_id": "tt8847712",
            "external_ids": {"imdb_id": "tt8847712"}
        }"#;
        let d: Details = serde_json::from_str(raw).unwrap();
        assert_eq!(d.id, 542178);
        assert_eq!(d.title.as_deref(), Some("The French Dispatch"));
        assert_eq!(d.imdb_id.as_deref(), Some("tt8847712"));
        assert_eq!(d.genres.len(), 2);
        assert_eq!(d.vote_average, Some(7.4));
    }

    #[test]
    fn parses_tv_details_with_external_ids() {
        let raw = r#"{
            "id": 1399,
            "name": "Game of Thrones",
            "overview": "Seven noble families fight.",
            "first_air_date": "2011-04-17",
            "vote_average": 8.4,
            "poster_path": "/got.jpg",
            "genres": [{"id": 10765, "name": "Sci-Fi & Fantasy"}],
            "external_ids": {"imdb_id": "tt0944947"}
        }"#;
        let d: Details = serde_json::from_str(raw).unwrap();
        assert_eq!(d.name.as_deref(), Some("Game of Thrones"));
        assert!(d.title.is_none());
        assert_eq!(
            d.external_ids.and_then(|e| e.imdb_id).as_deref(),
            Some("tt0944947")
        );
    }

    #[test]
    fn parses_appended_credits() {
        let raw = r#"{
            "id": 1,
            "title": "X",
            "credits": {
                "cast": [
                    {"name": "Bravo", "character": "B", "order": 1},
                    {"name": "Alpha", "character": "A", "order": 0},
                    {"name": "NoChar", "character": "", "order": 2}
                ]
            }
        }"#;
        let d: Details = serde_json::from_str(raw).unwrap();
        let mut cast = d.credits.unwrap().cast;
        cast.sort_by_key(|m| m.order.unwrap_or(u32::MAX));
        assert_eq!(cast[0].name, "Alpha");
        assert_eq!(cast[0].character.as_deref(), Some("A"));
        // Empty character strings are dropped during the Metadata mapping.
        assert_eq!(cast[2].character.as_deref(), Some(""));
    }

    #[test]
    fn posters_prefer_requested_language_then_votes_and_resolution() {
        let posters: Vec<ImageEntry> = serde_json::from_value(serde_json::json!([
            {"file_path":"/zh-small.jpg","iso_639_1":"zh","vote_average":3.0,"width":500,"height":750},
            {"file_path":"/en-large.jpg","iso_639_1":"en","vote_average":9.9,"width":2000,"height":3000},
            {"file_path":"/zh-large.jpg","iso_639_1":"zh","vote_average":3.0,"width":1000,"height":1500}
        ]))
        .unwrap();
        assert_eq!(
            pick_poster(&posters, "zh").as_deref(),
            Some("/zh-large.jpg")
        );
    }

    #[test]
    fn backdrops_prefer_language_and_deduplicate_by_provider_path() {
        let images: Vec<ImageEntry> = serde_json::from_value(serde_json::json!([
            {"file_path":"/neutral.jpg","vote_average":10.0,"width":3000,"height":1700},
            {"file_path":"/en.jpg","iso_639_1":"en","vote_average":10.0,"width":3000,"height":1700},
            {"file_path":"/zh.jpg","iso_639_1":"zh","vote_average":2.0,"width":1000,"height":560},
            {"file_path":"/zh.jpg","iso_639_1":"zh","vote_average":2.0,"width":2000,"height":1120}
        ]))
        .unwrap();
        assert_eq!(pick_backdrop(&images, "zh").as_deref(), Some("/zh.jpg"));
        assert_eq!(
            pick_backdrops(&images)
                .iter()
                .filter(|path| *path == "/zh.jpg")
                .count(),
            1
        );
    }

    #[test]
    fn gallery_paths_rank_by_vote_and_skip_entries_without_a_path() {
        let raw = r#"[
            {"file_path": "/low.jpg", "vote_average": 1.0},
            {"file_path": null, "vote_average": 9.9},
            {"file_path": "/best.jpg", "vote_average": 8.5},
            {"file_path": "/best.jpg", "vote_average": 8.5},
            {"file_path": "/mid.jpg", "vote_average": 4.0}
        ]"#;
        let entries: Vec<ImageEntry> = serde_json::from_str(raw).unwrap();

        assert_eq!(
            pick_backdrops(&entries),
            vec!["/best.jpg", "/mid.jpg", "/low.jpg"]
        );
    }

    #[test]
    fn the_gallery_stops_at_eight_slides() {
        let entries: Vec<ImageEntry> = (0..12)
            .map(|n| {
                serde_json::from_value(serde_json::json!({
                    "file_path": format!("/b{n}.jpg"),
                    "vote_average": n,
                }))
                .unwrap()
            })
            .collect();

        let picked = pick_backdrops(&entries);

        assert_eq!(picked.len(), MAX_BACKDROPS);
        assert_eq!(picked[0], "/b11.jpg");
    }

    #[test]
    fn collects_movie_and_tv_keywords() {
        // Movies nest under `keywords`.
        let movie: Keywords = serde_json::from_str(
            r#"{"keywords":[{"id":1,"name":"road movie"},{"id":2,"name":"summer"}]}"#,
        )
        .unwrap();
        assert_eq!(collect_keywords(movie), vec!["road movie", "summer"]);
        // Shows nest under `results`.
        let tv: Keywords =
            serde_json::from_str(r#"{"results":[{"id":3,"name":"dystopia"}]}"#).unwrap();
        assert_eq!(collect_keywords(tv), vec!["dystopia"]);
    }
}
