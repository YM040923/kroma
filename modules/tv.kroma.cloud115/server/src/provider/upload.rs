//! `POST /_port/cloud115/upload` and `/upload-status`: put one staged local file
//! into the drive, with no page in the middle.
//!
//! The job runs in the background and reports through [`status`], because a film
//! is hours of bytes and a point call is not. It is deliberately serial:
//!
//! 1. the local path must resolve under a library root, and its drive folder is
//!    either given (`cid` / `remote`) or derived from the library mappings;
//! 2. SHA1 + size, straight from the module's existing fingerprint code;
//! 3. 秒传 first — the drive answers `2` when it already has the bytes;
//! 4. otherwise the module's existing multipart start/part/finish;
//! 5. **verify the drive really lists the file before anything is deleted.**
//!    A callback that never fired is not a failed upload to retry blindly, but it
//!    is certainly not a reason to remove the only copy.
//!
//! Parts are read here, one at a time, exactly like the admin page's flow: the
//! point call never carries file bytes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use axum::extract::State;
use axum::Json;
use kroma_module_sdk::host::{port_reply, run_blocking, HostCtx, LibraryFolders};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::client;
use crate::settings::{Config, Library};

/// How many finished/abandoned job records the map keeps before the oldest goes.
const MAX_JOBS: usize = 200;
/// Listing pages one verification reads. A new upload sorts to the top, so one
/// page is normally enough.
const VERIFY_PAGES: usize = 5;
const VERIFY_PAGE_SIZE: u32 = 1000;
/// How many times verification re-lists while the drive's callback catches up.
const VERIFY_ATTEMPTS: usize = 3;
const VERIFY_WAIT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadBody {
    /// An absolute path under one of this server's library folders.
    pub path: String,
    /// The drive folder path (`/电影/动画电影`). Unused when `cid` is given.
    #[serde(default)]
    pub remote: String,
    /// The drive folder id, when the caller already knows it.
    #[serde(default)]
    pub cid: String,
    /// Remove the local file once the drive has been verified to hold it.
    #[serde(default)]
    pub delete_local: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Started {
    pub job_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UploadState {
    Queued,
    Running,
    Verifying,
    Done,
    Failed,
}

impl UploadState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            UploadState::Queued => "queued",
            UploadState::Running => "running",
            UploadState::Verifying => "verifying",
            UploadState::Done => "done",
            UploadState::Failed => "failed",
        }
    }

    #[cfg(test)]
    fn parse(value: &str) -> Self {
        match value {
            "running" => UploadState::Running,
            "verifying" => UploadState::Verifying,
            "done" => UploadState::Done,
            "failed" => UploadState::Failed,
            _ => UploadState::Queued,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadStatus {
    /// `queued`, `running`, `verifying`, `done` or `failed`.
    pub state: String,
    /// Parts sent so far (`0` for a 秒传 hit).
    pub uploaded: u64,
    pub parts: usize,
    pub pickcode: String,
    pub remote: String,
    pub size_bytes: u64,
    pub local_deleted: bool,
    pub error: String,
    /// A non-fatal problem: the upload succeeded but, say, the local copy could
    /// not be removed. Present only when there is one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub warning: String,
}

#[derive(Debug, Clone)]
struct Job {
    state: UploadState,
    uploaded: u64,
    parts: usize,
    pickcode: String,
    remote: String,
    size_bytes: u64,
    local_deleted: bool,
    error: String,
    warning: String,
    started_at: i64,
}

impl Job {
    fn queued() -> Self {
        Self {
            state: UploadState::Queued,
            uploaded: 0,
            parts: 0,
            pickcode: String::new(),
            remote: String::new(),
            size_bytes: 0,
            local_deleted: false,
            error: String::new(),
            warning: String::new(),
            started_at: crate::store::now_ms(),
        }
    }

    fn status(&self) -> UploadStatus {
        UploadStatus {
            state: self.state.as_str().to_string(),
            uploaded: self.uploaded,
            parts: self.parts,
            pickcode: self.pickcode.clone(),
            remote: self.remote.clone(),
            size_bytes: self.size_bytes,
            local_deleted: self.local_deleted,
            error: self.error.clone(),
            warning: self.warning.clone(),
        }
    }
}

fn jobs() -> &'static Mutex<HashMap<String, Job>> {
    static JOBS: OnceLock<Mutex<HashMap<String, Job>>> = OnceLock::new();
    JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn new_job_id() -> String {
    // `usize`, not `u64`: a WebAssembly component has no 64-bit atomics, and the
    // counter only has to make ids unique within one process.
    static SEQUENCE: AtomicUsize = AtomicUsize::new(1);
    format!(
        "up-{}-{}",
        crate::store::now_ms(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}

fn insert_job(id: &str, job: Job) {
    let mut all = jobs()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    all.insert(id.to_string(), job);
    while all.len() > MAX_JOBS {
        let Some(oldest) = all
            .iter()
            .min_by_key(|(_, job)| job.started_at)
            .map(|(key, _)| key.clone())
        else {
            break;
        };
        all.remove(&oldest);
    }
}

fn update_job(id: &str, change: impl FnOnce(&mut Job)) {
    let mut all = jobs()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(job) = all.get_mut(id) {
        change(job);
    }
}

fn job_status(id: &str) -> Option<UploadStatus> {
    jobs()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(id)
        .map(Job::status)
}

/// One upload, resolved before anything is started.
#[derive(Debug, Clone)]
struct Prepared {
    id: String,
    path: PathBuf,
    file_name: String,
    /// The drive folder path it should land in; empty when `cid` names it.
    remote: String,
    /// Numeric folder id, when the caller supplied one.
    cid: String,
    delete_local: bool,
}

/// `POST /_port/cloud115/upload`: resolve, register, spawn, answer.
pub(super) async fn upload<S: HostCtx + Clone + Send + Sync + 'static>(
    State(host): State<S>,
    Json(body): Json<UploadBody>,
) -> Json<Result<Started, String>> {
    // Path resolution touches the filesystem and the host's library list, so it
    // runs where `port_reply` would run it; the answer is the same envelope.
    let prepare_host = host.clone();
    let prepared = run_blocking(move || prepare(&prepare_host, &body)).await;
    match prepared {
        Ok(Ok(prepared)) => {
            let job_id = prepared.id.clone();
            spawn(prepared, host.clone());
            Json(Ok(Started { job_id }))
        }
        Ok(Err(error)) => Json(Err(error)),
        Err(_) => Json(Err("准备上传任务失败".into())),
    }
}

/// `POST /_port/cloud115/upload-status`: the job's record, or an `Err` when the
/// id is not one this process started.
pub(super) async fn status(Json(body): Json<StatusBody>) -> Json<Result<UploadStatus, String>> {
    port_reply(move || {
        let job_id = body.job_id.trim();
        if job_id.is_empty() {
            anyhow::bail!("缺少任务号");
        }
        job_status(job_id).ok_or_else(|| anyhow::anyhow!("任务不存在（模块可能重启过）"))
    })
    .await
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusBody {
    #[serde(default)]
    pub job_id: String,
}

/// Resolve everything that can fail before a job exists: the path against the
/// library roots, and the drive folder against the library mappings.
fn prepare<S: HostCtx>(host: &S, body: &UploadBody) -> Result<Prepared, String> {
    let raw = body.path.trim();
    if raw.is_empty() {
        return Err("缺少要上传的文件路径".into());
    }
    let config = Config::load();
    if config.cookie.trim().is_empty() {
        return Err("115 的 Cookie 还没配置".into());
    }
    let folders = host.library_folders();
    let roots: Vec<PathBuf> = folders
        .iter()
        .flat_map(|folder| folder.folders.iter())
        .map(PathBuf::from)
        .collect();
    let path = crate::upload::allowed_path(raw, &roots)?;

    let cid = body.cid.trim();
    if !cid.is_empty() && !cid.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("目录号无效".into());
    }
    let remote = if cid.is_empty() && body.remote.trim().is_empty() {
        remote_for_file(&config.libraries, &folders, &path).ok_or_else(|| {
            "这个本地路径不在任何已配置的目录映射里，请给出 remote 或 cid".to_string()
        })?
    } else {
        body.remote.trim().to_string()
    };
    let file_name = path
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("upload.bin")
        .to_string();
    let id = new_job_id();
    // Registered before the answer, so the first `upload-status` poll can never
    // race the job's own start.
    insert_job(&id, Job::queued());
    Ok(Prepared {
        id,
        path,
        file_name,
        remote,
        cid: cid.to_string(),
        delete_local: body.delete_local,
    })
}

/// The drive folder a local file belongs in, from the library mappings: the
/// longest local root that contains it wins, and the drive side keeps the
/// folders below that root. `None` when no mapping covers the file.
pub(crate) fn remote_for_file(
    libraries: &[Library],
    folders: &[LibraryFolders],
    file: &Path,
) -> Option<String> {
    let mut best: Option<(usize, String)> = None;
    for library in libraries {
        let remote_root = library.remote.trim();
        let local = library.local.trim();
        if remote_root.is_empty() || local.is_empty() {
            continue;
        }
        let Some(root) = crate::sync::resolve_local_path(folders, local) else {
            continue;
        };
        // `file` is already canonical; matching the stored root as it is keeps a
        // symlinked media root usable when it cannot be canonicalised here.
        let root = std::fs::canonicalize(&root).unwrap_or(root);
        let Ok(rest) = file.strip_prefix(&root) else {
            continue;
        };
        let mut remote = remote_root.trim_end_matches('/').to_string();
        if let Some(parent) = rest.parent() {
            for part in parent.components() {
                if !matches!(part, std::path::Component::Normal(_)) {
                    continue;
                }
                let name = part.as_os_str().to_str()?;
                if remote.is_empty() || remote == "/" {
                    remote = format!("/{name}");
                } else {
                    remote.push('/');
                    remote.push_str(name);
                }
            }
        }
        if remote.is_empty() {
            remote.push('/');
        }
        let depth = root.components().count();
        if best
            .as_ref()
            .is_none_or(|(best_depth, _)| depth > *best_depth)
        {
            best = Some((depth, remote));
        }
    }
    best.map(|(_, remote)| remote)
}

fn spawn<S: HostCtx + Clone + Send + Sync + 'static>(job: Prepared, host: S) {
    // A job outlives the request that started it; the module's own sync runs the
    // same way. Keep the host handle so we can rescan after replacing a staged
    // local file with its STRM.
    tokio::spawn(run(job, host));
}

/// The background half: resolve the folder, fingerprint, 秒传 or multipart, then
/// verify before deleting anything.
async fn run<S: HostCtx + Clone + Send + Sync + 'static>(job: Prepared, host: S) {
    let id = job.id.clone();
    let config = Config::load();
    update_job(&id, |entry| entry.state = UploadState::Running);

    let cid = match crate::upload::resolve_folder_cid(&config, &job.cid, &job.remote).await {
        Ok(cid) => cid,
        Err(error) => return fail(&id, format!("找不到目标目录：{error}")),
    };
    update_job(&id, |entry| entry.remote = job.remote.clone());

    let path = job.path.clone();
    let fingerprint = run_blocking(move || crate::upload::fingerprint(&path)).await;
    let (fileid, size) = match fingerprint {
        Ok(Ok(pair)) => pair,
        Ok(Err(error)) => return fail(&id, format!("读取文件失败：{error}")),
        Err(_) => return fail(&id, "读取文件失败"),
    };
    if size == 0 {
        return fail(&id, "空文件不能上传");
    }
    update_job(&id, |entry| entry.size_bytes = size);

    let Some(userid) = crate::upload::cookie_uid(&config.cookie) else {
        return fail(&id, "Cookie 里缺少 UID");
    };
    let userkey = match client::upload_key(&config.cookie, client::UPLOAD_UA).await {
        Ok(key) => key,
        Err(error) => return fail(&id, format!("拿不到上传密钥：{error}")),
    };
    let target = format!("U_1_{cid}");
    let answer = match crate::upload::init_with_challenge(
        &config,
        &userkey,
        &userid,
        &target,
        &job.file_name,
        &job.path,
        &fileid,
        size,
    )
    .await
    {
        Ok(answer) => answer,
        Err(error) => return fail(&id, format!("上传初始化失败：{error}")),
    };

    let status = crate::upload::status_of(&answer);
    let init_pickcode = answer
        .get("pickcode")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    if status == 2 {
        // 秒传: the drive already holds these exact bytes. Nothing is sent, but
        // "the drive says so" is not the same as "the folder lists it".
        return finish(&job, &config, &cid, size, &init_pickcode, &host).await;
    }
    if status != 1 {
        return fail(&id, format!("115 没有接受这次上传（状态 {status}）"));
    }

    let bucket = answer
        .get("bucket")
        .and_then(Value::as_str)
        .unwrap_or(crate::oss::DEFAULT_BUCKET)
        .to_string();
    let Some(object) = answer.get("object").and_then(Value::as_str) else {
        return fail(&id, "115 没有给出上传对象");
    };
    let object = object.to_string();
    let callback = answer
        .get("callback")
        .and_then(|callback| callback.get("callback"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let callback_var = answer
        .get("callback")
        .and_then(|callback| callback.get("callback_var"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let (Some(callback), Some(callback_var)) = (callback, callback_var) else {
        return fail(&id, "115 没有给出上传回调参数");
    };

    let part_size = crate::upload::part_size_for(size);
    let parts = crate::upload::part_count(size, part_size);
    update_job(&id, |entry| entry.parts = parts);
    let upload_id = match crate::oss::start(&bucket, &object) {
        Ok(upload_id) => upload_id,
        Err(error) => return fail(&id, error),
    };

    let mut etags = Vec::with_capacity(parts);
    for index in 0..parts {
        let offset = part_size * index as u64;
        let length = part_size.min(size.saturating_sub(offset));
        let data = match crate::upload::read_part(&job.path, offset, length) {
            Ok(data) => data,
            Err(error) => return fail(&id, format!("读取分片失败：{error}")),
        };
        match crate::oss::put_part(&bucket, &object, &upload_id, index as u32 + 1, &data) {
            Ok(etag) => etags.push(etag),
            Err(error) => return fail(&id, error),
        }
        update_job(&id, |entry| entry.uploaded = (index + 1) as u64);
    }
    if let Err(error) = crate::oss::complete(
        &bucket,
        &object,
        &upload_id,
        &etags,
        &callback,
        &callback_var,
    ) {
        return fail(&id, error);
    }
    finish(&job, &config, &cid, size, &init_pickcode, &host).await;
}

/// Verify the drive lists the file, then create the library's STRM before
/// deleting a staged local copy. The core scanner is triggered only after the
/// replacement path is ready.
async fn finish<S: HostCtx>(
    job: &Prepared,
    config: &Config,
    cid: &str,
    size: u64,
    known_pickcode: &str,
    host: &S,
) {
    let id = job.id.as_str();
    update_job(id, |entry| entry.state = UploadState::Verifying);
    let verified = match verify_remote(&config.cookie, cid, &job.file_name, size).await {
        Ok(pickcode) => pickcode,
        Err(error) => return fail(id, error),
    };
    let pickcode = if verified.is_empty() {
        known_pickcode.to_string()
    } else {
        verified
    };
    let mut warning = String::new();
    let mut local_deleted = false;
    let mut strm_ready = false;
    if job.delete_local {
        match write_staged_strm(config, &job.path, &job.file_name, &pickcode, size) {
            Ok(true) => {
                strm_ready = true;
                match evict_local(&job.path) {
                    Ok(()) => local_deleted = true,
                    Err(error) => warning = error,
                }
            }
            Ok(false) => {
                warning =
                    "上传已校验通过，但同名 STRM 指向其他资源；为避免替换错误，保留本地文件".into();
            }
            Err(error) => {
                warning = format!("上传已校验通过，但无法创建 STRM，保留本地文件：{error}");
            }
        }
    }
    update_job(id, |entry| {
        entry.state = UploadState::Done;
        entry.pickcode = pickcode;
        entry.size_bytes = size;
        entry.local_deleted = local_deleted;
        entry.warning = warning;
    });
    if strm_ready {
        host.trigger_job("library.scan", "cloud115 staged upload");
    }
}

/// Ensure the library still has a playable STRM before the staged source is
/// removed. An existing different STRM is a conflict, not permission to replace.
fn write_staged_strm(
    config: &Config,
    path: &Path,
    file_name: &str,
    pickcode: &str,
    size: u64,
) -> Result<bool, String> {
    if config.base_url.trim().is_empty() {
        return Err("服务器访问地址未配置".into());
    }
    let dir = path
        .parent()
        .ok_or_else(|| "暂存文件没有父目录".to_string())?;
    let stem_source = if config.options.keep_suffix {
        file_name
    } else {
        crate::sync::strip_extension(file_name)
    };
    let stem = crate::sync::safe_name(stem_source);
    let strm = dir.join(format!("{stem}.strm"));
    let wanted = pickcode.trim().to_ascii_lowercase();
    if strm.exists() {
        let current = std::fs::read_to_string(&strm).unwrap_or_default();
        let same = current
            .split("pc=")
            .nth(1)
            .and_then(|tail| tail.split(['&', '\r', '\n']).next())
            .is_some_and(|held| held.eq_ignore_ascii_case(&wanted));
        return Ok(same);
    }
    let mut seen = std::collections::HashSet::new();
    let mut report = crate::sync::Report::default();
    crate::sync::write_entry(
        config,
        dir,
        &stem,
        &wanted,
        size.min(i64::MAX as u64) as i64,
        &[],
        false,
        &mut seen,
        &mut report,
    )?;
    Ok(true)
}

fn fail(id: &str, error: impl Into<String>) {
    let error = error.into();
    tracing::warn!(job = id, %error, "a 115 provider upload failed");
    update_job(id, |entry| {
        entry.state = UploadState::Failed;
        entry.error = error;
    });
}

/// List the folder until the file appears, re-listing a few times because the
/// drive's callback can lag the merge by a moment. Any listing failure is fatal
/// for the verification: a folder nobody could read is not a folder that holds
/// the file.
async fn verify_remote(cookie: &str, cid: &str, name: &str, size: u64) -> Result<String, String> {
    let mut last = String::new();
    for attempt in 0..VERIFY_ATTEMPTS {
        if attempt > 0 {
            tokio::time::sleep(VERIFY_WAIT).await;
        }
        match find_remote(cookie, cid, name, size).await {
            Ok(Some(pickcode)) => return Ok(pickcode),
            Ok(None) => last = format!("目录里还没有 {name}"),
            Err(error) => last = error,
        }
    }
    Err(format!("上传后校验没通过：{last}"))
}

/// One read-only pass over the folder: same name and same size, never a folder.
async fn find_remote(
    cookie: &str,
    cid: &str,
    name: &str,
    size: u64,
) -> Result<Option<String>, String> {
    let mut offset = 0u32;
    for _ in 0..VERIFY_PAGES {
        let page = client::list(cookie, client::MODULE_UA, cid, offset, VERIFY_PAGE_SIZE)
            .await
            .map_err(|error| format!("列目录失败：{error}"))?;
        let read = page.entries.len();
        for entry in &page.entries {
            if entry.is_dir() || entry.name != name {
                continue;
            }
            // -1 is what the drive answers for a size it did not spell out.
            if size == 0 || entry.size <= 0 || entry.size as u64 == size {
                return Ok(Some(entry.pick_code.clone()));
            }
        }
        offset += read as u32;
        if read == 0 || i64::from(offset) >= page.total {
            break;
        }
    }
    Ok(None)
}

/// Remove the staged file. Only ever called after a verified upload.
pub(crate) fn evict_local(path: &Path) -> Result<(), String> {
    std::fs::remove_file(path)
        .map_err(|error| format!("上传已校验通过，但删除本地文件失败：{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    use kroma_module_sdk::testing::temp_dir;

    fn library(remote: &str, local: &str) -> Library {
        Library {
            remote: remote.into(),
            local: local.into(),
        }
    }

    #[test]
    fn a_file_below_a_mapping_keeps_its_folders_on_the_drive_side() {
        let libraries = vec![library("/电影", "/media/115/电影")];
        let remote = remote_for_file(
            &libraries,
            &[],
            Path::new("/media/115/电影/2024/Batman.Begins.mkv"),
        );
        assert_eq!(remote.as_deref(), Some("/电影/2024"));
    }

    #[test]
    fn a_file_in_the_library_root_maps_to_the_root_itself() {
        let libraries = vec![library("/电影", "/media/115/电影")];
        let remote = remote_for_file(&libraries, &[], Path::new("/media/115/电影/a.mkv"));
        assert_eq!(remote.as_deref(), Some("/电影"));

        let trailing = vec![library("/电影/", "/media/115/电影")];
        let remote = remote_for_file(&trailing, &[], Path::new("/media/115/电影/a.mkv"));
        assert_eq!(
            remote.as_deref(),
            Some("/电影"),
            "a trailing slash is not a folder"
        );
    }

    #[test]
    fn the_longest_matching_mapping_wins() {
        let libraries = vec![
            library("/电影", "/media/115"),
            library("/动画", "/media/115/动画"),
        ];
        let remote = remote_for_file(&libraries, &[], Path::new("/media/115/动画/吉卜力/a.mkv"));
        assert_eq!(
            remote.as_deref(),
            Some("/动画/吉卜力"),
            "the deeper local root is the mapping that covers the file"
        );
    }

    #[test]
    fn a_path_no_mapping_covers_has_no_remote_folder() {
        let libraries = vec![library("/电影", "/media/115/电影")];
        assert_eq!(
            remote_for_file(&libraries, &[], Path::new("/media/other/a.mkv")),
            None
        );
        assert_eq!(
            remote_for_file(&[], &[], Path::new("/media/115/电影/a.mkv")),
            None,
            "no mapping at all maps nothing"
        );
        let half = vec![library("/电影", "")];
        assert_eq!(
            remote_for_file(&half, &[], Path::new("/media/115/电影/a.mkv")),
            None,
            "a mapping with one side named is not a mapping"
        );
    }

    #[test]
    fn a_relative_mapping_is_anchored_at_the_first_library_folder() {
        let folders = vec![LibraryFolders {
            id: "1".into(),
            kind: "movies".into(),
            name: "Media".into(),
            folders: vec!["/media".into()],
        }];
        let libraries = vec![library("/电影", "115/电影")];
        let remote = remote_for_file(&libraries, &folders, Path::new("/media/115/电影/a.mkv"));
        assert_eq!(remote.as_deref(), Some("/电影"));
    }

    fn job_with(state: UploadState) -> Job {
        Job {
            state,
            ..Job::queued()
        }
    }

    #[test]
    fn a_job_moves_queued_running_verifying_done() {
        let id = new_job_id();
        insert_job(&id, job_with(UploadState::Queued));
        assert_eq!(job_status(&id).unwrap().state, "queued");

        update_job(&id, |job| job.state = UploadState::Running);
        assert_eq!(job_status(&id).unwrap().state, "running");

        update_job(&id, |job| {
            job.state = UploadState::Verifying;
            job.parts = 3;
            job.uploaded = 3;
        });
        let verifying = job_status(&id).unwrap();
        assert_eq!(verifying.state, "verifying");
        assert_eq!((verifying.uploaded, verifying.parts), (3, 3));

        update_job(&id, |job| {
            job.state = UploadState::Done;
            job.pickcode = "abc".into();
        });
        let done = job_status(&id).unwrap();
        assert_eq!(done.state, "done");
        assert_eq!(done.pickcode, "abc");
        assert!(done.error.is_empty());
    }

    #[test]
    fn a_failed_job_keeps_its_reason_and_never_claims_a_pickcode() {
        let id = new_job_id();
        insert_job(&id, job_with(UploadState::Queued));
        fail(&id, "OSS 分片 2 没传上");
        let status = job_status(&id).unwrap();
        assert_eq!(status.state, "failed");
        assert!(status.error.contains("分片 2"), "{status:?}");
        assert!(status.pickcode.is_empty());
    }

    #[test]
    fn an_unknown_job_is_not_found() {
        assert!(job_status("up-does-not-exist").is_none());
    }

    #[test]
    fn an_upload_status_keeps_the_points_field_names() {
        let value = serde_json::to_value(UploadStatus {
            state: "done".into(),
            uploaded: 2,
            parts: 2,
            pickcode: "abc".into(),
            remote: "/电影/Test.mkv".into(),
            size_bytes: 123,
            local_deleted: false,
            error: String::new(),
            warning: String::new(),
        })
        .unwrap();
        assert_eq!(value["state"], "done");
        assert_eq!(value["uploaded"], 2);
        assert_eq!(value["parts"], 2);
        assert_eq!(value["pickcode"], "abc");
        assert_eq!(value["error"], "");
        assert!(
            value.get("warning").is_none(),
            "an empty warning stays out of the answer: {value}"
        );
    }

    #[test]
    fn the_oldest_jobs_are_dropped_once_the_map_is_full() {
        for _ in 0..(MAX_JOBS + 5) {
            let id = new_job_id();
            insert_job(&id, job_with(UploadState::Done));
        }
        let total = jobs()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len();
        assert!(total <= MAX_JOBS, "the map kept {total} jobs");
    }

    #[test]
    fn the_local_file_is_only_removed_when_asked() {
        let dir = temp_dir("cloud115-evict");
        let keep = dir.path().join("keep.mkv");
        std::fs::write(&keep, b"bytes").unwrap();
        assert!(
            evict_local(&keep).is_ok(),
            "an explicit eviction removes it"
        );
        assert!(!keep.exists());

        let untouched = dir.path().join("untouched.mkv");
        std::fs::write(&untouched, b"bytes").unwrap();
        assert!(
            untouched.exists(),
            "nothing removes a file unless the job asked"
        );
    }

    #[test]
    fn a_playable_strm_is_written_before_the_staged_source_is_evicted() {
        let dir = temp_dir("cloud115-staged-strm");
        let local = dir.path().join("Film.mkv");
        let strm = dir.path().join("Film.strm");
        std::fs::write(&local, b"video bytes").unwrap();
        let config = Config {
            base_url: "http://kroma.local:4040".into(),
            ..Config::default()
        };

        assert!(write_staged_strm(&config, &local, "Film.mkv", "AbC123", 42).unwrap());
        assert!(
            local.exists(),
            "the source remains until its replacement is ready"
        );
        assert_eq!(
            std::fs::read_to_string(&strm).unwrap(),
            "http://kroma.local:4040/api/module/tv.kroma.cloud115/play?pc=abc123\n"
        );

        evict_local(&local).unwrap();
        assert!(!local.exists());
        assert!(strm.is_file());
    }

    #[test]
    fn a_staged_upload_never_replaces_a_different_existing_strm() {
        let dir = temp_dir("cloud115-staged-strm-conflict");
        let local = dir.path().join("Film.mkv");
        let strm = dir.path().join("Film.strm");
        std::fs::write(&local, b"video bytes").unwrap();
        std::fs::write(
            &strm,
            "http://kroma.local:4040/api/module/tv.kroma.cloud115/play?pc=other\\n",
        )
        .unwrap();
        let config = Config {
            base_url: "http://kroma.local:4040".into(),
            ..Config::default()
        };

        assert!(!write_staged_strm(&config, &local, "Film.mkv", "abc123", 42).unwrap());
        assert!(
            local.exists(),
            "a conflicting replacement never authorizes eviction"
        );
        assert_eq!(
            std::fs::read_to_string(&strm).unwrap(),
            "http://kroma.local:4040/api/module/tv.kroma.cloud115/play?pc=other\\n"
        );
    }

    #[test]
    fn an_eviction_that_cannot_happen_reports_instead_of_panicking() {
        let error = evict_local(Path::new("/definitely/not/here.mkv")).unwrap_err();
        assert!(error.contains("删除本地文件失败"), "{error}");
    }

    #[test]
    fn state_names_round_trip_through_their_wire_spelling() {
        for state in [
            UploadState::Queued,
            UploadState::Running,
            UploadState::Verifying,
            UploadState::Done,
            UploadState::Failed,
        ] {
            assert_eq!(UploadState::parse(state.as_str()), state);
        }
    }
}
