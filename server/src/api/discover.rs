//! `/api/discover/*` TMDB discovery for the request flow: search titles the
//! library may not have, trending for the empty state, a title detail (with the
//! season list for the picker) and one season's episodes. Every result is
//! flagged against the local catalog + open requests so cards render Play /
//! chip / Demander directly. Gated on `requests.create`.

mod flags;

use std::collections::{HashMap, HashSet};

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::api::dto::SearchHit;
use crate::api::error::{json_error, lerr};
use crate::api::extract::AuthUser;
use crate::api::permission::require;
use crate::api::util::{blocking, query};
use crate::db;
use crate::i18n;
use crate::i18n::{AskedLocale, ContentLocale};
use crate::infra::metadata::discover::{self, FilterQuery, FilterSort};
use crate::infra::metadata::season_episodes;
use crate::model::{DiscoverEpisode, DiscoverResponse, Permission, RequestKind, User};
use crate::services::search::{Asker, HitKind};
use crate::services::settings;
use crate::state::SharedState;

use flags::{flag_detail, flag_hits};

pub fn routes() -> Router<SharedState> {
    Router::new()
        .route("/discover/search", get(search))
        .route("/discover/trending", get(trending))
        .route("/discover/filter", get(filter))
        .route("/discover/local", get(local))
        .route("/discover/{kind}/{tmdb_id}", get(detail))
        .route("/discover/tv/{tmdb_id}/season/{season}", get(season))
}

// TMDB works zero-config via the built-in key, so absence means the operator
// explicitly blanked it: surface that rather than a silent empty page.
fn require_tmdb(state: &SharedState, user: &User) -> Result<String, Response> {
    state.config.tmdb_api_key.clone().ok_or_else(|| {
        lerr(
            i18n::user_locale(user),
            StatusCode::SERVICE_UNAVAILABLE,
            "error.tmdbUnavailable",
        )
    })
}

fn scope_from_type(kind: Option<&str>) -> discover::DiscoverScope {
    match kind {
        Some("movie") => discover::DiscoverScope::Movies,
        Some("tv") | Some("show") => discover::DiscoverScope::Shows,
        _ => discover::DiscoverScope::All,
    }
}

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    #[serde(default)]
    q: String,
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    page: Option<u32>,
    #[serde(default)]
    state: Option<String>,
}

pub async fn search(
    State(state): State<SharedState>,
    AskedLocale(reader): AskedLocale,
    AuthUser(user): AuthUser,
    Query(params): Query<SearchParams>,
) -> Result<Response, Response> {
    require(&user, Permission::RequestsCreate)?;
    let key = require_tmdb(&state, &user)?;
    let lang = settings::metadata_language_for(&state.settings, &state.config, reader);
    let query = params.q.trim().to_string();
    if query.is_empty() {
        return Ok(Json(DiscoverResponse {
            results: Vec::new(),
            page: 1,
            total_pages: 1,
        })
        .into_response());
    }
    let state_filter = match params.state.as_deref() {
        Some("in") => Some(true),
        Some("out") => Some(false),
        None | Some("any") => None,
        Some(_) => return Err(json_error(StatusCode::BAD_REQUEST, "unknown availability")),
    };
    let scope = scope_from_type(params.kind.as_deref());
    let page = params.page.unwrap_or(1);
    let out = blocking(move || {
        let found = discover::search(&key, &lang, scope, &query, page)
            .map_err(|()| anyhow::anyhow!("TMDB search failed"))?;
        let conn = state.db.get()?;
        let mut results = flag_hits(&conn, found.hits, &user)?;
        if let Some(want) = state_filter {
            results.retain(|entry| entry.in_library == want);
        }
        Ok(DiscoverResponse {
            results,
            page: found.page,
            total_pages: found.total_pages,
        })
    })
    .await?;
    Ok(Json(out).into_response())
}

#[derive(Debug, Deserialize)]
pub struct TrendingParams {
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    page: Option<u32>,
}

/// `GET /api/discover/trending?type=&page=` this week's trending titles. No
/// `type` = merged movies + shows (page 1) for the discover empty-state rails;
/// `type=movie|tv` = a paginated single-kind list backing the full page.
pub async fn trending(
    State(state): State<SharedState>,
    AskedLocale(reader): AskedLocale,
    AuthUser(user): AuthUser,
    Query(params): Query<TrendingParams>,
) -> Result<Response, Response> {
    require(&user, Permission::RequestsCreate)?;
    let key = require_tmdb(&state, &user)?;
    let lang = settings::metadata_language_for(&state.settings, &state.config, reader);
    let scope = scope_from_type(params.kind.as_deref());
    let page = params.page.unwrap_or(1);
    let out = blocking(move || {
        let found = discover::trending(&key, &lang, scope, page)
            .map_err(|()| anyhow::anyhow!("TMDB trending failed"))?;
        let conn = state.db.get()?;
        Ok(DiscoverResponse {
            results: flag_hits(&conn, found.hits, &user)?,
            page: found.page,
            total_pages: found.total_pages,
        })
    })
    .await?;
    Ok(Json(out).into_response())
}

// --- Filtered discovery -----------------------------------------------------

/// `GET /api/discover/filter?type=movie|tv&sort=&language=&year=&decade=&genre=&page=&state=`
/// TMDB's own browse endpoint, for readers who have no title in mind yet.
/// `state=in|out` narrows the page to titles this server does (not) hold, read
/// from the same local TMDB-id mapping the cards are flagged with.
#[derive(Debug, Deserialize)]
pub struct FilterParams {
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    sort: Option<String>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    year: Option<u32>,
    #[serde(default)]
    decade: Option<u32>,
    #[serde(default)]
    genre: Option<u32>,
    #[serde(default)]
    page: Option<u32>,
    #[serde(default)]
    state: Option<String>,
}

/// One page of TMDB discover results, flagged against this server.
pub async fn filter(
    State(state): State<SharedState>,
    AskedLocale(reader): AskedLocale,
    AuthUser(user): AuthUser,
    Query(params): Query<FilterParams>,
) -> Result<Response, Response> {
    require(&user, Permission::RequestsCreate)?;
    let key = require_tmdb(&state, &user)?;

    let kind = match params.kind.as_deref() {
        Some("tv") | Some("show") => RequestKind::Show,
        None | Some("movie") => RequestKind::Movie,
        Some(_) => return Err(json_error(StatusCode::BAD_REQUEST, "unknown media type")),
    };
    if let Some(year) = params.year {
        if !(1870..=2100).contains(&year) {
            return Err(json_error(StatusCode::BAD_REQUEST, "year out of range"));
        }
    }
    let decade = match params.decade {
        Some(d) if (1870..=2100).contains(&d) && d % 10 == 0 => Some(d),
        Some(_) => return Err(json_error(StatusCode::BAD_REQUEST, "decade out of range")),
        None => None,
    };
    let language = params
        .language
        .filter(|l| l.len() == 2 || l.len() == 3)
        .filter(|l| l.chars().all(|c| c.is_ascii_alphabetic()))
        .map(|l| l.to_ascii_lowercase());
    let state_filter = match params.state.as_deref() {
        Some("in") => Some(true),
        Some("out") => Some(false),
        None | Some("any") => None,
        Some(_) => return Err(json_error(StatusCode::BAD_REQUEST, "unknown availability")),
    };
    let sort = match params.sort.as_deref() {
        Some("release") => FilterSort::Release,
        Some("rating") => FilterSort::Rating,
        None | Some("popularity") => FilterSort::Popularity,
        Some(_) => return Err(json_error(StatusCode::BAD_REQUEST, "unknown sort order")),
    };

    let lang = settings::metadata_language_for(&state.settings, &state.config, reader);
    let page = params.page.unwrap_or(1);
    let out = blocking(move || {
        let found = discover::filter(&FilterQuery {
            api_key: &key,
            language: &lang,
            kind,
            sort,
            original_language: language.as_deref(),
            year: params.year,
            decade,
            genre: params.genre,
            page,
        })
        .map_err(|()| anyhow::anyhow!("TMDB discover failed"))?;
        let conn = state.db.get()?;
        let mut results = flag_hits(&conn, found.hits, &user)?;
        if let Some(want) = state_filter {
            results.retain(|entry| entry.in_library == want);
        }
        Ok(json!({
            "results": results,
            "page": found.page,
            "totalPages": found.total_pages,
            "totalResults": found.total_results,
        }))
    })
    .await?;
    Ok(Json(out).into_response())
}

// --- Local library discovery ------------------------------------------------

/// `GET /api/discover/local?q=&type=all|movie|tv&library=&year=&decade=&genre=&rating=&watched=&page=&perPage=`
/// This server's own catalogue as a filterable discovery grid: an optional text
/// query (the same ranked engine `/api/search` uses) plus library, year/decade,
/// TMDB genre id, minimum rating and watched-state filters. Movies and shows
/// only; episodes belong to their show here. The filters run in the database,
/// so a page is never a client-side slice of the whole library.
#[derive(Debug, Deserialize)]
pub struct LocalParams {
    #[serde(default)]
    q: Option<String>,
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    library: Option<String>,
    #[serde(default)]
    year: Option<u32>,
    #[serde(default)]
    decade: Option<u32>,
    #[serde(default)]
    genre: Option<u32>,
    #[serde(default)]
    rating: Option<f32>,
    #[serde(default)]
    watched: Option<String>,
    #[serde(default)]
    page: Option<u32>,
    #[serde(rename = "perPage", default)]
    per_page: Option<u32>,
}

const LOCAL_DEFAULT_PER_PAGE: u32 = 24;
const LOCAL_MAX_PER_PAGE: u32 = 60;
const LOCAL_QUERY_CHARS: usize = 128;
const LOCAL_MAX_SEARCH_HITS: usize = 300;

pub async fn local(
    State(state): State<SharedState>,
    AuthUser(user): AuthUser,
    ContentLocale(locale): ContentLocale,
    Query(params): Query<LocalParams>,
) -> Result<Response, Response> {
    if let Some(year) = params.year {
        if !(1870..=2100).contains(&year) {
            return Err(json_error(StatusCode::BAD_REQUEST, "year out of range"));
        }
    }
    let decade = match params.decade {
        Some(d) if (1870..=2100).contains(&d) && d % 10 == 0 => Some(d),
        Some(_) => return Err(json_error(StatusCode::BAD_REQUEST, "decade out of range")),
        None => None,
    };
    if let Some(rating) = params.rating {
        if !(0.0..=10.0).contains(&rating) {
            return Err(json_error(StatusCode::BAD_REQUEST, "rating out of range"));
        }
    }
    let kind = match params.kind.as_deref() {
        Some("movie") => Some(RequestKind::Movie),
        Some("tv") | Some("show") => Some(RequestKind::Show),
        None | Some("all") => None,
        Some(_) => return Err(json_error(StatusCode::BAD_REQUEST, "unknown media type")),
    };
    let watched = match params.watched.as_deref() {
        Some("watched") => Some(true),
        Some("unwatched") => Some(false),
        None | Some("any") => None,
        Some(_) => return Err(json_error(StatusCode::BAD_REQUEST, "unknown watched filter")),
    };
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params
        .per_page
        .unwrap_or(LOCAL_DEFAULT_PER_PAGE)
        .clamp(1, LOCAL_MAX_PER_PAGE);
    let q: String = params
        .q
        .unwrap_or_default()
        .trim()
        .chars()
        .take(LOCAL_QUERY_CHARS)
        .collect();
    let library = params.library.filter(|l| !l.is_empty());

    let engine = state.clone();
    let out = query(&state.db, move |pool| {
        let played = settings::played_percent(&engine.settings);
        let uid = &user.id;
        let mut watched_ids: HashSet<String> =
            db::list_watched(&pool, uid)?.into_iter().collect();
        for progress in db::list_progress(&pool, uid)? {
            let finished = progress
                .duration_ms
                .filter(|d| *d > 0)
                .is_some_and(|d| progress.position_ms * 100 >= d * played);
            if finished {
                watched_ids.insert(progress.item_id);
            }
        }
        let series = db::show_progress(&pool, uid).unwrap_or_default();

        let mut hits: Vec<SearchHit> = if q.is_empty() {
            browse_hits(&pool, &user, library.as_deref(), locale)?
        } else {
            let visible = |lib: &str| {
                library.as_deref().is_none_or(|want| lib == want) && user.sees_library(lib)
            };
            let favoured: HashMap<String, f32> = HashMap::new();
            let asker = Asker {
                visible: &visible,
                favoured: &favoured,
            };
            let answer = engine.search.answer(&q, LOCAL_MAX_SEARCH_HITS, &asker);
            hydrate(&pool, answer.hits, locale)?
        };

        let in_year = |year: Option<u32>| match (year, params.year, decade) {
            (_, Some(want), _) => year == Some(want),
            (_, None, Some(start)) => year.is_some_and(|y| (start..start + 10).contains(&y)),
            _ => true,
        };
        let has_genre = |ids: &[u32]| match params.genre {
            Some(want) => ids.contains(&want),
            None => true,
        };
        let min_rating = |rating: Option<f32>| match params.rating {
            Some(want) => rating.is_some_and(|r| r >= want),
            None => true,
        };
        let wanted_kind = kind;
        let movie_ok = |id: &str, library_id: &str, year: Option<u32>, meta: Option<&crate::model::Metadata>| {
            if wanted_kind == Some(RequestKind::Show) || !user.sees_library(library_id) {
                return false;
            }
            if library.as_deref().is_some_and(|want| library_id != want) {
                return false;
            }
            if !in_year(year)
                || !has_genre(meta.map(|m| m.tmdb_genre_ids.as_slice()).unwrap_or(&[]))
                || !min_rating(meta.and_then(|m| m.rating))
            {
                return false;
            }
            match watched {
                Some(want) => watched_ids.contains(id) == want,
                None => true,
            }
        };
        hits.retain(|hit| match hit {
            SearchHit::Movie { item } => {
                movie_ok(&item.id, &item.library, item.year, item.metadata.as_ref())
            }
            SearchHit::MovieCard { item } => {
                movie_ok(&item.id, &item.library, item.year, item.metadata.as_ref())
            }
            SearchHit::Show { show } => {
                if wanted_kind == Some(RequestKind::Movie) || !user.sees_library(&show.library) {
                    return false;
                }
                if library.as_deref().is_some_and(|want| show.library != want) {
                    return false;
                }
                let meta = show.metadata.as_ref();
                if !in_year(show.year)
                    || !has_genre(meta.map(|m| m.tmdb_genre_ids.as_slice()).unwrap_or(&[]))
                    || !min_rating(meta.and_then(|m| m.rating))
                {
                    return false;
                }
                match watched {
                    Some(want) => (series.get(&show.id).copied().unwrap_or(0) >= 100) == want,
                    None => true,
                }
            }
            // Episodes are not titles: their show carries the filters.
            SearchHit::Episode { .. } | SearchHit::EpisodeCard { .. } => false,
        });

        if q.is_empty() {
            hits.sort_by(|a, b| match (a, b) {
                (SearchHit::Movie { item: a }, SearchHit::Movie { item: b }) => {
                    b.added_at.cmp(&a.added_at).then_with(|| a.title.cmp(&b.title))
                }
                (SearchHit::Show { show: a }, SearchHit::Show { show: b }) => {
                    b.added_at.cmp(&a.added_at).then_with(|| a.title.cmp(&b.title))
                }
                _ => std::cmp::Ordering::Equal,
            });
        }

        let total = hits.len();
        let total_pages = ((total as u32) + per_page - 1) / per_page;
        let start = ((page - 1) as usize).saturating_mul(per_page as usize);
        let results: Vec<SearchHit> = hits
            .into_iter()
            .skip(start)
            .take(per_page as usize)
            .collect();
        Ok(json!({
            "results": results,
            "page": page,
            "totalPages": total_pages.max(1),
            "totalResults": total,
        }))
    })
    .await?;
    Ok(Json(out).into_response())
}

/// The unfiltered catalogue as cards, for the local discovery grid's first
/// paint (no text query). Sorted by recency, which is the browse default.
fn browse_hits(
    pool: &db::Pool,
    user: &User,
    library: Option<&str>,
    locale: &str,
) -> anyhow::Result<Vec<SearchHit>> {
    let mut items = db::list_movies(pool, library)?;
    db::localize::overlay_items(pool, &mut items, locale)?;
    let mut shows = db::list_shows(pool, library)?;
    db::localize::overlay_shows(pool, &mut shows, locale)?;
    let mut hits = Vec::with_capacity(items.len() + shows.len());
    for item in items {
        if user.sees_library(&item.library) {
            hits.push(SearchHit::Movie { item });
        }
    }
    for show in shows {
        if user.sees_library(&show.library) {
            hits.push(SearchHit::Show { show });
        }
    }
    Ok(hits)
}

/// Hydrate ranked ids into the same card shapes `/api/search` returns.
fn hydrate(
    pool: &db::Pool,
    hits: Vec<crate::services::search::Hit>,
    locale: &str,
) -> anyhow::Result<Vec<SearchHit>> {
    let item_ids: Vec<String> = hits
        .iter()
        .filter(|h| h.kind != HitKind::Show)
        .map(|h| h.id.clone())
        .collect();
    let show_ids: Vec<String> = hits
        .iter()
        .filter(|h| h.kind == HitKind::Show)
        .map(|h| h.id.clone())
        .collect();

    let mut item_vec = db::get_items_by_ids(pool, &item_ids)?;
    db::localize::overlay_items(pool, &mut item_vec, locale)?;
    let mut items: HashMap<String, _> = item_vec.into_iter().map(|i| (i.id.clone(), i)).collect();
    let mut show_vec = db::get_shows_by_ids(pool, &show_ids)?;
    db::localize::overlay_shows(pool, &mut show_vec, locale)?;
    let mut shows: HashMap<String, _> = show_vec.into_iter().map(|s| (s.id.clone(), s)).collect();

    let mut out = Vec::with_capacity(hits.len());
    for hit in hits {
        match hit.kind {
            HitKind::Show => {
                if let Some(show) = shows.remove(&hit.id) {
                    out.push(SearchHit::Show { show });
                }
            }
            HitKind::Movie | HitKind::Episode => {
                if let Some(item) = items.remove(&hit.id) {
                    out.push(if hit.kind == HitKind::Episode {
                        SearchHit::Episode { item }
                    } else {
                        SearchHit::Movie { item }
                    });
                }
            }
        }
    }
    Ok(out)
}

/// `GET /api/discover/{movie|tv}/:tmdbId` detail + seasons + availability.
pub async fn detail(
    State(state): State<SharedState>,
    AskedLocale(reader): AskedLocale,
    AuthUser(user): AuthUser,
    Path((kind, tmdb_id)): Path<(String, u64)>,
) -> Result<Response, Response> {
    require(&user, Permission::RequestsCreate)?;
    let key = require_tmdb(&state, &user)?;
    let loc = i18n::user_locale(&user);
    let kind = match kind.as_str() {
        "movie" => RequestKind::Movie,
        "tv" | "show" => RequestKind::Show,
        _ => return Err(json_error(StatusCode::NOT_FOUND, "unknown media type")),
    };
    let lang = settings::metadata_language_for(&state.settings, &state.config, reader);
    let out = blocking(move || {
        let detail = discover::detail(&key, &lang, kind, tmdb_id)
            .map_err(|()| anyhow::anyhow!("TMDB detail failed"))?;
        let conn = state.db.get()?;
        detail.map(|d| flag_detail(&conn, d, &user)).transpose()
    })
    .await?;
    match out {
        Some(d) => Ok(Json(d).into_response()),
        None => Err(lerr(loc, StatusCode::NOT_FOUND, "error.itemNotFound")),
    }
}

/// `GET /api/discover/tv/:tmdbId/season/:season` the episodes of one season, in
/// the viewer's language. A season TMDB cannot answer is an empty list.
pub async fn season(
    State(state): State<SharedState>,
    AskedLocale(reader): AskedLocale,
    AuthUser(user): AuthUser,
    Path((tmdb_id, season)): Path<(u64, u32)>,
) -> Result<Response, Response> {
    require(&user, Permission::RequestsCreate)?;
    let key = require_tmdb(&state, &user)?;
    let lang = settings::metadata_language_for(&state.settings, &state.config, reader);
    let found = blocking(move || Ok(season_episodes(&key, &lang, tmdb_id, season))).await?;
    let episodes: Vec<DiscoverEpisode> = found.episodes.into_iter().map(Into::into).collect();
    Ok(Json(json!({ "episodes": episodes })).into_response())
}
