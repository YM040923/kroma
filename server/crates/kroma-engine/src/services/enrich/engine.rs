use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::db::Pool;
use crate::infra::events::{Bus, ServerEvent};
use crate::infra::metadata::Cache;
use crate::point::Point;
use crate::services::activity::{self, Shared as Activity};
use crate::state::SharedState;

#[derive(Clone)]
pub(super) struct Engine {
    pub(super) pool: Pool,
    pub(super) cache: Arc<Cache>,
    pub(super) api_key: String,
    pub(super) language: String,
    /// Per-library scrape choices, by library id. A missing entry follows the
    /// global language and leaves NFO/art preferences alone.
    pub(super) scrape: BTreeMap<String, crate::services::settings::ScrapeOptions>,
    /// The household scraping choices a library follows until it overrides them.
    pub(super) defaults: crate::services::settings::ScrapeOptions,
    /// The administrator's Fanart.tv key; empty disables that source.
    pub(super) fanart_key: String,
    /// Library id -> scan folders, to find a `.strm` whose stored path is the
    /// stream URL rather than the file on disk.
    pub(super) folders: BTreeMap<String, Vec<String>>,
    pub(super) data_dir: PathBuf,
    pub(super) theme_songs: bool,
    pub(super) bus: Bus,
    pub(super) embedder: Point,
}

impl Engine {
    /// The effective choices for one library: the household defaults with the
    /// library's own values laid over them. A field the library left unset
    /// keeps following the defaults.
    pub(super) fn options_for(&self, library: &str) -> crate::services::settings::ScrapeOptions {
        let mut merged = self.defaults.clone();
        let Some(held) = self.scrape.get(library) else {
            return merged;
        };
        if held.language_priority.is_some() {
            merged.language_priority = held.language_priority.clone();
            merged.language = held.language.clone();
        }
        if let Some(nfo) = held.nfo_override {
            merged.nfo = nfo;
        }
        if held.poster_languages.is_some() {
            merged.poster_languages = held.poster_languages.clone();
            merged.poster_language = held.poster_language.clone();
        }
        if held.backdrop_languages.is_some() {
            merged.backdrop_languages = held.backdrop_languages.clone();
            merged.backdrop_language = held.backdrop_language.clone();
        }
        if held.poster_min_width.is_some() {
            merged.poster_min_width = held.poster_min_width;
        }
        if held.backdrop_min_width.is_some() {
            merged.backdrop_min_width = held.backdrop_min_width;
        }
        if held.poster_min_votes.is_some() {
            merged.poster_min_votes = held.poster_min_votes;
        }
        if held.backdrop_min_votes.is_some() {
            merged.backdrop_min_votes = held.backdrop_min_votes;
        }
        if held.poster_size.is_some() {
            merged.poster_size = held.poster_size.clone();
        }
        if held.backdrop_size.is_some() {
            merged.backdrop_size = held.backdrop_size.clone();
        }
        if held.logo_languages.is_some() {
            merged.logo_languages = held.logo_languages.clone();
        }
        if held.logo_min_width.is_some() {
            merged.logo_min_width = held.logo_min_width;
        }
        if held.logo_size.is_some() {
            merged.logo_size = held.logo_size.clone();
        }
        if held.season_languages.is_some() {
            merged.season_languages = held.season_languages.clone();
        }
        if held.season_min_width.is_some() {
            merged.season_min_width = held.season_min_width;
        }
        if held.season_size.is_some() {
            merged.season_size = held.season_size.clone();
        }
        if held.poster_sources.is_some() {
            merged.poster_sources = held.poster_sources.clone();
        }
        if held.backdrop_sources.is_some() {
            merged.backdrop_sources = held.backdrop_sources.clone();
        }
        if held.logo_sources.is_some() {
            merged.logo_sources = held.logo_sources.clone();
        }
        if held.season_sources.is_some() {
            merged.season_sources = held.season_sources.clone();
        }
        merged
    }

    /// The ordered text languages for one library. Never empty.
    pub(super) fn priority_for(&self, library: &str) -> Vec<String> {
        self.options_for(library)
            .language_priority
            .unwrap_or_default()
    }
}

#[derive(Default)]
pub(super) struct Counters {
    pub(super) processed: AtomicUsize,
    pub(super) resolved: AtomicUsize,
    pub(super) missed: AtomicUsize,
    pub(super) failed: AtomicUsize,
    pub(super) finished: AtomicUsize,
}

fn default_options(state: &SharedState) -> crate::services::settings::ScrapeOptions {
    let defaults = crate::services::settings::scrape_defaults(&state.settings, &state.config);
    crate::services::settings::ScrapeOptions {
        language: defaults
            .language_priority
            .as_ref()
            .and_then(|list| list.first())
            .cloned(),
        language_priority: defaults.language_priority,
        nfo: defaults.nfo,
        poster_language: defaults
            .poster_languages
            .as_ref()
            .and_then(|list| list.first())
            .cloned(),
        backdrop_language: defaults
            .backdrop_languages
            .as_ref()
            .and_then(|list| list.first())
            .cloned(),
        poster_languages: defaults.poster_languages,
        backdrop_languages: defaults.backdrop_languages,
        poster_min_width: defaults.poster_min_width,
        backdrop_min_width: defaults.backdrop_min_width,
        poster_min_votes: defaults.poster_min_votes,
        backdrop_min_votes: defaults.backdrop_min_votes,
        poster_size: defaults.poster_size,
        backdrop_size: defaults.backdrop_size,
        logo_languages: defaults.logo_languages,
        logo_min_width: defaults.logo_min_width,
        logo_size: defaults.logo_size,
        season_languages: defaults.season_languages,
        season_min_width: defaults.season_min_width,
        season_size: defaults.season_size,
        poster_sources: defaults.poster_sources,
        backdrop_sources: defaults.backdrop_sources,
        logo_sources: defaults.logo_sources,
        season_sources: defaults.season_sources,
        write_images: defaults.write_images,
        ..Default::default()
    }
}

pub(super) fn engine_for(state: &SharedState, api_key: String) -> Engine {
    Engine {
        pool: state.db.clone(),
        cache: state.metadata_cache.clone(),
        api_key,
        language: crate::services::settings::metadata_language(&state.settings, &state.config),
        defaults: default_options(state),
        fanart_key: crate::services::settings::fanart_api_key(&state.settings),
        scrape: crate::services::settings::all_library_scrape(&state.settings),
        folders: crate::services::settings::library_defs(&state.settings, &state.config)
            .unwrap_or_default()
            .into_iter()
            .map(|def| (def.id, def.folders))
            .collect(),
        data_dir: state.config.data_dir.clone(),
        theme_songs: crate::services::settings::theme_songs_enabled(&state.settings),
        bus: state.events.clone(),
        embedder: state.embedder.clone(),
    }
}

pub(super) fn bump(eng: &Engine, counters: &Counters, total: usize, activity: Option<&Activity>) {
    let done = counters.processed.fetch_add(1, Ordering::Relaxed) + 1;
    if let Some(activity) = activity {
        activity::enrich_progress(activity, done);
        if done.is_multiple_of(25) {
            // Read back rather than threading the job through: `bump` runs on
            // every outcome (hit, miss, write failure), so the snapshot is the
            // one place that already knows which title the pass is on.
            let current = activity::snapshot(activity).enrich_current;
            eng.bus.publish(ServerEvent::EnrichProgress {
                done,
                total,
                current,
                failed: counters.failed.load(Ordering::Relaxed),
            });
        }
    }
}
