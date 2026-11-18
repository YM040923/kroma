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
    show_page(state, user, id).await
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
