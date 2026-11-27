//! A shared daily ceiling and network pace; a full run has its own immutable
//! allowance. Configuring playback or the minute job cannot spend or overwrite it.
use crate::{
    budget::{self, Slot},
    settings::Options,
};
use std::cell::Cell;
use std::future::Future;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tokio::sync::{watch, Mutex};

// Only installed around a route's shared signing future. A real player may
// join (or leave) a prefetch while it is still waiting for a legal slot.
tokio::task_local! {pub(crate) static PLAYERS: watch::Receiver<usize>;}

struct RunCalls {
    limit: u32,
    spent: Cell<u32>,
}
tokio::task_local! {static RUN: RunCalls;}
static PACER: OnceLock<Pacer> = OnceLock::new();
pub fn pacer() -> &'static Pacer {
    PACER.get_or_init(|| Pacer::new(&Options::default()))
}
pub struct Pacer {
    policy: Mutex<Policy>,
}
#[derive(Clone, Copy)]
struct Policy {
    interval: u64,
    write_pause: u64,
    daily: u32,
}
impl From<&Options> for Policy {
    fn from(o: &Options) -> Self {
        Self {
            interval: o.interval_ms,
            write_pause: o.write_pause_ms,
            daily: o.daily_call_budget,
        }
    }
}
async fn wait(d: Duration) {
    let deadline = Instant::now() + d;
    while Instant::now() < deadline {
        tokio::task::yield_now().await;
    }
}
impl Pacer {
    pub fn new(o: &Options) -> Self {
        Self {
            policy: Mutex::new(o.into()),
        }
    }
    pub async fn configure(&self, o: &Options) {
        *self.policy.lock().await = o.into();
    }
    pub async fn with_run<F: Future>(&self, o: &Options, f: F) -> F::Output {
        self.configure(o).await;
        RUN.scope(
            RunCalls {
                limit: o.max_calls_per_run,
                spent: Cell::new(0),
            },
            f,
        )
        .await
    }
    fn check_run() -> Result<(), String> {
        RUN.try_with(|r| {
            if r.spent.get() >= r.limit {
                Err(format!(
                    "本轮已达到 {} 次调用上限，未完成部分下次继续",
                    r.limit
                ))
            } else {
                Ok(())
            }
        })
        .unwrap_or(Ok(()))
    }
    async fn reserve(&self, write: bool) -> Result<(), String> {
        let mut players = PLAYERS.try_with(Clone::clone).ok();
        let mut ticket = None;
        loop {
            let play = players.as_mut().is_some_and(|p| *p.borrow_and_update() > 0);
            Self::check_run()?;
            let o = *self.policy.lock().await;
            match budget::reserve(
                if write {
                    o.interval.max(o.write_pause)
                } else {
                    o.interval
                },
                o.daily,
                play,
                &mut ticket,
            )? {
                Slot::Ready => {
                    let _ = RUN.try_with(|r| r.spent.set(r.spent.get().saturating_add(1)));
                    return Ok(());
                }
                Slot::Wait(d) => match &mut players {
                    Some(p) => tokio::select! { _ = wait(d) => {}, _ = p.changed() => {} },
                    None => wait(d).await,
                },
            }
        }
    }
    pub async fn acquire(&self) -> Result<(), String> {
        self.reserve(false).await
    }
    pub async fn acquire_write(&self) -> Result<(), String> {
        self.reserve(true).await
    }
    pub fn note(&self, http: u16, code: i64) {
        if matches!(http, 405 | 429 | 430 | 599)
            || matches!(code, 10008 | 10019 | 590075 | 40140117)
        {
            let _ = budget::record_refusal();
        }
    }
    pub async fn spent(&self) -> u32 {
        RUN.try_with(|r| r.spent.get()).unwrap_or(0)
    }
    pub async fn budget(&self) -> u32 {
        RUN.try_with(|r| r.limit).unwrap_or(0)
    }
    pub async fn paused_for(&self) -> Option<Duration> {
        budget::paused_for()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn manual_run_limit_is_not_replaced_by_background_configuration() {
        let background = Options::default();
        let manual = Options {
            max_calls_per_run: 5000,
            ..background.clone()
        };
        let p = Pacer::new(&background);
        p.with_run(&manual, async {
            RUN.with(|r| r.spent.set(800));
            p.configure(&background).await;
            assert_eq!(p.budget().await, 5000);
            assert!(Pacer::check_run().is_ok());
            tokio::spawn(async {
                assert!(Pacer::check_run().is_ok());
                assert!(RUN.try_with(|_| ()).is_err());
            })
            .await
            .unwrap();
            RUN.with(|r| r.spent.set(5000));
            assert!(Pacer::check_run().unwrap_err().contains("5000"));
        })
        .await;
        assert!(Pacer::check_run().is_ok());
        assert_eq!(p.spent().await, 0);
    }
}
