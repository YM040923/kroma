//! The two independent WASI memories reserve request slots against one small
//! on-disk ledger. Playback, minute checks and full runs share the daily ceiling
//! and provider cooldown; full-run limits live in the caller's task, not here.
use crate::store;
use serde::{Deserialize, Serialize};
use std::{fs, io::ErrorKind, time::Duration};

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Ledger {
    day: i64,
    calls: u64,
    next: i64,
    until: i64,
    streak: u32,
    sequence: u64,
    playback: Vec<(u64, i64)>,
    background_until: i64,
    play_run: u8,
}

const PLAYBACK_LEASE_MS: i64 = 1_000;
const PLAYBACK_QUEUE_POLL_MS: u64 = 50;
const PLAY_BURST: u8 = 3;
#[derive(Debug, PartialEq)]
pub(crate) enum Slot {
    Ready,
    Wait(Duration),
}
fn time() -> i64 {
    store::now_ms()
}
fn load(now: i64) -> Result<Ledger, String> {
    let file = store::path().with_file_name("open-call-ledger.json");
    let day = now.div_euclid(86_400_000);
    let mut l: Ledger = match fs::read(file) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|_| "调用记录不可读，本轮未向 115 发起请求".to_owned())?,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            let s = store::load();
            Ledger {
                day: s.budget_day,
                calls: s.budget_calls,
                ..Default::default()
            }
        }
        Err(_) => return Err("调用记录不可读，本轮未向 115 发起请求".into()),
    };
    if l.day != day {
        l.calls = 0;
        l.day = day;
    }
    Ok(l)
}
fn save(l: &Ledger) -> Result<(), String> {
    let file = store::path().with_file_name("open-call-ledger.json");
    let tmp = file.with_extension("staged");
    let write = || -> std::io::Result<()> {
        fs::create_dir_all(file.parent().unwrap())?;
        fs::write(&tmp, serde_json::to_vec(l)?)?;
        fs::rename(&tmp, file)
    };
    write().map_err(|_| "无法保存调用记录，本轮未向 115 发起请求".to_string())
}
fn lock() -> Result<store::FileLock, String> {
    store::FileLock::acquire("cloud115-open-calls.lock", Duration::from_secs(2))
        .map_err(|_| "调用计数正忙，稍后再试".to_owned())
}
impl Ledger {
    fn reserve(&mut self, now: i64, interval: u64, daily: u32) -> Result<Slot, String> {
        if daily != 0 && self.calls >= daily.into() {
            return Err(format!("今日 {daily} 次调用上限已用完，明天再继续"));
        }
        let delay = self.next.max(self.until).saturating_sub(now);
        if delay > 0 {
            return Ok(Slot::Wait(Duration::from_millis(delay.min(600_000) as u64)));
        }
        self.calls = self.calls.saturating_add(1);
        self.next = now.saturating_add(interval as i64);
        Ok(Slot::Ready)
    }

    fn arbitrate(
        &mut self,
        now: i64,
        interval: u64,
        daily: u32,
        playback: bool,
        ticket: &mut Option<u64>,
    ) -> Result<(Slot, bool), String> {
        let before = self.playback.len();
        self.playback
            .retain(|&(id, end)| end > now && (playback || Some(id) != *ticket));
        let mut dirty = self.playback.len() != before;
        if !self.playback.iter().any(|&(id, _)| Some(id) == *ticket) {
            *ticket = None;
        }
        if daily != 0 && self.calls >= daily.into() {
            if let Some(id) = ticket.take() {
                self.playback.retain(|&(queued, _)| queued != id);
            }
            return Err(format!("今日 {daily} 次调用上限已用完，明天再继续"));
        }
        let gate = self.next.max(self.until);
        let back_waits = self.background_until > now;
        let owes_background = back_waits && self.play_run >= PLAY_BURST;
        let head = self.playback.first().map(|&(id, _)| id);
        let turn = if playback {
            !owes_background && (head.is_none() || *ticket == head)
        } else {
            head.is_none() || owes_background
        };
        if now >= gate && turn {
            let slot = self.reserve(now, interval, daily)?;
            if let Some(id) = ticket.take() {
                self.playback.retain(|&(queued, _)| queued != id);
            }
            if playback && back_waits {
                self.play_run = self.play_run.saturating_add(1);
                self.background_until = self.next.saturating_add(PLAYBACK_LEASE_MS);
            } else {
                self.play_run = 0;
                self.background_until = 0;
            }
            return Ok((slot, true));
        }
        let expires = gate.max(now).saturating_add(PLAYBACK_LEASE_MS);
        if playback {
            if let Some(row) = self.playback.iter_mut().find(|r| Some(r.0) == *ticket) {
                if expires.saturating_sub(row.1) >= PLAYBACK_LEASE_MS / 2 {
                    row.1 = expires;
                    dirty = true;
                }
            } else {
                self.sequence = self.sequence.wrapping_add(1).max(1);
                self.playback.push((self.sequence, expires));
                *ticket = Some(self.sequence);
                dirty = true;
            }
        } else if expires.saturating_sub(self.background_until) >= PLAYBACK_LEASE_MS / 2 {
            if !back_waits {
                self.play_run = 0;
            }
            self.background_until = expires;
            dirty = true;
        }
        let delay = if now < gate {
            (gate - now).min(600_000) as u64
        } else {
            PLAYBACK_QUEUE_POLL_MS
        };
        Ok((Slot::Wait(Duration::from_millis(delay)), dirty))
    }
}
pub(crate) fn reserve(
    interval: u64,
    daily: u32,
    playback: bool,
    ticket: &mut Option<u64>,
) -> Result<Slot, String> {
    let _g = lock()?;
    let now = time();
    let mut l = load(now)?;
    let queued = l.playback.clone();
    match l.arbitrate(now, interval, daily, playback, ticket) {
        Ok((slot, changed)) => {
            if changed {
                save(&l)?;
            }
            Ok(slot)
        }
        Err(error) => {
            if queued != l.playback {
                save(&l)?;
            }
            Err(error)
        }
    }
}
pub(crate) fn record_refusal() -> Result<(), String> {
    let _g = lock()?;
    let now = time();
    let mut l = load(now)?;
    l.streak = if l.until.saturating_add(600_000) < now {
        1
    } else {
        l.streak.saturating_add(1).min(8)
    };
    l.until = l
        .until
        .max(now.saturating_add(30_000 * i64::from(l.streak)));
    save(&l)
}
pub(crate) fn paused_for() -> Option<Duration> {
    let now = time();
    let ms = load(now).ok()?.until.saturating_sub(now);
    (ms > 0).then(|| Duration::from_millis(ms as u64))
}
pub fn calls_today() -> u64 {
    load(time()).map(|s| s.calls).unwrap_or(0)
}
pub fn flush() {} // Accounting already committed before each network call.
pub fn background_blocked(daily_budget: u32) -> bool {
    daily_budget > 0 && calls_today().saturating_mul(10) >= u64::from(daily_budget) * 7
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reserve_checks_cooldown_floor_and_daily_cap_before_spending() {
        let mut l = Ledger::default();
        assert_eq!(l.reserve(1000, 500, 2).unwrap(), Slot::Ready);
        assert_eq!(
            l.reserve(1200, 500, 2).unwrap(),
            Slot::Wait(Duration::from_millis(300))
        );
        assert_eq!(l.calls, 1);
        l.until = 3000;
        assert_eq!(
            l.reserve(1500, 500, 2).unwrap(),
            Slot::Wait(Duration::from_millis(1500))
        );
        assert_eq!(l.reserve(3000, 500, 2).unwrap(), Slot::Ready);
        assert!(l.reserve(4000, 500, 2).is_err());
    }

    #[test]
    fn playback_takes_the_next_legal_slot_without_bypassing_the_interval() {
        let mut l = Ledger {
            next: 1500,
            ..Default::default()
        };
        let mut first = None;
        let mut second = None;
        assert_eq!(
            l.arbitrate(1000, 500, 0, true, &mut first).unwrap().0,
            Slot::Wait(Duration::from_millis(500))
        );
        assert_eq!(
            l.arbitrate(1000, 500, 0, true, &mut second).unwrap().0,
            Slot::Wait(Duration::from_millis(500))
        );
        assert_eq!(l.playback.len(), 2);
        assert_eq!(
            l.arbitrate(1500, 500, 0, false, &mut None).unwrap().0,
            Slot::Wait(Duration::from_millis(PLAYBACK_QUEUE_POLL_MS))
        );
        assert_eq!(
            l.arbitrate(1500, 500, 0, true, &mut first).unwrap().0,
            Slot::Ready
        );
        assert_eq!(l.calls, 1);
        assert_eq!(l.next, 2000);
        assert_eq!(
            l.arbitrate(2000, 500, 0, true, &mut second).unwrap().0,
            Slot::Ready
        );
        assert_eq!(l.calls, 2);
        assert!(l.playback.is_empty());
    }

    #[test]
    fn legacy_ledger_records_every_idle_and_background_call() {
        let mut l: Ledger =
            serde_json::from_str(r#"{"day":3,"calls":8,"next":900,"until":0,"streak":0}"#).unwrap();
        let (slot, must_save) = l.arbitrate(1000, 1500, 10, false, &mut None).unwrap();
        assert_eq!(
            (slot, must_save, l.calls, l.next),
            (Slot::Ready, true, 9, 2500)
        );
        let mut restored: Ledger =
            serde_json::from_slice(&serde_json::to_vec(&l).unwrap()).unwrap();
        assert_eq!(
            restored
                .arbitrate(1001, 1500, 10, false, &mut None)
                .unwrap()
                .0,
            Slot::Wait(Duration::from_millis(1499))
        );
        assert_eq!(
            restored
                .arbitrate(2500, 1500, 10, false, &mut None)
                .unwrap()
                .0,
            Slot::Ready
        );
        assert!(restored.arbitrate(4000, 1500, 10, true, &mut None).is_err());
    }

    #[test]
    fn waiting_scan_gets_a_turn_during_sustained_playback() {
        let mut l = Ledger {
            next: 1500,
            ..Default::default()
        };
        for i in 0..3 {
            let now = 1500 + i * 500;
            let mut ticket = None;
            assert!(matches!(
                l.arbitrate(now - 1, 500, 0, true, &mut ticket).unwrap().0,
                Slot::Wait(_)
            ));
            assert!(matches!(
                l.arbitrate(now, 500, 0, false, &mut None).unwrap().0,
                Slot::Wait(_)
            ));
            assert_eq!(
                l.arbitrate(now, 500, 0, true, &mut ticket).unwrap().0,
                Slot::Ready
            );
        }
        assert!(matches!(
            l.arbitrate(3000, 500, 0, true, &mut None).unwrap().0,
            Slot::Wait(_)
        ));
        assert_eq!(
            l.arbitrate(3000, 500, 0, false, &mut None).unwrap().0,
            Slot::Ready
        );
    }

    #[test]
    fn an_expired_playback_lease_does_not_hold_a_background_slot() {
        let mut l = Ledger {
            next: 1000,
            playback: vec![(1, 1500)],
            ..Default::default()
        };
        assert_eq!(
            l.arbitrate(1500, 500, 0, false, &mut None).unwrap().0,
            Slot::Ready
        );
        assert!(l.playback.is_empty());
        assert_eq!(l.calls, 1);
    }

    #[test]
    fn playback_still_obeys_provider_cooldown_and_daily_ceiling() {
        let mut l = Ledger {
            until: 3000,
            ..Default::default()
        };
        let mut ticket = None;
        assert_eq!(
            l.arbitrate(1000, 500, 1, true, &mut ticket).unwrap().0,
            Slot::Wait(Duration::from_millis(2000))
        );
        assert_eq!(
            l.arbitrate(3000, 500, 1, true, &mut ticket).unwrap().0,
            Slot::Ready
        );
        assert!(l
            .arbitrate(4000, 500, 1, true, &mut ticket)
            .unwrap_err()
            .contains("今日 1 次调用上限已用完"));
        assert!(l.playback.is_empty());
    }
}
