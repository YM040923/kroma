//! Range streaming of a media file that lives behind an HTTP(S) URL.
//!
//! The catalog's `files.abs_path` normally names a file on disk. An entry whose
//! path is a `http://` or `https://` URL (a `.strm` on a cloud drive, say) is
//! served from its origin instead: the range the client asked for is forwarded
//! to the origin and the bytes are relayed back, so seeking costs one request
//! rather than a download of everything before it.
//!
//! The origin's own `Content-Range` is trusted, never recomputed: a provider
//! that ignores `Range` answers `200` with the whole object, which is relayed as
//! it is.

use axum::body::Body;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use futures_util::TryStreamExt;

use crate::json_error::json_error;

use crate::ByteMeter;

/// A client for origins that will not check certificates the way the rest of the
/// server does. Local media providers (a cloud-drive bridge on `127.0.0.1`, a dev
/// NAS) answer over a self-signed certificate often enough that a verify-on
/// client would refuse every stream with no way for an operator to say otherwise.
///
/// `follow` is off when a redirect is what we want to hand the player: following
/// it here would pull the bytes through this server, which is the opposite of
/// what `prefer_redirect` asks for.
fn client(follow: bool) -> Result<reqwest::Client, String> {
    let policy = if follow {
        reqwest::redirect::Policy::limited(10)
    } else {
        reqwest::redirect::Policy::none()
    };
    reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .redirect(policy)
        .connect_timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|error| format!("could not build the range client: {error}"))
}

/// A `302` carrying the origin's own link: the player fetches it itself, so the
/// bytes never pass through this server. `no-store` keeps a player (and any
/// proxy in front of it) from pinning a link that expires.
fn redirect_response(target: &str) -> Response {
    let mut resp = Response::new(Body::empty());
    *resp.status_mut() = StatusCode::FOUND;
    let headers = resp.headers_mut();
    if let Ok(location) = HeaderValue::from_str(target) {
        headers.insert(header::LOCATION, location);
    }
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
}

/// Is this catalog path something to fetch over HTTP rather than open on disk?
pub fn is_remote(path: &str) -> bool {
    let lower = path.trim_start().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// Range-stream `url`. The client's `Range` is forwarded unchanged; the origin's
/// status, `Content-Range`, `Content-Length` and `Accept-Ranges` are relayed, and
/// the body is metered as it is delivered.
///
/// `user_agent` is what the origin sees. Cloud drives sign a download link to the
/// client that asked for it, so relaying the *player's* UA rather than the
/// server's decides between a playable file and a `403`.
///
/// `prefer_redirect` asks the origin for a link the player can follow itself. A
/// cloud drive behind a bridge answers `302` and the bytes never touch this
/// server, which is the whole point: a household's upstream cannot carry a 4K
/// stream, and a proxy turns one viewer into one copy of the film through the
/// box. The origin's redirect is handed back as it is; only when the origin
/// insists on sending bytes (a link that needs a Cookie, an `f=3` style URL) does
/// this fall back to relaying them.
pub async fn stream_remote<M: ByteMeter>(
    url: &str,
    req_headers: &HeaderMap,
    user_agent: Option<&str>,
    prefer_redirect: bool,
    meter: M,
) -> Response {
    let client = match client(prefer_redirect) {
        Ok(client) => client,
        Err(error) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, &error),
    };

    let mut request = client.get(url);
    if let Some(range) = req_headers.get(header::RANGE).and_then(|v| v.to_str().ok()) {
        request = request.header(header::RANGE, range);
    }
    if let Some(ua) = user_agent {
        request = request.header(header::USER_AGENT, ua);
    }

    let upstream = match request.send().await {
        Ok(upstream) => upstream,
        Err(error) => {
            // The origin is unreachable (DNS, TLS, refused). The 502 says whose
            // fault it is rather than blaming the catalog.
            tracing::warn!(url, %error, "remote media origin did not answer");
            return json_error(
                StatusCode::BAD_GATEWAY,
                "the media origin did not answer; the cloud drive may be offline or the link expired",
            );
        }
    };

    // A redirect the player can follow itself: the bytes go straight from the
    // origin to it. `Location` is the only header that matters; the player's own
    // UA (and range) travel with its next request, which is what a cloud drive
    // bound the link to.
    if prefer_redirect {
        if let Some(location) = upstream.headers().get(header::LOCATION) {
            if let Ok(target) = location.to_str() {
                if target.starts_with("http://") || target.starts_with("https://") {
                    tracing::debug!(url, target, "remote media hands the player a redirect");
                    return redirect_response(target);
                }
            }
        }
    }

    let status = upstream.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        tracing::warn!(url, %status, "remote media origin refused the link");
        return json_error(
            StatusCode::BAD_GATEWAY,
            "the media origin refused the link; it may have expired or be bound to another client",
        );
    }
    if !status.is_success() {
        tracing::warn!(url, %status, "remote media origin answered an error");
        return json_error(
            StatusCode::BAD_GATEWAY,
            "the media origin answered an error rather than media bytes",
        );
    }

    // Read the headers we relay before the body is moved into the response.
    let relayed: Vec<(header::HeaderName, HeaderValue)> = [
        header::CONTENT_TYPE,
        header::CONTENT_LENGTH,
        header::CONTENT_RANGE,
        header::ACCEPT_RANGES,
        header::LAST_MODIFIED,
        header::ETAG,
    ]
    .into_iter()
    .filter_map(|name| {
        let text = upstream.headers().get(&name)?.to_str().ok()?;
        HeaderValue::from_str(text).ok().map(|value| (name, value))
    })
    .collect();

    let mut resp = Response::new(metered_body(upstream, meter));
    *resp.status_mut() = StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::OK);

    let headers = resp.headers_mut();
    for (name, value) in relayed {
        headers.insert(name, value);
    }
    if !headers.contains_key(header::ACCEPT_RANGES) {
        headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    }
    if !headers.contains_key(header::CONTENT_TYPE) {
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/octet-stream"),
        );
    }
    resp
}

fn metered_body<M: ByteMeter>(upstream: reqwest::Response, meter: M) -> Body {
    let stream = upstream
        .bytes_stream()
        .map_ok(move |chunk| {
            meter.add(chunk.len() as u64);
            chunk
        })
        .map_err(std::io::Error::other);
    Body::from_stream(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_url_is_remote_and_a_path_is_not() {
        assert!(is_remote("http://127.0.0.1:9934/media/1.mkv"));
        assert!(is_remote("HTTPS://example.test/a.mp4"));
        assert!(is_remote("  https://example.test/a.mp4"));
        assert!(!is_remote("/media/film.mkv"));
        assert!(!is_remote("C:\\media\\film.mkv"));
        assert!(!is_remote("rtsp://example.test/a"));
    }

    #[tokio::test]
    async fn a_redirect_is_handed_to_the_player_rather_than_followed() {
        // A bridge answers with a location on the cloud CDN; the answer must be
        // that location, not the bytes behind it.
        let resp = redirect_response("https://cdn.example.test/film.mkv?sig=abc");
        assert_eq!(resp.status(), StatusCode::FOUND);
        assert_eq!(
            resp.headers().get(header::LOCATION).unwrap(),
            "https://cdn.example.test/film.mkv?sig=abc"
        );
        assert_eq!(
            resp.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store"
        );
    }
}
