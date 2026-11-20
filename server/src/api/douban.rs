//! Read-only Douban browse endpoints: the ranked rows its mobile site offers,
//! and one title's detail. Nothing here joins the library or triggers a write;
//! the engine's client paces and caches every call.

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

use crate::api::error::json_error;
use crate::api::extract::AuthUser;
use crate::state::SharedState;
use kroma_engine::services::douban::{self, DoubanError, Kind};

pub fn routes() -> Router<SharedState> {
    Router::new()
        .route("/douban/discover/{kind}", get(layout))
        .route("/douban/rows/{kind}/{row}", get(row))
        .route("/douban/subject/{id}", get(subject))
        .route("/douban/search", get(search))
}

/// The image hop is fetched by `<img>`, which cannot send the account header.
pub fn public_routes() -> Router<SharedState> {
    Router::new().route("/douban/image", get(image))
}

fn kind_of(raw: &str) -> Result<Kind, Response> {
    Kind::parse(raw).ok_or_else(|| json_error(StatusCode::BAD_REQUEST, "未知的豆瓣类型"))
}

/// `GET /api/douban/discover/{kind}` → the rows to draw. Configuration only;
/// no call leaves this machine.
async fn layout(
    State(_state): State<SharedState>,
    AuthUser(_user): AuthUser,
    Path(kind): Path<String>,
) -> Result<Response, Response> {
    let kind = kind_of(&kind)?;
    Ok(Json(douban::layout(kind)).into_response())
}

fn failed(error: DoubanError) -> Response {
    match error {
        DoubanError::NotFound => json_error(StatusCode::NOT_FOUND, "该豆瓣榜单或条目不存在"),
        other => json_error(StatusCode::BAD_GATEWAY, &other.to_string()),
    }
}

/// `GET /api/douban/rows/{kind}/{row}` → one row of cards.
async fn row(
    State(_state): State<SharedState>,
    AuthUser(_user): AuthUser,
    Path((kind, row)): Path<(String, String)>,
) -> Result<Response, Response> {
    let kind = kind_of(&kind)?;
    let fetched = tokio::task::spawn_blocking(move || douban::row(kind, &row))
        .await
        .map_err(|_| json_error(StatusCode::BAD_GATEWAY, "豆瓣请求失败"))?;
    match fetched {
        Ok(row) => Ok(Json(row).into_response()),
        Err(error) => Err(failed(error)),
    }
}

/// `GET /api/douban/subject/{id}` → one title's detail.
async fn subject(
    State(_state): State<SharedState>,
    AuthUser(_user): AuthUser,
    Path(id): Path<String>,
) -> Result<Response, Response> {
    let fetched = tokio::task::spawn_blocking(move || douban::detail(&id))
        .await
        .map_err(|_| json_error(StatusCode::BAD_GATEWAY, "豆瓣请求失败"))?;
    match fetched {
        Ok(detail) => Ok(Json(detail).into_response()),
        Err(error) => Err(failed(error)),
    }
}

#[derive(Deserialize, Default)]
struct SearchQuery {
    #[serde(default)]
    q: String,
}

/// `GET /api/douban/search?q=` → Douban's own search results. An empty query
/// answers an empty list, never the whole site.
async fn search(
    State(_state): State<SharedState>,
    AuthUser(_user): AuthUser,
    Query(query): Query<SearchQuery>,
) -> Result<Response, Response> {
    let q = query.q.trim().chars().take(128).collect::<String>();
    if q.is_empty() {
        return Ok(Json(Vec::<douban::SearchItem>::new()).into_response());
    }
    let found = tokio::task::spawn_blocking(move || douban::search(&q))
        .await
        .map_err(|_| json_error(StatusCode::BAD_GATEWAY, "豆瓣请求失败"))?;
    match found {
        Ok(items) => Ok(Json(items).into_response()),
        Err(error) => Err(failed(error)),
    }
}

#[derive(Deserialize)]
struct ImageQuery {
    url: String,
}

const MAX_IMAGE_BYTES: u64 = 12 * 1024 * 1024;

/// Whether `raw` is an https URL on Douban's own image hosts. Only those are
/// fetched: this hop must not become a general-purpose proxy.
fn allowed_image_url(raw: &str) -> Option<&str> {
    let rest = raw.strip_prefix("https://")?;
    if rest.is_empty() || rest.contains('@') {
        return None;
    }
    let host_end = rest
        .find(|c: char| c == '/' || c == '?' || c == '#')
        .unwrap_or(rest.len());
    let host = rest[..host_end].split(':').next().unwrap_or("");
    let on_douban = host == "doubanio.com"
        || host.ends_with(".doubanio.com")
        || host == "douban.com"
        || host.ends_with(".douban.com");
    let well_formed = host
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
    (on_douban && well_formed).then_some(raw)
}

fn image_type(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        "image/jpeg"
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else if bytes.starts_with(b"GIF8") {
        "image/gif"
    } else {
        "application/octet-stream"
    }
}

/// `GET /api/douban/image?url=…` → one Douban poster, fetched with the referer
/// its CDN requires. The URL is restricted to Douban's image hosts.
async fn image(Query(query): Query<ImageQuery>) -> Result<Response, Response> {
    let Some(url) = allowed_image_url(query.url.trim()) else {
        return Err(json_error(StatusCode::BAD_REQUEST, "只能代理豆瓣图片地址"));
    };
    let url = url.to_string();
    let fetched = tokio::task::spawn_blocking(move || {
        kroma_http::Fetch::new()
            .purpose("douban")
            .header("User-Agent", douban::USER_AGENT)
            .header("Referer", douban::REFERER)
            .max_time(20)
            .max_bytes(MAX_IMAGE_BYTES)
            .get(&url)
    })
    .await
    .map_err(|_| json_error(StatusCode::BAD_GATEWAY, "豆瓣图片请求失败"))?;
    let response = fetched.map_err(|_| json_error(StatusCode::BAD_GATEWAY, "豆瓣图片请求失败"))?;
    if !(200..300).contains(&response.status) {
        return Err(json_error(
            StatusCode::BAD_GATEWAY,
            &format!("豆瓣图片返回 HTTP {}", response.status),
        ));
    }
    let content_type = HeaderValue::from_static(image_type(&response.body));
    let mut built = Response::new(Body::from(response.body));
    *built.status_mut() = StatusCode::OK;
    built
        .headers_mut()
        .insert(header::CONTENT_TYPE, content_type);
    built.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=86400"),
    );
    Ok(built)
}
