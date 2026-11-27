//! Host-token guarded provider point for an explicitly requested staged upload.

pub mod upload;

use axum::{routing::post, Router};
use kroma_module_sdk::host::HostCtx;

pub fn routes<S: HostCtx + Clone + Send + Sync + 'static>() -> Router<S> {
    Router::new()
        .route("/_port/cloud115/upload", post(upload::upload::<S>))
        .route("/_port/cloud115/upload-status", post(upload::status))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use kroma_module_sdk::host::testing::StubHost;
    use tower::ServiceExt;

    #[tokio::test]
    async fn only_staged_upload_and_poll_are_mounted() {
        let app = routes::<StubHost>().with_state(StubHost::new());
        for (path, expected) in [
            ("search", StatusCode::NOT_FOUND),
            ("acquire", StatusCode::NOT_FOUND),
            ("status", StatusCode::NOT_FOUND),
            ("readiness", StatusCode::NOT_FOUND),
            ("upload", StatusCode::UNPROCESSABLE_ENTITY),
            ("upload-status", StatusCode::OK),
        ] {
            let request = Request::post(format!("/_port/cloud115/{path}"))
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap();
            assert_eq!(
                app.clone().oneshot(request).await.unwrap().status(),
                expected,
                "{path}"
            );
        }
    }
}
