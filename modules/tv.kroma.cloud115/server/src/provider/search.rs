//! `POST /_port/cloud115/search`: the candidates the source knows for one title.
//!
//! The source reports a great deal more than the point contract carries (sharer,
//! date, the card's own "points" label, unlock counts). Only the fields the core
//! can act on are mapped, and every one of them is read defensively: the source
//! is a third-party page scraper, so a missing or renamed field must degrade to
//! an empty value rather than fail the whole search.

use axum::Json;
use kroma_module_sdk::host::port_reply;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::source_json;
use crate::settings::Config;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchBody {
    /// `movie` or `show` (`tv` is accepted as the source's own spelling).
    pub kind: String,
    pub tmdb_id: i64,
    #[serde(default)]
    pub title: String,
    /// Only meaningful for a show; 0 leaves the source's default.
    #[serde(default)]
    pub season: i64,
    /// Walk each candidate's confirmation page to read its real price. Off by
    /// default: that is about half a minute for eight candidates, and the drive
    /// reports `price: null` without it (the card's label is not the price).
    #[serde(default)]
    pub probe: bool,
    /// How many candidates to list/probe; `None` uses the source's own default.
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    /// The common host-point identity and ranking fields.
    pub id: String,
    pub provider: String,
    pub title: String,
    pub size_bytes: u64,
    pub price_points: Option<u32>,
    pub price_known: bool,
    pub grabbable: bool,
    pub reject_reason: Option<String>,
    pub extra: Value,
    /// Fields the 115 provider needs when the candidate is confirmed. These
    /// remain top-level too so the client can echo the exact candidate back.
    pub index: i64,
    pub file_name: String,
    pub resource: String,
    pub size: String,
    pub price: Option<i64>,
    pub link: String,
    pub share_code: String,
    pub tags: Vec<String>,
    pub already_unlocked: bool,
    pub probe_error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchReply {
    pub candidates: Vec<Candidate>,
    /// Why the search answered nothing. Present only when it went wrong, so a
    /// caller can show one sentence instead of an empty list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The source's kind spelling for one of ours.
pub(crate) fn source_kind(kind: &str) -> Option<&'static str> {
    match kind.trim().to_ascii_lowercase().as_str() {
        "movie" | "film" => Some("movie"),
        "show" | "tv" | "series" => Some("tv"),
        _ => None,
    }
}

pub(super) async fn search(Json(body): Json<SearchBody>) -> Json<Result<SearchReply, String>> {
    port_reply(move || {
        let config = Config::load();
        // A source that is down answers "no candidates, here is why" rather than
        // an `Err`: the point contract gives search an error field so the core can
        // fall back to another provider without a transport failure.
        Ok(match run(&config, &body) {
            Ok(candidates) => SearchReply {
                candidates,
                error: None,
            },
            Err(error) => SearchReply {
                candidates: Vec::new(),
                error: Some(error),
            },
        })
    })
    .await
}

fn run(config: &Config, body: &SearchBody) -> Result<Vec<Candidate>, String> {
    if body.tmdb_id <= 0 {
        return Err("缺少 TMDB 标识".into());
    }
    let kind = source_kind(&body.kind)
        .ok_or_else(|| format!("未知类型：{}（只支持 movie / show）", body.kind.trim()))?;
    if config.source_url.trim().is_empty() {
        return Err("订阅源服务地址还没配置".into());
    }
    let mut path = format!(
        "/search?tmdb={}&type={kind}&probe={}",
        body.tmdb_id,
        u8::from(body.probe)
    );
    if body.season > 0 {
        path.push_str(&format!("&season={}", body.season));
    }
    if let Some(limit) = body.limit {
        path.push_str(&format!("&limit={}", limit.clamp(1, 50)));
    }
    let value = source_json(config, &path, None)?;
    // The source answers 200 with `{"error": "..."}` for its own failures (a
    // logged-out browser, say); that is this search's reason, not a candidate.
    if let Some(error) = value
        .get("error")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|error| !error.is_empty())
    {
        return Err(error.to_string());
    }
    let list = value
        .get("candidates")
        .and_then(Value::as_array)
        .ok_or_else(|| "订阅源返回里没有候选列表".to_string())?;
    Ok(candidates_of(list))
}

/// Map the source's candidate array onto the point's shape. Never fails: an
/// entry that is not even an object is skipped, and a missing field is empty.
pub(crate) fn candidates_of(list: &[Value]) -> Vec<Candidate> {
    let mut out = Vec::with_capacity(list.len());
    for (position, item) in list.iter().enumerate() {
        if !item.is_object() {
            continue;
        }
        let size_bytes = size_bytes_of(item);
        let already_unlocked = item
            .get("alreadyUnlocked")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        // A share this account already unlocked costs nothing to open again, and
        // the source reports no price for it. Saying `0` beats `null`: it is what
        // the acquire gate needs to start without a pointless confirmation.
        let price = if already_unlocked {
            Some(0)
        } else {
            integer(&item["price"]).filter(|price| *price >= 0)
        };
        let index = integer(&item["index"]).unwrap_or(position as i64);
        let file_name = text(item, &["fileName", "resource", "name", "filename"]);
        let size = text(item, &["size", "sizeText"]);
        let link = text(item, &["link", "url"]);
        let share_code = text(item, &["shareCode", "shareKey", "code"]);
        let resource = if file_name.is_empty() {
            share_code.clone()
        } else {
            file_name.clone()
        };
        let price_points = price.and_then(|points| u32::try_from(points).ok());
        let probe_error = text(item, &["probeError"])
            .trim()
            .to_string()
            .into_non_empty();
        let extra = json!({
            "index": index,
            "resource": resource,
            "link": link,
            "shareCode": share_code,
            "alreadyUnlocked": already_unlocked,
            "probeError": probe_error,
        });
        out.push(Candidate {
            id: format!("115:{index}:{resource}"),
            provider: "cloud115".into(),
            title: if file_name.is_empty() { resource.clone() } else { file_name.clone() },
            size_bytes,
            price_points,
            price_known: price_points.is_some(),
            grabbable: !resource.is_empty() || !link.is_empty(),
            reject_reason: None,
            extra,
            index,
            file_name,
            resource,
            size,
            price,
            link,
            share_code,
            tags: strings(&item["tags"]),
            already_unlocked,
            probe_error,
        });
    }
    out
}

/// The first non-empty string among `keys`.
fn text(item: &Value, keys: &[&str]) -> String {
    keys.iter()
        .filter_map(|key| item.get(*key))
        .find_map(|value| match value {
            Value::String(text) => Some(text.clone()),
            Value::Number(number) => Some(number.to_string()),
            _ => None,
        })
        .map(|text| text.trim().to_string())
        .unwrap_or_default()
}

fn integer(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| match item {
                    Value::String(text) if !text.trim().is_empty() => Some(text.trim().to_string()),
                    Value::Number(number) => Some(number.to_string()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `sizeBytes` when the source has it, else the human `size` parsed ("17.59 GB").
fn size_bytes_of(item: &Value) -> u64 {
    if let Some(bytes) = integer(&item["sizeBytes"]).filter(|bytes| *bytes > 0) {
        return bytes as u64;
    }
    let text = text(item, &["size"]);
    parse_size(&text).unwrap_or(0)
}

/// `"17.59 GB"` → bytes, in the binary units the drive itself uses.
pub(crate) fn parse_size(raw: &str) -> Option<u64> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let digits: String = raw
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    if digits.is_empty() {
        return None;
    }
    let number: f64 = digits.parse().ok()?;
    let rest = raw[digits.len()..].trim().to_ascii_uppercase();
    let factor: f64 = if rest.starts_with("TB") {
        1024.0 * 1024.0 * 1024.0 * 1024.0
    } else if rest.starts_with("GB") {
        1024.0 * 1024.0 * 1024.0
    } else if rest.starts_with("MB") {
        1024.0 * 1024.0
    } else if rest.starts_with("KB") {
        1024.0
    } else if rest.starts_with('B') || rest.is_empty() {
        1.0
    } else {
        return None;
    };
    Some((number * factor).round() as u64)
}

/// Turn an empty string into `None`, so `probeError` is omitted rather than "".
trait IntoNonEmpty {
    fn into_non_empty(self) -> Option<String>;
}

impl IntoNonEmpty for String {
    fn into_non_empty(self) -> Option<String> {
        if self.trim().is_empty() {
            None
        } else {
            Some(self)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One real answer from madreel-svc, trimmed to the fields that matter, plus
    /// the shapes its scraper can degrade to.
    const SOURCE: &str = r#"{
        "mediaKey": "movie-155",
        "candidates": [
            {
                "index": 0,
                "fileName": "Batman.Begins.2005.1080p.BluRay.x264.mkv",
                "size": "17.59 GB",
                "sizeBytes": 18887041352,
                "tags": ["1080P", "BluRay", "中字"],
                "sharer": "someone",
                "labelValue": 8,
                "unlockCount": 4,
                "hasSub": true,
                "price": 4,
                "alreadyUnlocked": false,
                "shareKey": "swabc123",
                "link": "https://115cdn.com/s/swabc123?password=xy9z",
                "shareCode": "swabc123",
                "probeError": null
            },
            { "fileName": "second.mkv" },
            {
                "index": "7",
                "resource": "third.mkv",
                "size": "700.5 MB",
                "price": null,
                "probeError": "索引 7 超出范围"
            }
        ],
        "ranked": [{ "index": 0, "score": 128 }]
    }"#;

    fn source() -> Value {
        serde_json::from_str(SOURCE).unwrap()
    }

    #[test]
    fn a_full_candidate_maps_field_for_field() {
        let candidates = candidates_of(source()["candidates"].as_array().unwrap());
        assert_eq!(candidates.len(), 3);
        let first = &candidates[0];
        assert_eq!(first.index, 0);
        assert_eq!(first.file_name, "Batman.Begins.2005.1080p.BluRay.x264.mkv");
        assert_eq!(first.size, "17.59 GB");
        assert_eq!(first.size_bytes, 18_887_041_352);
        assert_eq!(first.price, Some(4));
        assert_eq!(first.share_code, "swabc123");
        assert_eq!(first.link, "https://115cdn.com/s/swabc123?password=xy9z");
        assert_eq!(first.tags, vec!["1080P", "BluRay", "中字"]);
        assert!(!first.already_unlocked);
        assert!(first.probe_error.is_none());
    }

    #[test]
    fn a_thin_candidate_keeps_its_position_and_empty_fields() {
        let candidates = candidates_of(source()["candidates"].as_array().unwrap());
        let thin = &candidates[1];
        assert_eq!(thin.index, 1, "a missing index is the array position");
        assert_eq!(thin.file_name, "second.mkv");
        assert_eq!(thin.price, None);
        assert_eq!(thin.size_bytes, 0);
        assert!(thin.tags.is_empty());
        assert!(thin.share_code.is_empty());
    }

    #[test]
    fn a_string_index_and_a_mebibyte_size_are_understood() {
        let candidates = candidates_of(source()["candidates"].as_array().unwrap());
        let third = &candidates[2];
        assert_eq!(third.index, 7);
        assert_eq!(third.size_bytes, (700.5 * 1024.0 * 1024.0_f64).round() as u64);
        assert_eq!(third.probe_error.as_deref(), Some("索引 7 超出范围"));
    }

    #[test]
    fn a_size_without_bytes_is_parsed_in_binary_units() {
        assert_eq!(
            parse_size("17.59 GB"),
            Some((17.59 * 1024.0_f64.powi(3)).round() as u64)
        );
        assert_eq!(parse_size("700 MB"), Some(700 * 1024 * 1024));
        assert_eq!(parse_size("2 TB"), Some(2 * 1024 * 1024 * 1024 * 1024));
        assert_eq!(parse_size("512 B"), Some(512));
        assert_eq!(parse_size(""), None);
        assert_eq!(parse_size("large"), None);
    }

    #[test]
    fn an_already_unlocked_share_reads_as_free_rather_than_unpriced() {
        let list: Vec<Value> = serde_json::from_str(
            r#"[{ "fileName": "a.mkv", "price": null, "alreadyUnlocked": true }]"#,
        )
        .unwrap();
        let candidates = candidates_of(&list);
        assert!(candidates[0].already_unlocked);
        assert_eq!(
            candidates[0].price,
            Some(0),
            "opening a share this account unlocked costs nothing, so the acquire gate may start"
        );
    }

    #[test]
    fn a_candidate_keeps_the_points_field_names() {
        let candidates = candidates_of(source()["candidates"].as_array().unwrap());
        let value = serde_json::to_value(&candidates[0]).unwrap();
        for key in [
            "index",
            "fileName",
            "size",
            "sizeBytes",
            "price",
            "link",
            "shareCode",
            "tags",
            "alreadyUnlocked",
            "probeError",
        ] {
            assert!(value.get(key).is_some(), "candidate is missing {key}: {value}");
        }
        assert!(value["alreadyUnlocked"].is_boolean());
    }

    #[test]
    fn the_kind_is_translated_to_the_sources_own_spelling() {
        assert_eq!(source_kind("movie"), Some("movie"));
        assert_eq!(source_kind(" Show "), Some("tv"));
        assert_eq!(source_kind("tv"), Some("tv"));
        assert_eq!(source_kind("sideways"), None);
    }

    #[test]
    fn a_search_without_a_tmdb_id_or_with_a_bad_kind_answers_an_error() {
        let config = Config {
            source_url: "http://127.0.0.1:9".into(),
            ..Config::default()
        };
        let no_tmdb = run(
            &config,
            &SearchBody {
                kind: "movie".into(),
                tmdb_id: 0,
                ..SearchBody::default()
            },
        )
        .unwrap_err();
        assert!(no_tmdb.contains("TMDB"), "{no_tmdb}");

        let bad_kind = run(
            &config,
            &SearchBody {
                kind: "sideways".into(),
                tmdb_id: 155,
                ..SearchBody::default()
            },
        )
        .unwrap_err();
        assert!(bad_kind.contains("未知类型"), "{bad_kind}");
    }
}