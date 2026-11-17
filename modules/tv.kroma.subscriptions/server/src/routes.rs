use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use kroma_module_sdk::host::{json_error, AuthUser, HostCtx};
use kroma_module_sdk::domain::Permission;
use serde::Deserialize;

use crate::store::{self, Item, Task};

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct TaskBody {
    name: String,
    source: String,
    monitor_path: String,
    target_path: String,
    failure_path: String,
    monitor_mode: String,
    enabled: bool,
}

pub fn routes<S: HostCtx + Clone + Send + Sync + 'static>() -> Router<S> {
    Router::new()
        .route("/subscriptions", get(list::<S>).post(create::<S>))
        .route("/subscriptions/{id}", put(update::<S>))
        .route("/subscriptions/{id}/scan", post(scan::<S>))
}

fn require<S: HostCtx>(host: &S, user: &kroma_module_sdk::domain::User) -> Result<(), axum::response::Response> {
    host.require(user, Permission::SettingsManage)
}

fn validate(body: &TaskBody) -> Result<(), String> {
    if body.name.trim().is_empty() || body.source.trim().is_empty() || body.monitor_path.trim().is_empty() || body.target_path.trim().is_empty() || body.failure_path.trim().is_empty() {
        return Err("任务名称、订阅源、监控目录、目标目录和失败目录都必须填写".into());
    }
    if !matches!(body.monitor_mode.as_str(), "manual" | "polling") {
        return Err("监控方式只能是 manual 或 polling".into());
    }
    Ok(())
}

async fn list<S: HostCtx + Clone + Send + Sync + 'static>(State(host): State<S>, AuthUser(user): AuthUser) -> Result<Json<store::Stored>, axum::response::Response> {
    require(&host, &user)?;
    Ok(Json(store::load()))
}

async fn create<S: HostCtx + Clone + Send + Sync + 'static>(State(host): State<S>, AuthUser(user): AuthUser, Json(body): Json<TaskBody>) -> Result<Json<Task>, axum::response::Response> {
    require(&host, &user)?;
    validate(&body).map_err(|e| json_error(StatusCode::BAD_REQUEST, &e))?;
    let mut stored = store::load();
    let id = format!("sub-{}", kroma_module_sdk::primitives::now_ms());
    let task = Task { id, name: body.name.trim().into(), source: body.source.trim().into(), monitor_path: body.monitor_path.trim().into(), target_path: body.target_path.trim().into(), failure_path: body.failure_path.trim().into(), monitor_mode: body.monitor_mode, enabled: body.enabled, status: "draft".into(), ..Task::default() };
    stored.tasks.push(task.clone());
    store::save(&stored).map_err(|e| json_error(StatusCode::INTERNAL_SERVER_ERROR, &e))?;
    Ok(Json(task))
}

async fn update<S: HostCtx + Clone + Send + Sync + 'static>(State(host): State<S>, AuthUser(user): AuthUser, Path(id): Path<String>, Json(body): Json<TaskBody>) -> Result<Json<Task>, axum::response::Response> {
    require(&host, &user)?;
    validate(&body).map_err(|e| json_error(StatusCode::BAD_REQUEST, &e))?;
    let mut stored = store::load();
    let task = stored.tasks.iter_mut().find(|t| t.id == id).ok_or_else(|| json_error(StatusCode::NOT_FOUND, "订阅任务不存在"))?;
    task.name = body.name.trim().into(); task.source = body.source.trim().into(); task.monitor_path = body.monitor_path.trim().into(); task.target_path = body.target_path.trim().into(); task.failure_path = body.failure_path.trim().into(); task.monitor_mode = body.monitor_mode; task.enabled = body.enabled; task.status = if task.enabled { "enabled" } else { "paused" }.into();
    let result = task.clone();
    store::save(&stored).map_err(|e| json_error(StatusCode::INTERNAL_SERVER_ERROR, &e))?;
    Ok(Json(result))
}

async fn scan<S: HostCtx + Clone + Send + Sync + 'static>(State(host): State<S>, AuthUser(user): AuthUser, Path(id): Path<String>) -> Result<Json<Vec<Item>>, axum::response::Response> {
    require(&host, &user)?;
    let mut stored = store::load();
    let task = stored.tasks.iter_mut().find(|t| t.id == id).ok_or_else(|| json_error(StatusCode::NOT_FOUND, "订阅任务不存在"))?;
    task.status = "running-dry-run".into(); task.last_run_at = kroma_module_sdk::primitives::now_ms(); task.last_error.clear();
    store::save(&stored).map_err(|e| json_error(StatusCode::INTERNAL_SERVER_ERROR, &e))?;
    Ok(Json(stored.items.into_iter().filter(|item| item.task_id == id).collect()))
}
