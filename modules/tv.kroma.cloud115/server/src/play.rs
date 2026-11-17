//! Signed links, kept for as long as the drive says they are good.
//!
//! A player never fetches a 115 link directly: the `.strm` this module writes
//! names a route here, and this route answers with the drive's own link as a
//! redirect, so the bytes go from the drive to the player and never through this
//! server. Signing is one API call, so a link is reused until shortly before it
//! expires — and it is cached per *player user agent*, because the drive binds a
//! link to whoever asked for it.

use std::collections::HashMap;
use std::sync::OnceLock;

use tokio::sync::Mutex;
use tokio::time::Instant;

use crate::client::{self, ApiError, DownloadLink};

/// Signed links kept at once. A household has a handful of players; past this
/// many agents the oldest are dropped rather than let a misbehaving client grow
/// the map without bound.
const MAX_CACHED: usize = 64;

/// Margin kept off a link's own deadline, so a player that starts fetching has
/// time to finish the request that follows.
const EXPIRY_MARGIN_SECS: u64 = 60;

struct Cached {
    link: DownloadLink,
    until: Instant,
}

/// A link handed to a player, and whether this call had to sign a new one.
pub struct Resolved {
    pub link: DownloadLink,
    pub cached: bool,
}

pub struct Links {
    entries: Mutex<HashMap<String, Cached>>,
}

static LINKS: OnceLock<Links> = OnceLock::new();

pub fn links() -> &'static Links {
    LINKS.get_or_init(|| Links {
        entries: Mutex::new(HashMap::new()),
    })
}

impl Links {
    /// A link for `pick_code` the agent `ua` can fetch. Cached per agent, since
    /// the drive signs to the client that asked.
    pub async fn resolve(
        &self,
        cookie: &str,
        pick_code: &str,
        ua: &str,
        ttl_secs: u64,
    ) -> Result<Resolved, ApiError> {
        let key = format!("{}\n{ua}", pick_code.trim().to_ascii_lowercase());
        {
            let mut entries = self.entries.lock().await;
            if let Some(cached) = entries.get(&key) {
                if cached.until > Instant::now() {
                    return Ok(Resolved {
                        link: cached.link.clone(),
                        cached: true,
                    });
                }
                entries.remove(&key);
            }
        }

        let link = client::sign(cookie, ua, pick_code).await?;
        let until = expiry_of(&link.url, ttl_secs);
        let mut entries = self.entries.lock().await;
        if entries.len() >= MAX_CACHED {
            let now = Instant::now();
            entries.retain(|_, cached| cached.until > now);
            if entries.len() >= MAX_CACHED {
                entries.clear();
            }
        }
        entries.insert(key, Cached {
            link: link.clone(),
            until,
        });
        Ok(Resolved { link, cached: false })
    }

    /// For the admin page: how many links are held, and how long the soonest
    /// one lasts.
    pub async fn snapshot(&self) -> (usize, Option<u64>) {
        let now = Instant::now();
        let entries = self.entries.lock().await;
        let soonest = entries
            .values()
            .filter(|cached| cached.until > now)
            .map(|cached| (cached.until - now).as_secs())
            .min();
        (entries.len(), soonest)
    }
}

/// When a signed link stops working.
///
/// The drive puts its own deadline in the URL (`t=`, unix seconds). Trust it,
/// less a margin; without one, fall back to the configured lifetime.
fn expiry_of(url: &str, ttl_secs: u64) -> Instant {
    let now = Instant::now();
    let ttl = std::time::Duration::from_secs(ttl_secs.max(30));
    let Some(deadline) = query_int(url, "t") else {
        return now + ttl;
    };
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(deadline.saturating_sub(ttl_secs));
    let left = deadline.saturating_sub(now_unix + EXPIRY_MARGIN_SECS);
    // A link already past its margin is not cached for long: sign again next time.
    now + ttl.min(std::time::Duration::from_secs(left.max(5)))
}

fn query_int(url: &str, key: &str) -> Option<u64> {
    let query = url.split_once('?')?.1.split('#').next()?;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        if k == key {
            return v.parse().ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_without_a_deadline_lives_for_the_configured_lifetime() {
        let until = expiry_of("https://cdn.test/a.mkv?sig=1", 600);
        let left = until.duration_since(Instant::now()).as_secs();
        assert!((590..=600).contains(&left), "{left}s");
    }

    #[test]
    fn a_links_own_deadline_wins_over_the_configured_one() {
        // 120 seconds from now, less the margin: far shorter than an hour.
        let deadline = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 120;
        let until = expiry_of(&format!("https://cdn.test/a.mkv?t={deadline}&f=1"), 3600);
        let left = until.duration_since(Instant::now()).as_secs();
        assert!(left <= 65, "cached {left}s past a deadline 120s away");
    }

    #[test]
    fn an_expired_link_is_replaced_almost_immediately() {
        let deadline = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .saturating_sub(600);
        let until = expiry_of(&format!("https://cdn.test/a.mkv?t={deadline}"), 3600);
        let left = until.duration_since(Instant::now()).as_secs();
        assert!(left <= 5, "an expired link must not be cached for {left}s");
    }
}
