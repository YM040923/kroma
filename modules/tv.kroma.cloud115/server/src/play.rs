//! Signed links, kept for as long as the drive says they are good.
//!
//! A player never fetches a 115 link directly: the `.strm` this module writes
//! names a route here, and this route answers with the drive's own link as a
//! redirect, so the bytes go from the drive to the player and never through this
//! server. Signing is one API call, so a link is reused until shortly before it
//! expires — and it is cached per *player user agent*, because the drive binds a
//! link to whoever asked for it.

mod shared;

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, Weak};
use std::time::Duration;
use tokio::sync::{watch, Mutex, OnceCell};
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

type Key = (String, String, String);
struct Flight {
    result: OnceCell<Result<DownloadLink, ApiError>>,
    players: watch::Sender<usize>,
    // OnceCell retries a cancelled initializer. Don't dispatch another downurl
    // on behalf of a waiter already attached to this flight.
    started: AtomicBool,
}
struct Player<'a>(&'a watch::Sender<usize>);
impl Drop for Player<'_> {
    fn drop(&mut self) {
        self.0.send_modify(|n| *n -= 1);
    }
}
#[derive(Default)]
struct State {
    entries: HashMap<Key, Cached>,
    flights: HashMap<Key, Weak<Flight>>,
}
pub struct Links {
    state: Mutex<State>,
    session: PathBuf,
}
impl Default for Links {
    fn default() -> Self {
        Self {
            state: Mutex::default(),
            session: crate::store::lock_path("cloud115-play-v1"),
        }
    }
}

static LINKS: OnceLock<Links> = OnceLock::new();

pub fn links() -> &'static Links {
    LINKS.get_or_init(Links::default)
}

impl Links {
    /// A link for `pick_code` the agent `ua` can fetch. Cached per agent, since
    /// the drive signs to the client that asked.
    pub async fn resolve(
        &self,
        grant: &str,
        pick_code: &str,
        ua: &str,
        ttl_secs: u64,
    ) -> Result<Resolved, ApiError> {
        self.resolve_with(grant, pick_code, ua, ttl_secs, false, async {
            client::sign(grant, ua, &pick_code.to_ascii_lowercase()).await
        })
        .await
    }
    pub async fn resolve_play(
        &self,
        grant: &str,
        pick_code: &str,
        ua: &str,
        ttl_secs: u64,
    ) -> Result<Resolved, ApiError> {
        self.resolve_with(grant, pick_code, ua, ttl_secs, true, async {
            client::sign(grant, ua, &pick_code.to_ascii_lowercase()).await
        })
        .await
    }
    async fn resolve_with(
        &self,
        grant: &str,
        pc: &str,
        ua: &str,
        ttl_secs: u64,
        play: bool,
        sign: impl Future<Output = Result<DownloadLink, ApiError>>,
    ) -> Result<Resolved, ApiError> {
        let key = (grant.to_owned(), pc.to_ascii_lowercase(), ua.to_owned());
        let flight = {
            let mut state = self.state.lock().await;
            if let Some(cached) = state.entries.get(&key) {
                if cached.until > Instant::now() {
                    return Ok(Resolved {
                        link: cached.link.clone(),
                        cached: true,
                    });
                }
                state.entries.remove(&key);
            }
            if let Some(f) = state.flights.get(&key).and_then(Weak::upgrade) {
                f
            } else {
                state.flights.retain(|_, f| f.strong_count() != 0);
                let f = Arc::new(Flight {
                    result: OnceCell::new(),
                    players: watch::channel(0).0,
                    started: AtomicBool::new(false),
                });
                state.flights.insert(key.clone(), Arc::downgrade(&f));
                f
            }
        };
        let _player = play.then(|| {
            flight.players.send_modify(|n| *n += 1);
            Player(&flight.players)
        });
        let mut signed = false;
        let link = flight
            .result
            .get_or_init(|| async {
                if flight.started.swap(true, Ordering::Relaxed) {
                    return Err(shared::interrupted());
                }
                // A remote join does not start another paced call or manufacture a
                // local player. Same-instance players still notify a queued prefetch.
                let sign = crate::pacer::PLAYERS.scope(flight.players.subscribe(), sign);
                let (reply, until_ms) =
                    shared::resolve(&self.session, &key, ttl_secs, sign).await?;
                let mut state = self.state.lock().await;
                let left = until_ms.saturating_sub(crate::store::now_ms());
                if left <= 0 {
                    return Err(ApiError::network("115 返回的播放链接已到期"));
                }
                let until = Instant::now() + Duration::from_millis(left as u64);
                if state.entries.len() >= MAX_CACHED {
                    let now = Instant::now();
                    state.entries.retain(|_, cached| cached.until > now);
                    if state.entries.len() >= MAX_CACHED {
                        state.entries.clear();
                    }
                }
                state.entries.insert(
                    key,
                    Cached {
                        link: reply.link.clone(),
                        until,
                    },
                );
                signed = !reply.cached;
                Ok(reply.link)
            })
            .await
            .clone()?;
        Ok(Resolved {
            link,
            cached: !signed,
        })
    }

    /// For the admin page: how many links are held, and how long the soonest
    /// one lasts.
    pub async fn snapshot(&self) -> (usize, Option<u64>) {
        let now = Instant::now();
        let state = self.state.lock().await;
        let entries = &state.entries;
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
fn valid_until_ms(url: &str, ttl_secs: u64, now: i64) -> i64 {
    let millis = |s: u64| i64::try_from(s.saturating_mul(1000)).unwrap_or(i64::MAX);
    let configured = now.saturating_add(millis(ttl_secs.max(30)));
    let Some(t) = query_int(url, "t") else {
        return configured;
    };
    let deadline = millis(t);
    // Inside the safety margin retain the old brief grace, but never pass t.
    configured
        .min(deadline)
        .min((deadline - millis(EXPIRY_MARGIN_SECS)).max(now.saturating_add(5000)))
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
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn link() -> DownloadLink {
        DownloadLink {
            url: "https://cdn.test/a.mkv?sig=1".into(),
            size: 1,
            name: "a.mkv".into(),
            needs_cookie: false,
        }
    }

    struct Session(PathBuf);
    impl Session {
        fn new() -> Self {
            let mut id = [0; 12];
            getrandom::fill(&mut id).unwrap();
            Self(std::env::temp_dir().join(format!("cloud115-play-test-{}", hex::encode(id))))
        }
        fn worker(&self) -> Links {
            Links {
                state: Mutex::default(),
                session: self.0.clone(),
            }
        }
    }
    impl Drop for Session {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    // Poll all the way to the first real wait, without a scheduling guess.
    async fn park<F: Future>(mut f: std::pin::Pin<&mut F>) {
        std::future::poll_fn(|cx| {
            assert!(f.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
    }

    async fn must_hit() -> Result<DownloadLink, ApiError> {
        panic!("waiter invoked sign")
    }

    #[test]
    fn expiry_uses_the_original_issue_time_drive_deadline_and_margin() {
        let now = 1_000_000;
        assert_eq!(
            valid_until_ms("https://cdn.test/a?sig=1", 600, now),
            now + 600_000
        );
        assert_eq!(
            valid_until_ms("https://cdn.test/a?t=1120&f=1", 3600, now),
            now + 60_000
        );
        assert_eq!(
            valid_until_ms("https://cdn.test/a?t=1025", 3600, now),
            now + 5_000
        );
        assert_eq!(
            valid_until_ms("https://cdn.test/a?t=1001", 3600, now),
            now + 1_000
        );
        assert!(valid_until_ms("https://cdn.test/a?t=999", 3600, now) < now);
    }

    #[tokio::test]
    async fn simultaneous_requests_share_one_signing_call() {
        let session = Session::new();
        let links = Arc::new(session.worker());
        let calls = Arc::new(AtomicUsize::new(0));
        let (started, wait_started) = tokio::sync::oneshot::channel();
        let (release, wait_release) = tokio::sync::oneshot::channel();
        let first_links = links.clone();
        let first_calls = calls.clone();
        let first = tokio::spawn(async move {
            first_links
                .resolve_with("grant", "PC", "player", 600, false, async move {
                    first_calls.fetch_add(1, Ordering::SeqCst);
                    let _ = started.send(());
                    let _ = wait_release.await;
                    Ok(link())
                })
                .await
        });
        wait_started.await.unwrap();
        let second_links = links.clone();
        let second_calls = calls.clone();
        let second = tokio::spawn(async move {
            second_links
                .resolve_with("grant", "pc", "player", 600, true, async move {
                    second_calls.fetch_add(1, Ordering::SeqCst);
                    Ok(link())
                })
                .await
        });
        tokio::task::yield_now().await;
        release.send(()).unwrap();
        let first = first.await.unwrap().unwrap();
        let second = second.await.unwrap().unwrap();
        assert!(!first.cached);
        assert!(second.cached);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn two_native_workers_racing_a_cold_directory_dispatch_once() {
        let session = Session::new();
        let start = std::sync::Barrier::new(2);
        let calls = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            let race = || {
                let worker = session.worker();
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_time()
                    .build()
                    .unwrap();
                start.wait();
                rt.block_on(worker.resolve_with("g", "pc", "ua", 600, true, async {
                    calls.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(70)).await;
                    Ok(link())
                }))
                .unwrap()
                .cached
            };
            let a = scope.spawn(race);
            let b = scope.spawn(race);
            assert_ne!(a.join().unwrap(), b.join().unwrap());
        });
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn two_workers_share_one_prefetch_then_both_hit_memory() {
        let session = Session::new();
        let (a, b) = (session.worker(), session.worker());
        let calls = AtomicUsize::new(0);
        let (release, wait) = tokio::sync::oneshot::channel();
        let first = a.resolve_with("grant", "PC", "Player/1", 600, false, async {
            calls.fetch_add(1, Ordering::SeqCst);
            wait.await.unwrap();
            // The other worker's player did not bypass the prefetch's pacing.
            crate::pacer::PLAYERS.with(|p| assert_eq!(*p.borrow(), 0));
            Ok(link())
        });
        tokio::pin!(first);
        park(first.as_mut()).await;
        let second = b.resolve_with("grant", "pc", "Player/1", 600, true, must_hit());
        tokio::pin!(second);
        park(second.as_mut()).await;
        release.send(()).unwrap();
        let first = first.await.unwrap();
        let second = second.await.unwrap();
        assert!(!first.cached && second.cached);
        assert_eq!(first.link.url, second.link.url);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        std::fs::remove_dir_all(&session.0).unwrap();
        for worker in [&a, &b] {
            assert!(
                worker
                    .resolve_with("grant", "PC", "Player/1", 600, true, must_hit())
                    .await
                    .unwrap()
                    .cached
            );
        }
        assert!(!session.0.exists(), "hot lookups touched the filesystem");
    }

    #[tokio::test]
    async fn different_keys_can_sign_while_the_first_is_still_pacing() {
        let session = Session::new();
        let (a, b) = (session.worker(), session.worker());
        let first = a.resolve_with("grant", "pc", "UA", 600, false, std::future::pending());
        tokio::pin!(first);
        park(first.as_mut()).await;
        for (grant, pc, ua) in [
            ("grant", "pc", "ua"),
            ("other", "pc", "UA"),
            ("grant", "pc2", "UA"),
            ("grantp", "c", "UA"),
        ] {
            let r = b
                .resolve_with(grant, pc, ua, 600, true, async { Ok(link()) })
                .await
                .unwrap();
            assert!(!r.cached);
        }
    }

    #[tokio::test]
    async fn a_joiner_gets_its_own_failure_even_if_a_later_request_succeeds() {
        let session = Session::new();
        let (a, b) = (session.worker(), session.worker());
        let (release, wait) = tokio::sync::oneshot::channel();
        let first = a.resolve_with("grant", "pc", "ua", 600, false, async {
            wait.await.unwrap();
            Err(ApiError {
                http: 429,
                errno: 10019,
                message: "offline".into(),
            })
        });
        tokio::pin!(first);
        park(first.as_mut()).await;
        let second = b.resolve_with("grant", "pc", "ua", 600, true, must_hit());
        tokio::pin!(second);
        park(second.as_mut()).await;
        release.send(()).unwrap();
        assert_eq!(first.await.err().unwrap().http, 429);
        let fresh = a
            .resolve_with("grant", "pc", "ua", 600, false, async { Ok(link()) })
            .await
            .unwrap();
        assert!(!fresh.cached);
        let old_error = second.await.err().unwrap();
        assert_eq!(
            (old_error.http, old_error.errno, &*old_error.message),
            (429, 10019, "offline")
        );
    }

    #[tokio::test]
    async fn cancelling_owner_does_not_resign_locally_or_in_the_other_worker() {
        let session = Session::new();
        let (a, b) = (session.worker(), session.worker());
        let mut owner =
            Box::pin(a.resolve_with("g", "pc", "ua", 600, true, std::future::pending()));
        park(owner.as_mut()).await;
        let local = a.resolve_with("g", "pc", "ua", 600, false, must_hit());
        let peer = b.resolve_with("g", "pc", "ua", 600, true, must_hit());
        tokio::pin!(local, peer);
        park(local.as_mut()).await;
        park(peer.as_mut()).await;
        drop(owner);
        assert!(local.await.is_err());
        assert!(peer.await.is_err());
        assert!(
            !b.resolve_with("g", "pc", "ua", 600, true, async { Ok(link()) })
                .await
                .unwrap()
                .cached
        );
    }

    #[tokio::test]
    async fn a_trapped_worker_does_not_permanently_own_the_gate_or_flight() {
        let session = Session::new();
        let (a, b) = (session.worker(), session.worker());
        let mut owner =
            Box::pin(a.resolve_with("g", "pc", "ua", 600, false, std::future::pending()));
        park(owner.as_mut()).await;
        let joiner = b.resolve_with("g", "pc", "ua", 600, true, must_hit());
        tokio::pin!(joiner);
        park(joiner.as_mut()).await;
        let shard = std::fs::read_dir(&session.0)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let abandoned = shard.join("gate");
        std::fs::create_dir(&abandoned).unwrap();
        std::fs::write(abandoned.join("0-dead"), b"").unwrap();
        let path = shard.join("rows");
        let mut rows: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        rows[0]["expires"] = 0.into(); // its signing lease has also elapsed
        std::fs::write(path, serde_json::to_vec(&rows).unwrap()).unwrap();
        assert!(joiner.await.is_err());
        assert!(!abandoned.exists());
        assert!(
            !b.resolve_with("g", "pc", "ua", 600, true, async { Ok(link()) })
                .await
                .unwrap()
                .cached
        );
        drop(owner); // delayed cleanup must not invalidate the replacement
        assert!(
            a.resolve_with("g", "pc", "ua", 600, true, must_hit())
                .await
                .unwrap()
                .cached
        );
    }

    #[tokio::test]
    async fn drive_deadline_is_not_extended_by_a_shared_or_memory_hit() {
        let session = Session::new();
        let (a, b) = (session.worker(), session.worker());
        let t = crate::store::now_ms() / 1000 + 2;
        let mut short = link();
        short.url = format!("https://cdn.test/a?t={t}");
        assert!(
            !a.resolve_with("g", "pc", "ua", 600, false, async { Ok(short) })
                .await
                .unwrap()
                .cached
        );
        assert!(
            b.resolve_with("g", "pc", "ua", 3600, true, must_hit())
                .await
                .unwrap()
                .cached
        );
        let left = (t * 1000 - crate::store::now_ms()).max(0) as u64;
        tokio::time::sleep(Duration::from_millis(left + 20)).await;
        assert!(
            !b.resolve_with("g", "pc", "ua", 3600, true, async { Ok(link()) })
                .await
                .unwrap()
                .cached
        );
        assert!(
            a.resolve_with("g", "pc", "ua", 600, true, must_hit())
                .await
                .unwrap()
                .cached
        );
    }
}
