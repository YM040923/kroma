use std::sync::atomic::Ordering;
use std::sync::Arc;

use kroma_module_supervisor::{FolderRoots, SelectedRoot};

use kroma_module_host::{NetworkAction, NetworkRoute};

mod component_support;

use component_support::wasm::guest_component;
use component_support::{http_server, Server, DECLARES, GUEST};

const NOTHING: &str = r#", "storage": {}"#;

#[test]
fn a_selected_nfo_download_lands_in_its_windows_or_unix_grant() {
    let server = Server::holding("selected-nfo", |machine| {
        let folder = machine.join("影片");
        std::fs::create_dir_all(folder.join("有 空格")).unwrap();
        FolderRoots {
            selected: Arc::new(move |id| {
                if id == GUEST {
                    vec![SelectedRoot {
                        guest: "/selected/films".into(),
                        host: folder.clone(),
                    }]
                } else {
                    vec![]
                }
            }),
            ..FolderRoots::none()
        }
    });
    let permissions = r#", "permissions": {"net":{"connect":[{"purpose":"probe","to":["lan"]}]},"fs":{"read":["module","selected"],"write":["selected"]}}"#;
    let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/component_guest");
    let component = component_support::wasm::build(&crate_dir, "asset");
    let endpoint = server.started_from(
        GUEST,
        &component,
        permissions,
        component_support::THIRD_PARTY,
    );
    let (port, hits) = http_server("<movie><title>中文</title></movie>");
    let get = |mode: &str| {
        server
            .get(&endpoint, &format!("/fixture-download/{port}?{mode}"))
            .text()
    };
    assert!(get("other").starts_with("refused:"));
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    assert_eq!(get("granted"), "200");
    let written = server.dir.path().join("影片/有 空格/电影 (2026).nfo");
    assert_eq!(
        std::fs::read_to_string(&written).unwrap(),
        "<movie><title>中文</title></movie>"
    );
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    let (missing, _) = component_support::http_server_status("404 Not Found", "not an NFO");
    assert_eq!(
        server
            .get(&endpoint, &format!("/fixture-download/{missing}?granted"))
            .text(),
        "404"
    );
    assert_eq!(
        std::fs::read_to_string(written).unwrap(),
        "<movie><title>中文</title></movie>"
    );
}

#[test]
fn a_call_a_component_makes_goes_through_its_gateway_and_is_journaled() {
    let server = Server::new("component-fetch", NetworkAction::Direct);
    let endpoint = server.started(GUEST, guest_component(), DECLARES);
    let (port, hits) = http_server("from the target");

    let answered = server.get(&endpoint, &format!("/fetch?http://127.0.0.1:{port}/"));

    assert_eq!(answered.text(), "200 from the target");
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    let flows = server.network.flows.lock().unwrap();
    assert_eq!(flows[0].purpose, "probe");
    assert_eq!(flows[0].route, NetworkRoute::Direct);
}

#[test]
fn a_call_the_policy_refuses_reaches_nothing_and_says_why() {
    let server = Server::new("component-refused", NetworkAction::Block);
    let endpoint = server.started(GUEST, guest_component(), DECLARES);
    let (port, hits) = http_server("from the target");

    let answered = server.get(&endpoint, &format!("/fetch?http://127.0.0.1:{port}/"));

    assert!(
        answered.text().contains("not for you"),
        "{}",
        answered.text()
    );
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    assert_eq!(
        server.network.flows.lock().unwrap()[0].route,
        NetworkRoute::Blocked
    );
}

#[test]
fn a_component_that_declares_nothing_has_no_network() {
    let server = Server::new("component-undeclared", NetworkAction::Direct);
    let endpoint = server.started(GUEST, guest_component(), NOTHING);
    let (port, hits) = http_server("from the target");

    let answered = server.get(&endpoint, &format!("/fetch?http://127.0.0.1:{port}/"));

    assert!(
        answered.text().contains("no network"),
        "{}",
        answered.text()
    );
    assert_eq!(hits.load(Ordering::SeqCst), 0);
}
