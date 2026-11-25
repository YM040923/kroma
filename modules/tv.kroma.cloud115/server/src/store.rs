//! The module's own configuration, kept in its own folder.
//!
//! A module cannot invent settings keys in the core's registry: the core only
//! reaches keys it declared itself. So the account, the mappings and the pacing
//! live in `cloud115.json` under the module's own writable folder
//! (`<data>/modules/tv.kroma.cloud115/`), read once and written atomically. The
//! file holds the cookie, so it never leaves this server and the admin API only
//! ever reports whether one is stored.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};

use serde::{Deserialize, Serialize};

use crate::settings::{Library, Options};

const FILE: &str = "cloud115.json";

/// Where the life-event poller stands between runs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct IncrState {
    /// The newest event id seen and consumed; newer ids are the next round's work.
    pub cursor_id: String,
    pub cursor_time: i64,
    /// Set while the drive's app channel is known to refuse (405): the web
    /// channel is used until then.
    pub web_until: i64,
    /// Consecutive "app refused, web answered" rounds; three in a row trips
    /// `web_until`.
    pub proapi_405: u32,
    /// Whether 115's own life-event recording answered; `None` until checked.
    pub gate_ok: Option<bool>,
    pub gate_checked_at: i64,
    pub last_poll_at: i64,
    pub last_events: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HistoryEntry {
    pub at: i64,
    pub dirs: u64,
    pub files: u64,
    pub written: u64,
    pub pruned: u64,
    pub errors: u32,
    pub truncated: bool,
}

/// One share-link transfer, kept for the task history and the log page.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TransferEntry {
    pub at: i64,
    pub code: String,
    pub title: String,
    pub files: u64,
    pub ok: bool,
    pub message: String,
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// The last profile the drive answered with. Rendering it from here is what
/// makes the account card appear at once: asking 115 costs two paced calls and
/// at least one interval, every time.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AccountSnapshot {
    pub user_name: String,
    pub user_id: String,
    pub vip: bool,
    pub avatar_url: String,
    pub total: u64,
    pub used: u64,
    pub remain: u64,
    pub total_text: String,
    pub used_text: String,
    pub remain_text: String,
    /// Unix seconds when the drive answered.
    pub fetched_at: i64,
}

/// One provider acquisition the module started: what was asked for, what it was
/// quoted, and the source's job id. A bounded log, not a queue: the core owns
/// the request/wanted ledger and the subscriptions module owns its own tasks.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AcquireEntry {
    pub at: i64,
    /// The core's idempotency key: a retry for this attempt must not unlock twice.
    #[serde(default)]
    pub attempt_id: String,
    /// `movie` or `show`.
    pub kind: String,
    pub tmdb_id: i64,
    /// The candidate's file name, as the source reported it.
    pub resource: String,
    pub index: i64,
    /// The price the caller declared, when it gave one.
    pub price: Option<i64>,
    pub link: String,
    /// The source's own job id.
    pub job_id: String,
    /// Last state seen: `started`, or whatever `/job/{id}` answered.
    pub state: String,
    #[serde(default)]
    pub error: String,
}

/// What the module remembers between runs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Stored {
    pub cookie: String,
    pub base_url: String,
    /// The subscription source the provider proxies to, for example
    /// `http://172.17.0.1:8790`.
    pub source_url: String,
    /// The drive folder every acquisition lands in, before the organiser
    /// (Symedia) classifies it, for example `/待整理`. Empty means the drive
    /// root, which is almost never what an operator wants: a readiness check
    /// names it as a blocker instead.
    pub transit_folder: String,
    pub libraries: Vec<Library>,
    pub options: Options,
    /// Every folder the mirror has listed: 115 folder id → its local directory.
    /// The watcher uses it to turn an event's parent id into a path without a
    /// lookup.
    pub dirs: BTreeMap<String, String>,
    pub incr: IncrState,
    /// The last reason a play route could not answer, so the page can say why a
    /// film refused to start instead of leaving it to the module log.
    pub last_play_error: String,
    pub last_play_at: i64,
    /// The last successful play: how it played and which link it handed out.
    pub last_play_mode: String,
    pub last_play_url: String,
    /// Play requests answered, and how many of those had to sign a fresh link.
    pub play_count: u64,
    pub sign_count: u64,
    /// The last few mirror runs, newest last.
    pub history: Vec<HistoryEntry>,
    /// The last share transfers, newest last.
    pub transfers: Vec<TransferEntry>,
    /// The provider acquisitions this module started, newest last. Bounded: it
    /// is the log the provider's `status` route annotates, not a work queue.
    pub acquisitions: Vec<AcquireEntry>,
    /// What the operator calls this account. Display only; 115 never learns it.
    pub alias: String,
    /// When the scheduled mirror last started, so the ten-minute tick can honour
    /// the configured interval without a cron entry per choice.
    pub last_auto_sync_at: i64,
    /// The UTC day the daily call counter belongs to, and its count.
    pub budget_day: i64,
    pub budget_calls: u64,
    /// The last profile and capacity the drive answered with.
    pub account: AccountSnapshot,
}

fn path() -> PathBuf {
    // WASI modules must use the guest path of their own preopened writable
    // directory. `KROMA_DATA_DIR` is a host path (for example `C:\\Users\\...`)
    // on Windows and is not a valid path inside the WASI guest.
    if let Some(module_dir) = std::env::var_os("KROMA_MODULE_DATA_DIR") {
        return PathBuf::from(module_dir).join(FILE);
    }
    // Kept for native development and older runtimes that do not provide the
    // portable module-data mount.
    let root = std::env::var_os("KROMA_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    root.join("modules").join(crate::MODULE_ID).join(FILE)
}

fn cache() -> &'static RwLock<Stored> {
    static CACHE: OnceLock<RwLock<Stored>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(read_disk()))
}

fn read_disk() -> Stored {
    std::fs::read(path())
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Stored>(&bytes).ok())
        // A settings file that no longer parses is a file to replace, not a
        // reason the module cannot start: the admin page edits every field of it.
        .unwrap_or_default()
}

/// The current configuration. Never fails: an unreadable file reads as unset,
/// and the page then asks for the account again.
pub fn load() -> Stored {
    cache()
        .read()
        .map(|stored| stored.clone())
        .unwrap_or_default()
}

/// Change and persist the configuration. The in-memory copy is updated only when
/// the file was written, so a failed save cannot make the page lie about what is
/// stored.
pub fn patch(change: impl FnOnce(&mut Stored)) -> Result<Stored, String> {
    let mut guard = cache()
        .write()
        .map_err(|_| "配置缓存不可用".to_string())?;
    let before = guard.clone();
    change(&mut guard);
    guard.cookie = guard.cookie.trim().to_string();
    guard.base_url = guard.base_url.trim().trim_end_matches('/').to_string();
    guard.source_url = guard.source_url.trim().trim_end_matches('/').to_string();
    guard.options = guard.options.clone().sane();
    if let Err(error) = write_disk(&guard) {
        *guard = before;
        return Err(error);
    }
    Ok(guard.clone())
}

fn write_disk(stored: &Stored) -> Result<(), String> {
    let target = path();
    if let Some(dir) = target.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("建配置目录失败：{e}"))?;
    }
    let body = serde_json::to_vec_pretty(stored).map_err(|e| format!("序列化配置失败：{e}"))?;
    let staged = target.with_extension("json.tmp");
    std::fs::write(&staged, body).map_err(|e| format!("写配置失败：{e}"))?;
    std::fs::rename(&staged, &target).map_err(|e| format!("替换配置失败：{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unset_account_reads_as_defaults() {
        let stored: Stored = serde_json::from_str("{}").expect("defaults");
        assert!(stored.cookie.is_empty());
        assert!(stored.libraries.is_empty());
        assert_eq!(stored.options, Options::default());
    }

    #[test]
    fn a_partial_file_keeps_the_defaults_for_the_fields_it_omits() {
        let stored: Stored =
            serde_json::from_str(r#"{"baseUrl":"http://nas:4040","options":{"intervalMs":2000}}"#)
                .expect("a partial file parses");
        assert_eq!(stored.base_url, "http://nas:4040");
        assert_eq!(stored.options.interval_ms, 2000);
        assert_eq!(stored.options.max_calls_per_run, Options::default().max_calls_per_run);
    }
}
