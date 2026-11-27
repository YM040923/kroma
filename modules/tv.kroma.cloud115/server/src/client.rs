//! 115 Open API. Callers pass grant IDs; only the fixed-origin transport sees a bearer.
use crate::pacer::pacer;
use kroma_module_sdk::http::Fetch;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

const ORIGIN: &str = "https://proapi.115.com";
pub const MODULE_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

#[derive(Debug, Clone)]
pub struct ApiError {
    pub http: u16,
    pub errno: i64,
    pub message: String,
}
impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)?;
        if self.errno != 0 {
            write!(f, "（errno {}）", self.errno)
        } else if self.http != 0 {
            write!(f, "（HTTP {}）", self.http)
        } else {
            Ok(())
        }
    }
}
impl ApiError {
    pub(crate) fn network(message: impl Into<String>) -> Self {
        Self {
            http: 0,
            errno: 0,
            message: message.into(),
        }
    }
    pub fn rate_limited(&self) -> bool {
        matches!(self.http, 405 | 429 | 430 | 599)
            || matches!(self.errno, 10008 | 10019 | 590075 | 40140117)
    }
    pub fn auth_expired(&self) -> bool {
        matches!(self.errno, 40140116 | 40140119 | 40140125)
    }
    pub(crate) fn sync_reason(&self) -> String {
        let message = if self.rate_limited() {
            "115 限流"
        } else if self.auth_expired() || matches!(self.http, 401 | 403) {
            "115 登录或访问权限被拒绝"
        } else if self.message.contains("授权") {
            return self.message.clone(); // Localized OAuth errors contain only fixed text and numeric status codes.
        } else if self.message.contains("调用上限") {
            "达到调用上限"
        } else if self.message == "path does not resolve to a folder" {
            "网盘目录不存在或不可访问"
        } else if self.message.contains("响应") {
            "115 响应格式不正确"
        } else if self.http == 0 {
            "无法连接到 115"
        } else {
            "115 拒绝请求"
        };
        Self {
            message: message.into(),
            ..self.clone()
        }
        .to_string()
    }
}
fn invalid() -> ApiError {
    ApiError {
        http: 200,
        errno: 0,
        message: "115 响应格式不正确".into(),
    }
}
fn integer(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| v.as_str()?.parse().ok())
}
fn valid_id(s: &str) -> bool {
    s == "0" || (!s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit()))
}
fn directory_id(v: &Value) -> Option<String> {
    if let Some(s) = v.as_str() {
        valid_id(s).then(|| s.to_owned())
    } else {
        v.as_u64().map(|n| n.to_string())
    }
}
fn id_of(v: &Value) -> Option<String> {
    directory_id(v).filter(|id| id != "0")
}
fn approved(path: &str, post: bool) -> bool {
    matches!(
        (post, path),
        (
            false,
            "/open/ufile/files" | "/open/user/info" | "/open/upload/get_token"
        ) | (
            true,
            "/open/folder/add"
                | "/open/ufile/downurl"
                | "/open/ufile/move"
                | "/open/ufile/update"
                | "/open/ufile/delete"
                | "/open/upload/init"
                | "/open/upload/resume"
        )
    )
}
fn check(status: u16, v: Value, path: &str) -> Result<Value, ApiError> {
    let code = integer(&v["code"]);
    // Only the documented init challenge is data, not an instruction to retry a write.
    let challenge = path == "/open/upload/init"
        && code == Some(701)
        && integer(&v["data"]["status"]) == Some(7)
        && v["data"]["sign_key"].is_string()
        && v["data"]["sign_check"].is_string();
    if (200..300).contains(&status)
        && (((v["state"] == true || integer(&v["state"]) == Some(1))
            && v.get("code").is_none_or(|_| code == Some(0)))
            || challenge)
        && v.get("data").is_some()
    {
        return Ok(v);
    }
    Err(ApiError {
        http: status,
        errno: code.unwrap_or(0),
        message: if v.is_object() {
            "115 拒绝请求"
        } else {
            "无法解析 115 响应"
        }
        .into(),
    })
}
async fn request(
    grant: &str,
    ua: &str,
    path: &str,
    query: &[(&str, &str)],
    form: Option<&[(&str, &str)]>,
) -> Result<Value, ApiError> {
    if !approved(path, form.is_some()) || ua.chars().any(char::is_control) {
        return Err(ApiError::network("无效的 115 请求"));
    }
    let mut token = crate::login::token(grant).await.map_err(ApiError::network)?;
    let mut refreshed = false;
    loop {
        if token.is_empty() || !token.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(ApiError::network("无效的 115 授权"));
        }
        if form.is_some() && path != "/open/ufile/downurl" {
            pacer().acquire_write().await
        } else {
            pacer().acquire().await
        }
        .map_err(ApiError::network)?;
        let mut req = Fetch::default()
            .purpose("cloud-drive")
            .no_redirects()
            .https_only()
            .max_time(30)
            .header("Authorization", format!("Bearer {token}"))
            .header("User-Agent", ua);
        for &(k, v) in query {
            req = req.query(k, v);
        }
        let url = format!("{ORIGIN}{path}");
        let res = match form {
            Some(fields) => req.post_form(&url, fields),
            None => req.get(&url),
        }
        .map_err(|_| ApiError::network("无法连接到 115"))?;
        let body = serde_json::from_slice(&res.body).unwrap_or(Value::Null);
        let code = integer(&body["code"]).unwrap_or(0);
        pacer().note(res.status, code);
        // 115 specifically documents 40140125 as recoverable by refreshing the access token.
        // Only re-issue reads and signing, never an ambiguous write or a rate-limited request.
        if code == 40140125
            && !matches!(res.status, 405 | 429 | 430 | 599)
            && !refreshed
            && (form.is_none() || path == "/open/ufile/downurl")
        {
            token = crate::login::renew(grant, &token).await.map_err(ApiError::network)?;
            refreshed = true;
            continue;
        }
        return check(res.status, body, path);
    }
}
pub(crate) async fn open_form(
    grant: &str,
    path: &str,
    fields: &[(&str, &str)],
) -> Result<Value, ApiError> {
    data(request(grant, MODULE_UA, path, &[], Some(fields)).await?)
}
pub(crate) async fn open_get(grant: &str, path: &str) -> Result<Value, ApiError> {
    data(request(grant, MODULE_UA, path, &[], None).await?)
}
fn data(mut v: Value) -> Result<Value, ApiError> {
    v.get_mut("data").map(Value::take).ok_or_else(invalid)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Entry {
    fid: Value,
    pid: Value,
    fc: Value,
    #[serde(rename = "fn")]
    pub name: String,
    #[serde(default, rename = "pc")]
    pub pick_code: String,
    #[serde(default, rename = "fs", deserialize_with = "wire_size")]
    pub size: i64,
}
fn wire_size<'de, D: serde::Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
    let v = Value::deserialize(d)?;
    integer(&v)
        .filter(|n| *n >= 0)
        .ok_or_else(|| serde::de::Error::custom("invalid file size"))
}
impl Entry {
    pub fn is_dir(&self) -> bool {
        integer(&self.fc) == Some(0)
    }
    pub fn parent_id(&self) -> Option<String> {
        directory_id(&self.pid)
    }
    pub fn file_id(&self) -> Option<String> {
        id_of(&self.fid).filter(|_| !self.is_dir())
    }
    pub fn folder_id(&self) -> Option<String> {
        id_of(&self.fid).filter(|_| self.is_dir())
    }
}
#[derive(Debug, Clone, Default)]
pub struct Page {
    pub entries: Vec<Entry>,
    pub total: i64,
    pub path: Vec<(String, String)>,
}
fn path_chain(rows: &Value, cid: &str) -> Result<Vec<(String, String)>, ApiError> {
    let mut path = Vec::new();
    let mut seen = HashSet::new();
    for row in rows.as_array().ok_or_else(invalid)? {
        let id = directory_id(&row["cid"]).ok_or_else(invalid)?;
        let name = row["name"].as_str().ok_or_else(invalid)?;
        if let Some((previous, _)) = path.last() {
            if directory_id(&row["pid"]).as_ref() != Some(previous) || name.is_empty() {
                return Err(invalid());
            }
        } else if id != "0" {
            return Err(invalid());
        }
        if !seen.insert(id.clone()) {
            return Err(invalid());
        }
        path.push((id, name.to_owned()));
    }
    if path.last().map(|(id, _)| id.as_str()) != Some(cid) {
        return Err(invalid());
    }
    Ok(path)
}
fn parse_page(
    mut v: Value,
    cid: &str,
    offset: u32,
    limit: u32,
    videos: bool,
) -> Result<Page, ApiError> {
    let total = integer(&v["count"])
        .filter(|&n| n >= 0)
        .ok_or_else(invalid)?;
    if v.get("offset")
        .is_some_and(|n| integer(n) != Some(offset.into()))
        || v.get("cid")
            .is_some_and(|n| directory_id(n).as_deref() != Some(cid))
    {
        return Err(invalid());
    }
    let rows = v["data"].as_array_mut().ok_or_else(invalid)?;
    let end = i64::from(offset) + rows.len() as i64;
    if rows.len() > limit as usize
        || (rows.is_empty() && end < total)
        || (!rows.is_empty() && end > total)
    {
        return Err(invalid());
    }
    let mut entries = Vec::with_capacity(rows.len());
    let mut seen = HashSet::new();
    for row in rows {
        // Folder rows can omit file-only fields; a file without a byte count
        // must not be silently persisted as a zero-length file.
        if integer(&row["fc"]) == Some(1) && row.get("fs").is_none() {
            return Err(invalid());
        }
        let e: Entry = serde_json::from_value(row.take()).map_err(|_| invalid())?;
        if !matches!(integer(&e.fc), Some(0 | 1))
            || e.name.is_empty()
            || e.size < 0
            || !seen.insert(id_of(&e.fid).ok_or_else(invalid)?)
        {
            return Err(invalid());
        }
        let parent = e.parent_id().ok_or_else(invalid)?;
        if (videos && e.is_dir()) || (!videos && parent != cid) {
            return Err(invalid());
        }
        entries.push(e);
    }
    let path = v
        .get("path")
        .map(|p| path_chain(p, cid))
        .transpose()?
        .unwrap_or_default();
    Ok(Page {
        entries,
        total,
        path,
    })
}
async fn listing(
    grant: &str,
    ua: &str,
    cid: &str,
    offset: u32,
    limit: u32,
    videos: bool,
) -> Result<Page, ApiError> {
    if !valid_id(cid) || limit == 0 {
        return Err(ApiError::network("无效的目录查询"));
    }
    let limit = limit.min(1150);
    let off = offset.to_string();
    let lim = limit.to_string();
    let mut q = vec![
        ("cid", cid),
        ("limit", lim.as_str()),
        ("offset", off.as_str()),
        ("custom_order", "1"),
    ];
    if videos {
        q.extend([
            ("cur", "0"),
            ("type", "4"),
            ("show_dir", "0"),
            ("stdir", "0"),
            ("o", "user_utime"),
            ("asc", "0"),
        ])
    } else {
        q.extend([
            ("cur", "1"),
            ("show_dir", "1"),
            ("o", "file_name"),
            ("asc", "1"),
        ])
    }
    parse_page(
        request(grant, ua, "/open/ufile/files", &q, None).await?,
        cid,
        offset,
        limit,
        videos,
    )
}
pub async fn list(
    grant: &str,
    ua: &str,
    cid: &str,
    offset: u32,
    limit: u32,
) -> Result<Page, ApiError> {
    listing(grant, ua, cid, offset, limit, false).await
}
pub async fn list_videos(
    grant: &str,
    cid: &str,
    offset: u32,
    limit: u32,
) -> Result<Page, ApiError> {
    listing(grant, MODULE_UA, cid, offset, limit, true).await
}
pub async fn directory_path(grant: &str, cid: &str) -> Result<Vec<(String, String)>, ApiError> {
    let path = list(grant, MODULE_UA, cid, 0, 1).await?.path;
    if path.is_empty() {
        Err(invalid())
    } else {
        Ok(path)
    }
}
pub async fn folder_id_by_path(grant: &str, ua: &str, path: &str) -> Result<String, ApiError> {
    let mut cid = "0".to_owned();
    for name in path.split('/').filter(|s| !s.is_empty()) {
        let mut offset = 0;
        let mut found = None;
        loop {
            let page = list(grant, ua, &cid, offset, 1150).await?;
            for e in &page.entries {
                if e.is_dir()
                    && e.name == name
                    && found.replace(e.folder_id().ok_or_else(invalid)?).is_some()
                {
                    return Err(invalid());
                }
            }
            offset = offset
                .checked_add(page.entries.len() as u32)
                .ok_or_else(invalid)?;
            if i64::from(offset) >= page.total {
                break;
            }
        }
        cid = found.ok_or_else(|| ApiError::network("path does not resolve to a folder"))?;
    }
    Ok(cid)
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadLink {
    pub url: String,
    pub size: i64,
    pub name: String,
    pub needs_cookie: bool,
}
fn link(data: &Value, pc: &str) -> Result<DownloadLink, ApiError> {
    let obj = data
        .as_object()
        .filter(|obj| obj.len() == 1)
        .ok_or_else(invalid)?;
    let (id, v) = obj.iter().next().ok_or_else(invalid)?;
    if !valid_id(id) || id == "0" || v["pick_code"].as_str() != Some(pc) {
        return Err(invalid());
    }
    let url = v["url"]["url"].as_str().ok_or_else(invalid)?;
    let uri = url.parse::<axum::http::Uri>().map_err(|_| invalid())?;
    if !matches!(uri.scheme_str(), Some("https" | "http"))
        || uri.host().is_none()
        || uri.authority().is_some_and(|a| a.as_str().contains('@'))
    {
        return Err(invalid());
    }
    Ok(DownloadLink {
        url: url.into(),
        size: integer(&v["file_size"])
            .filter(|&s| s >= 0)
            .ok_or_else(invalid)?,
        name: v["file_name"].as_str().ok_or_else(invalid)?.into(),
        needs_cookie: uri
            .query()
            .is_some_and(|q| q.split('&').any(|p| p == "f=3")),
    })
}
pub async fn sign(grant: &str, ua: &str, pc: &str) -> Result<DownloadLink, ApiError> {
    if pc.is_empty() || !pc.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(ApiError::network("无效的 pickcode"));
    }
    link(
        &request(
            grant,
            ua,
            "/open/ufile/downurl",
            &[],
            Some(&[("pick_code", pc)]),
        )
        .await?["data"],
        pc,
    )
}
pub async fn account(grant: &str) -> Result<crate::store::AccountSnapshot, ApiError> {
    let v = open_get(grant, "/open/user/info").await?;
    let space = |k: &str| -> Result<(u64, String), ApiError> {
        let s = &v["rt_space_info"][k];
        Ok((
            integer(&s["size"])
                .and_then(|v| u64::try_from(v).ok())
                .ok_or_else(invalid)?,
            s["size_format"].as_str().ok_or_else(invalid)?.into(),
        ))
    };
    let (total, total_text) = space("all_total")?;
    let (used, used_text) = space("all_use")?;
    let (remain, remain_text) = space("all_remain")?;
    let fetched_at = crate::events::unix_now();
    Ok(crate::store::AccountSnapshot {
        user_id: id_of(&v["user_id"]).ok_or_else(invalid)?,
        user_name: v["user_name"].as_str().ok_or_else(invalid)?.into(),
        avatar_url: v["user_face_l"].as_str().unwrap_or_default().into(),
        vip: integer(&v["vip_info"]["expire"]).is_some_and(|t| t > fetched_at),
        total,
        total_text,
        used,
        used_text,
        remain,
        remain_text,
        fetched_at,
    })
}
pub async fn mkdir(grant: &str, ua: &str, parent_id: &str, name: &str) -> Result<String, ApiError> {
    if !valid_id(parent_id) {
        return Err(invalid());
    }
    id_of(
        &request(
            grant,
            ua,
            "/open/folder/add",
            &[],
            Some(&[("pid", parent_id), ("file_name", name)]),
        )
        .await?["data"]["file_id"],
    )
    .ok_or_else(invalid)
}
fn joined(ids: &[String]) -> Result<String, ApiError> {
    if ids.iter().any(|s| !valid_id(s) || s == "0") {
        Err(ApiError::network("无效的文件 ID"))
    } else {
        Ok(ids.join(","))
    }
}
pub async fn move_to(grant: &str, ua: &str, ids: &[String], dest_id: &str) -> Result<(), ApiError> {
    if ids.is_empty() {
        return Ok(());
    }
    if !valid_id(dest_id) {
        return Err(invalid());
    }
    request(
        grant,
        ua,
        "/open/ufile/move",
        &[],
        Some(&[("file_ids", &joined(ids)?), ("to_cid", dest_id)]),
    )
    .await
    .map(drop)
}
pub async fn rename(grant: &str, ua: &str, id: &str, new_name: &str) -> Result<(), ApiError> {
    if !valid_id(id) || id == "0" {
        return Err(invalid());
    }
    request(
        grant,
        ua,
        "/open/ufile/update",
        &[],
        Some(&[("file_id", id), ("file_name", new_name)]),
    )
    .await
    .map(drop)
}
pub async fn remove(grant: &str, ua: &str, ids: &[String]) -> Result<(), ApiError> {
    if ids.is_empty() {
        return Ok(());
    }
    request(
        grant,
        ua,
        "/open/ufile/delete",
        &[],
        Some(&[("file_ids", &joined(ids)?)]),
    )
    .await
    .map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn official_listing_and_chain() {
        let v = json!({"state":true,"count":2,"offset":0,"cid":5,"data":[{"fid":"12","pid":"5","fc":"1","fn":"A.mkv","pc":"abc","fs":9},{"fid":"34","pid":"5","fc":"0","fn":"电影"}],"path":[{"name":"根目录","cid":0},{"name":"Media","cid":"5","pid":"0"}]});
        let p = parse_page(v.clone(), "5", 0, 10, false).unwrap();
        assert_eq!(p.entries[0].file_id().as_deref(), Some("12"));
        assert_eq!(p.entries[1].folder_id().as_deref(), Some("34"));
        assert_eq!(p.entries[0].parent_id().as_deref(), Some("5"));
        assert_eq!(p.path[1], ("5".into(), "Media".into()));
        for (key, bad) in [
            ("count", json!(1)),
            ("data", json!([])),
            ("path", json!([{"cid":0,"name":"root"}])),
        ] {
            let mut x = v.clone();
            x[key] = bad;
            assert!(parse_page(x, "5", 0, 10, false).is_err())
        }
    }
    #[test]
    fn refusals_are_not_empty_pages() {
        assert!(check(429, Value::Null, "").unwrap_err().rate_limited());
        assert!(
            check(200, json!({"state":false,"code":"40140125","data":[]}), "")
                .unwrap_err()
                .auth_expired()
        );
        assert!(parse_page(json!({"count":0}), "0", 0, 10, false).is_err());
        assert!(!approved("//other/open/upload/init", true));
        assert!(!approved("/open/upload/init", false));
    }
    #[test]
    fn signed_link_by_file_id() {
        let v = json!({"123":{"pick_code":"abc","file_name":"a.mkv","file_size":10,"url":{"url":"https://cdn.example/a"}}});
        assert_eq!(link(&v, "abc").unwrap().size, 10);
        assert!(link(&v, "wrong").is_err());
    }
}
