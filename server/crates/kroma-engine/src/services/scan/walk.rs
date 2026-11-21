//! The phase-1 filesystem walk: a parallel jwalk traversal of one library
//! folder that `stat`s each video file and groups it (via [`crate::domain::naming`])
//! into the shared logical-item / show maps.

use std::collections::HashSet;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use jwalk::{Parallelism, WalkDirGeneric};
use tracing::{debug, warn};

use crate::domain::naming;
use crate::model::MediaFile;

use super::batches::Batches;
use super::found::LibraryWalk;
use super::ids::{detect_edition, short_hash};
use super::index::index_parsed;
use super::nfo;
use super::now_iso8601;

pub use kroma_media::video::{has_video_extension, VIDEO_EXTENSIONS};

const PHOTO_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "webp", "gif", "tif", "tiff", "bmp", "heic", "heif", "avif",
];

fn has_photo_extension(path: &Path) -> bool {
    path.extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|ext| {
            PHOTO_EXTENSIONS
                .iter()
                .any(|known| ext.eq_ignore_ascii_case(known))
        })
}

fn photo_mtime_date(mtime: i64) -> Option<String> {
    let date = time::OffsetDateTime::from_unix_timestamp(mtime)
        .ok()?
        .date();
    Some(format!(
        "{:04}-{:02}-{:02}T00:00:00Z",
        date.year(),
        u8::from(date.month()),
        date.day()
    ))
}

fn photo_taken_at(path: &Path) -> Option<String> {
    let file = File::open(path).ok()?;
    let exif = exif::Reader::new()
        .read_from_container(&mut BufReader::new(file))
        .ok()?;
    for tag in [
        exif::Tag::DateTimeOriginal,
        exif::Tag::DateTimeDigitized,
        exif::Tag::DateTime,
    ] {
        let Some(field) = exif.get_field(tag, exif::In::PRIMARY) else {
            continue;
        };
        let exif::Value::Ascii(values) = &field.value else {
            continue;
        };
        let Some(value) = values.first() else {
            continue;
        };
        let value = std::str::from_utf8(value)
            .ok()?
            .trim_matches(char::from(0))
            .trim();
        if value.len() >= 19
            && value.as_bytes()[4] == b':'
            && value.as_bytes()[7] == b':'
            && value.as_bytes()[10] == b' '
        {
            return Some(format!(
                "{}-{}-{}T{}",
                &value[..4],
                &value[5..7],
                &value[8..10],
                &value[11..19]
            ));
        }
    }
    None
}

/// Scan one folder of `lib`'s library, accumulating items (by logical id) and
/// shows into the shared maps. Flags/ids track what this library contributed so
/// the caller can compute its kind + item count across all its folders.
///
/// Returns how many entries the walk could not read: a symlink whose target is
/// not mounted resolves to nothing, and a whole library of those otherwise
/// scans clean and empty.
pub(super) fn scan_root<S>(
    root: &Path,
    lib: &mut LibraryWalk<'_>,
    batches: &mut Batches<'_, S>,
) -> Result<usize, S> {
    let tv_library = lib.def.kind == "shows";
    let lib_name = root
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("library")
        .to_string();
    let added_at = now_iso8601();

    // Resolve the root once (cheap, single syscall) so file abs-paths are stable
    // without a per-file `canonicalize()` (which is very slow over SMB).
    let abs_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());

    // Phase-1 walk: jwalk traverses directories on a large thread pool and stats
    // files *in that pool*, so the readdir/stat network round-trips over SMB run
    // concurrently instead of serially (minutes → seconds). Synology `@eaDir`
    // and hidden dirs are pruned from the descent.
    let descended = Arc::new(Mutex::new(HashSet::from([abs_root.clone()])));
    let walk = WalkDirGeneric::<((), FileMeta)>::new(root)
        .follow_links(true)
        .skip_hidden(false)
        .parallelism(Parallelism::RayonNewPool(walk_threads()))
        .process_read_dir(move |_depth, _path, _state, children| {
            prepare_children(&descended, children);
        });

    let mut unreadable = 0usize;
    let mut first_unreadable: Option<PathBuf> = None;
    for result in walk {
        if batches.due() {
            batches.close(&lib.library(), lib.found)?;
        }
        let entry = match result {
            Ok(entry) => entry,
            Err(err) => {
                unreadable += 1;
                first_unreadable.get_or_insert_with(|| err.path().unwrap_or(root).to_path_buf());
                debug!("skipping an unreadable entry: {err}");
                continue;
            }
        };
        if entry.file_type().is_dir() {
            let path = entry.path();
            if let Some((stream, container)) = disc_main_stream(&path) {
                let indexed =
                    index_disc(lib, &root, &abs_root, &path, &stream, container, &added_at);
                batches.note(indexed);
            }
            continue;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        // A disc's own streams live inside BDMV/VIDEO_TS; the folder above them
        // is the one item, and its main stream is the file that was indexed.
        if inside_disc(&path) {
            continue;
        }
        let photo_library = lib.def.kind == "photos" || lib.def.kind == "photo";
        let other_library = lib.def.kind == "other";
        if photo_library && !has_photo_extension(&path) {
            continue;
        }
        if other_library
            && !has_video_extension(&path)
            && !path.extension().is_some_and(|ext| {
                ext.to_str()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("strm"))
            })
        {
            continue;
        }
        let is_strm = path
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|ext| ext.eq_ignore_ascii_case("strm"));
        if !photo_library && !other_library && !is_strm && !has_video_extension(&path) {
            continue;
        }
        // A `.strm` holds one line: the URL the entry streams from. The path the
        // catalog carries is that URL, not the file on disk, so playback fetches
        // the origin and the file's own id stays put when the link is rewritten.
        let streamed = if is_strm {
            match read_strm_url(&path) {
                Some(url) => Some(url),
                None => {
                    debug!(path = %path.display(), "a .strm holds no usable URL; skipped");
                    continue;
                }
            }
        } else {
            None
        };
        let rel = path.trim_prefix(root).to_string_lossy().replace('\\', "/");
        // Build the absolute path by joining the resolved root with the relative
        // path no extra stat / symlink resolution per file.
        let abs = abs_root.join(&rel);

        let (size, mtime) = match entry.client_state {
            Some((s, m)) => (Some(s), Some(m)),
            None => (None, None),
        };

        let file_name = path
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or("");
        let edition = detect_edition(file_name);

        // A bridge that manages a cloud drive writes what ffprobe would have
        // learnt beside the `.strm`, because this server's ffprobe only accepts
        // the `file` protocol and the entry's path is a URL. Reading it here
        // means those entries arrive fully described and the probe pass, which
        // could only ever fail on them, leaves them alone.
        let sidecar = if is_strm { read_sidecar(&path) } else { None };

        let photo_time = photo_library
            .then(|| photo_taken_at(&path).or_else(|| mtime.and_then(photo_mtime_date)))
            .flatten();
        let file_added_at = photo_time.as_deref().unwrap_or(&added_at);
        let file = MediaFile {
            // A streamed entry is identified by what it points at, so rewriting
            // the URL inside the `.strm` keeps the same catalog row (and its
            // watch state, collections and request link) instead of orphaning it.
            id: {
                let by_url = streamed.clone();
                let by_path = abs.to_string_lossy();
                short_hash(by_url.as_deref().unwrap_or(&by_path))
            },
            rel_path: Some(rel),
            container: if is_strm {
                "strm".to_string()
            } else {
                path.extension()
                    .and_then(std::ffi::OsStr::to_str)
                    .unwrap_or("")
                    .to_ascii_lowercase()
            },
            duration_ms: sidecar.as_ref().and_then(|s| s.duration_ms),
            video: sidecar.as_ref().and_then(|s| s.video.clone()),
            audio: sidecar.as_ref().and_then(|s| s.audio.clone()),
            audio_tracks: sidecar
                .as_ref()
                .map(|s| s.audio_tracks.clone())
                .unwrap_or_default(),
            subtitles: sidecar
                .as_ref()
                .map(|s| s.subtitles.clone())
                .unwrap_or_default(),
            // A `.strm` is a few bytes and says nothing about the film; the
            // sidecar's size is the one a player cares about.
            size: sidecar.as_ref().and_then(|s| s.size).or(size),
            edition,
            // A sidecar is a verdict from a reader that could open the URL, so
            // the entry is described and the probe pass must not retry it.
            probed: sidecar.is_some() || photo_library,
            probe_version: if photo_library {
                Some(crate::model::MEDIA_PROBE_VERSION)
            } else {
                sidecar
                    .as_ref()
                    .filter(|s| s.unreadable.is_none())
                    .and_then(|s| s.probe_version)
            },
            unreadable: sidecar.as_ref().and_then(|s| s.unreadable.clone()),
            edition_id: None,
            abs_path: Some(match streamed {
                Some(url) => url,
                None => abs.to_string_lossy().to_string(),
            }),
        };
        let parsed = if photo_library || other_library {
            let title = path
                .file_stem()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or("Untitled")
                .to_string();
            naming::Parsed::Movie { title, year: None }
        } else {
            nfo::apply(naming::parse(root, &path), &path)
        };
        let item_kind = if photo_library {
            Some(crate::model::Kind::Photo)
        } else if other_library {
            Some(crate::model::Kind::Other)
        } else {
            None
        };
        // A TV library's season folder vouches for the show; a file in it
        // without an episode marker is an unnumbered copy, not a film that
        // belongs in the movies rows. Jellyfin drops these too.
        if tv_library && matches!(parsed, naming::Parsed::Movie { .. }) {
            let in_season = path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(naming::is_season_folder);
            if in_season {
                warn!(
                    path = %path.display(),
                    "no episode number in a season folder; left out of the TV library"
                );
                continue;
            }
        }
        batches.note(index_parsed(
            parsed,
            &lib.def.id,
            file_added_at,
            file,
            mtime,
            item_kind,
            &mut lib.found.items,
            &mut lib.found.shows,
            &mut lib.found.mtimes,
            &mut lib.item_ids,
            &mut lib.movie_seen,
            &mut lib.episode_seen,
            &mut lib.photo_seen,
            &mut lib.other_seen,
        ));

        debug!("indexed file under {}", lib_name);
    }

    if let Some(path) = first_unreadable {
        warn!(
            folder = %root.display(),
            unreadable,
            example = %path.display(),
            "entries skipped: a symlink whose target is not mounted reads as missing"
        );
    }
    Ok(unreadable)
}

const DISC_STREAM_EXTENSIONS: &[&str] = &["m2ts", "mts"];

/// The file a disc folder plays: the largest stream under `BDMV/STREAM`, or the
/// largest VOB under `VIDEO_TS`. `None` for an ordinary folder.
fn disc_main_stream(dir: &Path) -> Option<(PathBuf, &'static str)> {
    let bdmv = dir.join("BDMV").join("STREAM");
    if bdmv.is_dir() {
        return largest_with_extension(&bdmv, DISC_STREAM_EXTENSIONS).map(|path| (path, "bluray"));
    }
    let video_ts = dir.join("VIDEO_TS");
    if video_ts.is_dir() {
        return largest_with_extension(&video_ts, &["vob"]).map(|path| (path, "dvd"));
    }
    None
}

fn largest_with_extension(dir: &Path, extensions: &[&str]) -> Option<PathBuf> {
    let mut best: Option<(u64, PathBuf)> = None;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        let matches = path
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|ext| {
                extensions
                    .iter()
                    .any(|known| ext.eq_ignore_ascii_case(known))
            });
        if !matches {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        if best.as_ref().is_none_or(|(size, _)| meta.len() > *size) {
            best = Some((meta.len(), path));
        }
    }
    best.map(|(_, path)| path)
}

fn inside_disc(path: &Path) -> bool {
    path.ancestors().skip(1).any(|ancestor| {
        ancestor
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|name| {
                name.eq_ignore_ascii_case("BDMV") || name.eq_ignore_ascii_case("VIDEO_TS")
            })
    })
}

/// One disc folder as one item, backed by its main stream. Named after the
/// folder, so `Avatar (2009)/BDMV` is the film and not a pile of clips.
fn index_disc(
    lib: &mut LibraryWalk<'_>,
    root: &Path,
    abs_root: &Path,
    dir: &Path,
    stream: &Path,
    container: &'static str,
    added_at: &str,
) -> super::index::Indexed {
    let rel = stream
        .trim_prefix(root)
        .to_string_lossy()
        .replace('\\', "/");
    let abs = abs_root.join(&rel);
    let meta = std::fs::metadata(stream).ok();
    let size = meta.as_ref().map(std::fs::Metadata::len);
    let mtime = meta
        .as_ref()
        .and_then(|meta| meta.modified().ok())
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|since| since.as_secs() as i64);
    let file = MediaFile {
        id: short_hash(&abs.to_string_lossy()),
        rel_path: Some(rel),
        container: container.to_string(),
        duration_ms: None,
        video: None,
        audio: None,
        audio_tracks: Vec::new(),
        subtitles: Vec::new(),
        size,
        edition: None,
        // The probe pass reads the main stream itself.
        probed: false,
        probe_version: None,
        unreadable: None,
        edition_id: None,
        abs_path: Some(abs.to_string_lossy().to_string()),
    };
    let parsed = nfo::apply(naming::parse(root, dir), dir);
    index_parsed(
        parsed,
        &lib.def.id,
        added_at,
        file,
        mtime,
        None,
        &mut lib.found.items,
        &mut lib.found.shows,
        &mut lib.found.mtimes,
        &mut lib.item_ids,
        &mut lib.movie_seen,
        &mut lib.episode_seen,
        &mut lib.photo_seen,
        &mut lib.other_seen,
    )
}

// jwalk `process_read_dir` body: prune Synology/hidden dirs and already-visited
// symlink targets from the descent, and stat each file *in the walk's thread
// pool* (so the SMB round-trips overlap).
fn prepare_children(
    descended: &Mutex<HashSet<PathBuf>>,
    children: &mut Vec<jwalk::Result<jwalk::DirEntry<((), FileMeta)>>>,
) {
    children.retain(|res| match res {
        Ok(e) => !(e.file_type().is_dir() && is_pruned_dir(&e.file_name)),
        Err(_) => true,
    });
    for e in children.iter_mut().flatten() {
        if e.file_type().is_file() {
            e.client_state = file_meta(&e.path());
        } else if e.file_type().is_dir()
            && e.path_is_symlink()
            && !first_descent(descended, &e.path())
        {
            e.read_children = None;
        }
    }
}

// jwalk's own guard compares a symlink's raw target against the ancestor paths,
// so `link -> /abs/ancestor` is caught and `link -> .` is not: the kernel's
// 32-symlink ceiling then bounds the depth, which two such links in one
// directory turn into 2^32 paths.
fn first_descent(descended: &Mutex<HashSet<PathBuf>>, dir: &Path) -> bool {
    let Ok(resolved) = dir.canonicalize() else {
        return false;
    };
    match descended.lock() {
        Ok(mut seen) => seen.insert(resolved),
        Err(poisoned) => poisoned.into_inner().insert(resolved),
    }
}

// Per-file metadata carried through the parallel jwalk: (size, mtime-secs).
type FileMeta = Option<(u64, i64)>;

// Concurrency for the directory walk. Metadata ops are latency-bound (not CPU),
// so more threads than cores overlap the round-trips but a NAS serving its
// own local disks gains nothing past a handful, and 64 idle-blocked threads
// still cost stacks + scheduler churn on a 2-4 core box. `KROMA_WALK_THREADS`
// overrides for genuinely remote mounts.
fn walk_threads() -> usize {
    if let Some(n) = std::env::var("KROMA_WALK_THREADS")
        .ok()
        .and_then(|s| s.parse().ok())
    {
        return n;
    }
    let cores = std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(4);
    (cores * 4).clamp(8, 32)
}

// Directories pruned from the walk: Synology metadata (`@eaDir`) and hidden.
fn is_pruned_dir(name: &std::ffi::OsStr) -> bool {
    let n = name.to_string_lossy();
    n == "@eaDir" || n.starts_with('.')
}

/// The URL a `.strm` names. The format every Kodi/Jellyfin/Emby client reads is
/// one line holding the URL; a file may carry more lines in practice, so the
/// first non-empty one that looks like a URL wins and anything else is ignored.
fn read_strm_url(path: &Path) -> Option<String> {
    // A `.strm` is a few hundred bytes at most. Anything larger is not one, and
    // reading it into memory would be the wrong response to a mislabelled file.
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() > 64 * 1024 {
        return None;
    }
    let text = String::from_utf8_lossy(&bytes);
    // A BOM from a Windows-authored file would otherwise hide the scheme.
    let text = text.trim_start_matches('\u{feff}');
    text.lines()
        .map(str::trim)
        .find(|line| {
            let lower = line.to_ascii_lowercase();
            lower.starts_with("http://") || lower.starts_with("https://")
        })
        .map(str::to_string)
}

/// What a bridge learnt about a streamed entry, kept beside it as
/// `<name>.info.json`. The shape is ffprobe's own: `format.duration` in seconds
/// and `streams` with `codec_type`, so a bridge can write it straight from the
/// probe it already ran instead of inventing a format for this server.
#[derive(Debug, Default, serde::Deserialize)]
struct Sidecar {
    #[serde(default)]
    probe_version: Option<u32>,
    #[serde(default)]
    duration_ms: Option<u64>,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    video: Option<kroma_domain::VideoStream>,
    #[serde(default)]
    audio: Option<kroma_domain::AudioStream>,
    #[serde(default)]
    audio_tracks: Vec<kroma_domain::AudioStream>,
    #[serde(default)]
    subtitles: Vec<kroma_domain::SubtitleTrack>,
    #[serde(default)]
    unreadable: Option<String>,
}

/// `<stem>.info.json` beside the `.strm`, or nothing. A malformed sidecar is a
/// bridge bug, not an entry to drop: the entry still plays, it just arrives
/// undescribed and the probe pass fails it the way it always did.
fn read_sidecar(strm: &Path) -> Option<Sidecar> {
    let sidecar = strm.with_extension("info.json");
    let bytes = std::fs::read(&sidecar).ok()?;
    if bytes.len() > 512 * 1024 {
        debug!(path = %sidecar.display(), "an .info.json is implausibly large; ignored");
        return None;
    }
    match serde_json::from_slice::<Sidecar>(&bytes) {
        Ok(sidecar) => Some(sidecar),
        Err(error) => {
            debug!(path = %sidecar.display(), %error, "an .info.json could not be read; ignored");
            None
        }
    }
}

/// Whether a walk writes the file at `below`, its path below the folder the
/// walk starts in: a video container, in no folder the walk prunes.
pub fn takes(below: &Path) -> bool {
    let mut folders = below.parent().into_iter().flat_map(Path::components);
    has_video_extension(below) && !folders.any(|folder| is_pruned_dir(folder.as_os_str()))
}

// `stat` one file (run inside the parallel walk pool): (size, mtime-as-unix-secs).
fn file_meta(path: &Path) -> FileMeta {
    let md = std::fs::metadata(path).ok()?;
    let mtime = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    Some((md.len(), mtime))
}

#[cfg(test)]
mod tests;
