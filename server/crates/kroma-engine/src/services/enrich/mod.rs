//! Background TMDB enrichment: resolve poster / backdrop / overview / IDs for
//! every movie and show after a scan and write them into the DB, on a small
//! pool of threads so a large library never blocks startup or `/api/scan`.

mod engine;
mod episodes;
mod jobs;
mod lang;
mod nfo;
mod pins;
mod process;
mod run;

pub use run::{enrich_one, maybe_spawn, run_tracked, EnrichSummary};

/// Re-read provider artwork for shows whose library asks for a periodic sweep.
/// Only the show's primary/gallery art is touched; a manual override locks it,
/// and `metadata_core.updated_at` keeps a title from being fetched again before
/// its configured interval.
pub(crate) fn refresh_artwork(
    state: &crate::state::SharedState,
    progress: impl Fn(usize, usize),
    cancelled: impl Fn() -> bool,
) -> anyhow::Result<(usize, usize)> {
    let Some(api_key) = state.config.tmdb_api_key.clone() else {
        return Ok((0, 0));
    };
    let eng = engine::engine_for(state, api_key);
    let shows = crate::db::list_shows(&state.db, None)?;
    let total = shows.len();
    let mut refreshed = 0usize;
    let mut checked = 0usize;
    for (at, show) in shows.into_iter().enumerate() {
        if cancelled() {
            break;
        }
        progress(at + 1, total);
        let options = eng.options_for(&show.library);
        let days = options.artwork_refresh_days.unwrap_or(0);
        if days == 0 {
            continue;
        }
        let Some(tmdb_id) = show
            .metadata
            .as_ref()
            .map(|meta| meta.tmdb_id)
            .filter(|id| *id > 0)
        else {
            continue;
        };
        let now = crate::services::jobs::now_ms();
        let last = crate::db::metadata_core::updated_at(
            &state.db,
            crate::db::metadata_core::SHOW,
            &show.id,
        )?
        .unwrap_or(0);
        if last > 0 && now.saturating_sub(last) < i64::from(days) * 86_400_000 {
            continue;
        }
        checked += 1;
        let mut local = eng.clone();
        if let Some(language) = options
            .language
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            local.language = language.to_string();
        }
        let job = jobs::Job {
            id: show.id.clone(),
            library: show.library.clone(),
            target: crate::infra::metadata::Target::Tv,
            title: show.title.clone(),
            year: show.year,
            is_show: true,
            resolved_tmdb: Some(tmdb_id),
            pin: None,
            refetch: None,
            unasked: None,
        };
        if process::refresh_art(&local, &job, &options) {
            refreshed += 1;
        }
    }
    Ok((checked, refreshed))
}

#[cfg(test)]
mod tests;
