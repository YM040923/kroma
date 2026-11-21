//! The `cloud115` point: what the core calls to search 115, price and acquire a
//! candidate, and to stage a local file into the drive.
//!
//! Every route lives under `/_port/cloud115/...`, which the runtime wraps in the
//! host-token guard: the caller is the core, never a browser, so no route takes
//! an [`AuthUser`](kroma_module_sdk::host::AuthUser). The answer uses the point
//! contract's `Result<T, String>` envelope, exactly like the acquisition
//! module's `serve.rs`.
//!
//! The drive itself is reached through the module's existing client, pacer and
//! crypto code; the one external dependency is the subscription source
//! (`/search`, `/unlock`, `/job/{id}`), configured as `sourceUrl` in this
//! module's own settings.

pub mod acquire;
pub mod search;
pub mod upload;

use axum::routing::post;
use axum::{Json, Router};
use kroma_module_sdk::host::{port_reply, HostCtx};
use kroma_module_sdk::http::Fetch;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::settings::Config;

/// The `net.connect` purpose every call to the subscription source is made
/// under. The manifest declares it, so a call with any other purpose is refused
/// by the core's network policy.
pub(crate) const SOURCE_PURPOSE: &str = "subscription-source";

/// Ceiling for one source call. `/search?probe=1` opens one confirmation page per
/// candidate (about half a minute for eight), so the transport must not cut it
/// short; a caller that cannot wait should leave probing off.
pub(crate) const SOURCE_TIMEOUT_SECS: u32 = 180;

/// The module's provider point, mounted by the binary as its extra router.
pub fn routes<S: HostCtx + Clone + Send + Sync + 'static>() -> Router<S> {
    Router::new()
        .route("/_port/cloud115/readiness", post(readiness))
        .route("/_port/cloud115/search", post(search::search))
        .route("/_port/cloud115/acquire", post(acquire::acquire))
        .route("/_port/cloud115/status", post(acquire::status))
        .route("/_port/cloud115/upload", post(upload::upload::<S>))
        .route("/_port/cloud115/upload-status", post(upload::status))
}

/// Whether the provider can answer at all, and what stands in the way.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Readiness {
    pub ready: bool,
    #[serde(default)]
    pub blockers: Vec<String>,
}

/// What is missing before this provider can search or acquire. Both sides are
/// operator settings, so this is a configuration check and never a network one:
/// readiness must answer when the source is down.
pub(crate) fn readiness_of(config: &Config) -> Readiness {
    let mut blockers = Vec::new();
    if config.cookie.trim().is_empty() {
        blockers.push("115 的 Cookie 还没配置".to_string());
    }
    if config.source_url.trim().is_empty() {
        blockers.push(
            "订阅源服务地址还没配置（填 madreel-svc 的地址，如 http://172.17.0.1:8790）".to_string(),
        );
    }
    Readiness {
        ready: blockers.is_empty(),
        blockers,
    }
}

async fn readiness() -> Json<Result<Readiness, String>> {
    port_reply(|| Ok(readiness_of(&Config::load()))).await
}

/// One JSON exchange with the subscription source: `GET` when `body` is `None`,
/// `POST` when it is `Some`. Every failure reads as one sentence an operator can
/// act on, because it is surfaced through the point's `Err` envelope.
pub(crate) fn source_json(
    config: &Config,
    path: &str,
    body: Option<&Value>,
) -> Result<Value, String> {
    let base = config.source_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err("订阅源服务地址还没配置".into());
    }
    let url = format!("{base}{path}");
    let fetch = Fetch::default()
        .purpose(SOURCE_PURPOSE)
        .max_time(SOURCE_TIMEOUT_SECS);
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
    serde_json::from_str(&text).map_err(|_| {
        let snippet: String = text.trim().chars().take(200).collect();
        format!("订阅源返回的不是 JSON：{snippet}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt as _;

    use kroma_module_sdk::host::testing::StubHost;

    fn config() -> Config {
        Config::default()
    }

    #[test]
    fn readiness_names_both_settings_until_they_are_filled() {
        let bare = readiness_of(&config());
        assert!(!bare.ready);
        assert_eq!(bare.blockers.len(), 2, "{:?}", bare.blockers);
        assert!(bare.blockers.iter().any(|b| b.contains("Cookie")), "{bare:?}");
        assert!(bare.blockers.iter().any(|b| b.contains("订阅源")), "{bare:?}");

        let mut half = config();
        half.cookie = "UID=1_A1_2; CID=x".into();
        let readiness = readiness_of(&half);
        assert!(!readiness.ready, "a cookie alone cannot search a source");
        assert_eq!(readiness.blockers.len(), 1);

        let mut ready = half;
        ready.source_url = "http://172.17.0.1:8790".into();
        let readiness = readiness_of(&ready);
        assert!(readiness.ready, "{readiness:?}");
        assert!(readiness.blockers.is_empty());
    }

    #[test]
    fn a_source_call_without_a_configured_source_says_so() {
        let error = source_json(&config(), "/search", None).unwrap_err();
        assert!(error.contains("订阅源服务地址还没配置"), "{error}");
    }

    #[test]
    fn readiness_keeps_the_points_field_names() {
        let value = serde_json::to_value(Readiness {
            ready: false,
            blockers: vec!["x".into()],
        })
        .unwrap();
        assert_eq!(value["ready"], false);
        assert_eq!(value["blockers"][0], "x");
    }

    async fn post(
        app: axum::Router,
        path: &str,
        body: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        let request = Request::builder()
            .method("POST")
            .uri(path)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    #[tokio::test]
    async fn readiness_is_mounted_and_answers_the_point_envelope() {
        let host = StubHost::new();
        let (status, body) =
            post(routes::<StubHost>().with_state(host), "/_port/cloud115/readiness", serde_json::json!({}))
                .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let answer = body.get("Ok").expect("the point envelope's Ok side");
        assert!(answer.get("ready").and_then(serde_json::Value::as_bool).is_some());
        assert!(answer.get("blockers").and_then(serde_json::Value::as_array).is_some());
    }

    #[tokio::test]
    async fn every_route_is_mounted_where_the_core_calls_them() {
        let host = StubHost::new();
        let app = routes::<StubHost>().with_state(host);
        for (path, body) in [
            (
                "/_port/cloud115/search",
                serde_json::json!({ "kind": "movie", "tmdbId": 155, "title": "Film" }),
            ),
            (
                "/_port/cloud115/acquire",
                serde_json::json!({ "kind": "movie", "tmdbId": 155, "resource": "Film.mkv", "index": 0, "price": 4, "link": "", "confirmed": false }),
            ),
            ("/_port/cloud115/status", serde_json::json!({ "jobId": "nope" })),
            ("/_port/cloud115/upload-status", serde_json::json!({ "jobId": "nope" })),
        ] {
            let (status, _) = post(app.clone(), path, body).await;
            assert_eq!(status, StatusCode::OK, "{path} must be mounted");
        }
    }

    #[tokio::test]
    async fn a_malformed_body_is_rejected_by_the_extractor() {
        let host = StubHost::new();
        let app = routes::<StubHost>().with_state(host);
        let (status, _) = post(app, "/_port/cloud115/search", serde_json::json!({})).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }
}