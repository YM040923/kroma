//! Explicit uploads of files under a configured library folder.

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

use crate::client;
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
    resolve_folder_cid(config, cid, remote)
        .await
        .map_err(|error| json_error(StatusCode::BAD_GATEWAY, &error))
}

/// The folder a file should land in: a numeric `cid` as given, or the folder the
/// drive resolves `remote` to. Shared with the provider point, which needs the
/// same answer without axum's `Response`.
pub(crate) async fn resolve_folder_cid(
    config: &Config,
    cid: &str,
    remote: &str,
) -> Result<String, String> {
    let cid = cid.trim();
    if !cid.is_empty() && cid.bytes().all(|byte| byte.is_ascii_digit()) {
        return Ok(cid.to_string());
    }
    let remote = remote.trim();
    if remote.is_empty() {
        return Err("目录号无效".into());
    }
    client::folder_id_by_path(&config.auth_id, client::MODULE_UA, remote)
        .await
        .map_err(|error| error.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RapidResult {
    hit: bool,
    need_upload: bool,
    pickcode: Option<String>,
    status: i64,
}

pub(crate) fn status_of(data: &Value) -> i64 {
    data.get("status")
        .map(|status| {
            status
                .as_i64()
                .or_else(|| status.as_str().and_then(|text| text.parse().ok()))
                .unwrap_or(0)
        })
        .unwrap_or(0)
}

pub(crate) fn fingerprint(path: &Path) -> std::io::Result<(String, u64)> {
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
        let want = left.min(buffer.len() as u64) as usize;
        let read = file.read(&mut buffer[..want])?;
        if read == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        sha.update(&buffer[..read]);
        left -= read as u64;
    }
    Ok(hex::encode(sha.finalize()).to_uppercase())
}

/// Resolve `path` only when it sits under one of the module's selected folders.
pub(crate) fn allowed_path(raw: &str, roots: &[PathBuf]) -> Result<PathBuf, String> {
    let path = std::fs::canonicalize(raw).map_err(|error| format!("读不到这个文件：{error}"))?;
    if !path.is_file() {
        return Err("这不是一个文件".into());
    }
    let inside = roots.iter().any(|root| {
        let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.clone());
        path.starts_with(root)
    });
    if !inside {
        return Err("只能读取已选择并授权给 115 的本地文件夹".into());
    }
    Ok(path)
}

// ---------------------------------------------------------------------------
// Real upload: one part per request, so a large film is bounded by the request
// timeout and a dropped connection resumes at the part that failed.

const MIN_PART: u64 = 20 * 1024 * 1024;
const MAX_PARTS: u64 = 9000;

struct Pending {
    grant: String,
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

pub(crate) fn part_size_for(size: u64) -> u64 {
    let per_part = size.div_ceil(MAX_PARTS);
    let rounded = per_part.div_ceil(1024 * 1024) * 1024 * 1024;
    rounded.max(MIN_PART)
}

pub(crate) fn part_count(size: u64, part_size: u64) -> usize {
    size.div_ceil(part_size.max(1)).max(1) as usize
}

pub(crate) fn read_part(path: &Path, offset: u64, length: u64) -> std::io::Result<Vec<u8>> {
    let mut file = std::fs::File::open(path)?;
    file.seek(SeekFrom::Start(offset))?;
    let mut data = Vec::with_capacity(length as usize);
    file.take(length).read_to_end(&mut data)?;
    if data.len() as u64 != length {
        return Err(std::io::ErrorKind::UnexpectedEof.into());
    }
    Ok(data)
}

pub(crate) fn new_upload_id() -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_nanos())
        .unwrap_or(0)
        .hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

pub(crate) enum Init {
    Hit(String),
    Multipart {
        pickcode: String,
        bucket: String,
        object: String,
        callback: String,
        callback_var: String,
    },
}

fn field<'a>(data: &'a Value, name: &str) -> Result<&'a str, String> {
    data[name]
        .as_str()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| format!("115 上传响应缺少 {name}"))
}

fn parse_init(data: &Value) -> Result<Init, String> {
    let pickcode = field(data, "pick_code")?.to_owned();
    match status_of(data) {
        2 => Ok(Init::Hit(pickcode)),
        1 => Ok(Init::Multipart {
            pickcode,
            bucket: field(data, "bucket")?.into(),
            object: field(data, "object")?.into(),
            callback: field(&data["callback"], "callback")?.into(),
            callback_var: field(&data["callback"], "callback_var")?.into(),
        }),
        _ => Err("115 没有接受这次上传".into()),
    }
}

fn range_answer(path: &Path, size: u64, check: &str) -> Result<String, String> {
    let invalid = "115 返回无效的校验范围";
    let (first, last) = check.split_once('-').ok_or(invalid)?;
    let start = first.parse::<u64>().map_err(|_| invalid)?;
    let end = last.parse::<u64>().map_err(|_| invalid)?;
    if start > end || end >= size {
        return Err(invalid.into());
    }
    range_sha1(path, start, end - start + 1).map_err(|_| "无法读取校验范围".into())
}

pub(crate) async fn init_with_challenge(
    config: &Config,
    target: &str,
    name: &str,
    path: &Path,
    fileid: &str,
    size: u64,
) -> Result<Init, String> {
    let bytes = size.to_string();
    let mut form = vec![
        ("target", target),
        ("file_name", name),
        ("file_size", bytes.as_str()),
        ("fileid", fileid),
    ];
    let data = client::open_form(&config.auth_id, "/open/upload/init", &form)
        .await
        .map_err(|_| "115 上传初始化失败")?;
    if status_of(&data) != 7 {
        return parse_init(&data);
    }
    let key = field(&data, "sign_key")?;
    let digest = range_answer(path, size, field(&data, "sign_check")?)?;
    form.extend([("sign_key", key), ("sign_val", digest.as_str())]);
    if let Some(code) = data["pick_code"].as_str().filter(|v| !v.is_empty()) {
        form.push(("pick_code", code));
    }
    let answer = client::open_form(&config.auth_id, "/open/upload/init", &form)
        .await
        .map_err(|_| "115 二次认证失败")?;
    parse_init(&answer)
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
    if config.auth_id.trim().is_empty() {
        return Err(json_error(StatusCode::SERVICE_UNAVAILABLE, "115 尚未授权"));
    }
    let cid = folder_cid(&config, &body.cid, &body.remote).await?;
    let roots: Vec<PathBuf> = config
        .libraries
        .iter()
        .filter_map(|library| crate::sync::resolve_local_path(&[], &library.local))
        .collect();
    let raw = body.path.trim().to_string();
    let resolved =
        allowed_path(&raw, &roots).map_err(|error| json_error(StatusCode::BAD_REQUEST, &error))?;
    let (fileid, size) = fingerprint(&resolved)
        .map_err(|error| json_error(StatusCode::BAD_REQUEST, &format!("读取文件失败：{error}")))?;
    let name = resolved
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("upload.bin")
        .to_string();
    let target = format!("U_1_{cid}");
    let init = init_with_challenge(&config, &target, &name, &resolved, &fileid, size)
        .await
        .map_err(|err| json_error(StatusCode::BAD_GATEWAY, &err))?;
    let (pickcode, bucket, object, callback, callback_var) = match init {
        Init::Hit(code) => {
            return Ok(Json(StartResult {
                hit: true,
                upload: None,
                parts: 0,
                part_size: 0,
                pickcode: Some(code),
            }))
        }
        Init::Multipart {
            pickcode,
            bucket,
            object,
            callback,
            callback_var,
        } => (pickcode, bucket, object, callback, callback_var),
    };
    let upload_id = crate::oss::start(&config.auth_id, &bucket, &object)
        .await
        .map_err(|err| json_error(StatusCode::BAD_GATEWAY, &err))?;
    let part_size = part_size_for(size);
    let parts = part_count(size, part_size);
    let upload = new_upload_id();
    pending()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(
            upload.clone(),
            Pending {
                grant: config.auth_id,
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
    let config = Config::load();
    let (path, bucket, object, upload_id, part_size, size, index) = {
        let all = pending()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entry = all
            .get(&body.upload)
            .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "这次上传已经结束"))?;
        if config.auth_id != entry.grant {
            return Err(json_error(StatusCode::FORBIDDEN, "上传授权已更改"));
        }
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
    let roots = config
        .libraries
        .iter()
        .filter_map(|lib| crate::sync::resolve_local_path(&[], &lib.local))
        .collect::<Vec<_>>();
    let path = allowed_path(path.to_str().unwrap_or(""), &roots)
        .map_err(|err| json_error(StatusCode::BAD_REQUEST, &err))?;
    let data = read_part(&path, offset, length)
        .map_err(|error| json_error(StatusCode::BAD_REQUEST, &format!("读取分片失败：{error}")))?;
    let etag = crate::oss::put_part(
        &config.auth_id,
        &bucket,
        &object,
        &upload_id,
        index as u32 + 1,
        &data,
    )
    .await
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
    if Config::load().auth_id != entry.grant {
        return Err(json_error(StatusCode::FORBIDDEN, "上传授权已更改"));
    }
    crate::oss::complete(
        &entry.grant,
        &entry.bucket,
        &entry.object,
        &entry.upload_id,
        &etags,
        &entry.callback,
        &entry.callback_var,
    )
    .await
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
    if config.auth_id.trim().is_empty() {
        return Err(json_error(StatusCode::SERVICE_UNAVAILABLE, "115 尚未授权"));
    }
    let cid = folder_cid(&config, &body.cid, &body.remote).await?;
    let roots: Vec<PathBuf> = config
        .libraries
        .iter()
        .filter_map(|library| crate::sync::resolve_local_path(&[], &library.local))
        .collect();
    let raw = body.path.trim().to_string();
    let resolved =
        allowed_path(&raw, &roots).map_err(|error| json_error(StatusCode::BAD_REQUEST, &error))?;
    let (fileid, size) = fingerprint(&resolved)
        .map_err(|error| json_error(StatusCode::BAD_REQUEST, &format!("读取文件失败：{error}")))?;
    let name = resolved
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("upload.bin")
        .to_string();
    let target = format!("U_1_{cid}");
    let init = init_with_challenge(&config, &target, &name, &resolved, &fileid, size)
        .await
        .map_err(|err| json_error(StatusCode::BAD_GATEWAY, &err))?;
    let (hit, pickcode, status) = match init {
        Init::Hit(code) => (true, code, 2),
        Init::Multipart { pickcode, .. } => (false, pickcode, 1),
    };
    Ok(Json(RapidResult {
        hit,
        need_upload: !hit,
        pickcode: Some(pickcode),
        status,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn official_init_data_distinguishes_hit_from_multipart() {
        let hit =
            json!({"state": true,"code":0,"data":{"status":2,"pick_code":"pc","file_id":"1"}});
        assert!(matches!(parse_init(&hit["data"]),Ok(Init::Hit(pc)) if pc == "pc"));
        let data = json!({"status":1,"pick_code":"pc","bucket":"example","object":"k","callback":{"callback":"cb","callback_var":"vars"}});
        assert!(
            matches!(parse_init(&data),Ok(Init::Multipart{object,callback_var,..}) if object=="k" && callback_var=="vars")
        );
        assert!(parse_init(&json!({"status":1,"pick_code":"pc"})).is_err());
        assert!(parse_init(&json!({"status":2,"pickcode":"legacy"})).is_err());
    }

    #[test]
    fn official_challenge_hashes_both_ends_and_rejects_short_reads() {
        let challenge = json!({"state":false,"code":701,"data":{"status":7,"sign_key":"key","sign_check":"1-3"}});
        let dir = kroma_module_sdk::testing::temp_dir("cloud115-range");
        let file = dir.path().join("input");
        std::fs::write(&file, b"XabcY").unwrap();
        let data = &challenge["data"];
        assert_eq!(status_of(data), 7);
        assert_eq!(
            range_answer(&file, 5, field(data, "sign_check").unwrap()).unwrap(),
            "A9993E364706816ABA3E25717850C26C9CD0D89D"
        );
        for invalid in ["2-1", "0-5", "1", "1-18446744073709551615"] {
            assert!(range_answer(&file, 5, invalid).is_err());
        }
        assert!(range_answer(&file, 6, "4-5").is_err());
    }
}
