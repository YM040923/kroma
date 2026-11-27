//! The stable STRM playback URL and the module's SettingsManage-only controls.
use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use kroma_module_sdk::domain::Permission;
use kroma_module_sdk::host::{json_error, AuthUser, HostCtx};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    client, login,
    play::links,
    settings::{Config, Library, Options},
    store,
};

pub fn routes<S: HostCtx + Clone + Send + Sync + 'static>() -> Router<S> {
    Router::new()
        .route("/play", get(play::<S>))
        .route("/settings", get(get_settings::<S>).put(put_settings::<S>))
        .route("/status", get(status::<S>))
        .route("/account", get(account::<S>))
        .route("/account/alias", post(account_alias::<S>))
        .route("/account/remove", post(account_remove::<S>))
        .route("/watch/now", post(watch_now::<S>))
        .route("/checkpoint/clear", post(checkpoint_clear::<S>))
        .route("/config/export", get(config_export::<S>))
        .route("/config/import", post(config_import::<S>))
        .route("/browse", get(browse::<S>))
        .route("/sync", post(sync::<S>))
        .route("/login/start", post(login_start::<S>))
        .route("/login/state", get(login_state::<S>))
        .route("/login/cancel", post(login_cancel::<S>))
        .route("/upload/rapid", post(crate::upload::rapid::<S>))
        .route("/upload/start", post(crate::upload::start::<S>))
        .route("/upload/part", post(crate::upload::part::<S>))
        .route("/upload/finish", post(crate::upload::finish::<S>))
        .route("/replace/preview", post(replace_preview::<S>))
        .route("/replace/run", post(replace_run::<S>))
}

fn require_settings<S: HostCtx>(
    host: &S,
    user: &kroma_module_sdk::domain::User,
) -> Result<(), Response> {
    host.require(user, Permission::SettingsManage)
}
fn not_authorized() -> Response {
    json_error(StatusCode::CONFLICT, "请先填写 AppID，用 115 App 扫码授权")
}
fn drive_error(e: client::ApiError) -> Response {
    let status = if e.rate_limited() {
        StatusCode::TOO_MANY_REQUESTS
    } else {
        StatusCode::BAD_GATEWAY
    };
    json_error(status, &e.sync_reason())
}
fn internal(e: String) -> Response {
    json_error(StatusCode::INTERNAL_SERVER_ERROR, &e)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LoginStartBody {
    app_id: String,
}
#[derive(Deserialize)]
struct FlowQuery {
    flow: String,
}

async fn login_start<S: HostCtx>(
    State(host): State<S>,
    AuthUser(user): AuthUser,
    Json(body): Json<LoginStartBody>,
) -> Result<Json<login::Started>, Response> {
    require_settings(&host, &user)?;
    let id = body.app_id.trim();
    if id.is_empty() || id.len() > 32 || !id.bytes().all(|b| b.is_ascii_digit()) {
        return Err(json_error(StatusCode::BAD_REQUEST, "AppID 必须是数字"));
    }
    login::begin(id)
        .map(Json)
        .map_err(|e| json_error(StatusCode::BAD_GATEWAY, &e))
}
async fn login_state<S: HostCtx>(
    State(host): State<S>,
    AuthUser(user): AuthUser,
    Query(q): Query<FlowQuery>,
) -> Result<Json<login::State>, Response> {
    require_settings(&host, &user)?;
    login::state(&q.flow)
        .map(Json)
        .map_err(|e| json_error(StatusCode::BAD_GATEWAY, &e))
}
async fn login_cancel<S: HostCtx>(
    State(host): State<S>,
    AuthUser(user): AuthUser,
    Json(q): Json<FlowQuery>,
) -> Result<StatusCode, Response> {
    require_settings(&host, &user)?;
    login::cancel(&q.flow);
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct PlayQuery {
    pc: String,
    next: Option<String>,
}
async fn play<S: HostCtx>(
    State(_host): State<S>,
    Query(q): Query<PlayQuery>,
    headers: HeaderMap,
) -> Response {
    let config = Config::load();
    if config.auth_id.is_empty() {
        return not_authorized();
    }
    crate::pacer::pacer().configure(&config.options).await;
    let ua = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(client::MODULE_UA);
    match links()
        .resolve_play(&config.auth_id, &q.pc, ua, config.options.link_ttl_secs)
        .await
    {
        Ok(resolved) if resolved.link.needs_cookie => json_error(
            StatusCode::BAD_GATEWAY,
            "115 未给出可供播放器直接访问的链接",
        ),
        Ok(resolved) => {
            let Ok(location) = HeaderValue::from_str(&resolved.link.url) else {
                return json_error(StatusCode::BAD_GATEWAY, "115 返回了无效的下载地址");
            };
            if login::authorization_id() != config.auth_id {
                return not_authorized();
            }
            let codes: Vec<String> = q
                .next
                .as_deref()
                .unwrap_or("")
                .split(',')
                .filter(|v| {
                    !v.is_empty() && v.len() <= 64 && v.bytes().all(|b| b.is_ascii_alphanumeric())
                })
                .take(usize::from(config.options.prefetch_next.min(3)))
                .map(str::to_owned)
                .collect();
            let limit = usize::from(config.options.prefetch_next.min(3));
            if limit != 0 {
                let ua = ua.to_string();
                tokio::spawn(async move {
                    let codes = if q.next.is_none() {
                        crate::watch::following(&config, &q.pc, limit)
                    } else {
                        codes
                    };
                    for pc in codes {
                        if links()
                            .resolve(&config.auth_id, &pc, &ua, config.options.link_ttl_secs)
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                });
            }
            let mut r = StatusCode::FOUND.into_response();
            let _ = store::patch(|s| {
                s.last_play_error.clear();
                s.last_play_at = crate::events::unix_now();
                s.last_play_mode = "302".into();
                s.play_count = s.play_count.saturating_add(1);
                if !resolved.cached {
                    s.sign_count = s.sign_count.saturating_add(1);
                }
            });
            r.headers_mut().insert(header::LOCATION, location);
            r.headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            r
        }
        Err(e) => {
            let _ = store::patch(|s| {
                s.last_play_error = e.sync_reason();
                s.last_play_at = crate::events::unix_now();
            });
            drive_error(e)
        }
    }
}

#[derive(Deserialize, Default)]
struct AccountQuery {
    refresh: Option<u8>,
}
async fn account<S: HostCtx>(
    State(host): State<S>,
    AuthUser(user): AuthUser,
    Query(q): Query<AccountQuery>,
) -> Result<Json<Value>, Response> {
    require_settings(&host, &user)?;
    let grant = login::authorization_id();
    if grant.is_empty() {
        return Err(not_authorized());
    }
    let s = store::load();
    let profile = if q.refresh.unwrap_or(0) == 0
        && s.account_grant == grant
        && !s.account.user_name.is_empty()
        && crate::events::unix_now().saturating_sub(s.account.fetched_at) < 600
    {
        s.account
    } else {
        let fresh = client::account(&grant).await.map_err(drive_error)?;
        if login::authorization_id() != grant {
            return Err(not_authorized());
        }
        store::patch(|s| {
            s.account = fresh.clone();
            s.account_grant = grant;
        })
        .map_err(internal)?;
        fresh
    };
    Ok(Json(
        json!({"userId":profile.user_id,"userName":profile.user_name,"avatarUrl":profile.avatar_url,"vip":profile.vip,"total":profile.total,"used":profile.used,"remain":profile.remain,"totalText":profile.total_text,"usedText":profile.used_text,"remainText":profile.remain_text,"alias":s.alias}),
    ))
}
#[derive(Deserialize)]
struct AliasBody {
    alias: String,
}
async fn account_alias<S: HostCtx>(
    State(host): State<S>,
    AuthUser(user): AuthUser,
    Json(body): Json<AliasBody>,
) -> Result<Json<Value>, Response> {
    require_settings(&host, &user)?;
    let s = store::patch(|s| s.alias = body.alias.trim().chars().take(40).collect())
        .map_err(internal)?;
    Ok(Json(json!({"alias":s.alias})))
}
async fn account_remove<S: HostCtx>(
    State(host): State<S>,
    AuthUser(user): AuthUser,
) -> Result<Json<Value>, Response> {
    require_settings(&host, &user)?;
    login::disconnect().map_err(internal)?;
    store::patch(|s| {
        s.account = Default::default();
        s.account_grant.clear();
        s.alias.clear();
        s.last_play_error.clear();
        s.last_play_mode.clear();
        s.last_play_at = 0;
        s.play_count = 0;
        s.sign_count = 0;
    })
    .map_err(internal)?;
    Ok(Json(json!({"removed":true,"filesDeleted":false})))
}

async fn watch_now<S: HostCtx + Clone + Send + Sync + 'static>(
    State(host): State<S>,
    AuthUser(user): AuthUser,
) -> Result<Json<Value>, Response> {
    require_settings(&host, &user)?;
    let c = Config::load();
    if c.auth_id.is_empty() {
        return Err(not_authorized());
    }
    if !c.runnable()
        || !c
            .libraries
            .iter()
            .any(|l| l.fast_sync && !l.local.is_empty() && !l.remote.is_empty())
    {
        return Err(json_error(
            StatusCode::CONFLICT,
            "请先设置服务器地址，并为至少一个映射启用快速检查",
        ));
    }
    crate::watch::start_now(host);
    Ok(Json(json!({"started":true})))
}
async fn checkpoint_clear<S: HostCtx>(
    State(host): State<S>,
    AuthUser(user): AuthUser,
) -> Result<Json<Value>, Response> {
    require_settings(&host, &user)?;
    crate::watch::clear_checkpoint().map_err(|s| json_error(StatusCode::CONFLICT, &s))?;
    Ok(Json(json!({"cleared":true})))
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct ConfigBody {
    base_url: Option<String>,
    libraries: Option<Vec<Library>>,
    options: Option<Options>,
    alias: Option<String>,
}
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
struct SettingsBody {
    base_url: Option<String>,
    libraries: Option<Vec<Library>>,
    options: Option<Options>,
}
fn normalize_base(raw: Option<String>) -> Result<Option<String>, Response> {
    let base = raw.map(|v| v.trim().trim_end_matches('/').to_owned());
    if let Some(v) = &base {
        if !v.is_empty()
            && ((!v.starts_with("http://") && !v.starts_with("https://"))
                || v.contains(['\r', '\n']))
        {
            return Err(json_error(
                StatusCode::BAD_REQUEST,
                "服务器地址必须形如 http://192.168.1.10:4041",
            ));
        }
    }
    Ok(base)
}
fn portable(mut l: Library) -> Library {
    l.local.clear();
    l.local_display.clear();
    l
}
async fn config_export<S: HostCtx>(
    State(host): State<S>,
    AuthUser(user): AuthUser,
) -> Result<Json<Value>, Response> {
    require_settings(&host, &user)?;
    let s = store::load();
    Ok(Json(
        json!({"format":"kroma-cloud115","version":2,"exportedAt":store::now_ms(),"baseUrl":s.base_url,"libraries":s.libraries.into_iter().map(portable).collect::<Vec<_>>(),"options":s.options,"alias":s.alias}),
    ))
}
async fn config_import<S: HostCtx>(
    State(host): State<S>,
    AuthUser(user): AuthUser,
    Json(b): Json<ConfigBody>,
) -> Result<Json<Value>, Response> {
    require_settings(&host, &user)?;
    let base = normalize_base(b.base_url)?;
    let folders = b.libraries.is_some();
    store::patch(|s| {
        if let Some(v) = base {
            s.base_url = v;
        }
        if let Some(v) = b.options {
            s.options = v;
        }
        if let Some(v) = b.alias {
            s.alias = v.trim().chars().take(40).collect();
        }
        if let Some(v) = b.libraries {
            s.libraries = v.into_iter().map(portable).collect();
        }
    })
    .map_err(internal)?;
    let restart = folders
        && host
            .commit_selected_folders(crate::MODULE_ID, &[])
            .map_err(|e| json_error(StatusCode::CONFLICT, &e))?;
    Ok(Json(
        json!({"saved":true,"errors":[],"restartRequired":restart}),
    ))
}
async fn get_settings<S: HostCtx>(
    State(host): State<S>,
    AuthUser(user): AuthUser,
) -> Result<Json<Value>, Response> {
    require_settings(&host, &user)?;
    let c = Config::load();
    let dirs = c
        .libraries
        .iter()
        .map(|l| &l.local_display)
        .collect::<Vec<_>>();
    let folders = host
        .library_folders()
        .into_iter()
        .map(|f| json!({"name":f.name,"kind":f.kind,"folders":f.folders}))
        .collect::<Vec<_>>();
    Ok(Json(
        json!({"authorized":!c.auth_id.is_empty(),"appId":login::app_id(),"baseUrl":c.base_url,"libraries":c.libraries,"options":c.options,"libraryFolders":folders,"resolvedLocal":dirs}),
    ))
}
async fn put_settings<S: HostCtx>(
    State(host): State<S>,
    AuthUser(user): AuthUser,
    Json(b): Json<SettingsBody>,
) -> Result<Json<Value>, Response> {
    require_settings(&host, &user)?;
    if let Some(rows) = &b.libraries {
        for l in rows {
            if l.remote.trim().is_empty() != l.local.trim().is_empty() {
                return Err(json_error(
                    StatusCode::BAD_REQUEST,
                    "每个映射都要同时选择网盘目录和本地目录",
                ));
            }
            if !l.local.is_empty() && crate::sync::resolve_local_path(&[], &l.local).is_none() {
                return Err(json_error(
                    StatusCode::BAD_REQUEST,
                    "请重新选择本地目录，旧路径没有授权给模块",
                ));
            }
        }
    }
    let base = normalize_base(b.base_url)?;
    let changed = b.libraries.is_some();
    let touched = changed || base.is_some() || b.options.is_some();
    if !touched {
        return Ok(Json(
            json!({"saved":false,"errors":[],"restartRequired":false}),
        ));
    }
    let s = store::patch(|s| {
        if let Some(v) = base {
            s.base_url = v;
        }
        if let Some(v) = b.libraries {
            s.libraries = v;
        }
        if let Some(v) = b.options {
            s.options = v;
        }
    })
    .map_err(internal)?;
    let restart = if changed {
        let selected = s
            .libraries
            .into_iter()
            .map(|l| l.local)
            .filter(|v| !v.is_empty())
            .collect::<Vec<_>>();
        host.commit_selected_folders(crate::MODULE_ID, &selected)
            .map_err(|e| json_error(StatusCode::CONFLICT, &e))?
    } else {
        false
    };
    Ok(Json(
        json!({"saved":true,"errors":[],"restartRequired":restart}),
    ))
}

async fn status<S: HostCtx>(
    State(host): State<S>,
    AuthUser(user): AuthUser,
) -> Result<Json<Value>, Response> {
    require_settings(&host, &user)?;
    let (running, report, history) = crate::sync::status();
    let (link_count, soonest) = links().snapshot().await;
    let paused = crate::pacer::pacer()
        .paused_for()
        .await
        .map(|v| v.as_secs());
    let s = store::load();
    let fast = crate::watch::snapshot();
    Ok(Json(
        json!({"running":running,"report":report,"history":history,"authorized":!login::authorization_id().is_empty(),"baseUrl":s.base_url,"watchEnabled":s.options.watch_enabled,"watchIntervalSecs":s.options.watch_interval_secs,"fastSync":fast,"linkCache":link_count,"soonestLinkSecs":soonest,"pausedSecs":paused,"lastPlayAt":s.last_play_at,"lastPlayMode":s.last_play_mode,"lastPlayUrl":"","lastPlayError":s.last_play_error,"playCount":s.play_count,"signCount":s.sign_count,"alias":s.alias,"dailyCallBudget":s.options.daily_call_budget,"callsToday":crate::budget::calls_today(),"autoSyncHours":s.options.auto_sync_hours,"lastAutoSyncAt":s.last_auto_sync_at}),
    ))
}
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct SyncBody {
    base_url: Option<String>,
}
async fn sync<S: HostCtx + Clone + Send + Sync + 'static>(
    State(host): State<S>,
    AuthUser(user): AuthUser,
    body: Option<Json<SyncBody>>,
) -> Result<Json<Value>, Response> {
    require_settings(&host, &user)?;
    let base = normalize_base(body.and_then(|b| b.0.base_url))?;
    crate::sync::start(host, base, true).map_err(|e| json_error(StatusCode::CONFLICT, &e))?;
    Ok(Json(json!({"started":true})))
}
async fn replace_preview<S: HostCtx>(
    State(host): State<S>,
    AuthUser(user): AuthUser,
) -> Result<Json<crate::replace::ReplaceReport>, Response> {
    require_settings(&host, &user)?;
    Ok(Json(crate::replace::preview(&Config::load())))
}
async fn replace_run<S: HostCtx>(
    State(host): State<S>,
    AuthUser(user): AuthUser,
) -> Result<Json<crate::replace::ReplaceReport>, Response> {
    require_settings(&host, &user)?;
    crate::replace::run(&Config::load())
        .map(Json)
        .map_err(|e| json_error(StatusCode::CONFLICT, &e))
}
#[derive(Deserialize, Default)]
#[serde(default)]
struct BrowseQuery {
    cid: String,
    offset: u32,
}
async fn browse<S: HostCtx>(
    State(host): State<S>,
    AuthUser(user): AuthUser,
    Query(q): Query<BrowseQuery>,
) -> Result<Json<Value>, Response> {
    require_settings(&host, &user)?;
    let c = Config::load();
    if c.auth_id.is_empty() {
        return Err(not_authorized());
    }
    let cid = match q.cid.trim() {
        "" => "0",
        s if s.len() <= 24 && s.bytes().all(|b| b.is_ascii_digit()) => s,
        _ => return Err(json_error(StatusCode::BAD_REQUEST, "目录号无效")),
    };
    let p = client::list(
        &c.auth_id,
        client::MODULE_UA,
        cid,
        q.offset,
        c.options.page_size.min(1150),
    )
    .await
    .map_err(drive_error)?;
    let rows=p.entries.into_iter().map(|e|json!({"isDir":e.is_dir(),"cid":e.folder_id(),"fid":e.file_id(),"pickCode":e.pick_code,"size":e.size,"name":e.name})).collect::<Vec<_>>();
    Ok(Json(json!({"cid":cid,"total":p.total,"entries":rows})))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_refuse_legacy_credentials() {
        assert!(serde_json::from_str::<SettingsBody>(r#"{"cookie":"not-a-secret"}"#).is_err());
        assert!(serde_json::from_str::<SettingsBody>(r#"{"accessToken":"not-a-secret"}"#).is_err());
    }
    #[test]
    fn portable_mapping_keeps_quick_check_but_never_transfers_authority() {
        let l: Library = serde_json::from_value(
            json!({"remote":"/日漫","local":"/selected/x","localDisplay":"E:\\test"}),
        )
        .unwrap();
        let p = portable(l);
        assert!(p.local.is_empty() && p.local_display.is_empty() && p.fast_sync);
    }
}
