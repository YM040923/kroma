//! The object storage 115 hands an upload to: a bucket signed with temporary
//! STS credentials. Multipart throughout, because a film is never small enough
//! to want in memory. Signing is OSS's V1 scheme (HMAC-SHA1 over a canonical
//! request), reimplemented from the wire behaviour.

use base64::Engine;
use hmac::{Hmac, Mac};
use md5::{Digest, Md5};
use serde_json::Value;
use sha1::Sha1;

use kroma_module_sdk::http::Fetch;

const TIMEOUT: u32 = 300;

// Request-scoped STS credentials: never saved in settings, a job or a global cache.
struct Token {
    endpoint: String,
    key_id: String,
    secret: String,
    security: String,
}

fn dns_label(value: &str) -> bool {
    (1..=63).contains(&value.len())
        && !value.starts_with('-')
        && !value.ends_with('-')
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn token_from_data(data: &Value) -> Result<Token, String> {
    let field = |keys: &[&str]| {
        keys.iter()
            .find_map(|key| data[*key].as_str())
            .filter(|v| !v.is_empty() && !v.bytes().any(|b| b.is_ascii_control()))
            .ok_or("115 上传凭证不完整")
    };
    let raw = field(&["endpoint"])?.trim_end_matches('/');
    let endpoint = raw
        .strip_prefix("https://")
        .or_else(|| raw.strip_prefix("http://"))
        .unwrap_or(raw);
    let region = endpoint
        .strip_suffix(".aliyuncs.com")
        .ok_or("无效的 OSS 域名")?;
    if !region.starts_with("oss-") || !dns_label(region) {
        return Err("无效的 OSS 域名".into());
    }
    // OSS enforces the STS Expiration; fetch a fresh credential for each request.
    field(&["Expiration", "expiration"])?;
    Ok(Token {
        endpoint: endpoint.to_owned(),
        key_id: field(&["AccessKeyId", "access_key_id"])?.into(),
        secret: field(&["AccessKeySecret", "AccessKeySecrett", "access_key_secret"])?.into(),
        security: field(&["SecurityToken", "security_token"])?.into(),
    })
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
        .filter(|(name, _)| {
            matches!(
                name.as_str(),
                "uploads" | "uploadId" | "partNumber" | "sequential"
            )
        })
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
    let mut mac =
        Hmac::<Sha1>::new_from_slice(token.secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(text.as_bytes());
    let digest = base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());
    (date, format!("OSS {}:{digest}", token.key_id))
}

fn url(endpoint: &str, bucket: &str, key: &str, params: &[(String, String)]) -> String {
    let mut url = format!("https://{bucket}.{endpoint}/{}", quote_path(key));
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

async fn request(
    grant: &str,
    method: &str,
    bucket: &str,
    key: &str,
    content_type: &str,
    extra: &[(String, String)],
    params: &[(String, String)],
    body: &[u8],
) -> Result<Response, String> {
    if !dns_label(bucket) {
        return Err("无效的 OSS bucket".into());
    }
    let data = crate::client::open_get(grant, "/open/upload/get_token")
        .await
        .map_err(|_| "无法取得 OSS 临时凭证")?;
    let token = token_from_data(&data)?;
    let (date, authorization) = signature(&token, method, bucket, key, content_type, extra, params);
    let mut fetch = Fetch::default()
        .purpose("cloud-drive")
        .no_redirects()
        .https_only()
        .header("Date", date)
        .header("Authorization", authorization)
        .header("x-oss-security-token", token.security.clone())
        .max_time(TIMEOUT);
    for (name, value) in extra {
        fetch = fetch.header(name, value.clone());
    }
    let target = url(&token.endpoint, bucket, key, params);
    let response = match method {
        "PUT" => fetch.put_bytes(&target, content_type, body),
        "POST" => fetch.post_bytes(&target, content_type, body),
        _ => return Err("不支持的 OSS 操作".into()),
    }
    .map_err(|_| "OSS 请求失败")?;
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
pub(crate) async fn start(grant: &str, bucket: &str, key: &str) -> Result<String, String> {
    let params = vec![
        ("uploads".to_string(), "1".to_string()),
        ("sequential".to_string(), "1".to_string()),
    ];
    let response = request(
        grant,
        "POST",
        bucket,
        key,
        "application/octet-stream",
        &[],
        &params,
        &[],
    )
    .await?;
    let body = response.text();
    if !(200..300).contains(&response.status) {
        return Err(format!("OSS 分片上传开不了头（{}）", response.status));
    }
    xml_text(&body, "UploadId").ok_or_else(|| "OSS 没有给出 UploadId".to_string())
}

/// Upload one part; answers its ETag once it matches the part's MD5.
pub(crate) async fn put_part(
    grant: &str,
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
    let response = request(
        grant,
        "PUT",
        bucket,
        key,
        "application/octet-stream",
        &[],
        &params,
        data,
    )
    .await?;
    if !(200..300).contains(&response.status) {
        return Err(format!("OSS 分片 {number} 没传上（{}）", response.status));
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
pub(crate) async fn complete(
    grant: &str,
    bucket: &str,
    key: &str,
    upload_id: &str,
    etags: &[String],
    callback: &str,
    callback_var: &str,
) -> Result<(), String> {
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
        grant,
        "POST",
        bucket,
        key,
        "text/xml",
        &extra,
        &params,
        body.as_bytes(),
    )
    .await?;
    let text = response.text();
    if !(200..300).contains(&response.status) {
        return Err(format!("OSS 合并分片失败（{}）", response.status));
    }
    let answer: Value = serde_json::from_str(&text).map_err(|_| "115 上传回调无法解析")?;
    if answer["state"].as_bool() != Some(true) {
        return Err("115 没有确认接收文件".into());
    }
    Ok(())
}

use kroma_module_sdk::http::Response;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sts_uses_only_the_official_data_and_oss_host() {
        let mut envelope = serde_json::json!({"state":true,"code":0,"data":{
            "endpoint":"http://oss-cn-shenzhen.aliyuncs.com", "Expiration":"2030-01-01T00:00:00Z",
            "AccessKeyId":"example","AccessKeySecret":"example","SecurityToken":"example"}});
        let token = token_from_data(&envelope["data"]).unwrap();
        assert_eq!(
            url(&token.endpoint, "example", "a b", &[]),
            "https://example.oss-cn-shenzhen.aliyuncs.com/a%20b"
        );
        for endpoint in [
            "oss-test.aliyuncs.com.evil.test",
            "oss-test@evil.test",
            "evil.test/path/oss-test.aliyuncs.com",
        ] {
            envelope["data"]["endpoint"] = endpoint.into();
            assert!(token_from_data(&envelope["data"]).is_err());
        }
        assert!(!dns_label("example.test/path"));
    }
}
