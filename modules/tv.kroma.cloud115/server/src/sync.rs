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

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

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
const ASSET_EXTS: [&str; 8] = ["srt", "ass", "ssa", "sup", "nfo", "jpg", "jpeg", "png"];

/// A companion larger than this is not a subtitle or a poster.
const MAX_ASSET_BYTES: i64 = 200 * 1024 * 1024;

/// What the module calls a play route: the only `.strm` files a prune may
/// remove are ones naming this module.
fn play_marker() -> String {
    format!("/api/module/{MODULE_ID}/play?pc=")
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
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
}

static RUNNING: AtomicBool = AtomicBool::new(false);
static REPORT: OnceLock<Mutex<Report>> = OnceLock::new();

fn report_slot() -> &'static Mutex<Report> {
    REPORT.get_or_init(|| Mutex::new(Report::default()))
}

/// Hand the run's numbers to the status route while it is still working, so the
/// page shows a moving progress rather than the previous run's totals.
fn publish(report: &Report) {
    if let Ok(mut slot) = report_slot().lock() {
        *slot = report.clone();
    }
}

pub fn is_running() -> bool {
    RUNNING.load(Ordering::Acquire)
}

/// The last finished run, and whether one is running now.
pub fn status() -> (bool, Report) {
    let report = report_slot()
        .lock()
        .map(|r| r.clone())
        .unwrap_or_default();
    (is_running(), report)
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
    if RUNNING.swap(true, Ordering::AcqRel) {
        return Err("同步已经在运行".into());
    }
    let result = prepare_and_spawn(host, base_override, manual);
    if result.is_err() {
        RUNNING.store(false, Ordering::Release);
    }
    result
}

fn prepare_and_spawn<S: HostCtx + Clone + Send + Sync + 'static>(
    host: S,
    base_override: Option<String>,
    manual: bool,
) -> Result<(), String> {
    if let Some(base) = base_override.map(|b| b.trim().trim_end_matches('/').to_string()) {
        if !base.starts_with("http://") && !base.starts_with("https://") {
            return Err("服务器地址必须形如 http://192.168.1.10:8096".into());
        }
        crate::store::patch(|stored| stored.base_url = base)?;
    }
    let config = Config::load();
    if config.cookie.is_empty() {
        return Err("先填写 115 的 Cookie".into());
    }
    if config.base_url.is_empty() {
        return Err("先填写服务器地址（播放链接要用它拼接）".into());
    }
    if !config.runnable() {
        return Err("先配置至少一个目录映射（网盘目录 → 本地子目录）".into());
    }
    let folders = host.library_folders();
    // A run an operator started by hand may spend more of the account's call
    // allowance: they are watching this one finish.
    let mut config = config;
    if manual {
        config.options.max_calls_per_run = config.options.max_calls_per_run.max(5000);
    }
    let probe_cap = if manual {
        MANUAL_PROBES_PER_RUN
    } else {
        MAX_PROBES_PER_RUN
    };

    tokio::spawn(async move {
        let report = run(&host, &config, &folders, probe_cap).await;
        if let Ok(mut slot) = report_slot().lock() {
            *slot = report;
        }
        RUNNING.store(false, Ordering::Release);
    });
    Ok(())
}

enum Failure {
    /// The per-run call budget ran out. Not an error: the rest waits for the next
    /// run, and nothing may be pruned on the way out.
    Budget,
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
    fn changed(&self) -> bool {
        self.written > 0 || self.pruned > 0
    }
}

async fn run<S: HostCtx>(
    host: &S,
    config: &Config,
    folders: &[LibraryFolders],
    probe_cap: u32,
) -> Report {
    let started = tokio::time::Instant::now();
    pacer().begin_run(&config.options).await;
    let mut report = Report::default();
    report.started_at = Some(unix_now());
    report.calls_total = pacer().budget().await;
    publish(&report);
    let mut dirs = std::collections::BTreeMap::new();
    let mut probe_left = probe_cap;
    let mut assets_left = MAX_ASSETS_PER_RUN;

    for library in config.libraries.iter().filter(|l| !l.remote.trim().is_empty() && !l.local.trim().is_empty()) {
        let Some(local) = resolve_local_path(folders, &library.local) else {
            report.errors.push(format!(
                "{}: 本地目录无效（填一个存储里的绝对路径，如 /media/115/动画电影）",
                library.remote
            ));
            continue;
        };
        match sync_library(
            config,
            library,
            &local,
            &mut dirs,
            &mut probe_left,
            &mut assets_left,
            &mut report,
        )
        .await
        {
            Ok(()) => {}
            Err(Failure::Budget) => {
                report.truncated = true;
                report.errors.push(format!(
                    "本轮达到调用上限，{} 的内剩余部分留给下一轮（本轮不做删除）",
                    library.remote
                ));
                break;
            }
            Err(Failure::Api(error)) => report
                .errors
                .push(format!("{}: {error}（本轮不做删除）", library.remote)),
            Err(Failure::Incomplete(why)) => report
                .errors
                .push(format!("{}: {why}（本轮不做删除）", library.remote)),
        }
    }

    report.calls = pacer().spent().await;
    report.elapsed_ms = started.elapsed().as_millis() as u64;
    report.finished_at = Some(unix_now());
    report.current.clear();
    publish(&report);
    let summary = crate::store::HistoryEntry {
        at: unix_now(),
        dirs: report.dirs,
        files: report.files,
        written: report.written,
        pruned: report.pruned,
        errors: report.errors.len() as u32,
        truncated: report.truncated,
    };
    let _ = crate::store::patch(move |stored| {
        stored.history.push(summary);
        let excess = stored.history.len().saturating_sub(20);
        if excess > 0 {
            stored.history.drain(0..excess);
        }
    });
    // A run that failed halfway has no business replacing the watcher's map: the
    // half it did not walk would look like it left the library.
    if !report.truncated && !dirs.is_empty() {
        let _ = crate::store::patch(|stored| stored.dirs = dirs);
    }
    if !report.errors.is_empty() {
        // The page shows these; the container log needs at least the first one,
        // or "nothing happened" is all an operator can see afterwards.
        tracing::warn!(
            errors = report.errors.len(),
            first = report.errors.first().map(String::as_str).unwrap_or(""),
            more = %report.errors.iter().skip(1).take(2).cloned().collect::<Vec<_>>().join(" | "),
            truncated = report.truncated,
            "cloud115 sync finished with errors"
        );
    }
    if report.changed() {
        // The catalogue only learns about a written or removed .strm when the
        // scanner walks the folders again. Ask for that now instead of leaving
        // the new entries invisible until the next scheduled scan.
        host.trigger_job("library.scan", "cloud115 sync");
    }
    report
}

/// The local folder a mapping writes into. The mapping names storage, never a
/// media library: an absolute path is used as it stands, and an old relative
/// path is anchored at the first real library folder (or `/media`), because
/// that is what such a config meant when it was written.
pub(crate) fn resolve_local_path(folders: &[LibraryFolders], local: &str) -> Option<PathBuf> {
    let trimmed = local.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with('/') {
        return Some(PathBuf::from(trimmed));
    }
    let root = folders
        .iter()
        .flat_map(|f| f.folders.first())
        .find(|root| !root.starts_with("<demo>"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/media"));
    clean_relative(trimmed).map(|relative| root.join(relative))
}

/// A mapping's local side, as a path that cannot leave the library: no absolute
/// path, no `..`, nothing that resolves outside.
fn clean_relative(raw: &str) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for part in raw.split(['/', '\\']) {
        let part = part.trim();
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." || part.contains('\0') {
            return None;
        }
        out.push(part);
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

async fn sync_library(
    config: &Config,
    library: &Library,
    local_root: &Path,
    dirs: &mut std::collections::BTreeMap<String, String>,
    probe_left: &mut u32,
    assets_left: &mut u32,
    report: &mut Report,
) -> Result<(), Failure> {
    let root_cid = client::folder_id_by_path(&config.cookie, client::MODULE_UA, &library.remote)
        .await
        .map_err(map_api)?;

    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut budget_left = MAX_TREE_ENTRIES;
    let mut stack = vec![(root_cid, local_root.to_path_buf(), 0usize)];

    while let Some((cid, dir, depth)) = stack.pop() {
        if depth > MAX_DEPTH {
            report
                .errors
                .push(format!("{} 超过 {} 层，已跳过", dir.display(), MAX_DEPTH));
            continue;
        }
        // The watcher turns an event's parent id into a path with this map.
        dirs.insert(cid.clone(), dir.to_string_lossy().into_owned());
        report.current = dir.to_string_lossy().into_owned();
        report.calls = pacer().spent().await;
        publish(report);
        let page_size = config.options.page_size;
        let mut offset = 0u32;
        let mut entries = Vec::new();
        let mut total;
        loop {
            let page = client::list(&config.cookie, client::MODULE_UA, &cid, offset, page_size)
                .await
                .map_err(map_api)?;
            total = page.total;
            let read = page.entries.len();
            entries.extend(page.entries);
            offset += read as u32;
            if read < page_size as usize || (total > 0 && entries.len() as i64 >= total) {
                break;
            }
        }

        if total > 0 && (entries.len() as i64) < total {
            return Err(Failure::Incomplete(format!(
                "{} 只读到 {}/{} 项",
                dir.display(),
                entries.len(),
                total
            )));
        }
        // One folder that cannot be created ends this folder, with its own path:
        // per-file failures here would repeat the same cause dozens of times.
        if let Err(error) = std::fs::create_dir_all(&dir) {
            report
                .errors
                .push(format!("{}: 建目录失败：{error}", dir.display()));
            continue;
        }
        let ceiling = (config.options.max_entries_per_folder as usize).min(HARD_ENTRY_CEILING);
        if entries.len() > ceiling {
            // A folder this large is the single biggest reason an account gets
            // flagged, and reading it is also the slowest thing a run can do.
            report.errors.push(format!(
                "{} 有 {} 项，超过上限 {}，已跳过（先拆分这个目录）",
                dir.display(),
                entries.len(),
                ceiling
            ));
            continue;
        }
        if (entries.len() as usize) > budget_left {
            report.truncated = true;
            return Err(Failure::Budget);
        }
        budget_left -= entries.len() as usize;

        let mut subfolders = Vec::new();
        for entry in &entries {
            if entry.is_dir() {
                if let Some(id) = entry.folder_id() {
                    subfolders.push((id, safe_name(&entry.name)));
                }
                continue;
            }
            let name = entry.name.trim();
            let ext = Path::new(name)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if !is_video_ext(&ext) {
                if is_asset_ext(&ext) && *assets_left > 0 {
                    match download_asset(config, &entry.pick_code, name, entry.size, &dir).await {
                        Ok(true) => {
                            report.assets += 1;
                            *assets_left -= 1;
                        }
                        Ok(false) => {}
                        Err(error) => report.errors.push(format!("{name}: {error}")),
                    }
                }
                continue;
            }
            if ext == "iso" && config.options.skip_iso {
                continue;
            }
            report.files += 1;
            if entry.pick_code.is_empty() {
                report.errors.push(format!("{name} 没有 pickcode，已跳过"));
                continue;
            }
            let stem = safe_name(strip_extension(name));
            if let Err(error) = write_entry(config, &dir, &stem, &entry.pick_code, entry.size, &mut seen, report) {
                report.errors.push(format!("{}: {error}", dir.display()));
                continue;
            }
            if config.options.probe_enabled && needs_probe(&dir, &stem) {
                if *probe_left == 0 {
                    report.pending_probe += 1;
                    continue;
                }
                match crate::probe::probe(&config.cookie, &entry.pick_code, entry.size).await {
                    Ok(Some(described)) => {
                        if let Err(error) = write_sidecar(&dir, &stem, entry.size, &described) {
                            report.errors.push(format!("{}: {error}", dir.display()));
                        } else {
                            report.probed += 1;
                            *probe_left -= 1;
                        }
                    }
                    Ok(None) => {}
                    Err(error) if error.http == 0 && error.message.contains("调用上限") => {
                        return Err(Failure::Budget);
                    }
                    Err(error) => {
                        tracing::debug!(file = %name, %error, "probe gave up on an entry");
                    }
                }
            }
        }

        for (id, name) in subfolders {
            stack.push((id, dir.join(name), depth + 1));
        }
        report.dirs += 1;
        if report.dirs % 25 == 0 {
            tracing::info!(
                dir = %dir.display(),
                dirs = report.dirs,
                files = report.files,
                "cloud115 mirror progress"
            );
        }
    }

    prune(local_root, &seen, report);
    Ok(())
}

pub(crate) fn write_entry(
    config: &Config,
    dir: &Path,
    stem: &str,
    pick_code: &str,
    size: i64,
    seen: &mut HashSet<PathBuf>,
    report: &mut Report,
) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("建目录失败：{e}"))?;
    let strm = dir.join(format!("{stem}.strm"));
    let content = format!(
        "{}/api/module/{MODULE_ID}/play?pc={}\n",
        config.base_url,
        pick_code.trim().to_ascii_lowercase()
    );
    if std::fs::read(&strm).ok().as_deref() == Some(content.as_bytes()) {
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
fn prune(local_root: &Path, seen: &HashSet<PathBuf>, report: &mut Report) {
    let marker = play_marker();
    let mut stack = vec![local_root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if is_dir {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("strm") {
                continue;
            }
            if seen.contains(&path) {
                continue;
            }
            let ours = std::fs::read_to_string(&path)
                .map(|text| text.contains(&marker))
                .unwrap_or(false);
            if !ours {
                continue;
            }
            if std::fs::remove_file(&path).is_ok() {
                let _ = std::fs::remove_file(path.with_extension("info.json"));
                report.pruned += 1;
            }
        }
    }
}

fn sidecar_path(dir: &Path, stem: &str) -> PathBuf {
    dir.join(format!("{stem}.info.json"))
}

/// Whether this entry still needs its container read: a sidecar with no
/// duration is one the mirror wrote before it could describe the file (or an
/// earlier probe that failed).
fn needs_probe(dir: &Path, stem: &str) -> bool {
    let Ok(bytes) = std::fs::read(sidecar_path(dir, stem)) else {
        return true;
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return true;
    };
    value
        .get("duration_ms")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        == 0
}

/// Replace the size-only sidecar with one the scanner can describe the entry
/// from: the wire shapes KROMA's own probe pass writes.
fn write_sidecar(
    dir: &Path,
    stem: &str,
    size: i64,
    described: &crate::probe::Probed,
) -> Result<(), String> {
    let mut body = serde_json::json!({ "size": size.max(0) });
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
    std::fs::write(sidecar_path(dir, stem), body.to_string()).map_err(|e| format!("写 .info.json 失败：{e}"))
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
    if target.exists() {
        return Ok(false);
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("建目录失败（{}）：{e}", dir.display()))?;
    let link = client::sign(&config.cookie, client::MODULE_UA, pick_code)
        .await
        .map_err(|e| e.to_string())?;
    // The cookie travels with the download: a link the CDN wants a cookie for
    // still delivers, unlike playback where a player cannot send one.
    let response = Fetch::default()
        .purpose("cloud-drive")
        .header("User-Agent", client::MODULE_UA)
        .header("Cookie", config.cookie.as_str())
        .max_time(120)
        .download(&link.url, &target)
        .map_err(|e| e.to_string())?;
    if !(200..300).contains(&response.status) {
        let _ = std::fs::remove_file(&target);
        return Err(format!("下载失败 HTTP {}", response.status));
    }
    Ok(true)
}

pub(crate) fn is_video_ext(ext: &str) -> bool {
    matches!(
        ext,
        "mkv" | "mp4" | "ts" | "m2ts" | "iso" | "avi" | "mov" | "wmv" | "flv" | "webm" | "rmvb"
            | "mpg" | "mpeg" | "m4v" | "rm"
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
    fn a_name_is_made_safe_for_every_filesystem() {
        assert_eq!(safe_name("A/B:C*D?"), "A_B_C_D_");
        assert_eq!(safe_name("  ..  "), "_");
        assert_eq!(safe_name(""), "_");
        assert_eq!(safe_name("Movie. "), "Movie");
        let long = "x".repeat(400);
        assert_eq!(safe_name(&long).chars().count(), 150);
    }

    #[test]
    fn a_mapping_cannot_escape_the_library() {
        assert!(clean_relative("../etc").is_none());
        assert!(clean_relative("a/../../b").is_none());
        assert_eq!(
            clean_relative("115/电影/").as_deref(),
            Some(Path::new("115/电影"))
        );
        assert!(clean_relative("   ").is_none());
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
        prune(&root, &HashSet::new(), &mut report);

        assert!(!ours.exists(), "our own stale entry is removed");
        assert!(!root.join("ours.info.json").exists(), "its sidecar goes too");
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
        let mut seen = HashSet::new();
        seen.insert(kept.clone());

        let mut report = Report::default();
        prune(&root, &seen, &mut report);

        assert!(kept.exists());
        assert_eq!(report.pruned, 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_local_mapping_is_a_storage_path_and_never_a_library_name() {
        let folders = vec![LibraryFolders {
            id: "1".into(),
            kind: "movies".into(),
            name: "media".into(),
            folders: vec!["/media".into()],
        }];
        let absolute = Library {
            remote: "/媒体库".into(),
            local: "/media/115/电影".into(),
        };
        assert_eq!(
            resolve_local_path(&folders, &absolute.local).as_deref(),
            Some(Path::new("/media/115/电影"))
        );
        let legacy = Library {
            remote: "/媒体库".into(),
            local: "115/电影".into(),
        };
        assert_eq!(
            resolve_local_path(&folders, &legacy.local).as_deref(),
            Some(Path::new("/media/115/电影"))
        );
        assert!(resolve_local_path(&folders, "../../etc").is_none());
    }
}
