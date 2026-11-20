//! Share-link transfer: paste a 115 share link (with its extract code), read
//! the share, then save the chosen files into a folder of the same account.
//!
//! Only the web API is used, because a cookie is enough for it. Cloud download
//! (magnet/ed2k) is a different API and needs an open-platform token, so it is
//! deliberately not here.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::client;

const MAX_FILES: usize = 2000;
const MAX_DEPTH: usize = 12;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedShare {
    pub code: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareEntry {
    pub id: String,
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareView {
    pub code: String,
    pub password: String,
    pub title: String,
    pub total_size: u64,
    pub entries: Vec<ShareEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedEntry {
    pub id: String,
    #[serde(default)]
    pub is_dir: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiveReport {
    pub files: usize,
    pub message: String,
}

/// Share codes and their extract codes as pasted. A link without its own
/// `password=` takes the next standalone code in the text.
pub fn parse_links(text: &str) -> Vec<ParsedShare> {
    let chars: Vec<char> = text.chars().collect();
    let mut shares: Vec<ParsedShare> = Vec::new();
    let mut i = 0;
    while i + 2 < chars.len() {
        if chars[i] == '/' && chars[i + 1] == 's' && chars[i + 2] == '/' {
            let mut j = i + 3;
            let mut code = String::new();
            while j < chars.len() && chars[j].is_ascii_alphanumeric() {
                code.push(chars[j]);
                j += 1;
            }
            if code.len() >= 4 {
                let mut password = String::new();
                let mut k = j;
                while k < chars.len() && !chars[k].is_whitespace() {
                    for marker in ["password=", "pwd=", "code="] {
                        if starts_with(&chars, k, marker) {
                            let mut m = k + marker.len();
                            while m < chars.len() && chars[m].is_ascii_alphanumeric() {
                                password.push(chars[m]);
                                m += 1;
                            }
                        }
                    }
                    k += 1;
                }
                if !shares.iter().any(|held| held.code == code) {
                    shares.push(ParsedShare { code, password });
                }
                i = j;
                continue;
            }
        }
        i += 1;
    }
    // "提取码：abcd" or "密码 abcd": fill the links that did not carry one.
    let loose = loose_codes(&chars);
    if !loose.is_empty() {
        let mut next = 0;
        for share in &mut shares {
            if share.password.is_empty() {
                share.password = loose
                    .get(next)
                    .cloned()
                    .unwrap_or_else(|| loose[0].clone());
                next += 1;
            }
        }
    }
    shares
}

fn starts_with(chars: &[char], at: usize, needle: &str) -> bool {
    let mut i = at;
    for expected in needle.chars() {
        if chars.get(i) != Some(&expected) {
            return false;
        }
        i += 1;
    }
    true
}

fn loose_codes(chars: &[char]) -> Vec<String> {
    let mut out = Vec::new();
    let text: String = chars.iter().collect();
    for marker in ["提取码", "密码", "访问码"] {
        let mut from = 0;
        while let Some(found) = text[from..].find(marker) {
            let mut i = from + found + marker.len();
            while i < chars.len() && !chars[i].is_ascii_alphanumeric() {
                i += 1;
            }
            let mut code = String::new();
            while i < chars.len() && chars[i].is_ascii_alphanumeric() {
                code.push(chars[i]);
                i += 1;
            }
            if (4..=8).contains(&code.len()) {
                if !out.contains(&code) {
                    out.push(code);
                }
            }
            from = from + found + marker.len();
            if from >= text.len() {
                break;
            }
        }
    }
    out
}

fn decode_name(text: &str) -> String {
    let mut bytes = Vec::with_capacity(text.len());
    let raw = text.as_bytes();
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%' && i + 2 < raw.len() {
            let hi = (raw[i + 1] as char).to_digit(16);
            let lo = (raw[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                bytes.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        if raw[i] == b'+' {
            bytes.push(b' ');
        } else {
            bytes.push(raw[i]);
        }
        i += 1;
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn text_of(item: &Value, keys: &[&str]) -> String {
    keys.iter()
        .filter_map(|key| item.get(*key))
        .find_map(|value| match value {
            Value::String(text) => Some(text.clone()),
            Value::Number(number) => Some(number.to_string()),
            _ => None,
        })
        .unwrap_or_default()
}

fn id_of(item: &Value, key: &str) -> Option<String> {
    item.get(key).and_then(|value| match value {
        Value::String(text) if !text.is_empty() && text != "0" => Some(text.clone()),
        Value::Number(number) => number.as_i64().filter(|n| *n > 0).map(|n| n.to_string()),
        _ => None,
    })
}

fn entries_of(data: &Value) -> Vec<ShareEntry> {
    data.get("list")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|item| {
                    let file_id = id_of(item, "fid");
                    let folder_id = id_of(item, "cid");
                    let is_dir = file_id.is_none() && folder_id.is_some();
                    let id = file_id.clone().or(folder_id)?;
                    let name = decode_name(&text_of(item, &["n", "fn", "name"]));
                    let size = item
                        .get("s")
                        .or_else(|| item.get("size"))
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    Some(ShareEntry {
                        id,
                        name,
                        is_dir,
                        size,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Read every pasted share at its root. Directories are shown as entries; their
/// contents are only expanded when the operator actually receives them.
pub async fn preview(cookie: &str, ua: &str, text: &str) -> Result<Vec<ShareView>, String> {
    let shares = parse_links(text);
    if shares.is_empty() {
        return Err("没有识别到 115 分享链接".into());
    }
    let mut out = Vec::new();
    for share in shares {
        let data = client::share_snap(
            cookie,
            ua,
            &share.code,
            &share.password,
            "0",
            0,
            1000,
        )
        .await
        .map_err(|error| error.to_string())?;
        let title = decode_name(&text_of(
            data.get("shareinfo").unwrap_or(&Value::Null),
            &["share_title", "title"],
        ));
        let total_size = data
            .get("shareinfo")
            .and_then(|info| info.get("file_size"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        out.push(ShareView {
            code: share.code,
            password: share.password,
            title: if title.is_empty() {
                "115 分享".into()
            } else {
                title
            },
            total_size,
            entries: entries_of(&data),
        });
    }
    Ok(out)
}

async fn collect_files(
    cookie: &str,
    ua: &str,
    code: &str,
    password: &str,
    cid: &str,
    depth: usize,
    out: &mut Vec<String>,
) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err("分享目录层级过深，已停止".into());
    }
    let mut offset = 0u32;
    loop {
        let data = client::share_snap(cookie, ua, code, password, cid, offset, 1000)
            .await
            .map_err(|error| error.to_string())?;
        let entries = entries_of(&data);
        let total = data
            .get("count")
            .and_then(Value::as_u64)
            .unwrap_or(entries.len() as u64) as usize;
        for entry in &entries {
            if entry.is_dir {
                Box::pin(collect_files(
                    cookie,
                    ua,
                    code,
                    password,
                    &entry.id,
                    depth + 1,
                    out,
                ))
                .await?;
            } else {
                out.push(entry.id.clone());
            }
            if out.len() >= MAX_FILES {
                return Err(format!("分享文件超过 {MAX_FILES} 个，请按作品拆分链接"));
            }
        }
        offset += entries.len() as u32;
        if entries.is_empty() || offset as usize >= total {
            return Ok(());
        }
    }
}

/// Save the chosen entries. A chosen directory is expanded to its files first,
/// because `share/receive` takes file ids.
pub async fn receive(
    cookie: &str,
    ua: &str,
    code: &str,
    password: &str,
    cid: &str,
    selected: &[SelectedEntry],
) -> Result<ReceiveReport, String> {
    let mut files: Vec<String> = Vec::new();
    for entry in selected {
        if entry.is_dir {
            collect_files(cookie, ua, code, password, &entry.id, 0, &mut files).await?;
        } else {
            files.push(entry.id.clone());
        }
        if files.len() >= MAX_FILES {
            return Err(format!("所选文件超过 {MAX_FILES} 个，请分批转存"));
        }
    }
    if files.is_empty() {
        return Err("没有选中可转存的文件".into());
    }
    let data = client::share_receive(cookie, ua, code, password, &files, cid)
        .await
        .map_err(|error| error.to_string())?;
    let received = data
        .get("recv_file_count")
        .and_then(Value::as_u64)
        .map(|count| count as usize)
        .unwrap_or(files.len());
    Ok(ReceiveReport {
        files: received,
        message: format!("已转存 {received} 个文件到目标目录"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_and_an_extract_code_are_paired() {
        let parsed = parse_links("https://115.com/s/swabc123?password=xy9z");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].code, "swabc123");
        assert_eq!(parsed[0].password, "xy9z");
    }

    #[test]
    fn a_standalone_extract_code_is_used() {
        let parsed = parse_links("https://115.com/s/swabc123\n提取码：k4m2");
        assert_eq!(parsed[0].password, "k4m2");
    }

    #[test]
    fn duplicates_collapse() {
        let parsed = parse_links("https://115.com/s/swabc123\nhttps://115cdn.com/s/swabc123");
        assert_eq!(parsed.len(), 1);
    }
}
