//! Fanart.tv: the optional second artwork source. The administrator supplies
//! their own API key; answers are cached for a week, and requests are spaced
//! so a large sweep does not hammer the service.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use serde::Deserialize;

use super::client::{ArtImage, Target};

const CACHE_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const MIN_GAP: Duration = Duration::from_millis(1100);
const API: &str = "https://webservice.fanart.tv/v3";

static LAST_CALL: Mutex<Option<Instant>> = Mutex::new(None);

#[derive(Debug, Default)]
pub struct FanartLists {
    pub posters: Vec<ArtImage>,
    pub backdrops: Vec<ArtImage>,
    pub logos: Vec<ArtImage>,
    /// Season posters keyed by season number (shows only).
    pub seasons: BTreeMap<u32, Vec<ArtImage>>,
}

#[derive(Debug, Deserialize, Default)]
struct RawLists {
    #[serde(default)]
    movieposter: Vec<Raw>,
    #[serde(default)]
    moviebackground: Vec<Raw>,
    #[serde(default)]
    hdmovielogo: Vec<Raw>,
    #[serde(default)]
    movielogo: Vec<Raw>,
    #[serde(default)]
    tvposter: Vec<Raw>,
    #[serde(default)]
    showbackground: Vec<Raw>,
    #[serde(default)]
    hdtvlogo: Vec<Raw>,
    #[serde(default)]
    clearlogo: Vec<Raw>,
    #[serde(default)]
    seasonposter: Vec<Raw>,
}

#[derive(Debug, Deserialize, Default)]
struct Raw {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    lang: Option<String>,
    #[serde(default)]
    likes: f64,
    #[serde(default)]
    width: u32,
    #[serde(default)]
    height: u32,
    #[serde(default)]
    season: Option<serde_json::Value>,
}

impl Raw {
    fn into_image(self) -> Option<ArtImage> {
        let url = self.url?;
        if url.trim().is_empty() {
            return None;
        }
        Some(ArtImage {
            url,
            width: self.width,
            height: self.height,
            language: self.lang.filter(|lang| !lang.trim().is_empty() && lang != "00"),
            votes: self.likes,
            popularity: self.likes,
        })
    }
}

fn cache_path(data_dir: &Path, target: Target, tmdb_id: u64) -> PathBuf {
    let kind = match target {
        Target::Movie => "movie",
        Target::Tv => "tv",
    };
    data_dir
        .join("fanart")
        .join(format!("{kind}-{tmdb_id}.json"))
}

fn fresh(path: &Path) -> bool {
    path.metadata()
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|at| SystemTime::now().duration_since(at).ok())
        .is_some_and(|age| age < CACHE_TTL)
}

fn pace() {
    let mut held = LAST_CALL.lock().unwrap_or_else(|poison| poison.into_inner());
    if let Some(last) = *held {
        let elapsed = last.elapsed();
        if elapsed < MIN_GAP {
            std::thread::sleep(MIN_GAP - elapsed);
        }
    }
    *held = Some(Instant::now());
}

fn fetch_body(api_key: &str, target: Target, tmdb_id: u64) -> Option<String> {
    let kind = match target {
        Target::Movie => "movies",
        Target::Tv => "tv",
    };
    pace();
    let response = kroma_http::Fetch::new()
        .purpose("metadata.fanart")
        .max_time(20)
        .query("api_key", api_key)
        .get(&format!("{API}/{kind}/{tmdb_id}"))
        .ok()?
        .ensure_ok()
        .ok()?;
    Some(response.text())
}

/// Every Fanart list for one title, from the week-long cache when possible.
/// `None` when no key is configured or the service could not answer.
pub fn fetch(
    data_dir: &Path,
    api_key: &str,
    target: Target,
    tmdb_id: u64,
) -> Option<FanartLists> {
    if api_key.trim().is_empty() || tmdb_id == 0 {
        return None;
    }
    let path = cache_path(data_dir, target, tmdb_id);
    let body = if fresh(&path) {
        std::fs::read_to_string(&path).ok()?
    } else {
        let body = fetch_body(api_key, target, tmdb_id)?;
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, &body).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
        body
    };
    let raw: RawLists = serde_json::from_str(&body).ok()?;
    let mut seasons: BTreeMap<u32, Vec<ArtImage>> = BTreeMap::new();
    for entry in raw.seasonposter {
        let Some(number) = entry
            .season
            .as_ref()
            .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok()))
            .map(|number| number as u32)
        else {
            continue;
        };
        if let Some(image) = entry.into_image() {
            seasons.entry(number).or_default().push(image);
        }
    }
    Some(FanartLists {
        posters: raw
            .movieposter
            .into_iter()
            .chain(raw.tvposter)
            .filter_map(Raw::into_image)
            .collect(),
        backdrops: raw
            .moviebackground
            .into_iter()
            .chain(raw.showbackground)
            .filter_map(Raw::into_image)
            .collect(),
        logos: raw
            .hdmovielogo
            .into_iter()
            .chain(raw.movielogo)
            .chain(raw.hdtvlogo)
            .chain(raw.clearlogo)
            .filter_map(Raw::into_image)
            .collect(),
        seasons,
    })
}