//! The object storage 115 hands an upload to: a bucket signed with temporary
//! STS credentials. Multipart throughout, because a film is never small enough
//! to want in memory. Signing is OSS's V1 scheme (HMAC-SHA1 over a canonical
//! request), reimplemented from the wire behaviour.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::Engine;
use hmac::{Hmac, Mac};
use md5::{Digest, Md5};
use serde_json::Value;
use sha1::Sha1;

use kroma_module_sdk::http::Fetch;

const ENDPOINT: &str = "oss-cn-shenzhen.aliyuncs.com";
pub(crate) const DEFAULT_BUCKET: &str = "fhnfile";
const TOKEN_TTL: Duration = Duration::from_secs(50 * 60);
const TIMEOUT: u32 = 300;

#[derive(Clone)]
struct Token {
    key_id: String,
    secret: String,
    security: String,
}

fn token_slot() -> &'static Mutex<Option<(Instant, Token)>> {
    static SLOT: std::sync::OnceLock<Mutex<Option<(Instant, Token)>>> = std::sync::OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

fn credentials() -> Result<Token, String> {
    if let Some((at, token)) = token_slot()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
    {
        if at.elapsed() < TOKEN_TTL {
            return Ok(token.clone());
        }
    }
    let response = Fetch::default()
        .purpose("cloud-drive")
        .header("User-Agent", crate::client::UPLOAD_UA)
        .max_time(30)
        .get("https://uplb.115.com/3.0/gettoken.php")
        .map_err(|error| format!("拿不到上传凭证：{error}"))?;
    let body: Value = response
        .json()
        .map_err(|error| format!("上传凭证无法解析：{error}"))?;
    let text = |key: &str| {
        body.get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    let token = Token {
        key_id: text("AccessKeyId").ok_or("上传凭证缺少 AccessKeyId")?,
        secret: text("AccessKeySecret").ok_or("上传凭证缺少 AccessKeySecret")?,
        security: text("SecurityToken").ok_or("上传凭证缺少 SecurityToken")?,
    };
    *token_slot()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some((Instant::now(), token.clone()));
    Ok(token)
}

fn quote_path(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(*byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

fn quote_value(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

fn http_date() -> String {
    use time::macros::format_description;
    const FORMAT: &[time::format_description::FormatItem<'static>] = format_description!(
        "[weekday repr:short], [day] [month repr:short] [year] [hour]:[minute]:[second] GMT"
    );
    time::OffsetDateTime::now_utc()
        .format(FORMAT)
        .unwrap_or_default()
}

fn signature(
    token: &Token,
    method: &str,
    bucket: &str,
    key: &str,
    content_type: &str,
    headers: &[(String, String)],
    params: &[(String, String)],
) -> (String, String) {
    let date = http_date();
    let mut all: Vec<(String, String)> = headers
        .iter()
        .map(|(name, value)| (name.to_ascii_lowercase(), value.clone()))
        .collect();
    all.push(("date".into(), date.clone()));
    all.push(("x-oss-security-token".into(), token.security.clone()));
    all.sort_by(|a, b| a.0.cmp(&b.0));
    let oss_headers = all
        .iter()
        .filter(|(name, _)| name.starts_with("x-oss-"))
        .map(|(name, value)| format!("{name}:{value}"))
        .collect::<Vec<_>>()
        .join("\n");

    let mut subresources: Vec<(String, String)> = params
        .iter()
        .filter(|(name, _)| matches!(name.as_str(), "uploads" | "uploadId" | "partNumber" | "sequential"))
        .cloned()
        .collect();
    subresources.sort_by(|a, b| a.0.cmp(&b.0));
    let resource = if subresources.is_empty() {
        format!("/{bucket}/{}", quote_path(key))
    } else {
        let query = subresources
            .iter()
            .map(|(name, value)| format!("{}={}", quote_value(name), quote_value(value)))
            .collect::<Vec<_>>()
            .join("&");
        format!("/{bucket}/{}?{query}", quote_path(key))
    };
    let text = format!("{method}\n\n{content_type}\n{date}\n{oss_headers}\n{resource}");
    let mut mac = Hmac::<Sha1>::new_from_slice(token.secret.as_bytes())
        .expect("HMAC accepts any key length");
    mac.update(text.as_bytes());
    let digest = base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());
    (date, format!("OSS {}:{digest}", token.key_id))
}

fn url(bucket: &str, key: &str, params: &[(String, String)]) -> String {
    let mut url = format!("https://{bucket}.{ENDPOINT}/{}", quote_path(key));
    if !params.is_empty() {
        let query = params
            .iter()
            .map(|(name, value)| format!("{}={}", quote_value(name), quote_value(value)))
            .collect::<Vec<_>>()
            .join("&");
        url.push('?');
        url.push_str(&query);
    }
    url
}

fn request(
    method: &str,
    bucket: &str,
    key: &str,
    content_type: &str,
    extra: &[(String, String)],
    params: &[(String, String)],
    body: &[u8],
) -> Result<Response, String> {
    let token = credentials()?;
    let (date, authorization) = signature(&token, method, bucket, key, content_type, extra, params);
    let mut fetch = Fetch::default()
        .purpose("cloud-drive")
        .header("User-Agent", crate::client::UPLOAD_UA)
        .header("Date", date)
        .header("Authorization", authorization)
        .header("x-oss-security-token", token.security.clone())
        .max_time(TIMEOUT);
    for (name, value) in extra {
        fetch = fetch.header(name, value.clone());
    }
    let target = url(bucket, key, params);
    let response = match method {
        "PUT" => fetch.put_bytes(&target, content_type, body),
        "POST" => fetch.post_bytes(&target, content_type, body),
        _ => fetch.get(&target),
    }
    .map_err(|error| format!("OSS 请求失败：{error}"))?;
    Ok(response)
}

fn xml_text(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    Some(xml[start..end].trim().to_string())
}

/// Open a multipart upload and answer its id.
pub(crate) fn start(bucket: &str, key: &str) -> Result<String, String> {
    let params = vec![
        ("uploads".to_string(), "1".to_string()),
        ("sequential".to_string(), "1".to_string()),
    ];
    let response = request("POST", bucket, key, "application/octet-stream", &[], &params, &[])?;
    let body = response.text();
    if !(200..300).contains(&response.status) {
        return Err(format!(
            "OSS 分片上传开不了头（{}）",
            xml_text(&body, "Code").unwrap_or_else(|| response.status.to_string())
        ));
    }
    xml_text(&body, "UploadId").ok_or_else(|| "OSS 没有给出 UploadId".to_string())
}

/// Upload one part; answers its ETag once it matches the part's MD5.
pub(crate) fn put_part(
    bucket: &str,
    key: &str,
    upload_id: &str,
    number: u32,
    data: &[u8],
) -> Result<String, String> {
    let params = vec![
        ("partNumber".to_string(), number.to_string()),
        ("uploadId".to_string(), upload_id.to_string()),
    ];
    let response = request("PUT", bucket, key, "application/octet-stream", &[], &params, data)?;
    if !(200..300).contains(&response.status) {
        return Err(format!(
            "OSS 分片 {number} 没传上（{}）",
            xml_text(&response.text(), "Code").unwrap_or_else(|| response.status.to_string())
        ));
    }
    let etag = response
        .header("etag")
        .map(|value| value.trim_matches('"').to_string())
        .unwrap_or_default();
    let expected = hex::encode(Md5::digest(data)).to_uppercase();
    if etag.to_uppercase() != expected {
        return Err(format!("OSS 分片 {number} 校验不一致"));
    }
    Ok(etag)
}

/// Merge the parts; 115 only learns the file exists through the callback.
pub(crate) fn complete(
    bucket: &str,
    key: &str,
    upload_id: &str,
    etags: &[String],
    callback: &str,
    callback_var: &str,
) -> Result<String, String> {
    let mut parts = String::new();
    for (at, etag) in etags.iter().enumerate() {
        parts.push_str(&format!(
            "<Part><PartNumber>{}</PartNumber><ETag>\"{etag}\"</ETag></Part>",
            at + 1
        ));
    }
    let body = format!("<CompleteMultipartUpload>{parts}</CompleteMultipartUpload>");
    let extra = vec![
        (
            "x-oss-callback".to_string(),
            base64::engine::general_purpose::STANDARD.encode(callback.as_bytes()),
        ),
        (
            "x-oss-callback-var".to_string(),
            base64::engine::general_purpose::STANDARD.encode(callback_var.as_bytes()),
        ),
    ];
    let params = vec![("uploadId".to_string(), upload_id.to_string())];
    let response = request(
        "POST",
        bucket,
        key,
        "text/xml",
        &extra,
        &params,
        body.as_bytes(),
    )?;
    let text = response.text();
    if !(200..300).contains(&response.status) {
        return Err(format!(
            "OSS 合并分片失败（{}）",
            xml_text(&text, "Code").unwrap_or_else(|| response.status.to_string())
        ));
    }
    let answer: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if answer.get("state").and_then(Value::as_bool) == Some(false) {
        return Err(format!("115 没有接收上传的文件：{text}"));
    }
    Ok(text)
}

use kroma_module_sdk::http::Response;
