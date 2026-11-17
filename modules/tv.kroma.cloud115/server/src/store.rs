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

/// What the module remembers between runs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Stored {
    pub cookie: String,
    pub base_url: String,
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
}

fn path() -> PathBuf {
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
