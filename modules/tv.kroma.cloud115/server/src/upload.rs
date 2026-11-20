//! 秒传: test a local file's fingerprint against the drive before any byte is
//! sent. A hit is one or two signed calls; anything else is reported as "needs
//! a real upload", which this module does not do yet.
//!
//! Only files under a configured library folder are read, and never more than
//! the operator asked for: the path comes from the admin page, the fingerprint
//! is computed with ordinary file reads.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};


use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha1::{Digest, Sha1};

use kroma_module_sdk::domain::Permission;
use kroma_module_sdk::host::{json_error, AuthUser, HostCtx};

use crate::client::{self, ApiError};
use crate::crypto::{self, UploadInit};
use crate::settings::Config;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RapidBody {
    /// The drive folder the file should land in; `remote` may name it instead.
    #[serde(default)]
    cid: String,
    #[serde(default)]
    remote: String,
    /// An absolute path under one of this server's library folders.
    path: String,
}

async fn folder_cid(config: &Config, cid: &str, remote: &str) -> Result<String, Response> {
    let cid = cid.trim();
    if !cid.is_empty() && cid.bytes().all(|byte| byte.is_ascii_digit()) {
        return Ok(cid.to_string());
    }
    let remote = remote.trim();
    if remote.is_empty() {
        return Err(json_error(StatusCode::BAD_REQUEST, "目录号无效"));
    }
    client::folder_id_by_path(&config.cookie, client::MODULE_UA, remote)
        .await
        .map_err(failed)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RapidResult {
    hit: bool,
    need_upload: bool,
    pickcode: Option<String>,
    status: i64,
}

fn cookie_uid(cookie: &str) -> Option<String> {
    cookie.split(';').find_map(|pair| {
        let (key, value) = pair.trim().split_once('=')?;
        (key.eq_ignore_ascii_case("UID")).then(|| value.trim().to_string())
    })
}

fn status_of(data: &Value) -> i64 {
    data.get("status")
        .map(|status| {
            status
                .as_i64()
                .or_else(|| status.as_str().and_then(|text| text.parse().ok()))
                .unwrap_or(0)
        })
        .unwrap_or(0)
}

fn fingerprint(path: &Path) -> std::io::Result<(String, u64)> {
    let mut file = std::fs::File::open(path)?;
    let mut sha = Sha1::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    let mut size = 0u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        sha.update(&buffer[..read]);
        size += read as u64;
    }
    Ok((hex::encode(sha.finalize()).to_uppercase(), size))
}

fn range_sha1(path: &Path, start: u64, length: u64) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    file.seek(SeekFrom::Start(start))?;
    let mut sha = Sha1::new();
    let mut left = length;
    let mut buffer = vec![0u8; 1024 * 1024];
    while left > 0 {
        let want = buffer.len().min(left as usize);
        let read = file.read(&mut buffer[..want])?;
        if read == 0 {
            break;
        }
        sha.update(&buffer[..read]);
        left -= read as u64;
    }
    Ok(hex::encode(sha.finalize()).to_uppercase())
}

/// Resolve `path` only when it sits under one of the configured roots.
fn allowed_path(raw: &str, roots: &[PathBuf]) -> Result<PathBuf, String> {
    let path = std::fs::canonicalize(raw).map_err(|error| format!("读不到这个文件：{error}"))?;
    if !path.is_file() {
        return Err("这不是一个文件".into());
    }
    let inside = roots.iter().any(|root| {
        let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.clone());
        path.starts_with(root)
    });
    if !inside {
        return Err("只能秒传媒体库目录里的文件".into());
    }
    Ok(path)
}

async fn init_once(
    config: &Config,
    userkey: &str,
    userid: &str,
    target: &str,
    name: &str,
    fileid: &str,
    size: u64,
    sign_key: &str,
    sign_val: &str,
) -> Result<Value, ApiError> {
    let payload = crypto::upload_payload(&UploadInit {
        userkey,
        userid,
        fileid,
        filename: name,
        filesize: size,
        target,
        sign_key,
        sign_val,
        appversion: client::APP_VERSION,
    });
    client::upload_init(&config.cookie, &payload.0, &payload.1).await
}

fn failed(error: ApiError) -> Response {
    json_error(StatusCode::BAD_GATEWAY, &error.to_string())
}

// ---------------------------------------------------------------------------
// Real upload: one part per request, so a large film is bounded by the request
// timeout and a dropped connection resumes at the part that failed.

const MIN_PART: u64 = 20 * 1024 * 1024;
const MAX_PARTS: u64 = 9000;

struct Pending {
    path: PathBuf,
    bucket: String,
    object: String,
    upload_id: String,
    pickcode: String,
    callback: String,
    callback_var: String,
    part_size: u64,
    size: u64,
    etags: Vec<Option<String>>,
}

fn pending() -> &'static Mutex<HashMap<String, Pending>> {
    static PENDING: OnceLock<Mutex<HashMap<String, Pending>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

fn part_size_for(size: u64) -> u64 {
    let per_part = size.div_ceil(MAX_PARTS);
    let rounded = per_part.div_ceil(1024 * 1024) * 1024 * 1024;
    rounded.max(MIN_PART)
}

fn part_count(size: u64, part_size: u64) -> usize {
    size.div_ceil(part_size.max(1)).max(1) as usize
}

fn read_part(path: &Path, offset: u64, length: u64) -> std::io::Result<Vec<u8>> {
    let mut file = std::fs::File::open(path)?;
    file.seek(SeekFrom::Start(offset))?;
    let mut data = Vec::with_capacity(length as usize);
    file.take(length).read_to_end(&mut data)?;
    Ok(data)
}

fn new_upload_id() -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_nanos())
        .unwrap_or(0)
        .hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Run the signed init, answering the range challenge once when the drive asks.
async fn init_with_challenge(
    config: &Config,
    userkey: &str,
    userid: &str,
    target: &str,
    name: &str,
    path: &Path,
    fileid: &str,
    size: u64,
) -> Result<Value, ApiError> {
    let mut answer = init_once(config, userkey, userid, target, name, fileid, size, "", "").await?;
    let mut status = status_of(&answer);
    if matches!(status, 6 | 7 | 8) {
        if let Some(check) = answer.get("sign_check").and_then(Value::as_str) {
            if let Some((start, end)) = check.split_once('-') {
                if let (Ok(start), Ok(end)) = (start.trim().parse::<u64>(), end.trim().parse::<u64>()) {
                    let length = end.saturating_sub(start).saturating_add(1).min(size);
                    let digest = range_sha1(path, start, length)
                        .map_err(|error| ApiError::network(format!("读取文件失败：{error}")))?;
                    let sign_key = answer.get("sign_key").and_then(Value::as_str).unwrap_or("");
                    answer = init_once(
                        config, userkey, userid, target, name, fileid, size, sign_key, &digest,
                    )
                    .await?;
                }
            }
        }
    }
    Ok(answer)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StartResult {
    hit: bool,
    upload: Option<String>,
    parts: usize,
    part_size: u64,
    pickcode: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PartBody {
    upload: String,
    index: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PartResult {
    uploaded: usize,
    parts: usize,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FinishBody {
    upload: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FinishResult {
    pickcode: String,
}

/// `POST /upload/start` → 秒传 hit, or an open multipart upload to feed parts to.
pub(crate) async fn start<S: HostCtx + Clone + Send + Sync + 'static>(
    State(host): State<S>,
    AuthUser(user): AuthUser,
    Json(body): Json<RapidBody>,
) -> Result<Json<StartResult>, Response> {
    host.require(&user, Permission::SettingsManage)?;
    let config = Config::load();
    if config.cookie.trim().is_empty() {
        return Err(json_error(StatusCode::SERVICE_UNAVAILABLE, "115 的 Cookie 还没配置"));
    }
    let cid = folder_cid(&config, &body.cid, &body.remote).await?;
    let roots: Vec<PathBuf> = host
        .library_folders()
        .into_iter()
        .flat_map(|folder| folder.folders)
        .map(PathBuf::from)
        .collect();
    let raw = body.path.trim().to_string();
    let resolved = allowed_path(&raw, &roots)
        .map_err(|error| json_error(StatusCode::BAD_REQUEST, &error))?;
    let (fileid, size) = fingerprint(&resolved)
        .map_err(|error| json_error(StatusCode::BAD_REQUEST, &format!("读取文件失败：{error}")))?;
    let name = resolved
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("upload.bin")
        .to_string();
    let userid = cookie_uid(&config.cookie)
        .ok_or_else(|| json_error(StatusCode::BAD_GATEWAY, "Cookie 里缺少 UID"))?;
    let userkey = client::upload_key(&config.cookie, client::UPLOAD_UA)
        .await
        .map_err(failed)?;
    let target = format!("U_1_{cid}");
    let answer = init_with_challenge(
        &config, &userkey, &userid, &target, &name, &resolved, &fileid, size,
    )
    .await
    .map_err(failed)?;
    let status = status_of(&answer);
    if status == 2 {
        return Ok(Json(StartResult {
            hit: true,
            upload: None,
            parts: 0,
            part_size: 0,
            pickcode: answer
                .get("pickcode")
                .and_then(Value::as_str)
                .map(str::to_string),
        }));
    }
    if status != 1 {
        return Err(json_error(
            StatusCode::BAD_GATEWAY,
            &format!("115 没有接受这次上传（状态 {status}）"),
        ));
    }
    let bucket = answer
        .get("bucket")
        .and_then(Value::as_str)
        .unwrap_or(crate::oss::DEFAULT_BUCKET)
        .to_string();
    let object = answer
        .get("object")
        .and_then(Value::as_str)
        .ok_or_else(|| json_error(StatusCode::BAD_GATEWAY, "115 没有给出上传对象"))?
        .to_string();
    let callback = answer
        .get("callback")
        .and_then(|callback| callback.get("callback"))
        .and_then(Value::as_str)
        .ok_or_else(|| json_error(StatusCode::BAD_GATEWAY, "115 没有给出回调参数"))?
        .to_string();
    let callback_var = answer
        .get("callback")
        .and_then(|callback| callback.get("callback_var"))
        .and_then(Value::as_str)
        .ok_or_else(|| json_error(StatusCode::BAD_GATEWAY, "115 没有给出回调变量"))?
        .to_string();
    let pickcode = answer
        .get("pickcode")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let upload_id = crate::oss::start(&bucket, &object)
        .map_err(|error| json_error(StatusCode::BAD_GATEWAY, &error))?;
    let part_size = part_size_for(size);
    let parts = part_count(size, part_size);
    let upload = new_upload_id();
    pending()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(
            upload.clone(),
            Pending {
                path: resolved,
                bucket,
                object,
                upload_id,
                pickcode: pickcode.clone(),
                callback,
                callback_var,
                part_size,
                size,
                etags: vec![None; parts],
            },
        );
    Ok(Json(StartResult {
        hit: false,
        upload: Some(upload),
        parts,
        part_size,
        pickcode: (!pickcode.is_empty()).then_some(pickcode),
    }))
}

/// `POST /upload/part` → upload one part of a started upload.
pub(crate) async fn part<S: HostCtx + Clone + Send + Sync + 'static>(
    State(host): State<S>,
    AuthUser(user): AuthUser,
    Json(body): Json<PartBody>,
) -> Result<Json<PartResult>, Response> {
    host.require(&user, Permission::SettingsManage)?;
    let (path, bucket, object, upload_id, part_size, size, index) = {
        let all = pending()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entry = all
            .get(&body.upload)
            .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "这次上传已经结束"))?;
        if body.index >= entry.etags.len() {
            return Err(json_error(StatusCode::BAD_REQUEST, "分片号超出范围"));
        }
        (
            entry.path.clone(),
            entry.bucket.clone(),
            entry.object.clone(),
            entry.upload_id.clone(),
            entry.part_size,
            entry.size,
            body.index,
        )
    };
    let offset = part_size * index as u64;
    let length = part_size.min(size.saturating_sub(offset));
    let data = read_part(&path, offset, length)
        .map_err(|error| json_error(StatusCode::BAD_REQUEST, &format!("读取分片失败：{error}")))?;
    let etag = crate::oss::put_part(&bucket, &object, &upload_id, index as u32 + 1, &data)
        .map_err(|error| json_error(StatusCode::BAD_GATEWAY, &error))?;
    let parts = {
        let mut all = pending()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entry = all
            .get_mut(&body.upload)
            .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "这次上传已经结束"))?;
        entry.etags[index] = Some(etag);
        entry.etags.len()
    };
    Ok(Json(PartResult {
        uploaded: index + 1,
        parts,
    }))
}

/// `POST /upload/finish` → merge the parts; the callback puts the file on 115.
pub(crate) async fn finish<S: HostCtx + Clone + Send + Sync + 'static>(
    State(host): State<S>,
    AuthUser(user): AuthUser,
    Json(body): Json<FinishBody>,
) -> Result<Json<FinishResult>, Response> {
    host.require(&user, Permission::SettingsManage)?;
    let entry = pending()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&body.upload)
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "这次上传已经结束"))?;
    let Some(etags) = entry.etags.iter().cloned().collect::<Option<Vec<String>>>() else {
        return Err(json_error(StatusCode::CONFLICT, "还有分片没传完"));
    };
    crate::oss::complete(
        &entry.bucket,
        &entry.object,
        &entry.upload_id,
        &etags,
        &entry.callback,
        &entry.callback_var,
    )
    .map_err(|error| json_error(StatusCode::BAD_GATEWAY, &error))?;
    Ok(Json(FinishResult {
        pickcode: entry.pickcode,
    }))
}

/// `POST /upload/rapid` → try 秒传 for one local file into one drive folder.
pub(crate) async fn rapid<S: HostCtx + Clone + Send + Sync + 'static>(
    State(host): State<S>,
    AuthUser(user): AuthUser,
    Json(body): Json<RapidBody>,
) -> Result<Json<RapidResult>, Response> {
    host.require(&user, Permission::SettingsManage)?;
    let config = Config::load();
    if config.cookie.trim().is_empty() {
        return Err(json_error(StatusCode::SERVICE_UNAVAILABLE, "115 的 Cookie 还没配置"));
    }
    let cid = folder_cid(&config, &body.cid, &body.remote).await?;
    let roots: Vec<PathBuf> = host
        .library_folders()
        .into_iter()
        .flat_map(|folder| folder.folders)
        .map(PathBuf::from)
        .collect();
    let raw = body.path.trim().to_string();
    let resolved = allowed_path(&raw, &roots)
        .map_err(|error| json_error(StatusCode::BAD_REQUEST, &error))?;
    let (fileid, size) = fingerprint(&resolved)
        .map_err(|error| json_error(StatusCode::BAD_REQUEST, &format!("读取文件失败：{error}")))?;
    let name = resolved
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("upload.bin")
        .to_string();
    let userid = cookie_uid(&config.cookie)
        .ok_or_else(|| json_error(StatusCode::BAD_GATEWAY, "Cookie 里缺少 UID"))?;
    let userkey = client::upload_key(&config.cookie, client::UPLOAD_UA)
        .await
        .map_err(failed)?;
    let target = format!("U_1_{cid}");

    let first = init_once(
        &config, &userkey, &userid, &target, &name, &fileid, size, "", "",
    )
    .await
    .map_err(failed)?;
    let mut status = status_of(&first);
    let mut answer = first;

    // A range challenge: hash the bytes the drive names, then ask again.
    if matches!(status, 6 | 7 | 8) {
        if let Some(check) = answer.get("sign_check").and_then(Value::as_str) {
            if let Some((start, end)) = check.split_once('-') {
                if let (Ok(start), Ok(end)) = (start.trim().parse::<u64>(), end.trim().parse::<u64>()) {
                    let length = end.saturating_sub(start).saturating_add(1).min(size);
                    let path = resolved.clone();
                    let digest = tokio::task::spawn_blocking(move || range_sha1(&path, start, length))
                        .await
                        .map_err(|_| json_error(StatusCode::BAD_REQUEST, "读取文件失败"))?
                        .map_err(|error| {
                            json_error(StatusCode::BAD_REQUEST, &format!("读取文件失败：{error}"))
                        })?;
                    let sign_key = answer
                        .get("sign_key")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    answer = init_once(
                        &config, &userkey, &userid, &target, &name, &fileid, size, sign_key,
                        &digest,
                    )
                    .await
                    .map_err(failed)?;
                    status = status_of(&answer);
                }
            }
        }
    }

    let pickcode = answer
        .get("pickcode")
        .and_then(Value::as_str)
        .filter(|code| !code.is_empty())
        .map(str::to_string);
    let hit = status == 2 && pickcode.is_some();
    Ok(Json(RapidResult {
        hit,
        need_upload: !hit,
        pickcode,
        status,
    }))
}
