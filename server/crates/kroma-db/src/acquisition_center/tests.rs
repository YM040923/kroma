use super::*;
use crate::testing::TempPool;

fn pool() -> TempPool {
    crate::testing::temp_pool("acq-centre")
}

fn watch() -> WatchTitle {
    WatchTitle {
        id: "w1".into(),
        tmdb_id: 1396,
        kind: "show".into(),
        title: "Breaking Bad".into(),
        year: Some(2008),
        poster_url: None,
        seasons: vec![
            SeasonStart {
                season: 1,
                start_episode: 2,
            },
            SeasonStart {
                season: 2,
                start_episode: 1,
            },
        ],
        source_order: vec!["madreel".into(), "download".into()],
        enabled: true,
        last_checked: None,
        created_at: 10,
        updated_at: 10,
    }
}

fn share() -> ShareSubscription {
    ShareSubscription {
        id: "s1".into(),
        tmdb_id: 1396,
        kind: "show".into(),
        title: "Breaking Bad".into(),
        poster_url: None,
        share_code: "abc".into(),
        receive_code: "1234".into(),
        seasons: vec![SeasonStart {
            season: 1,
            start_episode: 1,
        }],
        enabled: true,
        status: "active".into(),
        last_checked: None,
        last_error: String::new(),
        created_at: 10,
        updated_at: 10,
    }
}

fn share_file(id: &str, file_id: &str, episode: u32) -> ShareFile {
    ShareFile {
        id: id.into(),
        subscription_id: "s1".into(),
        file_id: file_id.into(),
        name: format!("S01E{episode:02}.mkv"),
        size: Some(100),
        sha: None,
        season: Some(1),
        episode: Some(episode),
        state: "seen".into(),
        remote_id: None,
        error: String::new(),
        first_seen: 10,
        updated_at: 10,
    }
}

fn job(id: &str, status: &str) -> AcquisitionJob {
    AcquisitionJob {
        id: id.into(),
        kind: "one_shot".into(),
        unit_key: String::new(),
        tmdb_id: 1396,
        title: "Breaking Bad".into(),
        media_kind: "show".into(),
        season: Some(1),
        episode: Some(2),
        destination: Destination::CloudIntake,
        source: "madreel".into(),
        candidate: None,
        status: status.into(),
        stage_detail: String::new(),
        price_points: Some(3),
        price_known: true,
        paid_confirmed: false,
        remote_id: String::new(),
        progress: None,
        error_code: String::new(),
        error_message: String::new(),
        created_at: 10,
        updated_at: 10,
        finished_at: None,
    }
}

#[test]
fn a_watch_title_keeps_its_per_season_start_episode() {
    let p = pool();
    let conn = p.get().unwrap();
    upsert_watch_title(&conn, &watch()).unwrap();

    let stored = get_watch_title(&conn, "show", 1396).unwrap().unwrap();
    assert_eq!(
        stored.seasons,
        vec![
            SeasonStart {
                season: 1,
                start_episode: 2
            },
            SeasonStart {
                season: 2,
                start_episode: 1
            }
        ]
    );
    assert_eq!(stored.source_order, ["madreel", "download"]);
}

#[test]
fn a_share_file_is_listed_once_per_link() {
    let p = pool();
    let conn = p.get().unwrap();
    upsert_share_subscription(&conn, &share()).unwrap();

    assert!(note_share_file(&conn, &share_file("f1", "file-1", 1)).unwrap());
    assert!(!note_share_file(&conn, &share_file("f1b", "file-1", 1)).unwrap());

    let files = list_share_files(&conn, "s1").unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].file_id, "file-1");
}

#[test]
fn a_payment_is_confirmed_once_and_only_while_waiting() {
    let p = pool();
    let conn = p.get().unwrap();
    upsert_job(&conn, &job("j1", "waiting_confirm")).unwrap();
    drop(conn);

    assert!(confirm_job_payment(&p, "j1", 20).unwrap());
    assert!(!confirm_job_payment(&p, "j1", 21).unwrap());

    let conn = p.get().unwrap();
    let stored = get_job(&conn, "j1").unwrap().unwrap();
    assert!(stored.paid_confirmed);
    assert_eq!(stored.updated_at, 20);
    drop(conn);

    let conn = p.get().unwrap();
    upsert_job(&conn, &job("j2", "searching")).unwrap();
    drop(conn);
    assert!(
        !confirm_job_payment(&p, "j2", 22).unwrap(),
        "a job that is not waiting for a quote cannot record a payment"
    );
}

#[test]
fn disabling_a_watch_hides_it_from_the_enabled_list() {
    let p = pool();
    let conn = p.get().unwrap();
    upsert_watch_title(&conn, &watch()).unwrap();
    drop(conn);

    assert!(set_watch_enabled(&p, "show", 1396, false, 30).unwrap());
    let conn = p.get().unwrap();
    assert!(list_watch_titles(&conn, true).unwrap().is_empty());
    assert_eq!(list_watch_titles(&conn, false).unwrap().len(), 1);
}
