use std::sync::atomic::Ordering;

use tracing::warn;

use crate::db;
use crate::infra::events::ServerEvent;
use crate::infra::image;
use crate::infra::metadata;
use crate::infra::theme;
use crate::model::Metadata;
use crate::services::activity;
use crate::services::activity::Shared as Activity;

use super::engine::{bump, Counters, Engine};
use super::episodes::enrich_episodes;
use super::jobs::Job;
use super::lang::{primary_lang, stale_langs};

pub(super) fn subject_kind(is_show: bool) -> &'static str {
    if is_show {
        db::metadata_core::SHOW
    } else {
        db::metadata_core::ITEM
    }
}

pub(super) fn fill_langs(eng: &Engine, job: &Job, tmdb_id: u64, missing: &[String]) {
    let want: Vec<&str> = missing
        .iter()
        .map(String::as_str)
        .filter(|l| crate::i18n::content_locales().contains(l))
        .collect();
    if want.is_empty() {
        return;
    }
    let Some(resolved) = metadata::lookup_all_by_id(
        &eng.cache,
        &eng.api_key,
        &eng.language,
        &want,
        job.target,
        tmdb_id,
    ) else {
        warn!(id = %job.id, "metadata unreachable; keeping the language it already has");
        return;
    };
    let by_lang: std::collections::HashMap<String, Metadata> = resolved
        .by_lang
        .into_iter()
        .map(|(lang, m)| (lang, image::localize_title_art(&eng.data_dir, m)))
        .collect();
    // Only the languages TMDB actually answered for. One whose request failed
    // says nothing about the title, and recording "there is nothing here" for it
    // would overwrite what is stored and stamp it current, so nothing would ever
    // ask again.
    let asked: Vec<String> = want
        .iter()
        .filter(|l| !resolved.unreachable.contains(**l))
        .map(|l| (*l).to_string())
        .collect();
    if asked.is_empty() && by_lang.is_empty() {
        return;
    }
    if let Err(e) = db::fill_languages(
        &eng.pool,
        subject_kind(job.is_show),
        &job.id,
        &by_lang,
        &asked,
    ) {
        warn!(id = %job.id, error = %e, "failed to fill missing languages");
    }
}

pub(super) fn post_process(eng: &Engine, job: &Job) {
    let Some(options) = eng.scrape.get(&job.library) else {
        return;
    };
    if options.nfo {
        super::nfo::write(eng, job);
    }
    apply_art_preference(eng, job, options);
}

/// Picks the preferred poster/backdrop (a language tag, or "textless") from the
/// provider's full image list and stores it in the title's metadata. This is an
/// automatic preference, distinct from the manual per-title override table.
fn apply_art_preference(
    eng: &Engine,
    job: &Job,
    options: &crate::services::settings::ScrapeOptions,
) {
    let poster = options
        .poster_language
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("auto"));
    let backdrop = options
        .backdrop_language
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("auto"));
    let backdrop_count = options.backdrop_count.map(usize::from);
    if poster.is_none() && backdrop.is_none() && backdrop_count.is_none() {
        return;
    }
    let (mut meta, tmdb_id, is_show) = if job.is_show {
        let Ok(Some(detail)) = db::get_show(&eng.pool, &job.id) else {
            return;
        };
        let Some(meta) = detail.show.metadata.clone() else {
            return;
        };
        let tmdb = meta.tmdb_id;
        (meta, tmdb, true)
    } else {
        let Ok(Some(item)) = db::get_item(&eng.pool, &job.id) else {
            return;
        };
        let Some(meta) = item.metadata.clone() else {
            return;
        };
        let tmdb = meta.tmdb_id;
        (meta, tmdb, false)
    };
    if tmdb_id == 0 {
        return;
    }
    if poster.is_some() || backdrop.is_some() {
        let Ok((posters, backdrops)) =
            metadata::images(&eng.api_key, &eng.language, job.target, tmdb_id)
        else {
            return;
        };
        if let Some(pref) = poster {
            if let Some(url) = pick_image(&posters, pref) {
                meta.poster_url = Some(url);
            }
        }
        if let Some(pref) = backdrop {
            if let Some(url) = pick_image(&backdrops, pref) {
                meta.backdrop_url = Some(url);
            }
        }
    }
    if let Some(limit) = backdrop_count {
        meta.backdrops.truncate(limit);
    }
    let meta = image::localize(&eng.data_dir, meta);
    let _ = if is_show {
        db::set_show_metadata(&eng.pool, &job.id, &meta)
    } else {
        db::set_item_metadata(&eng.pool, &job.id, &meta)
    };
}

fn pick_image(images: &[metadata::ArtImage], preference: &str) -> Option<String> {
    let wanted = if preference.eq_ignore_ascii_case("textless") {
        None
    } else {
        Some(preference)
    };
    images
        .iter()
        .filter(|image| image.language.as_deref() == wanted)
        .max_by(|a, b| a.votes.total_cmp(&b.votes))
        .map(|image| image.url.clone())
}

pub(super) fn process_job(
    eng: &Engine,
    counters: &Counters,
    total: usize,
    activity: Option<&Activity>,
    job: Job,
) {
    // Named before the first network call, which is where a pass spends its
    // time: a client watching the bar wants the row, not the count it is on.
    if let Some(activity) = activity {
        activity::enrich_current(activity, job.title.clone());
    }
    // A library may scrape in its own language; everything below reads the
    // engine, so overriding the clone keeps every lookup on one language.
    let mut local = eng.clone();
    if let Some(options) = local.scrape.get(&job.library) {
        if let Some(language) = options
            .language
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            local.language = language.to_string();
        }
    }
    let eng = &local;
    let langs = crate::i18n::content_locales();
    if let Some(tmdb_id) = job.resolved_tmdb {
        if job.is_show {
            enrich_episodes(
                &eng.pool,
                &eng.api_key,
                &eng.language,
                langs,
                &eng.data_dir,
                &eng.bus,
                &job.id,
                tmdb_id,
            );
        }
        let kind = subject_kind(job.is_show);
        let stale = stale_langs(&eng.pool, kind, &job.id, langs);
        if !stale.is_empty() {
            fill_langs(eng, &job, tmdb_id, &stale);
        }
        counters.resolved.fetch_add(1, Ordering::Relaxed);
        bump(eng, counters, total, activity);
        return;
    }
    let resolved = match job.pin.or(job.refetch) {
        Some(tmdb_id) => metadata::lookup_all_by_id(
            &eng.cache,
            &eng.api_key,
            &eng.language,
            langs,
            job.target,
            tmdb_id,
        ),
        None => metadata::lookup_all(
            &eng.cache,
            &eng.api_key,
            &eng.language,
            langs,
            job.target,
            &job.title,
            job.year,
        ),
    };
    let Some(resolved) = resolved else {
        counters.missed.fetch_add(1, Ordering::Relaxed);
        bump(eng, counters, total, activity);
        return;
    };
    let primary_key = primary_lang(&resolved.by_lang, &eng.language);
    let Some(meta) = resolved.by_lang.get(&primary_key).cloned() else {
        counters.missed.fetch_add(1, Ordering::Relaxed);
        bump(eng, counters, total, activity);
        return;
    };
    let meta = image::localize(&eng.data_dir, meta);
    let by_lang: std::collections::HashMap<String, Metadata> = resolved
        .by_lang
        .into_iter()
        .map(|(lang, m)| {
            let m = if lang == primary_key {
                meta.clone()
            } else {
                image::localize_title_art(&eng.data_dir, m)
            };
            (lang, m)
        })
        .collect();
    // Disabled leaves `theme_url` None, so a re-scan also clears any theme
    // cached while the feature was on.
    let meta = if eng.theme_songs {
        theme::localize(&eng.data_dir, meta)
    } else {
        meta
    };
    let doc = kroma_domain::build_doc(&job.title, job.year, &meta);
    let vector = crate::services::embeddings::embed(&eng.embedder, &doc);
    let write = if job.is_show {
        db::set_show_metadata(&eng.pool, &job.id, &meta)
    } else {
        db::set_item_metadata(&eng.pool, &job.id, &meta)
    };
    match write {
        Ok(()) => on_write_ok(eng, counters, &job, &meta, &vector, &by_lang, langs),
        Err(e) => {
            // Report the new total, not "+1": the snapshot holds a number the
            // panel renders, and `activity::enrich_failed` keeps it monotonic
            // across workers that write in a different order than they counted.
            let failed = counters.failed.fetch_add(1, Ordering::Relaxed) + 1;
            if let Some(activity) = activity {
                activity::enrich_failed(activity, failed);
            }
            warn!(id = %job.id, error = %e, "failed to store metadata");
        }
    }
    bump(eng, counters, total, activity);
}

// Every secondary write here is best-effort: a failure must not drop the
// blob/art already stored.
#[allow(clippy::too_many_arguments)]
pub(super) fn on_write_ok(
    eng: &Engine,
    counters: &Counters,
    job: &Job,
    meta: &Metadata,
    vector: &[f32],
    by_lang: &std::collections::HashMap<String, Metadata>,
    langs: &[&str],
) {
    rename_if_pinned(eng, job, meta);
    let kind = if job.is_show {
        db::metadata_core::SHOW
    } else {
        db::metadata_core::ITEM
    };
    if let Err(e) = db::store_localized(&eng.pool, kind, &job.id, meta, by_lang) {
        warn!(id = %job.id, error = %e, "failed to store localized metadata cache");
    }
    if let Err(e) = db::set_item_vector(&eng.pool, &job.id, vector) {
        warn!(id = %job.id, error = %e, "failed to store embedding");
    }
    if job.is_show && meta.tmdb_id != 0 {
        enrich_episodes(
            &eng.pool,
            &eng.api_key,
            &eng.language,
            langs,
            &eng.data_dir,
            &eng.bus,
            &job.id,
            meta.tmdb_id,
        );
    }
    counters.resolved.fetch_add(1, Ordering::Relaxed);
    eng.bus.publish(if job.is_show {
        ServerEvent::ShowUpdated { id: job.id.clone() }
    } else {
        ServerEvent::ItemUpdated { id: job.id.clone() }
    });
}

// Only a pinned (trusted) match renames the row; a plain auto-search match
// keeps its parsed title, which is the safer label for a low-confidence guess.
pub(super) fn rename_if_pinned(eng: &Engine, job: &Job, meta: &Metadata) {
    if job.pin.is_none() {
        return;
    }
    let Some(title) = meta
        .title
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty() && *t != job.title)
    else {
        return;
    };
    let renamed = if job.is_show {
        db::set_show_title(&eng.pool, &job.id, title)
    } else {
        db::set_item_title(&eng.pool, &job.id, title)
    };
    if let Err(e) = renamed {
        warn!(id = %job.id, error = %e, "failed to rename catalog title after a pinned match");
    }
}
