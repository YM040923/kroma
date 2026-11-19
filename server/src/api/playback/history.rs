//! The personal watch history: what the caller finished and what they left
//! half-watched, with the edits the history screen offers.

use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::api::extract::AuthUser;
use crate::api::util::query;
use crate::api::visibility;
use crate::db;
use crate::i18n::ContentLocale;
use crate::model::{HistoryView, WatchedEntry};
use crate::services::settings;
use crate::state::SharedState;

/// `GET /api/history` (Bearer) → `HistoryView`. Both lists are hydrated and
/// permission-filtered, watched newest first.
pub async fn history(
    State(state): State<SharedState>,
    AuthUser(user): AuthUser,
    ContentLocale(locale): ContentLocale,
) -> Response {
    let uid = user.id.clone();
    let played_percent = settings::played_percent(&state.settings);
    let result = query(&state.db, move |pool| {
        let marks = db::list_watched_entries(&pool, &uid)?;
        let ids: Vec<String> = marks.iter().map(|(id, _)| id.clone()).collect();
        let mut items = db::get_items_by_ids(&pool, &ids)?;
        db::localize::overlay_items(&pool, &mut items, locale)?;
        let mut by_id: HashMap<String, _> =
            items.into_iter().map(|item| (item.id.clone(), item)).collect();
        let watched = marks
            .into_iter()
            .filter_map(|(id, watched_at)| {
                by_id.remove(&id).map(|item| WatchedEntry { item, watched_at })
            })
            .collect::<Vec<_>>();

        let mut in_progress = db::continue_watching(&pool, &uid, played_percent)?;
        db::localize::overlay_each(&pool, in_progress.iter_mut().map(|row| &mut row.item), locale)?;
        Ok(HistoryView {
            watched,
            in_progress,
        })
    })
    .await;
    let HistoryView {
        mut watched,
        mut in_progress,
    } = match result {
        Ok(view) => view,
        Err(resp) => return resp,
    };
    if !user.sees_every_library() {
        watched.retain(|entry| user.sees_library(&entry.item.library));
        in_progress.retain(|row| user.sees_library(&row.item.library));
    }
    Json(HistoryView {
        watched,
        in_progress,
    })
    .into_response()
}

/// `DELETE /api/history` (Bearer) → 204. Forgets every watched flag and resume
/// position the caller holds.
pub async fn clear_all(State(state): State<SharedState>, AuthUser(user): AuthUser) -> Response {
    match query(&state.db, move |pool| {
        let watched = db::delete_watched_all(&pool, &user.id)?;
        let progress = db::delete_progress_all(&pool, &user.id)?;
        Ok((watched, progress))
    })
    .await
    {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(resp) => resp,
    }
}

/// `DELETE /api/history/shows/:id` (Bearer) → 204. Clears a show's episodes
/// from the caller's history (watched flags and resume positions).
pub async fn clear_show(
    State(state): State<SharedState>,
    AuthUser(user): AuthUser,
    Path(show_id): Path<String>,
) -> Response {
    if let Err(resp) = visibility::gate_saved(&state, &user, &show_id).await {
        return resp;
    }
    match query(&state.db, move |pool| {
        db::clear_show_history(&pool, &user.id, &show_id)
    })
    .await
    {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(resp) => resp,
    }
}

/// `DELETE /api/history/items` (Bearer) `{ itemIds: [...] }` → 204. Forgets a
/// batch of watched flags and resume positions, for the history screen's
/// multi-select.
pub async fn clear_items(
    State(state): State<SharedState>,
    AuthUser(user): AuthUser,
    Json(body): Json<ClearItemsBody>,
) -> Response {
    if body.item_ids.is_empty() {
        return StatusCode::NO_CONTENT.into_response();
    }
    for id in &body.item_ids {
        if let Err(resp) = visibility::gate_saved(&state, &user, id).await {
            return resp;
        }
    }
    match query(&state.db, move |pool| {
        let ids = body.item_ids;
        let watched = db::delete_watched_many(&pool, &user.id, &ids)?;
        let progress = db::delete_progress_many(&pool, &user.id, &ids)?;
        Ok((watched, progress))
    })
    .await
    {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(resp) => resp,
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct ClearItemsBody {
    #[serde(rename = "itemIds")]
    pub item_ids: Vec<String>,
}
