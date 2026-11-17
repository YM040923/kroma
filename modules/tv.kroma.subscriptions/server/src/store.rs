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
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Item {
    pub key: String,
    pub task_id: String,
    pub title: String,
    pub state: String,
    pub last_error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Stored {
    /// Where releases are searched for. A local service that speaks the
    /// source's own protocol; the module never talks to the site itself.
    pub source_url: String,
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
