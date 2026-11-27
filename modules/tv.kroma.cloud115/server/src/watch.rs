//! Read-only recursive video discovery. Full sync owns reconciliation and pruning.
use crate::{
    client,
    events::unix_now,
    settings::{Config, Library},
    sync,
};
use kroma_module_sdk::host::HostCtx;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    io::{self, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

// Small mappings are compared in one or two recursive pages. The common
// steady-state check must not list every season to discover one new file.
const PAGE_CAP: u32 = 1150;
const LOOKUPS_PER_MAPPING: u32 = 12;
type Result<T> = std::result::Result<T, &'static str>;

#[derive(Default, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MappingStatus {
    pub remote: String,
    pub state: String,
    pub last_checked_at: i64,
    pub last_added: u64,
    pub next_offset: u32,
    pub total: u32,
}
#[derive(Default, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FastStatus {
    pub last_poll_at: i64,
    pub last_added: u64,
    pub indexed_dirs: usize,
    pub last_error: String,
    pub mappings: Vec<MappingStatus>,
}
#[derive(Default, Serialize, Deserialize)]
struct Mapping {
    local: String,
    root: String,
    status: MappingStatus,
    completed: bool,
    #[serde(default)]
    audit_index: u32,
    // These are observations, never authority to write a path on another tick.
    dirs: BTreeMap<String, PathBuf>,
    files: BTreeMap<String, Video>,
}
#[derive(Serialize, Deserialize, PartialEq)]
struct Video {
    parent: String,
    name: String,
    pc: String,
    size: i64,
    relative: PathBuf,
    stem: String,
}
#[derive(Default, Serialize, Deserialize)]
struct Checkpoint {
    auth_id: String,
    last_poll_at: i64,
    last_error: String,
    turn: usize,
    mappings: Vec<Mapping>,
}
fn path() -> PathBuf {
    crate::store::path().with_file_name("fast-discovery.json")
}
fn load() -> io::Result<Checkpoint> {
    match fs::File::open(path()) {
        Ok(f) => Ok(serde_json::from_reader(BufReader::new(f))?),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Checkpoint::default()),
        Err(e) => Err(e),
    }
}
fn save(cp: &Checkpoint) -> io::Result<()> {
    let path = path();
    let tmp = path.with_extension("tmp");
    fs::create_dir_all(path.parent().unwrap())?;
    let mut w = BufWriter::new(fs::File::create(&tmp)?);
    serde_json::to_writer(&mut w, cp)?;
    w.flush()?;
    drop(w);
    fs::rename(tmp, path)
}
impl Checkpoint {
    fn reconcile(&mut self, cfg: &Config) {
        if self.auth_id != cfg.auth_id {
            *self = Self {
                auth_id: cfg.auth_id.clone(),
                ..Default::default()
            };
        }
        let mut old = std::mem::take(&mut self.mappings);
        for lib in cfg
            .libraries
            .iter()
            .filter(|l| l.fast_sync && !l.remote.trim().is_empty() && !l.local.trim().is_empty())
        {
            if self.mappings.iter().any(|m| matches_lib(m, lib)) {
                continue;
            }
            self.mappings.push(
                old.iter()
                    .position(|m| matches_lib(m, lib))
                    .map(|i| old.swap_remove(i))
                    .unwrap_or_else(|| Mapping {
                        local: lib.local.clone(),
                        status: MappingStatus {
                            remote: lib.remote.clone(),
                            state: "pending".into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
            );
        }
    }
}
fn matches_lib(m: &Mapping, l: &Library) -> bool {
    m.local == l.local && m.status.remote == l.remote
}

fn indexed_siblings(cp: &Checkpoint, cfg: &Config, pc: &str, limit: usize) -> Vec<String> {
    if limit == 0 || cfg.auth_id.is_empty() || cp.auth_id != cfg.auth_id {
        return Vec::new();
    }
    for m in &cp.mappings {
        if !cfg
            .libraries
            .iter()
            .any(|l| l.fast_sync && matches_lib(m, l))
        {
            continue;
        }
        let Some(current) = m.files.values().find(|v| v.pc.eq_ignore_ascii_case(pc)) else {
            continue;
        };
        let siblings = m
            .files
            .values()
            .filter(|v| v.parent == current.parent && v.relative == current.relative)
            .map(|v| (v.name.as_str(), v.pc.as_str()));
        return sync::following_in_folder(siblings, limit)
            .remove(current.pc.as_str())
            .unwrap_or_default();
    }
    Vec::new()
}

pub(crate) fn following(cfg: &Config, pc: &str, limit: usize) -> Vec<String> {
    load()
        .map(|cp| indexed_siblings(&cp, cfg, pc, limit))
        .unwrap_or_default()
}
pub fn snapshot() -> FastStatus {
    let Ok(mut cp) = load() else {
        return FastStatus {
            last_error: "快速检查记录无法读取；可以重置检查点后重新比对，已生成的 STRM 不受影响。"
                .into(),
            ..Default::default()
        };
    };
    cp.reconcile(&Config::load());
    FastStatus {
        last_poll_at: cp.last_poll_at,
        last_added: cp.mappings.iter().map(|m| m.status.last_added).sum(),
        indexed_dirs: cp.mappings.iter().map(|m| m.dirs.len()).sum(),
        last_error: cp.last_error,
        mappings: cp.mappings.into_iter().map(|m| m.status).collect(),
    }
}
// The two WASI instances share this host-session mount; only one quick poll runs.
fn marker() -> PathBuf {
    #[cfg(target_family = "wasm")]
    let dir = PathBuf::from("/tmp");
    #[cfg(not(target_family = "wasm"))]
    let dir = std::env::temp_dir();
    dir.join("cloud115-fast.run")
}
struct Guard(PathBuf);
impl Drop for Guard {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.0);
    }
}
pub fn is_running() -> bool {
    marker().is_dir()
}
/// Reset discovery metadata only. Use the worker's own lock so a running tick
/// cannot resurrect the cleared cursor. Never touch playback or STRM files.
pub fn clear_checkpoint() -> std::result::Result<(), String> {
    let lock = marker();
    fs::create_dir(&lock).map_err(|_| "快速检查正在进行，请完成后再清除检查点".to_owned())?;
    let _guard = Guard(lock);
    let mut cp = Checkpoint::default();
    cp.reconcile(&Config::load());
    save(&cp).map_err(|_| "未能重置快速检查记录".to_owned())
}
pub fn start<S: HostCtx + Clone + Send + Sync + 'static>(host: S) {
    start_inner(host, false);
}
pub fn start_now<S: HostCtx + Clone + Send + Sync + 'static>(host: S) {
    start_inner(host, true);
}
fn start_inner<S: HostCtx + Clone + Send + Sync + 'static>(host: S, force: bool) {
    let lock = marker();
    if fs::create_dir(&lock).is_err() {
        return;
    }
    let guard = Guard(lock);
    // The full worker and discovery share per-entry write/prune locks. Do not
    // stop checking for new releases while the full worker enriches an old library.
    let cfg = Config::load();
    if !cfg.runnable() || (!force && !cfg.options.watch_enabled) {
        return;
    }
    let Ok(mut cp) = load() else {
        tracing::warn!("fast checkpoint unreadable");
        return;
    };
    cp.reconcile(&cfg);
    let now = unix_now();
    if cp.mappings.is_empty()
        || (!force
            && now >= cp.last_poll_at
            && now - cp.last_poll_at < cfg.options.watch_interval_secs as i64)
    {
        return;
    }
    tokio::spawn(async move {
        let _guard = guard;
        crate::pacer::pacer()
            .with_run(&cfg.options, async {
                let mut budget = Budget {
                    left: cfg.options.max_calls_per_run,
                    background: !force,
                    daily: cfg.options.daily_call_budget,
                };
                cp.last_poll_at = now;
                cp.last_error.clear();
                for m in &mut cp.mappings {
                    m.status.last_added = 0;
                }
                let count = cp.mappings.len();
                let first = cp.turn % count;
                let mut written = 0;
                for n in 0..count {
                    let i = (first + n) % count;
                    let m = &mut cp.mappings[i];
                    let mut report = sync::Report::default();
                    let result = poll(&cfg, m, &mut budget, &mut report).await;
                    m.status.last_checked_at = unix_now();
                    m.status.last_added = report.written;
                    written += report.written;
                    m.status.state = match result {
                        Ok(()) if m.completed => "checked",
                        Ok(()) => "baselining",
                        Err(e) => {
                            cp.last_error = format!("{}：{}", m.status.remote, diagnostic(e));
                            if e == "rate limited" {
                                "rate limited"
                            } else {
                                "error"
                            }
                        }
                    }
                    .into();
                    cp.turn = if n + 1 == count {
                        (first + 1) % count
                    } else {
                        (i + 1) % count
                    };
                    if save(&cp).is_err() {
                        tracing::warn!("fast checkpoint write failed");
                        break;
                    }
                    if matches!(result, Err("rate limited" | "budget" | "authorization"))
                        || budget.left == 0
                    {
                        break;
                    }
                }
                if written > 0 {
                    host.trigger_job("library.scan", "cloud115 watch");
                }
            })
            .await;
    });
}
struct Budget {
    left: u32,
    background: bool,
    daily: u32,
}
impl Budget {
    fn take(&mut self) -> Result<()> {
        if self.left == 0
            || (self.background && self.daily > 0 && crate::budget::background_blocked(self.daily))
        {
            return Err("budget");
        }
        self.left -= 1;
        Ok(())
    }
}
fn diagnostic(e: &str) -> &str {
    match e {
        "rate limited" => "115 限流，稍后再检查",
        "budget" => "达到 KROMA 调用预算，下次继续",
        "authorization" => "115 授权无效，请重新扫码",
        "mapping or account changed" | "mapping path changed" => "映射或授权已更改，下次将重新比对",
        "parent outside mapping" | "invalid ancestry" | "incomplete ancestry" => {
            "未能确认文件仍在映射目录内，已跳过"
        }
        "incomplete video listing" | "invalid listing count" => "视频清单不完整，已保留现有文件",
        "invalid output grant" => "本地文件夹未授权",
        "output write failed" | "output path inaccessible" => "无法写入本地文件夹",
        "drive request failed" => "读取 115 开放平台文件列表失败",
        "unsafe name" | "linked output path" => "目标文件名或路径不可用",
        "invalid pick code" | "missing video identity" => "115 未返回有效的文件标识",
        _ => "快速检查未完成",
    }
}
fn api(e: client::ApiError) -> &'static str {
    if e.rate_limited() {
        "rate limited"
    } else if e.message.contains("调用上限") {
        "budget"
    } else if e.auth_expired() || e.message.contains("授权") {
        "authorization"
    } else {
        "drive request failed"
    }
}
fn component(name: &str) -> Result<String> {
    if name.trim_end_matches([' ', '.']).is_empty() || name.contains(['/', '\\']) {
        return Err("unsafe name");
    }
    let clean = sync::safe_name(name);
    let device = name
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end()
        .to_ascii_uppercase();
    if matches!(
        device.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].iter().any(|p| {
        device.strip_prefix(*p).is_some_and(|s| {
            s.len() == 1 && s.as_bytes()[0].is_ascii_digit() || matches!(s, "¹" | "²" | "³")
        })
    }) {
        return Err("unsafe name");
    }
    Ok(clean)
}
fn relative(root: &str, parent: &str, chain: &[(String, String)]) -> Result<PathBuf> {
    if chain.last().map(|p| p.0.as_str()) != Some(parent) {
        return Err("incomplete ancestry");
    }
    let mut ids = HashSet::new();
    if chain.iter().any(|(id, _)| id.is_empty() || !ids.insert(id)) {
        return Err("invalid ancestry");
    }
    let anchor = chain
        .iter()
        .position(|p| p.0 == root)
        .ok_or("parent outside mapping")?;
    let mut rel = PathBuf::new();
    for (_, name) in &chain[anchor + 1..] {
        rel.push(component(name)?);
    }
    Ok(rel)
}
fn no_link(p: &Path) -> Result<()> {
    match fs::symlink_metadata(p) {
        Ok(m) if m.is_symlink() => Err("linked output path"),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("output path inaccessible"),
    }
}
fn write(cfg: &Config, local: &Path, v: &Video, report: &mut sync::Report) -> Result<()> {
    let mut dir = local.to_owned();
    no_link(&dir)?;
    for part in v.relative.components() {
        dir.push(part);
        no_link(&dir)?;
    }
    for suffix in ["strm", "info.json"] {
        no_link(&dir.join(format!("{}.{suffix}", v.stem)))?;
    }
    sync::write_entry(
        cfg,
        &dir,
        &v.stem,
        &v.pc,
        v.size,
        &[],
        cfg.options.overwrite_strm,
        &mut HashSet::new(),
        report,
    )
    .map_err(|_| "output write failed")
}
fn authorize(cfg: &Config, m: &Mapping, current: &Config) -> Result<()> {
    if current.auth_id != cfg.auth_id
        || !current
            .libraries
            .iter()
            .any(|l| l.fast_sync && matches_lib(m, l))
    {
        Err("mapping or account changed")
    } else {
        Ok(())
    }
}
trait Listing {
    async fn root(&self, remote: &str) -> Result<String>;
    async fn page(&self, root: &str, offset: u32, limit: u32) -> Result<client::Page>;
    async fn chain(&self, parent: &str) -> Result<Vec<(String, String)>>;
    fn current(&self) -> Config;
}
struct Drive<'a>(&'a str);
impl Listing for Drive<'_> {
    async fn root(&self, remote: &str) -> Result<String> {
        client::folder_id_by_path(self.0, client::MODULE_UA, remote)
            .await
            .map_err(api)
    }
    async fn page(&self, root: &str, offset: u32, limit: u32) -> Result<client::Page> {
        client::list_videos(self.0, root, offset, limit)
            .await
            .map_err(api)
    }
    async fn chain(&self, parent: &str) -> Result<Vec<(String, String)>> {
        client::directory_path(self.0, parent).await.map_err(api)
    }
    fn current(&self) -> Config {
        Config::load()
    }
}
async fn poll(
    cfg: &Config,
    m: &mut Mapping,
    budget: &mut Budget,
    report: &mut sync::Report,
) -> Result<()> {
    let local = sync::resolve_local_path(&[], &m.local).ok_or("invalid output grant")?;
    discover(&Drive(&cfg.auth_id), cfg, m, &local, budget, report).await
}
async fn discover(
    drive: &impl Listing,
    cfg: &Config,
    m: &mut Mapping,
    local: &Path,
    budget: &mut Budget,
    report: &mut sync::Report,
) -> Result<()> {
    let page_size = cfg.options.page_size.clamp(1, PAGE_CAP);
    // Resolve a saved path on its first check. The listing's own path (or a
    // parent chain before writing) proves that a cached CID still names it.
    if m.root.is_empty() {
        budget.take()?;
        m.root = drive.root(&m.status.remote).await?;
        if m.root.is_empty() {
            return Err("missing root id");
        }
    }
    budget.take()?;
    let head = match drive.page(&m.root, 0, page_size).await {
        Ok(page) => page,
        Err(e) => {
            if e != "rate limited" {
                m.root.clear();
            }
            return Err(e);
        }
    };
    if !head.path.is_empty()
        && !head
            .path
            .iter()
            .filter(|(id, _)| id != "0")
            .map(|(_, s)| s.as_str())
            .eq(m.status.remote.split('/').filter(|s| !s.is_empty()))
    {
        m.root.clear();
        m.dirs.clear();
        m.files.clear();
        m.status.next_offset = 0;
        m.completed = false;
        return Err("mapping path changed");
    }
    let total = valid_page(&head, 0, page_size)?;
    m.status.total = total;
    let mut fresh = HashMap::new();
    // Compare the listing locally before spending a request on a parent. A
    // rotating audit catches directory-only renames; all NEW writes still
    // require fresh ancestry and a live host grant.
    let audit = if m.dirs.is_empty() {
        None
    } else {
        m.dirs
            .keys()
            .nth(m.audit_index as usize % m.dirs.len())
            .cloned()
    };
    m.audit_index = m.audit_index.wrapping_add(1);
    let mut lookups = LOOKUPS_PER_MAPPING;
    let head_result = apply(
        drive,
        cfg,
        m,
        local,
        &head.entries,
        &mut fresh,
        &mut lookups,
        audit.as_deref(),
        budget,
        report,
    )
    .await;
    if matches!(
        head_result,
        Err("rate limited" | "budget" | "authorization" | "mapping or account changed")
    ) {
        return head_result;
    }
    if total <= page_size {
        if head_result == Err("baselining") {
            m.completed = false;
            return Ok(());
        }
        head_result?;
        let live: HashSet<_> = head.entries.iter().filter_map(|e| e.file_id()).collect();
        m.files.retain(|id, _| live.contains(id));
        m.status.next_offset = 0;
        m.completed = true;
        return Ok(());
    }
    let offset = if (page_size..total).contains(&m.status.next_offset) {
        m.status.next_offset
    } else {
        page_size
    };
    m.status.next_offset = offset;
    budget.take()?;
    let page = drive.page(&m.root, offset, page_size).await?;
    let total = valid_page(&page, offset, page_size)?;
    let applied = apply(
        drive,
        cfg,
        m,
        local,
        &page.entries,
        &mut fresh,
        &mut lookups,
        audit.as_deref(),
        budget,
        report,
    )
    .await;
    if applied == Err("baselining") {
        m.completed = false;
        return if head_result == Err("baselining") {
            Ok(())
        } else {
            head_result
        };
    }
    if matches!(
        applied,
        Err("rate limited" | "budget" | "authorization" | "mapping or account changed")
    ) {
        return applied;
    }
    // A permanent error in one row must not strand every older video. Only
    // validated listing pages advance; failed rows have no file checkpoint and
    // are reconsidered when the cursor wraps.

    let next = offset.saturating_add(page.entries.len() as u32);
    m.status.total = total;
    if next >= total {
        m.completed = true;
        m.status.next_offset = page_size;
    } else {
        m.status.next_offset = next;
    }
    if head_result == Err("baselining") {
        m.completed = false;
        return applied;
    }
    head_result.and(applied)
}
fn valid_page(page: &client::Page, offset: u32, limit: u32) -> Result<u32> {
    let total = u32::try_from(page.total).map_err(|_| "invalid listing count")?;
    if page.entries.len() != limit.min(total.saturating_sub(offset)) as usize {
        return Err("incomplete video listing");
    }
    Ok(total)
}
async fn apply(
    drive: &impl Listing,
    cfg: &Config,
    m: &mut Mapping,
    local: &Path,
    entries: &[client::Entry],
    fresh: &mut HashMap<String, PathBuf>,
    lookups: &mut u32,
    audit: Option<&str>,
    budget: &mut Budget,
    report: &mut sync::Report,
) -> Result<()> {
    let mut failure = None;
    for e in entries {
        let stem = match stem(cfg, &e.name, e.size) {
            Ok(Some(s)) => s,
            Ok(None) => continue,
            Err(e) => {
                failure = Some(e);
                continue;
            }
        };
        let (Some(id), Some(parent)) = (e.file_id(), e.parent_id()) else {
            failure = Some("missing video identity");
            continue;
        };
        if e.pick_code.is_empty()
            || !e
                .pick_code
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        {
            failure = Some("invalid pick code");
            continue;
        }
        if audit != Some(parent.as_str()) && !fresh.contains_key(&parent) {
            if let Some(prev) = m.files.get(&id) {
                if prev.parent == parent
                    && prev.name == e.name
                    && prev.pc == e.pick_code
                    && prev.size == e.size
                    && prev.stem == stem
                    && local
                        .join(&prev.relative)
                        .join(format!("{stem}.strm"))
                        .is_file()
                {
                    continue;
                }
            }
        }
        if !fresh.contains_key(&parent) {
            if *lookups == 0 {
                failure.get_or_insert("baselining");
                continue;
            }
            *lookups -= 1;
            budget.take()?;
            let chain = match drive.chain(&parent).await {
                Ok(chain) => chain,
                Err(e @ ("rate limited" | "budget" | "authorization")) => return Err(e),
                Err(e) => {
                    failure = Some(e);
                    continue;
                }
            };
            // A mapping root itself may have been renamed or reparented since
            // root resolution. Check its full remote name as well as its id.
            let names = chain
                .iter()
                .take_while(|(id, _)| id != &m.root)
                .filter(|(id, _)| id != "0")
                .map(|(_, name)| name.as_str());
            let anchor = chain
                .iter()
                .find(|(id, _)| id == &m.root && id != "0")
                .map(|(_, name)| name.as_str());
            if !names
                .chain(anchor)
                .eq(m.status.remote.split('/').filter(|s| !s.is_empty()))
            {
                failure = Some("parent outside mapping");
                continue;
            }
            let rel = match relative(&m.root, &parent, &chain) {
                Ok(r) => r,
                Err(e) => {
                    failure = Some(e);
                    continue;
                }
            };
            m.dirs.insert(parent.clone(), rel.clone());
            fresh.insert(parent.clone(), rel);
        }
        let v = Video {
            relative: fresh[&parent].clone(),
            parent,
            name: e.name.clone(),
            pc: e.pick_code.clone(),
            size: e.size,
            stem,
        };
        // Full sync may have removed an old path while this file was away.
        // A matching checkpoint alone must not suppress a moved-back video.
        if m.files.get(&id) == Some(&v)
            && local
                .join(&v.relative)
                .join(format!("{}.strm", v.stem))
                .is_file()
        {
            continue;
        }
        authorize(cfg, m, &drive.current())?;
        if let Err(e) = write(cfg, local, &v, report) {
            failure = Some(e);
            continue;
        }
        m.files.insert(id, v);
    }
    failure.map_or(Ok(()), Err)
}
fn stem(cfg: &Config, name: &str, size: i64) -> Result<Option<String>> {
    let ext = Path::new(name)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !sync::is_video_ext(&ext)
        || (cfg.options.skip_iso && ext == "iso")
        || (size.max(0) as u64) < cfg.options.min_video_size_mb.saturating_mul(1024 * 1024)
    {
        return Ok(None);
    }
    component(name)?;
    component(if cfg.options.keep_suffix {
        name
    } else {
        sync::strip_extension(name)
    })
    .map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    const HEAD: u32 = 8;
    const SWEEP: u32 = HEAD;
    struct Fake {
        cfg: Config,
        entries: Vec<client::Entry>,
        folder: String,
        anchor: String,
        fail: Option<u32>,
    }
    impl Listing for Fake {
        async fn root(&self, _: &str) -> Result<String> {
            Ok("1".into())
        }
        async fn page(&self, _: &str, offset: u32, limit: u32) -> Result<client::Page> {
            if self.fail == Some(offset) {
                return Err("drive request failed");
            }
            Ok(client::Page {
                entries: self
                    .entries
                    .iter()
                    .skip(offset as usize)
                    .take(limit as usize)
                    .cloned()
                    .collect(),
                total: self.entries.len() as i64,
                ..Default::default()
            })
        }
        async fn chain(&self, parent: &str) -> Result<Vec<(String, String)>> {
            Ok(vec![
                (self.anchor.clone(), "Library".into()),
                (parent.into(), self.folder.clone()),
            ])
        }
        fn current(&self) -> Config {
            self.cfg.clone()
        }
    }
    struct Output(PathBuf);
    impl Output {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!(
                "cloud115-watch-{}-{}",
                std::process::id(),
                std::thread::current()
                    .name()
                    .unwrap_or("watch")
                    .replace(':', "_")
            ));
            fs::create_dir(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for Output {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn setup(n: u32) -> (Fake, Config, Checkpoint) {
        let cfg = Config {
            auth_id: "test-grant".into(),
            options: crate::settings::Options {
                page_size: HEAD,
                ..Default::default()
            },
            base_url: "http://test.invalid".into(),
            libraries: vec![Library {
                remote: "/Library".into(),
                local: "/selected/grant".into(),
                fast_sync: true,
                ..Default::default()
            }],
            ..Default::default()
        };
        let entries=(1..=n).map(|i|serde_json::from_value(serde_json::json!({"fid":i,"pid":2,"fc":1,"fn":format!("Film{i}.mkv"),"pc":format!("pc{i}"),"fs":12345})).unwrap()).collect();
        let mut cp = Checkpoint::default();
        cp.reconcile(&cfg);
        (
            Fake {
                cfg: cfg.clone(),
                entries,
                folder: "Series".into(),
                anchor: "1".into(),
                fail: None,
            },
            cfg,
            cp,
        )
    }
    async fn tick(d: &Fake, c: &Config, cp: &mut Checkpoint, p: &Output) -> Result<u64> {
        let mut r = sync::Report::default();
        discover(
            d,
            c,
            &mut cp.mappings[0],
            &p.0,
            &mut Budget {
                left: 50,
                background: false,
                daily: 0,
            },
            &mut r,
        )
        .await?;
        Ok(r.written)
    }
    #[tokio::test]
    async fn small_listing_writes_immediately_and_revalidates_known_parents() {
        let (mut d, c, mut cp) = setup(3);
        let p = Output::new();
        assert_eq!(tick(&d, &c, &mut cp, &p).await, Ok(3));
        let content = fs::read_to_string(p.0.join("Series/Film2.strm")).unwrap();
        assert!(content.ends_with("/play?pc=pc2\n"));
        assert_eq!(tick(&d, &c, &mut cp, &p).await, Ok(0));
        d.entries[2].name = "Renamed.mkv".into();
        d.entries[2].pick_code = "changed".into();
        assert_eq!(tick(&d, &c, &mut cp, &p).await, Ok(1));
        assert!(p.0.join("Series/Film3.strm").exists());
        d.folder = "Moved".into();
        d.anchor = "99".into();
        assert!(tick(&d, &c, &mut cp, &p).await.is_err());
        assert!(!p.0.join("Moved").exists());
        d.anchor = "1".into();
        assert_eq!(tick(&d, &c, &mut cp, &p).await, Ok(3));
    }
    #[tokio::test]
    async fn plain_quick_sync_urls_get_only_later_siblings_in_the_current_grant() {
        let (mut drive, cfg, mut cp) = setup(3);
        let root = Output::new();
        tick(&drive, &cfg, &mut cp, &root).await.unwrap();
        let old = fs::read(root.0.join("Series/Film2.strm")).unwrap();
        assert_eq!(indexed_siblings(&cp, &cfg, "PC2", 2), ["pc3"]);
        assert!(indexed_siblings(&cp, &cfg, "pc2", 0).is_empty());
        let mut other = cfg.clone();
        other.auth_id = "different".into();
        assert!(indexed_siblings(&cp, &other, "pc2", 1).is_empty());
        other = cfg.clone();
        other.libraries.clear();
        assert!(indexed_siblings(&cp, &other, "pc2", 2).is_empty());
        drive.entries[2].name = "Before.mkv".into();
        tick(&drive, &cfg, &mut cp, &root).await.unwrap();
        assert!(indexed_siblings(&cp, &cfg, "pc2", 2).is_empty());
        assert_eq!(fs::read(root.0.join("Series/Film2.strm")).unwrap(), old);
    }

    #[tokio::test]
    async fn large_listing_resumes_partial_sweep_after_restart() {
        let (mut d, c, mut cp) = setup(HEAD + 2 * SWEEP + 1);
        let p = Output::new();
        assert_eq!(tick(&d, &c, &mut cp, &p).await, Ok((HEAD + SWEEP).into()));
        let cursor = cp.mappings[0].status.next_offset;
        assert_eq!(cursor, HEAD + SWEEP);
        d.fail = Some(cursor);
        d.entries[0].name = "NewHead.mkv".into();
        assert!(tick(&d, &c, &mut cp, &p).await.is_err());
        assert!(p.0.join("Series/NewHead.strm").exists());
        assert_eq!(cp.mappings[0].status.next_offset, cursor);
        cp = serde_json::from_slice(&serde_json::to_vec(&cp).unwrap()).unwrap();
        d.fail = None;
        assert_eq!(tick(&d, &c, &mut cp, &p).await, Ok(SWEEP.into()));
        assert_eq!(tick(&d, &c, &mut cp, &p).await, Ok(1));
        d.entries.last_mut().unwrap().name = "OldMovedIn.mkv".into();
        for _ in 0..3 {
            tick(&d, &c, &mut cp, &p).await.unwrap();
        }
        assert!(p.0.join("Series/OldMovedIn.strm").exists());
    }
    #[tokio::test]
    async fn account_switch_and_unsafe_components_never_authorize_output() {
        let (mut d, c, mut cp) = setup(1);
        let p = Output::new();
        d.folder = "..".into();
        assert!(tick(&d, &c, &mut cp, &p).await.is_err());
        d.folder = "Series".into();
        d.cfg.auth_id = "other".into();
        assert_eq!(
            tick(&d, &c, &mut cp, &p).await,
            Err("mapping or account changed")
        );
        assert!(!p.0.join("Series").exists());
        cp.reconcile(&d.cfg);
        assert!(cp.mappings[0].dirs.is_empty());
        assert_eq!(cp.last_poll_at, 0);
        for name in ["../bad", "..\\bad", ".. ", "CON.txt"] {
            assert!(component(name).is_err());
        }
    }
}
