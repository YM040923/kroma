//! Jellyfin compatibility, first slice: public system info, password login,
//! library views, movies/series browsing, seasons/episodes, poster redirects,
//! playback info and direct playback. Shapes follow the Jellyfin 10.9 API; ids
//! stay KROMA's own (clients treat BaseItemDto.Id as an opaque string).
//!
//! The routes live at the ROOT, because that is where Jellyfin clients look
//! (`/Users/AuthenticateByName`, `/Videos/{id}/stream`), not under `/api`.

use std::collections::{HashMap, HashSet};

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::api::error::json_error;
use crate::api::util::query;
use crate::db;
use crate::model::{Kind, MediaItem, Show, User};
use crate::state::SharedState;

mod catalog;

const SERVER_VERSION: &str = "10.9.11";

fn query_value<'a>(params: &'a HashMap<String, String>, key: &str) -> Option<&'a String> {
    params.get(key).or_else(|| {
        params
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(key))
            .map(|(_, value)| value)
    })
}

fn metadata_locale(
    state: &SharedState,
    _headers: &HeaderMap,
    params: &HashMap<String, String>,
) -> &'static str {
    // Jellyfin clients disagree on what they send: 小幻 renders a Chinese UI
    // while requesting these endpoints without a Chinese Accept-Language. The
    // library's own metadata language is the only answer both clients share,
    // so a request header only speaks when it names the same language.
    let configured = crate::services::settings::metadata_language(&state.settings, &state.config);
    let asked = query_value(params, "MetadataLanguage").map(String::as_str);
    let tag = asked.unwrap_or(&configured);
    crate::i18n::content_locale(crate::i18n::detect_locale(None, Some(tag)))
}

async fn localized_items(
    state: &SharedState,
    locale: &'static str,
) -> Result<Vec<MediaItem>, Response> {
    query(&state.db, move |pool| {
        let mut items = db::list_items(&pool, None)?;
        db::localize::overlay_items(&pool, &mut items, locale)?;
        Ok(items)
    })
    .await
}

async fn localized_shows(state: &SharedState, locale: &'static str) -> Result<Vec<Show>, Response> {
    query(&state.db, move |pool| {
        let mut shows = db::list_shows(&pool, None)?;
        db::localize::overlay_shows(&pool, &mut shows, locale)?;
        Ok(shows)
    })
    .await
}

async fn localized_show(
    state: &SharedState,
    id: String,
    locale: &'static str,
) -> Result<Option<crate::model::ShowDetail>, Response> {
    query(&state.db, move |pool| {
        let mut detail = db::get_show(&pool, &id)?;
        if let Some(detail) = detail.as_mut() {
            db::localize::overlay_show_detail(&pool, detail, locale)?;
        }
        Ok(detail)
    })
    .await
}

/// Two rows describe one series when they sit in the same library and matched
/// the same TMDB show. A library holds `欢迎来到实力至上主义的教室` and
/// `…主义教室` as two rows today; without this they browse as two half series.
fn same_series(a: &Show, b: &Show) -> bool {
    if a.library != b.library {
        return false;
    }
    match (
        a.metadata
            .as_ref()
            .map(|meta| meta.tmdb_id)
            .filter(|id| *id > 0),
        b.metadata
            .as_ref()
            .map(|meta| meta.tmdb_id)
            .filter(|id| *id > 0),
    ) {
        (Some(left), Some(right)) => left == right,
        _ => a.id == b.id,
    }
}

fn preferred_row(candidate: &Show, current: &Show) -> bool {
    if candidate.episode_count != current.episode_count {
        candidate.episode_count > current.episode_count
    } else if candidate.added_at != current.added_at {
        candidate.added_at < current.added_at
    } else {
        candidate.id < current.id
    }
}

/// One card per series: the row holding most of it stands for the group, and
/// the others are only reachable through their episodes.
fn dedupe_series(shows: Vec<Show>) -> Vec<Show> {
    let mut kept: Vec<Show> = Vec::new();
    for show in shows {
        match kept
            .iter_mut()
            .find(|existing| same_series(existing, &show))
        {
            Some(existing) => {
                if preferred_row(&show, existing) {
                    *existing = show;
                }
            }
            None => kept.push(show),
        }
    }
    kept.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    kept
}

/// Every row that belongs to the same series as `id`, the preferred one first.
async fn series_group(
    state: &SharedState,
    user: &User,
    locale: &'static str,
    id: &str,
) -> Result<Option<Vec<Show>>, Response> {
    let mut shows = localized_shows(state, locale).await?;
    crate::api::visibility::keep_shows(user, &mut shows);
    let Some(target) = shows.iter().find(|show| show.id == id).cloned() else {
        return Ok(None);
    };
    let mut group: Vec<Show> = shows
        .into_iter()
        .filter(|show| same_series(show, &target))
        .collect();
    group.sort_by(|a, b| {
        if a.episode_count != b.episode_count {
            b.episode_count.cmp(&a.episode_count)
        } else if a.added_at != b.added_at {
            a.added_at.cmp(&b.added_at)
        } else {
            a.id.cmp(&b.id)
        }
    });
    Ok(Some(group))
}

/// A series as the client must see it: every season of every row that matched
/// the same provider show, under the id of the row holding most of it.
async fn merged_detail(
    state: &SharedState,
    user: &User,
    locale: &'static str,
    id: &str,
) -> Result<Option<(crate::model::ShowDetail, Vec<Show>)>, Response> {
    let Some(group) = series_group(state, user, locale, id).await? else {
        return Ok(None);
    };
    let canonical = group[0].clone();
    let Some(mut detail) = localized_show(state, canonical.id.clone(), locale).await? else {
        return Ok(None);
    };
    for member in group.iter().filter(|show| show.id != canonical.id) {
        let Some(other) = localized_show(state, member.id.clone(), locale).await? else {
            continue;
        };
        for season in other.seasons {
            match detail
                .seasons
                .iter_mut()
                .find(|existing| existing.number == season.number)
            {
                Some(existing) => {
                    for episode in season.episodes {
                        if !existing.episodes.iter().any(|known| {
                            known.episode == episode.episode && known.id == episode.id
                                || (known.episode == episode.episode
                                    && known.rel_path == episode.rel_path)
                        }) {
                            existing.episodes.push(episode);
                        }
                    }
                    existing
                        .episodes
                        .sort_by(|a, b| a.episode.cmp(&b.episode).then_with(|| a.id.cmp(&b.id)));
                    if existing.name.is_none() {
                        existing.name = season.name;
                    }
                    if existing.overview.is_none() {
                        existing.overview = season.overview;
                    }
                    if existing.poster_url.is_none() {
                        existing.poster_url = season.poster_url;
                    }
                }
                None => detail.seasons.push(season),
            }
        }
    }
    detail.seasons.sort_by_key(|season| season.number);
    detail.show = canonical;
    detail.show.episode_count = detail
        .seasons
        .iter()
        .map(|season| season.episodes.len() as u32)
        .sum();
    detail.show.season_count = detail.seasons.len() as u32;
    Ok(Some((detail, group)))
}

fn season_name(locale: &str, number: u32) -> String {
    crate::i18n::t(locale, "content.season", &[("number", &number.to_string())])
}

pub fn routes() -> Router<SharedState> {
    Router::new()
        .route("/System/Info/Public", get(info_public))
        .route("/System/Info", get(info_public))
        .route("/System/Ping", get(ping).post(ping))
        .route("/System/Endpoint", get(endpoint))
        .route("/Branding/Configuration", get(branding))
        .route("/Users/Public", get(users_public))
        .route("/Users/AuthenticateByName", post(authenticate))
        .route("/Users/Me", get(me))
        .route("/Users/{user_id}", get(me))
        .route("/Users/{user_id}/Views", get(views))
        .route("/Users/{user_id}/GroupingOptions", get(grouping_options))
        .route("/Users/{user_id}/Items", get(items))
        .route("/Users/{user_id}/Items/Latest", get(latest))
        .route("/Users/{user_id}/Items/Resume", get(resume_items))
        .route("/Users/{user_id}/Items/Root", get(items_root))
        .route(
            "/Users/{user_id}/PlayedItems/{id}",
            post(mark_played).delete(unmark_played),
        )
        .route(
            "/Users/{user_id}/FavoriteItems/{id}",
            post(mark_favorite).delete(unmark_favorite),
        )
        // Legacy spellings older Emby-shaped clients still call.
        .route(
            "/UserPlayedItems/{id}",
            post(mark_played).delete(unmark_played),
        )
        .route(
            "/UserFavoriteItems/{id}",
            post(mark_favorite).delete(unmark_favorite),
        )
        .route("/UserItems/{id}/UserData", get(user_item_data))
        .route("/Users/{user_id}/Items/{id}", get(item_detail))
        .route("/Users/{user_id}/Items/{id}/Similar", get(catalog::similar))
        .route("/UserViews", get(user_views))
        .route("/UserViews/GroupingOptions", get(grouping_options))
        .route("/Items", get(items))
        .route("/Items/Latest", get(latest))
        .route("/Items/Resume", get(resume_items))
        .route("/Items/Root", get(items_root))
        .route("/UserItems/Resume", get(resume_items))
        .route("/Items/{id}", get(item_detail))
        .route("/Items/{id}/Similar", get(catalog::similar))
        .route("/Items/Counts", get(item_counts))
        .route("/Items/Filters", get(catalog::filters))
        .route("/Items/Filters2", get(catalog::filters2))
        .route("/Items/{id}/Images", get(image_list))
        .route("/Items/{id}/SpecialFeatures", get(empty_items))
        .route("/Items/{id}/LocalTrailers", get(empty_items))
        .route("/Items/{id}/Intros", get(empty_items))
        .route("/Items/{id}/Ancestors", get(ancestors))
        .route("/Videos/{id}/AdditionalParts", get(empty_items))
        .route("/Items/{id}/ThemeMedia", get(theme_media))
        .route("/Genres", get(catalog::genres))
        .route("/Studios", get(catalog::studios))
        .route("/Persons", get(catalog::persons))
        .route("/Persons/{name}", get(catalog::person_detail))
        .route("/Persons/{name}/Images/{kind}", get(catalog::person_image))
        .route("/Search/Hints", get(search_hints))
        .route("/Sessions", get(empty_sessions))
        .route("/Library/VirtualFolders", get(virtual_folders))
        .route(
            "/Notifications/{user_id}/Summary",
            get(notifications_summary),
        )
        .route("/Shows/{id}/Seasons", get(seasons))
        .route("/Shows/{id}/Episodes", get(episodes))
        .route("/Shows/{id}/Similar", get(catalog::similar))
        .route("/Items/{id}/PlaybackInfo", post(playback_info))
        .route("/Items/{id}/PlaybackInfo", get(playback_info))
        .route("/Items/{id}/Images/{kind}", get(image))
        .route("/Items/{id}/Images/{kind}/{index}", get(image_index))
        .route("/Videos/{id}/stream", get(stream))
        .route("/Videos/{id}/stream.{ext}", get(stream))
        .route("/Sessions/Playing", post(playback_progress))
        .route("/Sessions/Playing/Progress", post(playback_progress))
        .route("/Sessions/Playing/Stopped", post(playback_progress))
        .route("/Sessions/Capabilities", post(no_content))
        .route("/Sessions/Capabilities/Full", post(no_content))
        .route("/QuickConnect/Enabled", get(quick_connect_enabled))
        .route("/DisplayPreferences/{id}", get(display_preferences))
        .route("/Shows/NextUp", get(empty_items))
        .route(
            "/Shows/{id}/Episodes/{episode_id}/Similar",
            get(empty_items),
        )
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct LoginBody {
    username: String,
    pw: String,
}

fn token_of(headers: &HeaderMap, query: &HashMap<String, String>) -> Option<String> {
    for name in ["x-emby-token", "x-mediabrowser-token"] {
        if let Some(value) = headers.get(name).and_then(|v| v.to_str().ok()) {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    for name in ["authorization", "x-emby-authorization"] {
        if let Some(value) = headers.get(name).and_then(|v| v.to_str().ok()) {
            let value = value.trim();
            let rest = value
                .strip_prefix("MediaBrowser ")
                .or_else(|| value.strip_prefix("Bearer "))
                .unwrap_or(value);
            let token = rest
                .split(',')
                .find_map(|part| part.trim().strip_prefix("Token="))
                .map(|t| t.trim_matches('"').to_string())
                .or_else(|| {
                    rest.starts_with('"')
                        .then(|| rest.trim_matches('"').to_string())
                })
                .unwrap_or_else(|| rest.to_string());
            if !token.is_empty() && !token.contains(' ') {
                return Some(token);
            }
        }
    }
    query
        .get("api_key")
        .or_else(|| query.get("ApiKey"))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

async fn user_of(
    state: &SharedState,
    headers: &HeaderMap,
    query_params: &HashMap<String, String>,
) -> Result<(User, String), Response> {
    let token = token_of(headers, query_params)
        .ok_or_else(|| json_error(StatusCode::UNAUTHORIZED, "missing Jellyfin token"))?;
    let key = token.clone();
    let found = query(&state.db, move |pool| db::access_token_user(&pool, &key))
        .await?
        .ok_or_else(|| json_error(StatusCode::UNAUTHORIZED, "unknown or expired token"))?;
    Ok((found.0, token))
}

fn server_id(state: &SharedState) -> String {
    state.instance_id.clone()
}

fn user_dto(state: &SharedState, user: &User) -> Value {
    json!({
        "Name": user.username,
        "ServerId": server_id(state),
        "Id": user.id,
        "HasPassword": true,
        "HasConfiguredPassword": true,
        "PrimaryImageTag": Value::Null,
        "Configuration": { "PlayDefaultAudioTrack": true, "DisplayMissingEpisodes": false },
        "Policy": {
            "IsAdministrator": user.can(crate::model::Permission::SettingsManage),
            "IsHidden": false,
            "EnableAllFolders": true,
            "EnabledFolders": [],
        },
    })
}

async fn info_public(State(state): State<SharedState>) -> Json<Value> {
    Json(json!({
        "ServerName": "KROMA",
        "Version": SERVER_VERSION,
        "ProductName": "Jellyfin Server",
        "Id": server_id(&state),
        "OperatingSystem": "",
        "StartupWizardCompleted": true,
        "LocalAddress": "",
    }))
}

async fn branding() -> Json<Value> {
    Json(json!({ "LoginDisclaimer": "", "SplashscreenEnabled": false, "CustomCss": "" }))
}

async fn authenticate(State(state): State<SharedState>, Json(body): Json<LoginBody>) -> Response {
    let identifier = body.username.trim().to_string();
    let password = body.pw;
    let found = match query(&state.db, move |pool| {
        db::find_user_by_login(&pool, &identifier)
    })
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some((user, hash)) = found else {
        return json_error(StatusCode::UNAUTHORIZED, "invalid username or password");
    };
    if !crate::services::auth::verify_password(&password, &hash) {
        return json_error(StatusCode::UNAUTHORIZED, "invalid username or password");
    }
    let minted =
        crate::api::accounts::mint_device_tokens(&state, &user.id, db::DeviceHints::default())
            .await;
    match minted {
        Ok((_session, access)) => Json(json!({
            "User": user_dto(&state, &user),
            "SessionInfo": { "Id": access, "UserId": user.id, "UserName": user.username },
            "AccessToken": access,
            "ServerId": server_id(&state),
        }))
        .into_response(),
        Err(response) => response,
    }
}

async fn me(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, Response> {
    let (user, _) = user_of(&state, &headers, &params).await?;
    Ok(Json(user_dto(&state, &user)))
}

fn library_type(def: &crate::services::settings::LibraryDef) -> &'static str {
    if def.kind.contains("series") || def.kind.contains("show") || def.kind.contains("tv") {
        "tvshows"
    } else {
        "movies"
    }
}

async fn views(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    Path(_user_id): Path<String>,
) -> Result<Json<Value>, Response> {
    user_of(&state, &headers, &params).await?;
    Ok(Json(views_payload(&state).await?))
}

async fn user_views(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, Response> {
    user_of(&state, &headers, &params).await?;
    Ok(Json(views_payload(&state).await?))
}

async fn views_payload(state: &SharedState) -> Result<Value, Response> {
    let defs =
        crate::services::settings::library_defs(&state.settings, &state.config).unwrap_or_default();
    let items = query(&state.db, |pool| db::list_items(&pool, None)).await?;
    let covers = query(&state.db, |pool| {
        db::art_overrides::all(&pool, db::art_overrides::LIBRARY_KIND)
    })
    .await?;
    let items: Vec<Value> = defs
        .iter()
        .map(|def| {
            let picked = covers.get(&def.id);
            let cover = items
                .iter()
                .find(|item| item.library == def.name || item.library == def.id)
                .and_then(|item| item.metadata.as_ref());
            // A library has one cover: whichever side of the picker filled it.
            let poster = picked
                .and_then(|art| art.get("poster").or_else(|| art.get("backdrop")).cloned())
                .or_else(|| cover.and_then(|meta| meta.poster_url.clone()));
            let backdrop = picked
                .and_then(|art| art.get("backdrop").cloned())
                .or_else(|| cover.and_then(|meta| meta.backdrop_url.clone()));
            let tag = poster.as_deref().map(library_image_tag_value);
            json!({
                "Name": def.name,
                "Id": def.id,
                "Type": "CollectionFolder",
                "CollectionType": library_type(def),
                "IsFolder": true,
                "ServerId": server_id(state),
                "ImageTags": if tag.is_some() { json!({ "Primary": tag }) } else { json!({}) },
                "PrimaryImageTag": tag,
                "BackdropImageTags": if backdrop.is_some() { json!(["1"]) } else { json!([]) },
            })
        })
        .collect();
    Ok(json!({ "Items": items, "TotalRecordCount": items.len() }))
}

/// One title, by the item id a list handed out. Jellyfin clients address this
/// for the detail page, under both `/Items/{id}` and the per-user spelling.
async fn item_detail(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    Path(path): Path<HashMap<String, String>>,
) -> Result<Json<Value>, Response> {
    let (user, access) = user_of(&state, &headers, &params).await?;
    let id = path.get("id").cloned().unwrap_or_default();
    if id.is_empty() {
        return Err(json_error(StatusCode::NOT_FOUND, "item not found"));
    }
    let defs =
        crate::services::settings::library_defs(&state.settings, &state.config).unwrap_or_default();
    if let Some(def) = defs.iter().find(|def| def.id == id || def.name == id) {
        let items = query(&state.db, |pool| db::list_items(&pool, None)).await?;
        let picked = query(&state.db, {
            let id = def.id.clone();
            move |pool| db::art_overrides::get(&pool, db::art_overrides::LIBRARY_KIND, &id)
        })
        .await?;
        let cover = items
            .iter()
            .find(|item| item.library == def.name || item.library == def.id)
            .and_then(|item| item.metadata.as_ref());
        let poster = picked
            .get("poster")
            .or_else(|| picked.get("backdrop"))
            .cloned()
            .or_else(|| cover.and_then(|meta| meta.poster_url.clone()));
        let backdrop = picked
            .get("backdrop")
            .cloned()
            .or_else(|| cover.and_then(|meta| meta.backdrop_url.clone()));
        let tag = poster.as_deref().map(library_image_tag_value);
        return Ok(Json(json!({
            "Name": def.name,
            "Id": def.id,
            "ServerId": server_id(&state),
            "Type": "CollectionFolder",
            "CollectionType": library_type(def),
            "IsFolder": true,
            "ChildCount": 0,
            "ImageTags": if tag.is_some() { json!({ "Primary": tag }) } else { json!({}) },
            "PrimaryImageTag": tag,
            "BackdropImageTags": if backdrop.is_some() { json!(["1"]) } else { json!([]) },
        })));
    }
    // A season id ("<show>:s1") is a real address in this tree: the season
    // page asks for it directly.
    if let Some((show_id, number)) = id.split_once(":s") {
        if let Ok(number) = number.parse::<u32>() {
            crate::api::visibility::gate_show(&state, &user, show_id).await?;
            let locale = metadata_locale(&state, &headers, &params);
            if let Some(detail) = localized_show(&state, show_id.to_string(), locale).await? {
                if let Some(season) = detail.seasons.iter().find(|season| season.number == number) {
                    return Ok(Json(
                        with_user_data(
                            &state,
                            &user,
                            season_dto(&state, &detail.show, season, locale),
                        )
                        .await?,
                    ));
                }
                return Err(json_error(StatusCode::NOT_FOUND, "season not found"));
            }
        }
    }
    if id.starts_with("person:") {
        let locale = metadata_locale(&state, &headers, &params);
        return Ok(Json(
            catalog::person_value_with_locale(&state, &user, &id, locale).await?,
        ));
    }
    let locale = metadata_locale(&state, &headers, &params);
    if let Some((detail, _)) = merged_detail(&state, &user, locale, &id).await? {
        crate::api::visibility::gate_show(&state, &user, &detail.show.id).await?;
        return Ok(Json(
            with_user_data(&state, &user, series_detail_dto(&state, &detail.show)).await?,
        ));
    }
    let item = query(&state.db, move |pool| db::get_item(&pool, &id))
        .await?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "item not found"))?;
    crate::api::visibility::gate_item(&state, &user, &item.id).await?;
    if let Some(show_id) = item.show_id.clone() {
        let show = query(&state.db, move |pool| db::get_show(&pool, &show_id)).await?;
        let dto = episode_detail_dto(
            &state,
            &headers,
            &item,
            show.as_ref().map(|d| &d.show),
            &access,
        );
        return Ok(Json(with_user_data(&state, &user, dto).await?));
    }
    Ok(Json(
        with_user_data(
            &state,
            &user,
            movie_detail_dto(&state, &headers, &item, &access),
        )
        .await?,
    ))
}

async fn item_counts(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, Response> {
    let (user, _) = user_of(&state, &headers, &params).await?;
    let mut items = query(&state.db, |pool| db::list_items(&pool, None)).await?;
    let mut shows = query(&state.db, |pool| db::list_shows(&pool, None)).await?;
    crate::api::visibility::keep_items(&user, &mut items);
    crate::api::visibility::keep_shows(&user, &mut shows);
    let movie_count = items
        .iter()
        .filter(|item| matches!(item.kind, Kind::Movie | Kind::Video) && item.show_id.is_none())
        .count();
    let episode_count = items.iter().filter(|item| item.show_id.is_some()).count();
    let series_count = shows.len();
    Ok(Json(json!({
        "MovieCount": movie_count,
        "SeriesCount": series_count,
        "EpisodeCount": episode_count,
        "ArtistCount": 0,
        "ProgramCount": 0,
        "TrailerCount": 0,
        "SongCount": 0,
        "AlbumCount": 0,
        "MusicVideoCount": 0,
        "BoxSetCount": 0,
        "BookCount": 0,
        "ItemCount": movie_count + series_count + episode_count,
    })))
}

async fn notifications_summary(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    Path(_user_id): Path<String>,
) -> Result<Json<Value>, Response> {
    user_of(&state, &headers, &params).await?;
    Ok(Json(json!({
        "UnreadCount": 0,
        "MaxUnreadCount": 0,
        "UnreadCountsByType": {},
        "UnreadCountsByLevel": {},
    })))
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct JellyfinProgressBody {
    item_id: String,
    position_ticks: i64,
}

async fn playback_progress(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    Json(body): Json<JellyfinProgressBody>,
) -> Result<StatusCode, Response> {
    let (user, _) = user_of(&state, &headers, &params).await?;
    if body.item_id.is_empty() {
        return Ok(StatusCode::NO_CONTENT);
    }
    crate::api::visibility::gate_item(&state, &user, &body.item_id).await?;
    let item = query(&state.db, {
        let id = body.item_id.clone();
        move |pool| db::get_item(&pool, &id)
    })
    .await?;
    let position_ms = (body.position_ticks / 10_000).max(0);
    let played_percent = crate::services::settings::played_percent(&state.settings);
    let uid = user.id.clone();
    let item_id = body.item_id;
    let duration = item
        .as_ref()
        .and_then(|item| item.duration_ms.map(|ms| ms as i64));
    query(&state.db, move |pool| {
        db::save_progress(&pool, &uid, &item_id, position_ms, duration, played_percent)
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn mark_played(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    Path(path): Path<HashMap<String, String>>,
) -> Result<StatusCode, Response> {
    let (user, _) = user_of(&state, &headers, &params).await?;
    let id = path.get("id").cloned().unwrap_or_default();
    crate::api::visibility::gate_item(&state, &user, &id).await?;
    let uid = user.id.clone();
    query(&state.db, move |pool| db::mark_watched(&pool, &uid, &id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn unmark_played(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    Path(path): Path<HashMap<String, String>>,
) -> Result<StatusCode, Response> {
    let (user, _) = user_of(&state, &headers, &params).await?;
    let id = path.get("id").cloned().unwrap_or_default();
    crate::api::visibility::gate_item(&state, &user, &id).await?;
    let uid = user.id.clone();
    query(&state.db, {
        let uid = uid.clone();
        let id = id.clone();
        move |pool| db::unmark_watched(&pool, &uid, &id)
    })
    .await?;
    // Jellyfin's "mark unplayed" also clears the resume point.
    query(&state.db, move |pool| db::delete_progress(&pool, &uid, &id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn mark_favorite(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    Path(path): Path<HashMap<String, String>>,
) -> Result<StatusCode, Response> {
    let (user, _) = user_of(&state, &headers, &params).await?;
    let id = path.get("id").cloned().unwrap_or_default();
    crate::api::visibility::gate_item(&state, &user, &id).await?;
    let uid = user.id.clone();
    query(&state.db, move |pool| db::add_to_list(&pool, &uid, &id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn unmark_favorite(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    Path(path): Path<HashMap<String, String>>,
) -> Result<StatusCode, Response> {
    let (user, _) = user_of(&state, &headers, &params).await?;
    let id = path.get("id").cloned().unwrap_or_default();
    crate::api::visibility::gate_item(&state, &user, &id).await?;
    let uid = user.id.clone();
    query(&state.db, move |pool| {
        db::remove_from_list(&pool, &uid, &id)
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn apply_user_data(
    state: &SharedState,
    user: &User,
    items: &mut [Value],
) -> Result<(), Response> {
    if items.is_empty() {
        return Ok(());
    }
    let uid = user.id.clone();
    let watched: HashSet<String> = query(&state.db, {
        let uid = uid.clone();
        move |pool| db::list_watched(&pool, &uid)
    })
    .await?
    .into_iter()
    .collect();
    let favorites: HashSet<String> = query(&state.db, {
        let uid = uid.clone();
        move |pool| db::list_my_list(&pool, &uid)
    })
    .await?
    .into_iter()
    .collect();
    let progress: HashMap<String, (i64, Option<i64>)> = query(&state.db, {
        let uid = uid.clone();
        move |pool| db::list_progress(&pool, &uid)
    })
    .await?
    .into_iter()
    .map(|entry| (entry.item_id, (entry.position_ms, entry.duration_ms)))
    .collect();
    let folders = items
        .iter()
        .any(|item| matches!(item["Type"].as_str(), Some("Series" | "Season")));
    let descendants = if folders {
        let mut all = query(&state.db, |pool| db::list_items(&pool, None)).await?;
        crate::api::visibility::keep_items(user, &mut all);
        all
    } else {
        Vec::new()
    };
    let series = if folders {
        let mut shows = query(&state.db, |pool| db::list_shows(&pool, None)).await?;
        crate::api::visibility::keep_shows(user, &mut shows);
        shows
    } else {
        Vec::new()
    };
    for item in items.iter_mut() {
        let Some(id) = item.get("Id").and_then(Value::as_str).map(str::to_string) else {
            continue;
        };
        let played = watched.contains(&id);
        let (position_ms, duration_ms) = progress.get(&id).copied().unwrap_or((0, None));
        let mut user_data = json!({
            "Played": played,
            "PlaybackPositionTicks": position_ms * 10_000,
            "PlayCount": if played { 1 } else { 0 },
            "IsFavorite": favorites.contains(&id),
        });
        if let Some(duration_ms) = duration_ms.filter(|duration| *duration > 0) {
            if position_ms > 0 {
                user_data["PlayedPercentage"] =
                    json!(position_ms as f64 / duration_ms as f64 * 100.0);
            }
        }
        if matches!(item["Type"].as_str(), Some("Series" | "Season")) {
            let (show_id, season) = if item["Type"] == "Season" {
                (
                    item["SeriesId"].as_str().unwrap_or(""),
                    item["IndexNumber"].as_u64(),
                )
            } else {
                (id.as_str(), None)
            };
            // One series may currently live in several rows that matched the
            // same provider show: their episodes belong to the one card.
            let members: HashSet<&str> = series
                .iter()
                .find(|show| show.id == show_id)
                .map(|target| {
                    series
                        .iter()
                        .filter(|show| same_series(show, target))
                        .map(|show| show.id.as_str())
                        .collect()
                })
                .unwrap_or_else(|| std::iter::once(show_id).collect());
            let episodes: Vec<_> = descendants
                .iter()
                .filter(|episode| {
                    episode
                        .show_id
                        .as_deref()
                        .is_some_and(|id| members.contains(id))
                        && season.is_none_or(|number| episode.season.map(u64::from) == Some(number))
                })
                .collect();
            let count = episodes.len();
            let seen = episodes
                .iter()
                .filter(|episode| watched.contains(&episode.id))
                .count();
            user_data["Played"] = json!(count > 0 && seen == count);
            user_data["UnplayedItemCount"] = json!(count - seen);
            user_data["PlayedPercentage"] = json!(if count == 0 {
                0.0
            } else {
                seen as f64 * 100.0 / count as f64
            });
            item["RecursiveItemCount"] = json!(count);
            item["ChildCount"] = json!(if season.is_some() {
                count
            } else {
                episodes
                    .iter()
                    .filter_map(|episode| episode.season)
                    .collect::<HashSet<_>>()
                    .len()
            });
        }
        item["UserData"] = user_data;
    }
    Ok(())
}

async fn with_user_data(state: &SharedState, user: &User, item: Value) -> Result<Value, Response> {
    let mut items = vec![item];
    apply_user_data(state, user, &mut items).await?;
    items
        .pop()
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "item not found"))
}

async fn resume_items(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    Path(_path): Path<HashMap<String, String>>,
) -> Result<Json<Value>, Response> {
    let (user, access) = user_of(&state, &headers, &params).await?;
    let played_percent = crate::services::settings::played_percent(&state.settings);
    let locale = metadata_locale(&state, &headers, &params);
    let mut shows = localized_shows(&state, locale).await?;
    crate::api::visibility::keep_shows(&user, &mut shows);
    let canonical_shows = dedupe_series(shows.clone());
    let show_for: HashMap<String, &Show> = shows
        .iter()
        .map(|show| {
            let id = canonical_shows
                .iter()
                .find(|other| same_series(other, show))
                .map(|other| other.id.clone())
                .unwrap_or_else(|| show.id.clone());
            (
                show.id.clone(),
                canonical_shows
                    .iter()
                    .find(|other| other.id == id)
                    .unwrap_or(show),
            )
        })
        .collect();
    let uid = user.id.clone();
    let rows = query(&state.db, move |pool| {
        db::continue_watching(&pool, &uid, played_percent)
    })
    .await?;
    let mut items: Vec<Value> = rows
        .iter()
        .filter(|row| match row.item.show_id.as_deref() {
            Some(show_id) => show_for
                .get(show_id)
                .is_some_and(|show| user.sees_library(&show.library)),
            None => user.sees_library(&row.item.library),
        })
        .map(|row| {
            match row
                .item
                .show_id
                .as_deref()
                .and_then(|id| show_for.get(id).copied())
            {
                Some(show) => episode_detail_dto(&state, &headers, &row.item, Some(show), &access),
                None => movie_dto(&state, &row.item),
            }
        })
        .collect();
    apply_user_data(&state, &user, &mut items).await?;
    Ok(Json(json!({
        "Items": items,
        "TotalRecordCount": items.len(),
    })))
}

fn ticks(ms: Option<u64>) -> i64 {
    ms.map(|ms| (ms as i64) * 10_000).unwrap_or(0)
}

fn image_tag(art: Option<&String>) -> Value {
    match art {
        // The tag follows the image: an episode whose still changes (or a
        // re-scraped poster) must not keep serving the old cached bytes.
        Some(url) => json!({ "Primary": library_image_tag_value(url) }),
        None => json!({}),
    }
}

/// A library's cover tag is derived from the cover URL: swapping the cover
/// swaps the tag, so a client holding the old image refetches instead of
/// serving a stale cache hit under the constant tag titles use.
fn library_image_tag_value(url: &str) -> String {
    crate::services::scan::short_hash(url)
}

/// Every backdrop a title advertises: the primary hero image first, then the
/// extra gallery stored with the metadata, without repeating one image.
fn backdrop_urls(meta: Option<&crate::model::Metadata>) -> Vec<String> {
    let Some(meta) = meta else {
        return Vec::new();
    };
    let mut urls: Vec<String> = meta.backdrop_url.iter().cloned().collect();
    for url in &meta.backdrops {
        if urls.len() >= 4 {
            break;
        }
        if !urls.iter().any(|seen| seen == url) {
            urls.push(url.clone());
        }
    }
    urls
}

fn tagged_backdrop<'a>(meta: &'a crate::model::Metadata, tag: &str) -> Option<&'a String> {
    meta.backdrop_url
        .iter()
        .chain(meta.backdrops.iter())
        .find(|url| library_image_tag_value(url) == tag)
}

/// Jellyfin expects one tag per backdrop, index-aligned to the images it serves.
fn backdrop_tags(urls: &[String]) -> Value {
    Value::Array(
        urls.iter()
            .map(|url| json!(library_image_tag_value(url)))
            .collect(),
    )
}

/// The backdrop at `index` of an item's gallery: 0 is the primary art the DTO
/// leads with, later indices walk `metadata.backdrops`. Out of range reads as
/// the primary image, so an old client asking for a slide that was re-scraped
/// away still gets art instead of a 404.
fn backdrop_at(meta: &crate::model::Metadata, index: usize) -> Option<String> {
    match index {
        0 => meta.backdrop_url.clone(),
        n => meta
            .backdrops
            .get(n - 1)
            .cloned()
            .or_else(|| meta.backdrop_url.clone()),
    }
}

fn movie_dto(state: &SharedState, item: &MediaItem) -> Value {
    let poster = item.metadata.as_ref().and_then(|m| m.poster_url.clone());
    let backdrops = backdrop_urls(item.metadata.as_ref());
    json!({
        "Name": item.title,
        "Id": item.id,
        "ServerId": server_id(state),
        "Type": "Movie",
        "MediaType": "Video",
        "IsFolder": false,
        "ProductionYear": item.year,
        "RunTimeTicks": ticks(item.duration_ms),
        "Container": if item.container == "strm" { "mkv" } else { item.container.as_str() },
        "Overview": item.metadata.as_ref().and_then(|m| m.overview.clone()),
        "CommunityRating": item.metadata.as_ref().and_then(|m| m.rating),
        "OfficialRating": item.metadata.as_ref().and_then(|m| m.certification.clone()),
        "Genres": item.metadata.as_ref().map(|m| localized_genres(&m.genres)).unwrap_or_default(),
        "DateCreated": item.added_at,
        "ImageTags": image_tag(poster.as_ref()),
        "BackdropImageTags": backdrop_tags(&backdrops),
        "UserData": { "Played": false, "PlaybackPositionTicks": 0, "PlayCount": 0, "IsFavorite": false },
    })
}

fn series_dto(state: &SharedState, show: &Show) -> Value {
    let poster = show.metadata.as_ref().and_then(|m| m.poster_url.clone());
    let backdrops = backdrop_urls(show.metadata.as_ref());
    json!({
        "Name": show.title,
        "Id": show.id,
        "ServerId": server_id(state),
        "Type": "Series",
        "MediaType": "Unknown",
        "IsFolder": true,
        "ProductionYear": show.year,
        "Overview": show.metadata.as_ref().and_then(|m| m.overview.clone()),
        "CommunityRating": show.metadata.as_ref().and_then(|m| m.rating),
        "OfficialRating": show.metadata.as_ref().and_then(|m| m.certification.clone()),
        "Genres": show.metadata.as_ref().map(|m| localized_genres(&m.genres)).unwrap_or_default(),
        "DateCreated": show.added_at,
        "ImageTags": image_tag(poster.as_ref()),
        "BackdropImageTags": backdrop_tags(&backdrops),
        "ChildCount": show.episode_count,
        "RecursiveItemCount": show.episode_count,
        "UserData": { "Played": false, "PlaybackPositionTicks": 0, "PlayCount": 0, "IsFavorite": false },
    })
}

fn studios_of(meta: &crate::model::Metadata) -> Value {
    Value::Array(
        meta.studios
            .iter()
            .map(|name| json!({ "Name": name, "Id": name }))
            .collect(),
    )
}

fn provider_ids(meta: Option<&crate::model::Metadata>) -> Value {
    let Some(meta) = meta else {
        return json!({});
    };
    let mut ids = serde_json::Map::new();
    ids.insert("Tmdb".to_string(), json!(meta.tmdb_id.to_string()));
    if let Some(imdb) = meta.imdb_id.as_ref() {
        ids.insert("Imdb".to_string(), json!(imdb));
    }
    Value::Object(ids)
}

/// TMDB's zh-Hans catalog leaves a few combined genres in English; the card
/// row should read as one language.
fn localized_genres(genres: &[String]) -> Vec<String> {
    genres
        .iter()
        .map(|genre| match genre.as_str() {
            "Sci-Fi & Fantasy" => "科幻奇幻",
            "Action & Adventure" => "动作冒险",
            "War & Politics" => "战争与政治",
            "Animation" => "动画",
            "Comedy" => "喜剧",
            "Drama" => "剧情",
            "Mystery" => "悬疑",
            "Romance" => "爱情",
            other => other,
        })
        .map(str::to_string)
        .collect()
}

fn reduced_ratio(width: u32, height: u32) -> Option<String> {
    fn gcd(a: u32, b: u32) -> u32 {
        if b == 0 {
            a
        } else {
            gcd(b, a % b)
        }
    }
    (width > 0 && height > 0).then(|| {
        let divisor = gcd(width, height).max(1);
        format!("{}:{}", width / divisor, height / divisor)
    })
}

fn media_streams(item: &MediaItem) -> Value {
    let mut streams = Vec::new();
    if let Some(video) = item.video.as_ref() {
        let technical = video.technical.as_ref();
        let aspect = technical.and_then(|t| t.aspect_ratio.clone()).or_else(|| {
            video
                .width
                .zip(video.height)
                .and_then(|(w, h)| reduced_ratio(w, h))
        });
        streams.push(json!({
            "Type": "Video",
            "Index": technical.and_then(|t| t.stream_index).unwrap_or(0),
            "Codec": video.codec,
            "Width": video.width,
            "Height": video.height,
            "BitDepth": video.bit_depth,
            "VideoRange": if video.hdr { "HDR" } else { "SDR" },
            "Profile": technical.and_then(|t| t.profile.clone()),
            "Level": technical.and_then(|t| t.level),
            "AspectRatio": aspect,
            "RealFrameRate": technical.and_then(|t| t.frame_rate),
            "BitRate": technical.and_then(|t| t.bit_rate),
            "PixelFormat": technical.and_then(|t| t.pixel_format.clone()),
            "ColorPrimaries": video.color.as_ref().and_then(|c| c.primaries.clone()),
            "ColorTransfer": video.color.as_ref().and_then(|c| c.transfer.clone()),
            "ColorSpace": video.color.as_ref().and_then(|c| c.matrix.clone()),
            "IsDefault": true,
        }));
    }
    // A file flag wins. Without one, the client should not be invited to start
    // with a commentary/Dolby track when a standard AAC mix is present: Jellyfin
    // clients pick the compatible stereo mix, so say which one that is.
    let default_audio = item
        .audio_tracks
        .iter()
        .position(|audio| audio.default)
        .or_else(|| {
            let standard = |audio: &crate::model::AudioStream| {
                audio.codec.eq_ignore_ascii_case("aac")
                    && audio.title.as_deref().is_some_and(|title| {
                        title.contains("标准") || title.to_lowercase().contains("standard")
                    })
            };
            item.audio_tracks
                .iter()
                .position(standard)
                .or_else(|| {
                    item.audio_tracks
                        .iter()
                        .position(|audio| audio.codec.eq_ignore_ascii_case("aac"))
                })
                .or((!item.audio_tracks.is_empty()).then_some(0))
        });
    for (i, audio) in item.audio_tracks.iter().enumerate() {
        let technical = audio.technical.as_ref();
        streams.push(json!({
            "Type": "Audio",
            "Index": technical.and_then(|t| t.stream_index).unwrap_or(i as u32 + 1),
            "Codec": audio.codec,
            "Channels": audio.channels,
            "Language": audio.language.clone().or_else(|| Some("und".to_string())),
            "Title": audio.title,
            "DisplayTitle": audio.title,
            "Profile": technical.and_then(|t| t.profile.clone()),
            "SampleRate": technical.and_then(|t| t.sample_rate),
            "BitRate": technical.and_then(|t| t.bit_rate),
            "ChannelLayout": technical.and_then(|t| t.channel_layout.clone()),
            "IsDefault": default_audio == Some(i),
        }));
    }
    for (i, subtitle) in item.subtitles.iter().enumerate() {
        streams.push(json!({
            "Type": "Subtitle",
            "Index": 1000 + i,
            "Codec": subtitle.codec,
            "Language": subtitle.language,
            "DisplayTitle": subtitle.title,
            "IsForced": subtitle.forced,
        }));
    }
    json!(streams)
}

fn source_size(item: &MediaItem) -> Option<u64> {
    item.files
        .iter()
        .find(|file| item.default_file_id.as_deref() == Some(file.id.as_str()))
        .or_else(|| item.files.first())
        .and_then(|file| file.size)
}

fn media_source(state: &SharedState, headers: &HeaderMap, item: &MediaItem, access: &str) -> Value {
    let ticket = crate::services::media_ticket::for_access_token(&state.media_ticket_key, access);
    // Never put the drive's signed URL in a DTO: media info would show the
    // module/115 address. The server stream is the same bytes and the module
    // sign is logged when the redirect is served.
    let path = format!("/api/items/{}/stream?t={ticket}", item.id);
    let name = item
        .rel_path
        .as_deref()
        .and_then(|path| std::path::Path::new(path).file_stem())
        .and_then(|stem| stem.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| item.title.clone());
    let size = source_size(item);
    let bitrate = size
        .zip(item.duration_ms)
        .filter(|(_, ms)| *ms > 0)
        .map(|(bytes, ms)| bytes.saturating_mul(8000) / ms);
    json!({
        "Id": item.id,
        "Name": name,
        "Path": client_url(headers, &path),
        "Container": if item.container == "strm" { "mkv" } else { item.container.as_str() },
        "RunTimeTicks": ticks(item.duration_ms),
        "Size": size,
        "Bitrate": bitrate,
        "SupportsDirectPlay": true,
        "SupportsDirectStream": true,
        "SupportsTranscoding": false,
        "IsRemote": false,
        "MediaStreams": media_streams(item),
    })
}

fn movie_detail_dto(
    state: &SharedState,
    headers: &HeaderMap,
    item: &MediaItem,
    access: &str,
) -> Value {
    let mut dto = movie_dto(state, item);
    if let Some(meta) = item.metadata.as_ref() {
        dto["Genres"] = json!(localized_genres(&meta.genres));
        if let Some(tagline) = meta.tagline.as_ref() {
            dto["Taglines"] = json!([tagline]);
        }
        if let Some(certification) = meta.certification.as_ref() {
            dto["OfficialRating"] = json!(certification);
        }
        if let Some(release_date) = meta.release_date.as_ref() {
            dto["PremiereDate"] = json!(release_date);
        }
        dto["ProviderIds"] = provider_ids(Some(meta));
        dto["People"] = catalog::people(Some(meta));
        dto["Studios"] = studios_of(meta);
    }
    dto["DateCreated"] = json!(item.added_at);
    dto["CanDownload"] = json!(true);
    dto["LocationType"] = json!("FileSystem");
    dto["VideoType"] = json!("VideoFile");
    dto["MediaSources"] = json!([media_source(state, headers, item, access)]);
    dto
}

fn series_detail_dto(state: &SharedState, show: &Show) -> Value {
    let mut dto = series_dto(state, show);
    if let Some(meta) = show.metadata.as_ref() {
        dto["Genres"] = json!(localized_genres(&meta.genres));
        if let Some(tagline) = meta.tagline.as_ref() {
            dto["Taglines"] = json!([tagline]);
        }
        if let Some(certification) = meta.certification.as_ref() {
            dto["OfficialRating"] = json!(certification);
        }
        if let Some(release_date) = meta.release_date.as_ref() {
            dto["PremiereDate"] = json!(release_date);
        }
        dto["ProviderIds"] = provider_ids(Some(meta));
        dto["People"] = catalog::people(Some(meta));
        dto["Studios"] = studios_of(meta);
    }
    dto["DateCreated"] = json!(show.added_at);
    dto["CanDownload"] = json!(false);
    dto["LocationType"] = json!("FileSystem");
    dto
}

fn episode_detail_dto(
    state: &SharedState,
    headers: &HeaderMap,
    item: &MediaItem,
    show: Option<&Show>,
    access: &str,
) -> Value {
    let metadata = item.metadata.as_ref();
    let show_metadata = show.and_then(|show| show.metadata.as_ref());
    let poster = metadata
        .and_then(|meta| {
            meta.backdrop_url
                .clone()
                .or_else(|| meta.poster_url.clone())
        })
        .or_else(|| {
            item.metadata
                .as_ref()
                .and_then(|meta| meta.poster_url.clone())
        })
        .or_else(|| {
            show_metadata.and_then(|meta| {
                meta.backdrop_url
                    .clone()
                    .or_else(|| meta.poster_url.clone())
            })
        });
    let fallback_name = match (&item.show_title, item.season, item.episode) {
        (Some(show), Some(season), Some(episode)) => {
            format!("{show} S{season:02}E{episode:02}")
        }
        (_, Some(season), Some(episode)) => format!("S{season:02}E{episode:02}"),
        _ => format!("Episode {}", item.episode.unwrap_or(0)),
    };
    let name = item
        .episode_title
        .clone()
        .or_else(|| metadata.and_then(|meta| meta.title.clone()))
        .unwrap_or(fallback_name);
    json!({
        "Name": name,
        "Id": item.id,
        "ServerId": server_id(state),
        "Type": "Episode",
        "MediaType": "Video",
        "IsFolder": false,
        "SeriesId": show.map(|show| show.id.clone()).or_else(|| item.show_id.clone()),
        "SeriesName": show.map(|show| show.title.clone()).or_else(|| item.show_title.clone()),
        "SeasonId": show.map(|show| show.id.clone()).or_else(|| item.show_id.clone())
            .map(|id| format!("{id}:s{}", item.season.unwrap_or(0))),
        "ParentIndexNumber": item.season,
        "IndexNumber": item.episode,
        "ProductionYear": show.and_then(|show| show.year).or(item.year),
        "RunTimeTicks": ticks(item.duration_ms),
        "Container": if item.container == "strm" { "mkv" } else { item.container.as_str() },
        "Overview": metadata.and_then(|meta| meta.overview.clone())
            .or_else(|| show_metadata.and_then(|meta| meta.overview.clone())),
        "Genres": show_metadata.map(|meta| localized_genres(&meta.genres)).unwrap_or_default(),
        "CommunityRating": metadata.and_then(|meta| meta.rating).or_else(|| show_metadata.and_then(|meta| meta.rating)),
        "OfficialRating": metadata.and_then(|meta| meta.certification.clone())
            .or_else(|| show_metadata.and_then(|meta| meta.certification.clone())),
        "ImageTags": image_tag(poster.as_ref()),
        "BackdropImageTags": if poster.is_some() { json!(["1"]) } else { json!([]) },
        "DateCreated": item.added_at,
        "CanDownload": true,
        "LocationType": "FileSystem",
        "VideoType": "VideoFile",
        "UserData": { "Played": false, "PlaybackPositionTicks": 0, "PlayCount": 0, "IsFavorite": false },
        "MediaSources": [media_source(state, headers, item, access)],
    })
}

async fn items(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    Path(_params): Path<HashMap<String, String>>,
) -> Result<Json<Value>, Response> {
    let (user, access) = user_of(&state, &headers, &params).await?;
    let parent = params.get("ParentId").cloned().unwrap_or_default();
    let include = params.get("IncludeItemTypes").cloned().unwrap_or_default();
    let search = params
        .get("SearchTerm")
        .cloned()
        .unwrap_or_default()
        .to_lowercase();
    let limit = params
        .get("Limit")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(500);
    let skip = params
        .get("StartIndex")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    let defs =
        crate::services::settings::library_defs(&state.settings, &state.config).unwrap_or_default();
    let wanted = defs
        .iter()
        .find(|def| def.id == parent || def.name == parent);
    // A ParentId may also name a series ("<id>") or one of its seasons
    // ("<id>:s1"): clients browse Series → Seasons → Episodes through
    // /Items as well as through the /Shows routes.
    if wanted.is_none() && !parent.is_empty() {
        if let Some((show_id, number)) = parent.split_once(":s") {
            if let Ok(number) = number.parse::<u32>() {
                return episodes_payload(
                    &state,
                    &user,
                    &headers,
                    &params,
                    &access,
                    show_id.to_string(),
                    Some(number),
                )
                .await;
            }
        }
        let wanted_show = parent.clone();
        if query(&state.db, move |pool| db::get_show(&pool, &wanted_show))
            .await?
            .is_some()
        {
            return seasons_payload(
                &state,
                &user,
                parent.clone(),
                metadata_locale(&state, &headers, &params),
            )
            .await;
        }
    }
    // A person id browses everything that person is credited in.
    let person_parent = (!parent.is_empty() && parent.starts_with("person:") && wanted.is_none())
        .then(|| parent.clone());
    // Filters a grid sends down; without them a genre chip or a sort order
    // changes nothing.
    let ids: Option<std::collections::HashSet<String>> = params.get("Ids").map(|value| {
        value
            .split(',')
            .filter(|part| !part.is_empty())
            .map(str::to_string)
            .collect()
    });
    let genre_filter: Vec<String> = params
        .get("GenreIds")
        .into_iter()
        .chain(params.get("Genres"))
        .flat_map(|value| value.split(','))
        .filter(|part| !part.is_empty())
        .map(|part| part.to_lowercase())
        .collect();
    let sort_by = params
        .get("SortBy")
        .and_then(|value| value.split(',').next())
        .unwrap_or("SortName")
        .to_string();
    let descending = params
        .get("SortOrder")
        .is_some_and(|value| value.eq_ignore_ascii_case("Descending"));
    let matches_genres = |meta: Option<&crate::model::Metadata>| {
        genre_filter.is_empty()
            || meta.is_some_and(|meta| {
                meta.genres
                    .iter()
                    .any(|genre| genre_filter.contains(&genre.to_lowercase()))
            })
    };
    let library_names: Vec<String> = match wanted {
        Some(def) => vec![def.name.clone(), def.id.clone()],
        None => defs.iter().map(|d| d.name.clone()).collect(),
    };
    let filter_library = wanted.is_some();

    let mut out: Vec<Value> = Vec::new();
    // A named IncludeItemTypes wins: asking for `Movie` must not also hand
    // back every series, and asking for `Episode` must return episodes.
    let want_series = if include.is_empty() {
        parent_is_series(&defs, wanted)
    } else {
        include.contains("Series")
    };
    let want_movies = if include.is_empty() {
        true
    } else {
        include.contains("Movie") || include.contains("Video")
    };
    let want_episodes = include.contains("Episode");

    let locale = metadata_locale(&state, &headers, &params);
    let mut all_items = localized_items(&state, locale).await?;
    let mut all_shows = localized_shows(&state, locale).await?;
    crate::api::visibility::keep_items(&user, &mut all_items);
    crate::api::visibility::keep_shows(&user, &mut all_shows);
    let canonical_shows = dedupe_series(all_shows.clone());
    let canonical: Vec<&Show> = canonical_shows.iter().collect();
    let canonical_of: HashMap<String, String> = all_shows
        .iter()
        .map(|show| {
            let id = canonical_shows
                .iter()
                .find(|other| same_series(other, show))
                .map(|other| other.id.clone())
                .unwrap_or_else(|| show.id.clone());
            (show.id.clone(), id)
        })
        .collect();
    let show_map: HashMap<String, &Show> = all_shows
        .iter()
        .map(|show| {
            let id = canonical_of
                .get(&show.id)
                .cloned()
                .unwrap_or_else(|| show.id.clone());
            (
                show.id.clone(),
                canonical
                    .iter()
                    .find(|other| other.id == id)
                    .copied()
                    .unwrap_or(show),
            )
        })
        .collect();
    let person_params = person_parent.as_ref().map(|person| {
        let mut scoped = params.clone();
        scoped.insert("PersonIds".into(), person.clone());
        scoped
    });
    let effective = person_params.as_ref().unwrap_or(&params);
    if want_movies {
        for item in &all_items {
            if !matches!(item.kind, Kind::Movie | Kind::Video) || item.show_id.is_some() {
                continue;
            }
            if filter_library && !library_names.contains(&item.library) {
                continue;
            }
            if ids.as_ref().is_some_and(|ids| !ids.contains(&item.id)) {
                continue;
            }
            if !matches_genres(item.metadata.as_ref())
                || !catalog::matches_metadata(item.metadata.as_ref(), item.year, effective)
            {
                continue;
            }
            let text = item.title.to_lowercase();
            let original = item
                .metadata
                .as_ref()
                .and_then(|m| m.title.as_ref())
                .map(|t| t.to_lowercase());
            if !search.is_empty()
                && !text.contains(&search)
                && !original.is_some_and(|t| t.contains(&search))
            {
                continue;
            }
            out.push(movie_dto(&state, item));
        }
    }
    if want_series {
        for show in &canonical {
            if filter_library && !library_names.contains(&show.library) {
                continue;
            }
            if ids.as_ref().is_some_and(|ids| !ids.contains(&show.id)) {
                continue;
            }
            if !matches_genres(show.metadata.as_ref())
                || !catalog::matches_metadata(show.metadata.as_ref(), show.year, effective)
            {
                continue;
            }
            let text = show.title.to_lowercase();
            let original = show
                .metadata
                .as_ref()
                .and_then(|m| m.title.as_ref())
                .map(|t| t.to_lowercase());
            if !search.is_empty()
                && !text.contains(&search)
                && !original.is_some_and(|t| t.contains(&search))
            {
                continue;
            }
            out.push(series_dto(&state, show));
        }
    }
    if want_episodes {
        for item in &all_items {
            if !matches!(item.kind, Kind::Episode) || item.show_id.is_none() {
                continue;
            }
            if filter_library && !library_names.contains(&item.library) {
                continue;
            }
            if ids.as_ref().is_some_and(|ids| !ids.contains(&item.id)) {
                continue;
            }
            let show = item
                .show_id
                .as_deref()
                .and_then(|id| show_map.get(id).copied());
            if !matches_genres(show.and_then(|show| show.metadata.as_ref()))
                || !catalog::matches_metadata(
                    show.and_then(|show| show.metadata.as_ref()),
                    item.year,
                    effective,
                )
            {
                continue;
            }
            let text = item
                .episode_title
                .as_deref()
                .unwrap_or(&item.title)
                .to_lowercase();
            let original = item
                .metadata
                .as_ref()
                .and_then(|m| m.title.as_ref())
                .map(|t| t.to_lowercase());
            if !search.is_empty()
                && !text.contains(&search)
                && !original.is_some_and(|t| t.contains(&search))
            {
                continue;
            }
            out.push(episode_detail_dto(&state, &headers, item, show, &access));
        }
    }
    if !sort_by.eq_ignore_ascii_case("Random") {
        out.sort_by(|a, b| {
            let key_of = |value: &Value| -> String {
                match sort_by.to_ascii_lowercase().as_str() {
                    "datecreated" => value
                        .get("DateCreated")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    "productionyear" => format!(
                        "{:010}",
                        value
                            .get("ProductionYear")
                            .and_then(Value::as_i64)
                            .unwrap_or(0)
                    ),
                    "communityrating" => format!(
                        "{:012.4}",
                        value
                            .get("CommunityRating")
                            .and_then(Value::as_f64)
                            .unwrap_or(0.0)
                    ),
                    "runtime" => format!(
                        "{:014}",
                        value
                            .get("RunTimeTicks")
                            .and_then(Value::as_i64)
                            .unwrap_or(0)
                    ),
                    _ => value
                        .get("SortName")
                        .or_else(|| value.get("Name"))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_lowercase(),
                }
            };
            key_of(a).cmp(&key_of(b))
        });
        if descending {
            out.reverse();
        }
    }
    let total = out.len();
    let mut page: Vec<Value> = out.into_iter().skip(skip).take(limit).collect();
    apply_user_data(&state, &user, &mut page).await?;
    Ok(Json(json!({
        "Items": page,
        "TotalRecordCount": total,
        "StartIndex": skip,
    })))
}

fn parent_is_series(
    defs: &[crate::services::settings::LibraryDef],
    wanted: Option<&crate::services::settings::LibraryDef>,
) -> bool {
    wanted.is_some_and(|d| library_type(d) == "tvshows")
        || (wanted.is_none() && defs.iter().any(|d| library_type(d) == "tvshows"))
}

async fn latest(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    Path(_params): Path<HashMap<String, String>>,
) -> Result<Json<Value>, Response> {
    let (user, access) = user_of(&state, &headers, &params).await?;
    let parent = params.get("ParentId").cloned().unwrap_or_default();
    let defs =
        crate::services::settings::library_defs(&state.settings, &state.config).unwrap_or_default();
    let wanted = defs
        .iter()
        .find(|def| def.id == parent || def.name == parent);
    if !parent.is_empty() && wanted.is_none() {
        return Err(json_error(StatusCode::NOT_FOUND, "library not found"));
    }
    let locale = metadata_locale(&state, &headers, &params);
    let tv = wanted.is_some_and(|def| library_type(def) == "tvshows");
    let mut all = localized_items(&state, locale).await?;
    crate::api::visibility::keep_items(&user, &mut all);
    let uid = user.id.clone();
    let watched: HashSet<String> = query(&state.db, move |pool| db::list_watched(&pool, &uid))
        .await?
        .into_iter()
        .collect();
    if let Some(played) =
        query_value(&params, "IsPlayed").and_then(|value| value.parse::<bool>().ok())
    {
        if tv {
            all.retain(|item| {
                item.show_id
                    .as_ref()
                    .is_some_and(|_| watched.contains(&item.id) == played)
            });
        } else {
            all.retain(|item| watched.contains(&item.id) == played);
        }
    }
    if let Some(def) = wanted {
        all.retain(|item| item.library == def.id || item.library == def.name);
    }
    let include = query_value(&params, "IncludeItemTypes")
        .map(String::as_str)
        .unwrap_or("");
    all.retain(|item| {
        let kind = if item.kind == Kind::Episode {
            "Episode"
        } else {
            "Movie"
        };
        let included = include.is_empty()
            || include
                .split(',')
                .any(|wanted| wanted.eq_ignore_ascii_case(kind));
        included
            && match (tv, &item.kind) {
                (true, Kind::Episode) => item.show_id.is_some(),
                (true, _) => false,
                (false, Kind::Episode) => wanted.is_none() && item.show_id.is_some(),
                (false, _) => item.show_id.is_none(),
            }
    });
    all.sort_by(|a, b| {
        b.added_at
            .cmp(&a.added_at)
            .then_with(|| b.title.cmp(&a.title))
            .then_with(|| a.id.cmp(&b.id))
    });
    let mut shows = localized_shows(&state, locale).await?;
    crate::api::visibility::keep_shows(&user, &mut shows);
    let canonical_shows = dedupe_series(shows.clone());
    let show_for: HashMap<String, &Show> = shows
        .iter()
        .map(|show| {
            let id = canonical_shows
                .iter()
                .find(|other| same_series(other, show))
                .map(|other| other.id.clone())
                .unwrap_or_else(|| show.id.clone());
            (
                show.id.clone(),
                canonical_shows
                    .iter()
                    .find(|other| other.id == id)
                    .unwrap_or(show),
            )
        })
        .collect();
    let take = query_value(&params, "Limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(20);
    let skip = query_value(&params, "StartIndex")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let grouped = !query_value(&params, "GroupItems")
        .is_some_and(|value| value.eq_ignore_ascii_case("false"));
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for item in &all {
        if item.kind == Kind::Episode {
            let show = item
                .show_id
                .as_deref()
                .and_then(|id| show_for.get(id))
                .copied();
            if grouped {
                if let Some(show) = show {
                    if seen.insert(show.id.clone()) {
                        out.push(series_dto(&state, show));
                    }
                }
            } else {
                out.push(episode_detail_dto(&state, &headers, item, show, &access));
            }
        } else {
            out.push(movie_dto(&state, item));
        }
    }
    let mut out: Vec<Value> = out.into_iter().skip(skip).take(take).collect();
    apply_user_data(&state, &user, &mut out).await?;
    Ok(Json(Value::Array(out)))
}

async fn seasons(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, Response> {
    let (user, _) = user_of(&state, &headers, &params).await?;
    seasons_payload(
        &state,
        &user,
        id,
        metadata_locale(&state, &headers, &params),
    )
    .await
}

fn season_dto(
    state: &SharedState,
    show: &Show,
    season: &crate::model::Season,
    locale: &str,
) -> Value {
    let poster = season
        .poster_url
        .as_ref()
        .or_else(|| show.metadata.as_ref().and_then(|m| m.poster_url.as_ref()));
    let year = season
        .air_date
        .as_deref()
        .and_then(|date| date.get(..4))
        .and_then(|year| year.parse::<u32>().ok())
        .or(show.year);
    json!({
        "Name": season.name.clone().unwrap_or_else(|| season_name(locale, season.number)),
        "Id": format!("{}:s{}", show.id, season.number),
        "ServerId": server_id(state),
        "Type": "Season",
        "IsFolder": true,
        "ParentId": show.id,
        "SeriesId": show.id,
        "SeriesName": show.title,
        "IndexNumber": season.number,
        "ChildCount": season.episodes.len(),
        "ProductionYear": year,
        "PremiereDate": season.air_date,
        "Overview": season.overview,
        "ImageTags": image_tag(poster),
        "UserData": { "Played": false, "PlaybackPositionTicks": 0 },
    })
}

async fn seasons_payload(
    state: &SharedState,
    user: &User,
    id: String,
    locale: &'static str,
) -> Result<Json<Value>, Response> {
    crate::api::visibility::gate_show(state, user, &id).await?;
    let (detail, _) = merged_detail(state, user, locale, &id)
        .await?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "show not found"))?;
    let mut items: Vec<Value> = detail
        .seasons
        .iter()
        .map(|season| season_dto(state, &detail.show, season, locale))
        .collect();
    apply_user_data(state, user, &mut items).await?;
    Ok(Json(
        json!({ "Items": items, "TotalRecordCount": items.len() }),
    ))
}

async fn episodes(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, Response> {
    let (user, access) = user_of(&state, &headers, &params).await?;
    let wanted_season = if let Some(season_id) = query_value(&params, "SeasonId") {
        let Some((show, number)) = season_id.rsplit_once(":s") else {
            return Err(json_error(StatusCode::NOT_FOUND, "season not found"));
        };
        if show != id {
            return Err(json_error(StatusCode::NOT_FOUND, "season not found"));
        }
        Some(
            number
                .parse::<u32>()
                .map_err(|_| json_error(StatusCode::BAD_REQUEST, "invalid season"))?,
        )
    } else {
        query_value(&params, "Season").and_then(|value| value.parse::<u32>().ok())
    };
    episodes_payload(&state, &user, &headers, &params, &access, id, wanted_season).await
}

async fn episodes_payload(
    state: &SharedState,
    user: &User,
    headers: &HeaderMap,
    params: &HashMap<String, String>,
    access: &str,
    id: String,
    wanted_season: Option<u32>,
) -> Result<Json<Value>, Response> {
    crate::api::visibility::gate_show(state, user, &id).await?;
    let (detail, _) = merged_detail(state, user, metadata_locale(state, headers, params), &id)
        .await?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "show not found"))?;
    let mut out: Vec<Value> = detail
        .seasons
        .iter()
        .filter(|season| wanted_season.is_none_or(|wanted| wanted == season.number))
        .flat_map(|season| season.episodes.iter())
        .map(|episode| episode_detail_dto(state, headers, episode, Some(&detail.show), access))
        .collect();
    apply_user_data(state, user, &mut out).await?;
    filter_user_data(&mut out, params);
    if query_value(params, "SortOrder")
        .is_some_and(|value| value.eq_ignore_ascii_case("Descending"))
    {
        out.reverse();
    }
    if let Some(start) = query_value(params, "StartItemId") {
        if let Some(index) = out
            .iter()
            .position(|item| item["Id"].as_str() == Some(start))
        {
            out.drain(..index);
        }
    }
    let total = out.len();
    let skip = query_value(params, "StartIndex")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let limit = query_value(params, "Limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(total);
    let out: Vec<Value> = out.into_iter().skip(skip).take(limit).collect();
    Ok(Json(
        json!({ "Items": out, "TotalRecordCount": total, "StartIndex": skip }),
    ))
}

fn filter_user_data(items: &mut Vec<Value>, params: &HashMap<String, String>) {
    for (key, field) in [("IsPlayed", "Played"), ("IsFavorite", "IsFavorite")] {
        if let Some(value) = query_value(params, key).and_then(|value| value.parse::<bool>().ok()) {
            items.retain(|item| item["UserData"][field].as_bool().unwrap_or(false) == value);
        }
    }
    if let Some(filters) = query_value(params, "Filters") {
        for filter in filters.split(',') {
            if filter.eq_ignore_ascii_case("IsUnplayed") {
                items.retain(|item| item["UserData"]["Played"] != true);
            } else if filter.eq_ignore_ascii_case("IsPlayed") {
                items.retain(|item| item["UserData"]["Played"] == true);
            } else if filter.eq_ignore_ascii_case("IsFavorite") {
                items.retain(|item| item["UserData"]["IsFavorite"] == true);
            }
        }
    }
}

async fn playback_info(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, Response> {
    let (user, access) = user_of(&state, &headers, &params).await?;
    let item = query(&state.db, move |pool| db::get_item(&pool, &id))
        .await?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "item not found"))?;
    // Episodes are playable too: a client asks for PlaybackInfo before every
    // play, and an episode card that cannot answer it never starts.
    if !matches!(item.kind, Kind::Movie | Kind::Video | Kind::Episode) {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "not a playable movie item",
        ));
    }
    crate::api::visibility::gate_item(&state, &user, &item.id).await?;
    Ok(Json(json!({
        "MediaSources": [media_source(&state, &headers, &item, &access)],
        "PlaySessionId": item.id,
    })))
}

async fn image(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    Path((id, kind)): Path<(String, String)>,
) -> Result<Response, Response> {
    let tag = query_value(&params, "tag").cloned();
    image_art(&state, &headers, &params, id, kind, 0, tag.as_deref()).await
}

async fn image_index(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    Path((id, kind, index)): Path<(String, String, String)>,
) -> Result<Response, Response> {
    // Clients disagree on this slot: some send the numeric slide index, others
    // send the ImageTag itself. A tag must resolve to its slide, never to 0,
    // or every card in an artwork gallery shows the primary backdrop.
    let (index, tag) = match index.trim().parse::<usize>() {
        Ok(index) => (index, None),
        Err(_) => (0, Some(index.trim().to_string())),
    };
    image_art(&state, &headers, &params, id, kind, index, tag.as_deref()).await
}

/// The images an item advertises, matching the tags its DTO carries.
async fn image_list(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, Response> {
    user_of(&state, &headers, &params).await?;
    let mut images = Vec::new();
    let item = query(&state.db, {
        let id = id.clone();
        move |pool| db::get_item(&pool, &id)
    })
    .await?;
    let (primary, backdrops) = if let Some(item) = &item {
        let meta = item.metadata.as_ref();
        (
            meta.and_then(|m| m.poster_url.clone().or_else(|| m.backdrop_url.clone())),
            backdrop_urls(meta),
        )
    } else {
        let show = query(&state.db, {
            let id = id.clone();
            move |pool| db::get_show(&pool, &id)
        })
        .await?;
        match show {
            Some(detail) => {
                let meta = detail.show.metadata.as_ref();
                (
                    meta.and_then(|m| m.poster_url.clone().or_else(|| m.backdrop_url.clone())),
                    backdrop_urls(meta),
                )
            }
            None => (None, Vec::new()),
        }
    };
    if let Some(url) = primary {
        images.push(json!({
            "ImageType": "Primary",
            "ImageIndex": 0,
            "ImageTag": library_image_tag_value(&url),
        }));
    }
    for (index, url) in backdrops.iter().enumerate() {
        images.push(json!({
            "ImageType": "Backdrop",
            "ImageIndex": index,
            "ImageTag": library_image_tag_value(url),
        }));
    }
    Ok(Json(json!(images)))
}

async fn image_art(
    state: &SharedState,
    headers: &HeaderMap,
    params: &HashMap<String, String>,
    id: String,
    kind: String,
    index: usize,
    tag: Option<&str>,
) -> Result<Response, Response> {
    let (user, _) = user_of(state, headers, params).await?;
    if id.starts_with("person:") {
        return match catalog::person_art(state, &user, &id).await? {
            Some(target) => redirect(&client_url(headers, &target)),
            None => Err(json_error(StatusCode::NOT_FOUND, "no image")),
        };
    }
    let wanted = id.clone();
    let item = query(&state.db, move |pool| db::get_item(&pool, &wanted)).await?;
    let backdrop = kind.eq_ignore_ascii_case("backdrop");
    let art_key = if backdrop {
        db::art_overrides::BACKDROP
    } else {
        db::art_overrides::POSTER
    };
    let art = if let Some(item) = item {
        let overrides = query(&state.db, {
            let id = item.id.clone();
            move |pool| db::art_overrides::get(&pool, db::art_overrides::ITEM_KIND, &id)
        })
        .await?;
        // Extra slides are provider art: an operator override only ever
        // replaces the first backdrop, so later indices bypass it.
        let own = if backdrop && tag.is_some() {
            item.metadata
                .as_ref()
                .and_then(|meta| tagged_backdrop(meta, tag.unwrap()).cloned())
        } else if backdrop && index > 0 {
            item.metadata
                .as_ref()
                .and_then(|meta| backdrop_at(meta, index))
        } else {
            overrides.get(art_key).cloned().or_else(|| {
                item.metadata.as_ref().and_then(|meta| {
                    // An episode's own still travels in `backdropUrl`; Primary must
                    // use it too, or every card in a season shows the series art.
                    if item.show_id.is_some() {
                        meta.backdrop_url
                            .clone()
                            .or_else(|| meta.poster_url.clone())
                    } else if backdrop {
                        meta.backdrop_url.clone()
                    } else {
                        meta.poster_url.clone()
                    }
                })
            })
        };
        match own {
            Some(url) => Some(url),
            // An episode carries no art of its own; its series' art stands in.
            None => match item.show_id.clone() {
                Some(show_id) => {
                    let detail =
                        query(&state.db, move |pool| db::get_show(&pool, &show_id)).await?;
                    match detail {
                        Some(detail) => {
                            let show_overrides = query(&state.db, {
                                let id = detail.show.id.clone();
                                move |pool| {
                                    db::art_overrides::get(&pool, db::art_overrides::SHOW_KIND, &id)
                                }
                            })
                            .await?;
                            show_overrides.get(art_key).cloned().or_else(|| {
                                detail.show.metadata.as_ref().and_then(|meta| {
                                    if backdrop {
                                        meta.backdrop_url.clone()
                                    } else {
                                        meta.poster_url.clone()
                                    }
                                })
                            })
                        }
                        None => None,
                    }
                }
                None => None,
            },
        }
    } else if let Some(detail) = {
        let wanted = id.clone();
        query(&state.db, move |pool| db::get_show(&pool, &wanted)).await?
    } {
        let overrides = query(&state.db, {
            let id = detail.show.id.clone();
            move |pool| db::art_overrides::get(&pool, db::art_overrides::SHOW_KIND, &id)
        })
        .await?;
        // Only the first backdrop is replaceable by an operator pick; the rest
        // of the gallery is whatever the provider ordered by vote.
        let picked = if backdrop && index > 0 {
            None
        } else {
            overrides.get(art_key).cloned()
        };
        if backdrop {
            if let Some(tag) = tag {
                if let Some(url) = detail
                    .show
                    .metadata
                    .as_ref()
                    .and_then(|meta| tagged_backdrop(meta, tag).cloned())
                {
                    return redirect(&client_url(headers, &url));
                }
            }
        }
        picked.or_else(|| {
            detail.show.metadata.as_ref().and_then(|meta| {
                if backdrop {
                    backdrop_at(meta, index)
                } else {
                    meta.poster_url.clone()
                }
            })
        })
    } else if let Some((detail, season)) = {
        // A season id ("<show>:s1") prefers its own poster, else the series'.
        let parsed = id
            .split_once(":s")
            .and_then(|(show_id, number)| Some((show_id.to_string(), number.parse::<u32>().ok()?)));
        match parsed {
            Some((show_id, number)) => {
                query(&state.db, move |pool| {
                    let detail = db::get_show(&pool, &show_id)?;
                    let season = detail.as_ref().and_then(|detail| {
                        detail
                            .seasons
                            .iter()
                            .find(|season| season.number == number)
                            .cloned()
                    });
                    Ok(detail.map(|detail| (detail, season)))
                })
                .await?
            }
            None => None,
        }
    } {
        if let Some(poster) = season.as_ref().and_then(|season| season.poster_url.clone()) {
            Some(poster)
        } else {
            let overrides = query(&state.db, {
                let id = detail.show.id.clone();
                move |pool| db::art_overrides::get(&pool, db::art_overrides::SHOW_KIND, &id)
            })
            .await?;
            overrides.get(art_key).cloned().or_else(|| {
                detail.show.metadata.as_ref().and_then(|meta| {
                    if backdrop {
                        meta.backdrop_url.clone()
                    } else {
                        meta.poster_url.clone()
                    }
                })
            })
        }
    } else {
        // A library has no row of its own; an operator-picked cover wins,
        // otherwise the first title's art is what a CollectionFolder shows.
        let defs = crate::services::settings::library_defs(&state.settings, &state.config)
            .unwrap_or_default();
        if let Some(def) = defs.iter().find(|def| def.id == id || def.name == id) {
            let picked = query(&state.db, {
                let id = def.id.clone();
                move |pool| db::art_overrides::get(&pool, db::art_overrides::LIBRARY_KIND, &id)
            })
            .await?;
            // The one cover an operator picked serves as Primary even when it
            // was picked on the backdrop side of the artwork dialog.
            let picked_cover = picked
                .get(art_key)
                .or_else(|| {
                    if backdrop {
                        None
                    } else {
                        picked.get(db::art_overrides::BACKDROP)
                    }
                })
                .cloned();
            if let Some(url) = picked_cover {
                Some(url)
            } else {
                let items = query(&state.db, |pool| db::list_items(&pool, None)).await?;
                items
                    .iter()
                    .find(|item| item.library == def.name || item.library == def.id)
                    .and_then(|item| item.metadata.as_ref())
                    .and_then(|meta| {
                        if backdrop {
                            meta.backdrop_url.clone()
                        } else {
                            meta.poster_url.clone()
                        }
                    })
            }
        } else {
            None
        }
    };
    let Some(target) = art else {
        return Err(json_error(StatusCode::NOT_FOUND, "no image"));
    };
    redirect(&client_url(headers, &target))
}

async fn stream(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    Path(id): Path<String>,
) -> Result<Response, Response> {
    let (_user, access) = user_of(&state, &headers, &params).await?;
    let item = query(&state.db, move |pool| db::get_item(&pool, &id))
        .await?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "item not found"))?;
    if let Some(path) = item
        .abs_path
        .as_deref()
        .filter(|p| p.starts_with("http://") || p.starts_with("https://"))
    {
        return redirect(&client_url(&headers, path));
    }
    let ticket = crate::services::media_ticket::for_access_token(&state.media_ticket_key, &access);
    redirect(&client_url(
        &headers,
        &format!("/api/items/{}/stream?t={ticket}", item.id),
    ))
}

/// A playback URL as the calling client must see it. The `.strm` files name the
/// server as `127.0.0.1` (the address it was scanned from), which a phone or TV
/// resolves to itself; the same goes for a relative path. Both are re-anchored
/// to the authority the request arrived on, so only that header decides where
/// the client is told to fetch bytes from.
fn client_url(headers: &HeaderMap, target: &str) -> String {
    let Some(origin) = client_origin(headers) else {
        return target.to_string();
    };
    for scheme in ["http://", "https://"] {
        for host in ["127.0.0.1", "localhost", "[::1]"] {
            let prefix = format!("{scheme}{host}");
            let Some(rest) = target.strip_prefix(&prefix) else {
                continue;
            };
            let rest = if rest.is_empty() {
                String::new()
            } else if let Some(after) = rest.strip_prefix(':') {
                after
                    .find('/')
                    .map(|at| after[at..].to_string())
                    .unwrap_or_default()
            } else if rest.starts_with('/') {
                rest.to_string()
            } else {
                continue;
            };
            return format!("{origin}{rest}");
        }
    }
    if target.starts_with('/') {
        return format!("{origin}{target}");
    }
    target.to_string()
}

fn client_origin(headers: &HeaderMap) -> Option<String> {
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())?;
    let proto = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(str::trim)
        .filter(|v| *v == "http" || *v == "https")
        .unwrap_or("http");
    Some(format!("{proto}://{host}"))
}

fn redirect(target: &str) -> Result<Response, Response> {
    let Ok(location) = HeaderValue::from_str(target) else {
        return Err(json_error(StatusCode::BAD_GATEWAY, "invalid redirect"));
    };
    let mut response = (StatusCode::FOUND, ()).into_response();
    response.headers_mut().insert(header::LOCATION, location);
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

async fn no_content() -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn quick_connect_enabled() -> Json<Value> {
    Json(json!(false))
}

async fn display_preferences(Path(id): Path<String>) -> Json<Value> {
    Json(json!({
        "Id": id,
        "CustomPrefs": {},
        "SortBy": "SortName",
        "SortOrder": "Ascending",
        "RememberIndexing": false,
        "RememberSorting": false,
        "ScrollDirection": "Vertical",
        "ShowBackdrop": true,
        "ShowSidebar": true,
        "Client": "kroma",
    }))
}

async fn user_item_data(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, Response> {
    let (user, _) = user_of(&state, &headers, &params).await?;
    let value = with_user_data(&state, &user, json!({ "Id": id })).await?;
    Ok(Json(
        value.get("UserData").cloned().unwrap_or_else(|| json!({})),
    ))
}

async fn ancestors(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    Path(_path): Path<HashMap<String, String>>,
) -> Result<Json<Value>, Response> {
    user_of(&state, &headers, &params).await?;
    Ok(Json(json!([])))
}

#[allow(dead_code)]
async fn next_up(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, Response> {
    let (user, access) = user_of(&state, &headers, &params).await?;
    let locale = metadata_locale(&state, &headers, &params);
    let mut shows = localized_shows(&state, locale).await?;
    crate::api::visibility::keep_shows(&user, &mut shows);
    let mut episodes = localized_items(&state, locale).await?;
    crate::api::visibility::keep_items(&user, &mut episodes);
    let uid = user.id.clone();
    let progress = query(&state.db, move |pool| db::list_progress(&pool, &uid)).await?;
    let uid = user.id.clone();
    let watched: HashSet<String> = query(&state.db, move |pool| db::list_watched(&pool, &uid))
        .await?
        .into_iter()
        .collect();
    let canonical = dedupe_series(shows.clone());
    let canonical_of: HashMap<String, String> = shows
        .iter()
        .map(|show| {
            let id = canonical
                .iter()
                .find(|other| same_series(other, show))
                .map(|other| other.id.clone())
                .unwrap_or_else(|| show.id.clone());
            (show.id.clone(), id)
        })
        .collect();
    let series_filter = query_value(&params, "SeriesId").cloned();
    let started: HashSet<String> = progress
        .iter()
        .filter(|entry| entry.position_ms > 0)
        .chain(
            progress
                .iter()
                .filter(|entry| entry.position_ms == 0 && watched.contains(&entry.item_id)),
        )
        .filter_map(|entry| episodes.iter().find(|item| item.id == entry.item_id))
        .filter_map(|item| item.show_id.as_deref())
        .filter_map(|id| canonical_of.get(id).cloned())
        .chain(
            watched
                .iter()
                .filter_map(|id| episodes.iter().find(|item| &item.id == id))
                .filter_map(|item| item.show_id.as_deref())
                .filter_map(|id| canonical_of.get(id).cloned()),
        )
        .collect();
    let limit = query_value(&params, "Limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(20);
    let mut out = Vec::new();
    for show in &canonical {
        let member_ids: Vec<&String> = canonical_of
            .iter()
            .filter(|(_, canonical_id)| *canonical_id == &show.id)
            .map(|(id, _)| id)
            .collect();
        if let Some(series) = series_filter.as_ref() {
            if &show.id != series && !member_ids.iter().any(|id| *id == series) {
                continue;
            }
        } else if !started.contains(&show.id) {
            continue;
        }
        let mut ordered: Vec<&MediaItem> = episodes
            .iter()
            .filter(|item| {
                item.show_id
                    .as_ref()
                    .is_some_and(|id| member_ids.iter().any(|member| *member == id))
            })
            .collect();
        ordered.sort_by_key(|item| (item.season.unwrap_or(0), item.episode.unwrap_or(0)));
        if ordered.is_empty() {
            continue;
        }
        let resumable = progress
            .iter()
            .filter(|entry| entry.position_ms > 15_000)
            .filter(|entry| {
                entry
                    .duration_ms
                    .is_none_or(|duration| entry.position_ms * 100 < duration * 95)
            })
            .filter(|entry| ordered.iter().any(|item| item.id == entry.item_id))
            .max_by(|a, b| a.updated_at.cmp(&b.updated_at));
        let chosen = if let Some(resume) = resumable {
            ordered
                .iter()
                .find(|item| item.id == resume.item_id)
                .copied()
        } else {
            let highest = ordered.iter().rposition(|item| watched.contains(&item.id));
            match highest {
                Some(position) => ordered.get(position + 1).copied(),
                None => ordered.first().copied(),
            }
        };
        let Some(episode) = chosen else { continue };
        out.push(episode_detail_dto(
            &state,
            &headers,
            episode,
            Some(show),
            &access,
        ));
        if out.len() >= limit {
            break;
        }
    }
    apply_user_data(&state, &user, &mut out).await?;
    Ok(Json(json!({ "Items": out, "TotalRecordCount": out.len() })))
}

async fn empty_items(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    Path(_params): Path<HashMap<String, String>>,
) -> Result<Json<Value>, Response> {
    user_of(&state, &headers, &params).await?;
    Ok(Json(json!({ "Items": [], "TotalRecordCount": 0 })))
}

async fn ping() -> StatusCode {
    StatusCode::OK
}

async fn endpoint() -> Json<Value> {
    Json(json!({ "IsLocal": true, "IsInNetwork": true }))
}

async fn users_public() -> Json<Value> {
    Json(json!([]))
}

async fn items_root(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    Path(_params): Path<HashMap<String, String>>,
) -> Result<Json<Value>, Response> {
    user_of(&state, &headers, &params).await?;
    Ok(Json(json!({
        "Name": "Media Folders",
        "ServerId": server_id(&state),
        "Id": "root",
        "Type": "UserRootFolder",
        "MediaType": "Unknown",
        "IsFolder": true,
    })))
}

async fn grouping_options(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    Path(_params): Path<HashMap<String, String>>,
) -> Result<Json<Value>, Response> {
    user_of(&state, &headers, &params).await?;
    let defs =
        crate::services::settings::library_defs(&state.settings, &state.config).unwrap_or_default();
    let options: Vec<Value> = defs
        .iter()
        .map(|def| json!({ "Name": def.name, "Id": def.id }))
        .collect();
    Ok(Json(json!(options)))
}

async fn virtual_folders(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, Response> {
    user_of(&state, &headers, &params).await?;
    let defs =
        crate::services::settings::library_defs(&state.settings, &state.config).unwrap_or_default();
    let folders: Vec<Value> = defs
        .iter()
        .map(|def| {
            let kind = library_type(def);
            json!({
                "Name": def.name,
                "Locations": def.folders,
                "CollectionType": kind,
                "LibraryOptions": {
                    "ContentType": kind,
                    "MetadataCountryCode": "",
                    "PreferredMetadataLanguage": "",
                },
                "ItemId": def.id,
                "PrimaryImageItemId": Value::Null,
            })
        })
        .collect();
    Ok(Json(json!(folders)))
}

async fn empty_sessions(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, Response> {
    user_of(&state, &headers, &params).await?;
    Ok(Json(json!([])))
}

async fn theme_media(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    Path(_params): Path<HashMap<String, String>>,
) -> Result<Json<Value>, Response> {
    user_of(&state, &headers, &params).await?;
    Ok(Json(json!({
        "OwnedMedia": [],
        "MediaSourceId": Value::Null,
        "TotalRecordCount": 0,
    })))
}

async fn search_hints(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, Response> {
    let (user, _) = user_of(&state, &headers, &params).await?;
    let term = params
        .get("SearchTerm")
        .map(|value| value.to_lowercase())
        .unwrap_or_default();
    if term.is_empty() {
        return Ok(Json(json!({ "SearchHints": [], "TotalRecordCount": 0 })));
    }
    let locale = metadata_locale(&state, &headers, &params);
    let mut items = localized_items(&state, locale).await?;
    let mut shows = localized_shows(&state, locale).await?;
    crate::api::visibility::keep_items(&user, &mut items);
    crate::api::visibility::keep_shows(&user, &mut shows);
    let shows = dedupe_series(shows);
    let mut hints = Vec::new();
    for item in items
        .iter()
        .filter(|item| item.show_id.is_none() && item.title.to_lowercase().contains(&term))
    {
        hints.push(json!({
            "ItemId": item.id,
            "Id": item.id,
            "Name": item.title,
            "Type": "Movie",
            "MediaType": "Video",
            "ProductionYear": item.year,
            "RunTimeTicks": ticks(item.duration_ms),
        }));
    }
    for show in shows
        .iter()
        .filter(|show| show.title.to_lowercase().contains(&term))
    {
        hints.push(json!({
            "ItemId": show.id,
            "Id": show.id,
            "Name": show.title,
            "Type": "Series",
            "MediaType": "Unknown",
            "ProductionYear": show.year,
        }));
    }
    Ok(Json(json!({
        "SearchHints": hints,
        "TotalRecordCount": hints.len(),
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(primary: Option<&str>, extras: &[&str]) -> crate::model::Metadata {
        serde_json::from_value(json!({
            "tmdbId": 1,
            "tmdbUrl": "https://www.themoviedb.org/movie/1",
            "genres": [],
            "backdropUrl": primary,
            "backdrops": extras,
        }))
        .unwrap()
    }

    #[test]
    fn the_gallery_leads_with_the_primary_backdrop_without_repeating_it() {
        let art = meta(Some("/api/images/hero.webp"), &["/api/images/two.webp"]);

        assert_eq!(
            backdrop_urls(Some(&art)),
            vec![
                "/api/images/hero.webp".to_string(),
                "/api/images/two.webp".to_string()
            ]
        );

        // A stored gallery that still repeats the hero only advertises it once.
        let repeated = meta(Some("/api/images/hero.webp"), &["/api/images/hero.webp"]);
        assert_eq!(backdrop_urls(Some(&repeated)).len(), 1);
    }

    #[test]
    fn a_title_without_backdrop_art_advertises_an_empty_tag_list() {
        assert_eq!(backdrop_urls(None), Vec::<String>::new());
        assert_eq!(backdrop_tags(&[]), json!([]));
    }

    #[test]
    fn each_backdrop_tag_follows_its_own_url() {
        let art = meta(
            Some("/api/images/hero.webp"),
            &["/api/images/two.webp", "/api/images/three.webp"],
        );
        let tags = backdrop_tags(&backdrop_urls(Some(&art)));

        assert_eq!(tags.as_array().unwrap().len(), 3);
        assert_eq!(
            tags[0],
            json!(library_image_tag_value("/api/images/hero.webp"))
        );
        assert_eq!(
            tags[2],
            json!(library_image_tag_value("/api/images/three.webp"))
        );
    }

    #[test]
    fn a_backdrop_index_walks_the_gallery_and_falls_back_to_the_hero() {
        let art = meta(
            Some("/api/images/hero.webp"),
            &["/api/images/two.webp", "/api/images/three.webp"],
        );

        assert_eq!(
            backdrop_at(&art, 0).as_deref(),
            Some("/api/images/hero.webp")
        );
        assert_eq!(
            backdrop_at(&art, 2).as_deref(),
            Some("/api/images/three.webp")
        );
        // Past the last slide a stale client still gets the primary image.
        assert_eq!(
            backdrop_at(&art, 9).as_deref(),
            Some("/api/images/hero.webp")
        );
    }
}
