//! Jellyfin compatibility, first slice: public system info, password login,
//! library views, movies/series browsing, seasons/episodes, poster redirects,
//! playback info and direct playback. Shapes follow the Jellyfin 10.9 API; ids
//! stay KROMA's own (clients treat BaseItemDto.Id as an opaque string).
//!
//! The routes live at the ROOT, because that is where Jellyfin clients look
//! (`/Users/AuthenticateByName`, `/Videos/{id}/stream`), not under `/api`.

use std::collections::HashMap;

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

const SERVER_VERSION: &str = "10.9.11";

pub fn routes() -> Router<SharedState> {
    Router::new()
        .route("/System/Info/Public", get(info_public))
        .route("/System/Info", get(info_public))
        .route("/Branding/Configuration", get(branding))
        .route("/Users/AuthenticateByName", post(authenticate))
        .route("/Users/Me", get(me))
        .route("/Users/{user_id}", get(me))
        .route("/Users/{user_id}/Views", get(views))
        .route("/Users/{user_id}/Items", get(items))
        .route("/Users/{user_id}/Items/Latest", get(latest))
        .route("/Users/{user_id}/Items/{id}", get(item_detail))
        .route("/Users/{user_id}/Items/{id}/Similar", get(similar))
        .route("/UserViews", get(user_views))
        .route("/Items/{id}", get(item_detail))
        .route("/Items/{id}/Similar", get(similar))
        .route("/Items/Counts", get(item_counts))
        .route("/Items/Filters", get(filters))
        .route("/Items/Filters2", get(filters))
        .route("/Genres", get(genres))
        .route("/Notifications/{user_id}/Summary", get(notifications_summary))
        .route("/Shows/{id}/Seasons", get(seasons))
        .route("/Shows/{id}/Episodes", get(episodes))
        .route("/Items/{id}/PlaybackInfo", post(playback_info))
        .route("/Items/{id}/PlaybackInfo", get(playback_info))
        .route("/Items/{id}/Images/{kind}", get(image))
        .route("/Items/{id}/Images/{kind}/{index}", get(image_index))
        .route("/Videos/{id}/stream", get(stream))
        .route("/Videos/{id}/stream.{ext}", get(stream))
        .route("/Sessions/Playing", post(no_content))
        .route("/Sessions/Playing/Progress", post(no_content))
        .route("/Sessions/Playing/Stopped", post(no_content))
        .route("/Sessions/Capabilities", post(no_content))
        .route("/Sessions/Capabilities/Full", post(no_content))
        .route("/QuickConnect/Enabled", get(quick_connect_enabled))
        .route("/DisplayPreferences/{id}", get(display_preferences))
        .route("/Users/{user_id}/Items/Resume", get(empty_items))
        .route("/Shows/NextUp", get(empty_items))
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
    Ok(Json(views_payload(&state)))
}

async fn user_views(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, Response> {
    user_of(&state, &headers, &params).await?;
    Ok(Json(views_payload(&state)))
}

fn views_payload(state: &SharedState) -> Value {
    let defs = crate::services::settings::library_defs(&state.settings, &state.config);
    let items: Vec<Value> = defs
        .iter()
        .map(|def| {
            json!({
                "Name": def.name,
                "Id": def.id,
                "Type": "CollectionFolder",
                "CollectionType": library_type(def),
                "IsFolder": true,
                "ServerId": server_id(state),
            })
        })
        .collect();
    json!({ "Items": items, "TotalRecordCount": items.len() })
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
    let wanted = id.clone();
    if let Some(detail) = query(&state.db, move |pool| db::get_show(&pool, &wanted)).await? {
        crate::api::visibility::gate_show(&state, &user, &detail.show.id).await?;
        return Ok(Json(series_detail_dto(&state, &detail.show)));
    }
    let item = query(&state.db, move |pool| db::get_item(&pool, &id))
        .await?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "item not found"))?;
    crate::api::visibility::gate_item(&state, &user, &item.id).await?;
    if let Some(show_id) = item.show_id.clone() {
        let show = query(&state.db, move |pool| db::get_show(&pool, &show_id)).await?;
        return Ok(Json(episode_detail_dto(
            &state,
            &headers,
            &item,
            show.as_ref().map(|d| &d.show),
            &access,
        )));
    }
    Ok(Json(movie_detail_dto(
        &state,
        &headers,
        &item,
        &access,
    )))
}

async fn similar(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    Path(_path): Path<HashMap<String, String>>,
) -> Result<Json<Value>, Response> {
    user_of(&state, &headers, &params).await?;
    Ok(Json(json!({ "Items": [], "TotalRecordCount": 0 })))
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

async fn filters(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, Response> {
    user_of(&state, &headers, &params).await?;
    Ok(Json(json!({
        "Genres": [],
        "Tags": [],
        "OfficialRatings": [],
        "Years": [],
    })))
}

async fn genres(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, Response> {
    let (user, _) = user_of(&state, &headers, &params).await?;
    let mut items = query(&state.db, |pool| db::list_items(&pool, None)).await?;
    crate::api::visibility::keep_items(&user, &mut items);
    let mut names: Vec<String> = items
        .iter()
        .filter_map(|item| item.metadata.as_ref())
        .flat_map(|meta| meta.genres.iter().cloned())
        .collect();
    names.sort();
    names.dedup();
    let items: Vec<Value> = names
        .iter()
        .map(|name| json!({ "Name": name, "Id": name }))
        .collect();
    Ok(Json(
        json!({ "Items": items, "TotalRecordCount": items.len() }),
    ))
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

fn ticks(ms: Option<u64>) -> i64 {
    ms.map(|ms| (ms as i64) * 10_000).unwrap_or(0)
}

fn image_tag(art: Option<&String>) -> Value {
    match art {
        Some(_) => json!({ "Primary": "1" }),
        None => json!({}),
    }
}

fn movie_dto(state: &SharedState, item: &MediaItem) -> Value {
    let poster = item.metadata.as_ref().and_then(|m| m.poster_url.clone());
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
        "ImageTags": image_tag(poster.as_ref()),
        "BackdropImageTags": if item.metadata.as_ref().and_then(|m| m.backdrop_url.as_ref()).is_some() { json!(["1"]) } else { json!([]) },
        "UserData": { "Played": false, "PlaybackPositionTicks": 0, "PlayCount": 0, "IsFavorite": false },
    })
}

fn series_dto(state: &SharedState, show: &Show) -> Value {
    let poster = show.metadata.as_ref().and_then(|m| m.poster_url.clone());
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
        "ImageTags": image_tag(poster.as_ref()),
        "BackdropImageTags": if show.metadata.as_ref().and_then(|m| m.backdrop_url.as_ref()).is_some() { json!(["1"]) } else { json!([]) },
        "ChildCount": show.episode_count,
        "RecursiveItemCount": show.episode_count,
        "UserData": { "Played": false, "PlaybackPositionTicks": 0, "PlayCount": 0, "IsFavorite": false },
    })
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

fn item_people(meta: Option<&crate::model::Metadata>) -> Value {
    let Some(meta) = meta else {
        return json!([]);
    };
    let mut people = Vec::new();
    for member in &meta.cast {
        people.push(json!({
            "Name": member.name,
            "Type": "Actor",
            "Role": member.character,
        }));
    }
    for member in &meta.crew {
        people.push(json!({
            "Name": member.name,
            "Type": if member.job.eq_ignore_ascii_case("director") { "Director" } else { "Writer" },
            "Role": member.job,
        }));
    }
    json!(people)
}

fn media_streams(item: &MediaItem) -> Value {
    let mut streams = Vec::new();
    if let Some(video) = item.video.as_ref() {
        streams.push(json!({
            "Type": "Video",
            "Index": 0,
            "Codec": video.codec,
            "Width": video.width,
            "Height": video.height,
            "IsDefault": true,
        }));
    }
    for (i, audio) in item.audio_tracks.iter().enumerate() {
        streams.push(json!({
            "Type": "Audio",
            "Index": i + 1,
            "Codec": audio.codec,
            "Channels": audio.channels,
            "Language": audio.language,
            "DisplayTitle": audio.title,
            "IsDefault": audio.default,
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

fn media_source(
    state: &SharedState,
    headers: &HeaderMap,
    item: &MediaItem,
    access: &str,
) -> Value {
    let ticket = crate::services::media_ticket::for_access_token(&state.media_ticket_key, access);
    let path = item
        .abs_path
        .clone()
        .filter(|p| p.starts_with("http://") || p.starts_with("https://"))
        .unwrap_or_else(|| format!("/api/items/{}/stream?t={ticket}", item.id));
    json!({
        "Id": item.id,
        "Name": item.title,
        "Path": client_url(headers, &path),
        "Container": if item.container == "strm" { "mkv" } else { item.container.as_str() },
        "RunTimeTicks": ticks(item.duration_ms),
        "Size": Value::Null,
        "SupportsDirectPlay": true,
        "SupportsDirectStream": true,
        "SupportsTranscoding": false,
        "IsRemote": item.abs_path.as_deref().is_some_and(|p| p.starts_with("http")),
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
        dto["Genres"] = json!(meta.genres.clone());
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
        dto["People"] = item_people(Some(meta));
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
        dto["Genres"] = json!(meta.genres.clone());
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
        dto["People"] = item_people(Some(meta));
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
    let poster = show
        .and_then(|show| show.metadata.as_ref())
        .and_then(|meta| meta.poster_url.clone());
    json!({
        "Name": item.episode_title.clone().unwrap_or_else(|| format!("Episode {}", item.episode.unwrap_or(0))),
        "Id": item.id,
        "ServerId": server_id(state),
        "Type": "Episode",
        "MediaType": "Video",
        "IsFolder": false,
        "SeriesId": item.show_id,
        "SeriesName": item.show_title,
        "SeasonId": item.show_id.as_ref().map(|id| format!("{id}:s{}", item.season.unwrap_or(0))),
        "ParentIndexNumber": item.season,
        "IndexNumber": item.episode,
        "ProductionYear": show.and_then(|show| show.year).or(item.year),
        "RunTimeTicks": ticks(item.duration_ms),
        "Container": if item.container == "strm" { "mkv" } else { item.container.as_str() },
        "Overview": show.and_then(|show| show.metadata.as_ref()).and_then(|meta| meta.overview.clone()),
        "ImageTags": image_tag(poster.as_ref()),
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
    Path(_user_id): Path<String>,
) -> Result<Json<Value>, Response> {
    user_of(&state, &headers, &params).await?;
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
    let defs = crate::services::settings::library_defs(&state.settings, &state.config);
    let wanted = defs
        .iter()
        .find(|def| def.id == parent || def.name == parent);
    let library_names: Vec<String> = match wanted {
        Some(def) => vec![def.name.clone(), def.id.clone()],
        None => defs.iter().map(|d| d.name.clone()).collect(),
    };
    let filter_library = wanted.is_some();

    let mut out: Vec<Value> = Vec::new();
    let want_series = include.contains("Series") || parent_is_series(&defs, wanted);
    let want_movies = include.is_empty()
        || include.contains("Movie")
        || (!want_series && wanted.is_some_and(|d| library_type(d) == "movies"));

    if want_movies {
        let all = query(&state.db, |pool| db::list_items(&pool, None)).await?;
        for item in all {
            if !matches!(item.kind, Kind::Movie | Kind::Video) || item.show_id.is_some() {
                continue;
            }
            if filter_library && !library_names.contains(&item.library) {
                continue;
            }
            if !search.is_empty() && !item.title.to_lowercase().contains(&search) {
                continue;
            }
            out.push(movie_dto(&state, &item));
        }
    }
    if want_series {
        let shows = query(&state.db, |pool| db::list_shows(&pool, None)).await?;
        for show in shows {
            if filter_library && !library_names.contains(&show.library) {
                continue;
            }
            if !search.is_empty() && !show.title.to_lowercase().contains(&search) {
                continue;
            }
            out.push(series_dto(&state, &show));
        }
    }
    let total = out.len();
    let page: Vec<Value> = out.into_iter().skip(skip).take(limit).collect();
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
    Path(_user_id): Path<String>,
) -> Result<Json<Value>, Response> {
    user_of(&state, &headers, &params).await?;
    let all = query(&state.db, |pool| db::list_items(&pool, None)).await?;
    let latest: Vec<Value> = all
        .into_iter()
        .filter(|i| matches!(i.kind, Kind::Movie) && i.show_id.is_none())
        .take(20)
        .map(|item| movie_dto(&state, &item))
        .collect();
    Ok(Json(Value::Array(latest)))
}

async fn seasons(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, Response> {
    user_of(&state, &headers, &params).await?;
    let detail = query(&state.db, move |pool| db::get_show(&pool, &id))
        .await?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "show not found"))?;
    let items: Vec<Value> = detail
        .seasons
        .iter()
        .map(|season| {
            json!({
                "Name": format!("Season {}", season.number),
                "Id": format!("{}:s{}", detail.show.id, season.number),
                "ServerId": server_id(&state),
                "Type": "Season",
                "IsFolder": true,
                "SeriesId": detail.show.id,
                "SeriesName": detail.show.title,
                "IndexNumber": season.number,
                "ImageTags": image_tag(detail.show.metadata.as_ref().and_then(|m| m.poster_url.as_ref())),
                "UserData": { "Played": false, "PlaybackPositionTicks": 0 },
            })
        })
        .collect();
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
    user_of(&state, &headers, &params).await?;
    let wanted_season = params.get("season").and_then(|v| v.parse::<u32>().ok());
    let detail = query(&state.db, move |pool| db::get_show(&pool, &id))
        .await?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "show not found"))?;
    let poster = detail
        .show
        .metadata
        .as_ref()
        .and_then(|m| m.poster_url.clone());
    let mut out = Vec::new();
    for season in &detail.seasons {
        if wanted_season.is_some_and(|wanted| wanted != season.number) {
            continue;
        }
        for episode in &season.episodes {
            let name = episode
                .episode_title
                .clone()
                .unwrap_or_else(|| format!("Episode {}", episode.episode.unwrap_or(0)));
            out.push(json!({
                "Name": name,
                "Id": episode.id,
                "ServerId": server_id(&state),
                "Type": "Episode",
                "MediaType": "Video",
                "IsFolder": false,
                "SeriesId": detail.show.id,
                "SeriesName": detail.show.title,
                "SeasonId": format!("{}:s{}", detail.show.id, season.number),
                "ParentIndexNumber": season.number,
                "IndexNumber": episode.episode,
                "ProductionYear": detail.show.year,
                "RunTimeTicks": ticks(episode.duration_ms),
                "Container": if episode.container == "strm" { "mkv" } else { episode.container.as_str() },
                "ImageTags": image_tag(poster.as_ref()),
                "UserData": { "Played": false, "PlaybackPositionTicks": 0 },
            }));
        }
    }
    Ok(Json(json!({ "Items": out, "TotalRecordCount": out.len() })))
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
    match item.kind {
        Kind::Movie | Kind::Video => {}
        _ => {
            return Err(json_error(
                StatusCode::BAD_REQUEST,
                "not a playable movie item",
            ));
        }
    }
    crate::api::visibility::gate_item(&state, &user, &item.id).await?;
    let ticket = crate::services::media_ticket::for_access_token(&state.media_ticket_key, &access);
    let path = item
        .abs_path
        .clone()
        .filter(|p| p.starts_with("http://") || p.starts_with("https://"))
        .unwrap_or_else(|| format!("/api/items/{}/stream?t={ticket}", item.id));
    let path = client_url(&headers, &path);
    let container = if item.container == "strm" {
        "mkv".to_string()
    } else {
        item.container.clone()
    };
    let mut streams = Vec::new();
    if let Some(video) = item.video.as_ref() {
        streams.push(json!({
            "Type": "Video",
            "Index": 0,
            "Codec": video.codec,
            "Width": video.width,
            "Height": video.height,
            "IsDefault": true,
        }));
    }
    Ok(Json(json!({
        "MediaSources": [{
            "Id": item.id,
            "Name": item.title,
            "Path": path,
            "Container": container,
            "RunTimeTicks": ticks(item.duration_ms),
            "Size": Value::Null,
            "SupportsDirectPlay": true,
            "SupportsDirectStream": true,
            "SupportsTranscoding": false,
            "IsRemote": item.abs_path.as_deref().is_some_and(|p| p.starts_with("http")),
            "MediaStreams": streams,
        }],
        "PlaySessionId": item.id,
    })))
}

async fn image(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    Path((id, kind)): Path<(String, String)>,
) -> Result<Response, Response> {
    image_art(&state, &headers, &params, id, kind).await
}

async fn image_index(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    Path((id, kind, _index)): Path<(String, String, String)>,
) -> Result<Response, Response> {
    image_art(&state, &headers, &params, id, kind).await
}

async fn image_art(
    state: &SharedState,
    headers: &HeaderMap,
    params: &HashMap<String, String>,
    id: String,
    kind: String,
) -> Result<Response, Response> {
    user_of(state, headers, params).await?;
    let item = query(&state.db, move |pool| db::get_item(&pool, &id))
        .await?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "item not found"))?;
    let art = item.metadata.as_ref().and_then(|m| {
        if kind.eq_ignore_ascii_case("backdrop") {
            m.backdrop_url.clone()
        } else {
            m.poster_url.clone()
        }
    });
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

async fn empty_items(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    Path(_params): Path<HashMap<String, String>>,
) -> Result<Json<Value>, Response> {
    user_of(&state, &headers, &params).await?;
    Ok(Json(json!({ "Items": [], "TotalRecordCount": 0 })))
}
