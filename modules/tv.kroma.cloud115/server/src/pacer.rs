//! One pacer for every call the module makes.
//!
//! The drive counts requests, and its risk control treats a burst as the
//! signature of a script. Three things keep a run looking like a person:
//!
//!   - **a floor between calls** (`interval_ms`), applied before every request
//!     rather than after one failed;
//!   - **a cooldown** when the drive answers that it is being leaned on — HTTP
//!     405/429/430/599, or errno 10008/10019/590075 — held globally so one
//!     refused call pauses *all* traffic, not just the next one on that path;
//!   - **a budget per run**, so a library too large to mirror in one pass stops
//!     cleanly and continues next time instead of grinding for hours.
//!
//! The waits are cooperative rather than timers: the host advances this
//! component's runtime in small slices (one millisecond per tick, a tick every
//! hundred), so a `tokio::time::sleep` of a second and a half would take
//! minutes. Polling the wall clock and yielding lets the caller resume on the
//! next tick, at the right time.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use crate::settings::Options;

/// How long a refused call parks every call, and the ceiling for repeats.
const COOLDOWN: Duration = Duration::from_secs(30);
const COOLDOWN_CAP: Duration = Duration::from_secs(10 * 60);

/// The one pacer every call goes through: playback signing and a mirror share
/// it, so a busy sync cannot slip a burst past the drive by running beside the
/// player.
static PACER: OnceLock<Pacer> = OnceLock::new();

pub fn pacer() -> &'static Pacer {
    PACER.get_or_init(|| Pacer::new(&Options::default()))
}

pub struct Pacer {
    state: Mutex<State>,
}

struct State {
    interval: Duration,
    /// The floor between one write and the next, on top of `interval`: a drive
    /// weighs a rename or a move more heavily than a listing.
    write_interval: Duration,
    budget: u32,
    calls: u32,
    /// The earliest instant the next call may start.
    next_at: Option<Instant>,
    /// While set, nothing may call at all. Every refusal pushes it out.
    until: Option<Instant>,
    refusals: u32,
}

/// Wait for a wall-clock instant without a timer: each `yield_now` hands the
/// thread back so the host can come round again, and the loop checks the clock
/// when it does.
async fn wait_until(deadline: Instant) {
    while Instant::now() < deadline {
        tokio::task::yield_now().await;
    }
}

impl Pacer {
    pub fn new(options: &Options) -> Self {
        Self {
            state: Mutex::new(State {
                interval: Duration::from_millis(options.interval_ms),
                write_interval: Duration::from_millis(options.write_pause_ms),
                budget: options.max_calls_per_run,
                calls: 0,
                next_at: None,
                until: None,
                refusals: 0,
            }),
        }
    }

    /// Take a new interval and budget without discarding a running cooldown or
    /// the calls already spent.
    pub async fn configure(&self, options: &Options) {
        let mut state = self.state.lock().await;
        state.interval = Duration::from_millis(options.interval_ms);
        state.write_interval = Duration::from_millis(options.write_pause_ms);
        state.budget = options.max_calls_per_run;
    }

    /// Start a new run: the budget is per run, the cooldown is not (a refusal
    /// from the previous run still stands).
    pub async fn begin_run(&self, options: &Options) {
        let mut state = self.state.lock().await;
        state.interval = Duration::from_millis(options.interval_ms);
        state.write_interval = Duration::from_millis(options.write_pause_ms);
        state.budget = options.max_calls_per_run;
        state.calls = 0;
    }

    /// Wait for a call a player asked for. It takes the same floor and budget as
    /// any other, so playback cannot become a side channel around the pacing.
    pub async fn acquire(&self) -> Result<(), String> {
        loop {
            let wake_at = {
                let mut state = self.state.lock().await;
                if state.calls >= state.budget {
                    return Err(format!(
                        "本轮已达到 {} 次调用上限，剩余内容留给下一轮",
                        state.budget
                    ));
                }
                let now = Instant::now();
                match state.until {
                    Some(until) if until > now => Some(until),
                    Some(_) => {
                        state.until = None;
                        continue;
                    }
                    None => {
                        let next = state.next_at.unwrap_or(now);
                        if next > now {
                            Some(next)
                        } else {
                            state.calls += 1;
                            state.next_at = Some(now + state.interval);
                            return Ok(());
                        }
                    }
                }
            };
            if let Some(deadline) = wake_at {
                wait_until(deadline).await;
            }
        }
    }

    /// Wait for a call that changes the account. A write also holds its own, much
    /// longer floor: a batch of renames that looks like a list of downloads is
    /// what gets an account flagged.
    pub async fn acquire_write(&self) -> Result<(), String> {
        let write_interval = self.state.lock().await.write_interval;
        self.acquire().await?;
        wait_until(Instant::now() + write_interval).await;
        Ok(())
    }

    /// Record what the drive answered. A refusal parks every call for a while,
    /// backing off on repeats; a success clears the streak.
    pub async fn note(&self, http_status: u16, errno: i64) {
        let refused = matches!(http_status, 405 | 429 | 430 | 599)
            || matches!(errno, 10008 | 10019 | 590075);
        let mut state = self.state.lock().await;
        if !refused {
            state.refusals = 0;
            return;
        }
        state.refusals = state.refusals.saturating_add(1);
        let wait = (COOLDOWN * state.refusals.min(8)).min(COOLDOWN_CAP);
        let until = Instant::now() + wait;
        if state.until.map_or(true, |current| current < until) {
            state.until = Some(until);
        }
        // A refusal also spends the slot, so the retry is not immediate once the
        // cooldown lifts.
        state.next_at = Some(until);
    }

    /// Calls spent this run.
    pub async fn spent(&self) -> u32 {
        self.state.lock().await.calls
    }

    /// The per-run ceiling this run is spending against.
    pub async fn budget(&self) -> u32 {
        self.state.lock().await.budget
    }

    /// Seconds left on a cooldown, if one is running.
    pub async fn paused_for(&self) -> Option<Duration> {
        let state = self.state.lock().await;
        state
            .until
            .and_then(|until| until.checked_duration_since(Instant::now()))
    }}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(budget: u32) -> Options {
        Options {
            interval_ms: 0,
            max_calls_per_run: budget,
            ..Options::default()
        }
    }

    #[tokio::test]
    async fn a_budget_ends_a_run_instead_of_grinding() {
        let pacer = Pacer::new(&options(2));
        assert!(pacer.acquire().await.is_ok());
        assert!(pacer.acquire().await.is_ok());
        let refused = pacer.acquire().await;
        assert!(refused.is_err(), "the third call is over budget");
        assert!(refused.unwrap_err().contains("调用上限"));
    }

    #[tokio::test]
    async fn a_new_run_starts_with_a_fresh_budget() {
        let pacer = Pacer::new(&options(1));
        assert!(pacer.acquire().await.is_ok());
        assert!(pacer.acquire().await.is_err());
        pacer.begin_run(&options(1)).await;
        assert!(pacer.acquire().await.is_ok());
    }

    #[tokio::test]
    async fn a_drive_refusal_parks_every_later_call() {
        let pacer = Pacer::new(&options(10));
        pacer.note(200, 0).await;
        assert!(pacer.paused_for().await.is_none());
        pacer.note(405, 0).await;
        let paused = pacer.paused_for().await.expect("a 405 must pause the pacer");
        assert!(paused.as_secs() >= 20, "the pause is long enough to matter");
    }

    #[tokio::test]
    async fn the_errnos_that_mean_slow_down_are_recognised() {
        for errno in [10008i64, 10019, 590075] {
            let pacer = Pacer::new(&options(10));
            pacer.note(200, errno).await;
            assert!(pacer.paused_for().await.is_some(), "errno {errno} should pause");
        }
    }

    #[tokio::test]
    async fn a_success_clears_the_refusal_streak() {
        let pacer = Pacer::new(&options(10));
        pacer.note(429, 0).await;
        pacer.note(200, 0).await;
        pacer.note(429, 0).await;
        // The streak restarted, so this is one cooldown rather than two stacked.
        let paused = pacer.paused_for().await.expect("still parked");
        assert!(paused.as_secs() < 45, "paused {}s, the streak did not reset", paused.as_secs());
    }

    #[tokio::test]
    async fn a_write_waits_longer_than_a_read() {
        let options = Options {
            interval_ms: 0,
            write_pause_ms: 1_000,
            ..Options::default()
        };
        let pacer = Pacer::new(&options);
        let started = Instant::now();
        pacer.acquire_write().await.expect("within budget");
        assert!(started.elapsed().as_millis() >= 1_000, "a write holds its own floor");
    }
}
