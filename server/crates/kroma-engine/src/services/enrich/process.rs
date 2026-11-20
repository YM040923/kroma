use std::collections::HashMap;
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
use super::pins::identified;

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
    let options = eng.options_for(&job.library);
    if options.nfo {
        super::nfo::write(eng, job);
    }
    apply_art_preference(eng, job, &options);
    apply_nfo_metadata(eng, job);
}

pub(super) fn apply_nfo_metadata(eng: &Engine, job: &Job) {
    let Some(mut nfo) = super::nfo::read_existing(eng, job) else {
        return;
    };
    if let Some(pin) = job.pin {
        nfo.tmdb_id = Some(pin);
    }
    let (kind, mut meta) = if job.is_show {
        let kind = db::metadata_core::SHOW;
        let Some(detail) = db::get_show(&eng.pool, &job.id).ok().flatten() else {
            return;
        };
        (kind, detail.show.metadata.unwrap_or_else(|| nfo.metadata(true)))
    } else {
        let kind = db::metadata_core::ITEM;
        let Some(item) = db::get_item(&eng.pool, &job.id).ok().flatten() else {
            return;
        };
        (kind, item.metadata.unwrap_or_else(|| nfo.metadata(false)))
    };
    crate::services::scan::nfo::overlay_metadata(&mut meta, &nfo);
    let write = if job.is_show {
        db::set_show_metadata(&eng.pool, &job.id, &meta)
    } else {
        db::set_item_metadata(&eng.pool, &job.id, &meta)
    }
    .and_then(|()| db::store_localized(&eng.pool, kind, &job.id, &meta, &HashMap::new()));
    if let Err(error) = write {
        warn!(id = %job.id, error = %error, "failed to store NFO metadata");
        return;
    }
    let language = nfo
        .language
        .as_deref()
        .or_else(|| eng.scrape.get(&job.library).and_then(|o| o.language.as_deref()))
        .unwrap_or(&eng.language);
    let language = crate::i18n::normalize(language).unwrap_or("en");
    let mut data = db::translations::load_all(&eng.pool, kind, &[job.id.as_str()])
        .ok()
        .and_then(|all| {
            all.get(&job.id)
                .and_then(|by_lang| by_lang.get(language))
                .cloned()
        })
        .unwrap_or_default();
    let has_localized_nfo = nfo.title.is_some()
        || nfo.original_title.is_some()
        || nfo.plot.is_some()
        || !nfo.genres.is_empty();
    if let Some(title) = nfo.title.or(nfo.original_title) {
        data.title = Some(title);
    }
    if let Some(overview) = nfo.plot {
        data.overview = Some(overview);
    }
    if !nfo.genres.is_empty() {
        data.genres = nfo.genres;
    }
    data.rev = db::translations::REV;
    if has_localized_nfo {
        if let Err(error) = db::translations::put(
            &eng.pool,
            kind,
            &job.id,
            language,
            db::translations::NFO,
            &data,
        ) {
            warn!(id = %job.id, error = %error, "failed to store NFO translation");
        }
    }
    eng.bus.publish(if job.is_show {
        ServerEvent::ShowUpdated { id: job.id.clone() }
    } else {
        ServerEvent::ItemUpdated { id: job.id.clone() }
    });
}

/// Picks the preferred poster/backdrop from the provider's full image list and
/// stores it in the title's metadata. The explicit language/textless choice
/// wins; otherwise the best-voted, largest image wins. Manual overrides always
/// win and are never touched by this pass.
fn apply_art_preference(
    eng: &Engine,
    job: &Job,
    options: &crate::services::settings::ScrapeOptions,
) {
    apply_art_policy(eng, job, options, false);
}

/// Re-check artwork even when the title is already enriched. Used by the
/// scheduled show-artwork sweep; each manual override locks only its image type.
pub(super) fn refresh_art(
    eng: &Engine,
    job: &Job,
    options: &crate::services::settings::ScrapeOptions,
) -> bool {
    apply_art_policy(eng, job, options, true)
}

fn apply_art_policy(
    eng: &Engine,
    job: &Job,
    options: &crate::services::settings::ScrapeOptions,
    force: bool,
) -> bool {
    let mut poster = art_prefs(options.poster_languages.as_ref(), options.poster_language.as_deref());
    let mut backdrop =
        art_prefs(options.backdrop_languages.as_ref(), options.backdrop_language.as_deref());
    if force && poster.is_empty() {
        poster.push("auto".to_string());
    }
    if force && backdrop.is_empty() {
        backdrop.push("auto".to_string());
    }
    let backdrop_count = options.backdrop_count.map(usize::from);
    let poster_size = options
        .poster_size
        .as_deref()
        .or(options.artwork_size.as_deref());
    let backdrop_size = options
        .backdrop_size
        .as_deref()
        .or(options.artwork_size.as_deref());
    if !force
        && poster.is_empty()
        && backdrop.is_empty()
        && backdrop_count.is_none()
        && poster_size.is_none()
        && backdrop_size.is_none()
        && !options.write_images
    {
        return false;
    }
    let (mut meta, tmdb_id, is_show) = if job.is_show {
        let Ok(Some(detail)) = db::get_show(&eng.pool, &job.id) else {
            return false;
        };
        let Some(meta) = detail.show.metadata.clone() else {
            return false;
        };
        let tmdb = meta.tmdb_id;
        (meta, tmdb, true)
    } else {
        let Ok(Some(item)) = db::get_item(&eng.pool, &job.id) else {
            return false;
        };
        let Some(meta) = item.metadata.clone() else {
            return false;
        };
        let tmdb = meta.tmdb_id;
        (meta, tmdb, false)
    };
    if tmdb_id == 0 {
        return false;
    }
    let kind = if is_show {
        db::art_overrides::SHOW_KIND
    } else {
        db::art_overrides::ITEM_KIND
    };
    let overrides = match db::art_overrides::get(&eng.pool, kind, &job.id) {
        Ok(overrides) => overrides,
        Err(error) => {
            warn!(id = %job.id, error = %error, "failed to read artwork locks");
            return false;
        }
    };
    let poster_locked = overrides.contains_key(db::art_overrides::POSTER);
    let backdrop_locked = overrides.contains_key(db::art_overrides::BACKDROP);
    if force && poster_locked && backdrop_locked {
        return false;
    }
    let poster_min = options.poster_min_width.unwrap_or(500);
    let backdrop_min = options.backdrop_min_width.unwrap_or(1920);
    let needs_images = force
        || backdrop_count.is_some()
        || poster_size.is_some()
        || backdrop_size.is_some()
        || (!poster_locked && !poster.is_empty())
        || (!backdrop_locked && !backdrop.is_empty());
    let wanted_languages: Vec<String> = poster
        .iter()
        .chain(&backdrop)
        .filter(|tag| !tag.eq_ignore_ascii_case("auto") && !tag.eq_ignore_ascii_case("textless"))
        .cloned()
        .collect();
    let (posters, gallery) = if needs_images {
        match metadata::images(
            &eng.api_key,
            &eng.language,
            &wanted_languages,
            job.target,
            tmdb_id,
        ) {
            Ok(images) => images,
            Err(()) => return false,
        }
    } else {
        (Vec::new(), Vec::new())
    };
    if !poster_locked && !poster.is_empty() {
        if let Some(url) = pick_image(&posters, &poster, poster_size, poster_min) {
            meta.poster_url = Some(url);
        }
    }
    let backdrop_policy = force
        || !backdrop.is_empty()
        || backdrop_count.is_some()
        || backdrop_size.is_some();
    let selected_backdrops = if backdrop_policy {
        selected_images(
            &gallery,
            &backdrop,
            backdrop_size,
            backdrop_count,
            backdrop_min,
        )
    } else {
        Vec::new()
    };
    if backdrop_policy && !backdrop_locked {
        if let Some(primary) = selected_backdrops.first() {
            meta.backdrop_url = Some(primary.clone());
        }
    }
    if backdrop_policy {
        let primary = overrides
            .get(db::art_overrides::BACKDROP)
            .or(meta.backdrop_url.as_ref());
        let primary_key = primary.map(|url| image_key(url));
        meta.backdrops = selected_backdrops
            .into_iter()
            .filter(|url| primary_key.as_ref().is_none_or(|key| image_key(url) != *key))
            .collect();
    } else if let Some(limit) = backdrop_count {
        meta.backdrops.truncate(limit);
    }
    if let Some(size) = poster_size {
        if !poster_locked {
            if let Some(url) = meta.poster_url.as_mut() {
                *url = sized_url(url, size);
            }
        }
    }
    if let Some(size) = backdrop_size {
        if !backdrop_locked {
            if let Some(url) = meta.backdrop_url.as_mut() {
                *url = sized_url(url, size);
            }
        }
        for url in &mut meta.backdrops {
            *url = sized_url(url, size);
        }
    }
    if options.write_images {
        write_art_files(eng, job, &meta);
    }
    let meta = image::localize(&eng.data_dir, meta);
    let kind = if is_show {
        db::metadata_core::SHOW
    } else {
        db::metadata_core::ITEM
    };
    let write = if is_show {
        db::set_show_metadata(&eng.pool, &job.id, &meta)
    } else {
        db::set_item_metadata(&eng.pool, &job.id, &meta)
    }
    .and_then(|()| db::store_localized(&eng.pool, kind, &job.id, &meta, &HashMap::new()));
    if let Err(error) = write {
        warn!(id = %job.id, error = %error, "failed to store artwork policy");
        false
    } else {
        true
    }
}

/// The ordered artwork preferences: the list when present, else the legacy
/// single tag, else empty (any language, best voted).
fn art_prefs(list: Option<&Vec<String>>, legacy: Option<&str>) -> Vec<String> {
    if let Some(list) = list {
        return list.clone();
    }
    legacy
        .map(str::trim)
        .filter(|tag| !tag.is_empty() && !tag.eq_ignore_ascii_case("auto"))
        .map(|tag| vec![tag.to_string()])
        .unwrap_or_default()
}

/// `None` = any language, `Some(None)` = textless, `Some(Some(tag))` = exact.
fn wanted_of(preference: &str) -> Option<Option<&str>> {
    if preference.eq_ignore_ascii_case("textless") {
        Some(None)
    } else if preference.is_empty() || preference.eq_ignore_ascii_case("auto") {
        None
    } else {
        Some(Some(preference))
    }
}

fn matches(image: &metadata::ArtImage, preference: &str) -> bool {
    wanted_of(preference).is_none_or(|wanted| image.language.as_deref() == wanted)
}

fn pick_image(
    images: &[metadata::ArtImage],
    preferences: &[String],
    size: Option<&str>,
    min_width: u16,
) -> Option<String> {
    let prefer: &[String] = if preferences.is_empty() {
        &[]
    } else {
        preferences
    };
    for relaxed in [false, true] {
        if prefer.is_empty() {
            if let Some(best) = best_of(images, None, min_width, relaxed) {
                return Some(fit(best, size));
            }
            continue;
        }
        for preference in prefer {
            if let Some(best) = best_of(images, Some(preference), min_width, relaxed) {
                return Some(fit(best, size));
            }
        }
    }
    None
}

fn best_of<'a>(
    images: &'a [metadata::ArtImage],
    preference: Option<&str>,
    min_width: u16,
    relaxed: bool,
) -> Option<&'a metadata::ArtImage> {
    let mut seen = std::collections::HashSet::new();
    images
        .iter()
        .filter(|image| preference.is_none_or(|preference| matches(image, preference)))
        .filter(|image| relaxed || image.width >= u32::from(min_width))
        .filter(|image| seen.insert(image_key(&image.url)))
        .max_by(|a, b| {
            a.votes
                .total_cmp(&b.votes)
                .then((a.width * a.height).cmp(&(b.width * b.height)))
                .then(a.url.cmp(&b.url))
        })
}

fn fit(image: &metadata::ArtImage, size: Option<&str>) -> String {
    size.map_or_else(|| image.url.clone(), |size| sized_url(&image.url, size))
}

fn selected_images(
    images: &[metadata::ArtImage],
    preferences: &[String],
    size: Option<&str>,
    limit: Option<usize>,
    min_width: u16,
) -> Vec<String> {
    let limit = limit.unwrap_or(8).min(8);
    let prefer: &[String] = if preferences.is_empty() {
        &[]
    } else {
        preferences
    };
    let mut ordered: Vec<&metadata::ArtImage> = Vec::new();
    for relaxed in [false, true] {
        ordered.clear();
        let mut seen = std::collections::HashSet::new();
        for preference in prefer {
            let mut batch: Vec<_> = images
                .iter()
                .filter(|image| matches(image, preference))
                .filter(|image| relaxed || image.width >= u32::from(min_width))
                .filter(|image| seen.insert(image_key(&image.url)))
                .collect();
            batch.sort_by(|a, b| {
                b.votes
                    .total_cmp(&a.votes)
                    .then((b.width * b.height).cmp(&(a.width * a.height)))
                    .then(a.url.cmp(&b.url))
            });
            ordered.extend(batch);
        }
        if prefer.is_empty() || ordered.len() < limit {
            let mut rest: Vec<_> = images
                .iter()
                .filter(|image| relaxed || image.width >= u32::from(min_width))
                .filter(|image| !ordered.iter().any(|held| held.url == image.url))
                .collect();
            rest.sort_by(|a, b| {
                b.votes
                    .total_cmp(&a.votes)
                    .then((b.width * b.height).cmp(&(a.width * a.height)))
                    .then(a.url.cmp(&b.url))
            });
            ordered.extend(rest);
        }
        if !ordered.is_empty() || relaxed {
            break;
        }
    }
    ordered
        .into_iter()
        .take(limit)
        .map(|image| fit(image, size))
        .collect()
}

/// Write the chosen poster/backdrop beside the media for readers that read
/// files rather than this catalogue. Existing files are never replaced, and a
/// show's files land at the show root rather than inside its season folder.
fn write_art_files(eng: &Engine, job: &Job, meta: &crate::model::Metadata) {
    let Some(dir) = art_directory(eng, job) else {
        return;
    };
    for (url, name) in [
        (meta.poster_url.as_deref(), "poster.jpg"),
        (meta.backdrop_url.as_deref(), "fanart.jpg"),
    ] {
        let Some(url) = url else {
            continue;
        };
        let target = dir.join(name);
        if target.exists() {
            continue;
        }
        if !image::fetch_to(url, &target) {
            warn!(id = %job.id, file = %target.display(), "artwork file was not written");
        }
    }
}

fn art_directory(eng: &Engine, job: &Job) -> Option<std::path::PathBuf> {
    let files = if job.is_show {
        db::local_paths::show_file_paths(&eng.pool, &job.id).ok()?
    } else {
        db::local_paths::item_file_paths(&eng.pool, &job.id).ok()?
    };
    let first = files.first()?;
    let path = if !first.abs_path.trim().is_empty() && !first.abs_path.starts_with("http") {
        std::path::PathBuf::from(first.abs_path.trim())
    } else {
        let folders = eng.folders.get(&job.library)?;
        let rel = first.rel_path.trim();
        folders
            .iter()
            .map(|folder| std::path::Path::new(folder).join(rel))
            .find(|path| path.exists())
            .or_else(|| folders.first().map(|folder| std::path::Path::new(folder).join(rel)))?
    };
    let parent = path.parent()?;
    let in_season = parent
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.to_lowercase().starts_with("season"));
    if job.is_show && in_season {
        parent.parent().map(std::path::Path::to_path_buf)
    } else {
        Some(parent.to_path_buf())
    }
}

fn image_key(url: &str) -> String {
    let path = url.split('?').next().unwrap_or(url);
    path.split_once("/t/p/")
        .map(|(_, rest)| rest.split_once('/').map(|(_, file)| file).unwrap_or(rest))
        .unwrap_or(path)
        .to_string()
}

/// Rewrite a TMDB image URL to the chosen size; local/cached paths are left
/// alone.
fn sized_url(url: &str, size: &str) -> String {
    if !url.contains("/t/p/") {
        return url.to_string();
    }
    let mut parts = url.splitn(2, "/t/p/");
    let head = parts.next().unwrap_or_default();
    let tail = parts.next().unwrap_or_default();
    let rest = tail.split_once('/').map(|(_, rest)| rest).unwrap_or(tail);
    format!("{head}/t/p/{size}/{rest}")
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
    // A library may scrape with its own language list; everything below reads
    // the engine, so overriding the clone keeps every lookup on the library's
    // primary language.
    let mut local = eng.clone();
    if let Some(first) = local.priority_for(&job.library).into_iter().next() {
        local.language = first;
    }
    let eng = &local;
    let mut job = job;
    if let Some(nfo) = super::nfo::read_existing(eng, &job) {
        if job.pin.is_none() {
            if let Some(tmdb_id) = nfo.tmdb_id {
                job.pin = Some(tmdb_id);
                job.resolved_tmdb = None;
                job.refetch = None;
            }
        }
    }
    let langs = crate::i18n::content_locales();
    let job = identified(eng, job);
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

#[cfg(test)]
mod artwork_tests {
    use super::*;

    fn image(url: &str, language: Option<&str>, width: u32, height: u32, votes: f64) -> metadata::ArtImage {
        metadata::ArtImage {
            url: url.into(),
            width,
            height,
            language: language.map(str::to_string),
            votes,
        }
    }

    #[test]
    fn an_explicit_language_wins_over_a_more_voted_other_language() {
        let images = vec![
            image("https://image.test/en.jpg", Some("en"), 1000, 1500, 9.0),
            image("https://image.test/zh.jpg", Some("zh"), 500, 750, 1.0),
        ];
        assert_eq!(
            pick_image(&images, "zh", None).as_deref(),
            Some("https://image.test/zh.jpg")
        );
    }

    #[test]
    fn textless_only_considers_images_without_a_language() {
        let images = vec![
            image("https://image.test/en.jpg", Some("en"), 1000, 1500, 9.0),
            image("https://image.test/clean.jpg", None, 500, 750, 1.0),
        ];
        assert_eq!(
            pick_image(&images, "textless", None).as_deref(),
            Some("https://image.test/clean.jpg")
        );
    }

    #[test]
    fn auto_prefers_votes_then_resolution_and_deduplicates() {
        let images = vec![
            image("https://image.test/a.jpg", Some("en"), 500, 750, 4.0),
            image("https://image.test/b.jpg", Some("en"), 1000, 1500, 4.0),
            image("https://image.test/b.jpg", Some("en"), 1000, 1500, 4.0),
        ];
        assert_eq!(
            pick_image(&images, "auto", None).as_deref(),
            Some("https://image.test/b.jpg")
        );
    }

    #[test]
    fn a_provider_url_is_rewritten_to_the_requested_size() {
        assert_eq!(
            sized_url("https://image.tmdb.org/t/p/w1280/abc.jpg", "w780"),
            "https://image.tmdb.org/t/p/w780/abc.jpg"
        );
        assert_eq!(
            sized_url("/data/images/local.webp", "w500"),
            "/data/images/local.webp"
        );
    }
}
