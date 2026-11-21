//! Enforces per-user library visibility (ACCT-20, ACCT-21) on the catalogue
//! handlers: a title in a library the caller was not granted is absent from
//! every listing and unreachable by its id.

use std::collections::HashSet;
use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::Response;

use crate::api::content_limit;
use crate::api::error::json_error;
use crate::api::util::query;
use crate::db;
use crate::model::{LibraryScope, MediaItem, Section, SectionItem, Show, User};
use crate::state::SharedState;

/// A title the caller may not see answers exactly as one that was never
/// scanned: a `404` distinguishable from "forbidden" would itself confirm the
/// title exists.
pub fn out_of_scope() -> Response {
    json_error(StatusCode::NOT_FOUND, "item not found")
}

/// Refuse unless `item_id` is in a library `user` was granted. An unknown id
/// refuses too, so probing cannot tell the two apart.
pub async fn gate_item(state: &SharedState, user: &User, item_id: &str) -> Result<(), Response> {
    if user.sees_every_library() {
        return Ok(());
    }
    let id = item_id.to_string();
    let library = query(&state.db, move |pool| db::item_library(&pool, &id)).await?;
    match library {
        Some(library) if user.sees_library(&library) => {}
        _ => return Err(out_of_scope()),
    }
    if let Some(limit) = content_limit::for_user(user) {
        let id = item_id.to_string();
        // An episode inherits its show's rating, which is why the lookup reads
        // the show when the episode carries none of its own.
        let certification =
            query(&state.db, move |pool| db::item_certification(&pool, &id)).await?;
        if !limit.allows(certification.as_deref()) {
            return Err(out_of_scope());
        }
    }
    Ok(())
}

/// Refuse unless the caller may see what `subject_id` saves to "Ma liste" or
/// marks watched: an item or a show in a granted library, or `tmdb:<id>`, a
/// title not in the library yet, which no grant covers and none hides.
pub async fn gate_saved(
    state: &SharedState,
    user: &User,
    subject_id: &str,
) -> Result<(), Response> {
    if user.sees_every_library() || is_tmdb_title(subject_id) {
        return Ok(());
    }
    let id = subject_id.to_string();
    let library = query(&state.db, move |pool| db::subject_library(&pool, &id)).await?;
    match library {
        Some(library) if user.sees_library(&library) => Ok(()),
        _ => Err(out_of_scope()),
    }
}

/// Which of the saved `subject_ids` the caller may see, under the rule of
/// [`gate_saved`].
pub async fn visible_saved_ids(
    state: &SharedState,
    user: &User,
    subject_ids: Vec<String>,
) -> Result<HashSet<String>, Response> {
    if user.sees_every_library() {
        return Ok(subject_ids.into_iter().collect());
    }
    let (tmdb, local): (Vec<String>, Vec<String>) =
        subject_ids.into_iter().partition(|id| is_tmdb_title(id));
    let mut visible: HashSet<String> = tmdb.into_iter().collect();
    if local.is_empty() {
        return Ok(visible);
    }
    let libraries = query(&state.db, move |pool| db::subject_libraries(&pool, &local)).await?;
    visible.extend(
        libraries
            .into_iter()
            .filter(|(_, library)| user.sees_library(library))
            .map(|(id, _)| id),
    );
    Ok(visible)
}

fn is_tmdb_title(subject_id: &str) -> bool {
    subject_id
        .strip_prefix("tmdb:")
        .is_some_and(|n| n.bytes().all(|b| b.is_ascii_digit()) && n.parse::<u64>().is_ok())
}

pub async fn gate_show(state: &SharedState, user: &User, show_id: &str) -> Result<(), Response> {
    if user.sees_every_library() {
        return Ok(());
    }
    let id = show_id.to_string();
    let library = query(&state.db, move |pool| db::show_library(&pool, &id)).await?;
    match library {
        Some(library) if user.sees_library(&library) => {}
        _ => return Err(out_of_scope()),
    }
    if let Some(limit) = content_limit::for_user(user) {
        let id = show_id.to_string();
        let certification =
            query(&state.db, move |pool| db::show_certification(&pool, &id)).await?;
        if !limit.allows(certification.as_deref()) {
            return Err(out_of_scope());
        }
    }
    Ok(())
}

/// The item behind `item_id` when the caller may see it. `None` both for an
/// unknown id and for one outside an identified caller's grant; an anonymous
/// caller is outside this requirement's reach (see the byte routes in
/// [`crate::api::stream`]).
pub async fn item_in_scope(
    state: &SharedState,
    caller: Option<&User>,
    item_id: String,
) -> Option<MediaItem> {
    let item = query(&state.db, move |pool| db::get_item(&pool, &item_id))
        .await
        .ok()
        .flatten()?;
    if let Some(user) = caller {
        if !user.sees_library(&item.library) {
            return None;
        }
        if let Some(limit) = content_limit::for_user(user) {
            let id = item.id.clone();
            // Episodes carry no rating of their own; the show's is the answer.
            let certification = if item.show_id.is_some() {
                query(&state.db, move |pool| db::item_certification(&pool, &id))
                    .await
                    .ok()
                    .flatten()
            } else {
                item.metadata
                    .as_ref()
                    .and_then(|meta| meta.certification.clone())
            };
            if !limit.allows(certification.as_deref()) {
                return None;
            }
        }
    }
    Some(item)
}

pub fn keep_items(user: &User, items: &mut Vec<MediaItem>) {
    if let Some(limit) = content_limit::for_user(user) {
        items.retain(|item| limit.allows_metadata(item.metadata.as_ref()));
    }
    if user.sees_every_library() {
        return;
    }
    items.retain(|item| user.sees_library(&item.library));
}

/// [`keep_items`] with an episode's rating taken from its show. A list that can
/// carry episodes must use this, or an R-rated series would come back through
/// an episode whose own metadata carries no rating.
pub async fn keep_items_certified(
    state: &SharedState,
    user: &User,
    items: &mut Vec<MediaItem>,
) -> Result<(), Response> {
    keep_items(user, items);
    let Some(limit) = content_limit::for_user(user) else {
        return Ok(());
    };
    let show_ids: HashSet<String> = items
        .iter()
        .filter_map(|item| item.show_id.clone())
        .collect();
    if show_ids.is_empty() {
        return Ok(());
    }
    let ids: Vec<String> = show_ids.into_iter().collect();
    let shows = query(&state.db, move |pool| db::show_certifications(&pool, &ids)).await?;
    items.retain(|item| {
        if item.show_id.is_none() {
            return true;
        }
        let own = item
            .metadata
            .as_ref()
            .and_then(|meta| meta.certification.as_deref());
        let inherited = item
            .show_id
            .as_ref()
            .and_then(|id| shows.get(id))
            .and_then(|cert| cert.as_deref());
        limit.allows(own.or(inherited))
    });
    Ok(())
}

pub fn keep_shows(user: &User, shows: &mut Vec<Show>) {
    if let Some(limit) = content_limit::for_user(user) {
        shows.retain(|show| limit.allows_metadata(show.metadata.as_ref()));
    }
    if user.sees_every_library() {
        return;
    }
    shows.retain(|show| user.sees_library(&show.library));
}

/// Drop hidden entries from every home rail, then drop the rails left empty: a
/// titled row with nothing in it announces what was removed. A caller held to
/// no grant gets the shared rails back without a copy.
pub fn keep_sections(user: &User, sections: Arc<Vec<Section>>) -> Arc<Vec<Section>> {
    if user.sees_every_library() || user.libraries == LibraryScope::All {
        return sections;
    }
    let limit = content_limit::for_user(user);
    let mut kept = Vec::clone(&sections);
    for section in kept.iter_mut() {
        section.items.retain(|entry| {
            user.sees_library(library_of(entry))
                && limit.is_none_or(|limit| limit.allows_metadata(metadata_of(entry)))
        });
    }
    kept.retain(|section| !section.items.is_empty());
    Arc::new(kept)
}

pub fn keep_section_item(user: &User, entry: Option<SectionItem>) -> Option<SectionItem> {
    entry.filter(|entry| user.sees_library(library_of(entry)))
}

/// Which of `item_ids` the caller may see. An unrestricted caller gets every id
/// back, so the answer is safe to use as the only filter.
pub async fn visible_item_ids(
    state: &SharedState,
    user: &User,
    item_ids: Vec<String>,
) -> Result<HashSet<String>, Response> {
    if user.sees_every_library() {
        return Ok(item_ids.into_iter().collect());
    }
    if item_ids.is_empty() {
        return Ok(HashSet::new());
    }
    let libraries = query(&state.db, move |pool| db::item_libraries(&pool, &item_ids)).await?;
    Ok(libraries
        .into_iter()
        .filter(|(_, library)| user.sees_library(library))
        .map(|(id, _)| id)
        .collect())
}

fn library_of(entry: &SectionItem) -> &str {
    match entry {
        SectionItem::Movie { item } => &item.library,
        SectionItem::Show { show } => &show.library,
    }
}

fn metadata_of(entry: &SectionItem) -> Option<&crate::model::Metadata> {
    match entry {
        SectionItem::Movie { item } => item.metadata.as_ref(),
        SectionItem::Show { show } => show.metadata.as_ref(),
    }
}

#[cfg(test)]
mod tests;
