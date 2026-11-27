//! Mirroring a drive folder into the library as `.strm` entries.
//!
//! A run walks the drive's folders, writes one `.strm` per video naming this
//! module's play route, and writes the sidecar the scanner reads beside it (this
//! server's ffprobe cannot open a URL, so the metadata has to come from the
//! bridge side). It then removes what the drive no longer has — but only after a
//! run that read every folder it meant to, because a listing that failed is not
//! an empty folder, and treating it as one is how a mirror deletes a library.
//!
//! Runs are started, not awaited: a library takes longer than any request should,
//! and the admin page reads progress from [`status`].

use crate::store::HistoryEntry;
use std::collections::{HashMap, HashSet};
use std::io::{self, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;

use kroma_module_sdk::host::{HostCtx, LibraryFolders};
use kroma_module_sdk::http::Fetch;

use crate::client::{self, ApiError};
use crate::pacer::pacer;
use crate::settings::{Config, Library};
use crate::MODULE_ID;

/// How deep a mirror goes. A media tree deeper than this is a tree to split.
const MAX_DEPTH: usize = 12;
/// How many entries one folder may hold before a run refuses to descend into it.
/// The limit itself is a setting; this is the hard ceiling a bad folder cannot
/// raise.
const HARD_ENTRY_CEILING: usize = 50_000;
/// Folder entries read in one run, across the whole tree, regardless of the API
/// budget. Bounds a pathological tree.
const MAX_TREE_ENTRIES: usize = 500_000;
/// Files described per automatic run. One probe is one signed link plus one or
/// two range reads; a fresh library is described over several runs rather than
/// making the first one last an hour. A run the operator started by hand gets a
/// larger allowance, because they are waiting for it.
const MAX_PROBES_PER_RUN: u32 = 200;
const MANUAL_PROBES_PER_RUN: u32 = 1000;

/// Subtitle/NFO/poster files downloaded per run. Each one is a signed link plus
/// a transfer through the host, so the count is what costs.
const MAX_ASSETS_PER_RUN: u32 = 200;

/// Companion files that live beside a video and belong with its `.strm`.
const ASSET_EXTS: [&str; 13] = [
    "srt", "ass", "ssa", "sup", "vtt", "idx", "sub", "nfo", "jpg", "jpeg", "png", "webp", "tbn",
];

pub(crate) fn asset_allowed(config: &Config, ext: &str) -> bool {
    match ext {
        "jpg" | "jpeg" | "png" | "webp" | "tbn" => config.options.sync_images,
        "nfo" => config.options.sync_nfo,
        _ => config.options.sync_subtitles,
    }
}

/// A companion larger than this is not a subtitle or a poster.
const MAX_ASSET_BYTES: i64 = 200 * 1024 * 1024;

/// What the module calls a play route: the only `.strm` files a prune may
/// remove are ones naming this module.
fn play_marker() -> String {
    format!("/api/module/{MODULE_ID}/play?pc=")
}

#[derive(Debug, Clone, Default, Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum State {
    #[default]
    Queued,
    Discovering,
    Processing,
    Completed,
    Partial,
    Failed,
    Paused,
    Interrupted,
}

#[derive(Debug, Clone, Default, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mapping {
    remote: String,
    local_display: String,
    state: State,
    discovered_dirs: u64,
    total_dirs: Option<u64>,
    done_dirs: u64,
    files: u64,
    written: u64,
    error_count: u64,
    errors: Vec<MappingError>,
}
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
struct MappingError {
    path: String,
    stage: String,
    message: String,
}

#[derive(Debug, Clone, Default, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Report {
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    /// The folder being read right now, for the live panel.
    pub current: String,
    pub elapsed_ms: u64,
    pub dirs: u64,
    pub files: u64,
    pub written: u64,
    pub kept: u64,
    pub pruned: u64,
    pub probed: u64,
    /// Entries still without a description when the run ended; the next run
    /// continues where this one stopped.
    pub pending_probe: u64,
    /// Subtitle/NFO/poster files written locally this run.
    pub assets: u64,
    pub calls: u32,
    /// The per-run call ceiling this run is spending against; 0 when the run
    /// is not running.
    pub calls_total: u32,
    pub truncated: bool,
    pub errors: Vec<String>,
    pub mappings: Vec<Mapping>,
    #[serde(skip)]
    enrichment: Vec<Extra>,
    #[serde(skip)]
    removals: Vec<(usize, PathBuf, Baseline)>,
    #[serde(skip)]
    listed: HashSet<PathBuf>,
    #[serde(skip)]
    baselines: HashMap<PathBuf, Baseline>,
    #[serde(skip)]
    listing_start: Option<std::time::SystemTime>,
    #[serde(skip)]
    active: usize,
    #[serde(skip)]
    active_root: PathBuf,
    #[serde(skip)]
    last_publish: Option<std::time::Instant>,
    #[serde(skip)]
    checkpoint: Option<PathBuf>,
    #[cfg(test)]
    #[serde(skip)]
    observed: Vec<(State, Option<u64>, u64)>,
}

fn guard_path() -> PathBuf {
    #[cfg(target_family = "wasm")]
    let tmp = PathBuf::from("/tmp");
    #[cfg(not(target_family = "wasm"))]
    let tmp = std::env::temp_dir();
    tmp.join("cloud115-sync.run")
}
struct RunGuard(PathBuf);
impl RunGuard {
    fn acquire(path: PathBuf) -> Result<Self, String> {
        std::fs::create_dir(&path).map_err(|e| {
            if e.kind() == io::ErrorKind::AlreadyExists {
                "同步已经在运行".to_owned()
            } else {
                format!("创建同步锁失败：{e}")
            }
        })?;
        Ok(Self(path))
    }
}
impl Drop for RunGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
pub fn is_running() -> bool {
    guard_path().is_dir()
}
fn latest() -> PathBuf {
    crate::store::path().with_file_name("sync-report.json")
}
fn recent() -> PathBuf {
    latest().with_file_name("sync-history.json")
}
fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    serde_json::from_reader(BufReader::new(std::fs::File::open(path).ok()?)).ok()
}
fn save_json(path: &Path, value: &impl Serialize) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let mut writer = BufWriter::new(std::fs::File::create(&tmp)?);
    serde_json::to_writer(&mut writer, value)?;
    writer.flush()?;
    drop(writer);
    std::fs::rename(tmp, path)
}
fn publish(report: &mut Report, force: bool) {
    #[cfg(test)]
    if let Some(row) = report.mappings.get(report.active) {
        report
            .observed
            .push((row.state.clone(), row.total_dirs, row.done_dirs));
    }
    if force
        || report
            .last_publish
            .is_none_or(|t| t.elapsed().as_secs() >= 1)
    {
        let target = report.checkpoint.clone().unwrap_or_else(latest);
        if let Err(e) = save_json(&target, report) {
            tracing::warn!(%e,"sync checkpoint failed");
        }
        report.last_publish = Some(std::time::Instant::now());
    }
}
fn read_status(
    latest: &Path,
    recent: &Path,
    running: bool,
    legacy: impl FnOnce() -> Vec<HistoryEntry>,
) -> (bool, Report, Vec<HistoryEntry>) {
    let history: Vec<HistoryEntry> = read_json(recent).unwrap_or_else(legacy);
    let mut report: Report = read_json(latest).unwrap_or_else(|| {
        history
            .last()
            .map(|h| Report {
                finished_at: Some(h.at),
                dirs: h.dirs,
                files: h.files,
                written: h.written,
                pruned: h.pruned,
                truncated: h.truncated,
                mappings: h.mappings.clone(),
                ..Report::default()
            })
            .unwrap_or_default()
    });
    if !running && report.finished_at.is_none() {
        for m in &mut report.mappings {
            if matches!(
                m.state,
                State::Queued | State::Discovering | State::Processing
            ) {
                m.state = State::Interrupted;
                report.truncated = true;
            }
        }
    }
    (running, report, history)
}
pub fn status() -> (bool, Report, Vec<HistoryEntry>) {
    read_status(&latest(), &recent(), is_running(), || {
        crate::store::load().history
    })
}

/// Start a run against the settings as they are now. Returns immediately; the
/// page follows it through [`status`].
///
/// `base_override` is the origin the admin's browser reached this server at: it
/// is what the `.strm` URLs are built from, and the browser is the only side
/// that knows it for certain.
pub fn start<S: HostCtx + Clone + Send + Sync + 'static>(
    host: S,
    base_override: Option<String>,
    manual: bool,
) -> Result<(), String> {
    let guard = RunGuard::acquire(guard_path())?;
    if let Some(base) = base_override.map(|b| b.trim().trim_end_matches('/').to_string()) {
        if !base.starts_with("http://") && !base.starts_with("https://") {
            return Err("服务器地址必须形如 http://192.168.1.10:8096".into());
        }
        crate::store::patch(|stored| stored.base_url = base)?;
    }
    let config = Config::load();
    if config.auth_id.is_empty() {
        return Err("请先授权 115 账号".into());
    }
    if config.base_url.is_empty() {
        return Err("先填写服务器地址（播放链接要用它拼接）".into());
    }
    if !config.runnable() {
        return Err("先配置至少一个目录映射（网盘目录 → 本地子目录）".into());
    }
    let folders = host.library_folders();
    let mut config = config;
    if manual {
        config.options.max_calls_per_run = config.options.max_calls_per_run.max(5000);
    }
    let probe_cap = if manual {
        MANUAL_PROBES_PER_RUN
    } else {
        MAX_PROBES_PER_RUN
    };

    // Preserve the last generation's unfinished checkpoint before replacing it.
    let (_, previous, _) =
        read_status(&latest(), &recent(), false, || crate::store::load().history);
    if previous.started_at.is_some() && previous.finished_at.is_none() {
        append_history(&recent(), &previous, || crate::store::load().history)
            .map_err(|e| format!("保存中断同步记录失败：{e}"))?;
    }
    let report = Report::queued(&config);
    save_json(&latest(), &report).map_err(|e| format!("保存同步进度失败：{e}"))?;
    tokio::spawn(async move {
        pacer()
            .with_run(
                &config.options,
                run(&host, &config, &folders, probe_cap, report),
            )
            .await;
        drop(guard);
    });
    Ok(())
}

enum Failure {
    /// The per-run call budget ran out. Not an error: the rest waits for the next
    /// run, and nothing may be pruned on the way out.
    Budget,
    Recorded,
    Api(ApiError),
    /// A folder could not be read completely. Nothing may be pruned.
    Incomplete(String),
}

fn map_api(error: ApiError) -> Failure {
    // The pacer's budget message is the one refusal that is not a drive answer.
    if error.http == 0 && error.message.contains("调用上限") {
        Failure::Budget
    } else {
        Failure::Api(error)
    }
}

impl Report {
    fn queued(config: &Config) -> Self {
        Self {
            started_at: Some(unix_now()),
            mappings: config
                .libraries
                .iter()
                .filter(|l| !l.remote.trim().is_empty() && !l.local.trim().is_empty())
                .map(|l| Mapping {
                    remote: l.remote.clone(),
                    local_display: if l.local_display.is_empty() {
                        l.local.clone()
                    } else {
                        l.local_display.clone()
                    },
                    ..Mapping::default()
                })
                .collect(),
            ..Self::default()
        }
    }
    fn issue(&mut self, path: &Path, stage: &str, reason: impl Into<String>) {
        let message = reason.into();
        let row = &mut self.mappings[self.active];
        let path = if !self.active_root.as_os_str().is_empty() {
            path.strip_prefix(&self.active_root).ok().map(|suffix| {
                let base = if stage == "list" {
                    &row.remote
                } else {
                    &row.local_display
                };
                if suffix.as_os_str().is_empty() {
                    return base.clone();
                }
                let separator = if base.contains('\\') { '\\' } else { '/' };
                let suffix = suffix
                    .to_string_lossy()
                    .replace(['/', '\\'], &separator.to_string());
                format!("{}{separator}{suffix}", base.trim_end_matches(['/', '\\']))
            })
        } else {
            None
        }
        .unwrap_or_else(|| path.display().to_string());
        row.error_count += 1;
        if self.errors.len() < 100 {
            self.errors.push(format!("{path}: {message}"));
        }
        if row.errors.len() < 32 {
            row.errors.push(MappingError {
                path,
                stage: stage.into(),
                message,
            });
        }
    }
    fn changed(&self) -> bool {
        self.written > 0 || self.pruned > 0 || self.probed > 0
    }
}

#[derive(Debug, Clone)]
struct Extra {
    row: usize,
    dir: PathBuf,
    name: String,
    pc: String,
    size: i64,
    probe: bool,
}

// Only identifiers, never credentials. Include the complete mapping list: changing its
// order or sharing a selected directory changes the meaning of a mapping index.
fn identity(config: &Config) -> serde_json::Value {
    serde_json::json!([
        config.auth_id,
        config.base_url,
        config.libraries,
        config.options.max_entries_per_folder,
        config.options.page_size,
        config.options.keep_suffix,
        config.options.skip_iso,
        config.options.min_video_size_mb,
        config.options.prefetch_next,
        config.options.overwrite_strm,
        config.options.probe_enabled,
        config.options.sync_images,
        config.options.sync_subtitles,
        config.options.sync_nfo
    ])
}

async fn enrich(config: &Config, report: &mut Report) {
    for job in std::mem::take(&mut report.enrichment) {
        if pacer().spent().await >= report.calls_total
            || crate::budget::background_blocked(config.options.daily_call_budget)
        {
            break;
        }
        report.active = job.row;
        report.active_root = config
            .libraries
            .iter()
            .filter(|l| !l.remote.trim().is_empty() && !l.local.trim().is_empty())
            .nth(job.row)
            .map(|l| PathBuf::from(&l.local))
            .unwrap_or_default();
        if !safe_descendant(&report.active_root, &job.dir) {
            report.issue(&job.dir, "metadata", "附属文件目录不安全");
            continue;
        }
        let path = job.dir.join(safe_name(&job.name));
        report.current = path.display().to_string();
        let result = if job.probe {
            match crate::probe::probe(&config.auth_id, &job.pc, job.size).await {
                Ok(Some(media)) => {
                    let strm = job.dir.join(format!("{}.strm", job.name));
                    let Ok(_guard) = EntryLock::take(&strm) else {
                        continue;
                    };
                    let expected = format!(
                        "{}{}{}",
                        config.base_url,
                        play_marker(),
                        job.pc.trim().to_ascii_lowercase()
                    );
                    if !safe_descendant(&report.active_root, &job.dir)
                        || !std::fs::read_to_string(&strm).is_ok_and(|url| {
                            url.trim().split('&').next() == Some(expected.as_str())
                        })
                        || std::fs::symlink_metadata(strm.with_extension("info.json"))
                            .is_ok_and(|m| indirect(&m))
                    {
                        continue;
                    }
                    write_sidecar(&job.dir, &job.name, job.size, &media).map(|()| {
                        report.probed += 1;
                        report.pending_probe -= 1;
                    })
                }
                Ok(None) => Ok(()),
                Err(e) => Err(e.sync_reason().to_string()),
            }
        } else {
            download_asset(config, &job.pc, &job.name, job.size, &job.dir)
                .await
                .map(|w| report.assets += u64::from(w))
        };
        if let Err(reason) = result {
            report.issue(&path, if job.probe { "metadata" } else { "asset" }, reason);
            if report.mappings[job.row].state == State::Completed {
                report.mappings[job.row].state = State::Partial;
            }
        }
        report.calls = pacer().spent().await;
        publish(report, false);
    }
}

async fn run<S: HostCtx>(
    host: &S,
    config: &Config,
    folders: &[LibraryFolders],
    probe_cap: u32,
    mut report: Report,
) -> Report {
    let started = tokio::time::Instant::now();
    report.calls_total = pacer().budget().await;
    let mut dirs = std::collections::BTreeMap::new();
    let mut probe_left = probe_cap;
    let mut assets_left = MAX_ASSETS_PER_RUN;

    report.listing_start = Some(std::time::SystemTime::now());
    let work = latest().with_file_name("sync-work");
    let rotation = work.join("next.json");
    let key = identity(config);
    let rows: Vec<_> = config
        .libraries
        .iter()
        .filter(|l| !l.remote.trim().is_empty() && !l.local.trim().is_empty())
        .collect();
    let start = read_json::<(serde_json::Value, usize)>(&rotation)
        .filter(|(id, _)| id == &key)
        .map_or(0, |(_, n)| n % rows.len().max(1));
    for library in &rows {
        if let Some(root) = resolve_local_path(folders, &library.local) {
            if let Ok(files) = baseline(&root, report.listing_start) {
                report.baselines.insert(root, files);
            }
        }
    }
    let mut next = None;
    for n in 0..rows.len() {
        let i = (start + n) % rows.len();
        let library = rows[i];
        report.active = i;
        report.active_root = PathBuf::from(&library.local);
        report.current = library.remote.clone();
        let result = if report.truncated
            || crate::budget::background_blocked(config.options.daily_call_budget)
        {
            Err(Failure::Budget)
        } else if let Some(local) = resolve_local_path(folders, &library.local) {
            report.mappings[i].state = State::Discovering;
            publish(&mut report, true);
            sync_library(
                config,
                library,
                &local,
                &mut dirs,
                &mut probe_left,
                &mut assets_left,
                &mut report,
                &work,
            )
            .await
        } else {
            report.issue(Path::new(&library.local), "mapping", "本地目录未授权");
            Err(Failure::Recorded)
        };
        if next.is_none() && matches!(result, Err(Failure::Budget)) {
            next = Some(i);
        }
        finish_mapping(&mut report, result);
        publish(&mut report, true);
    }
    if let Err(e) = save_json(&rotation, &(&key, next.unwrap_or(0))) {
        tracing::warn!(%e, "mapping cursor write failed");
    }
    if !report.truncated {
        for i in 0..rows.len() {
            let cursor = work.join(format!("cursor-{i}.json"));
            if std::fs::remove_file(cursor).is_ok() {
                let _ = std::fs::remove_file(work.join(format!("directories-{i}.jsons")));
            }
        }
    }
    let ownership = work.join("ownership.json");
    if report.mappings.iter().all(|m| m.state == State::Completed)
        && identity(&Config::load()) == key
    {
        if let Some((_, previously_listed)) =
            read_json::<(serde_json::Value, HashSet<PathBuf>)>(&ownership)
                .filter(|(id, _)| id == &key)
        {
            for (i, root, mut files) in std::mem::take(&mut report.removals) {
                if rows.iter().filter(|l| Path::new(&l.local) == root).count() != 1
                    || identity(&Config::load()) != key
                {
                    continue;
                }
                report.active = i;
                report.active_root = root.clone();
                files.retain(|p, _| previously_listed.contains(p));
                prune_checked(&root, &files, &mut report, || {
                    identity(&Config::load()) == key
                });
                if report.mappings[i].error_count != 0 {
                    report.mappings[i].state = State::Partial;
                }
            }
        }
        let _ = save_json(&ownership, &(&key, &report.listed));
    }
    if !report.truncated && identity(&Config::load()) == key {
        enrich(config, &mut report).await;
    }
    report.calls = pacer().spent().await;
    crate::budget::flush();
    report.elapsed_ms = started.elapsed().as_millis() as u64;
    report.finished_at = Some(unix_now());
    report.current.clear();
    publish(&mut report, true);
    if let Err(e) = append_history(&recent(), &report, || crate::store::load().history) {
        tracing::warn!(%e,"sync history write failed");
    }
    if report.mappings.iter().all(|m| m.state == State::Completed)
        && !dirs.is_empty()
        && identity(&Config::load()) == key
    {
        let _ = crate::store::patch(|stored| stored.dirs = dirs);
    }
    if !report.errors.is_empty() {
        tracing::warn!(first=?report.errors.first(),"cloud115 sync finished with errors");
    }
    if report.changed() {
        host.trigger_job("library.scan", "cloud115 sync");
    }
    report
}

fn finish_mapping(report: &mut Report, result: Result<(), Failure>) {
    let i = report.active;
    let path = PathBuf::from(&report.current);
    let state = match result {
        Ok(()) if report.mappings[i].error_count == 0 => State::Completed,
        Ok(()) => State::Partial,
        Err(Failure::Budget) => {
            report.issue(&path, "budget", "同步额度已用尽");
            report.truncated = true;
            State::Paused
        }
        Err(err) => {
            match err {
                Failure::Api(e) => report.issue(&path, "list", e.sync_reason()),
                Failure::Incomplete(why) => report.issue(&path, "list", why),
                _ => (),
            }
            if report.mappings[i].discovered_dirs > 0 {
                State::Partial
            } else {
                State::Failed
            }
        }
    };
    report.mappings[i].state = state;
}

fn append_history(
    path: &Path,
    report: &Report,
    legacy: impl FnOnce() -> Vec<HistoryEntry>,
) -> io::Result<()> {
    let summary = crate::store::HistoryEntry {
        at: unix_now(),
        dirs: report.dirs,
        files: report.files,
        written: report.written,
        pruned: report.pruned,
        errors: report
            .mappings
            .iter()
            .map(|m| m.error_count)
            .sum::<u64>()
            .min(u32::MAX as u64) as u32,
        truncated: report.truncated,
        mappings: report.mappings.clone(),
    };
    let mut history: Vec<HistoryEntry> = read_json(path).unwrap_or_else(legacy);
    history.push(summary);
    history.drain(..history.len().saturating_sub(20));
    save_json(path, &history)
}

/// The guest path of a host folder the operator explicitly selected and granted.
pub(crate) fn resolve_local_path(_folders: &[LibraryFolders], local: &str) -> Option<PathBuf> {
    let trimmed = local.trim();
    let id = trimmed.strip_prefix("/selected/")?;
    (!id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')))
    .then(|| PathBuf::from(trimmed))
}

#[derive(Serialize, serde::Deserialize)]
struct Cursor {
    key: serde_json::Value,
    root: String,
    stack: Vec<(String, PathBuf, usize)>,
    visited: HashSet<String>,
    partial: Vec<client::Entry>,
    total: i64,
    remaining: usize,
    discovered: u64,
    incomplete: bool,
    length: u64,
}
fn cursor_key(config: &Config, root: &Path, row: usize) -> serde_json::Value {
    serde_json::json!([identity(config), root, row])
}
fn saved_cursor(scratch: &Path, row: usize, key: &serde_json::Value) -> Option<Cursor> {
    read_json::<Cursor>(&scratch.join(format!("cursor-{row}.json"))).filter(|c| &c.key == key)
}
fn indirect(m: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if m.file_attributes() & 0x400 != 0 {
            return true;
        } // junction or other reparse point
    }
    m.is_symlink()
}
pub(crate) fn safe_descendant(root: &Path, dir: &Path) -> bool {
    dir.strip_prefix(root).is_ok_and(|p| p.components().all(|c| matches!(c, std::path::Component::Normal(s) if s.to_str().is_some_and(|n| safe_name(n) == n))))
        && dir.ancestors().take_while(|p| p.starts_with(root)).all(|p| match std::fs::symlink_metadata(p) { Ok(m) => !indirect(&m), Err(e) => e.kind() == io::ErrorKind::NotFound })
}

// Resolving the mapping itself is paginated with the open API. A deep mapping
// must not spend its entire allowance resolving the same ancestor every run.
#[derive(Serialize, serde::Deserialize)]
struct Lookup {
    key: serde_json::Value,
    cid: String,
    level: usize,
    offset: u32,
    total: Option<i64>,
    found: Option<String>,
}
async fn resolve_root(
    config: &Config,
    remote: &str,
    key: &serde_json::Value,
    file: &Path,
) -> Result<String, Failure> {
    let parts: Vec<_> = remote.split('/').filter(|n| !n.is_empty()).collect();
    let mut p = read_json::<Lookup>(file)
        .filter(|p| &p.key == key && p.level < parts.len())
        .unwrap_or_else(|| Lookup {
            key: key.clone(),
            cid: "0".into(),
            level: 0,
            offset: 0,
            total: None,
            found: None,
        });
    while p.level < parts.len() {
        let page = if crate::budget::background_blocked(config.options.daily_call_budget) {
            Err(Failure::Budget)
        } else {
            client::list(&config.auth_id, client::MODULE_UA, &p.cid, p.offset, 1150)
                .await
                .map_err(map_api)
        };
        let page = match page {
            Ok(v) => v,
            Err(Failure::Budget) => {
                save_json(file, &p).map_err(|e| Failure::Incomplete(e.to_string()))?;
                return Err(Failure::Budget);
            }
            Err(e) => {
                let _ = std::fs::remove_file(file);
                return Err(e);
            }
        };
        if file.exists() {
            std::fs::remove_file(file).map_err(|e| Failure::Incomplete(e.to_string()))?;
        }
        if p.total.is_some_and(|v| v != page.total) || page.total > MAX_TREE_ENTRIES as i64 {
            return Err(Failure::Incomplete("授权目录分页列表不完整".into()));
        }
        p.total = Some(page.total);
        p.offset += page.entries.len() as u32;
        for entry in page.entries {
            if entry.is_dir() && entry.name == parts[p.level] {
                if p.found.is_some() {
                    return Err(Failure::Incomplete("授权目录重名".into()));
                }
                p.found = entry.folder_id();
            }
        }
        if i64::from(p.offset) == page.total {
            p.cid = p
                .found
                .take()
                .ok_or_else(|| Failure::Incomplete("找不到授权目录".into()))?;
            p.level += 1;
            p.offset = 0;
            p.total = None;
        }
    }
    Ok(p.cid)
}

async fn sync_library(
    config: &Config,
    library: &Library,
    local_root: &Path,
    dirs: &mut std::collections::BTreeMap<String, String>,
    probe_left: &mut u32,
    assets_left: &mut u32,
    report: &mut Report,
    scratch: &Path,
) -> Result<(), Failure> {
    let key = cursor_key(config, local_root, report.active);
    let lookup = scratch.join(format!("lookup-{}.json", report.active));
    if lookup.exists() {
        report.baselines.remove(local_root);
    }
    let root_cid = match saved_cursor(scratch, report.active, &key) {
        Some(c) => c.root,
        None => resolve_root(config, &library.remote, &key, &lookup).await?,
    };

    sync_tree(
        config,
        root_cid,
        local_root,
        dirs,
        probe_left,
        assets_left,
        report,
        scratch,
        |cid, offset, size| async move {
            client::list(&config.auth_id, client::MODULE_UA, &cid, offset, size).await
        },
    )
    .await
}

async fn sync_tree<F, Fut>(
    config: &Config,
    root_cid: String,
    local_root: &Path,
    dirs: &mut std::collections::BTreeMap<String, String>,
    probe_left: &mut u32,
    assets_left: &mut u32,
    report: &mut Report,
    scratch: &Path,
    mut list: F,
) -> Result<(), Failure>
where
    F: FnMut(String, u32, u32) -> Fut,
    Fut: std::future::Future<Output = Result<client::Page, ApiError>>,
{
    let i = report.active;
    let spool_path = scratch.join(format!("directories-{i}.jsons"));
    let cursor_path = scratch.join(format!("cursor-{i}.json"));
    let spool_err = |_: io::Error| Failure::Incomplete("本地同步暂存文件不可用".into());
    std::fs::create_dir_all(scratch).map_err(spool_err)?;
    let key = cursor_key(config, local_root, i);
    let previous = saved_cursor(scratch, i, &key).filter(|c| c.root == root_cid);
    let resumed = previous.is_some();
    if !resumed && cursor_path.exists() {
        std::fs::remove_file(&cursor_path).map_err(spool_err)?;
    }
    let mut c = previous.unwrap_or_else(|| Cursor {
        key,
        root: root_cid.clone(),
        stack: vec![(root_cid, local_root.to_owned(), 0)],
        visited: Default::default(),
        partial: vec![],
        total: 0,
        remaining: MAX_TREE_ENTRIES,
        discovered: 1,
        incomplete: false,
        length: 0,
    });
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .truncate(false)
        .create(true)
        .open(&spool_path)
        .map_err(spool_err)?;
    if file.metadata().map_err(spool_err)?.len() < c.length {
        return Err(spool_err(io::ErrorKind::UnexpectedEof.into()));
    }
    file.set_len(c.length).map_err(spool_err)?;
    std::io::Seek::seek(&mut file, std::io::SeekFrom::Start(c.length)).map_err(spool_err)?;
    let mut spool = BufWriter::new(file);
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let baseline = if resumed {
        None
    } else if report.listing_start.is_some() {
        report.baselines.remove(local_root)
    } else {
        baseline(local_root, None).ok()
    };
    let mut budget_left = c.remaining;
    let mut stack = std::mem::take(&mut c.stack);
    let mut visited = std::mem::take(&mut c.visited);
    report.mappings[i].discovered_dirs = c.discovered;
    if c.incomplete {
        report.issue(local_root, "list", "上轮已有目录不完整，禁止清理");
    }
    let discovery = async {
        'discover: while let Some((cid, dir, depth)) = stack.pop() {
            if !safe_descendant(local_root, &dir) {
                return Err(Failure::Incomplete("目录不安全".into()));
            }
            if !visited.insert(cid.clone()) || depth > MAX_DEPTH {
                report.issue(&dir, "list", "目录重复或超过层数上限，已跳过");
                continue;
            }
            // The watcher turns an event's parent id into a path with this map.
            dirs.insert(cid.clone(), dir.to_string_lossy().into_owned());
            report.current = dir.to_string_lossy().into_owned();
            report.calls = pacer().spent().await;
            publish(report, false);
            let ceiling = (config.options.max_entries_per_folder as usize).min(HARD_ENTRY_CEILING);
            let page_size = config.options.page_size;
            let mut entries = std::mem::take(&mut c.partial);
            let mut offset = entries.len() as u32;
            let mut total = std::mem::take(&mut c.total);
            loop {
                let answer = if crate::budget::background_blocked(config.options.daily_call_budget)
                {
                    Err(Failure::Budget)
                } else {
                    list(cid.clone(), offset, page_size).await.map_err(map_api)
                };
                let page = match answer {
                    Ok(page) => page,
                    Err(Failure::Budget) => {
                        visited.remove(&cid);
                        stack.push((cid, dir, depth));
                        c.partial = entries;
                        c.total = total;
                        return Err(Failure::Budget);
                    }
                    Err(err) => return Err(err),
                };
                report.calls = pacer().spent().await;
                publish(report, false);
                if page.total < 0 || (offset > 0 && page.total != total) {
                    return Err(Failure::Incomplete(
                        "分页时目录项总数改变，无法确认完整列表".into(),
                    ));
                }
                // The open listing can include the current ancestry. Refuse a
                // cached folder ID that has moved outside its selected mapping.
                if !page.path.is_empty() && report.listing_start.is_some() {
                    let remote = report.mappings[i]
                        .remote
                        .split('/')
                        .filter(|n| !n.is_empty());
                    let suffix = dir
                        .strip_prefix(local_root)
                        .map_err(|_| Failure::Incomplete("授权目录不匹配".into()))?;
                    let expected: Vec<_> = remote
                        .map(safe_name)
                        .chain(suffix.iter().map(|n| n.to_string_lossy().into_owned()))
                        .collect();
                    if !page
                        .path
                        .iter()
                        .skip(1)
                        .map(|(_, n)| safe_name(n))
                        .eq(expected)
                    {
                        return Err(Failure::Incomplete("网盘目录已移动，禁止清理".into()));
                    }
                }
                total = page.total;
                let read = page.entries.len();
                if read > budget_left {
                    return Err(Failure::Incomplete("目录树超过安全上限".into()));
                }
                budget_left -= read;
                if total > ceiling as i64 || entries.len() + read > ceiling {
                    report.issue(&dir, "list", format!("目录超过 {ceiling} 项，已跳过"));
                    continue 'discover;
                }
                entries.extend(page.entries);
                offset += read as u32;
                if read < page_size as usize || (total > 0 && entries.len() as i64 >= total) {
                    break;
                }
            }

            if entries.len() as i64 != total {
                return Err(Failure::Incomplete(format!(
                    "{} 只读到 {}/{} 项",
                    dir.display(),
                    entries.len(),
                    total
                )));
            }
            let mut names = HashSet::new();
            let mut ids = HashSet::new();
            for entry in &entries {
                if !ids.insert(entry.file_id().or_else(|| entry.folder_id())) {
                    report.issue(&dir, "list", "分页列表包含重复 ID");
                }
                let name = safe_name(&entry.name);
                if name != entry.name || !names.insert(name.to_lowercase()) {
                    report.issue(&dir, "list", "目录项名称不安全或重复，禁止清理");
                }
            }
            for entry in entries.iter().filter(|e| e.is_dir()) {
                if let Some(id) = entry.folder_id() {
                    stack.push((id, dir.join(safe_name(&entry.name)), depth + 1));
                    report.mappings[i].discovered_dirs += 1;
                }
            }
            serde_json::to_writer(&mut spool, &(&cid, &dir, &entries))
                .map_err(|e| spool_err(e.into()))?;
        }
        Ok(())
    }
    .await;
    // Even a truncated walk may have fully listed folders. Mirror that prefix,
    // but never claim a denominator or prune on its evidence.
    let failure_path = report.current.clone();
    let budget_paused = matches!(discovery, Err(Failure::Budget));
    spool.flush().map_err(spool_err)?;
    c.length = spool.get_ref().metadata().map_err(spool_err)?.len();
    drop(spool);
    let cache_complete =
        discovery.is_ok() && report.listing_start.is_some() && report.mappings[i].error_count == 0;
    if budget_paused || cache_complete {
        c.incomplete = report.mappings[i].error_count != 0;
        c.visited = visited;
        c.stack = stack;
        c.discovered = report.mappings[i].discovered_dirs;
        c.remaining = budget_left;
        save_json(&cursor_path, &c).map_err(spool_err)?;
    } else if cursor_path.exists() {
        std::fs::remove_file(&cursor_path).map_err(spool_err)?;
    }
    let row = &mut report.mappings[i];
    if discovery.is_ok() && row.error_count == 0 {
        row.total_dirs = Some(row.discovered_dirs);
    }
    row.state = State::Processing;
    publish(report, true);
    let mut records = 0;
    let input = BufReader::new(std::fs::File::open(&spool_path).map_err(spool_err)?);
    for record in serde_json::Deserializer::from_reader(input)
        .into_iter::<(String, PathBuf, Vec<client::Entry>)>()
    {
        let (cid, dir, entries) = record.map_err(|e| spool_err(e.into()))?;
        records += 1;
        report.current = dir.display().to_string();
        if !safe_descendant(local_root, &dir) {
            return Err(Failure::Incomplete("暂存目录不安全".into()));
        }
        dirs.insert(cid, dir.display().to_string());
        if let Err(error) = std::fs::create_dir_all(&dir) {
            report.issue(&dir, "mkdir", error.to_string());
            continue;
        }

        let next_codes = if config.options.prefetch_next > 0 {
            let videos = entries
                .iter()
                .filter(|entry| {
                    if entry.is_dir() || entry.pick_code.is_empty() {
                        return false;
                    }
                    let ext = Path::new(entry.name.trim())
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    is_video_ext(&ext)
                        && !(ext == "iso" && config.options.skip_iso)
                        && (config.options.min_video_size_mb == 0
                            || (entry.size.max(0) as u64)
                                >= config.options.min_video_size_mb * 1024 * 1024)
                })
                .map(|entry| (entry.name.as_str(), entry.pick_code.as_str()));
            following_in_folder(videos, usize::from(config.options.prefetch_next))
        } else {
            HashMap::new()
        };

        for entry in &entries {
            if entry.is_dir() {
                continue;
            }
            publish(report, false);
            let name = entry.name.trim();
            let ext = Path::new(name)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if !is_video_ext(&ext) {
                if !budget_paused
                    && is_asset_ext(&ext)
                    && asset_allowed(config, &ext)
                    && *assets_left > 0
                {
                    if !dir.join(safe_name(name)).exists()
                        && !entry.pick_code.is_empty()
                        && (1..=MAX_ASSET_BYTES).contains(&entry.size)
                    {
                        report.enrichment.push(Extra {
                            row: i,
                            dir: dir.clone(),
                            name: name.into(),
                            pc: entry.pick_code.clone(),
                            size: entry.size,
                            probe: false,
                        });
                        *assets_left -= 1;
                    }
                }
                continue;
            }
            if ext == "iso" && config.options.skip_iso {
                continue;
            }
            if config.options.min_video_size_mb > 0
                && (entry.size.max(0) as u64) < config.options.min_video_size_mb * 1024 * 1024
            {
                continue;
            }
            report.files += 1;
            report.mappings[i].files += 1;
            if entry.pick_code.is_empty() {
                report.issue(&dir.join(safe_name(name)), "strm", "没有 pickcode，已跳过");
                continue;
            }
            let stem = if config.options.keep_suffix {
                safe_name(name)
            } else {
                safe_name(strip_extension(name))
            };
            let next = next_codes
                .get(entry.pick_code.as_str())
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let before = report.written;
            let result = write_entry(
                config,
                &dir,
                &stem,
                &entry.pick_code,
                entry.size,
                next,
                config.options.overwrite_strm && !resumed,
                &mut seen,
                report,
            );
            report.mappings[i].written += report.written - before;
            if let Err(error) = result {
                let metadata = error.contains(".info.json");
                report.issue(
                    &dir.join(format!(
                        "{stem}.{}",
                        if metadata { "info.json" } else { "strm" }
                    )),
                    if metadata { "metadata" } else { "strm" },
                    error,
                );
                continue;
            }
            if config.options.probe_enabled && needs_probe(&dir, &stem) {
                report.pending_probe += 1;
                if !budget_paused && *probe_left > 0 {
                    *probe_left -= 1;
                    report.enrichment.push(Extra {
                        row: i,
                        dir: dir.clone(),
                        name: stem,
                        pc: entry.pick_code.clone(),
                        size: entry.size,
                        probe: true,
                    });
                }
            }
        }

        report.mappings[i].done_dirs += 1;
        report.calls = pacer().spent().await;
        report.dirs += 1;
        publish(report, false);
        if report.dirs % 25 == 0 {
            tracing::info!(
                dir = %dir.display(),
                dirs = report.dirs,
                files = report.files,
                "cloud115 mirror progress"
            );
        }
    }

    if crate::budget::background_blocked(config.options.daily_call_budget) {
        return Err(Failure::Budget);
    }
    if let Err(err) = discovery {
        report.current = failure_path;
        return Err(err);
    }
    if !cache_complete {
        std::fs::remove_file(&spool_path).map_err(spool_err)?;
    }
    if report.mappings[i].error_count == 0 && records != report.mappings[i].discovered_dirs {
        return Err(Failure::Incomplete("暂存目录数不完整，禁止清理".into()));
    }
    report.listed.extend(seen.iter().cloned());
    if let Some(mut candidates) = baseline.filter(|_| report.mappings[i].error_count == 0) {
        let folded: HashSet<_> = seen
            .iter()
            .map(|p| p.to_string_lossy().to_lowercase())
            .collect();
        candidates.retain(|path, _| !folded.contains(&path.to_string_lossy().to_lowercase()));
        if report.listing_start.is_none() {
            prune(local_root, &candidates, report);
        } else {
            report.removals.push((i, local_root.to_owned(), candidates));
        }
    }
    Ok(())
}

pub(crate) fn write_entry(
    config: &Config,
    dir: &Path,
    stem: &str,
    pick_code: &str,
    size: i64,
    next: &[String],
    overwrite: bool,
    seen: &mut HashSet<PathBuf>,
    report: &mut Report,
) -> Result<(), String> {
    if safe_name(stem) != stem {
        return Err("文件名不安全".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("建目录失败：{e}"))?;
    let strm = dir.join(format!("{stem}.strm"));
    let _lock = EntryLock::take(&strm).map_err(|e| format!("同步文件忙：{e}"))?;
    for target in [&strm, &strm.with_extension("info.json")] {
        if std::fs::symlink_metadata(target).is_ok_and(|m| !m.is_file()) {
            return Err("目标不是普通文件".into());
        }
    }
    let mut content = format!(
        "{}/api/module/{MODULE_ID}/play?pc={}",
        config.base_url,
        pick_code.trim().to_ascii_lowercase()
    );
    if !next.is_empty() {
        content.push_str("&next=");
        content.push_str(&next.join(","));
    }
    content.push('\n');
    if std::fs::read(&strm).ok().as_deref() == Some(content.as_bytes()) {
        report.kept += 1;
    } else if strm.exists() && !overwrite {
        report.kept += 1;
    } else {
        std::fs::write(&strm, &content).map_err(|e| format!("写 .strm 失败：{e}"))?;
        report.written += 1;
    }
    // The scanner reads this beside the `.strm` and marks the entry described, so
    // the probe pass (which can only fail on a URL) leaves it alone. Only the size
    // is known here; nothing invents a duration.
    let sidecar = dir.join(format!("{stem}.info.json"));
    if !sidecar.exists() {
        let body = serde_json::json!({ "size": size.max(0) }).to_string();
        std::fs::write(&sidecar, body).map_err(|e| format!("写 .info.json 失败：{e}"))?;
    }
    seen.insert(strm);
    Ok(())
}

/// Remove what the drive no longer has. Only files naming this module's play
/// route are considered, so an operator's own `.strm` in the same tree survives.
struct EntryLock(PathBuf);
impl EntryLock {
    fn take(file: &Path) -> io::Result<Self> {
        let path = file.with_extension("strm.cloud115-lock");
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }
}
impl Drop for EntryLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir(&self.0);
    }
}

type Stamp = (std::time::SystemTime, u64, u64);
type Baseline = HashMap<PathBuf, (Stamp, Option<Stamp>)>;
fn stamp(path: &Path) -> io::Result<Stamp> {
    let m = std::fs::symlink_metadata(path)?;
    if !m.is_file() {
        return Err(io::ErrorKind::InvalidData.into());
    }
    // A quick writer may replace a same-length URL within one timestamp tick.
    // Only small mirror-owned files are ever eligible for cleanup.
    use std::hash::Hasher;
    use std::io::Read;
    const LIMIT: u64 = 64 * 1024;
    if m.len() > LIMIT {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 != m.len() {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    hash.write(&bytes);
    Ok((m.modified()?, m.len(), hash.finish()))
}
fn baseline(local_root: &Path, start: Option<std::time::SystemTime>) -> io::Result<Baseline> {
    let mut files = HashMap::new();
    if !safe_descendant(local_root, local_root) {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut dirs = vec![local_root.to_owned()];
    let mut count = 0;
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let ty = entry.file_type()?;
            let path = entry.path();
            count += 1;
            if count > MAX_TREE_ENTRIES {
                return Err(io::ErrorKind::InvalidData.into());
            }
            if ty.is_dir() {
                if indirect(&std::fs::symlink_metadata(&path)?) {
                    return Err(io::ErrorKind::InvalidData.into());
                }
                dirs.push(path);
                continue;
            }
            if !ty.is_file() || path.extension().is_none_or(|ext| ext != "strm") {
                continue;
            }
            let s = stamp(&path)?;
            if start.is_some_and(|start| s.0 >= start) {
                continue;
            }
            let side = match stamp(&path.with_extension("info.json")) {
                Ok(s) => Some(s),
                Err(e) if e.kind() == io::ErrorKind::NotFound => None,
                Err(_) => continue,
            };
            files.insert(path, (s, side));
        }
    }
    Ok(files)
}
fn prune(local_root: &Path, files: &Baseline, report: &mut Report) {
    prune_checked(local_root, files, report, || true)
}
fn prune_checked(
    local_root: &Path,
    files: &Baseline,
    report: &mut Report,
    current: impl Fn() -> bool,
) {
    for (path, (original, side)) in files {
        if !path
            .parent()
            .is_some_and(|d| safe_descendant(local_root, d))
        {
            continue;
        }
        let Ok(_guard) = EntryLock::take(path) else {
            continue;
        };
        if stamp(path).ok().as_ref() != Some(original)
            || !std::fs::read_to_string(path).is_ok_and(|s| s.contains(&play_marker()))
        {
            continue;
        }
        if !current() {
            report.issue(path, "mapping", "授权或映射已变化，停止清理");
            return;
        }
        if let Err(e) = std::fs::remove_file(path) {
            report.issue(path, "strm", e.to_string());
            continue;
        }
        report.pruned += 1;
        let info = path.with_extension("info.json");
        if side.is_some() && stamp(&info).ok() == *side {
            if let Err(e) = std::fs::remove_file(&info) {
                report.issue(&info, "metadata", e.to_string());
            }
        }
    }
}

fn sidecar_path(dir: &Path, stem: &str) -> PathBuf {
    dir.join(format!("{stem}.info.json"))
}

/// A successful probe at the current schema version is complete even when the
/// container omits optional fields. Legacy duration-only results need a bounded
/// retry; failed attempts never advance this version.
fn needs_probe(dir: &Path, stem: &str) -> bool {
    let Ok(bytes) = std::fs::read(sidecar_path(dir, stem)) else {
        return true;
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return true;
    };
    let version_ok = value
        .get("probe_version")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        >= u64::from(crate::probe::MEDIA_PROBE_VERSION);
    // A sidecar from a pass that could not read the video track's own
    // parameters is still incomplete even though its duration was written.
    let video_ok = value.get("video").is_none_or(|video| {
        let technical = video.get("technical");
        let present = technical.is_some_and(|technical| !technical.is_null());
        let needs_profile = matches!(
            video.get("codec").and_then(serde_json::Value::as_str),
            Some("hevc" | "h264")
        );
        present
            && (!needs_profile
                || technical
                    .and_then(|technical| technical.get("profile"))
                    .is_some_and(|profile| !profile.is_null()))
    });
    let audio_ok = value.get("audio_tracks").is_none_or(|tracks| {
        tracks.as_array().is_none_or(|tracks| {
            tracks.iter().all(|track| {
                let aac = track.get("codec").and_then(serde_json::Value::as_str) == Some("aac");
                !aac || track
                    .get("technical")
                    .and_then(|technical| technical.get("profile"))
                    .is_some_and(|profile| !profile.is_null())
            })
        })
    });
    !version_ok || !video_ok || !audio_ok
}

/// Replace the size-only sidecar with one the scanner can describe the entry
/// from: the wire shapes KROMA's own probe pass writes.
fn write_sidecar(
    dir: &Path,
    stem: &str,
    size: i64,
    described: &crate::probe::Probed,
) -> Result<(), String> {
    let mut body = serde_json::json!({
        "size": size.max(0),
        "probe_version": crate::probe::MEDIA_PROBE_VERSION,
    });
    let map = body
        .as_object_mut()
        .ok_or_else(|| "sidecar is not an object".to_string())?;
    if let Some(duration) = described.duration_ms {
        map.insert("duration_ms".into(), duration.into());
    }
    if let Some(video) = described.video.as_ref() {
        map.insert(
            "video".into(),
            serde_json::to_value(video).map_err(|e| e.to_string())?,
        );
    }
    if let Some(audio) = described.audio.as_ref() {
        map.insert(
            "audio".into(),
            serde_json::to_value(audio).map_err(|e| e.to_string())?,
        );
    }
    if !described.audio_tracks.is_empty() {
        map.insert(
            "audio_tracks".into(),
            serde_json::to_value(&described.audio_tracks).map_err(|e| e.to_string())?,
        );
    }
    if !described.subtitles.is_empty() {
        map.insert(
            "subtitles".into(),
            serde_json::to_value(&described.subtitles).map_err(|e| e.to_string())?,
        );
    }
    std::fs::write(sidecar_path(dir, stem), body.to_string())
        .map_err(|e| format!("写 .info.json 失败：{e}"))
}

// Transport errors can contain a signed URL. Persist the failure, not the secret.
fn download_error(raw: &str) -> String {
    let err = raw.to_ascii_lowercase();
    if err.contains("permission denied")
        || err.contains("access is denied")
        || err.contains("os error 5)")
        || err.contains("folder this module may write")
        || err.contains("拒绝访问")
    {
        return "目标文件夹不可写：请检查该映射的目录授权".into();
    }
    if err.contains("no space left") || err.contains("disk full") || err.contains("os error 112)") {
        return "目标磁盘空间不足".into();
    }
    if err.contains("cannot write ") || err.contains("not found") || err.contains("os error 3)") {
        return "无法打开目标文件夹或文件已不可访问".into();
    }
    if let Some(code) = err
        .split("curl exit ")
        .nth(1)
        .and_then(|s| s.split(':').next())
        .and_then(|s| s.parse::<u32>().ok())
    {
        let reason = match code {
            6 => "无法解析下载服务器",
            7 => "无法连接下载服务器",
            23 => "无法写入文件",
            28 => "下载超时",
            63 => "文件超过大小限制",
            _ => "下载传输失败",
        };
        return format!("{reason}（curl {code}）");
    }
    if err.contains("blocked") || err.contains("refused") {
        return "下载被宿主网络策略拒绝".into();
    }
    "宿主未能完成附属文件下载或保存".into()
}

pub(crate) fn is_asset_ext(ext: &str) -> bool {
    ASSET_EXTS.contains(&ext)
}

/// Fetch one companion file (subtitle, NFO, poster) beside its video. Existing
/// files are left alone, so a season's subtitles are downloaded once.
pub(crate) async fn download_asset(
    config: &Config,
    pick_code: &str,
    name: &str,
    size: i64,
    dir: &Path,
) -> Result<bool, String> {
    if pick_code.is_empty() || size <= 0 || size > MAX_ASSET_BYTES {
        return Ok(false);
    }
    let target = dir.join(safe_name(name));
    match std::fs::symlink_metadata(&target) {
        Ok(_) => return Ok(false), // Includes dangling links, never follow one on download.
        Err(e) if e.kind() == io::ErrorKind::NotFound => (),
        Err(e) => return Err(e.to_string()),
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("建目录失败（{}）：{e}", dir.display()))?;
    let link = client::sign(&config.auth_id, client::MODULE_UA, pick_code)
        .await
        .map_err(|e| format!("获取附属文件直链失败：{}", e.sync_reason()))?;
    let response = Fetch::default()
        .purpose("cloud-drive")
        .header("User-Agent", client::MODULE_UA)
        .max_time(120)
        .max_bytes(MAX_ASSET_BYTES as u64)
        .download(&link.url, &target)
        .map_err(|e| download_error(&e.to_string()))?;
    if !(200..300).contains(&response.status) {
        return Err(format!("下载附属文件失败（HTTP {}）", response.status));
    }
    Ok(true)
}

fn episode_number(name: &str) -> Option<(u16, u16)> {
    fn digits(bytes: &[u8]) -> Option<(u16, usize)> {
        let len = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
        if !(1..=4).contains(&len) {
            return None;
        }
        Some((
            bytes[..len]
                .iter()
                .fold(0, |n, b| n * 10 + u16::from(*b - b'0')),
            len,
        ))
    }
    let bytes = name.as_bytes();
    bytes.iter().enumerate().find_map(|(i, &b)| {
        if !b.eq_ignore_ascii_case(&b's') {
            return None;
        }
        let (season, len) = digits(&bytes[i + 1..])?;
        let ep = i + 1 + len;
        if !bytes.get(ep)?.eq_ignore_ascii_case(&b'e') {
            return None;
        }
        Some((season, digits(&bytes[ep + 1..])?.0))
    })
}

pub(crate) fn following_in_folder<'a>(
    videos: impl Iterator<Item = (&'a str, &'a str)>,
    count: usize,
) -> HashMap<&'a str, Vec<String>> {
    let mut ordered: Vec<_> = videos
        .filter(|(_, pc)| {
            !pc.is_empty() && pc.len() <= 64 && pc.bytes().all(|b| b.is_ascii_alphanumeric())
        })
        .map(|(name, pc)| (episode_number(name), name.to_lowercase(), name, pc))
        .collect();
    ordered.sort_unstable();
    let count = count.min(3);
    let mut hints = HashMap::new();
    let mut after = Vec::<String>::new();
    let mut end = ordered.len();
    while end > 0 && count != 0 {
        let number = ordered[end - 1].0;
        let start = if number.is_some() {
            ordered[..end].partition_point(|v| v.0 < number)
        } else {
            end - 1
        };
        for entry in &ordered[start..end] {
            if !after.is_empty() {
                hints.insert(entry.3, after.clone());
            }
        }
        after.insert(0, ordered[start].3.to_ascii_lowercase());
        after.truncate(count);
        if start > 0 && ordered[start - 1].0.is_none() && number.is_some() {
            after.clear();
        }
        end = start;
    }
    hints
}

pub(crate) fn is_video_ext(ext: &str) -> bool {
    matches!(
        ext,
        "mkv"
            | "mp4"
            | "ts"
            | "m2ts"
            | "iso"
            | "avi"
            | "mov"
            | "wmv"
            | "flv"
            | "webm"
            | "rmvb"
            | "mpg"
            | "mpeg"
            | "m4v"
            | "rm"
    )
}

pub(crate) fn strip_extension(name: &str) -> &str {
    match name.rfind('.') {
        Some(at) if at > 0 => &name[..at],
        _ => name,
    }
}

/// A file name as the local filesystem can hold it, for every filesystem this
/// server might run on. Long names are cut so a deep tree stays under PATH_MAX.
pub(crate) fn safe_name(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect::<String>()
        .trim()
        .trim_end_matches('.')
        .to_string();
    if out.is_empty() || out == "." || out == ".." {
        out = "_".to_string();
    }
    if out.chars().count() > 150 {
        out = out.chars().take(150).collect();
    }
    out.truncate(out.trim_end_matches(['.', ' ']).len());
    let base = out
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end()
        .to_ascii_uppercase();
    let device = matches!(
        base.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].iter().any(|p| {
        base.strip_prefix(*p).is_some_and(|n| {
            matches!(
                n,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    });
    if device {
        out = format!("_{}", out.chars().take(149).collect::<String>());
    }
    if out.is_empty() {
        out.push('_');
    }
    out
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn episode_hints_follow_numbers_not_release_prefix_or_edition() {
        let hints = following_in_folder(
            [
                ("新集 S01E2.mkv", "two"),
                ("保留 S01E10.mkv", "ten"),
                ("旧集 S01E1.mkv", "one"),
                ("另一版 s01e2.mkv", "twoalt"),
                ("花絮.mkv", "bonus"),
            ]
            .into_iter(),
            2,
        );
        assert_eq!(hints["two"], ["ten"]);
        assert_eq!(hints["twoalt"], ["ten"]);
        assert_eq!(hints["one"].len(), 2);
        assert_eq!(hints["one"][1], "ten");
        assert!(!hints.contains_key("bonus") && !hints.contains_key("ten"));
    }

    #[test]
    fn history_keeps_an_actionable_reason_and_a_display_path_not_download_credentials() {
        let mut report = Report {
            active_root: PathBuf::from("/selected/films"),
            mappings: vec![Mapping {
                remote: "/电影".into(),
                local_display: r"F:\电影".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        report.issue(
            Path::new("/selected/films/动画/片.nfo"),
            "asset",
            download_error("curl exit 28: timeout at https://invalid.example/asset?token=secret"),
        );
        assert_eq!(report.mappings[0].errors[0].path, r"F:\电影\动画\片.nfo");
        assert_eq!(report.mappings[0].errors[0].message, "下载超时（curl 28）");
        assert!(!serde_json::to_string(&report).unwrap().contains("secret"));
    }

    #[tokio::test]
    async fn short_listing_preserves_owned_files_and_has_no_denominator() {
        let base = std::env::temp_dir().join(format!("cloud115-short-test-{}", std::process::id()));
        let _guard = RunGuard::acquire(base.clone()).unwrap();
        let root = base.join("local");
        std::fs::create_dir(&root).unwrap();
        let old = root.join("previous.strm");
        std::fs::write(&old, play_marker()).unwrap();
        let mut config = Config::default();
        config.options.daily_call_budget = 0;
        config.options.page_size = 10;
        let mut report = Report {
            mappings: vec![Mapping::default()],
            checkpoint: Some(base.join("report.json")),
            ..Default::default()
        };
        let result = sync_tree(
            &config,
            "1".into(),
            &root,
            &mut Default::default(),
            &mut 0,
            &mut 0,
            &mut report,
            &base,
            |_, _, _| {
                std::future::ready(Ok(client::Page {
                    entries: vec![],
                    total: 3,
                    ..Default::default()
                }))
            },
        )
        .await;
        finish_mapping(&mut report, result);
        let row = &report.mappings[0];
        assert_eq!(row.state, State::Partial);
        assert_eq!(row.total_dirs, None);
        assert_eq!(row.done_dirs, 0);
        assert_eq!(row.errors[0].path, root.display().to_string());
        assert_eq!(row.errors[0].stage, "list");
        assert!(old.exists());
    }

    #[tokio::test]
    async fn two_roots_replay_each_page_once_and_checkpoint_independently() {
        let scratch =
            std::env::temp_dir().join(format!("cloud115-row-test-{}", std::process::id()));
        let guard = RunGuard::acquire(scratch.clone()).unwrap();
        assert!(RunGuard::acquire(scratch.clone()).is_err());
        let latest = scratch.join("report.json");
        let recent = scratch.join("history.json");
        let mut config = Config::default();
        config.options.page_size = 1;
        config.options.max_entries_per_folder = 100;
        config.options.probe_enabled = false;
        config.options.daily_call_budget = 0;
        config.options.min_video_size_mb = 0;
        for remote in ["Movies", "Shows"] {
            config.libraries.push(Library {
                remote: remote.into(),
                local: format!("/selected/{remote}"),
                local_display: format!("D:/{remote}"),
                ..Library::default()
            });
        }
        let mut report = Report::queued(&config);
        report.checkpoint = Some(latest.clone());
        publish(&mut report, true);
        assert!(read_status(&latest, &recent, true, Vec::new)
            .1
            .mappings
            .iter()
            .all(|m| m.state == State::Queued && m.total_dirs.is_none()));
        let mut dirs = Default::default();
        for i in 0..2 {
            let root = scratch.join(&report.mappings[i].remote);
            std::fs::create_dir_all(&root).unwrap();
            let stale = root.join("old.strm");
            std::fs::write(&stale, play_marker()).unwrap();
            if i == 1 {
                std::fs::write(root.join("Season"), "blocks directory creation").unwrap();
            }
            report.active = i;
            report.mappings[i].state = State::Discovering;
            publish(&mut report, true);
            let mut calls = Vec::new();
            report.observed.clear();
            let result=sync_tree(&config,"1".into(),&root,&mut dirs,&mut 0,&mut 0,&mut report,&scratch,|cid,offset,_| {
                let live=read_status(&latest,&recent,true,Vec::new).1;
                assert_eq!(live.mappings[i].state,State::Discovering);
                let (total,j)=match (cid.as_str(),offset) {
                    ("1",0)=>(2,serde_json::json!({"fc":"0","pid":"1","fid":"2","fn":"Season"})),
                    ("1",1)=>(2,serde_json::json!({"fid":"3","pid":"1","fc":1,"pc":if i==0 {"abc"}else{""},"fn":"Film.mkv"})),
                    ("2",0)=>(1,serde_json::json!({"fid":"4","pid":"2","fc":1,"pc":"def","fn":"Episode.mkv"})),
                    _=>panic!("unexpected page") };
                calls.push((cid,offset)); std::future::ready(Ok(client::Page{entries:vec![serde_json::from_value(j).unwrap()],total,..Default::default()}))
            }).await;
            finish_mapping(&mut report, result);
            assert_eq!(
                calls,
                vec![("1".into(), 0), ("1".into(), 1), ("2".into(), 0)]
            );
            let progress: Vec<_> = report
                .observed
                .iter()
                .filter(|(s, _, _)| *s == State::Processing)
                .collect();
            assert!(progress.iter().all(|(_, t, _)| *t == Some(2)));
            assert_eq!(progress.first().unwrap().2, 0);
            assert_eq!(progress.last().unwrap().2, if i == 0 { 2 } else { 1 });
            let row = &report.mappings[i];
            assert_eq!(row.discovered_dirs, 2);
            assert_eq!(stale.exists(), i == 1);
            if i == 0 {
                assert_eq!(row.state, State::Completed);
                assert_eq!((row.files, row.written), (2, 2));
            } else {
                assert_eq!(row.state, State::Partial);
                assert_eq!(row.error_count, 2);
                assert!(row
                    .errors
                    .iter()
                    .any(|e| e.stage == "strm" && e.path.ends_with("Film.mkv")));
                assert!(row
                    .errors
                    .iter()
                    .any(|e| e.stage == "mkdir" && e.path.ends_with("Season")));
            }
        }
        let legacy =
            || vec![serde_json::from_str::<HistoryEntry>(r#"{"files":17,"at":1}"#).unwrap()];
        assert_eq!(read_status(&recent, &recent, false, legacy).1.files, 17);
        report.finished_at = Some(unix_now());
        publish(&mut report, true);
        append_history(&recent, &report, legacy).unwrap();
        let (_, snapshot, history) = read_status(&latest, &recent, false, || panic!("not cache"));
        assert!(history[0].mappings.is_empty());
        assert_eq!(
            serde_json::to_value(history[1].mappings.clone()).unwrap(),
            serde_json::to_value(snapshot.mappings).unwrap()
        );
        report.finished_at = None;
        report.mappings[1].state = State::Processing;
        publish(&mut report, true);
        assert_eq!(
            read_status(&latest, &recent, false, Vec::new).1.mappings[1].state,
            State::Interrupted
        );
        drop(guard);
        assert!(!scratch.exists());
    }

    #[tokio::test]
    async fn budget_resumes_exact_child_page_without_spending_on_sidecars() {
        let scratch = std::env::temp_dir().join(format!("cloud115-resume-{}", std::process::id()));
        let _guard = RunGuard::acquire(scratch.clone()).unwrap();
        let root = scratch.join("电影");
        std::fs::create_dir(&root).unwrap();
        let stale = root.join("keep.strm");
        std::fs::write(&stale, play_marker()).unwrap();
        let mut config = Config::default();
        config.options.page_size = 1;
        for pass in 0..2 {
            let mut report = Report {
                mappings: vec![Mapping::default()],
                checkpoint: Some(scratch.join("progress.json")),
                ..Default::default()
            };
            let mut requests = vec![];
            let mut remaining = if pass == 0 { 4 } else { 1 };
            let result = sync_tree(&config,"1".into(),&root,&mut Default::default(),&mut 4,&mut 4,&mut report,&scratch,|cid,offset,_| {
                requests.push((cid.clone(),offset));
                if remaining == 0 { return std::future::ready(Err(ApiError {http:0,errno:0,message:"调用上限".into()})); }
                remaining -= 1;
                let (total, fid, fc, name) = match (cid.as_str(),offset) {
                    ("1",0) => (3,2,0,"Season"), ("1",1) => (3,3,1,"Film.nfo"), ("1",2) => (3,4,1,"Film.mkv"),
                    ("2",0) => (2,5,1,"First.mkv"), ("2",1) => (2,6,1,"Last.mkv"), _=>panic!("unexpected list")
                };
                let entry = serde_json::from_value(serde_json::json!({"fid":fid,"pid":cid,"fc":fc,"fn":name,"fs":10,"pc":"abc"})).unwrap();
                std::future::ready(Ok(client::Page{total,entries:vec![entry],..Default::default()}))
            }).await;
            if pass == 0 {
                assert!(matches!(result, Err(Failure::Budget)));
                assert!(root.join("Film.strm").exists());
                assert_eq!(report.mappings[0].total_dirs, None);
                let mut other = config.clone();
                other.auth_id = "different".into();
                assert!(saved_cursor(&scratch, 0, &cursor_key(&other, &root, 0)).is_none());
                other = config.clone();
                other.options.keep_suffix = !config.options.keep_suffix;
                assert!(saved_cursor(&scratch, 0, &cursor_key(&other, &root, 0)).is_none());
            } else {
                assert!(result.is_ok());
                assert_eq!(requests, vec![("2".into(), 1)]);
                assert_eq!(report.mappings[0].total_dirs, Some(2));
                assert!(root.join("Season/Last.strm").exists());
                assert!(report.enrichment.iter().any(|j| !j.probe));
            }
            assert!(stale.exists());
            assert!(!root.join("Film.nfo").exists());
        }
    }

    #[tokio::test]
    async fn exhausted_enrichment_keeps_playable_outputs_in_both_mappings() {
        let scratch = std::env::temp_dir().join(format!("cloud115-extras-{}", std::process::id()));
        let _guard = RunGuard::acquire(scratch.clone()).unwrap();
        let cfg = Config::default();
        let mut report = Report {
            mappings: vec![Mapping::default(); 2],
            checkpoint: Some(scratch.join("report.json")),
            ..Default::default()
        };
        for i in 0..2 {
            report.active = i;
            let root = scratch.join(i.to_string());
            let result=sync_tree(&cfg,"0".into(),&root,&mut Default::default(),&mut 2,&mut 2,&mut report,&scratch,|_,_,_| {
                let entries=[("1","Film.nfo"),("2","Film.mkv")].into_iter().map(|(fid,name)|serde_json::from_value(serde_json::json!({"fid":fid,"pid":"0","fc":1,"fn":name,"fs":8,"pc":"abc"})).unwrap()).collect();
                std::future::ready(Ok(client::Page{entries,total:2,..Default::default()}))
            }).await;
            assert!(result.is_ok());
        }
        assert_eq!(report.enrichment.len(), 4);
        enrich(&cfg, &mut report).await; // No allowance remains. No signing or probing is possible.
        assert_eq!(report.pending_probe, 2);
        for i in 0..2 {
            let root = scratch.join(i.to_string());
            assert!(root.join("Film.strm").exists());
            assert!(!root.join("Film.nfo").exists());
        }
    }

    #[test]
    fn a_name_is_made_safe_for_every_filesystem() {
        for name in [
            "CON",
            "nul.nfo",
            "Lpt1",
            "COM¹.txt",
            "C:\\escape",
            ".",
            "..",
            "x/y",
        ] {
            let out = safe_name(name);
            assert_ne!(out, name);
            assert_eq!(safe_name(&out), out);
        }
        assert_eq!(safe_name("A/B:C*D?"), "A_B_C_D_");
        assert_eq!(safe_name("  ..  "), "_");
        assert_eq!(safe_name(""), "_");
        assert_eq!(safe_name("Movie. "), "Movie");
        let long = "x".repeat(400);
        assert_eq!(safe_name(&long).chars().count(), 150);
    }

    #[test]
    fn only_files_naming_this_module_are_prunable() {
        let root = std::env::temp_dir().join(format!("cloud115-prune-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("sub")).unwrap();
        let ours = root.join("ours.strm");
        let theirs = root.join("theirs.strm");
        std::fs::write(&ours, format!("http://host{}abc\n", play_marker())).unwrap();
        std::fs::write(&theirs, "http://127.0.0.1:4569/115/play?pickcode=abc\n").unwrap();
        std::fs::write(root.join("ours.info.json"), "{\"size\":1}").unwrap();

        let mut report = Report::default();
        let snapshot = baseline(&root, None).unwrap();
        let added = root.join("quick.strm");
        std::fs::write(&added, play_marker()).unwrap();
        prune(&root, &snapshot, &mut report);
        assert!(added.exists());

        assert!(!ours.exists(), "our own stale entry is removed");
        assert!(
            !root.join("ours.info.json").exists(),
            "its sidecar goes too"
        );
        assert!(theirs.exists(), "another bridge's .strm is left alone");
        assert_eq!(report.pruned, 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_entry_in_the_listing_is_never_pruned() {
        let root = std::env::temp_dir().join(format!("cloud115-keep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let kept = root.join("kept.strm");
        std::fs::write(&kept, format!("http://host{}abc\n", play_marker())).unwrap();
        let snapshot = baseline(&root, None).unwrap();
        let mut report = Report::default();
        write_entry(
            &Config::default(),
            &root,
            "kept",
            "replacement",
            42,
            &[],
            true,
            &mut HashSet::new(),
            &mut report,
        )
        .unwrap();
        prune(&root, &snapshot, &mut report);
        let locked_snapshot = baseline(&root, None).unwrap();
        let lock = EntryLock::take(&kept).unwrap();
        prune(&root, &locked_snapshot, &mut report);
        drop(lock);

        assert!(kept.exists());
        assert_eq!(report.pruned, 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_local_mapping_must_name_a_host_issued_selected_folder() {
        assert_eq!(
            resolve_local_path(&[], "/selected/5f36c2a1-0000-4000-8000-000000000000").as_deref(),
            Some(Path::new("/selected/5f36c2a1-0000-4000-8000-000000000000"))
        );
        assert!(resolve_local_path(&[], "/media/115/电影").is_none());
        assert!(resolve_local_path(&[], "D:\\Media\\Movies").is_none());
        assert!(resolve_local_path(&[], "/selected/../module").is_none());
        assert!(resolve_local_path(&[], "/selected/..").is_none());
        assert!(resolve_local_path(&[], "/selected/").is_none());
    }
}
