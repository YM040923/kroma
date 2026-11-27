use axum::http::StatusCode;
use serde_json::json;

use crate::api::test_support::{get, seed_session, send, test_app, test_app_with_releases};
use crate::model::Permission;
use crate::services::updates::STATUS_KEY;

#[tokio::test]
async fn windows_package_ignores_old_releases_and_never_schedules_or_installs_one() {
    let t = crate::api::test_support::test_app_windows_package();
    let cached = json!({"channel":"Stable", "latest":{"version":"999.0.0", "build":1,"url":"https://example.test/old"}, "checkedAt":"2026-01-01T00:00:00Z", "error":null});
    let pending = json!({"id":"old-request", "target":{"channel":"Stable", "version":"999.0.0", "build":1}, "phase":"queued", "requestedAt":"2026-01-01T00:00:00Z"});
    t.state.settings.set_internal(&t.state.db, STATUS_KEY, cached.clone()).unwrap();
    t.state.settings.set_internal(&t.state.db, crate::services::updates::OPERATION_KEY, pending.clone()).unwrap();
    t.state.settings.set_patch(&t.state.db, [("autoInstallUpdates".into(), json!(true))].into());
    crate::services::updates::recover(&t.state).unwrap();
    assert_eq!(t.state.settings.get(crate::services::updates::OPERATION_KEY), pending);
    assert!(t.state.jobs.resolve("update.check").is_none());
    assert!(t.state.jobs.resolve("update.install").is_none());
    assert!(t.state.jobs.resolve("library.scan").is_some());
    let (status, body) = get(&t.app, "/api/admin/updates", Some(&t.token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["updateSource"], "windows-package");
    assert!(body["version"].is_string());
    assert!(body.get("channel").is_none());
    assert!(body["checkedAt"].is_null() && body["latest"].is_null() && body["operation"].is_null());
    for key in ["auto", "autoInstall", "available"] {
        assert_eq!(body[key], false, "{key}");
    }
    assert_eq!(body["installer"]["ready"], false);
    assert_eq!(send(&t.app, "POST", "/api/admin/updates/check", Some(&t.token), None).await.0, StatusCode::CONFLICT);
    assert_eq!(send(&t.app, "POST", "/api/admin/updates/install", Some(&t.token), Some(json!({"channel":"Stable", "version":"999.0.0", "build":1}))).await.0, StatusCode::CONFLICT);
    assert_eq!(send(&t.app, "PUT", "/api/admin/settings", Some(&t.token), Some(json!({"updateChannel":"Canary"}))).await.0, StatusCode::CONFLICT);
    let (_, settings) = get(&t.app, "/api/admin/settings?view=general", Some(&t.token)).await;
    let has_key = |key: &str| settings["groups"].as_array().unwrap().iter().flat_map(|group| group["rows"].as_array().unwrap()).any(|row| row["key"] == key);
    assert!(has_key("serverName"));
    assert!(!has_key("updateChannel") && !has_key("autoUpdate"));
    assert!(crate::services::updates::check(&t.state).is_err());
    assert!(crate::services::updates::install_automatically(&t.state).unwrap().is_none());
    assert_eq!(t.state.settings.get(STATUS_KEY), cached);
    assert!(!t.state.config.data_dir.join("update-backups").exists());
    assert!(!t.state.config.update_dir.as_ref().unwrap().join("request.json").exists());
}

#[tokio::test]
async fn a_server_that_never_checked_says_so_and_offers_nothing() {
    let t = test_app();

    let (status, body) = get(&t.app, "/api/admin/updates", Some(&t.token)).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["channel"], json!("Stable"));
    assert_eq!(body["available"], json!(false));
    assert!(body["latest"].is_null());
    assert!(body["checkedAt"].is_null());
}

#[tokio::test]
async fn a_stored_newer_release_is_offered_on_its_channel() {
    let t = test_app();
    t.state
        .settings
        .set_internal(
            &t.state.db,
            STATUS_KEY,
            json!({
                "channel": "Stable",
                "latest": { "version": "999.0.0", "build": null, "url": "https://x", "publishedAt": null },
                "checkedAt": "2026-10-20T10:00:00Z",
                "error": null,
            }),
        )
        .unwrap();

    let (_, body) = get(&t.app, "/api/admin/updates", Some(&t.token)).await;

    assert_eq!(body["available"], json!(true));
    assert_eq!(body["latest"]["version"], json!("999.0.0"));
    assert_eq!(body["checkedAt"], json!("2026-10-20T10:00:00Z"));
}

#[tokio::test]
async fn a_member_cannot_read_or_run_the_check() {
    let t = test_app();
    let ana = seed_session(&t.state, "ana@test.dev", "ana", &[Permission::Playback]).1;

    let (read, _) = get(&t.app, "/api/admin/updates", Some(&ana)).await;
    let (run, _) = send(&t.app, "POST", "/api/admin/updates/check", Some(&ana), None).await;

    assert_eq!(read, StatusCode::FORBIDDEN);
    assert_eq!(run, StatusCode::FORBIDDEN);
    let (install, _) = send(
        &t.app,
        "POST",
        "/api/admin/updates/install",
        Some(&ana),
        Some(json!({"channel":"Stable", "version":"999.0.0", "build":1})),
    )
    .await;
    assert_eq!(install, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn an_install_without_a_host_updater_explains_the_conflict() {
    let t = test_app();
    let (status, body) = send(
        &t.app,
        "POST",
        "/api/admin/updates/install",
        Some(&t.token),
        Some(json!({"channel":"Stable", "version":"999.0.0", "build":1})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body["error"].as_str().unwrap().contains("host updater"));
}

async fn spawn_feed() -> String {
    let release = json!({
        "tag_name": "v999.0.0",
        "html_url": "https://github.com/kromatv/kroma/releases/tag/v999.0.0",
        "assets": [],
    });
    let app = axum::Router::new().route(
        "/latest",
        axum::routing::get(move || {
            let release = release.clone();
            async move { axum::Json(release) }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn an_owner_can_ask_the_feed_now() {
    let t = test_app_with_releases(&spawn_feed().await);

    let (status, body) = send(
        &t.app,
        "POST",
        "/api/admin/updates/check",
        Some(&t.token),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["latest"]["version"], json!("999.0.0"));
    assert_eq!(body["available"], json!(true));
}

#[tokio::test]
async fn changing_the_channel_checks_the_new_one() {
    let t = test_app();

    let (status, _) = send(
        &t.app,
        "PUT",
        "/api/admin/settings",
        Some(&t.token),
        Some(json!({ "updateChannel": "Canary" })),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let mut checked = None;
    for _ in 0..50 {
        let (_, body) = get(&t.app, "/api/admin/updates", Some(&t.token)).await;
        if !body["checkedAt"].is_null() {
            checked = Some(body);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let body = checked.expect("a check ran after the channel changed");
    assert_eq!(body["channel"], json!("Canary"));
    assert!(body["error"].is_string());
}

#[tokio::test]
async fn health_names_the_channel_a_desktop_app_should_follow() {
    let t = test_app();
    t.state.settings.set_patch(
        &t.state.db,
        [("updateChannel".to_string(), json!("Canary"))].into(),
    );

    let (_, body) = get(&t.app, "/api/health", None).await;

    assert_eq!(body["updateChannel"], json!("Canary"));
}

#[tokio::test]
async fn automatic_installation_enables_checks_and_disabling_checks_revokes_it() {
    let t = test_app();
    let (code, _) = send(
        &t.app,
        "PUT",
        "/api/admin/settings",
        Some(&t.token),
        Some(json!({"autoInstallUpdates": true})),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    let (_, body) = get(&t.app, "/api/admin/updates", Some(&t.token)).await;
    assert_eq!(body["autoInstall"], json!(true));
    assert_eq!(body["auto"], json!(true));
    send(
        &t.app,
        "PUT",
        "/api/admin/settings",
        Some(&t.token),
        Some(json!({"autoUpdate": false})),
    )
    .await;
    let (_, body) = get(&t.app, "/api/admin/updates", Some(&t.token)).await;
    assert_eq!(body["autoInstall"], json!(false));
}
