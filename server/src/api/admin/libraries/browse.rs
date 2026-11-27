//! The library folder picker: the browseable sub-directories under the volume
//! roots a library may be pointed at.

#[cfg(not(windows))]
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use axum::extract::Query;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::api::admin::{require, user_locale};
use crate::api::error::lerr;
use crate::api::extract::AuthUser;
use crate::model::Permission;

#[cfg(test)]
mod tests;

#[derive(Debug, Deserialize)]
pub struct BrowseQuery {
    #[serde(default)]
    pub path: Option<String>,
}

/// Browseable sub-directories of `path`, as `{ path, parent, entries: [{ name,
/// path }] }`. With no `path`, list Windows drives, Synology volumes, or `/`.
pub async fn browse_libraries(
    AuthUser(user): AuthUser,
    Query(q): Query<BrowseQuery>,
) -> Result<Response, Response> {
    require(&user, Permission::LibraryManage)?;
    let raw = q.path.unwrap_or_default();
    // Never resolve a traversal segment, even before touching the filesystem.
    if raw.split(std::path::is_separator).any(|part| part == "..") {
        return Err(lerr(
            user_locale(&user),
            StatusCode::FORBIDDEN,
            "error.forbidden",
        ));
    }
    match tokio::task::spawn_blocking(move || browse_dirs(raw)).await {
        Ok(Ok(body)) => Ok(Json(body).into_response()),
        Ok(Err(BrowseErr::Forbidden)) => Err(lerr(
            user_locale(&user),
            StatusCode::FORBIDDEN,
            "error.forbidden",
        )),
        Ok(Err(BrowseErr::NotFound)) => Err(lerr(
            user_locale(&user),
            StatusCode::NOT_FOUND,
            "error.itemNotFound",
        )),
        Err(_) => Err(lerr(
            user_locale(&user),
            StatusCode::INTERNAL_SERVER_ERROR,
            "error.internal",
        )),
    }
}

enum BrowseErr {
    Forbidden,
    NotFound,
}

fn browse_dirs(raw: String) -> Result<Value, BrowseErr> {
    let roots = volume_roots();
    let raw = raw.trim();

    if raw.is_empty() {
        if cfg!(windows) || !roots.is_empty() {
            return Ok(json!({ "path": "", "parent": Value::Null, "entries": to_entries(roots) }));
        }
        let entries = read_subdirs(Path::new("/"))?;
        return Ok(json!({ "path": "/", "parent": Value::Null, "entries": entries }));
    }

    // Old library entries can have the verbatim drive prefix from canonicalize.
    let requested = picker_path(Path::new(raw));
    #[cfg(windows)]
    let drive = roots
        .iter()
        .find(|r| requested.starts_with(r))
        .filter(|_| !has_dos_device_name(requested))
        .ok_or(BrowseErr::Forbidden)?;
    let canon = std::fs::canonicalize(requested).map_err(|_| BrowseErr::NotFound)?;
    #[cfg(windows)]
    let path = {
        // A mapped drive can canonicalize to UNC; only resolve the drive the user actually opened.
        let resolved_drive = drive.canonicalize().map_err(|_| BrowseErr::NotFound)?;
        let suffix = canon
            .strip_prefix(resolved_drive)
            .map_err(|_| BrowseErr::Forbidden)?;
        if suffix.as_os_str().is_empty() {
            drive.clone()
        } else {
            drive.join(suffix)
        }
    };
    #[cfg(not(windows))]
    let path = canon;
    if !within_roots(&path, &roots) {
        return Err(BrowseErr::Forbidden);
    }
    let entries = read_subdirs(&path)?;
    let parent = if roots.contains(&path) {
        cfg!(windows).then_some(Path::new(""))
    } else {
        path.parent()
    };
    Ok(
        json!({ "path": path.to_string_lossy(), "parent": parent.map(Path::to_string_lossy), "entries": entries }),
    )
}

// Confines the browse to the volume roots; with none (a dev box, not a DSM
// one) anywhere is allowed.
fn within_roots(path: &Path, roots: &[PathBuf]) -> bool {
    (!cfg!(windows) && roots.is_empty()) || roots.iter().any(|r| path.starts_with(r))
}

// Only canonical drive-letter prefixes have an ordinary picker spelling.
// In particular, never turn an extended UNC or device path into a drive path.
fn picker_path(path: &Path) -> &Path {
    #[cfg(windows)]
    if let Some(std::path::Component::Prefix(prefix)) = path.components().next() {
        if matches!(prefix.kind(), std::path::Prefix::VerbatimDisk(_)) {
            if let Some(s) = path.to_str().and_then(|s| s.strip_prefix(r"\\?\")) {
                return Path::new(s);
            }
        }
    }
    path
}

// DOS devices can also be addressed underneath an ordinary drive prefix.
#[cfg(windows)]
fn has_dos_device_name(path: &Path) -> bool {
    path.components().any(|part| {
        let std::path::Component::Normal(name) = part else {
            return false;
        };
        let name = name.to_string_lossy();
        let stem = name
            .split(['.', ':'])
            .next()
            .unwrap_or("")
            .trim_end_matches(' ')
            .to_ascii_uppercase();
        matches!(
            stem.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
        ) || stem
            .strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT"))
            .is_some_and(|n| matches!(n.as_bytes(), [b'0'..=b'9']) || matches!(n, "¹" | "²" | "³"))
    })
}

#[cfg(windows)]
fn volume_roots() -> Vec<PathBuf> {
    let mask = unsafe { windows_sys::Win32::Storage::FileSystem::GetLogicalDrives() };
    (b'A'..=b'Z')
        .filter(|&letter| mask & (1 << (letter - b'A')) != 0)
        .map(|letter| PathBuf::from(format!(r"{}:\", char::from(letter))))
        .collect()
}

#[cfg(not(windows))]
fn volume_roots() -> Vec<PathBuf> {
    std::fs::read_dir("/")
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_dir()
                && p.file_name()
                    .and_then(OsStr::to_str)
                    .map(|n| n.starts_with("volume"))
                    .unwrap_or(false)
        })
        .collect()
}

// Skips hidden and DSM system names (`@eaDir`, `#recycle`).
fn read_subdirs(dir: &Path) -> Result<Vec<Value>, BrowseErr> {
    let rd = std::fs::read_dir(dir).map_err(|_| BrowseErr::NotFound)?;
    let mut dirs: Vec<PathBuf> = Vec::new();
    for entry in rd.flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with(['.', '@', '#']) {
            continue;
        }
        let is_dir =
            entry.file_type().map(|t| t.is_dir()).unwrap_or(false) || entry.path().is_dir();
        if is_dir {
            dirs.push(entry.path());
        }
    }
    Ok(to_entries(dirs))
}

fn to_entries(mut paths: Vec<PathBuf>) -> Vec<Value> {
    paths.sort_by_key(|p| {
        p.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase()
    });
    paths
        .iter()
        .map(|p| {
            json!({
                "name": p.file_name().unwrap_or(p.as_os_str()).to_str().unwrap_or_default(),
                "path": picker_path(p).to_string_lossy(),
            })
        })
        .collect()
}
