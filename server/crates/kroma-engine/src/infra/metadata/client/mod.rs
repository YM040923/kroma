//! TMDB HTTP client: search for the best match, fetch its details + external
//! IDs / credits / images through [`kroma_http::Fetch`], and map the JSON into
//! a [`Metadata`].

mod certification;
mod details;
mod details_json;
mod lookup;

pub use lookup::{lookup, lookup_all, lookup_all_by_id};
pub(super) use kroma_requests::tmdb::{api, tmdb_json, IMG};

use serde::{Deserialize, Serialize};

/// One selectable artwork on TMDB, already sized for the picker.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtImage {
    pub url: String,
    pub width: u32,
    pub height: u32,
    pub language: Option<String>,
    pub votes: f64,
}

#[derive(Debug, Deserialize, Default)]
struct ImagesBlock {
    #[serde(default)]
    posters: Vec<RawArt>,
    #[serde(default)]
    backdrops: Vec<RawArt>,
}

#[derive(Debug, Deserialize, Default)]
struct RawArt {
    #[serde(default)]
    file_path: Option<String>,
    #[serde(default)]
    width: u32,
    #[serde(default)]
    height: u32,
    #[serde(default)]
    iso_639_1: Option<String>,
    #[serde(default)]
    vote_average: f64,
}

/// Posters and backdrops TMDB offers for one title. The default detail call
/// already appends `images`, but only for the logo; this asks the images
/// endpoint directly so the picker gets the full lists.
pub fn images(
    api_key: &str,
    language: &str,
    languages: &[String],
    target: Target,
    id: u64,
) -> Result<(Vec<ArtImage>, Vec<ArtImage>), ()> {
    // The list must carry every language the artwork rules can ask for; TMDB
    // otherwise answers only for the primary one.
    let mut include: Vec<String> = Vec::new();
    for tag in languages.iter().map(String::as_str).chain([language]) {
        let lang2 = tag
            .split('-')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if !lang2.is_empty() && !include.iter().any(|held| held == &lang2) {
            include.push(lang2);
        }
    }
    if !include.iter().any(|held| held == "en") {
        include.push("en".to_string());
    }
    include.push("null".to_string());
    let params = vec![
        ("language", language.to_string()),
        ("include_image_language", include.join(",")),
    ];
    let block: ImagesBlock = tmdb_json(
        "metadata.images",
        &format!("{}/{}/{id}/images", api(), target.detail_path()),
        api_key,
        &params,
    )?;
    let map = |raw: Vec<RawArt>, size: &str| -> Vec<ArtImage> {
        let mut unique: Vec<ArtImage> = Vec::new();
        for image in raw {
            let Some(path) = image.file_path else { continue };
            let candidate = ArtImage {
                url: format!("{IMG}/{size}{path}"),
                width: image.width,
                height: image.height,
                language: image.iso_639_1,
                votes: image.vote_average,
            };
            match unique.iter_mut().find(|seen| seen.url == candidate.url) {
                Some(seen)
                    if candidate
                        .votes
                        .total_cmp(&seen.votes)
                        .then((candidate.width * candidate.height).cmp(&(seen.width * seen.height)))
                        .is_gt() => *seen = candidate,
                Some(_) => {}
                None => unique.push(candidate),
            }
        }
        unique.sort_by(|a, b| {
            b.votes
                .total_cmp(&a.votes)
                .then((b.width * b.height).cmp(&(a.width * a.height)))
                .then(a.url.cmp(&b.url))
        });
        unique
    };
    Ok((map(block.posters, "w500"), map(block.backdrops, "w1280")))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Movie,
    Tv,
}

impl Target {
    pub(super) fn search_path(self) -> &'static str {
        match self {
            Target::Movie => "search/movie",
            Target::Tv => "search/tv",
        }
    }
    pub(super) fn detail_path(self) -> &'static str {
        match self {
            Target::Movie => "movie",
            Target::Tv => "tv",
        }
    }
    /// TMDB uses a different year query param for movies vs. shows.
    /// `primary_release_year` is the precise movie filter Seerr/Overseerr use.
    pub(super) fn year_param(self) -> &'static str {
        match self {
            Target::Movie => "primary_release_year",
            Target::Tv => "first_air_date_year",
        }
    }
    fn web_kind(self) -> &'static str {
        self.detail_path()
    }
    fn certification_append(self) -> &'static str {
        match self {
            Target::Movie => "release_dates",
            Target::Tv => "content_ratings",
        }
    }
}

const MAX_CAST: usize = 12;
const MAX_CREW: usize = 8;

// TMDB returns keywords unordered; this bounds how much feeds the embedding doc.
const MAX_KEYWORDS: usize = 20;

#[cfg(test)]
mod tests;
