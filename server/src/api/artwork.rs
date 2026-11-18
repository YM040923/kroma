//! Artwork overrides: list the provider's posters/backdrops and let an operator
//! pin one. The override lives in `art_overrides`, never in the scraped
//! metadata, so the enrichment pass cannot erase it.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::api::error::json_error;
use crate::api::extract::AuthUser;
use crate::api::util::{query, service};
use crate::db;
use crate::infra::metadata::{images, Target};
use crate::model::Permission;
use crate::state::SharedState;

pub fn routes() -> Router<SharedState> {
    Router::new().route("/items/{id}/artwork", get(get_artwork).put(put_artwork))
}

struct Page {
    kind: &'static str,
    id: String,
    tmdb_id: u64,
    target: Target,
    poster: Option<String>,
    backdrop: Option<String>,
    logo: Option<String>,
}

async fn page(state: &SharedState, user: &crate::model::User, id: &str) -> Result<Page, Response> {
    if !user.permissions.contains(&Permission::SettingsManage) {
        return Err(json_error(StatusCode::FORBIDDEN, "artwork needs settings.manage"));
    }
    // An episode's art is its show's art.
    let wanted = id.to_string();
    let item = query(&state.db, move |pool| db::get_item(&pool, &wanted)).await?;
    if let Some(item) = item {
        if let Some(show_id) = item.show_id.clone() {
            return show_page(state, user, &show_id).await;
        }
        crate::api::visibility::gate_item(state, user, &item.id).await?;
        let Some(meta) = item.metadata.as_ref() else {
            return Err(json_error(StatusCode::NOT_FOUND, "item has no metadata yet"));
        };
        if meta.tmdb_id == 0 {
            return Err(json_error(StatusCode::NOT_FOUND, "item has no TMDB id"));
        }
        let overrides = query(&state.db, {
            let id = item.id.clone();
            move |pool| db::art_overrides::get(&pool, db::art_overrides::ITEM_KIND, &id)
        })
        .await?;
        return Ok(Page {
            kind: db::art_overrides::ITEM_KIND,
            id: item.id,
            tmdb_id: meta.tmdb_id,
            target: Target::Movie,
            poster: overrides.get("poster").cloned().or_else(|| meta.poster_url.clone()),
            backdrop: overrides.get("backdrop").cloned().or_else(|| meta.backdrop_url.clone()),
            logo: overrides.get("logo").cloned().or_else(|| meta.logo_url.clone()),
        });
    }
    if let Some(page) = library_page(state, id).await? {
        return Ok(page);
    }
    show_page(state, user, id).await
}

/// A library has no TMDB id; its candidates are the art the titles inside it
/// already carry, and its "current" is whatever a client shows today (the
/// override, else the first title's poster).
async fn library_page(state: &SharedState, id: &str) -> Result<Option<Page>, Response> {
    let defs = crate::services::settings::library_defs(&state.settings, &state.config);
    let Some(def) = defs.iter().find(|def| def.id == id || def.name == id) else {
        return Ok(None);
    };
    let def_id = def.id.clone();
    let overrides = query(&state.db, {
        let id = def_id.clone();
        move |pool| db::art_overrides::get(&pool, db::art_overrides::LIBRARY_KIND, &id)
    })
    .await?;
    let (poster, backdrop) = library_cover(state, id).await?;
    // A library has one cover; whichever art the operator picked fills it,
    // even when they picked it on the backdrop side of the picker.
    let cover = overrides
        .get("poster")
        .or_else(|| overrides.get("backdrop"))
        .cloned();
    Ok(Some(Page {
        kind: db::art_overrides::LIBRARY_KIND,
        id: def_id,
        tmdb_id: 0,
        target: Target::Movie,
        poster: cover.or(poster),
        backdrop: overrides.get("backdrop").cloned().or(backdrop),
        logo: None,
    }))
}

/// The legacy cover: the first title's scraped art, which is what Jellyfin
/// clients have been shown until now.
async fn library_cover(
    state: &SharedState,
    id: &str,
) -> Result<(Option<String>, Option<String>), Response> {
    let defs = crate::services::settings::library_defs(&state.settings, &state.config);
    let Some(def) = defs.iter().find(|def| def.id == id || def.name == id) else {
        return Ok((None, None));
    };
    let name = def.name.clone();
    let def_id = def.id.clone();
    let items = query(&state.db, |pool| db::list_items(&pool, None)).await?;
    let cover = items
        .into_iter()
        .find(|item| item.library == name || item.library == def_id)
        .and_then(|item| item.metadata);
    Ok(match cover {
        Some(meta) => (meta.poster_url, meta.backdrop_url),
        None => (None, None),
    })
}

async fn show_page(state: &SharedState, user: &crate::model::User, id: &str) -> Result<Page, Response> {
    let wanted = id.to_string();
    let detail = query(&state.db, move |pool| db::get_show(&pool, &wanted))
        .await?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "title not found"))?;
    crate::api::visibility::gate_show(state, user, &detail.show.id).await?;
    let Some(meta) = detail.show.metadata.as_ref() else {
        return Err(json_error(StatusCode::NOT_FOUND, "show has no metadata yet"));
    };
    if meta.tmdb_id == 0 {
        return Err(json_error(StatusCode::NOT_FOUND, "show has no TMDB id"));
    }
    let overrides = query(&state.db, {
        let id = detail.show.id.clone();
        move |pool| db::art_overrides::get(&pool, db::art_overrides::SHOW_KIND, &id)
    })
    .await?;
    Ok(Page {
        kind: db::art_overrides::SHOW_KIND,
        id: detail.show.id,
        tmdb_id: meta.tmdb_id,
        target: Target::Tv,
        poster: overrides.get("poster").cloned().or_else(|| meta.poster_url.clone()),
        backdrop: overrides.get("backdrop").cloned().or_else(|| meta.backdrop_url.clone()),
        logo: overrides.get("logo").cloned().or_else(|| meta.logo_url.clone()),
    })
}

async fn get_artwork(
    State(state): State<SharedState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
) -> Response {
    let page = match page(&state, &user, &id).await {
        Ok(page) => page,
        Err(response) => return response,
    };
    if page.kind == db::art_overrides::LIBRARY_KIND {
        return library_artwork(&state, &page).await;
    }
    let Some(api_key) = state.config.tmdb_api_key.clone() else {
        return json_error(StatusCode::SERVICE_UNAVAILABLE, "TMDB API key 未配置");
    };
    let language = crate::services::settings::metadata_language(&state.settings, &state.config);
    let target = page.target;
    let tmdb_id = page.tmdb_id;
    let lists = service(move || {
        images(&api_key, &language, target, tmdb_id)
            .map_err(|()| anyhow::anyhow!("TMDB 图片列表读取失败"))
    })
    .await;
    let (posters, backdrops) = match lists {
        Ok(lists) => lists,
        Err(response) => return response,
    };
    Json(json!({
        "current": {
            "poster": page.poster,
            "backdrop": page.backdrop,
            "logo": page.logo,
        },
        "posters": posters,
        "backdrops": backdrops,
    }))
    .into_response()
}

/// A library's candidates are the posters and backdrops its titles already
/// carry, deduplicated and capped so the grid stays one screenful. Each one
/// names the title it came from: a library has no TMDB entry of its own, so
/// that note is the only label a picker can show.
async fn library_artwork(state: &SharedState, page: &Page) -> Response {
    let items = match query(&state.db, |pool| db::list_items(&pool, None)).await {
        Ok(items) => items,
        Err(response) => return response,
    };
    let defs = crate::services::settings::library_defs(&state.settings, &state.config);
    let name = defs
        .iter()
        .find(|def| def.id == page.id)
        .map(|def| def.name.clone())
        .unwrap_or_default();
    let mut seen_poster: Vec<String> = Vec::new();
    let mut seen_backdrop: Vec<String> = Vec::new();
    let mut posters: Vec<serde_json::Value> = Vec::new();
    let mut backdrops: Vec<serde_json::Value> = Vec::new();
    for item in items
        .iter()
        .filter(|item| item.library == name || item.library == page.id)
    {
        let Some(meta) = item.metadata.as_ref() else {
            continue;
        };
        for (url, seen, into, cap) in [
            (meta.poster_url.as_ref(), &mut seen_poster, &mut posters, 24usize),
            (meta.backdrop_url.as_ref(), &mut seen_backdrop, &mut backdrops, 24usize),
        ] {
            let Some(url) = url else { continue };
            if into.len() >= cap || seen.contains(url) {
                continue;
            }
            seen.push(url.clone());
            into.push(json!({
                "url": url,
                "width": 0,
                "height": 0,
                "language": null,
                "votes": 0,
                "note": item.show_title.clone().unwrap_or_else(|| item.title.clone()),
            }));
        }
    }
    Json(json!({
        "current": {
            "poster": page.poster,
            "backdrop": page.backdrop,
            "logo": page.logo,
        },
        "posters": posters,
        "backdrops": backdrops,
    }))
    .into_response()
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct ArtPick {
    poster: Option<String>,
    backdrop: Option<String>,
    logo: Option<String>,
}

async fn put_artwork(
    State(state): State<SharedState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
    Json(pick): Json<ArtPick>,
) -> Response {
    let page = match page(&state, &user, &id).await {
        Ok(page) => page,
        Err(response) => return response,
    };
    let kind = page.kind;
    let subject = page.id.clone();
    for (art, value) in [
        ("poster", pick.poster),
        ("backdrop", pick.backdrop),
        ("logo", pick.logo),
    ] {
        let Some(value) = value else { continue };
        let value = value.trim().to_string();
        let result = if value.is_empty() {
            query(&state.db, {
                let subject = subject.clone();
                move |pool| db::art_overrides::clear(&pool, kind, &subject, art)
            })
            .await
        } else {
            if !(value.starts_with("https://") || value.starts_with("http://") || value.starts_with("/api/images/")) {
                return json_error(StatusCode::BAD_REQUEST, "artwork must be an http(s) URL");
            }
            // Keep a provider image on this box: clients then read one cached
            // WebP instead of reaching TMDB (which may be unreachable), and the
            // card compositor can find it by its local name.
            let stored = if value.starts_with("/api/images/") {
                value.clone()
            } else {
                let data_dir = state.config.data_dir.clone();
                let remote = value.clone();
                match service(move || {
                    Ok(crate::infra::image::cache_remote(&data_dir, &remote))
                })
                .await
                {
                    Ok(Some(local)) => local,
                    _ => value.clone(),
                }
            };
            query(&state.db, {
                let subject = subject.clone();
                let stored = stored.clone();
                move |pool| db::art_overrides::set(&pool, kind, &subject, art, &stored, "manual")
            })
            .await
        };
        if let Err(response) = result {
            return response;
        }
    }
    Json(json!({ "saved": true })).into_response()
}
