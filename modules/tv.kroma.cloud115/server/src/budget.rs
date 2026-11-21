//! The day's call allowance, counted across every run and every player.
//!
//! The drive's risk control watches the day, not a run: four scheduled mirrors
//! that each stay under a per-run budget still add up. This keeps one counter
//! for the UTC day, persisted with the rest of the module's state, and lets the
//! background half stop at a reserve so an evening film still gets a link.

use std::sync::{Mutex, OnceLock};

use crate::events::unix_now;

/// Persist every this many calls rather than on each one: a file write per API
/// call would be its own kind of noise.
const PERSIST_EVERY: u64 = 20;

struct State {
    day: i64,
    calls: u64,
    pending: u64,
}

fn today() -> i64 {
    unix_now() / 86_400
}

fn slot() -> &'static Mutex<State> {
    static SLOT: OnceLock<Mutex<State>> = OnceLock::new();
    SLOT.get_or_init(|| {
        let stored = crate::store::load();
        let day = today();
        Mutex::new(State {
            day,
            calls: if stored.budget_day == day {
                stored.budget_calls
            } else {
                0
            },
            pending: 0,
        })
    })
}

fn roll(state: &mut State, day: i64) {
    if state.day != day {
        state.day = day;
        state.calls = 0;
        state.pending = 0;
    }
}

fn persist(day: i64, calls: u64) {
    let _ = crate::store::patch(|stored| {
        stored.budget_day = day;
        stored.budget_calls = calls;
    });
}

/// Calls spent today, rolling over at the UTC day boundary.
pub fn calls_today() -> u64 {
    let Ok(mut state) = slot().lock() else {
        return 0;
    };
    roll(&mut state, today());
    state.calls
}

/// Count one call. Called by the pacer for every request, playback included.
pub fn spend() -> u64 {
    let Ok(mut state) = slot().lock() else {
        return 0;
    };
    roll(&mut state, today());
    state.calls += 1;
    state.pending += 1;
    if state.pending >= PERSIST_EVERY {
        let (day, calls) = (state.day, state.calls);
        state.pending = 0;
        drop(state);
        persist(day, calls);
        return calls;
    }
    state.calls
}

/// Flush a partial count, for a run that just finished.
pub fn flush() {
    let Ok(mut state) = slot().lock() else {
        return;
    };
    roll(&mut state, today());
    if state.pending == 0 {
        return;
    }
    let (day, calls) = (state.day, state.calls);
    state.pending = 0;
    drop(state);
    persist(day, calls);
}

/// Whether background work (mirror, watcher, probes) should stand down so the
/// last part of the allowance is left for playback. 0 means no daily budget.
pub fn background_blocked(daily_budget: u32) -> bool {
    daily_budget > 0 && calls_today().saturating_mul(10) >= u64::from(daily_budget) * 7
}
