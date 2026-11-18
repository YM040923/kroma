//! Sidecar NFO writing, only when a library asked for it.
//!
//! KROMA reads its own database; these files exist for the other players in the
//! house (Jellyfin, Infuse, Kodi) that sit beside the same folders. A `.strm`'s
//! NFO goes next to the `.strm`; a show gets one `tvshow.nfo` at its folder.

use std::path::{Path, PathBuf};

use crate::db;
use crate::model::Metadata;

use super::engine::Engine;
use super::jobs::Job;

pub(super) fn write(eng: &Engine, job: &Job) {
    if job.is_show {
        write_show(eng, &job.id, &job.library);
    } else {
        write_movie(eng, &job.id, &job.library);
    }
}

fn write_movie(eng: &Engine, id: &str, library: &str) {
    let Ok(Some(item)) = db::get_item(&eng.pool, id) else {
        return;
    };
    let Some(meta) = item.metadata.as_ref() else {
        return;
    };
    let xml = movie_xml(meta, item.year);
    let Ok(files) = db::local_paths::item_file_paths(&eng.pool, id) else {
        return;
    };
    for path in local_paths(eng, library, files) {
        let target: PathBuf = path.with_extension("nfo");
        write_file(&target, &xml);
    }
}

fn write_show(eng: &Engine, id: &str, library: &str) {
    let Ok(Some(detail)) = db::get_show(&eng.pool, id) else {
        return;
    };
    let Some(meta) = detail.show.metadata.as_ref() else {
        return;
    };
    let Ok(files) = db::local_paths::show_file_paths(&eng.pool, id) else {
        return;
    };
    let Some(first) = local_paths(eng, library, files).into_iter().next() else {
        return;
    };
    let Some(parent) = first.parent() else {
        return;
    };
    let show_dir = if parent
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.to_lowercase().starts_with("season"))
    {
        parent.parent().unwrap_or(parent)
    } else {
        parent
    };
    write_file(&show_dir.join("tvshow.nfo"), &show_xml(meta, detail.show.year));
}

// A streamed library stores the play URL in `abs_path`; its scanned `.strm`
// is `rel_path` under one of the library's folders.
fn local_paths(eng: &Engine, library: &str, files: Vec<db::local_paths::LocalFile>) -> Vec<PathBuf> {
    let folders = eng.folders.get(library);
    files
        .into_iter()
        .filter_map(|file| {
            let abs = file.abs_path.trim();
            if !abs.is_empty() && !abs.starts_with("http") {
                return Some(PathBuf::from(abs));
            }
            let rel = file.rel_path.trim();
            if rel.is_empty() {
                return None;
            }
            let folders = folders?;
            folders
                .iter()
                .map(|folder| Path::new(folder).join(rel))
                .find(|path| path.exists())
                .or_else(|| folders.first().map(|folder| Path::new(folder).join(rel)))
        })
        .collect()
}

fn write_file(target: &Path, xml: &str) {
    if let Ok(existing) = std::fs::read_to_string(target) {
        if existing == xml {
            return;
        }
    }
    if !target.parent().is_some_and(Path::exists) {
        return;
    }
    let staged = target.with_extension("nfo.tmp");
    if std::fs::write(&staged, xml).is_ok() {
        let _ = std::fs::rename(&staged, target);
    }
}

fn esc(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn movie_xml(meta: &Metadata, year: Option<u32>) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<movie>\n");
    push_common(&mut out, meta, year);
    out.push_str("</movie>\n");
    out
}

fn show_xml(meta: &Metadata, year: Option<u32>) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<tvshow>\n");
    push_common(&mut out, meta, year);
    out.push_str("</tvshow>\n");
    out
}

fn push_common(out: &mut String, meta: &Metadata, year: Option<u32>) {
    if let Some(title) = meta.title.as_deref() {
        out.push_str(&format!("  <title>{}</title>\n", esc(title)));
        out.push_str(&format!("  <originaltitle>{}</originaltitle>\n", esc(title)));
    }
    if let Some(year) = year {
        out.push_str(&format!("  <year>{year}</year>\n"));
    }
    if let Some(date) = meta.release_date.as_deref() {
        out.push_str(&format!("  <premiered>{}</premiered>\n", esc(date)));
    }
    if let Some(plot) = meta.overview.as_deref() {
        out.push_str(&format!("  <plot>{}</plot>\n", esc(plot)));
    }
    if let Some(tagline) = meta.tagline.as_deref() {
        out.push_str(&format!("  <tagline>{}</tagline>\n", esc(tagline)));
    }
    if let Some(rating) = meta.rating {
        out.push_str(&format!("  <rating>{rating:.1}</rating>\n"));
    }
    if let Some(certification) = meta.certification.as_deref() {
        out.push_str(&format!("  <mpaa>{}</mpaa>\n", esc(certification)));
    }
    for genre in &meta.genres {
        out.push_str(&format!("  <genre>{}</genre>\n", esc(genre)));
    }
    out.push_str(&format!(
        "  <uniqueid type=\"tmdb\" default=\"true\">{}</uniqueid>\n",
        meta.tmdb_id
    ));
    if let Some(imdb) = meta.imdb_id.as_deref() {
        out.push_str(&format!("  <uniqueid type=\"imdb\">{}</uniqueid>\n", esc(imdb)));
    }
    if let Some(tmdb_url) = Some(meta.tmdb_url.as_str()) {
        if !tmdb_url.is_empty() {
            out.push_str(&format!("  <tmdburl>{}</tmdburl>\n", esc(tmdb_url)));
        }
    }
}