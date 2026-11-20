use axum::http::StatusCode;
use serde_json::json;

use crate::api::test_support::{demo_item_id, get, raw, seed_session, send, test_app};
use crate::model::Permission;

#[tokio::test]
async fn a_share_link_unlocks_streams_and_is_revoked() {
    let t = test_app();
    let item = demo_item_id("The Matrix");
    let (status, body) = send(
        &t.app,
        "POST",
        &format!("/api/items/{item}/share"),
        Some(&t.token),
        Some(json!({ "days": 7, "password": "abcd" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let slug = body["slug"].as_str().expect("a slug").to_string();
    assert_eq!(body["path"], json!(format!("/s/{slug}")));
    assert_eq!(body["password"], json!("abcd"));

    // Before the code, the page is not told what it holds.
    let (status, info) = get(&t.app, &format!("/api/share/{slug}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(info["requiresPassword"], json!(true));
    assert!(info["title"].is_null(), "{info}");

    let (status, _) = send(
        &t.app,
        "POST",
        &format!("/api/share/{slug}/unlock"),
        None,
        Some(json!({ "password": "wrong" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, headers, _) = raw(
        &t.app,
        "POST",
        &format!("/api/share/{slug}/unlock"),
        None,
        Some(json!({ "password": "abcd" })),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let cookie = headers
        .get("set-cookie")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .expect("a share cookie")
        .to_string();

    let (status, _, info) = raw(
        &t.app,
        "GET",
        &format!("/api/share/{slug}"),
        None,
        None,
        &[("cookie", &cookie)],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(info["title"], json!("The Matrix"));

    // Without the cookie the bytes are refused; with it the request reaches the
    // item (the demo row has no playable file, so anything but 401 proves it).
    let (status, _, _) = raw(
        &t.app,
        "GET",
        &format!("/api/share/{slug}/stream"),
        None,
        None,
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _, _) = raw(
        &t.app,
        "GET",
        &format!("/api/share/{slug}/stream"),
        None,
        None,
        &[("cookie", &cookie)],
    )
    .await;
    assert_ne!(status, StatusCode::UNAUTHORIZED);

    let (status, _) = send(
        &t.app,
        "DELETE",
        &format!("/api/items/{item}/share"),
        Some(&t.token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = get(&t.app, &format!("/api/share/{slug}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "a revoked link is gone");
}

#[tokio::test]
async fn only_a_manager_can_share() {
    let t = test_app();
    let (_, viewer) = seed_session(&t.state, "v@test.dev", "v", &[Permission::Playback]);
    let item = demo_item_id("The Matrix");

    let (status, _) = send(
        &t.app,
        "POST",
        &format!("/api/items/{item}/share"),
        Some(&viewer),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = get(&t.app, &format!("/api/items/{item}/share"), Some(&viewer)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
