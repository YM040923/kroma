//! 115's own record of what happened on the drive, polled cheaply.
//!
//! Every upload, save-from-share, move, rename and delete lands as a *life
//! event*. A poll asks for the newest page only and stops the moment it reaches
//! the id the last round consumed, so a quiet round is one request. The app
//! channel answers faster but refuses more readily; after it refuses while the
//! web channel answers, the web one is used for a day.
//!
//! Two things this cannot see, both covered by the full mirror instead: a batch
//! delete made in the web UI (the events exist, but a delete of a folder only
//! names the folder), and anything that happened while the server was down past
//! the event window. The periodic full sync is the backstop for both.
//!
//! Endpoint knowledge is from 115-Station (https://github.com/redwebsite/115-station),
//! by way of p115client: `behavior/detail` plus the `calendar/setoption` switch
//! that turns the recording on.

use serde::{Deserialize, Serialize};

use kroma_module_sdk::http::Fetch;

use crate::client::{ApiError, MODULE_UA};
use crate::pacer::pacer;
use crate::store::IncrState;

const APP_EVENTS: &str = "https://proapi.115.com/android/behavior/detail";
const WEB_EVENTS: &str = "https://webapi.115.com/behavior/detail";
const LIFE_SETOPTION: &str = "https://life.115.com/api/1.0/web/1.0/calendar/setoption";

/// The channels the endpoint can be reached on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    App,
    Web,
}

/// One recorded change on the drive.
#[derive(Debug, Clone, Default)]
pub struct Event {
    pub id: String,
    /// 2 upload, 14 saved-from-share, 6 move, 22 delete, 24 rename, 17/18 folder, …
    pub kind: u16,
    pub file_id: String,
    pub name: String,
    pub parent_cid: String,
    pub pick_code: String,
    pub is_dir: bool,
    pub time: i64,
    pub size: i64,
}

impl Event {
    /// Events that add or move content in (what the mirror cares about).
    pub fn adds(&self) -> bool {
        matches!(self.kind, 1 | 2 | 5 | 6 | 14 | 17 | 18 | 20 | 23 | 24)
    }

    pub fn deletes(&self) -> bool {
        self.kind == 22
    }

    /// Browsing, starring and labelling: irrelevant to the mirror and the bulk of
    /// a busy account's events.
    pub fn ignored(&self) -> bool {
        matches!(self.kind, 3 | 4 | 7 | 8 | 9 | 10 | 19)
    }
}

/// The cursor: the newest event this module has fully applied.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Cursor {
    pub id: String,
    pub time: i64,
}

impl Cursor {
    pub fn is_zero(&self) -> bool {
        self.id.is_empty() && self.time == 0
    }

    fn reached(&self, event: &Event) -> bool {
        if !self.id.is_empty() && !id_newer(&event.id, &self.id) {
            return true;
        }
        self.time > 0 && event.time > 0 && event.time < self.time
    }
}

/// Event ids are long decimal strings; comparing them as text would rank a
/// shorter id above a longer one.
fn id_newer(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return a.len() > b.len();
    }
    a > b
}

/// What one poll learned, for the caller to persist.
#[derive(Debug, Default)]
pub struct Poll {
    pub events: Vec<Event>,
    pub cursor: Cursor,
    pub proapi_405: u32,
    pub web_until: i64,
}

fn error(status: u16, message: impl Into<String>) -> ApiError {
    ApiError {
        http: status,
        errno: 0,
        message: message.into(),
    }
}

fn text_at(item: &serde_json::Value, keys: &[&str]) -> String {
    for key in keys {
        if let Some(value) = item.get(*key) {
            let text = match value {
                serde_json::Value::String(text) => text.clone(),
                serde_json::Value::Number(number) => number.to_string(),
                _ => continue,
            };
            if !text.is_empty() && text != "null" {
                return text;
            }
        }
    }
    String::new()
}

fn kind_of(item: &serde_json::Value) -> u16 {
    let raw = text_at(item, &["type"]);
    match raw.as_str() {
        "1" | "upload_image_file" => 1,
        "2" | "upload_file" => 2,
        "3" | "star_image" => 3,
        "4" | "star_file" => 4,
        "5" | "move_image_file" => 5,
        "6" | "move_file" => 6,
        "7" | "browse_image" => 7,
        "8" | "browse_video" => 8,
        "9" | "browse_audio" => 9,
        "10" | "browse_document" => 10,
        "14" | "receive_files" => 14,
        "17" | "new_folder" => 17,
        "18" | "copy_folder" => 18,
        "19" | "folder_label" => 19,
        "20" | "folder_rename" => 20,
        "22" | "delete_file" => 22,
        "23" | "copy_file" => 23,
        "24" | "file_rename" => 24,
        other => other.parse().unwrap_or(0),
    }
}

fn event_of(item: &serde_json::Value) -> Event {
    let category = text_at(item, &["file_category", "fc"]);
    Event {
        id: text_at(item, &["id"]),
        kind: kind_of(item),
        file_id: text_at(item, &["file_id", "fid"]),
        name: text_at(item, &["file_name", "n", "name"]),
        parent_cid: text_at(item, &["parent_id", "cid", "pid"]),
        pick_code: text_at(item, &["pick_code", "pickcode", "pc"]),
        is_dir: category == "0",
        time: text_at(item, &["update_time", "time", "create_time"])
            .parse()
            .unwrap_or(0),
        size: item
            .get("file_size")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0),
    }
}

/// Turn 115's life-event recording on. Without it `behavior/detail` answers an
/// empty list forever, which looks exactly like a quiet drive.
pub async fn enable_life(cookie: &str) -> Result<(), String> {
    pacer().acquire().await?;
    let response = Fetch::default()
        .purpose("cloud-drive")
        .header("Cookie", cookie)
        .header("User-Agent", MODULE_UA)
        .header("Referer", "https://life.115.com/")
        .max_time(20)
        .post_form(LIFE_SETOPTION, &[("locus", "1"), ("open_life", "1")])
        .map_err(|e| e.to_string())?;
    pacer().note(response.status, 0).await;
    if !(200..300).contains(&response.status) {
        return Err(format!("HTTP {}", response.status));
    }
    let body = response.text();
    let parsed: serde_json::Value =
        serde_json::from_str(&body).map_err(|_| "响应无法解析".to_string())?;
    if parsed.get("state").and_then(serde_json::Value::as_bool) == Some(true) {
        Ok(())
    } else {
        Err(text_at(&parsed, &["message", "error"]))
    }
}

/// Read one page of events.
async fn page(
    cookie: &str,
    channel: Channel,
    limit: u32,
    offset: u32,
) -> Result<(Vec<Event>, i64), ApiError> {
    pacer().acquire().await.map_err(|reason| error(0, reason))?;
    let url = match channel {
        Channel::App => APP_EVENTS,
        Channel::Web => WEB_EVENTS,
    };
    let query = format!("?limit={limit}&offset={offset}");
    let response = Fetch::default()
        .purpose("cloud-drive")
        .header("Cookie", cookie)
        .header("User-Agent", MODULE_UA)
        .header("Referer", "https://115.com/")
        .max_time(30)
        .get(&format!("{url}{query}"))
        .map_err(|e| error(0, e.to_string()))?;
    pacer().note(response.status, 0).await;
    if !(200..300).contains(&response.status) {
        return Err(error(
            response.status,
            if response.status == 405 {
                "事件接口被限流".to_string()
            } else {
                format!("事件接口 HTTP {}", response.status)
            },
        ));
    }
    let body = response.text();
    let parsed: serde_json::Value = serde_json::from_str(&body).map_err(|_| {
        error(
            200,
            format!(
                "事件响应无法解析：{}",
                body.chars().take(120).collect::<String>()
            ),
        )
    })?;
    if parsed.get("state").and_then(serde_json::Value::as_bool) != Some(true) {
        let message = text_at(&parsed, &["error", "message"]);
        return Err(error(
            200,
            if message.is_empty() {
                "事件接口 state=false".to_string()
            } else {
                message
            },
        ));
    }
    let data = parsed.get("data").cloned().unwrap_or_default();
    // The count is a string on the app channel and a number on the web one.
    let count = match data.get("count") {
        Some(serde_json::Value::String(text)) => text.parse().unwrap_or(0),
        Some(serde_json::Value::Number(number)) => number.as_i64().unwrap_or(0),
        _ => 0,
    };
    let list = data
        .get("list")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok((list.iter().map(event_of).collect(), count))
}

/// One page on the channel the last rounds settled on, with the fallback the
/// drive makes necessary.
async fn page_with_fallback(
    cookie: &str,
    state: &IncrState,
    limit: u32,
    offset: u32,
) -> Result<(Vec<Event>, i64, Channel, bool), ApiError> {
    let now = unix_now();
    let channel = if state.web_until > now {
        Channel::Web
    } else {
        Channel::App
    };
    match page(cookie, channel, limit, offset).await {
        Ok((events, count)) => Ok((events, count, channel, false)),
        Err(failure) if failure.http == 405 && channel == Channel::Web => {
            // The web channel refused too: forget the sticky choice and try the
            // app one again, reporting its answer.
            page(cookie, Channel::App, limit, offset)
                .await
                .map(|(events, count)| (events, count, Channel::App, true))
                .map_err(|_| failure)
        }
        Err(failure) if failure.http == 405 => {
            match page(cookie, Channel::Web, limit, offset).await {
                Ok((events, count)) => Ok((events, count, Channel::Web, true)),
                Err(_) => Err(failure),
            }
        }
        Err(failure) => Err(failure),
    }
}

/// Poll from the cursor to the newest event. Stops at the cursor, drops browse
/// noise, and keeps only the newest event per file.
pub async fn poll(cookie: &str, state: &IncrState, max: usize) -> Result<Poll, ApiError> {
    let cursor = Cursor {
        id: state.cursor_id.clone(),
        time: state.cursor_time,
    };
    let max = max.max(1);
    let mut limit: u32 = if cursor.is_zero() { 200 } else { 64 };
    let mut offset: u32 = 0;
    let mut poll = Poll {
        cursor: cursor.clone(),
        proapi_405: state.proapi_405,
        web_until: state.web_until,
        events: Vec::new(),
    };
    let mut seen = std::collections::HashSet::new();
    let mut first = true;

    for _ in 0..10 {
        let (events, count, channel, fell_back) =
            page_with_fallback(cookie, state, limit, offset).await?;
        if channel == Channel::App && !fell_back {
            poll.proapi_405 = 0;
        }
        if fell_back && channel == Channel::Web {
            poll.proapi_405 = state.proapi_405.saturating_add(1);
            if poll.proapi_405 >= 3 {
                poll.web_until = unix_now() + 24 * 3600;
            }
        } else if fell_back {
            poll.web_until = 0;
        }
        if first {
            first = false;
            if let Some(newest) = events.first() {
                if !newest.id.is_empty() {
                    poll.cursor = Cursor {
                        id: newest.id.clone(),
                        time: newest.time,
                    };
                }
            }
        }
        if events.is_empty() {
            break;
        }
        for event in &events {
            if event.id.is_empty() {
                continue;
            }
            if cursor.reached(event) {
                return Ok(poll);
            }
            if !event.file_id.is_empty() && !seen.insert(event.file_id.clone()) {
                continue;
            }
            if event.ignored() {
                continue;
            }
            poll.events.push(event.clone());
            if poll.events.len() >= max {
                return Ok(poll);
            }
        }
        offset += events.len() as u32;
        if count > 0 && offset >= count as u32 {
            break;
        }
        limit = 200;
    }
    Ok(poll)
}

pub fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_ordered_by_length_before_letters() {
        assert!(id_newer("1000000000000000000", "999999999999999999"));
        assert!(!id_newer("999999999999999999", "1000000000000000000"));
        assert!(id_newer("24", "23"));
    }

    #[test]
    fn a_cursor_stops_at_its_own_event() {
        let event = Event {
            id: "200".into(),
            time: 1_700_000_000,
            ..Event::default()
        };
        let cursor = Cursor {
            id: "200".into(),
            time: 1_700_000_000,
        };
        assert!(cursor.reached(&event));
        let older = Cursor {
            id: "199".into(),
            time: 0,
        };
        assert!(!older.reached(&event));
    }

    #[test]
    fn browse_noise_is_dropped_and_saves_are_kept() {
        assert!(Event {
            kind: 8,
            ..Event::default()
        }
        .ignored());
        assert!(Event {
            kind: 4,
            ..Event::default()
        }
        .ignored());
        assert!(!Event {
            kind: 14,
            ..Event::default()
        }
        .ignored());
        assert!(Event {
            kind: 14,
            ..Event::default()
        }
        .adds());
        assert!(Event {
            kind: 22,
            ..Event::default()
        }
        .deletes());
    }

    #[test]
    fn an_event_parses_every_spelling_the_channels_use() {
        let item = serde_json::json!({
            "id": 1234567890123456789u64,
            "type": "14",
            "file_id": "42",
            "n": "Show.S01E01.mkv",
            "pid": "7",
            "pc": "abc",
            "fc": "1",
            "update_time": "1700000000",
            "file_size": 1234
        });
        let event = event_of(&item);
        assert_eq!(event.kind, 14);
        assert_eq!(event.name, "Show.S01E01.mkv");
        assert_eq!(event.parent_cid, "7");
        assert_eq!(event.pick_code, "abc");
        assert!(!event.is_dir);
        assert_eq!(event.time, 1_700_000_000);
        assert_eq!(event.size, 1234);
    }
}
