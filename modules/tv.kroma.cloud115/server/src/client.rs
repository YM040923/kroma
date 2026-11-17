//! The drive's own HTTP API, as far as this module needs it.
//!
//! Only four write endpoints exist here, and none is called unless an operator
//! turns tidying on: the drive weighs a write far more heavily than a read, and
//! an account that gets flagged takes the library with it.
//!
//! Every call goes through the module's one [`pacer`](crate::pacer::pacer),
//! including the ones a player triggers, so a running mirror cannot slip a burst
//! past the drive by running beside playback.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use kroma_module_sdk::http::Fetch;

use crate::crypto;
use crate::pacer::pacer;

pub const WEB: &str = "https://webapi.115.com";
pub const PRO: &str = "https://proapi.115.com";

/// What the module calls itself to the drive. A player's own user agent is used
/// instead wherever a link is signed for that player: a 115 link is bound to the
/// agent that asked for it.
pub const MODULE_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

const TIMEOUT_SECS: u32 = 30;

/// One failed call, with the two numbers that decide what to do about it: the
/// HTTP status and the drive's errno.
#[derive(Debug, Clone)]
pub struct ApiError {
    pub http: u16,
    pub errno: i64,
    pub message: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.errno != 0 {
            write!(f, "{}（errno {}）", self.message, self.errno)
        } else if self.http != 0 {
            write!(f, "{}（HTTP {}）", self.message, self.http)
        } else {
            write!(f, "{}", self.message)
        }
    }
}

impl ApiError {
    fn network(message: impl Into<String>) -> Self {
        Self {
            http: 0,
            errno: 0,
            message: message.into(),
        }
    }

    /// Whether the drive is telling this module to slow down, rather than
    /// refusing the request itself. The caller keeps the message; the pacer has
    /// already parked every call.
    pub fn rate_limited(&self) -> bool {
        matches!(self.http, 405 | 429 | 430 | 599) || matches!(self.errno, 10008 | 10019 | 590075)
    }

    pub fn cookie_expired(&self) -> bool {
        matches!(self.errno, 99 | 990001 | 401) || self.http == 401 || self.http == 403
    }
}

/// The envelope every endpoint answers with. `id` and `count` sit at the top
/// level on the endpoints that use them (rather than inside `data`), so both are
/// kept here.
#[derive(Debug, Deserialize, Default)]
struct Envelope {
    #[serde(default)]
    state: bool,
    #[serde(default)]
    errno: i64,
    #[serde(default)]
    error: String,
    #[serde(default)]
    errmsg: String,
    #[serde(default)]
    id: Value,
    #[serde(default)]
    count: i64,
    #[serde(default)]
    data: Value,
}

/// Check a status and envelope together. A `state:false` is an error and never
/// an empty result: reading it as "this folder is empty" is how a mirror deletes
/// a library over a rate limit.
fn check(status: u16, body: &str) -> Result<Envelope, ApiError> {
    let envelope: Envelope = serde_json::from_str(body).map_err(|_| ApiError {
        http: status,
        errno: 0,
        message: format!("无法解析响应：{}", snippet(body)),
    })?;
    if !(200..300).contains(&status) {
        return Err(ApiError {
            http: status,
            errno: envelope.errno,
            message: first(&[
                envelope.error.as_str(),
                envelope.errmsg.as_str(),
                "请求被拒绝",
            ]),
        });
    }
    if !envelope.state {
        return Err(ApiError {
            http: status,
            errno: envelope.errno,
            message: first(&[
                envelope.error.as_str(),
                envelope.errmsg.as_str(),
                "接口返回 state=false",
            ]),
        });
    }
    Ok(envelope)
}

fn first(values: &[&str]) -> String {
    values
        .iter()
        .find(|v| !v.trim().is_empty())
        .map(|v| v.trim().to_string())
        .unwrap_or_else(|| "未说明原因".into())
}

fn snippet(body: &str) -> String {
    body.trim().chars().take(160).collect()
}

fn call(method: &str, url: &str, cookie: &str, ua: &str, form: Option<&[(&str, &str)]>) -> Result<(u16, String), ApiError> {
    let request = Fetch::default()
        .purpose("cloud-drive")
        .header("Cookie", cookie)
        .header("User-Agent", ua)
        .header("Referer", "https://115.com/")
        .header("Origin", "https://115.com")
        .max_time(TIMEOUT_SECS);
    let response = match (method, form) {
        ("POST", Some(fields)) => request.post_form(url, fields),
        _ => request.get(url),
    };
    match response {
        Ok(response) => Ok((response.status, response.text())),
        Err(error) => Err(ApiError::network(error.to_string())),
    }
}

/// One entry in a listing. The drive's JSON uses single letters, so the mapping
/// is here rather than on the type.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Entry {
    #[serde(default, rename = "fid")]
    file_id: Value,
    #[serde(default, rename = "cid")]
    folder_id: Value,
    #[serde(default, rename = "pc")]
    pub pick_code: String,
    #[serde(default, rename = "n")]
    pub name: String,
    #[serde(default, rename = "s")]
    pub size: i64,
}

impl Entry {
    /// A file carries a `fid`; a folder carries a `cid` instead.
    pub fn is_dir(&self) -> bool {
        id_of(&self.file_id).is_none() && id_of(&self.folder_id).is_some()
    }

    pub fn file_id(&self) -> Option<String> {
        id_of(&self.file_id)
    }

    pub fn folder_id(&self) -> Option<String> {
        id_of(&self.folder_id)
    }
}

fn id_of(value: &Value) -> Option<String> {
    match value {
        Value::String(text) if !text.is_empty() && text != "0" => Some(text.clone()),
        Value::Number(number) => number.as_i64().filter(|n| *n > 0).map(|n| n.to_string()),
        _ => None,
    }
}

/// One page of a folder.
#[derive(Debug, Clone, Default)]
pub struct Page {
    pub entries: Vec<Entry>,
    /// What the drive says the folder holds, which is how a page loop knows
    /// whether it has read it all when the last page came back short.
    pub total: i64,
    /// The folder's own chain of ancestors, when the drive sent it.
    pub path: Vec<(String, String)>,
}

/// The account's storage totals, as the drive reports them.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Space {
    pub total: u64,
    pub used: u64,
    pub remain: u64,
    pub total_text: String,
    pub used_text: String,
    pub remain_text: String,
}

pub async fn space(cookie: &str, ua: &str) -> Result<Space, ApiError> {
    pacer().acquire().await.map_err(ApiError::network)?;
    let (status, body) = call("GET", &format!("{WEB}/user/space"), cookie, ua, None)?;
    pacer().note(status, errno_of(&body)).await;
    let envelope = check(status, &body)?;
    let pick = |path: &[&str]| -> (u64, String) {
        let mut value = &envelope.data;
        for part in path {
            value = value.get(*part).unwrap_or(&Value::Null);
        }
        let size = value.get("size").and_then(Value::as_u64).unwrap_or(0);
        let text = value
            .get("size_format")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        (size, text)
    };
    let (total, total_text) = pick(&["all_total"]);
    let (used, used_text) = pick(&["all_use"]);
    let (remain, remain_text) = pick(&["all_remain"]);
    Ok(Space {
        total,
        used,
        remain,
        total_text,
        used_text,
        remain_text,
    })
}

/// List one page of a folder. `cid` is `"0"` for the root.
pub async fn list(cookie: &str, ua: &str, cid: &str, offset: u32, limit: u32) -> Result<Page, ApiError> {
    pacer().acquire().await.map_err(ApiError::network)?;
    let url = format!(
        "{WEB}/files?aid=1&cid={cid}&o=user_ptime&asc=0&offset={offset}&show_dir=1\
         &limit={limit}&code=&scid=&snap=0&natsort=1&record_open_time=1&source=&format=json&virtual=1"
    );
    let (status, body) = call("GET", &url, cookie, ua, None)?;
    pacer().note(status, errno_of(&body)).await;
    let envelope = check(status, &body)?;
    let entries = serde_json::from_value::<Vec<Entry>>(envelope.data.clone()).unwrap_or_default();
    let total = envelope.count.max(entries.len() as i64);
    Ok(Page {
        entries,
        total,
        path: Vec::new(),
    })
}

/// Resolve a path like `/电影` to its folder id. The drive's root is `0`, which
/// the lookup endpoint does not answer for, so it is answered here.
pub async fn folder_id_by_path(cookie: &str, ua: &str, path: &str) -> Result<String, ApiError> {
    if path.trim().is_empty() || path.trim() == "/" {
        return Ok("0".to_string());
    }
    pacer().acquire().await.map_err(ApiError::network)?;
    let encoded: String = form_encode(path);
    let url = format!("{WEB}/files/getid?path={encoded}");
    let (status, body) = call("GET", &url, cookie, ua, None)?;
    pacer().note(status, errno_of(&body)).await;
    let envelope = check(status, &body)?;
    let from_data = |key: &str| id_of(envelope.data.get(key).unwrap_or(&Value::Null));
    id_of(&envelope.id)
        .or_else(|| from_data("id"))
        .or_else(|| from_data("cid"))
        .or_else(|| id_of(&envelope.data))
        .ok_or_else(|| ApiError {
            http: status,
            errno: envelope.errno,
            message: "path does not resolve to a folder".into(),
        })
}

/// A link the drive signed, and whether a plain redirect can use it.
#[derive(Debug, Clone)]
pub struct DownloadLink {
    pub url: String,
    pub size: i64,
    pub name: String,
    /// The CDN wants the account's cookie on every fetch (`f=3` links, and what
    /// a BluRay image tends to get). A redirect cannot carry one, so the caller
    /// must say so rather than hand out a link that fails on the first byte.
    pub needs_cookie: bool,
}

/// Sign a download link for one file. `ua` must be the agent that will fetch it:
/// the drive binds the link to whoever asked.
pub async fn sign(cookie: &str, ua: &str, pick_code: &str) -> Result<DownloadLink, ApiError> {
    let pick_code = pick_code.trim().to_ascii_lowercase();
    if pick_code.is_empty() {
        return Err(ApiError::network("文件没有 pickcode"));
    }
    pacer().acquire().await.map_err(ApiError::network)?;
    let payload = format!(r#"{{"pick_code":"{pick_code}"}}"#);
    let sealed = crypto::encrypt(payload.as_bytes()).map_err(ApiError::network)?;
    let url = format!("{PRO}/android/2.0/ufile/download");
    let (status, body) = call("POST", &url, cookie, ua, Some(&[("data", &sealed)]))?;
    pacer().note(status, errno_of(&body)).await;
    let envelope = check(status, &body)?;
    let sealed_answer = envelope
        .data
        .as_str()
        .ok_or_else(|| ApiError {
            http: status,
            errno: envelope.errno,
            message: "the drive answered without a sealed payload".into(),
        })?
        .to_string();
    let plain = crypto::decrypt(&sealed_answer).map_err(|e| ApiError {
        http: status,
        errno: 0,
        message: format!("解开直链响应失败：{e}"),
    })?;
    let answer: SignAnswer = serde_json::from_str(&plain).map_err(|_| ApiError {
        http: status,
        errno: 0,
        message: format!("直链响应不是预期的 JSON：{}", snippet(&plain)),
    })?;
    let link = answer.into_link().ok_or_else(|| ApiError {
        http: status,
        errno: 0,
        message: "the drive returned no url".into(),
    })?;
    Ok(link)
}

#[derive(Debug, Deserialize, Default)]
struct SignAnswer {
    #[serde(default)]
    url: String,
    #[serde(default)]
    file_size: Value,
    #[serde(default)]
    file_name: String,
    #[serde(default, rename = "fileName")]
    file_name_camel: String,
}

impl SignAnswer {
    fn into_link(self) -> Option<DownloadLink> {
        if self.url.is_empty() {
            return None;
        }
        let needs_cookie = query_flag(&self.url, "f").as_deref() == Some("3");
        let size = match &self.file_size {
            Value::Number(n) => n.as_i64().unwrap_or(0),
            Value::String(text) => text.parse().unwrap_or(0),
            _ => 0,
        };
        Some(DownloadLink {
            needs_cookie,
            url: self.url,
            size,
            name: if self.file_name.is_empty() {
                self.file_name_camel
            } else {
                self.file_name
            },
        })
    }
}

/// One query parameter out of a URL, without pulling in a URL parser.
fn query_flag(url: &str, key: &str) -> Option<String> {
    let query = url.split_once('?')?.1.split('#').next()?;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        if k == key {
            return Some(v.to_string());
        }
    }
    None
}

/// The errno inside an envelope, for the pacer, which must see a refusal even
/// when the body is about to fail parsing.
fn errno_of(body: &str) -> i64 {
    serde_json::from_str::<Envelope>(body)
        .map(|e| e.errno)
        .unwrap_or(0)
}

/// `application/x-www-form-urlencoded`, which the drive's path lookup wants.
fn form_encode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() * 3);
    for byte in raw.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            b' ' => out.push('+'),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Writes. Every one goes through `acquire_write`, which holds a much longer
// floor than a read: the drive weighs a change to the account more heavily than
// a listing, and a burst of renames is the shape of a script.

/// Create a folder under `parent_id` and answer its id.
pub async fn mkdir(cookie: &str, ua: &str, parent_id: &str, name: &str) -> Result<String, ApiError> {
    pacer().acquire_write().await.map_err(ApiError::network)?;
    let url = format!("{WEB}/files/add");
    let (status, body) = call("POST", &url, cookie, ua, Some(&[("pid", parent_id), ("cname", name)]))?;
    pacer().note(status, errno_of(&body)).await;
    let envelope = check(status, &body)?;
    let from_data = |key: &str| id_of(envelope.data.get(key).unwrap_or(&Value::Null));
    id_of(&envelope.id)
        .or_else(|| from_data("cid"))
        .or_else(|| from_data("id"))
        .or_else(|| id_of(&envelope.data))
        .ok_or_else(|| ApiError {
            http: status,
            errno: envelope.errno,
            message: "the drive created the folder but named no id".into(),
        })
}

/// Move files and folders into `dest_id`. The ids travel as `fid[0]`, `fid[1]`…
pub async fn move_to(cookie: &str, ua: &str, ids: &[String], dest_id: &str) -> Result<(), ApiError> {
    if ids.is_empty() {
        return Ok(());
    }
    pacer().acquire_write().await.map_err(ApiError::network)?;
    let fields = keyed_fields("fid", ids, &[("pid", dest_id)]);
    let url = format!("{WEB}/files/move");
    let (status, body) = call("POST", &url, cookie, ua, Some(&borrowed(&fields)))?;
    pacer().note(status, errno_of(&body)).await;
    check(status, &body).map(drop)
}

/// Rename one entry.
///
/// Both spellings travel: the newer `files_new_name[<fid>]` and the legacy
/// `file_name`. The web API has accepted the first and the older app the second,
/// and sending both is what OpenList's driver settled on against this endpoint.
pub async fn rename(cookie: &str, ua: &str, id: &str, new_name: &str) -> Result<(), ApiError> {
    pacer().acquire_write().await.map_err(ApiError::network)?;
    let key = format!("files_new_name[{id}]");
    let fields = vec![
        ("fid".to_string(), id.to_string()),
        ("file_name".to_string(), new_name.to_string()),
        (key, new_name.to_string()),
    ];
    let url = format!("{WEB}/files/batch_rename");
    let (status, body) = call("POST", &url, cookie, ua, Some(&borrowed(&fields)))?;
    pacer().note(status, errno_of(&body)).await;
    check(status, &body).map(drop)
}

/// Delete entries into the drive's recycle bin (never a hard delete).
pub async fn remove(cookie: &str, ua: &str, ids: &[String]) -> Result<(), ApiError> {
    if ids.is_empty() {
        return Ok(());
    }
    pacer().acquire_write().await.map_err(ApiError::network)?;
    let fields = keyed_fields("fid", ids, &[]);
    let url = format!("{WEB}/rb/delete");
    let (status, body) = call("POST", &url, cookie, ua, Some(&borrowed(&fields)))?;
    pacer().note(status, errno_of(&body)).await;
    check(status, &body).map(drop)
}

/// `extra` first, then `name[0]=…`, `name[1]=…` in order.
fn keyed_fields(name: &str, ids: &[String], extra: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut fields: Vec<(String, String)> = extra
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    for (at, id) in ids.iter().enumerate() {
        fields.push((format!("{name}[{at}]"), id.clone()));
    }
    fields
}

fn borrowed(fields: &[(String, String)]) -> Vec<(&str, &str)> {
    fields
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_state_false_answer_is_an_error_not_an_empty_folder() {
        // The rule the whole mirror leans on: a refusal must never read as "this
        // folder has nothing in it", or the next prune deletes the library.
        let body = r#"{"state":false,"errno":990001,"error":"登录超时"}"#;
        let error = check(200, body).expect_err("state=false is a failure");
        assert!(error.cookie_expired());
        assert!(error.message.contains("登录超时"));
    }

    #[test]
    fn a_rate_limit_answer_is_recognised_so_every_call_parks() {
        let body = r#"{"state":false,"errno":590075,"error":"操作太频繁"}"#;
        let error = check(200, body).expect_err("refused");
        assert!(error.rate_limited());
        let throttled = check(405, r#"{"state":true,"data":{}}"#).expect_err("405 is refused");
        assert!(throttled.rate_limited());
    }

    #[test]
    fn entries_tell_files_from_folders() {
        let file: Entry = serde_json::from_str(r#"{"fid":"12","pc":"abc","n":"A.mkv","s":9}"#).unwrap();
        assert!(!file.is_dir());
        assert_eq!(file.file_id().as_deref(), Some("12"));
        let folder: Entry = serde_json::from_str(r#"{"cid":"34","n":"电影","fc":3}"#).unwrap();
        assert!(folder.is_dir());
        assert_eq!(folder.folder_id().as_deref(), Some("34"));
    }

    #[test]
    fn a_link_the_cdn_guards_per_fetch_is_flagged() {
        let answer: SignAnswer =
            serde_json::from_str(r#"{"url":"https://cdn.test/a.iso?f=3&t=1","file_size":"99"}"#)
                .unwrap();
        let link = answer.into_link().expect("a url");
        assert!(link.needs_cookie);
        assert_eq!(link.size, 99);

        let plain: SignAnswer =
            serde_json::from_str(r#"{"url":"https://cdn.test/a.mkv?t=1","file_size":5}"#).unwrap();
        assert!(!plain.into_link().unwrap().needs_cookie);
    }

    #[test]
    fn a_path_is_form_encoded_for_the_lookup() {
        assert_eq!(form_encode("/电影 A"), "%2F%E7%94%B5%E5%BD%B1+A");
    }
}
