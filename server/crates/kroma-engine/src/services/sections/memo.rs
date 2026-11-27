//! Each viewer's generated home and hero, remembered between requests. An entry
//! is served only while everything it was built from reads the same, and for
//! ten minutes at most, which bounds what no stamp sees (trending decaying with
//! the clock, an embedder coming up). The titles a hero is picked from depend on
//! the catalogue alone, so they are read once for every viewer.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use anyhow::Result;
use time::OffsetDateTime;

use crate::db::{self, HeroCandidate, HomeActivity, Pool};
use crate::model::{Section, SectionCard, SectionItem, User};
use crate::services::jobs::now_ms;
use crate::services::settings;
use crate::state::SharedState;

use super::context::PartOfDay;
use super::featured;

const FRESH_FOR_MS: i64 = 10 * 60_000;
// Every write that changes a candidate is announced on the bus; the hour only
// bounds one that is not.
const CANDIDATES_FRESH_FOR_MS: i64 = 60 * 60_000;
const DAY_MS: i64 = 86_400_000;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Inputs {
    catalog: u64,
    activity: HomeActivity,
    day: i64,
    part_of_day: PartOfDay,
    show_recent: bool,
    played_percent: i64,
}

impl Inputs {
    fn read(state: &SharedState, pool: &Pool, user_id: &str, now_ms: i64) -> Result<Self> {
        let hour = OffsetDateTime::from_unix_timestamp(now_ms.div_euclid(1000))?.hour();
        Ok(Self {
            catalog: state.events.catalog_version(),
            activity: db::home_activity(pool, user_id)?,
            day: now_ms.div_euclid(DAY_MS),
            part_of_day: PartOfDay::at(hour),
            show_recent: state.settings.get_bool("showRecentHome", true),
            played_percent: settings::played_percent(&state.settings),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Key {
    user_id: String,
    locale: String,
    grant: Option<(Option<Vec<String>>, Option<u8>, bool)>,
}

impl Key {
    fn new(user_id: &str, locale: &str) -> Self {
        Self {
            user_id: user_id.to_string(),
            locale: locale.to_string(),
            grant: None,
        }
    }

    fn discovery(user: &User, locale: &str) -> Self {
        let mut key = Self::new(&user.id, locale);
        if !user.sees_every_library() {
            key.grant = Some((user.libraries.clone().into(), user.max_age, user.allow_unrated));
        }
        key
    }
}

struct Held<V> {
    inputs: Inputs,
    built_at_ms: i64,
    value: V,
}

struct Shelf<V> {
    held: Mutex<HashMap<Key, Held<V>>>,
}

impl<V> Default for Shelf<V> {
    fn default() -> Self {
        Self {
            held: Mutex::default(),
        }
    }
}

impl<V: Clone> Shelf<V> {
    fn recall(&self, key: &Key, inputs: &Inputs, now_ms: i64) -> Option<V> {
        let held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        held.get(key)
            .filter(|h| &h.inputs == inputs && now_ms - h.built_at_ms < FRESH_FOR_MS)
            .map(|h| h.value.clone())
    }

    fn keep(&self, key: Key, inputs: Inputs, now_ms: i64, value: V) {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        held.retain(|_, h| now_ms - h.built_at_ms < FRESH_FOR_MS);
        held.insert(
            key,
            Held {
                inputs,
                built_at_ms: now_ms,
                value,
            },
        );
    }
}

struct Slot<V> {
    held: Mutex<Option<(u64, i64, V)>>,
}

impl<V> Default for Slot<V> {
    fn default() -> Self {
        Self {
            held: Mutex::default(),
        }
    }
}

impl<V: Clone> Slot<V> {
    fn recall(&self, catalog: u64, now_ms: i64) -> Option<V> {
        let held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        held.as_ref()
            .filter(|(at, built_at_ms, _)| {
                *at == catalog && now_ms - built_at_ms < CANDIDATES_FRESH_FOR_MS
            })
            .map(|(_, _, value)| value.clone())
    }

    fn keep(&self, catalog: u64, now_ms: i64, value: V) {
        *self.held.lock().unwrap_or_else(PoisonError::into_inner) = Some((catalog, now_ms, value));
    }
}

/// The remembered homes and heroes, one per viewer and locale, and the hero
/// candidates every viewer shares. Entries are held before any library grant
/// is applied, so the caller filters every answer for the viewer asking.
#[derive(Default)]
pub struct HomeMemo {
    sections: Shelf<Arc<Vec<Section>>>,
    discovery_ids: Shelf<Arc<Vec<String>>>,
    hero: Shelf<SectionItem>,
    candidates: Slot<Arc<Vec<HeroCandidate>>>,
    pub(super) catalog: crate::services::reco::CatalogMemo,
    pub(crate) world: super::world::WorldTrending,
}

// An empty catalogue is read again every time, so the first scan is seen at once.
pub(super) fn hero_candidates(
    state: &SharedState,
    read: impl FnOnce() -> Vec<HeroCandidate>,
) -> Arc<Vec<HeroCandidate>> {
    let catalog = state.events.catalog_version();
    let now = now_ms();
    if let Some(candidates) = state.home.candidates.recall(catalog, now) {
        return candidates;
    }
    let candidates = Arc::new(read());
    if !candidates.is_empty() {
        state
            .home
            .candidates
            .keep(catalog, now, Arc::clone(&candidates));
    }
    candidates
}

/// [`super::build_home`] for `user_id`, reused while nothing it reads has moved.
pub fn home(state: &SharedState, pool: &Pool, locale: &str, user_id: &str) -> Arc<Vec<Section>> {
    remember(
        &state.home.sections,
        state,
        pool,
        Key::new(user_id, locale),
        || Some(Arc::new(super::build_home(state, pool, locale, user_id))),
    )
    .unwrap_or_default()
}

pub fn discovery(
    state: &SharedState,
    pool: &Pool,
    locale: &str,
    user: &User,
    visible: impl Fn(&str, Option<&str>) -> bool,
) -> Result<Vec<SectionCard>> {
    let ids = remember(&state.home.discovery_ids, state, pool, Key::discovery(user, locale), || {
        let world = state.home.world.cached_ranking();
        let ids = super::with_board(state, pool, locale, &user.id, &world, |board| board.discovery_ids(&visible));
        Some(Arc::new(ids))
    }).unwrap_or_default();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
    let mut items = db::entities_by_ids(pool, &refs)?;
    // A grant or certification can change after the candidates were ranked.
    items.retain(|entry| {
        let (library, metadata) = match entry {
            SectionItem::Movie { item } => (&item.library, item.metadata.as_ref()),
            SectionItem::Show { show } => (&show.library, show.metadata.as_ref()),
        };
        visible(library, metadata.and_then(|m| m.certification.as_deref()))
    });
    db::localize::overlay_section_items(pool, &mut items, crate::i18n::content_locale(locale))?;
    Ok(items.into_iter().map(SectionCard::from).collect())
}

/// [`featured::pick`] for `user_id`, reused like [`home`]. An empty answer is
/// never remembered, so a catalogue that comes up is seen at once.
pub fn hero(state: &SharedState, pool: &Pool, locale: &str, user_id: &str) -> Option<SectionItem> {
    remember(
        &state.home.hero,
        state,
        pool,
        Key::new(user_id, locale),
        || featured::pick(state, pool, locale, user_id),
    )
}

// A stamp that cannot be read builds afresh rather than trusting an old entry.
fn remember<V: Clone>(
    shelf: &Shelf<V>,
    state: &SharedState,
    pool: &Pool,
    key: Key,
    build: impl FnOnce() -> Option<V>,
) -> Option<V> {
    let now = now_ms();
    let inputs = match Inputs::read(state, pool, &key.user_id, now) {
        Ok(inputs) => inputs,
        Err(e) => {
            tracing::warn!(target: "sections", "reading the home's stamp failed: {e:#}");
            return build();
        }
    };
    if let Some(value) = shelf.recall(&key, &inputs, now) {
        return Some(value);
    }
    let value = build()?;
    shelf.keep(key, inputs, now, value.clone());
    Some(value)
}

#[cfg(test)]
mod store_tests;
#[cfg(test)]
mod tests;
