use crate::model::RequestKind;

use super::super::{api, tmdb_json, IMG};
use super::wire::{PageResp, RawHit};
use super::{year_of, DiscoverHit, DiscoverPage, DiscoverScope, FilterQuery, FilterSort};

/// Search TMDB. `All` uses multi-search (drops `person` hits); the scoped
/// variants hit the movie/tv endpoints directly.
// The `()` Err is the unit error threaded up from `tmdb_json` via `?`; callers only
// care ok/err, so a richer error type would add nothing here.
#[allow(clippy::result_unit_err)]
pub fn search(
    api_key: &str,
    language: &str,
    scope: DiscoverScope,
    query: &str,
    page: u32,
) -> Result<DiscoverPage, ()> {
    let params = vec![
        ("language", language.to_string()),
        ("query", query.to_string()),
        ("include_adult", "false".to_string()),
        ("page", page.max(1).to_string()),
    ];
    let path = match scope {
        DiscoverScope::Movies => "search/movie",
        DiscoverScope::Shows => "search/tv",
        DiscoverScope::All => "search/multi",
    };
    let resp: PageResp = tmdb_json(
        super::super::PURPOSE,
        &format!("{}/{path}", api()),
        api_key,
        &params,
    )?;
    Ok(map_page(resp, scope))
}

/// This week's trending titles. `All` (movies + shows, page 1) powers the
/// discover empty-state rails; the scoped variants back the full "trending
/// movies" / "trending shows" pages with real pagination.
// The `()` Err is the unit error threaded up from `tmdb_json` via `?`; callers only
// care ok/err, so a richer error type would add nothing here.
#[allow(clippy::result_unit_err)]
pub fn trending(
    api_key: &str,
    language: &str,
    scope: DiscoverScope,
    page: u32,
) -> Result<DiscoverPage, ()> {
    let path = match scope {
        DiscoverScope::Movies => "trending/movie/week",
        DiscoverScope::Shows => "trending/tv/week",
        DiscoverScope::All => "trending/all/week",
    };
    let params = vec![
        ("language", language.to_string()),
        ("page", page.max(1).to_string()),
    ];
    let resp: PageResp = tmdb_json(
        super::super::PURPOSE,
        &format!("{}/{path}", api()),
        api_key,
        &params,
    )?;
    Ok(map_page(resp, scope))
}

// Skips rows with no usable title.
pub(super) fn hit_from(kind: RequestKind, h: RawHit) -> Option<DiscoverHit> {
    let title = h.title.or(h.name)?;
    Some(DiscoverHit {
        kind,
        tmdb_id: h.id,
        title,
        original_title: h.original_title.or(h.original_name).unwrap_or_default(),
        year: year_of(h.release_date.as_deref().or(h.first_air_date.as_deref())),
        poster_url: h.poster_path.map(|p| format!("{IMG}/w342{p}")),
        backdrop_url: h.backdrop_path.map(|p| format!("{IMG}/w780{p}")),
        overview: h.overview.filter(|s| !s.is_empty()),
        rating: h.vote_average.filter(|v| *v > 0.0),
    })
}

/// TMDB's scoped `discover` listing: the "browse by filters" counterpart of
/// [`search`] (a text query) and [`trending`] (this week). Movies and shows
/// answer on separate endpoints with different date/sort field names, so the
/// kind picks both the path and the vocabulary.
// The `()` Err is the unit error threaded up from `tmdb_json` via `?`; callers only
// care ok/err, so a richer error type would add nothing here.
#[allow(clippy::result_unit_err)]
pub fn filter(q: &FilterQuery<'_>) -> Result<DiscoverPage, ()> {
    let (path, scope) = match q.kind {
        RequestKind::Movie => ("discover/movie", DiscoverScope::Movies),
        RequestKind::Show => ("discover/tv", DiscoverScope::Shows),
    };
    let params = filter_params(q);
    let resp: PageResp = tmdb_json(
        super::super::PURPOSE,
        &format!("{}/{path}", api()),
        q.api_key,
        &params,
    )?;
    Ok(map_page(resp, scope))
}

/// The query string one filtered listing sends. Kept pure so the date/sort
/// vocabulary is testable without a network round trip.
pub(crate) fn filter_params(q: &FilterQuery<'_>) -> Vec<(&'static str, String)> {
    let sort_by = match q.sort {
        FilterSort::Popularity => "popularity.desc",
        FilterSort::Release => match q.kind {
            RequestKind::Show => "first_air_date.desc",
            RequestKind::Movie => "primary_release_date.desc",
        },
        FilterSort::Rating => "vote_average.desc",
    };
    let mut params: Vec<(&'static str, String)> = vec![
        ("language", q.language.to_string()),
        ("sort_by", sort_by.to_string()),
        ("include_adult", "false".to_string()),
        ("page", q.page.max(1).to_string()),
    ];
    if q.sort == FilterSort::Rating {
        // A single 10/10 vote would otherwise top the list.
        params.push(("vote_count.gte", "100".to_string()));
    }
    if let Some(language) = q.original_language.filter(|l| !l.is_empty()) {
        params.push(("with_original_language", language.to_string()));
    }
    if let Some(genre) = q.genre {
        params.push(("with_genres", genre.to_string()));
    }
    if let Some(year) = q.year {
        let key = match q.kind {
            RequestKind::Show => "first_air_date_year",
            RequestKind::Movie => "primary_release_year",
        };
        params.push((key, year.to_string()));
    }
    if let Some(decade) = q.decade {
        let (gte, lte) = match q.kind {
            RequestKind::Show => ("first_air_date.gte", "first_air_date.lte"),
            RequestKind::Movie => ("primary_release_date.gte", "primary_release_date.lte"),
        };
        params.push((gte, format!("{decade:04}-01-01")));
        params.push((lte, format!("{:04}-12-31", decade.saturating_add(9))));
    }
    params
}

fn map_page(resp: PageResp, scope: DiscoverScope) -> DiscoverPage {
    let hits = resp
        .results
        .into_iter()
        .filter_map(|h| {
            // Multi-search mixes movies, shows and people; scoped searches carry
            // no media_type (the endpoint implies it).
            let kind = match h.media_type.as_deref() {
                Some("movie") => RequestKind::Movie,
                Some("tv") => RequestKind::Show,
                Some(_) => return None, // person / collection
                None => match scope {
                    DiscoverScope::Movies => RequestKind::Movie,
                    DiscoverScope::Shows => RequestKind::Show,
                    DiscoverScope::All => return None,
                },
            };
            hit_from(kind, h)
        })
        .collect();
    DiscoverPage {
        hits,
        page: resp.page.max(1),
        total_pages: resp.total_pages.max(1),
        total_results: resp.total_results,
    }
}
