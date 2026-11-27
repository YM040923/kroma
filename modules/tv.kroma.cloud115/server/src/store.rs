//! The module's own configuration, kept in its own folder.
//!
//! A module cannot invent settings keys in the core's registry: the core only
//! reaches keys it declared itself. So the account, the mappings and the pacing
//! live in `cloud115.json` under the module's own writable folder
//! (`<data>/modules/tv.kroma.cloud115/`). Both guest instances read the same
//! atomic snapshots. Open authorization is stored separately, never exported.

use std::collections::BTreeMap;
use std::io::{self, ErrorKind};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::settings::{Library, Options};

const FILE: &str = "cloud115.json";

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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mappings: Vec<crate::sync::Mapping>,
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

/// What the module remembers between runs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Stored {
    // Older releases stored a session cookie here. Serde ignores the retired
    // field; it is never loaded as authorization or emitted on the next save.
    pub account_grant: String,
    pub base_url: String,
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
    /// The last reason a play route could not answer, so the page can say why a
    /// film refused to start instead of leaving it to the module log.
    pub last_play_error: String,
    pub last_play_at: i64,
    /// The last successful play: how it played and which link it handed out.
    pub last_play_mode: String,
    /// Play requests answered, and how many of those had to sign a fresh link.
    pub play_count: u64,
    pub sign_count: u64,
    /// The last few mirror runs, newest last.
    pub history: Vec<HistoryEntry>,
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

pub(crate) fn path() -> PathBuf {
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

/// The host recreates this module's shared /tmp on each module start. Locks
/// therefore cover both instances, but a stopped or trapped generation cannot
/// block authorization and settings writes forever after a restart.
#[cfg(target_family = "wasm")]
pub(crate) fn lock_path(name: &str) -> PathBuf {
    // KROMA preopens this private guest directory. std::env::temp_dir()
    // panics on wasm32-wasip2 even when TMPDIR is present in the environment.
    PathBuf::from("/tmp").join(name)
}
#[cfg(not(target_family = "wasm"))]
pub(crate) fn lock_path(name: &str) -> PathBuf {
    path().with_file_name(name)
}

pub(crate) struct FileLock(PathBuf);
impl FileLock {
    pub(crate) fn acquire(name: &str, timeout: Duration) -> io::Result<Self> {
        let path = lock_path(name);
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let start = Instant::now();
        loop {
            match std::fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(e) if e.kind() == ErrorKind::AlreadyExists && start.elapsed() < timeout => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(e) => return Err(e),
            }
        }
    }
}
impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir(&self.0);
    }
}

fn read_disk() -> io::Result<Stored> {
    match std::fs::read(path()) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Stored::default()),
        Err(e) => Err(e),
    }
}

pub fn load() -> Stored {
    read_disk().unwrap_or_default()
}

pub fn patch(change: impl FnOnce(&mut Stored)) -> Result<Stored, String> {
    let _guard = FileLock::acquire("cloud115-settings.lock", Duration::from_secs(2))
        .map_err(|_| "配置正忙，请重试".to_owned())?;
    // Refuse to overwrite malformed configuration or another guest's newer mappings.
    let mut s = read_disk().map_err(|_| "配置读取失败，未更改原文件".to_owned())?;
    change(&mut s);
    s.base_url = s.base_url.trim().trim_end_matches('/').to_owned();
    s.options = s.options.sane();
    write_disk(&s)?;
    Ok(s)
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
        assert!(stored.account_grant.is_empty());
        let old: Stored =
            serde_json::from_str(r#"{"cookie":"legacy-placeholder","baseUrl":"http://keep"}"#)
                .unwrap();
        let safe = serde_json::to_value(old).unwrap();
        assert!(safe.get("cookie").is_none());
        assert_eq!(safe["baseUrl"], "http://keep");
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
        assert_eq!(
            stored.options.max_calls_per_run,
            Options::default().max_calls_per_run
        );
    }
}
