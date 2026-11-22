//! Following what a subscription already unlocked, and watching for new
//! episodes of what it has not.
//!
//! Two shapes of a MADREEL resource, and they cost differently:
//!
//! * **One episode per unlock.** Every new episode is its own share, so
//!   following it means finding the release and paying again. The budget rule
//!   is the operator's: the first priced release of a title asks once, and the
//!   answer is kept on the task (`approvedAt`). Later episodes reuse it.
//! * **One unlock, many episodes.** The share keeps receiving files after it was
//!   unlocked. Those additions cost nothing, so they are transferred on sight
//!   (`/receive-new`), without asking.
//!
//! Nothing here spends points on its own: an unapproved title only records what
//! it found, and the page shows it for a decision.

use serde_json::Value;

use crate::routes::source_json;
use crate::store::{self, FollowedShare, Item, Stored, Task};

/// How often the expensive half (search + probe) runs.
pub const SEARCH_EVERY_MS: i64 = 30 * 60 * 1000;
/// How often followed shares are checked for new files.
pub const FOLLOW_EVERY_MS: i64 = 5 * 60 * 1000;

/// `S01E05`, `s1e5`, `1x05`, `第05集`, `EP05` → `(season, episode)`. `None` when
/// the name carries no episode marker, which is what a film or a pack looks
/// like.
pub fn episode_of(name: &str) -> Option<(i64, i64)> {
    let lower = name.to_ascii_lowercase();
    if let Some(caps) = find_season_episode(&lower) {
        return Some(caps);
    }
    // `第05集` / `第5話` / `第 05 集`
    let mut digits = String::new();
    let mut in_episode = false;
    let chars: Vec<char> = name.chars().collect();
    for (i, ch) in chars.iter().enumerate() {
        if *ch == '第' {
            in_episode = true;
            digits.clear();
            continue;
        }
        if in_episode {
            if ch.is_ascii_digit() {
                digits.push(*ch);
            } else if *ch == '集' || *ch == '話' || *ch == '话' || *ch == '期' {
                if let Ok(episode) = digits.parse::<i64>() {
                    let season = season_before(&chars[..i]);
                    return Some((season, episode));
                }
                in_episode = false;
            } else if !ch.is_whitespace() {
                in_episode = false;
            }
        }
    }
    None
}

fn season_before(chars: &[char]) -> i64 {
    // Look back for `第N季` before the episode marker.
    let text: String = chars.iter().collect();
    let mut digits = String::new();
    let mut found = None;
    let mut in_season = false;
    for ch in text.chars() {
        if ch == '第' {
            in_season = true;
            digits.clear();
            continue;
        }
        if in_season {
            if ch.is_ascii_digit() {
                digits.push(ch);
            } else if ch == '季' {
                found = digits.parse::<i64>().ok();
                in_season = false;
            } else if !ch.is_whitespace() {
                in_season = false;
            }
        }
    }
    found.unwrap_or(0).max(0)
}

fn find_season_episode(lower: &str) -> Option<(i64, i64)> {
    let bytes = lower.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i] != b's' {
            continue;
        }
        let mut j = i + 1;
        let start = j;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if j == start {
            continue;
        }
        let season: i64 = lower[start..j].parse().ok()?;
        if j >= bytes.len() || bytes[j] != b'e' {
            continue;
        }
        let ep_start = j + 1;
        let mut k = ep_start;
        while k < bytes.len() && bytes[k].is_ascii_digit() {
            k += 1;
        }
        if k == ep_start {
            continue;
        }
        // Bound the season/episode to something a series can actually have; a
        // name like `s2024e01` still fits, but a hash would not.
        if season > 200 {
            continue;
        }
        if let Ok(episode) = lower[ep_start..k].parse::<i64>() {
            return Some((season, episode));
        }
    }
    // `1x05`
    for i in 1..bytes.len().saturating_sub(1) {
        if bytes[i] != b'x' {
            continue;
        }
        let mut a = i;
        while a > 0 && bytes[a - 1].is_ascii_digit() {
            a -= 1;
        }
        let mut b = i + 1;
        while b < bytes.len() && bytes[b].is_ascii_digit() {
            b += 1;
        }
        if a == i || b == i + 1 {
            continue;
        }
        let Ok(season) = lower[a..i].parse::<i64>() else {
            continue;
        };
        let Ok(episode) = lower[i + 1..b].parse::<i64>() else {
            continue;
        };
        if season <= 200 && episode <= 2000 {
            return Some((season, episode));
        }
    }
    None
}

/// The ledger key for one episode. A release without an episode marker keys on
/// its own name instead, so a film is still deduplicated.
pub fn episode_key(name: &str) -> String {
    match episode_of(name) {
        Some((season, episode)) => format!("{season}:{episode}"),
        None => format!("name:{name}"),
    }
}

pub fn already_sent(task: &Task, name: &str) -> bool {
    let key = episode_key(name);
    task.sent_episodes.iter().any(|sent| sent == &key)
}

pub fn record_sent(task: &mut Task, name: &str) {
    let key = episode_key(name);
    if task.sent_episodes.iter().any(|sent| sent == &key) {
        return;
    }
    task.sent_episodes.push(key);
    const CAP: usize = 500;
    if task.sent_episodes.len() > CAP {
        let drop = task.sent_episodes.len() - CAP;
        task.sent_episodes.drain(0..drop);
    }
}

/// Whether the task may spend points on a priced release without asking again.
/// Per title, as the operator chose: the first approval covers its episodes.
pub fn approved_for_points(task: &Task) -> bool {
    task.approved_at > 0
}

/// Attach the shares this account already unlocked for a title, so their later
/// additions are followed for free. Used right after a transfer was started
/// outside this module (the acquisition panel), where the source, not the
/// module, owns the job id.
pub fn attach_unlocked_shares(source: &str, task: &mut Task) {
    if task.tmdb_id <= 0 {
        return;
    }
    let Ok(value) = source_json(source, &format!("/unlocked?tmdb={}", task.tmdb_id), None) else {
        return;
    };
    let Some(shares) = value.get("shares").and_then(Value::as_array) else {
        return;
    };
    for share in shares {
        let code = share.get("shareCode").and_then(Value::as_str).unwrap_or("").trim();
        if code.is_empty() {
            continue;
        }
        if task.following_shares.iter().any(|one| one.share_code == code) {
            continue;
        }
        // A season this task does not want is not followed: the same title can
        // have several unlocked shares, one per season.
        let season = share.get("season").and_then(Value::as_i64).unwrap_or(0);
        if task.season > 0 && season > 0 && season != task.season {
            continue;
        }
        task.following_shares.push(FollowedShare {
            share_code: code.to_string(),
            password: share.get("password").and_then(Value::as_str).unwrap_or("").to_string(),
            name: share
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            unlocked_at: kroma_module_sdk::primitives::now_ms(),
            ..FollowedShare::default()
        });
    }
}

/// Take the releases `discover` found on titles the operator already approved:
/// one transfer per task per pass, never the same episode twice. This is the
/// per-title approval in action — a priced new episode spends points without
/// asking again, because the operator's answer was for the title, not the file.
pub fn take_auto(now: i64) -> bool {
    let mut stored = store::load();
    let source = stored.source_url.clone();
    if source.trim().is_empty() {
        return false;
    }
    let mut touched = false;
    let ids: Vec<String> = stored
        .tasks
        .iter()
        .filter(|task| task.enabled && task.kind == "show" && task.approved_at > 0)
        .map(|task| task.id.clone())
        .collect();
    for id in ids {
        let Some(position) = stored.tasks.iter().position(|task| task.id == id) else {
            continue;
        };
        if matches!(
            stored.tasks[position].status.as_str(),
            "unlocking" | "running" | "transferring"
        ) {
            continue;
        }
        let Some(item) = stored
            .items
            .iter_mut()
            .find(|item| item.task_id == id && item.state == "auto")
        else {
            continue;
        };
        let resource = item.resource.clone();
        let share_code = item.share_code.clone();
        let listed_index = item.index;
        if already_sent(&stored.tasks[position], &resource) {
            item.state = "skipped".into();
            touched = true;
            continue;
        }
        // The listing shifts every time a share is added, so an index read last
        // time can point at a different release now. Re-resolve it by the
        // source's share key; without one, the listing's own order is the only
        // identity there is.
        let index = if share_code.is_empty() {
            listed_index
        } else {
            match resolve_index(&source, stored.tasks[position].tmdb_id, &share_code) {
                Some(fresh) => fresh,
                None => {
                    item.state = "failed".into();
                    item.last_error = "这个分享在源站已找不到，跳过".into();
                    touched = true;
                    continue;
                }
            }
        };
        let kind = stored.tasks[position].kind.clone();
        let task = &mut stored.tasks[position];
        task.resource = resource.clone();
        task.resource_index = index;
        task.price = item.price;
        task.link = item.link.clone();
        item.state = "sent".into();
        item.index = index;
        task.last_run_at = now;
        record_sent(task, &resource);
        let started = crate::routes::start_transfer(task, &source, &kind);
        if !started {
            item.state = "failed".into();
            item.last_error = task.last_error.clone();
        }
        touched = true;
    }
    if touched {
        let _ = store::save(&stored);
    }
    touched
}

/// Follow every enabled task's unlocked shares and transfer what arrived since
/// the last pass. Cheap: it lists a share and only writes when there is more.
pub fn follow_shares(now: i64) -> bool {
    let mut stored = store::load();
    let source = stored.source_url.clone();
    if source.trim().is_empty() {
        return false;
    }
    if now - stored.last_follow_at < FOLLOW_EVERY_MS {
        return false;
    }
    let mut touched = false;
    let ids: Vec<String> = stored
        .tasks
        .iter()
        .filter(|task| task.enabled && !task.following_shares.is_empty())
        .map(|task| task.id.clone())
        .collect();
    for id in ids {
        let Some(position) = stored.tasks.iter().position(|task| task.id == id) else {
            continue;
        };
        let shares = stored.tasks[position].following_shares.clone();
        for share in shares {
            let outcome = receive_new(&source, &share, &mut stored.tasks[position]);
            if outcome {
                touched = true;
            }
        }
    }
    stored.last_follow_at = now;
    let _ = store::save(&stored);
    touched
}

/// Ask the source to transfer files this share gained since last time.
fn receive_new(source: &str, share: &FollowedShare, task: &mut Task) -> bool {
    let payload = serde_json::json!({ "shareCode": share.share_code });
    match source_json(source, "/receive-new", Some(&payload)) {
        Ok(value) => {
            let fresh = value.get("fresh").and_then(Value::as_u64).unwrap_or(0);
            let files = value
                .get("files")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let mut changed = fresh > 0;
            if let Some(followed) = task
                .following_shares
                .iter_mut()
                .find(|one| one.share_code == share.share_code)
            {
                let mut seen: Vec<String> = files
                    .iter()
                    .filter_map(|file| file.get("fid").and_then(Value::as_str))
                    .map(str::to_string)
                    .collect();
                seen.extend(followed.seen.iter().cloned());
                seen.sort();
                seen.dedup();
                if seen != followed.seen {
                    followed.seen = seen;
                    changed = true;
                }
                followed.last_checked_at = kroma_module_sdk::primitives::now_ms();
                followed.last_error.clear();
            }
            if fresh > 0 {
                task.last_run_at = kroma_module_sdk::primitives::now_ms();
                task.status = "done".into();
                task.last_error.clear();
            }
            changed
        }
        Err(reason) => {
            if let Some(followed) = task
                .following_shares
                .iter_mut()
                .find(|one| one.share_code == share.share_code)
            {
                followed.last_checked_at = kroma_module_sdk::primitives::now_ms();
                followed.last_error = reason.chars().take(200).collect();
            }
            false
        }
    }
}

/// Look for releases the task has not sent yet. Records them as items; it never
/// spends points. A new item on a title that was already approved is marked
/// `auto` so the page can say it will be taken on the next pass.
pub fn discover(source: &str, stored: &mut Stored, now: i64) {
    if now - stored.last_search_at < SEARCH_EVERY_MS {
        return;
    }
    let tasks: Vec<(String, String, i64, bool)> = stored
        .tasks
        .iter()
        .filter(|task| task.enabled && task.kind == "show")
        .map(|task| {
            (
                task.id.clone(),
                task.name.clone(),
                task.tmdb_id,
                approved_for_points(task),
            )
        })
        .collect();
    for (id, name, tmdb_id, approved) in tasks {
        // Prices are probed here on purpose: on a show listing the card carries
        // no file name at all, so the confirm page is the only place an episode
        // marker (S01E05 / 第05集) can come from. Probing costs nothing.
        let path = format!("/search?tmdb={tmdb_id}&type=tv&limit=20");
        let Ok(value) = source_json(source, &path, None) else {
            continue;
        };
        let candidates = value
            .get("candidates")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for candidate in candidates {
            let resource = candidate
                .get("fileName")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let share_code = candidate
                .get("shareKey")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            // No name and no share key is nothing we can follow or dedupe.
            if resource.is_empty() && share_code.is_empty() {
                continue;
            }
            if already_sent_per_task(stored, &id, &resource) {
                continue;
            }
            let identity = if resource.is_empty() {
                format!("share:{share_code}")
            } else {
                resource.clone()
            };
            let key = format!("{id}:{identity}");
            if stored.items.iter().any(|item| item.key == key) {
                continue;
            }
            stored.items.push(Item {
                key,
                task_id: id.clone(),
                title: name.clone(),
                state: if approved { "auto".into() } else { "new".into() },
                last_error: String::new(),
                resource,
                index: candidate.get("index").and_then(Value::as_i64).unwrap_or(0),
                size: candidate
                    .get("size")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                link: candidate
                    .get("link")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                price: candidate.get("price").and_then(Value::as_i64).unwrap_or(0),
                share_code,
            });
        }
    }
    let excess = stored.items.len().saturating_sub(500);
    if excess > 0 {
        stored.items.drain(0..excess);
    }
    stored.last_search_at = now;
}

fn already_sent_per_task(stored: &Stored, task_id: &str, name: &str) -> bool {
    stored
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .is_some_and(|task| already_sent(task, name))
}

/// Where a share sits in the task's listing right now. The probe run is what
/// names shares, so it is the same call the discovery made.
fn resolve_index(source: &str, tmdb_id: i64, share_code: &str) -> Option<i64> {
    let value = source_json(source, &format!("/search?tmdb={tmdb_id}&type=tv&limit=20"), None).ok()?;
    value
        .get("candidates")
        .and_then(Value::as_array)?
        .iter()
        .find(|candidate| {
            candidate
                .get("shareKey")
                .and_then(Value::as_str)
                .is_some_and(|key| key.trim() == share_code)
        })
        .and_then(|candidate| candidate.get("index").and_then(Value::as_i64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn episode_markers_are_read_from_the_usual_spellings() {
        assert_eq!(episode_of("Show.S01E05.1080p.mkv"), Some((1, 5)));
        assert_eq!(episode_of("Show s2e12 WEB-DL.mkv"), Some((2, 12)));
        assert_eq!(episode_of("[Group] Show - 1x07.mkv"), Some((1, 7)));
        assert_eq!(episode_of("某剧 第05集 1080p.mp4"), Some((0, 5)));
        assert_eq!(episode_of("某剧 第2季 第12集.mp4"), Some((2, 12)));
        assert_eq!(episode_of("The.Matrix.1999.mkv"), None);
    }

    #[test]
    fn the_ledger_never_lets_one_episode_count_twice() {
        let mut task = Task::default();
        record_sent(&mut task, "Show.S01E05.mkv");
        assert!(already_sent(&task, "Show.S01E05.2160p.other-group.mkv"));
        assert!(!already_sent(&task, "Show.S01E06.mkv"));
        record_sent(&mut task, "Show.S01E05.mkv");
        assert_eq!(task.sent_episodes.len(), 1);
    }

    #[test]
    fn a_film_without_an_episode_marker_keys_on_its_name() {
        let mut task = Task::default();
        record_sent(&mut task, "The.Matrix.1999.mkv");
        assert!(already_sent(&task, "The.Matrix.1999.mkv"));
        assert!(!already_sent(&task, "The.Matrix.Reloaded.2003.mkv"));
    }
}
