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
    /// The household language list a library follows until it overrides it.
    pub(super) default_priority: Vec<String>,
    /// Library id -> scan folders, to find a `.strm` whose stored path is the
    /// stream URL rather than the file on disk.
    pub(super) folders: BTreeMap<String, Vec<String>>,
    pub(super) data_dir: PathBuf,
    pub(super) theme_songs: bool,
    pub(super) bus: Bus,
    pub(super) embedder: Point,
}

impl Engine {
    /// The ordered languages for one library: its own list, else the household
    /// default. Never empty once the engine has been built.
    pub(super) fn priority_for(&self, library: &str) -> Vec<String> {
        self.scrape
            .get(library)
            .and_then(|options| options.language_priority.clone())
            .unwrap_or_else(|| self.default_priority.clone())
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

pub(super) fn engine_for(state: &SharedState, api_key: String) -> Engine {
    Engine {
        pool: state.db.clone(),
        cache: state.metadata_cache.clone(),
        api_key,
        language: crate::services::settings::metadata_language(&state.settings, &state.config),
        default_priority: crate::services::settings::scrape_defaults(
            &state.settings,
            &state.config,
        )
        .language_priority
        .unwrap_or_default(),
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
