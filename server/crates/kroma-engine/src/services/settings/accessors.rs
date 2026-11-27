//! Typed, functional accessors over the raw store: the settings the server
//! actually reads to change behaviour (LAN nets, server name, transcode cap),
//! plus the persisted multi-folder library definitions.

use std::collections::BTreeMap;
use std::path::PathBuf;

use kroma_domain::DeploymentMode;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::db::Pool;

use super::store::Settings;

/// The configured local networks (CIDR or prefix strings) used to classify a
/// client IP as LAN vs WAN. Comma/space separated.
pub fn local_networks(settings: &Settings) -> Vec<String> {
    settings
        .get_str("localNetworks", "192.168.0.0/16, 10.0.0.0/8, 172.16.0.0/12")
        .split([',', ' '])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// A stable opaque identity for THIS install, minted on first boot and persisted
/// with the settings. Opaque and non-guessable: it is served on the public health
/// endpoint, so it must carry no information about the box.
pub fn ensure_instance_id(settings: &Settings, pool: &Pool) -> String {
    let existing = settings.get_str("instanceId", "");
    if !existing.trim().is_empty() {
        return existing;
    }
    let id = crate::services::auth::random_token();
    if let Err(error) = settings.set_internal(pool, "instanceId", json!(id.clone())) {
        tracing::warn!(%error, "could not store this install's id; the next start mints another");
    }
    id
}

/// The persisted display name for the server (falls back to "KROMA").
pub fn server_name(settings: &Settings) -> String {
    let n = settings.get_str("serverName", "KROMA");
    if n.trim().is_empty() {
        "KROMA".to_string()
    } else {
        n
    }
}

/// The percent of a runtime past which a title counts as watched when it carries
/// no credits marker (`playedThreshold`), held between 50 and 100.
pub fn played_percent(settings: &Settings) -> i64 {
    settings
        .get_i64("playedThreshold", kroma_domain::DEFAULT_PLAYED_PERCENT)
        .clamp(50, 100)
}

/// Max concurrent HLS sessions (functional cap), 1..=32. Seeds the semaphore in
/// [`kroma_media::hls::HlsEngine`].
pub fn max_transcodes(settings: &Settings) -> usize {
    settings.get_i64("maxConcurrent", 8).clamp(1, 32) as usize
}

/// How many CPU-heavy media passes (storyboard tiles/montage, subtitle extraction,
/// marker fingerprinting) may run at once, per
/// [`kroma_media::ffmpeg_gate::capacity_from`]. Seeds + live-updates the
/// process-wide gate.
pub fn media_workers(settings: &Settings) -> usize {
    kroma_media::ffmpeg_gate::capacity_from(settings.get_i64("mediaConcurrency", 0))
}

/// Byte budget for the on-disk transcode (HLS segment) cache, from the
/// `transcodeCacheLimit` setting. `0` = unlimited (any non-numeric label, e.g.
/// "Illimité"). Labels use decimal "Go" (1 Go = 1e9 bytes), matching the image
/// `cacheLimit`. Seeds the budget in [`kroma_media::hls::HlsEngine`].
pub fn transcode_cache_limit_bytes(settings: &Settings) -> u64 {
    let label = settings.get_str("transcodeCacheLimit", "20 Go");
    // Take the leading numeric run INCLUDING a decimal point (e.g. "1.5 Go" -> 1.5,
    // "20 Go" -> 20). Filtering the '.' out would concatenate "1.5" into "15" (a 10x
    // error); a non-numeric label ("Illimité") parses to nothing => unlimited (0).
    let num: String = label
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    match num.parse::<f64>() {
        Ok(gb) if gb > 0.0 => (gb * 1_000_000_000.0) as u64,
        _ => 0,
    }
}

/// Whether Plex-style theme songs are enabled: enrichment downloads a show's
/// theme and the detail page loops it. Opt-in (off by default).
pub fn theme_songs_enabled(settings: &Settings) -> bool {
    settings.get_bool("themeSongs", false)
}

/// Whether the managed `cloudflared` connector is enabled (off by default). When
/// on and a token is stored, the server supervises the tunnel (see
/// the `kroma-remote` crate). Installs with their own tunnel leave this off.
pub fn remote_access_enabled(settings: &Settings) -> bool {
    settings.get_bool("remoteAccess", false)
}

/// The public base URL clients reach the server at (e.g. `https://kroma.example.com`),
/// used for share / Quick Connect links. Trailing slash trimmed; empty if unset.
pub fn public_url(settings: &Settings) -> String {
    settings
        .get_str("remoteUrl", "")
        .trim()
        .trim_end_matches('/')
        .to_string()
}

/// The stored Cloudflare Tunnel token for the managed connector. Secret never
/// returned to clients (the admin API exposes only a `hasToken` bool).
pub fn remote_access_token(settings: &Settings) -> String {
    settings.get_str("remoteAccessToken", "")
}

/// Persist the remote-access config. `token` is `Some` only when the admin
/// provided a new value: a blank field keeps the stored secret rather than wiping
/// it.
pub fn set_remote_config(
    settings: &Settings,
    pool: &Pool,
    enabled: bool,
    url: &str,
    token: Option<&str>,
) {
    let mut patch = BTreeMap::new();
    patch.insert("remoteAccess".to_string(), json!(enabled));
    patch.insert("remoteUrl".to_string(), json!(url.trim()));
    if let Some(tok) = token {
        patch.insert("remoteAccessToken".to_string(), json!(tok));
    }
    settings.set_patch(pool, patch);
}

/// A named, runtime-editable library spanning one or more scan folders. Persisted
/// in the settings store under the `libraries` key, seeded from `KROMA_MEDIA_DIRS`
/// on first run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryDef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub kind: String,
    pub folders: Vec<String>,
    #[serde(rename = "autoScan", default = "default_true")]
    pub auto_scan: bool,
}

impl LibraryDef {
    pub fn catalog_kind(&self, detected: crate::model::LibraryKind) -> crate::model::LibraryKind {
        use crate::model::LibraryKind;
        match self.kind.as_str() {
            "movies" => LibraryKind::Movies,
            "shows" => LibraryKind::Shows,
            "mixed" => LibraryKind::Mixed,
            "photos" | "photo" => LibraryKind::Photos,
            "other" => LibraryKind::Other,
            _ => detected,
        }
    }
}

fn default_true() -> bool {
    true
}

/// Per-library scraping choices, kept under the `libraryScrape` settings key so
/// the persisted `LibraryDef` shape (and every constructor of it) stays put.
/// `None`/empty means "follow the global setting"; the language values are TMDB
/// tags ("zh-CN"), or "textless" for artwork with no printed title.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ScrapeOptions {
    /// The primary language, kept in sync with `language_priority[0]` so every
    /// reader that still asks for one tag keeps working.
    pub language: Option<String>,
    /// Ordered language fallbacks: TMDB is asked in the first one, the rest
    /// fill fields the primary translation does not carry. None = follow the
    /// global default, never "no language".
    pub language_priority: Option<Vec<String>>,
    pub nfo: bool,
    /// `None` follows the household NFO default; `Some` overrides it. The
    /// plain `nfo` above stays for old stored rows and for the effective value
    /// the enrichment pass reads.
    pub nfo_override: Option<bool>,
    pub poster_language: Option<String>,
    pub backdrop_language: Option<String>,
    /// Ordered artwork languages: "textless" picks the clean plate, a tag
    /// picks that language, "auto" accepts any. None = follow the default.
    pub poster_languages: Option<Vec<String>>,
    pub backdrop_languages: Option<Vec<String>>,
    /// Smallest candidate accepted before falling back to a relaxed pass.
    pub poster_min_width: Option<u16>,
    pub backdrop_min_width: Option<u16>,
    pub poster_min_votes: Option<u32>,
    pub backdrop_min_votes: Option<u32>,
    /// Per-artwork provider size; None keeps the provider's URL.
    pub poster_size: Option<String>,
    pub backdrop_size: Option<String>,
    pub logo_languages: Option<Vec<String>>,
    pub logo_min_width: Option<u16>,
    pub logo_size: Option<String>,
    pub season_languages: Option<Vec<String>>,
    pub season_min_width: Option<u16>,
    pub season_size: Option<String>,
    /// Where each kind of artwork may come from, in order: "tmdb", "fanart".
    pub poster_sources: Option<Vec<String>>,
    pub backdrop_sources: Option<Vec<String>>,
    pub logo_sources: Option<Vec<String>>,
    pub season_sources: Option<Vec<String>>,
    /// Write the chosen poster/backdrop beside the media as poster.jpg /
    /// fanart.jpg. Existing files are never replaced; the household default
    /// carries this, a library does not override it yet.
    pub write_images: bool,
}

/// Trim, dedupe and cap an ordered language list. The cap is what the UI
/// promises: a primary plus at most two fallbacks.
pub fn clean_language_priority(values: Vec<String>) -> Vec<String> {
    clean_languages(values, 3)
}

fn clean_languages(values: Vec<String>, cap: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in values {
        let tag = tag.trim();
        if tag.is_empty() || tag.len() > 16 {
            continue;
        }
        if !out.iter().any(|held| held.eq_ignore_ascii_case(tag)) {
            out.push(tag.to_string());
        }
        if out.len() == cap {
            break;
        }
    }
    out
}

/// Artwork preferences accept the same tags as text plus the two special
/// values: "textless" and "auto".
fn clean_art_languages(values: Vec<String>) -> Vec<String> {
    clean_languages(values, 4)
}

fn valid_art_size(size: &str) -> Option<String> {
    let size = size.trim();
    matches!(size, "w500" | "w780" | "w1280" | "original").then(|| size.to_string())
}

/// "tmdb" and "fanart", once each, in the order given.
fn clean_sources(values: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for value in values {
        let source = value.trim().to_ascii_lowercase();
        if (source == "tmdb" || source == "fanart") && !out.contains(&source) {
            out.push(source);
        }
    }
    out
}

impl ScrapeOptions {
    /// No override at all: every optional field is unset. `write_images` and
    /// the legacy `nfo` bool are not per-library overrides.
    pub fn follows_global(&self) -> bool {
        self.language.is_none()
            && self.language_priority.is_none()
            && self.nfo_override.is_none()
            && self.poster_language.is_none()
            && self.backdrop_language.is_none()
            && self.poster_languages.is_none()
            && self.backdrop_languages.is_none()
            && self.poster_min_width.is_none()
            && self.backdrop_min_width.is_none()
            && self.poster_min_votes.is_none()
            && self.backdrop_min_votes.is_none()
            && self.poster_size.is_none()
            && self.backdrop_size.is_none()
            && self.logo_languages.is_none()
            && self.logo_min_width.is_none()
            && self.logo_size.is_none()
            && self.season_languages.is_none()
            && self.season_min_width.is_none()
            && self.season_size.is_none()
            && self.poster_sources.is_none()
            && self.backdrop_sources.is_none()
            && self.logo_sources.is_none()
            && self.season_sources.is_none()
    }

    fn sane(mut self) -> Self {
        self.language_priority = self
            .language_priority
            .map(clean_language_priority)
            .filter(|list| !list.is_empty());
        if self.language_priority.is_none() {
            self.language_priority = self
                .language
                .as_deref()
                .map(|tag| clean_language_priority(vec![tag.to_string()]))
                .filter(|list| !list.is_empty());
        }
        if let Some(first) = self
            .language_priority
            .as_ref()
            .and_then(|list| list.first())
        {
            self.language = Some(first.clone());
        }
        self.poster_languages = self
            .poster_languages
            .map(clean_art_languages)
            .filter(|list| !list.is_empty());
        if self.poster_languages.is_none() {
            self.poster_languages = self
                .poster_language
                .as_deref()
                .map(str::trim)
                .filter(|tag| !tag.is_empty() && !tag.eq_ignore_ascii_case("auto"))
                .map(|tag| clean_art_languages(vec![tag.to_string()]))
                .filter(|list| !list.is_empty());
        }
        if let Some(first) = self.poster_languages.as_ref().and_then(|list| list.first()) {
            self.poster_language = Some(first.clone());
        }
        self.backdrop_languages = self
            .backdrop_languages
            .map(clean_art_languages)
            .filter(|list| !list.is_empty());
        if self.backdrop_languages.is_none() {
            self.backdrop_languages = self
                .backdrop_language
                .as_deref()
                .map(str::trim)
                .filter(|tag| !tag.is_empty() && !tag.eq_ignore_ascii_case("auto"))
                .map(|tag| clean_art_languages(vec![tag.to_string()]))
                .filter(|list| !list.is_empty());
        }
        if let Some(first) = self
            .backdrop_languages
            .as_ref()
            .and_then(|list| list.first())
        {
            self.backdrop_language = Some(first.clone());
        }
        self.poster_size = self.poster_size.as_deref().and_then(valid_art_size);
        self.backdrop_size = self.backdrop_size.as_deref().and_then(valid_art_size);
        self.poster_min_width = self.poster_min_width.filter(|width| *width > 0);
        self.backdrop_min_width = self.backdrop_min_width.filter(|width| *width > 0);
        self.poster_min_votes = self.poster_min_votes.filter(|votes| *votes > 0);
        self.backdrop_min_votes = self.backdrop_min_votes.filter(|votes| *votes > 0);
        self.logo_languages = self
            .logo_languages
            .map(clean_art_languages)
            .filter(|list| !list.is_empty());
        self.logo_min_width = self.logo_min_width.filter(|width| *width > 0);
        self.logo_size = self.logo_size.as_deref().and_then(valid_art_size);
        self.season_languages = self
            .season_languages
            .map(clean_art_languages)
            .filter(|list| !list.is_empty());
        self.season_min_width = self.season_min_width.filter(|width| *width > 0);
        self.season_size = self.season_size.as_deref().and_then(valid_art_size);
        self.poster_sources = self
            .poster_sources
            .map(clean_sources)
            .filter(|list| !list.is_empty());
        self.backdrop_sources = self
            .backdrop_sources
            .map(clean_sources)
            .filter(|list| !list.is_empty());
        self.logo_sources = self
            .logo_sources
            .map(clean_sources)
            .filter(|list| !list.is_empty());
        self.season_sources = self
            .season_sources
            .map(clean_sources)
            .filter(|list| !list.is_empty());
        self
    }
}

pub fn library_scrape(settings: &Settings, library_id: &str) -> ScrapeOptions {
    settings
        .get("libraryScrape")
        .get(library_id)
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .map(ScrapeOptions::sane)
        .map(migrate_nfo)
        .unwrap_or_default()
}

pub fn all_library_scrape(settings: &Settings) -> BTreeMap<String, ScrapeOptions> {
    settings
        .get("libraryScrape")
        .as_object()
        .map(|map| {
            map.iter()
                .filter_map(|(id, value)| {
                    serde_json::from_value::<ScrapeOptions>(value.clone())
                        .ok()
                        .map(ScrapeOptions::sane)
                        .map(migrate_nfo)
                        .map(|options| (id.clone(), options))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Which storage mode this deployment runs. Unknown or reserved values fall
/// back to cloud-only rather than enabling a mode the build cannot serve.
pub fn deployment_mode(settings: &Settings) -> DeploymentMode {
    DeploymentMode::parse(&settings.get_str("deploymentMode", "cloud_only"))
        .filter(|mode| mode.available())
        .unwrap_or_default()
}

/// The 115 folder Symedia watches, normalized to a leading slash. Empty when
/// the operator has not configured one; acquisition must then refuse to start.
pub fn cloud_intake_folder(settings: &Settings) -> String {
    let raw = settings.get_str("cloudIntakeFolder", "");
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    }
}

/// How often share subscriptions are re-listed, in minutes. Clamped to 5..=1440.
pub fn share_poll_minutes(settings: &Settings) -> i64 {
    settings.get_i64("sharePollMinutes", 30).clamp(5, 1440)
}

/// Old rows stored a plain `nfo` bool, where `false` could not mean "follow
/// the global default". An old `true` was an explicit choice, so it becomes an
/// override; an old `false` becomes "follow global".
fn migrate_nfo(mut options: ScrapeOptions) -> ScrapeOptions {
    if options.nfo_override.is_none() && options.nfo {
        options.nfo_override = Some(true);
    }
    options
}

pub fn set_library_scrape(
    settings: &Settings,
    pool: &Pool,
    library_id: &str,
    options: &ScrapeOptions,
) {
    let mut map = settings
        .get("libraryScrape")
        .as_object()
        .cloned()
        .unwrap_or_default();
    let options = options.clone().sane();
    if options.follows_global() {
        map.remove(library_id);
    } else {
        map.insert(
            library_id.to_string(),
            serde_json::to_value(options).unwrap_or_default(),
        );
    }
    let mut patch = BTreeMap::new();
    patch.insert("libraryScrape".to_string(), Value::Object(map));
    settings.set_patch(pool, patch);
}

/// The household defaults every library follows until it overrides them.
/// Stored under `scrapeDefaults`; the primary tag is mirrored into
/// `tmdbLanguage`, which is what the enrichment engine has always read.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ScrapeDefaults {
    pub language_priority: Option<Vec<String>>,
    pub nfo: bool,
    pub poster_languages: Option<Vec<String>>,
    pub backdrop_languages: Option<Vec<String>>,
    pub poster_min_width: Option<u16>,
    pub backdrop_min_width: Option<u16>,
    pub poster_min_votes: Option<u32>,
    pub backdrop_min_votes: Option<u32>,
    pub poster_size: Option<String>,
    pub backdrop_size: Option<String>,
    pub logo_languages: Option<Vec<String>>,
    pub logo_min_width: Option<u16>,
    pub logo_size: Option<String>,
    pub season_languages: Option<Vec<String>>,
    pub season_min_width: Option<u16>,
    pub season_size: Option<String>,
    pub poster_sources: Option<Vec<String>>,
    pub backdrop_sources: Option<Vec<String>>,
    pub logo_sources: Option<Vec<String>>,
    pub season_sources: Option<Vec<String>>,
    pub write_images: bool,
    /// Refresh show artwork after this many days; `None` disables the job.
    pub artwork_refresh_days: Option<u16>,
}

fn clean_defaults(mut defaults: ScrapeDefaults) -> ScrapeDefaults {
    defaults.language_priority = defaults
        .language_priority
        .map(clean_language_priority)
        .filter(|list| !list.is_empty());
    defaults.poster_languages = defaults
        .poster_languages
        .map(clean_art_languages)
        .filter(|list| !list.is_empty());
    defaults.backdrop_languages = defaults
        .backdrop_languages
        .map(clean_art_languages)
        .filter(|list| !list.is_empty());
    defaults.poster_size = defaults.poster_size.as_deref().and_then(valid_art_size);
    defaults.backdrop_size = defaults.backdrop_size.as_deref().and_then(valid_art_size);
    defaults.poster_min_width = defaults.poster_min_width.filter(|width| *width > 0);
    defaults.backdrop_min_width = defaults.backdrop_min_width.filter(|width| *width > 0);
    defaults.poster_min_votes = defaults.poster_min_votes.filter(|votes| *votes > 0);
    defaults.backdrop_min_votes = defaults.backdrop_min_votes.filter(|votes| *votes > 0);
    defaults.logo_languages = defaults
        .logo_languages
        .map(clean_art_languages)
        .filter(|list| !list.is_empty());
    defaults.logo_min_width = defaults.logo_min_width.filter(|width| *width > 0);
    defaults.logo_size = defaults.logo_size.as_deref().and_then(valid_art_size);
    defaults.season_languages = defaults
        .season_languages
        .map(clean_art_languages)
        .filter(|list| !list.is_empty());
    defaults.season_min_width = defaults.season_min_width.filter(|width| *width > 0);
    defaults.season_size = defaults.season_size.as_deref().and_then(valid_art_size);
    defaults.poster_sources = defaults
        .poster_sources
        .map(clean_sources)
        .filter(|list| !list.is_empty());
    defaults.backdrop_sources = defaults
        .backdrop_sources
        .map(clean_sources)
        .filter(|list| !list.is_empty());
    defaults.logo_sources = defaults
        .logo_sources
        .map(clean_sources)
        .filter(|list| !list.is_empty());
    defaults.season_sources = defaults
        .season_sources
        .map(clean_sources)
        .filter(|list| !list.is_empty());
    defaults.artwork_refresh_days = defaults
        .artwork_refresh_days
        .filter(|days| *days > 0)
        .map(|days| days.min(365));
    defaults
}

pub fn scrape_defaults(settings: &Settings, config: &crate::config::Config) -> ScrapeDefaults {
    let mut defaults =
        clean_defaults(serde_json::from_value(settings.get("scrapeDefaults")).unwrap_or_default());
    if defaults.language_priority.is_none() {
        defaults.language_priority = Some(vec![super::metadata_language(settings, config)]);
    }
    defaults
}

pub fn set_scrape_defaults(settings: &Settings, pool: &Pool, defaults: &ScrapeDefaults) {
    let stored = clean_defaults(defaults.clone());
    let mut patch = BTreeMap::new();
    patch.insert(
        "scrapeDefaults".to_string(),
        serde_json::to_value(&stored).unwrap_or_default(),
    );
    if let Some(first) = stored
        .language_priority
        .as_ref()
        .and_then(|list| list.first())
    {
        patch.insert("tmdbLanguage".to_string(), json!(first));
    }
    settings.set_patch(pool, patch);
}

/// The administrator's personal Fanart.tv API key. Empty disables the source.
pub const FANART_KEY: &str = "fanartApiKey";

pub fn fanart_api_key(settings: &Settings) -> String {
    settings.get_str(FANART_KEY, "").trim().to_string()
}

pub fn set_fanart_api_key(settings: &Settings, pool: &Pool, key: &str) {
    let mut patch = BTreeMap::new();
    patch.insert(FANART_KEY.to_string(), json!(key.trim()));
    settings.set_patch(pool, patch);
}

/// A library's options with the global language filled in when the library
/// itself has no list. Artwork and NFO choices pass through untouched.
pub fn effective_library_scrape(
    settings: &Settings,
    config: &crate::config::Config,
    library_id: &str,
) -> ScrapeOptions {
    let mut options = library_scrape(settings, library_id);
    if options.language_priority.is_none() {
        options.language_priority = scrape_defaults(settings, config).language_priority;
        options.language = options
            .language_priority
            .as_ref()
            .and_then(|list| list.first())
            .cloned();
    }
    options
}

/// The effective library definitions: persisted defs if present, else seeded
/// from the env-configured media dirs. Seeding prefers the typed
/// `KROMA_MOVIES_DIRS` / `KROMA_SERIES_DIRS` roots (one "Films" / "Séries" library
/// each) and always keeps the untyped one-per-folder `KROMA_MEDIA_DIRS` seed for
/// backward compatibility.
///
/// With nothing configured at all, the demo library under `<data>/demo-media` is
/// the seed. A saved list that cannot be read is an error and never falls back
/// to a seed: a scan of the seed would remove the libraries the owner saved.
pub fn library_defs(
    settings: &Settings,
    config: &crate::config::Config,
) -> Result<Vec<LibraryDef>, super::UnreadableLibraryList> {
    if let Some(saved) = super::stored_library_defs(settings)? {
        return Ok(saved);
    }
    let mut defs = Vec::new();
    if !config.movies_dirs.is_empty() {
        defs.push(LibraryDef {
            id: crate::services::scan::short_hash("lib|movies"),
            name: "Films".to_string(),
            kind: "movies".to_string(),
            folders: config
                .movies_dirs
                .iter()
                .map(|d| d.to_string_lossy().to_string())
                .collect(),
            auto_scan: true,
        });
    }
    if !config.series_dirs.is_empty() {
        defs.push(LibraryDef {
            id: crate::services::scan::short_hash("lib|shows"),
            name: "Séries".to_string(),
            kind: "shows".to_string(),
            folders: config
                .series_dirs
                .iter()
                .map(|d| d.to_string_lossy().to_string())
                .collect(),
            auto_scan: true,
        });
    }
    defs.extend(config.media_dirs.iter().map(|dir| {
        let path = dir.to_string_lossy().to_string();
        let name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Bibliothèque")
            .to_string();
        LibraryDef {
            id: crate::services::scan::short_hash(&path),
            name,
            kind: String::new(),
            folders: vec![path],
            auto_scan: true,
        }
    }));
    if defs.is_empty() {
        return Ok(demo_defs(config));
    }
    Ok(defs)
}

// Only the ones that EXIST. A definition for a folder nobody wrote would scan as
// zero items with folders configured, which is how an unmounted share looks: the
// scan would keep the stored index and the row-only demo seed would never run.
const DEMO_MOVIES: &str = "lib|demo|movies";
const DEMO_SHOWS: &str = "lib|demo|shows";

/// Whether `id` names a demo library: what a server with no media of its own
/// shows until somebody points it at a folder.
pub fn is_demo_library(id: &str) -> bool {
    [DEMO_MOVIES, DEMO_SHOWS]
        .iter()
        .any(|seed| crate::services::scan::short_hash(seed) == id)
}

fn demo_defs(config: &crate::config::Config) -> Vec<LibraryDef> {
    let (movies, shows) = crate::services::demo::media::roots(&config.data_dir);
    let mut defs = Vec::new();
    if movies.is_dir() {
        defs.push(LibraryDef {
            id: crate::services::scan::short_hash(DEMO_MOVIES),
            name: "Films (Démo)".to_string(),
            kind: "movies".to_string(),
            folders: vec![movies.to_string_lossy().to_string()],
            auto_scan: true,
        });
    }
    if shows.is_dir() {
        defs.push(LibraryDef {
            id: crate::services::scan::short_hash(DEMO_SHOWS),
            name: "Séries (Démo)".to_string(),
            kind: "shows".to_string(),
            folders: vec![shows.to_string_lossy().to_string()],
            auto_scan: true,
        });
    }
    defs
}

/// Every folder of every library. None when the saved list cannot be read.
pub fn all_folders(settings: &Settings, config: &crate::config::Config) -> Vec<PathBuf> {
    library_defs(settings, config)
        .unwrap_or_default()
        .into_iter()
        .flat_map(|d| d.folders.into_iter().map(PathBuf::from))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::testing::TempPool;

    fn test_pool() -> TempPool {
        crate::db::testing::temp_pool("settings-acc")
    }

    fn settings(pool: &Pool) -> Settings {
        Settings::load(pool)
    }

    // A Config with explicit fields (avoids reading the process env).
    fn test_config() -> crate::config::Config {
        crate::config::Config {
            host: "0.0.0.0".to_string(),
            port: 4040,
            data_dir: PathBuf::from("/tmp/kroma-data"),
            tmdb_language: "en-US".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn local_networks_splits_default_and_custom() {
        let pool = test_pool();
        let s = settings(&pool);
        let nets = local_networks(&s);
        assert!(nets.contains(&"192.168.0.0/16".to_string()));
        assert_eq!(nets.len(), 3);
        s.set_patch(
            &pool,
            BTreeMap::from([(
                "localNetworks".to_string(),
                json!("10.0.0.0/8,  172.16.0.0/12 ,"),
            )]),
        );
        let nets = local_networks(&s);
        assert_eq!(
            nets,
            vec!["10.0.0.0/8".to_string(), "172.16.0.0/12".to_string()]
        );
    }

    #[test]
    fn server_name_defaults_and_guards_blank() {
        let pool = test_pool();
        let s = settings(&pool);
        assert_eq!(server_name(&s), "KROMA");
        s.set_patch(
            &pool,
            BTreeMap::from([("serverName".to_string(), json!("Home"))]),
        );
        assert_eq!(server_name(&s), "Home");
        s.set_patch(
            &pool,
            BTreeMap::from([("serverName".to_string(), json!("   "))]),
        );
        assert_eq!(server_name(&s), "KROMA"); // blank -> fallback
    }

    #[test]
    fn the_played_threshold_defaults_to_ninety_and_stays_between_half_and_all() {
        let pool = test_pool();
        let s = settings(&pool);
        assert_eq!(played_percent(&s), 90);

        s.set_patch(
            &pool,
            BTreeMap::from([("playedThreshold".to_string(), json!("20"))]),
        );
        assert_eq!(played_percent(&s), 50);

        s.set_patch(
            &pool,
            BTreeMap::from([("playedThreshold".to_string(), json!(85))]),
        );
        assert_eq!(played_percent(&s), 85);
    }

    #[test]
    fn max_transcodes_parses_and_clamps() {
        let pool = test_pool();
        let s = settings(&pool);
        assert_eq!(max_transcodes(&s), 8); // default "8"
        s.set_patch(
            &pool,
            BTreeMap::from([("maxConcurrent".to_string(), json!("12"))]),
        );
        assert_eq!(max_transcodes(&s), 12);
        s.set_patch(
            &pool,
            BTreeMap::from([("maxConcurrent".to_string(), json!(0))]),
        );
        assert_eq!(max_transcodes(&s), 1); // clamped up
        s.set_patch(
            &pool,
            BTreeMap::from([("maxConcurrent".to_string(), json!(999))]),
        );
        assert_eq!(max_transcodes(&s), 32); // clamped down
    }

    #[test]
    fn media_workers_explicit_and_auto() {
        let pool = test_pool();
        let s = settings(&pool);
        assert!(media_workers(&s) >= 1);
        s.set_patch(
            &pool,
            BTreeMap::from([("mediaConcurrency".to_string(), json!("4"))]),
        );
        assert_eq!(media_workers(&s), 4);
        s.set_patch(
            &pool,
            BTreeMap::from([("mediaConcurrency".to_string(), json!(99))]),
        );
        assert_eq!(media_workers(&s), 32); // clamped
    }

    #[test]
    fn transcode_cache_limit_parses_decimal_and_unlimited() {
        let pool = test_pool();
        let s = settings(&pool);
        assert_eq!(transcode_cache_limit_bytes(&s), 20_000_000_000); // default "20 Go"
        s.set_patch(
            &pool,
            BTreeMap::from([("transcodeCacheLimit".to_string(), json!("1.5 Go"))]),
        );
        assert_eq!(transcode_cache_limit_bytes(&s), 1_500_000_000);
        s.set_patch(
            &pool,
            BTreeMap::from([("transcodeCacheLimit".to_string(), json!("Illimité"))]),
        );
        assert_eq!(transcode_cache_limit_bytes(&s), 0); // non-numeric -> unlimited
        s.set_patch(
            &pool,
            BTreeMap::from([("transcodeCacheLimit".to_string(), json!("0 Go"))]),
        );
        assert_eq!(transcode_cache_limit_bytes(&s), 0);
    }

    #[test]
    fn simple_bool_and_url_accessors() {
        let pool = test_pool();
        let s = settings(&pool);
        assert!(!theme_songs_enabled(&s));
        assert!(!remote_access_enabled(&s));
        assert_eq!(public_url(&s), "");
        assert_eq!(remote_access_token(&s), "");
        s.set_patch(
            &pool,
            BTreeMap::from([
                ("themeSongs".to_string(), json!(true)),
                ("remoteAccess".to_string(), json!(true)),
                ("remoteUrl".to_string(), json!("https://kroma.example.com/")),
                ("remoteAccessToken".to_string(), json!("secret")),
            ]),
        );
        assert!(theme_songs_enabled(&s));
        assert!(remote_access_enabled(&s));
        assert_eq!(public_url(&s), "https://kroma.example.com"); // trailing slash trimmed
        assert_eq!(remote_access_token(&s), "secret");
    }

    #[test]
    fn set_remote_config_merges_token() {
        let pool = test_pool();
        let s = settings(&pool);
        set_remote_config(&s, &pool, true, "  https://a.b/  ", Some("tok1"));
        assert!(remote_access_enabled(&s));
        assert_eq!(public_url(&s), "https://a.b");
        assert_eq!(remote_access_token(&s), "tok1");
        set_remote_config(&s, &pool, false, "https://a.b", None);
        assert!(!remote_access_enabled(&s));
        assert_eq!(remote_access_token(&s), "tok1");
    }

    #[test]
    fn library_defs_seed_from_config_when_unset() {
        let pool = test_pool();
        let s = settings(&pool); // libraries default null
        let mut cfg = test_config();
        cfg.movies_dirs = vec![PathBuf::from("/media/films")];
        cfg.series_dirs = vec![PathBuf::from("/media/series")];
        cfg.media_dirs = vec![PathBuf::from("/media/Misc")];
        let defs = library_defs(&s, &cfg).unwrap();
        assert_eq!(defs.len(), 3);
        assert_eq!(defs[0].name, "Films");
        assert_eq!(defs[0].kind, "movies");
        assert_eq!(defs[1].name, "Séries");
        assert_eq!(defs[1].kind, "shows");
        assert_eq!(defs[2].name, "Misc");
        assert!(defs[2].kind.is_empty());
    }

    #[test]
    fn an_instance_id_already_on_disk_is_reused_rather_than_re_minted() {
        let pool = test_pool();
        crate::db::settings_set(&pool, "instanceId", &json!("  kept-id  ")).unwrap();
        let s = settings(&pool);

        assert_eq!(ensure_instance_id(&s, &pool), "  kept-id  ");
    }

    #[test]
    fn a_box_with_no_instance_id_yet_mints_a_non_guessable_one() {
        let pool = test_pool();
        let s = settings(&pool);
        let id = ensure_instance_id(&s, &pool);
        assert_eq!(id.len(), 64, "a 32-byte token, hex-encoded");
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn the_instance_id_a_box_mints_survives_its_next_start() {
        let pool = test_pool();
        let first = ensure_instance_id(&settings(&pool), &pool);

        let restarted = settings(&pool);
        assert_eq!(restarted.get_str("instanceId", ""), first);
        assert_eq!(ensure_instance_id(&restarted, &pool), first);
    }

    #[test]
    fn no_caller_can_choose_the_instance_id_through_the_settings_patch() {
        let pool = test_pool();
        let s = settings(&pool);
        let minted = ensure_instance_id(&s, &pool);

        let written = s.set_patch(
            &pool,
            BTreeMap::from([("instanceId".to_string(), json!("chosen"))]),
        );

        assert!(
            written.is_empty(),
            "the patch allow-list let it through: {written:?}"
        );
        assert_eq!(ensure_instance_id(&s, &pool), minted);
    }

    #[test]
    fn nothing_configured_and_nothing_written_seeds_no_library_at_all() {
        let pool = test_pool();
        let s = settings(&pool);
        let mut cfg = test_config();
        cfg.data_dir = PathBuf::from("/nonexistent/kroma-data");

        assert!(library_defs(&s, &cfg).unwrap().is_empty());
    }

    #[test]
    fn a_written_demo_library_is_seeded_as_two_ordinary_libraries() {
        let pool = test_pool();
        let s = settings(&pool);
        let dir = kroma_testing::temp_dir("demo-defs");
        let mut cfg = test_config();
        cfg.data_dir = dir.path().to_path_buf();
        let (movies, shows) = crate::services::demo::media::roots(dir.path());
        std::fs::create_dir_all(&movies).unwrap();
        std::fs::create_dir_all(&shows).unwrap();

        let defs = library_defs(&s, &cfg).unwrap();

        assert_eq!(defs.len(), 2);
        assert_eq!(defs[0].kind, "movies");
        assert_eq!(defs[0].folders, vec![movies.to_string_lossy().to_string()]);
        assert_eq!(defs[1].kind, "shows");
        assert_eq!(defs[1].folders, vec![shows.to_string_lossy().to_string()]);
        assert_ne!(defs[0].id, defs[1].id);
        assert_ne!(defs[0].id, crate::services::scan::short_hash("lib|movies"));
    }

    #[test]
    fn a_configured_library_wins_over_the_written_demo_one() {
        let pool = test_pool();
        let s = settings(&pool);
        let dir = kroma_testing::temp_dir("demo-defs-configured");
        let (movies, _) = crate::services::demo::media::roots(dir.path());
        std::fs::create_dir_all(&movies).unwrap();
        let mut cfg = test_config();
        cfg.data_dir = dir.path().to_path_buf();
        cfg.movies_dirs = vec![PathBuf::from("/media/films")];

        let defs = library_defs(&s, &cfg).unwrap();

        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "Films");
        assert_eq!(defs[0].folders, vec!["/media/films".to_string()]);
    }

    #[test]
    fn an_untyped_media_dir_is_a_library_on_its_own() {
        let pool = test_pool();
        let s = settings(&pool);
        let dir = kroma_testing::temp_dir("media-dirs-only");
        let (movies, _) = crate::services::demo::media::roots(dir.path());
        std::fs::create_dir_all(&movies).unwrap();
        let mut cfg = test_config();
        cfg.data_dir = dir.path().to_path_buf();
        cfg.media_dirs = vec![PathBuf::from("/media/4k Shows")];

        let defs = library_defs(&s, &cfg).unwrap();

        assert_eq!(defs.len(), 1, "KROMA_MEDIA_DIRS alone configures a library");
        assert_eq!(defs[0].name, "4k Shows");
        assert_eq!(defs[0].folders, vec!["/media/4k Shows".to_string()]);
    }

    #[test]
    fn library_defs_round_trip_persisted() {
        let pool = test_pool();
        let s = settings(&pool);
        let cfg = test_config();
        let defs = vec![LibraryDef {
            id: "lib1".to_string(),
            name: "Cinema".to_string(),
            kind: "movies".to_string(),
            folders: vec!["/a".to_string(), "/b".to_string()],
            auto_scan: false,
        }];
        super::super::set_library_defs(&s, &pool, &defs);
        let got = library_defs(&s, &cfg).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "Cinema");
        assert!(!got[0].auto_scan);
        let folders = all_folders(&s, &cfg);
        assert_eq!(folders, vec![PathBuf::from("/a"), PathBuf::from("/b")]);
    }

    #[test]
    fn library_scrape_is_stored_under_its_own_key_and_reads_back() {
        let pool = test_pool();
        let s = settings(&pool);
        assert_eq!(library_scrape(&s, "lib1"), ScrapeOptions::default());

        let wanted = ScrapeOptions {
            language: Some("en-US".to_string()),
            language_priority: Some(vec!["en-US".to_string()]),
            nfo: true,
            nfo_override: Some(true),
            poster_language: Some("textless".to_string()),
            backdrop_language: None,
            poster_languages: Some(vec!["textless".to_string()]),
            backdrop_languages: None,
            poster_min_width: None,
            backdrop_min_width: None,
            poster_size: None,
            backdrop_size: None,
            logo_languages: None,
            logo_min_width: None,
            logo_size: None,
            write_images: false,
            ..Default::default()
        };
        set_library_scrape(&s, &pool, "lib1", &wanted);

        assert_eq!(library_scrape(&s, "lib1"), wanted);
        assert_eq!(all_library_scrape(&s).get("lib1"), Some(&wanted));
        assert_eq!(library_scrape(&s, "other"), ScrapeOptions::default());
    }

    #[test]
    fn library_def_auto_scan_defaults_true_on_deserialize() {
        let d: LibraryDef =
            serde_json::from_value(json!({"id":"x","name":"N","folders":["/f"]})).unwrap();
        assert!(d.auto_scan);
        assert!(d.kind.is_empty());
    }

    #[test]
    fn deployment_mode_defaults_to_cloud_and_refuses_the_reserved_modes() {
        let pool = test_pool();
        let s = settings(&pool);
        assert_eq!(deployment_mode(&s), DeploymentMode::CloudOnly);

        let mut patch = BTreeMap::new();
        patch.insert("deploymentMode".to_string(), json!("hybrid"));
        s.set_patch(&pool, patch);
        assert_eq!(
            deployment_mode(&s),
            DeploymentMode::CloudOnly,
            "a reserved mode must not enable a build that cannot serve it"
        );
    }

    #[test]
    fn the_cloud_intake_folder_needs_a_leading_slash_and_is_empty_by_default() {
        let pool = test_pool();
        let s = settings(&pool);
        assert_eq!(cloud_intake_folder(&s), "");

        let mut patch = BTreeMap::new();
        patch.insert("cloudIntakeFolder".to_string(), json!(" 待整理/网盘 "));
        s.set_patch(&pool, patch);
        assert_eq!(cloud_intake_folder(&s), "/待整理/网盘");
    }

    #[test]
    fn the_share_poll_interval_is_clamped() {
        let pool = test_pool();
        let s = settings(&pool);
        assert_eq!(share_poll_minutes(&s), 30);

        let mut patch = BTreeMap::new();
        patch.insert("sharePollMinutes".to_string(), json!(1));
        s.set_patch(&pool, patch);
        assert_eq!(share_poll_minutes(&s), 5);

        let mut patch = BTreeMap::new();
        patch.insert("sharePollMinutes".to_string(), json!(99999));
        s.set_patch(&pool, patch);
        assert_eq!(share_poll_minutes(&s), 1440);
    }
}
