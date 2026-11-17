use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use kroma_module_sdk::domain::Permission;
use kroma_module_sdk::host::{json_error, AuthUser, HostCtx};
use kroma_module_sdk::http::Fetch;
use serde::Deserialize;
use serde_json::Value;

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
    #[serde(default)]
    poster: String,
    #[serde(default)]
    year: i64,
    #[serde(default)]
    season: i64,
    #[serde(default)]
    episode_start: i64,
    #[serde(default)]
    episode_total: i64,
    #[serde(default)]
    link: String,
    #[serde(default)]
    tmdb_id: i64,
    #[serde(default)]
    kind: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct SourceBody {
    url: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct SearchBody {
    kind: String,
    tmdb_id: i64,
    title: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct SubscribeBody {
    tmdb_id: i64,
    title: String,
    kind: String,
    resource: String,
    index: i64,
    price: i64,
    #[serde(default)]
    poster: String,
    #[serde(default)]
    year: i64,
    #[serde(default)]
    link: String,
}

pub fn routes<S: HostCtx + Clone + Send + Sync + 'static>() -> Router<S> {
    Router::new()
        .route("/subscriptions", get(list::<S>).post(create::<S>))
        .route("/subscriptions/{id}", put(update::<S>))
        .route("/subscriptions/{id}/scan", post(scan::<S>))
        .route("/source", post(set_source::<S>))
        .route("/config", get(get_config::<S>).post(set_config::<S>))
        .route("/search", post(search::<S>))
        .route("/subscribe", post(subscribe::<S>))
        .route("/subscriptions/{id}/execute", post(execute::<S>))
        .route("/subscriptions/{id}/execute-item", post(execute_item::<S>))
        .route("/subscriptions/{id}/job", get(job::<S>))
}

fn require<S: HostCtx>(host: &S, user: &kroma_module_sdk::domain::User) -> Result<(), axum::response::Response> {
    host.require(user, Permission::SettingsManage)
}

async fn get_config<S: HostCtx + Clone + Send + Sync + 'static>(State(host): State<S>, AuthUser(user): AuthUser) -> Result<Json<Value>, axum::response::Response> {
    require(&host, &user)?;
    let source = store::load().source_url;
    if source.trim().is_empty() {
        return Err(json_error(StatusCode::BAD_REQUEST, "订阅源服务地址还没配置"));
    }
    source_json(&source, "/config", None)
        .map(Json)
        .map_err(|error| json_error(StatusCode::BAD_GATEWAY, &error))
}

async fn set_config<S: HostCtx + Clone + Send + Sync + 'static>(State(host): State<S>, AuthUser(user): AuthUser, Json(body): Json<Value>) -> Result<Json<Value>, axum::response::Response> {
    require(&host, &user)?;
    if !body.is_object() {
        return Err(json_error(StatusCode::BAD_REQUEST, "配置必须是一个对象"));
    }
    let source = store::load().source_url;
    if source.trim().is_empty() {
        return Err(json_error(StatusCode::BAD_REQUEST, "订阅源服务地址还没配置"));
    }
    source_json(&source, "/config", Some(&body))
        .map(Json)
        .map_err(|error| json_error(StatusCode::BAD_GATEWAY, &error))
}

fn validate(body: &TaskBody) -> Result<(), String> {
    if body.name.trim().is_empty() || body.source.trim().is_empty() {
        return Err("任务名称和订阅源都必须填写".into());
    }
    if !matches!(body.monitor_mode.as_str(), "manual" | "polling") {
        return Err("监控方式只能是 manual 或 polling".into());
    }
    if body.monitor_mode == "polling" && body.monitor_path.trim().is_empty() {
        return Err("轮询任务必须填写监控目录".into());
    }
    Ok(())
}

async fn list<S: HostCtx + Clone + Send + Sync + 'static>(State(host): State<S>, AuthUser(user): AuthUser) -> Result<Json<store::Stored>, axum::response::Response> {
    require(&host, &user)?;
    poll_transfers();
    discover();
    Ok(Json(store::load()))
}

/// Finds releases the enabled show subscriptions have not been sent yet. This
/// is discovery only: it never unlocks or spends points; a person still has to
/// confirm a candidate from the page.
pub(crate) fn discover() {
    let mut stored = store::load();
    let source = stored.source_url.clone();
    if source.trim().is_empty() {
        return;
    }
    let now = kroma_module_sdk::primitives::now_ms();
    if now - stored.last_discovery_at < 15 * 60 * 1000 {
        return;
    }
    let tasks: Vec<(String, String, i64)> = stored
        .tasks
        .iter()
        .filter(|task| task.enabled && task.kind == "show")
        .map(|task| (task.id.clone(), task.name.clone(), task.tmdb_id))
        .collect();
    for (id, name, tmdb_id) in tasks {
        let path = format!("/search?tmdb={tmdb_id}&type=tv&probe=0");
        let Ok(value) = source_json(&source, &path, None) else {
            continue;
        };
        let candidates = value
            .get("candidates")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for candidate in candidates {
            let resource = candidate
                .get("fileName")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            if resource.is_empty() {
                continue;
            }
            let key = format!("{id}:{resource}");
            if stored.items.iter().any(|item| item.key == key) {
                continue;
            }
            if stored
                .tasks
                .iter()
                .any(|task| task.id == id && task.resource == resource)
            {
                continue;
            }
            stored.items.push(store::Item {
                key,
                task_id: id.clone(),
                title: name.clone(),
                state: "new".into(),
                last_error: String::new(),
                resource,
                index: candidate.get("index").and_then(Value::as_i64).unwrap_or(0),
                size: candidate
                    .get("size")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                link: candidate
                    .get("link")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                price: candidate.get("price").and_then(Value::as_i64).unwrap_or(0),
            });
        }
    }
    let excess = stored.items.len().saturating_sub(500);
    if excess > 0 {
        stored.items.drain(0..excess);
    }
    stored.last_discovery_at = now;
    let _ = store::save(&stored);
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExecuteItemBody {
    resource: String,
    #[serde(default)]
    index: i64,
    #[serde(default)]
    price: i64,
    #[serde(default)]
    link: String,
}

async fn execute_item<S: HostCtx + Clone + Send + Sync + 'static>(State(host): State<S>, AuthUser(user): AuthUser, Path(id): Path<String>, Json(body): Json<ExecuteItemBody>) -> Result<Json<Task>, axum::response::Response> {
    require(&host, &user)?;
    if body.resource.trim().is_empty() {
        return Err(json_error(StatusCode::BAD_REQUEST, "缺少要转存的资源"));
    }
    let mut stored = store::load();
    let source = stored.source_url.clone();
    let kind = stored
        .tasks
        .iter()
        .find(|task| task.id == id)
        .map(|task| if task.kind == "show" { "show" } else { "movie" })
        .unwrap_or("movie")
        .to_string();
    let task = stored
        .tasks
        .iter_mut()
        .find(|task| task.id == id)
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "订阅任务不存在"))?;
    task.resource = body.resource.trim().to_string();
    task.resource_index = body.index;
    task.price = body.price;
    task.link = body.link.trim().into();
    start_transfer(task, &source, &kind);
    let result = task.clone();
    for item in stored.items.iter_mut() {
        if item.task_id == id && item.resource == result.resource {
            item.state = "sent".into();
        }
    }
    store::save(&stored).map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, &error))?;
    Ok(Json(result))
}

/// Reads the source's asynchronous unlock/transfer jobs and folds their result
/// back into the task: a 202 from `/unlock` is a started job, not a failure.
pub(crate) fn poll_transfers() {
    let mut stored = store::load();
    let source = stored.source_url.clone();
    if source.trim().is_empty() {
        return;
    }
    let mut changed = false;
    for task in stored.tasks.iter_mut() {
        if task.job_id.is_empty() {
            continue;
        }
        if !matches!(
            task.status.as_str(),
            "unlocking" | "running" | "transferring"
        ) {
            continue;
        }
        let Ok(job) = source_json(&source, &format!("/job/{}", task.job_id), None) else {
            continue;
        };
        let status = job.get("status").and_then(Value::as_str).unwrap_or("");
        match status {
            "succeeded" => {
                task.status = "done".into();
                task.last_error.clear();
                if let Some(price) = job
                    .pointer("/result/unlocked/pricePaid")
                    .and_then(Value::as_i64)
                {
                    task.price = price;
                }
                if let Some(link) = job
                    .pointer("/result/unlocked/link")
                    .and_then(Value::as_str)
                {
                    task.link = link.to_string();
                }
                task.last_run_at = kroma_module_sdk::primitives::now_ms();
                changed = true;
            }
            "failed" => {
                task.status = "failed".into();
                task.last_error = job
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("转存失败")
                    .chars()
                    .take(200)
                    .collect();
                changed = true;
            }
            _ => {
                let step = job.get("step").and_then(Value::as_str).unwrap_or("");
                if !step.is_empty() && task.last_error != format!("进行中：{step}") {
                    task.status = "running".into();
                    task.last_error = format!("进行中：{step}");
                    changed = true;
                }
            }
        }
    }
    if changed {
        let _ = store::save(&stored);
    }
}

async fn create<S: HostCtx + Clone + Send + Sync + 'static>(State(host): State<S>, AuthUser(user): AuthUser, Json(body): Json<TaskBody>) -> Result<Json<Task>, axum::response::Response> {
    require(&host, &user)?;
    validate(&body).map_err(|e| json_error(StatusCode::BAD_REQUEST, &e))?;
    let mut stored = store::load();
    let id = format!("sub-{}", kroma_module_sdk::primitives::now_ms());
    let task = Task {
        id,
        name: body.name.trim().into(),
        source: body.source.trim().into(),
        monitor_path: body.monitor_path.trim().into(),
        target_path: body.target_path.trim().into(),
        failure_path: body.failure_path.trim().into(),
        monitor_mode: body.monitor_mode,
        enabled: body.enabled,
        status: "draft".into(),
        poster: body.poster.trim().into(),
        year: body.year,
        season: body.season,
        episode_start: body.episode_start,
        episode_total: body.episode_total,
        link: body.link.trim().into(),
        tmdb_id: body.tmdb_id,
        kind: body.kind,
        ..Task::default()
    };
    stored.tasks.push(task.clone());
    store::save(&stored).map_err(|e| json_error(StatusCode::INTERNAL_SERVER_ERROR, &e))?;
    Ok(Json(task))
}

async fn update<S: HostCtx + Clone + Send + Sync + 'static>(State(host): State<S>, AuthUser(user): AuthUser, Path(id): Path<String>, Json(body): Json<TaskBody>) -> Result<Json<Task>, axum::response::Response> {
    require(&host, &user)?;
    validate(&body).map_err(|e| json_error(StatusCode::BAD_REQUEST, &e))?;
    let mut stored = store::load();
    let task = stored.tasks.iter_mut().find(|t| t.id == id).ok_or_else(|| json_error(StatusCode::NOT_FOUND, "订阅任务不存在"))?;
    task.name = body.name.trim().into();
    task.source = body.source.trim().into();
    task.monitor_path = body.monitor_path.trim().into();
    task.target_path = body.target_path.trim().into();
    task.failure_path = body.failure_path.trim().into();
    task.monitor_mode = body.monitor_mode;
    task.enabled = body.enabled;
    task.status = if task.enabled { "enabled" } else { "paused" }.into();
    task.poster = body.poster.trim().into();
    task.year = body.year;
    task.season = body.season;
    task.episode_start = body.episode_start;
    task.episode_total = body.episode_total;
    task.link = body.link.trim().into();
    task.tmdb_id = body.tmdb_id;
    task.kind = body.kind;
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

async fn set_source<S: HostCtx + Clone + Send + Sync + 'static>(State(host): State<S>, AuthUser(user): AuthUser, Json(body): Json<SourceBody>) -> Result<Json<store::Stored>, axum::response::Response> {
    require(&host, &user)?;
    let url = body.url.trim().trim_end_matches('/').to_string();
    if !url.is_empty() && !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(json_error(StatusCode::BAD_REQUEST, "订阅源服务地址必须形如 http://172.17.0.1:8790"));
    }
    let mut stored = store::load();
    stored.source_url = url;
    store::save(&stored).map_err(|e| json_error(StatusCode::INTERNAL_SERVER_ERROR, &e))?;
    Ok(Json(stored))
}

async fn search<S: HostCtx + Clone + Send + Sync + 'static>(State(host): State<S>, AuthUser(user): AuthUser, Json(body): Json<SearchBody>) -> Result<Json<Value>, axum::response::Response> {
    require(&host, &user)?;
    let source = store::load().source_url;
    if source.is_empty() {
        return Err(json_error(StatusCode::BAD_REQUEST, "订阅源服务地址还没配置：在订阅页填上本机来源服务的地址"));
    }
    if body.tmdb_id <= 0 {
        return Err(json_error(StatusCode::BAD_REQUEST, "缺少 TMDB 标识"));
    }
    let kind = if body.kind == "show" { "tv" } else { "movie" };
    let url = format!("{source}/search?tmdb={}&type={kind}&probe=0", body.tmdb_id);
    let response = Fetch::default()
        .purpose("subscription-source")
        .max_time(180)
        .get(&url)
        .map_err(|error| json_error(StatusCode::BAD_GATEWAY, &format!("订阅源服务连不上：{error}")))?;
    let status = response.status;
    let text = response.text();
    if status != 200 {
        let reason: String = text.trim().chars().take(300).collect();
        return Err(json_error(StatusCode::BAD_GATEWAY, &format!("订阅源返回 HTTP {status}：{reason}")));
    }
    let value: Value = serde_json::from_str(&text).map_err(|_| json_error(StatusCode::BAD_GATEWAY, "订阅源返回的不是 JSON"))?;
    Ok(Json(value))
}

async fn subscribe<S: HostCtx + Clone + Send + Sync + 'static>(State(host): State<S>, AuthUser(user): AuthUser, Json(body): Json<SubscribeBody>) -> Result<Json<Task>, axum::response::Response> {
    require(&host, &user)?;
    if body.tmdb_id <= 0 || body.title.trim().is_empty() || body.resource.trim().is_empty() {
        return Err(json_error(StatusCode::BAD_REQUEST, "订阅缺少影片或资源信息"));
    }
    let mut stored = store::load();
    if stored.tasks.iter().any(|task| task.tmdb_id == body.tmdb_id && task.resource == body.resource) {
        return Err(json_error(StatusCode::CONFLICT, "这个资源已经加入订阅"));
    }
    let id = format!("sub-{}", kroma_module_sdk::primitives::now_ms());
    let mut task = Task {
        id,
        name: body.title.trim().to_string(),
        source: "madreel".into(),
        monitor_path: String::new(),
        target_path: String::new(),
        failure_path: String::new(),
        monitor_mode: "manual".into(),
        enabled: true,
        status: "pending".into(),
        last_run_at: kroma_module_sdk::primitives::now_ms(),
        last_error: String::new(),
        tmdb_id: body.tmdb_id,
        kind: body.kind.clone(),
        resource: body.resource.trim().to_string(),
        resource_index: body.index,
        price: body.price,
        job_id: String::new(),
        poster: body.poster.trim().into(),
        year: body.year,
        season: 0,
        episode_start: 0,
        episode_total: 0,
        link: body.link.trim().into(),
    };
    start_transfer(&mut task, &stored.source_url, &body.kind);
    stored.tasks.push(task.clone());
    store::save(&stored).map_err(|e| json_error(StatusCode::INTERNAL_SERVER_ERROR, &e))?;
    Ok(Json(task))
}

fn start_transfer(task: &mut Task, source_url: &str, kind: &str) {
    if source_url.trim().is_empty() {
        task.status = "pending".into();
        task.last_error = "来源服务地址未配置".into();
        return;
    }
    let payload = serde_json::json!({
        "tmdb": task.tmdb_id,
        "type": if kind == "show" { "tv" } else { "movie" },
        "index": task.resource_index,
        "transfer": true,
    });
    match source_json(source_url, "/unlock", Some(&payload)) {
        Ok(value) => {
            task.job_id = value
                .get("jobId")
                .or_else(|| value.get("id"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            task.status = "unlocking".into();
            task.last_error.clear();
        }
        Err(reason) => {
            task.status = "failed".into();
            task.last_error = reason;
        }
    }
}

fn source_json(source_url: &str, path: &str, body: Option<&Value>) -> Result<Value, String> {
    let url = format!("{}{path}", source_url.trim_end_matches('/'));
    let fetch = Fetch::default().purpose("subscription-source").max_time(180);
    let response = match body {
        Some(body) => fetch.post_json(&url, body),
        None => fetch.get(&url),
    }
    .map_err(|error| format!("订阅源服务连不上：{error}"))?;
    let status = response.status;
    let text = response.text();
    if !(200..300).contains(&status) {
        let reason: String = text.trim().chars().take(300).collect();
        return Err(format!("订阅源返回 HTTP {status}：{reason}"));
    }
    serde_json::from_str(&text).map_err(|_| "订阅源返回的不是 JSON".to_string())
}

async fn execute<S: HostCtx + Clone + Send + Sync + 'static>(State(host): State<S>, AuthUser(user): AuthUser, Path(id): Path<String>) -> Result<Json<Task>, axum::response::Response> {
    require(&host, &user)?;
    let mut stored = store::load();
    let kind = stored
        .tasks
        .iter()
        .find(|task| task.id == id)
        .map(|task| if task.kind == "show" { "show" } else { "movie" })
        .unwrap_or("movie");
    let source = stored.source_url.clone();
    let task = stored.tasks.iter_mut().find(|task| task.id == id).ok_or_else(|| json_error(StatusCode::NOT_FOUND, "订阅任务不存在"))?;
    start_transfer(task, &source, kind);
    let result = task.clone();
    store::save(&stored).map_err(|e| json_error(StatusCode::INTERNAL_SERVER_ERROR, &e))?;
    Ok(Json(result))
}

async fn job<S: HostCtx + Clone + Send + Sync + 'static>(State(host): State<S>, AuthUser(user): AuthUser, Path(id): Path<String>) -> Result<Json<Value>, axum::response::Response> {
    require(&host, &user)?;
    let mut stored = store::load();
    let source = stored.source_url.clone();
    let task = stored.tasks.iter_mut().find(|task| task.id == id).ok_or_else(|| json_error(StatusCode::NOT_FOUND, "订阅任务不存在"))?;
    if task.job_id.is_empty() {
        return Err(json_error(StatusCode::CONFLICT, "这个订阅还没有转存任务"));
    }
    let value = source_json(&source, &format!("/job/{}", task.job_id), None)
        .map_err(|reason| json_error(StatusCode::BAD_GATEWAY, &reason))?;
    if let Some(state) = value.get("status").and_then(Value::as_str) {
        task.status = state.to_string();
    }
    if let Some(error) = value.get("error").and_then(Value::as_str) {
        task.last_error = error.to_string();
    }
    let result = value.clone();
    store::save(&stored).map_err(|e| json_error(StatusCode::INTERNAL_SERVER_ERROR, &e))?;
    Ok(Json(result))
}
