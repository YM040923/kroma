//! Media byte delivery: range-streamed original files, the from-zero HLS remux,
//! and on-demand WebVTT subtitle extraction. Bodies are media bytes / playlists.

use std::net::SocketAddr;

use axum::body::Body;
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::Response;
use serde::Deserialize;

use crate::api::error::json_error;
use crate::api::media_ticket::MediaViewer;
use crate::api::util::client_ip;
use crate::api::visibility;
use crate::infra::subtitles;
use crate::model::{MediaItem, User};
use crate::services::playback;
use crate::services::settings;
use crate::state::SharedState;
use axum::routing::get;
use axum::Router;
use kroma_media::input;
use kroma_media::stream::stream_file;

mod download;
mod external_subtitles;
mod hls;
mod hls_query;
mod native_subtitles;

use download::download_item;
use external_subtitles::nth_beside;
use hls::{hls_file, hls_master};

fn byte_sink(
    state: &SharedState,
    headers: &HeaderMap,
    addr: &SocketAddr,
) -> crate::infra::metrics::ByteSink {
    let ip = client_ip(headers, addr, &state.config.trusted_proxies);
    let is_lan = playback::is_lan(&ip, &settings::local_networks(&state.settings));
    state.metrics.sink(is_lan)
}

/// Direct-play streaming, HLS remux, storyboard previews and subtitle tracks.
/// Outside the session middleware because a `<video>` / hls.js element can't
/// attach a bearer to the URLs it fetches, and gated instead by
/// [`MediaViewer`]: every request names an account, and that account's library
/// grant decides what it may read (ACCT-35).
pub fn routes() -> Router<SharedState> {
    Router::new()
        .route("/items/{id}/stream", get(stream_item))
        .route("/items/{id}/photo", get(photo))
        .route(
            "/items/{id}/hls/{mode}/{anchor}/{audio}/index.m3u8",
            get(hls_master),
        )
        .route(
            "/items/{id}/hls/{mode}/{anchor}/{audio}/{file}",
            get(hls_file),
        )
        .route("/items/{id}/storyboard", get(storyboard))
        .route("/items/{id}/storyboard.img", get(storyboard_image))
        .route("/items/{id}/subtitles/{track}", get(subtitles))
        .route(
            "/items/{id}/subtitles/{track}/{duration}/{file}",
            get(native_subtitles::rendition),
        )
}

/// Session-gated media routes: the download is a plain fetch, so it can carry a
/// bearer - and must, because each call holds an ffmpeg for a whole film.
pub fn protected_routes() -> Router<SharedState> {
    Router::new().route("/items/{id}/download", get(download_item))
}

#[derive(Debug, Deserialize)]
pub struct StreamQuery {
    pub file: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct PhotoQuery {
    pub width: Option<u32>,
}

/// `GET /api/items/:id/photo?t=<media-ticket>&width=480` → a visible local photo.
/// Photos library. It streams the file without buffering the original in memory.
pub async fn photo(
    State(state): State<SharedState>,
    MediaViewer(viewer): MediaViewer,
    Path(id): Path<String>,
    Query(query): Query<PhotoQuery>,
) -> Response {
    let Some(item) = load_item(&state, &viewer, id).await else {
        return visibility::out_of_scope();
    };
    if item.kind != crate::model::Kind::Photo {
        return json_error(StatusCode::NOT_FOUND, "photo not found");
    }
    let Some(source_path) = item
        .abs_path
        .as_deref()
        .filter(|path| !kroma_media::remote::is_remote(path))
    else {
        return json_error(StatusCode::NOT_FOUND, "photo file not found");
    };
    let width = query.width.unwrap_or(0).clamp(0, 2400);
    let original = source_path.to_string();
    let data_dir = state.config.data_dir.clone();
    let path = match tokio::task::spawn_blocking(move || {
        if width == 0 {
            Ok(std::path::PathBuf::from(original))
        } else {
            crate::infra::image::cache_local_capped(
                &data_dir,
                std::path::Path::new(&original),
                width,
            )
            .ok_or_else(|| std::io::Error::other("thumbnail conversion failed"))
        }
    })
    .await
    {
        Ok(Ok(path)) => path,
        _ => return json_error(StatusCode::NOT_FOUND, "photo file not found"),
    };
    let extension = path
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let content_type = if width > 0 {
        "image/webp"
    } else {
        match extension.as_str() {
            "jpg" | "jpeg" => "image/jpeg",
            "png" => "image/png",
            "webp" => "image/webp",
            "gif" => "image/gif",
            "avif" => "image/avif",
            "heic" | "heif" => "image/heic",
            "tif" | "tiff" => "image/tiff",
            "bmp" => "image/bmp",
            _ => "application/octet-stream",
        }
    };
    let Ok(metadata) = tokio::fs::metadata(&path).await else {
        return json_error(StatusCode::NOT_FOUND, "photo file not found");
    };
    let Ok(file) = tokio::fs::File::open(&path).await else {
        return json_error(StatusCode::NOT_FOUND, "photo file not found");
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, metadata.len())
        .header(header::CACHE_CONTROL, "private, max-age=300")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .body(Body::from_stream(tokio_util::io::ReaderStream::new(file)))
        .unwrap_or_else(|_| json_error(StatusCode::INTERNAL_SERVER_ERROR, "photo response failed"))
}

/// `GET /api/items/:id/stream` (optional `?file=<fileId>`) → range-streamed
/// original file. Without `?file`, the item's default/best file is served.
///
/// A file whose path is an `http(s)://` URL — a `.strm` pointing at a cloud
/// drive — is served from its origin rather than opened on disk. The player's
/// User-Agent is relayed, because a cloud drive signs the link to whoever asked
/// for it, and the origin's own redirect is handed back so the bytes never pass
/// through this server: a home upstream cannot carry a 4K stream, and a photo of
/// one viewer through the box is exactly what a bridge exists to avoid.
pub async fn stream_item(
    State(state): State<SharedState>,
    MediaViewer(viewer): MediaViewer,
    Path(id): Path<String>,
    Query(q): Query<StreamQuery>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let item = load_item(&state, &viewer, id)
        .await
        .ok_or_else(visibility::out_of_scope)?;
    Ok(serve_selected_file(&state, &item, q.file.as_deref(), &headers, addr).await)
}

/// Serve one of `item`'s files to a caller already proven to reach it. Shared by
/// the session route and the share route, so a public share streams the same
/// bytes with the same range handling.
pub(super) async fn serve_selected_file(
    state: &SharedState,
    item: &MediaItem,
    file_id: Option<&str>,
    headers: &HeaderMap,
    addr: SocketAddr,
) -> Response {
    let sink = byte_sink(state, headers, &addr);
    let range = headers
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    match pick_file_path(item, file_id) {
        Some(path) if kroma_media::remote::is_remote(&path) => {
            tracing::info!(
                item_id = %item.id,
                mode = "remote",
                url = %path,
                range = ?range,
                "media stream"
            );
            let user_agent = headers
                .get(header::USER_AGENT)
                .and_then(|value| value.to_str().ok());
            kroma_media::remote::stream_remote(&path, headers, user_agent, true, sink).await
        }
        Some(path) => {
            tracing::info!(
                item_id = %item.id,
                mode = "local",
                path = %path,
                range = ?range,
                "media stream"
            );
            stream_file(std::path::Path::new(&path), headers, sink).await
        }
        None => json_error(StatusCode::NOT_FOUND, "demo item has no media"),
    }
}

fn pick_file_path(item: &MediaItem, file_id: Option<&str>) -> Option<String> {
    if let Some(fid) = file_id {
        if let Some(f) = item.files.iter().find(|f| f.id == fid) {
            return f.abs_path.clone();
        }
    }
    item.abs_path.clone()
}

pub(super) async fn load_item(state: &SharedState, viewer: &User, id: String) -> Option<MediaItem> {
    visibility::item_in_scope(state, Some(viewer), id).await
}

/// The disk path of an item's media file. A streamed library stores the play
/// URL in `abs_path`; sidecar subtitles live beside the `.strm` under one of
/// the library's roots, so that is what a beside-search must list.
fn local_media_path(state: &SharedState, item: &MediaItem, abs: &str) -> String {
    if !abs.starts_with("http://") && !abs.starts_with("https://") {
        return abs.to_string();
    }
    let Ok(defs) = crate::services::settings::library_defs(&state.settings, &state.config) else {
        return abs.to_string();
    };
    let Some(def) = defs.iter().find(|def| def.id == item.library) else {
        return abs.to_string();
    };
    let Some(rel) = item
        .rel_path
        .as_deref()
        .map(str::trim)
        .filter(|rel| !rel.is_empty())
    else {
        return abs.to_string();
    };
    def.folders
        .iter()
        .map(|folder| std::path::Path::new(folder).join(rel))
        .find(|path| path.exists())
        .or_else(|| {
            def.folders
                .first()
                .map(|folder| std::path::Path::new(folder).join(rel))
        })
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| abs.to_string())
}

/// `GET /api/items/:id/storyboard` → the sprite-sheet manifest mapping a cursor
/// time to a tile. 202 `{"status":"pending"}` while generating (the client polls).
pub async fn storyboard(
    State(state): State<SharedState>,
    MediaViewer(viewer): MediaViewer,
    Path(id): Path<String>,
) -> Response {
    let Some(item) = load_item(&state, &viewer, id).await else {
        return visibility::out_of_scope();
    };
    use crate::infra::storyboard::Status;
    match state.storyboard.get(&item).await {
        Status::Ready(m) => {
            json_no_store(StatusCode::OK, serde_json::to_vec(&m).unwrap_or_default())
        }
        Status::Pending => json_no_store(StatusCode::ACCEPTED, br#"{"status":"pending"}"#.to_vec()),
        Status::Unavailable => json_error(StatusCode::NOT_FOUND, "storyboard unavailable"),
    }
}

/// `GET /api/items/:id/storyboard.img` → the cached sprite sheet, 404 until it is
/// generated. Cached immutably; the manifest's `?v=<key>` busts it.
pub async fn storyboard_image(
    State(state): State<SharedState>,
    MediaViewer(viewer): MediaViewer,
    Path(id): Path<String>,
) -> Response {
    let Some(item) = load_item(&state, &viewer, id).await else {
        return visibility::out_of_scope();
    };
    match state.storyboard.sheet(&item).await {
        Some((bytes, content_type)) => Response::builder()
            .header(header::CONTENT_TYPE, content_type)
            .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
            .body(Body::from(bytes))
            .unwrap(),
        None => json_error(StatusCode::NOT_FOUND, "storyboard not generated"),
    }
}

fn json_no_store(status: StatusCode, body: Vec<u8>) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(body))
        .unwrap()
}

/// `GET /api/items/:id/subtitles/:track` → one text subtitle track as WebVTT.
/// `track` is the 0-based subtitle index (a trailing `.vtt` is allowed): the
/// file's own tracks first, then the subtitle files beside it. Image subtitles
/// (PGS/VobSub) can't convert and return 404.
pub async fn subtitles(
    State(state): State<SharedState>,
    MediaViewer(viewer): MediaViewer,
    Path((id, track)): Path<(String, String)>,
) -> Response {
    let index: usize = match track.trim_end_matches(".vtt").parse() {
        Ok(n) => n,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid subtitle index"),
    };

    let Some(item) = load_item(&state, &viewer, id).await else {
        return visibility::out_of_scope();
    };
    let Some(abs) = item.abs_path.clone() else {
        return json_error(StatusCode::NOT_FOUND, "no media file for item");
    };

    if let Some(nth) = index.checked_sub(item.subtitles.len()) {
        let beside_abs = local_media_path(&state, &item, &abs);
        if let Some(bytes) = nth_beside(&state, &beside_abs, nth).await {
            return vtt_response(bytes);
        }
    }

    // Extracting a text subtitle reads the whole file (cues are interleaved), so
    // cache per (file, mtime, track). Computing the key stats the file, which on a
    // slow mount would block the tokio worker.
    let data_dir = state.config.data_dir.clone();
    let cache = {
        let (abs, data_dir) = (abs.clone(), data_dir.clone());
        match tokio::task::spawn_blocking(move || subtitles::cache_path(&data_dir, &abs, index))
            .await
        {
            Ok(p) => p,
            Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "subtitle cache error"),
        }
    };
    if let Ok(bytes) = tokio::fs::read(&cache).await {
        return vtt_response(bytes);
    }
    // Cache miss: demux once and warm every text track. The per-file lock joins an
    // extraction already in flight instead of demuxing in parallel.
    let subs = item.subtitles.clone();
    let (abs2, data_dir2) = (abs.clone(), data_dir.clone());
    let _ = tokio::task::spawn_blocking(move || {
        subtitles::extract_pending_locked(&data_dir2, &abs2, &subs, &|| false)
    })
    .await;
    if let Ok(bytes) = tokio::fs::read(&cache).await {
        return vtt_response(bytes);
    }
    // `item.subtitles` metadata can be empty or stale, so the batch pass may not have
    // covered this index; extract it alone, codec-agnostically.
    if let Some(bytes) = extract_webvtt(&abs, index).await {
        if let Some(dir) = cache.parent() {
            let _ = tokio::fs::create_dir_all(dir).await;
        }
        let _ = tokio::fs::write(&cache, &bytes).await;
        return vtt_response(bytes);
    }
    json_error(
        StatusCode::NOT_FOUND,
        "subtitle unavailable (image-based or missing)",
    )
}

fn vtt_response(bytes: Vec<u8>) -> Response {
    Response::builder()
        .header(header::CONTENT_TYPE, "text/vtt; charset=utf-8")
        .header(header::CACHE_CONTROL, "public, max-age=86400")
        .body(Body::from(bytes))
        .unwrap()
}

/// Transcodes one text subtitle stream to WebVTT, bounded by a timeout. This
/// single-track variant backs subtitle translation; playback extraction goes
/// through [`subtitles::extract_batch_blocking`].
pub(crate) async fn extract_webvtt(path: &str, index: usize) -> Option<Vec<u8>> {
    let child = tokio::process::Command::new("ffmpeg")
        .args(["-v", "error", "-nostdin"])
        .args(input::file(path))
        .args(["-map", &format!("0:s:{index}"), "-f", "webvtt", "pipe:1"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .ok()?;

    // On timeout the future is dropped, which via kill_on_drop kills ffmpeg.
    let budget = kroma_media::subtitles::timeout_for(path);
    let out = tokio::time::timeout(budget, child.wait_with_output())
        .await
        .ok()?
        .ok()?;
    if out.status.success() && !out.stdout.is_empty() {
        Some(out.stdout)
    } else {
        None
    }
}
