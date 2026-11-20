//! A public link to one title. The link names a row; the row carries an
//! optional access code and an expiry; a visitor who unlocks it gets a cookie
//! that only opens this share. Playback reuses the ordinary byte path, so a
//! share streams exactly like a signed-in player does.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::api::error::json_error;
use crate::api::extract::AuthUser;
use crate::api::util::query;
use crate::api::stream;
use crate::db;
use crate::db::share::Share;
use crate::model::{Permission, User};
use crate::state::SharedState;

const COOKIE: &str = "kroma_share";
const MAX_TRIES: u32 = 5;
const TRY_WINDOW: Duration = Duration::from_secs(10 * 60);

pub fn protected_routes() -> Router<SharedState> {
    Router::new()
        .route("/items/{id}/share", get(current).post(create).delete(revoke))
        .route("/shares", get(list))
}

pub fn public_routes() -> Router<SharedState> {
    Router::new()
        .route("/share/{slug}", get(info))
        .route("/share/{slug}/unlock", post(unlock))
        .route("/share/{slug}/stream", get(stream_share))
}

fn require_manage(user: &User) -> Result<(), Response> {
    if user.can(Permission::UsersManage) {
        Ok(())
    } else {
        Err(json_error(StatusCode::FORBIDDEN, "只有管理员可以分享"))
    }
}

fn now() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

fn hash_password(slug: &str, password: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("{slug}|{password}").as_bytes());
    hex::encode(hasher.finalize())
}

fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|pair| {
            let (key, value) = pair.trim().split_once('=')?;
            (key == name).then(|| value.to_string())
        })
}

fn unlocked(share: &Share, headers: &HeaderMap) -> bool {
    if share.password_hash.is_none() {
        return true;
    }
    cookie(headers, COOKIE)
        .is_some_and(|value| value == format!("{}:{}", share.slug, share.access_key))
}

#[derive(Default)]
struct Attempts {
    at: Option<Instant>,
    count: u32,
}

fn attempts() -> &'static Mutex<HashMap<String, Attempts>> {
    static ATTEMPTS: OnceLock<Mutex<HashMap<String, Attempts>>> = OnceLock::new();
    ATTEMPTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn throttled(slug: &str) -> bool {
    let mut all = attempts()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let entry = all.entry(slug.to_string()).or_default();
    if entry.at.is_some_and(|at| at.elapsed() > TRY_WINDOW) {
        *entry = Attempts::default();
    }
    entry.count >= MAX_TRIES
}

fn note_failure(slug: &str) {
    let mut all = attempts()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let entry = all.entry(slug.to_string()).or_default();
    if entry.at.is_none_or(|at| at.elapsed() > TRY_WINDOW) {
        *entry = Attempts {
            at: Some(Instant::now()),
            count: 0,
        };
    }
    entry.count = entry.count.saturating_add(1);
}

fn forget_failures(slug: &str) {
    attempts()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(slug);
}

async fn item_of(state: &SharedState, id: &str) -> Result<crate::model::MediaItem, Response> {
    let id = id.to_string();
    query(&state.db, move |pool| db::get_item(&pool, &id))
        .await?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "item not found"))
}

fn view(share: &Share, password: Option<&str>) -> serde_json::Value {
    json!({
        "slug": share.slug,
        "path": format!("/s/{}", share.slug),
        "expiresAt": share.expires_at,
        "requiresPassword": share.password_hash.is_some(),
        "viewCount": share.view_count,
        "password": password,
    })
}

/// `GET /api/items/:id/share` → the live link, or `null`.
async fn current(
    State(state): State<SharedState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
) -> Result<Response, Response> {
    require_manage(&user)?;
    let live = query(&state.db, move |pool| {
        db::share::active_for_item(&pool, &id, now())
    })
    .await?;
    Ok(Json(live.map(|share| view(&share, None))).into_response())
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct CreateBody {
    days: Option<u32>,
    password: Option<String>,
}

/// `POST /api/items/:id/share` → the existing live link, or a new one.
async fn create(
    State(state): State<SharedState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
    Json(body): Json<CreateBody>,
) -> Result<Response, Response> {
    require_manage(&user)?;
    let item = item_of(&state, &id).await?;
    let id2 = id.clone();
    let existing = query(&state.db, move |pool| {
        db::share::active_for_item(&pool, &id2, now())
    })
    .await?;
    if let Some(share) = existing {
        return Ok(Json(view(&share, None)).into_response());
    }

    let days = match body.days.unwrap_or(7) {
        1 | 3 | 7 | 30 => body.days.unwrap_or(7),
        _ => 7,
    };
    let password = body
        .password
        .as_deref()
        .map(str::trim)
        .filter(|password| !password.is_empty())
        .map(str::to_string);
    if let Some(password) = password.as_deref() {
        if !(4..=32).contains(&password.chars().count()) {
            return Err(json_error(StatusCode::BAD_REQUEST, "密码需要 4–32 位"));
        }
    }

    let created = now();
    let slug = crate::services::auth::random_token()
        .chars()
        .take(16)
        .collect::<String>();
    let share = Share {
        id: crate::services::scan::short_hash(&format!("share|{slug}")),
        slug: slug.clone(),
        item_id: item.id.clone(),
        created_by: user.id.clone(),
        password_hash: password
            .as_deref()
            .map(|password| hash_password(&slug, password)),
        password_version: 1,
        access_key: crate::services::auth::random_token()
            .chars()
            .take(32)
            .collect(),
        expires_at: created + i64::from(days) * 86_400,
        revoked_at: None,
        view_count: 0,
        last_accessed_at: None,
        created_at: created,
    };
    let stored = share.clone();
    query(&state.db, move |pool| db::share::insert(&pool, &stored)).await?;
    Ok(Json(view(&share, password.as_deref())).into_response())
}

/// `DELETE /api/items/:id/share` → revoke the live link.
async fn revoke(
    State(state): State<SharedState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
) -> Result<Response, Response> {
    require_manage(&user)?;
    let id2 = id.clone();
    let live = query(&state.db, move |pool| {
        db::share::active_for_item(&pool, &id2, now())
    })
    .await?;
    if let Some(share) = live {
        let slug = share.slug;
        query(&state.db, move |pool| db::share::revoke(&pool, &slug, now())).await?;
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `GET /api/shares` → every live link, for the management list.
async fn list(
    State(state): State<SharedState>,
    AuthUser(user): AuthUser,
) -> Result<Response, Response> {
    require_manage(&user)?;
    let shares = query(&state.db, move |pool| db::share::live(&pool, now())).await?;
    let mut out = Vec::with_capacity(shares.len());
    for share in shares {
        let item = item_of(&state, &share.item_id).await.ok();
        out.push(json!({
            "slug": share.slug,
            "path": format!("/s/{}", share.slug),
            "itemId": share.item_id,
            "title": item.as_ref().map(|item| item.title.clone()),
            "year": item.as_ref().and_then(|item| item.year),
            "posterUrl": item.as_ref().and_then(|item| item.metadata.as_ref()).and_then(|meta| meta.poster_url.clone()),
            "expiresAt": share.expires_at,
            "requiresPassword": share.password_hash.is_some(),
            "viewCount": share.view_count,
        }));
    }
    Ok(Json(out).into_response())
}

/// `GET /api/share/:slug` → what the visitor may know. Before the password is
/// given, that is only "there is a password".
async fn info(
    State(state): State<SharedState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let now = now();
    let share = {
        let slug = slug.clone();
        query(&state.db, move |pool| db::share::by_slug(&pool, &slug)).await?
    }
    .filter(|share| share.is_live(now))
    .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "分享不存在或已过期"))?;
    let needs = share.password_hash.is_some();
    if needs && !unlocked(&share, &headers) {
        return Ok(Json(json!({
            "requiresPassword": true,
            "unlocked": false,
            "expiresAt": share.expires_at,
        }))
        .into_response());
    }
    let item = item_of(&state, &share.item_id).await?;
    let slug2 = share.slug.clone();
    let _ = query(&state.db, move |pool| db::share::touch(&pool, &slug2, now)).await;
    let meta = item.metadata.as_ref();
    Ok(Json(json!({
        "requiresPassword": needs,
        "unlocked": true,
        "expiresAt": share.expires_at,
        "title": item.title,
        "year": item.year,
        "overview": meta.and_then(|meta| meta.overview.clone()),
        "posterUrl": meta.and_then(|meta| meta.poster_url.clone()),
        "kind": item.kind,
        "hasFile": item.abs_path.is_some() || item.files.iter().any(|file| file.abs_path.is_some()),
    }))
    .into_response())
}

#[derive(Deserialize)]
struct UnlockBody {
    password: String,
}

/// `POST /api/share/:slug/unlock` → sets the share cookie when the code is right.
async fn unlock(
    State(state): State<SharedState>,
    Path(slug): Path<String>,
    Json(body): Json<UnlockBody>,
) -> Result<Response, Response> {
    let now = now();
    let share = {
        let slug = slug.clone();
        query(&state.db, move |pool| db::share::by_slug(&pool, &slug)).await?
    }
    .filter(|share| share.is_live(now))
    .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "分享不存在或已过期"))?;
    let Some(expected) = share.password_hash.as_deref() else {
        return Ok(Json(json!({ "unlocked": true })).into_response());
    };
    if throttled(&share.slug) {
        return Err(json_error(StatusCode::TOO_MANY_REQUESTS, "尝试次数过多，稍后再试"));
    }
    if hash_password(&share.slug, body.password.trim()) != expected {
        note_failure(&share.slug);
        return Err(json_error(StatusCode::UNAUTHORIZED, "密码不对"));
    }
    forget_failures(&share.slug);
    let cookie = format!(
        "{COOKIE}={}:{}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}",
        share.slug,
        share.access_key,
        (share.expires_at - now).max(0)
    );
    let mut response = Json(json!({ "unlocked": true })).into_response();
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
    Ok(response)
}

/// `GET /api/share/:slug/stream` → the file itself, once the link is unlocked.
async fn stream_share(
    State(state): State<SharedState>,
    Path(slug): Path<String>,
    Query(q): Query<stream::StreamQuery>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let now = now();
    let share = {
        let slug = slug.clone();
        query(&state.db, move |pool| db::share::by_slug(&pool, &slug)).await?
    }
    .filter(|share| share.is_live(now))
    .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "分享不存在或已过期"))?;
    if !unlocked(&share, &headers) {
        return Err(json_error(StatusCode::UNAUTHORIZED, "需要先输入访问密码"));
    }
    let item = item_of(&state, &share.item_id).await?;
    Ok(stream::serve_selected_file(&state, &item, q.file.as_deref(), &headers, addr).await)
}
