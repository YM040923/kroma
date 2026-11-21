//! Douban as a **browse-only** source: its mobile web endpoints, read at one
//! request per second behind a TTL cache. Nothing here is identity: a Douban id
//! never becomes a library match, and no write ever reaches the account.
//!
//! The endpoints are the ones Douban's own mobile site calls (the `rexxar` JSON
//! API), not an official surface; they can change without notice. A response
//! this module cannot read is reported as such rather than guessed at.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use kroma_http::Fetch;

const BASE: &str = "https://m.douban.com/rexxar/api/v2";
/// Douban answers the browser its own mobile site runs, and a desktop agent
/// gets a different (sometimes empty) answer.
pub const USER_AGENT: &str =
    "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15";
pub const REFERER: &str = "https://m.douban.com/movie/";
const TIMEOUT_SECS: u32 = 20;
/// One call per second, process-wide: the endpoint is unofficial and a burst is
/// what gets an IP refused.
const INTERVAL: Duration = Duration::from_secs(1);
const ROW_TTL: Duration = Duration::from_secs(6 * 60 * 60);
const DETAIL_TTL: Duration = Duration::from_secs(3 * 24 * 60 * 60);
/// A row with fewer than this many cards is not worth a heading.
const MIN_ROW_ITEMS: usize = 4;
const MAX_PEOPLE: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Movie,
    Tv,
}

impl Kind {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "movie" | "movies" => Some(Kind::Movie),
            "tv" | "show" | "shows" => Some(Kind::Tv),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    pub id: String,
    pub kind: Kind,
    pub title: String,
    pub original_title: String,
    pub year: Option<u32>,
    pub rating: f32,
    pub genres: Vec<String>,
    pub overview: String,
    pub poster_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backdrop_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RowStub {
    pub id: String,
    pub title: String,
    pub ranked: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Layout {
    pub rows: Vec<RowStub>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub id: String,
    pub title: String,
    pub ranked: bool,
    pub items: Vec<Card>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(rename = "avatarUrl", skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
}

/// One row of Douban's own search results: what its mobile page lists.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchItem {
    pub id: String,
    pub title: String,
    pub rating: f32,
    pub poster_url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Detail {
    pub card: Card,
    pub directors: Vec<Person>,
    pub cast: Vec<Person>,
    pub country: String,
    pub language: String,
    pub released: String,
    pub aliases: Vec<String>,
    pub source_url: String,
}

#[derive(Debug)]
pub enum DoubanError {
    NotFound,
    Unreachable(String),
    Upstream(String),
    Unreadable(String),
}

impl std::fmt::Display for DoubanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DoubanError::NotFound => write!(f, "该豆瓣条目或榜单不存在"),
            DoubanError::Unreachable(message) => write!(f, "无法连通豆瓣：{message}"),
            DoubanError::Upstream(message) => write!(f, "豆瓣请求失败：{message}"),
            DoubanError::Unreadable(message) => write!(f, "豆瓣返回了无法解析的内容：{message}"),
        }
    }
}

impl std::error::Error for DoubanError {}

struct Cached {
    at: Instant,
    ttl: Duration,
    value: Value,
}

struct Client {
    cache: Mutex<HashMap<String, Cached>>,
    gate: Mutex<Instant>,
}

fn client() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT.get_or_init(|| Client {
        cache: Mutex::new(HashMap::new()),
        gate: Mutex::new(Instant::now() - INTERVAL),
    })
}

/// Wait until this process may call Douban again. Reserving the slot under the
/// lock (and sleeping after releasing it) keeps concurrent callers spaced apart
/// without ever holding the mutex across a network read.
fn claim_slot() {
    let gate = &client().gate;
    let start = {
        let mut next = gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let start = (*next).max(Instant::now());
        *next = start + INTERVAL;
        start
    };
    let now = Instant::now();
    if start > now {
        std::thread::sleep(start - now);
    }
}

fn fetch_json(url: &str) -> Result<Value, DoubanError> {
    claim_slot();
    let response = Fetch::new()
        .purpose("douban")
        .header("User-Agent", USER_AGENT)
        .header("Referer", REFERER)
        .max_time(TIMEOUT_SECS)
        .get(url)
        .map_err(|error| DoubanError::Unreachable(error.to_string()))?;
    if response.status == 404 {
        return Err(DoubanError::NotFound);
    }
    if !(200..300).contains(&response.status) {
        return Err(DoubanError::Upstream(format!("HTTP {}", response.status)));
    }
    response
        .json()
        .map_err(|error| DoubanError::Unreadable(error.to_string()))
}

/// A cached JSON answer for `key`, fetched when it has aged out. The lock is
/// only held for the map, never for the call.
fn cached_json(
    key: String,
    ttl: Duration,
    fetch: impl FnOnce() -> Result<Value, DoubanError>,
) -> Result<Value, DoubanError> {
    {
        let cache = client()
            .cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(entry) = cache.get(&key) {
            if entry.at.elapsed() < entry.ttl {
                return Ok(entry.value.clone());
            }
        }
    }
    let value = fetch()?;
    let mut cache = client()
        .cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    cache.insert(
        key,
        Cached {
            at: Instant::now(),
            ttl,
            value: value.clone(),
        },
    );
    Ok(value)
}

struct Spec {
    collection: &'static str,
    title: &'static str,
    ranked: bool,
}

// The rows Douban's own movie page carries, in the order a browse page wants
// them: what is hot now, then what is in cinemas, then the enduring lists.
const MOVIE: &[Spec] = &[
    Spec {
        collection: "movie_real_time_hotest",
        title: "豆瓣实时热门电影",
        ranked: true,
    },
    Spec {
        collection: "movie_showing",
        title: "影院热映",
        ranked: false,
    },
    Spec {
        collection: "movie_soon",
        title: "即将上映",
        ranked: false,
    },
    Spec {
        collection: "movie_hot_gaia",
        title: "豆瓣热门电影",
        ranked: false,
    },
    Spec {
        collection: "movie_weekly_best",
        title: "豆瓣一周口碑电影榜",
        ranked: true,
    },
    Spec {
        collection: "movie_high_score",
        title: "豆瓣高分电影",
        ranked: false,
    },
    Spec {
        collection: "movie_top250",
        title: "豆瓣电影 Top 250",
        ranked: true,
    },
    Spec {
        collection: "movie_classic",
        title: "经典电影",
        ranked: false,
    },
    Spec {
        collection: "movie_scifi",
        title: "高分经典科幻片榜",
        ranked: false,
    },
    Spec {
        collection: "movie_comedy",
        title: "高分经典喜剧片榜",
        ranked: false,
    },
    Spec {
        collection: "movie_action",
        title: "高分经典动作片榜",
        ranked: false,
    },
    Spec {
        collection: "movie_love",
        title: "高分经典爱情片榜",
        ranked: false,
    },
];

const TV: &[Spec] = &[
    Spec {
        collection: "tv_real_time_hotest",
        title: "豆瓣实时热门剧集",
        ranked: true,
    },
    Spec {
        collection: "tv_hot",
        title: "近期热门剧集",
        ranked: false,
    },
    Spec {
        collection: "tv_chinese_best_weekly",
        title: "华语口碑剧集榜",
        ranked: true,
    },
    Spec {
        collection: "tv_global_best_weekly",
        title: "全球口碑剧集榜",
        ranked: true,
    },
    Spec {
        collection: "tv_domestic",
        title: "近期热门国产剧",
        ranked: false,
    },
    Spec {
        collection: "tv_american",
        title: "近期热门美剧",
        ranked: false,
    },
    Spec {
        collection: "tv_japanese",
        title: "近期热门日剧",
        ranked: false,
    },
    Spec {
        collection: "tv_korean",
        title: "近期热门韩剧",
        ranked: false,
    },
    Spec {
        collection: "tv_animation",
        title: "近期热门动画",
        ranked: false,
    },
    Spec {
        collection: "show_domestic",
        title: "近期热门国内综艺",
        ranked: false,
    },
    Spec {
        collection: "show_foreign",
        title: "近期热门国外综艺",
        ranked: false,
    },
];

fn specs(kind: Kind) -> &'static [Spec] {
    match kind {
        Kind::Movie => MOVIE,
        Kind::Tv => TV,
    }
}

fn spec(kind: Kind, row_id: &str) -> Option<&'static Spec> {
    specs(kind)
        .iter()
        .find(|spec| format!("douban-{}", spec.collection) == row_id)
}

/// The rows this source offers, as pure configuration: no request is made.
pub fn layout(kind: Kind) -> Layout {
    Layout {
        rows: specs(kind)
            .iter()
            .map(|spec| RowStub {
                id: format!("douban-{}", spec.collection),
                title: spec.title.to_string(),
                ranked: spec.ranked,
            })
            .collect(),
    }
}

pub fn row(kind: Kind, row_id: &str) -> Result<Row, DoubanError> {
    let spec = spec(kind, row_id).ok_or(DoubanError::NotFound)?;
    let url = format!(
        "{BASE}/subject_collection/{}/items?start=0&count=30&items_only=1&for_mobile=1",
        spec.collection
    );
    let data = cached_json(format!("collection:{}", spec.collection), ROW_TTL, || {
        fetch_json(&url)
    })?;
    let mut items: Vec<Card> = data
        .get("subject_collection_items")
        .and_then(Value::as_array)
        .map(|raw| {
            raw.iter()
                .filter_map(|entry| card_from(entry, kind))
                .collect()
        })
        .unwrap_or_default();
    if items.len() < MIN_ROW_ITEMS {
        items.clear();
    }
    Ok(Row {
        id: format!("douban-{}", spec.collection),
        title: spec.title.to_string(),
        ranked: spec.ranked,
        items,
    })
}

/// Douban's own search. Its mobile page is HTML, so this reads the list it
/// prints; the shapes here are the ones its search results carry, nothing more.
pub fn search(raw: &str) -> Result<Vec<SearchItem>, DoubanError> {
    let query = raw.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let key = format!("search:{}", query.to_lowercase());
    let url = format!(
        "https://m.douban.com/search/?query={}&type=1002",
        percent_encode(query)
    );
    let data = cached_json(key, SEARCH_TTL, || {
        let html = fetch_text(&url)?;
        Ok(Value::Array(parse_search(&html)))
    })?;
    Ok(serde_json::from_value(data).unwrap_or_default())
}

const SEARCH_TTL: Duration = Duration::from_secs(10 * 60);

fn fetch_text(url: &str) -> Result<String, DoubanError> {
    claim_slot();
    let response = Fetch::new()
        .purpose("douban")
        .header("User-Agent", USER_AGENT)
        .header("Referer", REFERER)
        .max_time(TIMEOUT_SECS)
        .get(url)
        .map_err(|error| DoubanError::Unreachable(error.to_string()))?;
    if response.status == 404 {
        return Err(DoubanError::NotFound);
    }
    if !(200..300).contains(&response.status) {
        return Err(DoubanError::Upstream(format!("HTTP {}", response.status)));
    }
    Ok(response.text())
}

fn percent_encode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            b' ' => out.push_str("%20"),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

fn parse_search(html: &str) -> Vec<Value> {
    let Some(start) = html.find("search_results_subjects") else {
        return Vec::new();
    };
    let body = &html[start..];
    let end = body.find("</ul>").unwrap_or(body.len());
    let body = &body[..end];
    let Ok(item) = Regex::new(r"(?s)<li[^>]*>(.*?)</li>") else {
        return Vec::new();
    };
    let Ok(id) = Regex::new(r#"/movie/subject/(\d+)/"#) else {
        return Vec::new();
    };
    let Ok(title) = Regex::new(r#"class="subject-title"[^>]*>([^<]+)<"#) else {
        return Vec::new();
    };
    let Ok(image) = Regex::new(r#"<img[^>]+src="([^"]+)""#) else {
        return Vec::new();
    };
    let Ok(rating) = Regex::new(r#"data-rating="([\d.]+)""#) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for block in item.captures_iter(body) {
        let text = block.get(1).map(|part| part.as_str()).unwrap_or_default();
        let Some(id) = id.captures(text).and_then(|caps| caps.get(1)) else {
            continue;
        };
        let Some(title) = title.captures(text).and_then(|caps| caps.get(1)) else {
            continue;
        };
        let Some(poster) = image.captures(text).and_then(|caps| caps.get(1)) else {
            continue;
        };
        let score = rating
            .captures(text)
            .and_then(|caps| caps.get(1))
            .and_then(|value| value.as_str().parse::<f32>().ok())
            // The search page prints the score out of 100; the rest of this
            // module speaks the ten-point scale.
            .map(|value| ((value / 10.0) * 10.0).round() / 10.0)
            .unwrap_or(0.0);
        let poster = poster
            .as_str()
            .replace("/s_ratio_poster/", "/m_ratio_poster/");
        out.push(serde_json::json!(SearchItem {
            id: id.as_str().to_string(),
            title: title.as_str().trim().to_string(),
            rating: score,
            poster_url: poster,
        }));
    }
    out
}

pub fn detail(id: &str) -> Result<Detail, DoubanError> {
    let id = id.trim();
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return Err(DoubanError::NotFound);
    }
    let data = cached_json(format!("detail:{id}"), DETAIL_TTL, || {
        fetch_json(&format!("{BASE}/movie/{id}?for_mobile=1"))
    })?;
    // The cast strip needs avatars and role names, which the detail payload
    // does not carry. This hop failing only degrades the strip.
    let celebrities = cached_json(format!("celebrities:{id}"), DETAIL_TTL, || {
        fetch_json(&format!("{BASE}/movie/{id}/celebrities?for_mobile=1"))
    })
    .unwrap_or(Value::Null);
    detail_from(&data, &celebrities)
}

fn text(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn number(value: Option<&Value>) -> Option<f64> {
    match value {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.parse().ok(),
        _ => None,
    }
}

fn array_of_strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn year_of(raw: &Value, fallback: Option<&str>) -> Option<u32> {
    let text = match raw {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        _ => fallback.unwrap_or("").to_string(),
    };
    let head: String = text.chars().take(4).collect();
    (head.len() == 4 && head.bytes().all(|b| b.is_ascii_digit()))
        .then(|| head.parse().ok())
        .flatten()
}

fn poster_of(raw: &Value) -> Option<String> {
    text(raw.get("cover").and_then(|cover| cover.get("url")))
        .or_else(|| text(raw.get("pic").and_then(|pic| pic.get("large"))))
        .or_else(|| text(raw.get("cover_url")))
}

fn backdrop_of(raw: &Value) -> Option<String> {
    let photo = raw.get("photos").and_then(Value::as_array)?.first()?;
    text(Some(photo)).or_else(|| text(photo.get("large")).or_else(|| text(photo.get("url"))))
}

fn rating_of(raw: &Value) -> f32 {
    number(raw.get("rating").and_then(|rating| rating.get("value")))
        .map(|value| (value * 10.0).round() / 10.0)
        .unwrap_or(0.0) as f32
}

fn card_from(raw: &Value, kind: Kind) -> Option<Card> {
    if let Some(raw_kind) = raw.get("type").and_then(Value::as_str) {
        if raw_kind != kind_str(kind) {
            return None;
        }
    }
    let id = text(raw.get("id"))?;
    let title = text(raw.get("title"))?;
    let subtitle = text(raw.get("card_subtitle")).unwrap_or_default();
    let fallback_year = subtitle.split(" / ").next();
    let year = year_of(raw.get("year").unwrap_or(&Value::Null), fallback_year)?;
    let poster = poster_of(raw)?;
    let mut genres = array_of_strings(raw.get("genres"));
    if genres.is_empty() {
        let parts: Vec<&str> = subtitle.split(" / ").collect();
        if let Some(text) = parts.get(2) {
            genres = text.split_whitespace().map(str::to_string).collect();
        }
    }
    genres.truncate(3);
    let overview = text(raw.get("description"))
        .or_else(|| text(raw.get("info")))
        .unwrap_or_else(|| subtitle.clone());
    let original_title = text(raw.get("original_title")).unwrap_or_else(|| title.clone());
    Some(Card {
        id,
        kind,
        title,
        original_title,
        year: Some(year),
        rating: rating_of(raw),
        genres,
        overview,
        poster_url: poster,
        backdrop_url: backdrop_of(raw),
    })
}

fn kind_str(kind: Kind) -> &'static str {
    match kind {
        Kind::Movie => "movie",
        Kind::Tv => "tv",
    }
}

fn person_avatar(person: &Value) -> Option<String> {
    match person.get("avatar").or_else(|| person.get("cover_url")) {
        Some(Value::String(url)) => {
            let url = url.trim();
            (!url.is_empty()).then(|| url.to_string())
        }
        Some(avatar) => text(avatar.get("large"))
            .or_else(|| text(avatar.get("normal")))
            .or_else(|| text(avatar.get("small"))),
        None => None,
    }
}

/// Strip the "饰 " prefix and the Latin tail Dubbo appends to a Chinese role,
/// as the reference implementation does: a narrow card only has room for one.
fn role_of(person: &Value) -> Option<String> {
    let role = text(person.get("character"))?;
    let role = role
        .trim_start_matches("饰 ")
        .trim_start_matches("饰演 ")
        .trim();
    if role.is_empty() {
        return None;
    }
    if role.chars().any(is_cjk) {
        if let Some(cut) = latin_tail_start(role) {
            let trimmed = role[..cut].trim();
            return (!trimmed.is_empty()).then_some(trimmed.to_string());
        }
    }
    Some(role.to_string())
}

/// Where the Latin original starts ("安迪·杜佛兰 Andy Dufresne"): the first
/// whitespace followed by nothing but Latin letters, digits and their usual
/// punctuation through the end. `None` when the role is all one script.
fn latin_tail_start(role: &str) -> Option<usize> {
    for (index, c) in role.char_indices() {
        if !c.is_whitespace() {
            continue;
        }
        let rest = role[index..].trim_start();
        let latin_tail = rest.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
            && rest
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | '\'' | '’' | '-'));
        if latin_tail {
            return Some(index);
        }
    }
    None
}

fn is_cjk(c: char) -> bool {
    matches!(
        c as u32,
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F
    )
}

fn people_from(people: Option<&Value>) -> Vec<Person> {
    people
        .and_then(Value::as_array)
        .map(|people| {
            people
                .iter()
                .take(MAX_PEOPLE)
                .filter_map(|person| {
                    let name = text(person.get("name"))?;
                    Some(Person {
                        name,
                        role: role_of(person),
                        avatar_url: person_avatar(person),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn detail_from(data: &Value, celebrities: &Value) -> Result<Detail, DoubanError> {
    let kind = if data.get("type").and_then(Value::as_str) == Some("tv")
        || data.get("is_tv").and_then(Value::as_bool) == Some(true)
    {
        Kind::Tv
    } else {
        Kind::Movie
    };
    let title =
        text(data.get("title")).ok_or_else(|| DoubanError::Unreadable("缺少标题".into()))?;
    let year = year_of(data.get("year").unwrap_or(&Value::Null), None)
        .ok_or_else(|| DoubanError::Unreadable("缺少年份".into()))?;
    let poster = poster_of(data).ok_or_else(|| DoubanError::Unreadable("缺少海报".into()))?;
    let aliases = array_of_strings(data.get("aka"));
    let original_title = text(data.get("original_title"))
        .or_else(|| aliases.iter().find(|alias| alias.is_ascii()).cloned())
        .unwrap_or_else(|| title.clone());
    let mut genres = array_of_strings(data.get("genres"));
    genres.truncate(3);
    let overview = text(data.get("intro")).unwrap_or_default();
    let card = Card {
        id: text(data.get("id")).unwrap_or_default(),
        kind,
        title,
        original_title,
        year: Some(year),
        rating: rating_of(data),
        genres,
        overview,
        poster_url: poster,
        backdrop_url: backdrop_of(data),
    };
    let directors = people_from(
        celebrities
            .get("directors")
            .or_else(|| data.get("directors")),
    );
    let cast = people_from(celebrities.get("actors").or_else(|| data.get("actors")));
    let released = text(data.get("release_date"))
        .or_else(|| array_of_strings(data.get("pubdate")).into_iter().next())
        .unwrap_or_default();
    Ok(Detail {
        card,
        directors,
        cast,
        country: array_of_strings(data.get("countries")).join(" / "),
        language: array_of_strings(data.get("languages")).join(" / "),
        released,
        aliases,
        source_url: text(data.get("url"))
            .or_else(|| text(data.get("sharing_url")))
            .unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_card_keeps_what_a_poster_needs_and_drops_the_rest() {
        // The live shape: no `year`, no `cover` object; the year rides in the
        // subtitle and the poster in `cover_url`.
        let raw = serde_json::json!({
            "id": "1292052",
            "type": "movie",
            "title": "肖申克的救赎",
            "card_subtitle": "1994 / 美国 / 剧情 犯罪",
            "rating": {"value": 9.7},
            "cover_url": "https://img9.doubanio.com/x.jpg",
            "photos": [{"large": "https://img9.doubanio.com/b.jpg"}],
            "description": "希望让人自由。"
        });

        let card = card_from(&raw, Kind::Movie).expect("a card");

        assert_eq!(card.id, "1292052");
        assert_eq!(card.year, Some(1994));
        assert_eq!(card.rating, 9.7);
        assert_eq!(card.genres, ["剧情", "犯罪"]);
        assert_eq!(card.poster_url, "https://img9.doubanio.com/x.jpg");
        assert_eq!(
            card.backdrop_url.as_deref(),
            Some("https://img9.doubanio.com/b.jpg")
        );
    }

    #[test]
    fn a_card_of_the_other_kind_is_not_mixed_into_a_row() {
        let raw = serde_json::json!({
            "id": "1", "type": "tv", "title": "X", "year": "2020",
            "cover": {"url": "https://img/x.jpg"}
        });

        assert!(card_from(&raw, Kind::Movie).is_none());
    }

    #[test]
    fn a_card_without_a_poster_is_left_out() {
        let raw = serde_json::json!({"id": "1", "type": "movie", "title": "X", "year": "2020"});

        assert!(card_from(&raw, Kind::Movie).is_none());
    }

    #[test]
    fn a_detail_takes_avatars_from_the_celebrities_hop() {
        let data = serde_json::json!({
            "id": "1292052",
            "type": "movie",
            "title": "肖申克的救赎",
            "year": "1994",
            "cover_url": "https://img9.doubanio.com/x.jpg",
            "rating": {"value": 9.7},
            "genres": ["剧情", "犯罪"],
            "intro": "希望让人自由。",
            "countries": ["美国"],
            "languages": ["英语"],
            "pubdate": ["1994-09-10"],
            "aka": ["The Shawshank Redemption"],
            "url": "https://movie.douban.com/subject/1292052/",
            "directors": [{"name": "弗兰克·德拉邦特"}],
            "actors": [{"name": "蒂姆·罗宾斯"}]
        });
        let celebrities = serde_json::json!({
            "directors": [{"name": "弗兰克·德拉邦特", "avatar": {"large": "https://img/d.jpg"}}],
            "actors": [{
                "name": "蒂姆·罗宾斯",
                "character": "安迪·杜佛兰 Andy Dufresne",
                "avatar": {"large": "https://img/a.jpg"}
            }]
        });

        let detail = detail_from(&data, &celebrities).expect("a detail");

        assert_eq!(detail.card.year, Some(1994));
        assert_eq!(
            detail.directors[0].avatar_url.as_deref(),
            Some("https://img/d.jpg")
        );
        assert_eq!(detail.cast[0].role.as_deref(), Some("安迪·杜佛兰"));
        assert_eq!(detail.country, "美国");
        assert_eq!(detail.aliases, ["The Shawshank Redemption"]);
    }

    #[test]
    fn a_row_is_known_by_its_static_id_without_a_request() {
        let layout = layout(Kind::Movie);

        assert!(layout
            .rows
            .iter()
            .any(|row| row.id == "douban-movie_top250"));
        assert!(spec(Kind::Movie, "douban-movie_top250").is_some());
        assert!(spec(Kind::Movie, "douban-tv_hot").is_none());
        assert!(spec(Kind::Tv, "douban-tv_hot").is_some());
    }
}
