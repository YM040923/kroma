use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Task {
    pub id: String,
    pub name: String,
    pub source: String,
    pub monitor_path: String,
    pub target_path: String,
    pub failure_path: String,
    pub monitor_mode: String,
    pub enabled: bool,
    pub status: String,
    pub last_run_at: i64,
    pub last_error: String,
    /// The title this task was opened for, so a transfer never has to identify
    /// the film again (the title page already resolved it).
    pub tmdb_id: i64,
    pub kind: String,
    /// The chosen release on the source, as the source names it.
    pub resource: String,
    /// The source's own position for that release, which its unlock takes.
    pub resource_index: i64,
    pub price: i64,
    /// The source's asynchronous job, once a transfer was asked for.
    pub job_id: String,
    /// What the card draws: the title's own art, its year, season and episode
    /// span, and the subscription link the operator picked.
    pub poster: String,
    pub year: i64,
    pub season: i64,
    pub episode_start: i64,
    pub episode_total: i64,
    pub link: String,
    /// The unlock budgets were approved for this title. Set when the operator
    /// accepts the first priced release; later episodes of the same title reuse
    /// it instead of asking again. Zero while nothing was approved.
    pub approved_at: i64,
    /// Every `season:episode` this task already sent, so a release that appears
    /// again (a re-post, or a second candidate for the same episode) never
    /// spends points twice. Newest last; bounded.
    pub sent_episodes: Vec<String>,
    /// Shares this task unlocked and now follows for free. Each `(shareCode,
    /// password)` is polled for new files and its additions are transferred
    /// without paying again.
    pub following_shares: Vec<FollowedShare>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Item {
    pub key: String,
    pub task_id: String,
    pub title: String,
    pub state: String,
    pub last_error: String,
    /// The candidate as the source listed it, so the page can show and send it.
    pub resource: String,
    pub index: i64,
    pub size: String,
    pub link: String,
    pub price: i64,
}

/// One share this task unlocked and now follows. A share keeps receiving
/// episodes after the unlock, so its additions are transferred for free.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct FollowedShare {
    pub share_code: String,
    pub password: String,
    /// What the share was called when it was unlocked, for the task card.
    pub name: String,
    pub unlocked_at: i64,
    pub last_checked_at: i64,
    pub last_error: String,
    /// File ids already transferred from this share.
    pub seen: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct CalendarEntry {
    pub date: String,
    pub title: String,
    pub season: i64,
    pub episode: i64,
    pub episode_name: String,
    pub poster: String,
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Stored {
    /// Where releases are searched for. A local service that speaks the
    /// source's own protocol; the module never talks to the site itself.
    pub source_url: String,
    /// Last discovery pass, so opening the page does not hammer the source.
    pub last_discovery_at: i64,
    /// Last pass that asked the source for *new releases* (the expensive half:
    /// it probes prices). Followed shares are checked on their own, cheaper
    /// cadence, so a five-minute follow never turns into five-minute probing.
    pub last_search_at: i64,
    /// Last pass over followed shares for new files.
    pub last_follow_at: i64,
    /// Upcoming episodes, cached so opening the calendar does not read TMDB
    /// once per task on every visit.
    pub last_calendar_at: i64,
    pub calendar: Vec<CalendarEntry>,
    pub tasks: Vec<Task>,
    pub items: Vec<Item>,
}

fn path() -> PathBuf {
    std::env::var_os("KROMA_DATA_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
        .join("modules").join(crate::MODULE_ID).join("subscriptions.json")
}

fn cache() -> &'static RwLock<Stored> {
    static CACHE: OnceLock<RwLock<Stored>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(std::fs::read(path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()))
}

pub fn load() -> Stored { cache().read().map(|v| v.clone()).unwrap_or_default() }

pub fn save(stored: &Stored) -> Result<(), String> {
    let target = path();
    std::fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
    let staged = target.with_extension("json.tmp");
    std::fs::write(&staged, serde_json::to_vec_pretty(stored).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(staged, &target).map_err(|e| e.to_string())?;
    *cache().write().map_err(|_| "store unavailable".to_string())? = stored.clone();
    Ok(())
}
