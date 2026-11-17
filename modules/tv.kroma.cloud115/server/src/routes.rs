//! The routes this module serves.
//!
//! `/play` is the one a player reaches: the `.strm` entries name it, and it
//! answers with the drive's own signed link so the bytes never pass through this
//! server. Everything else is the module's admin page, gated on the account's
//! settings permission — the cookie is a credential and the sync writes into the
//! library, so neither is open to a plain viewer.

use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use kroma_module_sdk::domain::Permission;
use kroma_module_sdk::host::{json_error, AuthUser, HostCtx};

use crate::client;
use crate::play::links;
use crate::settings::{Config, Library, Options};

pub fn routes<S: HostCtx + Clone + Send + Sync + 'static>() -> Router<S> {
    Router::new()
        .route("/play", get(play::<S>))
        .route("/settings", get(get_settings::<S>).put(put_settings::<S>))
        .route("/status", get(status::<S>))
        .route("/account", get(account::<S>))
        .route("/browse", get(browse::<S>))
        .route("/browse-local", get(browse_local::<S>))
        .route("/sync", post(sync::<S>))
}

// ---------------------------------------------------------------------------
// Playback
#[derive(Deserialize)]
struct PlayQuery {
    pc: String,
}

async fn play<S: HostCtx + Clone + Send + Sync + 'static>(
    State(_host): State<S>,
    Query(query): Query<PlayQuery>,
    headers: HeaderMap,
) -> Response {
    let config = Config::load();
    if config.cookie.trim().is_empty() {
        return json_error(StatusCode::SERVICE_UNAVAILABLE, "115 的 Cookie 还没配置");
    }
    // The drive signs a link to the agent that asks, so the player's own agent is
    // the one to sign with. KROMA relays it to this route.
    let ua = headers
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(client::MODULE_UA);

    match links()
        .resolve(&config.cookie, &query.pc, ua, config.options.link_ttl_secs)
        .await
    {
        Ok(resolved) if resolved.link.needs_cookie => {
            // An `f=3` link only works with the account's cookie attached to every
            // byte request, which a redirect cannot carry and a player will not
            // send. Saying so beats handing out a link that fails on the first
            // read with a bare 403.
            tracing::warn!(pick_code = %query.pc, "a 115 link needs the cookie on fetch; playback refuses it");
            json_error(
                StatusCode::BAD_GATEWAY,
                "该文件（多见于 ISO/BDMV 原盘）的直链要求携带 Cookie，无法直接 302 播放",
            )
        }
        Ok(resolved) => {
            let cached = resolved.cached;
            let link = resolved.link;
            tracing::info!(
                pick_code = %query.pc,
                mode = "302",
                cached,
                url = %link.url,
                "115 direct playback"
            );
            let url = link.url.clone();
            let _ = crate::store::patch(|stored| {
                stored.last_play_error.clear();
                stored.last_play_at = crate::events::unix_now();
                stored.last_play_mode = "302".into();
                stored.last_play_url = url.clone();
                stored.play_count = stored.play_count.saturating_add(1);
                if !cached {
                    stored.sign_count = stored.sign_count.saturating_add(1);
                }
            });
            redirect_to(&url)
        }
        Err(error) if error.cookie_expired() => {
            remember_play_failure("115 登录已失效，请更新 Cookie");
            json_error(StatusCode::BAD_GATEWAY, "115 登录已失效，请更新 Cookie")
        }
        Err(error) if error.rate_limited() => json_error(
            StatusCode::TOO_MANY_REQUESTS,
            "115 正在限流，稍后再试（模块已自动暂停所有调用）",
        ),
        Err(error) => {
            tracing::warn!(pick_code = %query.pc, %error, "signing a 115 link failed");
            remember_play_failure(&format!("签直链失败：{error}"));
            json_error(StatusCode::BAD_GATEWAY, &error.to_string())
        }
    }
}

fn remember_play_failure(reason: &str) {
    let reason = reason.chars().take(200).collect::<String>();
    let _ = crate::store::patch(|stored| {
        stored.last_play_error = reason;
        stored.last_play_at = crate::events::unix_now();
    });
}

fn redirect_to(target: &str) -> Response {
    let Ok(location) = HeaderValue::from_str(target) else {
        return json_error(StatusCode::BAD_GATEWAY, "直链无法作为跳转地址");
    };
    let mut response = (StatusCode::FOUND, ()).into_response();
    let headers = response.headers_mut();
    headers.insert(header::LOCATION, location);
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

// ---------------------------------------------------------------------------
// Admin

fn require_settings<S: HostCtx>(host: &S, user: &kroma_module_sdk::domain::User) -> Result<(), Response> {
    host.require(user, Permission::SettingsManage)
}

async fn account<S: HostCtx + Clone + Send + Sync + 'static>(
    State(host): State<S>,
    AuthUser(user): AuthUser,
) -> Result<Json<serde_json::Value>, Response> {
    require_settings(&host, &user)?;
    let config = Config::load();
    if config.cookie.trim().is_empty() {
        return Err(json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "115 的 Cookie 还没配置",
        ));
    }
    match client::space(&config.cookie, client::MODULE_UA).await {
        Ok(space) => Ok(Json(
            serde_json::to_value(space).unwrap_or_default(),
        )),
        Err(error) if error.cookie_expired() => Err(json_error(
            StatusCode::BAD_GATEWAY,
            "115 登录已失效，请更新 Cookie",
        )),
        Err(error) => Err(json_error(StatusCode::BAD_GATEWAY, &error.to_string())),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FolderView {
    name: String,
    kind: String,
    folders: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsView {
    has_cookie: bool,
    base_url: String,
    libraries: Vec<Library>,
    options: Options,
    library_folders: Vec<FolderView>,
    /// The absolute local directory each mapping resolves to, in the same order
    /// as `libraries`. Empty where no library can take it — that is the answer
    /// to "where is my folder".
    resolved_local: Vec<String>,
}

async fn get_settings<S: HostCtx + Clone + Send + Sync + 'static>(
    State(host): State<S>,
    AuthUser(user): AuthUser,
) -> Result<Json<SettingsView>, Response> {
    require_settings(&host, &user)?;
    let config = Config::load();
    let folders = host.library_folders();
    let resolved_local = config
        .libraries
        .iter()
        .map(|library| {
            crate::sync::resolve_local_path(&folders, &library.local)
                .map(|path| path.display().to_string())
                .unwrap_or_default()
        })
        .collect();
    Ok(Json(SettingsView {
        has_cookie: !config.cookie.is_empty(),
        base_url: config.base_url,
        libraries: config.libraries,
        options: config.options,
        library_folders: folders
            .into_iter()
            .map(|f| FolderView {
                name: f.name,
                kind: f.kind,
                folders: f.folders,
            })
            .collect(),
        resolved_local,
    }))
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct SettingsBody {
    /// A new cookie. Absent leaves the stored one alone; present and empty is the
    /// same as absent, so a password manager that autofills nothing cannot erase
    /// it.
    cookie: Option<String>,
    #[serde(default)]
    clear_cookie: bool,
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    libraries: Option<Vec<Library>>,
    #[serde(default)]
    options: Option<Options>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Saved {
    saved: bool,
    /// Mappings whose local directory could not be resolved or created. The save
    /// itself succeeded; these are what the operator has to fix next.
    errors: Vec<String>,
}

async fn put_settings<S: HostCtx + Clone + Send + Sync + 'static>(
    State(host): State<S>,
    AuthUser(user): AuthUser,
    Json(body): Json<SettingsBody>,
) -> Result<Json<Saved>, Response> {
    require_settings(&host, &user)?;

    if let Some(libraries) = body.libraries.as_ref() {
        for library in libraries {
            if library.remote.trim().is_empty() != library.local.trim().is_empty() {
                return Err(json_error(
                    StatusCode::BAD_REQUEST,
                    "每个映射都要同时填网盘目录和本地子目录",
                ));
            }
        }
    }
    let base = body.base_url.as_deref().map(|base| base.trim().trim_end_matches('/').to_string());
    if let Some(base) = base.as_ref() {
        if !base.is_empty() && !base.starts_with("http://") && !base.starts_with("https://") {
            return Err(json_error(
                StatusCode::BAD_REQUEST,
                "服务器地址必须形如 http://192.168.1.10:8096",
            ));
        }
    }
    let cookie = body
        .cookie
        .as_deref()
        .map(str::trim)
        .filter(|cookie| !cookie.is_empty())
        .map(str::to_string);
    if let Some(cookie) = cookie.as_ref() {
        if cookie.contains(['\r', '\n']) {
            return Err(json_error(StatusCode::BAD_REQUEST, "Cookie 不能包含换行"));
        }
    }

    let touched = body.clear_cookie
        || cookie.is_some()
        || base.is_some()
        || body.libraries.is_some()
        || body.options.is_some();
    if !touched {
        return Ok(Json(Saved {
            saved: false,
            errors: Vec::new(),
        }));
    }

    crate::store::patch(|stored| {
        if body.clear_cookie {
            stored.cookie.clear();
        } else if let Some(cookie) = cookie {
            stored.cookie = cookie;
        }
        if let Some(base) = base {
            stored.base_url = base;
        }
        if let Some(libraries) = body.libraries {
            stored.libraries = libraries;
        }
        if let Some(options) = body.options {
            stored.options = options;
        }
    })
    .map_err(|reason| json_error(StatusCode::INTERNAL_SERVER_ERROR, &reason))?;

    // Make each mapping's local folder now, so it is visible in KROMA's own
    // "add folder" picker before the first sync ever runs.
    let config = Config::load();
    let folders = host.library_folders();
    let mut errors = Vec::new();
    for mapping in &config.libraries {
        if mapping.remote.trim().is_empty() || mapping.local.trim().is_empty() {
            continue;
        }
        match crate::sync::resolve_local_path(&folders, &mapping.local) {
            Some(path) => {
                if let Err(error) = std::fs::create_dir_all(&path) {
                    errors.push(format!("{}: 创建本地目录失败：{error}", path.display()));
                }
            }
            None => errors.push(format!(
                "{}: 本地目录无效（填存储里的绝对路径，如 /media/115/动画电影）",
                mapping.remote
            )),
        }
    }

    Ok(Json(Saved {
        saved: true,
        errors,
    }))
}

// ---------------------------------------------------------------------------
// Browsing the storage side, for the mapping's local folder

#[derive(Deserialize, Default)]
struct LocalBrowseQuery {
    #[serde(default)]
    path: String,
}

async fn browse_local<S: HostCtx + Clone + Send + Sync + 'static>(
    State(host): State<S>,
    AuthUser(user): AuthUser,
    Query(query): Query<LocalBrowseQuery>,
) -> Result<Json<BrowseView>, Response> {
    require_settings(&host, &user)?;
    let raw = if query.path.trim().is_empty() {
        "/media".to_string()
    } else {
        query.path.trim().to_string()
    };
    if raw.contains("..") {
        return Err(json_error(StatusCode::BAD_REQUEST, "路径无效"));
    }
    let dir = std::path::PathBuf::from(&raw);
    let listed = std::fs::read_dir(&dir)
        .map_err(|error| json_error(StatusCode::BAD_REQUEST, &format!("无法读取 {}：{error}", dir.display())))?;
    let mut entries = Vec::new();
    for entry in listed.flatten() {
        if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
            entries.push(BrowseEntry {
                name: entry.file_name().to_string_lossy().to_string(),
                is_dir: true,
                cid: None,
                fid: None,
                pick_code: String::new(),
                size: 0,
            });
        }
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Json(BrowseView {
        total: entries.len() as i64,
        cid: raw,
        entries,
    }))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusView {
    running: bool,
    report: crate::sync::Report,
    link_cache: usize,
    soonest_link_secs: Option<u64>,
    paused_secs: Option<u64>,
    cookie_set: bool,
    base_url: String,
    watch_enabled: bool,
    watch_interval_secs: u64,
    last_poll_at: i64,
    last_events: u64,
    gate_ok: Option<bool>,
    indexed_dirs: usize,
    last_play_error: String,
    last_play_at: i64,
    last_play_mode: String,
    last_play_url: String,
    play_count: u64,
    sign_count: u64,
    history: Vec<crate::store::HistoryEntry>,
}

async fn status<S: HostCtx + Clone + Send + Sync + 'static>(State(_host): State<S>) -> Json<StatusView> {
    let (running, report) = crate::sync::status();
    let (cached, soonest) = links().snapshot().await;
    let stored = crate::store::load();
    let config = Config::load();
    let incr = stored.incr.clone();
    Json(StatusView {
        running,
        report,
        link_cache: cached,
        soonest_link_secs: soonest,
        paused_secs: crate::pacer::pacer()
            .paused_for()
            .await
            .map(|d| d.as_secs()),
        cookie_set: !config.cookie.is_empty(),
        base_url: config.base_url,
        watch_enabled: config.options.watch_enabled,
        watch_interval_secs: config.options.watch_interval_secs,
        last_poll_at: incr.last_poll_at,
        last_events: incr.last_events,
        gate_ok: incr.gate_ok,
        indexed_dirs: stored.dirs.len(),
        last_play_error: stored.last_play_error,
        last_play_at: stored.last_play_at,
        last_play_mode: stored.last_play_mode,
        last_play_url: stored.last_play_url,
        play_count: stored.play_count,
        sign_count: stored.sign_count,
        history: stored.history,
    })
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct SyncBody {
    /// `window.location.origin` from the page that started the run.
    base_url: Option<String>,
}#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Started {
    started: bool,
}

async fn sync<S: HostCtx + Clone + Send + Sync + 'static>(
    State(host): State<S>,
    AuthUser(user): AuthUser,
    body: Option<Json<SyncBody>>,
) -> Result<Json<Started>, Response> {
    require_settings(&host, &user)?;
    let base = body.and_then(|Json(body)| body.base_url);
    match crate::sync::start(host.clone(), base, true) {
        Ok(()) => Ok(Json(Started { started: true })),
        Err(reason) => Err(json_error(StatusCode::CONFLICT, &reason)),
    }
}

// ---------------------------------------------------------------------------
// Browsing the drive, for the admin page's file panel

#[derive(Deserialize, Default)]
struct BrowseQuery {
    /// A folder id; absent is the drive's root.
    #[serde(default)]
    cid: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BrowseEntry {
    name: String,
    is_dir: bool,
    cid: Option<String>,
    fid: Option<String>,
    pick_code: String,
    size: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BrowseView {
    cid: String,
    total: i64,
    entries: Vec<BrowseEntry>,
}

async fn browse<S: HostCtx + Clone + Send + Sync + 'static>(
    State(host): State<S>,
    AuthUser(user): AuthUser,
    Query(query): Query<BrowseQuery>,
) -> Result<Json<BrowseView>, Response> {
    require_settings(&host, &user)?;
    let config = Config::load();
    if config.cookie.trim().is_empty() {
        return Err(json_error(StatusCode::SERVICE_UNAVAILABLE, "115 的 Cookie 还没配置"));
    }
    let cid = match query.cid.trim() {
        "" => "0".to_string(),
        value if value.chars().all(|c| c.is_ascii_digit()) => value.to_string(),
        _ => return Err(json_error(StatusCode::BAD_REQUEST, "目录号无效")),
    };
    match client::list(
        &config.cookie,
        client::MODULE_UA,
        &cid,
        0,
        // One page is what a browser pane shows; the mirror is what walks it all.
        config.options.page_size.min(1000),
    )
    .await
    {
        Ok(page) => Ok(Json(BrowseView {
            cid,
            total: page.total,
            entries: page
                .entries
                .into_iter()
                .map(|entry| BrowseEntry {
                    is_dir: entry.is_dir(),
                    cid: entry.folder_id(),
                    fid: entry.file_id(),
                    pick_code: entry.pick_code,
                    size: entry.size,
                    name: entry.name,
                })
                .collect(),
        })),
        Err(error) if error.cookie_expired() => {
            Err(json_error(StatusCode::BAD_GATEWAY, "115 登录已失效，请更新 Cookie"))
        }
        Err(error) if error.rate_limited() => Err(json_error(
            StatusCode::TOO_MANY_REQUESTS,
            "115 正在限流，稍后再试（模块已自动暂停所有调用）",
        )),
        Err(error) => Err(json_error(StatusCode::BAD_GATEWAY, &error.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_body_that_only_touches_the_cookie_leaves_the_rest_alone() {
        let body: SettingsBody = serde_json::from_str(r#"{"cookie":"UID=1; CID=2"}"#).unwrap();
        assert_eq!(body.cookie.as_deref(), Some("UID=1; CID=2"));
        assert!(body.libraries.is_none() && body.options.is_none());
    }

    #[test]
    fn a_blank_local_side_of_a_mapping_is_refused_by_the_handler_contract() {
        let library = Library {
            remote: "/电影".into(),
            local: String::new(),
        };
        // Mirrors the check in `put_settings`: one side named is a typo, not a
        // mapping.
        assert!(library.remote.trim().is_empty() != library.local.trim().is_empty());
    }
}
