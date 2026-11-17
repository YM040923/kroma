//! The near-real-time half of the mirror: turn 115's life events into `.strm`
//! files within about a minute, then let the scanner pick them up.
//!
//! A file event that carries a `pick_code` is written straight to its folder —
//! no listing at all. Everything else (a new folder, a delete, an event without
//! a code) re-reads just the affected folder, using the folder map the full sync
//! left behind. The full sync stays the backstop: it repairs anything a lost
//! event window skipped and prunes what an event only half described.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use kroma_module_sdk::host::HostCtx;

use crate::client;
use crate::events::{self, Event};
use crate::settings::Config;
use crate::store;
use crate::sync::{self, Report};

/// Folders one round may re-read. A burst (a whole season dropped in at once)
/// is spread over rounds rather than turning into one long walk.
const MAX_RESCAN_DIRS: u32 = 40;

static RUNNING: AtomicBool = AtomicBool::new(false);

/// Start a round if one is not already running and the interval has elapsed.
/// Called from the every-minute job; returns immediately.
pub fn start<S: HostCtx + Clone + Send + Sync + 'static>(host: S) {
    if RUNNING.swap(true, Ordering::AcqRel) {
        return;
    }
    let config = Config::load();
    if !config.runnable() || !config.options.watch_enabled {
        RUNNING.store(false, Ordering::Release);
        return;
    }
    let incr = store::load().incr;
    let now = events::unix_now();
    if incr.last_poll_at > 0 && now - incr.last_poll_at < config.options.watch_interval_secs as i64 {
        RUNNING.store(false, Ordering::Release);
        return;
    }

    tokio::spawn(async move {
        run_once(&host, &config).await;
        RUNNING.store(false, Ordering::Release);
    });
}

/// One poll + apply pass. Never panics and never returns an error: a failed
/// round simply leaves the cursor where it was and the next one retries.
pub async fn run_once<S: HostCtx>(host: &S, config: &Config) {
    let stored = store::load();
    let mut incr = stored.incr.clone();

    if incr.gate_checked_at == 0 {
        let ok = match events::enable_life(&config.cookie).await {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(%error, "115 生活事件开关没打开，事件流会一直是空的");
                false
            }
        };
        incr.gate_ok = Some(ok);
        incr.gate_checked_at = events::unix_now();
    }

    let poll = match events::poll(&config.cookie, &incr, 500).await {
        Ok(poll) => poll,
        Err(error) => {
            tracing::warn!(%error, "生活事件拉取失败，本轮跳过");
            incr.last_poll_at = events::unix_now();
            let _ = store::patch(|saved| saved.incr = incr);
            return;
        }
    };

    let mut report = Report::default();
    let mut dirs = stored.dirs.clone();
    let mut budget = MAX_RESCAN_DIRS;
    let mut seen_dirs = HashSet::new();

    // Oldest first: a rename followed by a move must end at the final path.
    let mut events = poll.events.clone();
    events.sort_by_key(|event| event.time);
    for event in &events {
        // A folder that appeared, was copied, or moved is anchored by its
        // parent's path — its own id is not in the map yet. Index it first, then
        // read what it holds; a following file event in the same round then
        // finds the folder through the same map.
        if event.is_dir && event.adds() {
            if let Some(parent_dir) = dirs.get(&event.parent_cid).cloned() {
                let dir = Path::new(&parent_dir).join(sync::safe_name(&event.name));
                if !event.file_id.is_empty() {
                    dirs.insert(event.file_id.clone(), dir.to_string_lossy().into_owned());
                    if budget > 0 && seen_dirs.insert(event.file_id.clone()) {
                        rescan(config, &event.file_id, &dir, 2, &mut budget, &mut dirs, &mut report)
                            .await;
                    }
                }
            }
            continue;
        }
        if let Some(done) = try_direct(config, event, &dirs, &mut report) {
            if done {
                continue;
            }
        }
        let Some((cid, depth)) = target_of(event) else {
            continue;
        };
        let Some(dir) = dirs.get(&cid).cloned() else {
            // Outside every mapping, or a folder the full sync has not indexed
            // yet. The full sync will pick it up.
            continue;
        };
        if !seen_dirs.insert(cid.clone()) {
            continue;
        }
        if budget == 0 {
            break;
        }
        rescan(config, &cid, Path::new(&dir), depth, &mut budget, &mut dirs, &mut report).await;
    }

    incr.cursor_id = poll.cursor.id;
    incr.cursor_time = poll.cursor.time;
    incr.proapi_405 = poll.proapi_405;
    incr.web_until = poll.web_until;
    incr.last_poll_at = events::unix_now();
    incr.last_events = events.len() as u64;
    let _ = store::patch(|saved| {
        saved.dirs = dirs;
        saved.incr = incr;
    });

    if !report.errors.is_empty() {
        tracing::warn!(
            errors = report.errors.len(),
            first = report.errors.first().map(String::as_str).unwrap_or(""),
            "准实时同步本轮有错误"
        );
    }
    if report.written > 0 || report.pruned > 0 {
        tracing::info!(
            events = events.len(),
            written = report.written,
            pruned = report.pruned,
            "准实时同步：已写入新条目"
        );
        host.trigger_job("library.scan", "cloud115 watch");
    }
}

/// A file event that names its own folder and carries a download code needs no
/// listing: write the `.strm` straight into the mapped folder. Answers
/// `Some(true)` when it was handled, `Some(false)`/`None` to fall through.
fn try_direct(
    config: &Config,
    event: &Event,
    dirs: &BTreeMap<String, String>,
    report: &mut Report,
) -> Option<bool> {
    // Only events that *create* a path this module has never written are safe to
    // write straight. A move or a rename would leave the old `.strm` behind, so
    // those fall through to the folder re-read, which prunes the old name.
    if !matches!(event.kind, 2 | 14 | 23) || event.is_dir || event.pick_code.is_empty() {
        return None;
    }
    let ext = Path::new(&event.name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !sync::is_video_ext(&ext) || (ext == "iso" && config.options.skip_iso) {
        return None;
    }
    let dir = dirs.get(&event.parent_cid)?;
    let dir = Path::new(dir);
    let stem = sync::safe_name(sync::strip_extension(&event.name));
    match sync::write_entry(
        config,
        dir,
        &stem,
        &event.pick_code,
        event.size,
        &mut HashSet::new(),
        report,
    ) {
        Ok(()) => Some(true),
        Err(error) => {
            report.errors.push(format!("{}: {error}", dir.display()));
            Some(false)
        }
    }
}

/// Which folder an event makes stale, and how deep to re-read it.
fn target_of(event: &Event) -> Option<(String, u8)> {
    if event.deletes() {
        return non_root(&event.parent_cid).map(|cid| (cid, 1));
    }
    if !event.adds() || event.is_dir {
        // Folder events are anchored through their parent above.
        return None;
    }
    non_root(&event.parent_cid).map(|cid| (cid, 1))
}

fn non_root(cid: &str) -> Option<String> {
    (!cid.is_empty() && cid != "0").then(|| cid.to_string())
}

/// Re-read one folder (and, for a new folder, its children), write the videos
/// it holds, and prune the `.strm` files in it the drive no longer has.
async fn rescan(
    config: &Config,
    root_cid: &str,
    root_dir: &Path,
    depth: u8,
    budget: &mut u32,
    dirs: &mut BTreeMap<String, String>,
    report: &mut Report,
) {
    let mut stack = vec![(root_cid.to_string(), root_dir.to_path_buf(), depth)];
    while let Some((cid, dir, depth)) = stack.pop() {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        dirs.insert(cid.clone(), dir.to_string_lossy().into_owned());

        let page_size = config.options.page_size;
        let mut offset = 0u32;
        let mut entries = Vec::new();
        let mut total = 0i64;
        let mut complete = true;
        loop {
            match client::list(&config.cookie, client::MODULE_UA, &cid, offset, page_size).await {
                Ok(page) => {
                    total = page.total;
                    let read = page.entries.len();
                    entries.extend(page.entries);
                    offset += read as u32;
                    if read < page_size as usize || (total > 0 && entries.len() as i64 >= total) {
                        break;
                    }
                }
                Err(error) => {
                    report
                        .errors
                        .push(format!("{}: 列目录失败 {error}", dir.display()));
                    complete = false;
                    break;
                }
            }
        }
        if !complete || (total > 0 && (entries.len() as i64) < total) {
            // Incomplete listing: keep what was read, never prune from it.
            report.errors.push(format!(
                "{} 只读到 {}/{} 项，本轮不删除",
                dir.display(),
                entries.len(),
                total
            ));
            continue;
        }

        let mut seen = HashSet::new();
        let mut subfolders = Vec::new();
        let mut assets_left = 10u32;
        for entry in &entries {
            if entry.is_dir() {
                if let Some(id) = entry.folder_id() {
                    subfolders.push((id, sync::safe_name(&entry.name)));
                }
                continue;
            }
            let ext = Path::new(&entry.name)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if !sync::is_video_ext(&ext) {
                if sync::is_asset_ext(&ext) && assets_left > 0 {
                    match sync::download_asset(config, &entry.pick_code, &entry.name, entry.size, &dir).await {
                        Ok(true) => {
                            report.assets += 1;
                            assets_left -= 1;
                        }
                        Ok(false) => {}
                        Err(error) => report.errors.push(format!("{}: {error}", dir.display())),
                    }
                }
                continue;
            }
            if ext == "iso" && config.options.skip_iso {
                continue;
            }
            if entry.pick_code.is_empty() {
                continue;
            }
            let stem = sync::safe_name(sync::strip_extension(&entry.name));
            if let Err(error) = sync::write_entry(
                config,
                &dir,
                &stem,
                &entry.pick_code,
                entry.size,
                &mut seen,
                report,
            ) {
                report.errors.push(format!("{}: {error}", dir.display()));
            }
        }
        prune_folder(&dir, &seen, report);

        if depth > 1 {
            for (id, name) in subfolders {
                stack.push((id, dir.join(name), depth - 1));
            }
        }
    }
}

/// Remove the `.strm` files this module wrote in `dir` that the listing no
/// longer names. Only files naming this module's play route are touched.
fn prune_folder(dir: &Path, seen: &HashSet<PathBuf>, report: &mut Report) {
    let marker = format!("/api/module/{}/play?pc=", crate::MODULE_ID);
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("strm") || seen.contains(&path) {
            continue;
        }
        let ours = std::fs::read_to_string(&path)
            .map(|text| text.contains(&marker))
            .unwrap_or(false);
        if ours && std::fs::remove_file(&path).is_ok() {
            let _ = std::fs::remove_file(path.with_extension("info.json"));
            report.pruned += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(kind: u16, parent: &str, file: &str, is_dir: bool) -> Event {
        Event {
            kind,
            parent_cid: parent.into(),
            file_id: file.into(),
            is_dir,
            ..Event::default()
        }
    }

    #[test]
    fn a_file_event_targets_its_own_folder_shallowly() {
        assert_eq!(
            target_of(&event(14, "15", "99", false)),
            Some(("15".into(), 1))
        );
    }

    #[test]
    fn a_folder_event_is_anchored_through_its_parent_not_its_own_id() {
        // Handled by the folder branch in `run_once`, never as a plain target.
        assert_eq!(target_of(&event(17, "15", "77", true)), None);
        assert_eq!(target_of(&event(6, "15", "77", true)), None);
    }

    #[test]
    fn a_delete_targets_the_folder_it_left() {
        assert_eq!(
            target_of(&event(22, "15", "99", false)),
            Some(("15".into(), 1))
        );
    }

    #[test]
    fn a_root_parent_is_never_a_target() {
        assert_eq!(target_of(&event(14, "0", "99", false)), None);
    }
}
