use super::super::batches::BatchSize;
use super::super::found::Found;
use super::*;
use crate::model::{Kind, MediaItem, Show};
use crate::services::settings::LibraryDef;
use std::collections::HashMap;
use std::convert::Infallible;
use std::ffi::OsStr;

#[test]
fn has_video_extension_matches_known_containers_case_insensitively() {
    assert!(has_video_extension(Path::new("/movies/Film.mkv")));
    assert!(has_video_extension(Path::new("/movies/Film.MP4"))); // case-insensitive
    assert!(has_video_extension(Path::new("clip.MOV")));
    assert!(has_video_extension(Path::new("a.ts")));
    // Non-video and edge cases.
    assert!(!has_video_extension(Path::new("/movies/poster.jpg")));
    assert!(!has_video_extension(Path::new("/movies/subs.srt")));
    assert!(!has_video_extension(Path::new("/movies/noext")));
    assert!(!has_video_extension(Path::new("/movies/.hidden")));
}

#[test]
fn is_pruned_dir_skips_synology_and_hidden() {
    assert!(is_pruned_dir(OsStr::new("@eaDir")));
    assert!(is_pruned_dir(OsStr::new(".git")));
    assert!(is_pruned_dir(OsStr::new(".DS_Store")));
    assert!(!is_pruned_dir(OsStr::new("Season 1")));
    assert!(!is_pruned_dir(OsStr::new("Movies")));
}

#[test]
fn walk_threads_default_range_and_env_override() {
    // Default (no env): clamped into [8, 32].
    std::env::remove_var("KROMA_WALK_THREADS");
    let n = walk_threads();
    assert!((8..=32).contains(&n), "default {n} out of clamp range");

    // Explicit override is honored verbatim (not re-clamped).
    std::env::set_var("KROMA_WALK_THREADS", "3");
    assert_eq!(walk_threads(), 3);
    std::env::set_var("KROMA_WALK_THREADS", "100");
    assert_eq!(walk_threads(), 100);

    // A non-numeric value falls back to the computed default.
    std::env::set_var("KROMA_WALK_THREADS", "not-a-number");
    assert!((8..=32).contains(&walk_threads()));

    std::env::remove_var("KROMA_WALK_THREADS");
}

fn temp_scan_dir() -> kroma_testing::TempDir {
    kroma_testing::temp_dir("scanroot")
}

struct ScanOutcome {
    items: HashMap<String, MediaItem>,
    shows: HashMap<String, Show>,
    mtimes: HashMap<String, Option<i64>>,
    item_ids: HashSet<String>,
    movie_seen: bool,
    episode_seen: bool,
    photo_seen: bool,
    other_seen: bool,
    unreadable: usize,
}

fn scan_into(lib_id: &str, root: &Path) -> ScanOutcome {
    scan_into_kind(lib_id, root, false)
}

fn scan_into_kind(lib_id: &str, root: &Path, tv_library: bool) -> ScanOutcome {
    let def = LibraryDef {
        id: lib_id.into(),
        name: "Library".into(),
        kind: if tv_library { "shows".into() } else { String::new() },
        folders: vec![root.to_string_lossy().into_owned()],
        auto_scan: true,
    };
    let mut found = Found::default();
    let mut lib = LibraryWalk::new(&def, &mut found);
    let mut unobserved = |_: &crate::db::ScanBatch<'_>| Ok::<(), Infallible>(());
    let mut batches = Batches::new(BatchSize::LIVE, &mut unobserved);

    let Ok(unreadable) = scan_root(root, &mut lib, &mut batches);

    ScanOutcome {
        item_ids: lib.item_ids,
        movie_seen: lib.movie_seen,
        episode_seen: lib.episode_seen,
        photo_seen: lib.photo_seen,
        other_seen: lib.other_seen,
        items: found.items,
        shows: found.shows,
        mtimes: found.mtimes,
        unreadable,
    }
}

#[test]
fn scan_root_groups_movies_and_episodes_and_skips_noise() {
    let root_dir = temp_scan_dir();
    let root = root_dir.path();
    // One movie (year in filename), a show with two episodes in its own folder,
    // a non-video file, and a hidden dir that must be pruned from the descent.
    std::fs::write(root.join("The Matrix (1999).mkv"), b"x").unwrap();
    let show_dir = root.join("Breaking Bad");
    std::fs::create_dir_all(&show_dir).unwrap();
    std::fs::write(show_dir.join("Breaking Bad S01E01.mkv"), b"x").unwrap();
    std::fs::write(show_dir.join("Breaking Bad S01E02.mkv"), b"x").unwrap();
    std::fs::write(root.join("poster.jpg"), b"x").unwrap();
    let hidden = root.join(".hidden");
    std::fs::create_dir_all(&hidden).unwrap();
    std::fs::write(hidden.join("Ghost (2000).mkv"), b"x").unwrap();

    let scan = scan_into("lib1", root);

    assert!(scan.movie_seen);
    assert!(scan.episode_seen);
    // 1 movie + 2 episodes = 3 logical items; jpg and hidden dir ignored.
    assert_eq!(scan.items.len(), 3, "movie + 2 episodes, noise excluded");
    assert_eq!(scan.shows.len(), 1, "both episodes grouped under one show");
    assert_eq!(scan.item_ids.len(), 3);
    assert_eq!(
        scan.mtimes.len(),
        3,
        "an mtime is recorded per scanned file"
    );

    let movie = scan.items.values().find(|i| i.kind == Kind::Movie).unwrap();
    assert_eq!(movie.title, "The Matrix");
    assert_eq!(movie.year, Some(1999));
    assert_eq!(movie.files.len(), 1);
    assert_eq!(movie.library, "lib1");

    let show = scan.shows.values().next().unwrap();
    assert_eq!(show.title, "Breaking Bad");
    let episodes: Vec<&MediaItem> = scan
        .items
        .values()
        .filter(|i| i.kind == Kind::Episode)
        .collect();
    assert_eq!(episodes.len(), 2);
    assert!(episodes
        .iter()
        .all(|e| e.show_id.as_deref() == Some(show.id.as_str())));
}

#[test]
fn a_directory_symlinked_to_itself_is_walked_once() {
    let root_dir = temp_scan_dir();
    let root = root_dir.path();
    std::fs::write(root.join("The Matrix (1999).mkv"), b"x").unwrap();
    std::os::unix::fs::symlink(".", root.join("loop")).unwrap();

    let scan = scan_into("lib1", root);

    assert_eq!(scan.items.len(), 1);
    assert_eq!(
        scan.items.values().next().unwrap().files.len(),
        1,
        "the loop yielded the file again"
    );
}

#[test]
fn a_library_of_symlinks_indexes_what_they_point_at() {
    let root_dir = temp_scan_dir();
    let base = root_dir.path();
    let dump = base.join("dump");
    std::fs::create_dir_all(&dump).unwrap();
    std::fs::write(dump.join("The Matrix (1999).mkv"), b"x").unwrap();
    std::fs::write(dump.join("Breaking.Bad.S01E01.mkv"), b"x").unwrap();
    std::fs::write(dump.join("Breaking.Bad.S01E02.mkv"), b"x").unwrap();
    let root = base.join("4k Movies");
    std::fs::create_dir_all(&root).unwrap();
    for name in [
        "The Matrix (1999).mkv",
        "Breaking.Bad.S01E01.mkv",
        "Breaking.Bad.S01E02.mkv",
    ] {
        std::os::unix::fs::symlink(dump.join(name), root.join(name)).unwrap();
    }

    let scan = scan_into("lib1", &root);

    assert_eq!(scan.unreadable, 0);
    assert_eq!(scan.items.len(), 3);
    assert_eq!(scan.shows.len(), 1, "both episodes grouped under one show");
    let matrix = scan
        .items
        .values()
        .find(|i| i.kind == Kind::Movie)
        .expect("the symlinked movie is indexed");
    assert_eq!(matrix.files[0].size, Some(1), "the target was stat-ed");
}

#[test]
fn a_symlinked_folder_of_episodes_groups_them_by_filename() {
    let root_dir = temp_scan_dir();
    let base = root_dir.path();
    let dump = base.join("dump");
    std::fs::create_dir_all(&dump).unwrap();
    std::fs::write(dump.join("Breaking.Bad.S01E01.mkv"), b"x").unwrap();
    std::fs::write(dump.join("Breaking.Bad.S01E02.mkv"), b"x").unwrap();
    std::fs::write(dump.join("The.Office.S01E01.mkv"), b"x").unwrap();
    let root = base.join("4k Shows");
    std::fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(&dump, root.join("all")).unwrap();

    let scan = scan_into("lib1", &root);

    let mut titles: Vec<&str> = scan.shows.values().map(|s| s.title.as_str()).collect();
    titles.sort_unstable();
    assert_eq!(titles, ["Breaking Bad", "The Office"]);
    assert_eq!(scan.items.len(), 3);
}

#[test]
fn a_symlink_whose_target_is_gone_is_counted_rather_than_dropped() {
    let root_dir = temp_scan_dir();
    let root = root_dir.path();
    std::fs::write(root.join("The Matrix (1999).mkv"), b"x").unwrap();
    std::os::unix::fs::symlink("/nowhere/Gone (2014).mkv", root.join("Gone (2014).mkv")).unwrap();
    std::os::unix::fs::symlink("/nowhere/shows", root.join("Shows")).unwrap();

    let scan = scan_into("lib1", root);

    assert_eq!(scan.unreadable, 2);
    assert_eq!(scan.items.len(), 1, "only the real file is indexed");
}

#[test]
fn scan_root_untitled_movie_gets_placeholder_title() {
    let root_dir = temp_scan_dir();
    let root = root_dir.path();
    // A bare numeric/year-only name parses to an empty movie title, which
    // index_parsed replaces with "Untitled".
    std::fs::write(root.join("1999.mkv"), b"x").unwrap();

    let scan = scan_into("lib1", root);

    assert!(scan.movie_seen);
    assert!(!scan.episode_seen);
    assert_eq!(scan.items.len(), 1);
    assert_eq!(scan.items.values().next().unwrap().title, "Untitled");
}

#[test]
fn a_strm_is_indexed_by_the_url_it_holds() {
    let root_dir = temp_scan_dir();
    let root = root_dir.path();
    std::fs::write(
        root.join("Dune (2021).strm"),
        b"https://example.test/drive/dune.mkv\n",
    )
    .unwrap();

    let scan = scan_into("lib1", root);

    assert!(scan.movie_seen);
    assert_eq!(scan.items.len(), 1);
    let item = scan.items.values().next().unwrap();
    assert_eq!(item.title, "Dune");
    assert_eq!(item.year, Some(2021));
    let file = &item.files[0];
    assert_eq!(file.container, "strm");
    assert_eq!(
        file.abs_path.as_deref(),
        Some("https://example.test/drive/dune.mkv"),
        "the catalog carries the URL, not the .strm on disk"
    );
    assert_eq!(
        file.id,
        super::short_hash("https://example.test/drive/dune.mkv"),
        "the row is identified by the target, so rewriting the URL keeps it"
    );
}

#[test]
fn a_strm_without_a_url_is_not_indexed() {
    let root_dir = temp_scan_dir();
    let root = root_dir.path();
    std::fs::write(root.join("Broken (2020).strm"), b"not a url\n").unwrap();

    let scan = scan_into("lib1", root);

    assert_eq!(scan.items.len(), 0, "a .strm that names nothing is skipped");
}

#[test]
fn a_strm_tolerates_a_bom_and_leading_blank_lines() {
    let root_dir = temp_scan_dir();
    let root = root_dir.path();
    std::fs::write(
        root.join("Arrival (2016).strm"),
        "\u{feff}\n\n   https://example.test/a.mkv   \nhttps://example.test/ignored.mkv\n".as_bytes(),
    )
    .unwrap();

    let scan = scan_into("lib1", root);

    let item = scan.items.values().next().unwrap();
    assert_eq!(
        item.files[0].abs_path.as_deref(),
        Some("https://example.test/a.mkv"),
        "the first URL wins, trimmed, with the BOM stripped"
    );
}

#[test]
fn a_strm_with_a_sidecar_arrives_described_and_probed() {
    let root_dir = temp_scan_dir();
    let root = root_dir.path();
    std::fs::write(
        root.join("Dune (2021).strm"),
        b"https://example.test/drive/dune.mkv\n",
    )
    .unwrap();
    // What a bridge writes: duration in ms, size in bytes, one video stream.
    std::fs::write(
        root.join("Dune (2021).info.json"),
        br#"{
            "duration_ms": 9330000,
            "size": 21474836480,
            "video": { "codec": "hevc", "width": 3840, "height": 2160, "hdr": true },
            "audio": { "codec": "eac3", "channels": 6, "language": "eng" },
            "audio_tracks": [ { "codec": "eac3", "channels": 6, "language": "eng" } ],
            "subtitles": [ { "language": "eng", "codec": "subrip", "title": "English" } ]
        }"#,
    )
    .unwrap();

    let scan = scan_into("lib1", root);

    let item = scan.items.values().next().unwrap();
    let file = &item.files[0];
    assert!(
        file.probed,
        "a described entry must not be handed to a probe that cannot open a URL"
    );
    assert_eq!(file.duration_ms, Some(9_330_000));
    assert_eq!(file.size, Some(21_474_836_480));
    let video = file.video.as_ref().expect("the sidecar named a video stream");
    assert_eq!(video.codec, "hevc");
    assert_eq!(video.width, Some(3840));
    assert_eq!(file.audio_tracks.len(), 1, "the audio tracks survive");
    assert_eq!(file.subtitles.len(), 1, "the subtitle tracks survive");
}

#[test]
fn a_strm_without_a_sidecar_stays_unprobed() {
    let root_dir = temp_scan_dir();
    let root = root_dir.path();
    std::fs::write(
        root.join("Arrival (2016).strm"),
        b"https://example.test/a.mkv\n",
    )
    .unwrap();

    let scan = scan_into("lib1", root);

    let file = &scan.items.values().next().unwrap().files[0];
    assert!(!file.probed, "nothing described it, so a later pass may try");
    assert_eq!(file.duration_ms, None);
}

#[test]
fn a_malformed_sidecar_is_ignored_rather_than_failing_the_scan() {
    let root_dir = temp_scan_dir();
    let root = root_dir.path();
    std::fs::write(
        root.join("Solaris (1972).strm"),
        b"https://example.test/s.mkv\n",
    )
    .unwrap();
    std::fs::write(root.join("Solaris (1972).info.json"), b"{ not json").unwrap();

    let scan = scan_into("lib1", root);

    let file = &scan.items.values().next().unwrap().files[0];
    assert!(!file.probed, "an unreadable sidecar describes nothing");
    assert_eq!(
        file.abs_path.as_deref(),
        Some("https://example.test/s.mkv"),
        "the entry itself still indexed"
    );
}

#[test]
fn a_tv_library_drops_an_unnumbered_file_from_a_season_folder() {
    // The stray shape a mirror produces now and then: a file in a bare
    // "Season" folder, no SxxExx anywhere. It cannot be numbered, and in a TV
    // library it must not surface as a film either.
    let root_dir = temp_scan_dir();
    let root = root_dir.path();
    let show_dir = root.join("Some Show (2023) {tmdb-1}");
    std::fs::create_dir_all(show_dir.join("Season")).unwrap();
    std::fs::write(show_dir.join("Season/Some Show - 1080p.mkv"), b"x").unwrap();
    std::fs::create_dir_all(show_dir.join("Season 1")).unwrap();
    std::fs::write(show_dir.join("Season 1/Some Show S01E01.mkv"), b"x").unwrap();

    let tv = scan_into_kind("lib1", root, true);
    assert_eq!(tv.items.len(), 1, "only the numbered episode is indexed");
    assert!(tv.items.values().all(|i| i.kind == kroma_domain::Kind::Episode));

    let films = scan_into_kind("lib1", root, false);
    assert_eq!(films.items.len(), 2, "a film library still takes the file");
}

#[test]
fn takes_says_of_each_file_what_a_walk_does_with_it() {
    let dir = temp_scan_dir();
    let files = [
        "Heat (1995)/Heat (1995).mkv",
        "Heat (1995)/Heat (1995).wmv",
        "Heat (1995)/poster.jpg",
        "@eaDir/Ronin (1998).mkv",
        ".trash/Thief (1981).mkv",
        "Dark/.extras/Dark S01E01.mkv",
        "Dark/Season 01/Dark S01E02.MKV",
        ".Alien (1979).mkv",
    ];
    for file in files {
        let path = dir.path().join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"x").unwrap();
    }

    let walked = scan_into("lib", dir.path());

    let written: HashSet<String> = walked
        .items
        .values()
        .flat_map(|item| &item.files)
        .filter_map(|file| file.rel_path.clone())
        .collect();
    for file in files {
        assert_eq!(takes(Path::new(file)), written.contains(file), "{file}");
    }
    assert_eq!(written.len(), 3);
}

#[test]
fn an_nfo_overrides_the_filename_and_show_folder() {
    let dir = temp_scan_dir();
    let root = dir.path();
    let show = root.join("Wrong Show");
    let season = show.join("Season 1");
    std::fs::create_dir_all(&season).unwrap();
    std::fs::write(
        show.join("tvshow.nfo"),
        "<tvshow><title>Right Show</title><year>2020</year></tvshow>",
    )
    .unwrap();
    std::fs::write(
        season.join("Wrong Show S01E01.mkv"),
        b"x",
    )
    .unwrap();
    std::fs::write(
        season.join("Wrong Show S01E01.nfo"),
        "<episodedetails><title>Right Episode</title><season>2</season><episode>4</episode></episodedetails>",
    )
    .unwrap();

    let scan = scan_into("lib", root);
    let episode = scan
        .items
        .values()
        .find(|item| item.kind == kroma_domain::Kind::Episode)
        .expect("the episode");
    assert_eq!(episode.show_title.as_deref(), Some("Right Show"));
    assert_eq!(episode.season, Some(2));
    assert_eq!(episode.episode, Some(4));
    assert_eq!(episode.episode_title.as_deref(), Some("Right Episode"));
    assert_eq!(episode.year, Some(2020));
}
