//! Module management for the admin console. State persists in the settings store
//! under the `moduleStates` blob (see `kroma_engine::modules`).

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::{Component, Path as FsPath, PathBuf};
use std::sync::Arc;

use axum::extract::{ConnectInfo, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Extension, Json, Router};
use kroma_module_supervisor::Supervisor;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::api::error::json_error;
use crate::api::extract::AuthUser;
use crate::model::Permission;
use crate::state::SharedState;

/// Paths are relative to the `/api/admin` nest.
pub fn routes() -> Router<SharedState> {
    Router::new()
        .route("/modules", get(list_modules))
        .route("/modules/{id}/enabled", post(set_enabled))
        .route("/modules/{id}/config", put(set_config))
        .route("/modules/{id}/restart", post(restart_module))
        .route("/modules/{id}/pick-folder", post(pick_module_folder))
        .route("/modules/{id}/reinstall", post(reinstall_module))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminModule {
    #[serde(flatten)]
    manifest: kroma_module_manifest::ModuleManifest,
    enabled: bool,
    config_values: BTreeMap<String, Value>,
    removable: bool,
    /// Where this module came from, and whether a local build has replaced its
    /// component since. `None` for a compile-time module (it came from the server).
    #[serde(skip_serializing_if = "Option::is_none")]
    origin: Option<kroma_module_supervisor::Origin>,
    /// A module the supervisor is running right now.
    running: bool,
    /// This module ships code to run at all, a native build that will not start
    /// included. `false` for a library module, whose code is co-linked into
    /// another module: it is never "running", and reporting it as stopped would
    /// be a false alarm.
    has_sidecar: bool,
    /// Points this module `consumes` that no ENABLED module answers, as `point` or
    /// `point#id`. A module with entries here is installed, running and inert,
    /// which is otherwise silent: it answers nothing useful and nothing says why.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    unmet: Vec<String>,
}

async fn list_modules(
    State(state): State<SharedState>,
    Extension(supervisor): Extension<Arc<Supervisor>>,
    AuthUser(user): AuthUser,
) -> Result<Response, Response> {
    super::require(&user, Permission::SettingsManage)?;
    // Runtime-installed `.kmod` modules are removable; compile-time ones are not.
    let removable_ids: std::collections::HashSet<String> =
        kroma_module_kernel::installed_ids(&state)
            .into_iter()
            .collect();
    let all = kroma_module_kernel::manifests(&state);
    let answered = crate::api::modules::answered_by(&all, &state);
    let mods: Vec<AdminModule> = all
        .into_iter()
        .map(|m| {
            let enabled = kroma_engine::modules::module_enabled(&state.settings, &m.id);
            let unmet = crate::api::modules::unmet_of(&m, &answered);
            let removable = removable_ids.contains(&m.id);
            let stored = kroma_engine::modules::module_config(&state.settings, &m.id);
            let config_values = m
                .config
                .iter()
                .map(|f| {
                    let value = stored.get(&f.key).cloned().unwrap_or_else(|| {
                        f.default.clone().map(Value::from).unwrap_or(Value::Null)
                    });
                    (f.key.clone(), value)
                })
                .collect();
            let origin = removable.then(|| supervisor.origin(&m.id));
            let running = supervisor.is_running(&m.id);
            let has_sidecar =
                removable && (supervisor.has_component(&m.id) || supervisor.is_native_build(&m.id));
            AdminModule {
                manifest: m,
                enabled,
                config_values,
                removable,
                origin,
                running,
                has_sidecar,
                unmet,
            }
        })
        .collect();
    Ok(Json(mods).into_response())
}

#[derive(Deserialize)]
struct EnabledBody {
    enabled: bool,
}

async fn set_enabled(
    State(state): State<SharedState>,
    Extension(sup): Extension<Arc<Supervisor>>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
    Json(body): Json<EnabledBody>,
) -> Result<Response, Response> {
    super::require(&user, Permission::SettingsManage)?;
    kroma_engine::modules::set_module_enabled(&state.settings, &state.db, &id, body.enabled);
    // Drive the module's lifecycle so the toggle starts/stops its live services,
    // not just its listing flag.
    if let Some(module) = kroma_module_kernel::find_server(&id) {
        if body.enabled {
            module.on_enable(state.clone()).await;
        } else {
            module.on_disable(state.clone()).await;
        }
    }
    // A runtime-installed module follows the toggle too: it comes up or goes
    // down now, not at the next server restart. A failed start is
    // reported but does not undo the flag: the admin asked for "enabled".
    let mut warning = None;
    if sup.installed_ids().iter().any(|installed| installed == &id) {
        let sup = sup.clone();
        let module_id = id.clone();
        let enabled = body.enabled;
        let outcome = tokio::task::spawn_blocking(move || {
            if enabled {
                sup.start_installed(&module_id)
            } else {
                sup.stop(&module_id);
                Ok(())
            }
        })
        .await;
        match outcome {
            Ok(Ok(())) => {}
            Ok(Err(e)) => warning = Some(format!("{e:#}")),
            Err(_) => warning = Some("module lifecycle task panicked".to_string()),
        }
    }
    state
        .events
        .publish_value(json!({ "type": "module.changed", "id": id, "enabled": body.enabled }));
    Ok(Json(json!({ "id": id, "enabled": body.enabled, "warning": warning })).into_response())
}

async fn set_config(
    State(state): State<SharedState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
    Json(values): Json<BTreeMap<String, Value>>,
) -> Result<Response, Response> {
    super::require(&user, Permission::SettingsManage)?;
    // Allow-list against the manifest, so a client can only write fields the module
    // actually declares.
    let allowed: std::collections::HashSet<String> = kroma_module_kernel::manifests(&state)
        .into_iter()
        .find(|m| m.id == id)
        .map(|m| m.config.into_iter().map(|f| f.key).collect())
        .unwrap_or_default();
    let map: serde_json::Map<String, Value> = values
        .into_iter()
        .filter(|(k, _)| allowed.contains(k))
        .collect();
    kroma_engine::modules::set_module_config(&state.settings, &state.db, &id, map);
    Ok(Json(json!({ "id": id, "ok": true })).into_response())
}

fn bad(msg: &str) -> Response {
    (StatusCode::BAD_REQUEST, msg.to_string()).into_response()
}

fn is_local_folder_picker_request(peer: SocketAddr, headers: &HeaderMap) -> bool {
    if !peer.ip().is_loopback() {
        return false;
    }
    let Some(host) = headers
        .get(axum::http::header::HOST)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<axum::http::uri::Authority>().ok())
        .map(|authority| authority.host().trim_matches(['[', ']']).to_string())
    else {
        return false;
    };
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PickedFolder {
    selected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    guest_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    display_path: Option<String>,
}

/// Opens the native folder picker only for a local administrator request. The
/// selected directory is stored as a pending, module-scoped grant; it does not
/// reach the running component until its settings save and module restart.
async fn pick_module_folder(
    State(state): State<SharedState>,
    AuthUser(user): AuthUser,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, Response> {
    super::require(&user, Permission::SettingsManage)?;
    if !is_local_folder_picker_request(peer, &headers) {
        return Err(json_error(
            StatusCode::FORBIDDEN,
            "请在运行 KROMA 的 Windows 电脑上通过 localhost 或 127.0.0.1 打开此页，再选择文件夹。",
        ));
    }
    if id != "tv.kroma.cloud115" {
        return Err((StatusCode::NOT_FOUND, "module folder picker not found").into_response());
    }
    let declares_selected = kroma_module_kernel::manifests(&state)
        .into_iter()
        .find(|manifest| manifest.id == id)
        .and_then(|manifest| manifest.permissions)
        .is_some_and(|permissions| {
            permissions
                .fs
                .read
                .contains(&kroma_module_manifest::FsRoot::Selected)
                || permissions
                    .fs
                    .write
                    .contains(&kroma_module_manifest::FsRoot::Selected)
        });
    if !declares_selected {
        return Err((
            StatusCode::CONFLICT,
            "module does not declare selected-folder access",
        )
            .into_response());
    }

    let picked = tokio::task::spawn_blocking(crate::boot::folder_picker::pick_folder)
        .await
        .map_err(|_| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Windows 文件夹选择器未能启动",
            )
        })?
        .map_err(|error| json_error(StatusCode::CONFLICT, &error))?;
    let Some(display_path) = picked else {
        return Ok(Json(PickedFolder {
            selected: false,
            guest_path: None,
            display_path: None,
        })
        .into_response());
    };
    let path = validate_selected_host_path(&state, &display_path)?;
    let guest_path = format!("/selected/{}", uuid::Uuid::new_v4());
    kroma_engine::modules::add_pending_selected_folder(
        &state.settings,
        &state.db,
        &id,
        guest_path.clone(),
        path.to_string_lossy().into_owned(),
    )
    .map_err(|error| json_error(StatusCode::BAD_REQUEST, &error))?;
    Ok(Json(PickedFolder {
        selected: true,
        guest_path: Some(guest_path),
        display_path: Some(display_path.to_string_lossy().into_owned()),
    })
    .into_response())
}

fn validate_selected_host_path(state: &SharedState, path: &FsPath) -> Result<PathBuf, Response> {
    let canonical = path.canonicalize().map_err(|error| {
        json_error(
            StatusCode::BAD_REQUEST,
            &format!("无法打开所选文件夹：{error}"),
        )
    })?;
    if !canonical.is_dir()
        || !canonical
            .components()
            .any(|component| matches!(component, Component::Normal(_)))
    {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "请在磁盘中选择一个文件夹，而不是整个磁盘。",
        ));
    }
    let executable_dir = std::env::current_exe()
        .ok()
        .and_then(|executable| executable.parent().map(FsPath::to_path_buf));
    let protected = std::iter::once(state.config.data_dir.clone()).chain(executable_dir);
    if protected.into_iter().any(|root| {
        let root = root.canonicalize().unwrap_or(root);
        canonical.starts_with(&root) || root.starts_with(&canonical)
    }) {
        return Err(json_error(
            StatusCode::FORBIDDEN,
            "不能授权 KROMA 的程序目录、数据目录或它们的上级目录。请选择其他媒体文件夹。",
        ));
    }
    Ok(canonical)
}

/// `POST /admin/modules/{id}/restart`: stop the module and start it again from
/// what is currently on disk. This is how a locally rebuilt component is picked
/// up without touching the enable toggle.
async fn restart_module(
    State(state): State<SharedState>,
    Extension(supervisor): Extension<Arc<Supervisor>>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
) -> Result<Response, Response> {
    super::require(&user, Permission::SettingsManage)?;
    if !kroma_engine::modules::module_enabled(&state.settings, &id) {
        return Err(bad("module is disabled"));
    }
    let sup = supervisor.clone();
    let started = tokio::task::spawn_blocking(move || {
        sup.stop(&id);
        sup.start_installed(&id).map(|()| id)
    })
    .await
    .map_err(|_| bad("restart task panicked"))?;
    match started {
        Ok(id) => {
            let running = supervisor.is_running(&id);
            Ok(Json(json!({ "restarted": true, "running": running })).into_response())
        }
        Err(e) => Err(bad(&format!("restart failed: {e:#}"))),
    }
}

/// `POST /admin/modules/{id}/reinstall`: fetch the module again from the source
/// it was installed from and unpack it over the current copy.
///
/// Only possible when that source is refetchable: a module installed by
/// uploading a `.kmod` has no URL to go back to, and says so rather than
/// pretending. Use restart when the component is already the one you want.
async fn reinstall_module(
    State(state): State<SharedState>,
    Extension(supervisor): Extension<Arc<Supervisor>>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
) -> Result<Response, Response> {
    super::require(&user, Permission::SettingsManage)?;
    // Unpacking starts the module when it ships a component, which would start
    // a module the operator has switched off.
    if !kroma_engine::modules::module_enabled(&state.settings, &id) {
        return Err(bad("module is disabled"));
    }
    if supervisor.origin(&id).url.is_none() {
        return Err(bad("this module has no recorded source to reinstall from"));
    }
    let installed = super::store::install::reinstall(&state, &supervisor, &id)
        .await
        .map_err(|e| bad(&format!("reinstall failed: {e:#}")))?;
    let running = supervisor.is_running(&id);
    Ok(
        Json(json!({ "reinstalled": true, "running": running, "module": installed }))
            .into_response(),
    )
}
