//! TMDB metadata endpoints: details + IDs for one item or show. Results are
//! cached; returns 503 when `KROMA_TMDB_API_KEY` is unset, 404 on no match.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::api::error::json_error;
use crate::api::extract::AuthUser;
use crate::api::util::{blocking, query};
use crate::api::visibility;
use crate::db;
use crate::i18n::AskedLocale;
use crate::infra::metadata::{self, Target};
use crate::model::Kind;
use crate::state::SharedState;
use axum::routing::get;
use axum::Router;

pub fn routes() -> Router<SharedState> {
    Router::new()
        .route("/shows/{id}/metadata", get(show_metadata))
        .route("/items/{id}/metadata", get(item_metadata))
}

/// `GET /api/items/:id/metadata` → TMDB details + IDs for one item.
///
/// Episodes serve their stored, localized metadata without a TMDB key. An
/// unenriched episode returns 404 rather than substituting the parent show.
/// Movies resolve against TMDB movies and require `KROMA_TMDB_API_KEY`.
pub async fn item_metadata(
    State(state): State<SharedState>,
    AuthUser(user): AuthUser,
    AskedLocale(reader): AskedLocale,
    Path(id): Path<String>,
) -> Result<Response, Response> {
    visibility::gate_item(&state, &user, &id).await?;
    let language =
        crate::services::settings::metadata_language_for(&state.settings, &state.config, reader);
    let item = query(&state.db, move |pool| {
        let mut item = db::get_item(&pool, &id)?;
        if let Some(ep) = item.as_mut().filter(|i| i.kind == Kind::Episode) {
            db::localize::overlay_episode_metadata(&pool, ep, &language)?;
        }
        Ok(item)
    })
    .await?
    .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "item not found"))?;

    if item.kind == Kind::Episode {
        return item.metadata.map(|meta| Json(meta).into_response())
            .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "no episode metadata"));
    }
    let api_key = require_tmdb_key(&state)?;
    resolve_metadata(state, api_key, Target::Movie, item.title, item.year, reader).await
}

/// `GET /api/shows/:id/metadata` → TMDB details + IDs for one show.
pub async fn show_metadata(
    State(state): State<SharedState>,
    AuthUser(user): AuthUser,
    AskedLocale(reader): AskedLocale,
    Path(id): Path<String>,
) -> Result<Response, Response> {
    visibility::gate_show(&state, &user, &id).await?;
    let api_key = require_tmdb_key(&state)?;

    let show = query(&state.db, move |pool| db::get_show(&pool, &id))
        .await?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "show not found"))?
        .show;

    resolve_metadata(state, api_key, Target::Tv, show.title, show.year, reader).await
}

fn require_tmdb_key(state: &SharedState) -> Result<String, Response> {
    state.config.tmdb_api_key.clone().ok_or_else(|| {
        json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "metadata disabled: set KROMA_TMDB_API_KEY",
        )
    })
}

async fn resolve_metadata(
    state: SharedState,
    api_key: String,
    target: Target,
    title: String,
    year: Option<u32>,
    reader: Option<&str>,
) -> Result<Response, Response> {
    let language =
        crate::services::settings::metadata_language_for(&state.settings, &state.config, reader);
    let result = blocking(move || {
        Ok(metadata::lookup(
            &state.metadata_cache,
            &api_key,
            &language,
            target,
            &title,
            year,
        ))
    })
    .await?;

    let resp = match result {
        Some(meta) => Json(meta).into_response(),
        None => json_error(StatusCode::NOT_FOUND, "no metadata match"),
    };
    Ok(resp)
}
