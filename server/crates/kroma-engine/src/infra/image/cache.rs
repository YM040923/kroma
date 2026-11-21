use std::path::{Path, PathBuf};

use kroma_media::tmp_file::unique_tmp;
use kroma_primitives::short_hash;

use super::encode::{encode_webp, encode_webp_capped};
use super::scratch::finalize;
use super::{images_dir, PUBLIC_PREFIX};

const MAX_TIME_SECS: u32 = 25;
const MAX_ARTWORK_BYTES: u64 = 32 * 1024 * 1024;

pub(super) fn cache_verbatim(data_dir: &Path, remote_url: &str, ext: &str) -> Option<String> {
    if !remote_url.starts_with("http") {
        return Some(remote_url.to_string());
    }
    let dir = images_dir(data_dir);
    std::fs::create_dir_all(&dir).ok()?;
    let name = format!("{}.{ext}", short_hash(remote_url));
    let out = dir.join(&name);
    if !out.exists() {
        let tmp = unique_tmp(&out);
        if !download(remote_url, &tmp) {
            let _ = std::fs::remove_file(&tmp);
            return None;
        }
        finalize(&tmp, &out)?;
    }
    Some(format!("{PUBLIC_PREFIX}{name}"))
}

/// Downloads a remote image straight to `target`, through a temp file so a
/// failed transfer never lands as a poster. Refuses to touch an existing file:
/// the artwork writer never replaces a file somebody else put there.
pub fn fetch_to(remote_url: &str, target: &Path) -> bool {
    if !remote_url.starts_with("http") || target.exists() {
        return false;
    }
    let Some(dir) = target.parent() else {
        return false;
    };
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let tmp = unique_tmp(target);
    if !download(remote_url, &tmp) {
        let _ = std::fs::remove_file(&tmp);
        return false;
    }
    if std::fs::rename(&tmp, target).is_ok() {
        return true;
    }
    let copied = std::fs::copy(&tmp, target).is_ok();
    let _ = std::fs::remove_file(&tmp);
    copied
}

/// Ensures a remote image is cached as WebP and returns its public path, or
/// `None` on failure (the caller keeps the provider URL). The entry point for
/// art that arrives outside title enrichment, e.g. a person's portrait.
pub fn cache_remote(data_dir: &Path, remote_url: &str) -> Option<String> {
    cache(data_dir, remote_url)
}

/// Whether `url` names a cached image whose file is no longer on disk.
pub fn local_art_missing(data_dir: &Path, url: &str) -> bool {
    url.strip_prefix(PUBLIC_PREFIX)
        .is_some_and(|name| !images_dir(data_dir).join(name).exists())
}

pub fn cache_local_capped(data_dir: &Path, source: &Path, max_width: u32) -> Option<PathBuf> {
    if max_width == 0 {
        return Some(source.to_path_buf());
    }
    let metadata = std::fs::metadata(source).ok()?;
    let modified = metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    let key = format!("{}:{}:{}", source.display(), metadata.len(), modified);
    let dir = images_dir(data_dir);
    std::fs::create_dir_all(&dir).ok()?;
    let name = format!("{}-w{max_width}.webp", short_hash(&key));
    let out = dir.join(name);
    if !out.exists() {
        let tmp = unique_tmp(&out);
        if !encode_webp_capped(source, &tmp, max_width) {
            let _ = std::fs::remove_file(&tmp);
            return None;
        }
        finalize(&tmp, &out)?;
    }
    Some(out)
}

pub(super) fn cache(data_dir: &Path, remote_url: &str) -> Option<String> {
    // Already a local path (idempotent if called twice).
    if !remote_url.starts_with("http") {
        return Some(remote_url.to_string());
    }
    let dir = images_dir(data_dir);
    std::fs::create_dir_all(&dir).ok()?;

    let name = format!("{}.webp", short_hash(remote_url));
    let out = dir.join(&name);
    if !out.exists() && !transcode(remote_url, &out) {
        return None;
    }
    Some(format!("{PUBLIC_PREFIX}{name}"))
}

fn transcode(remote_url: &str, out: &Path) -> bool {
    let src_tmp = unique_tmp(out);
    if !download(remote_url, &src_tmp) {
        let _ = std::fs::remove_file(&src_tmp);
        return false;
    }

    let out_tmp = unique_tmp(out);
    let ok = encode_webp(&src_tmp, &out_tmp) && out_tmp.exists();
    let _ = std::fs::remove_file(&src_tmp);
    if !ok {
        let _ = std::fs::remove_file(&out_tmp);
        return false;
    }
    finalize(&out_tmp, out).is_some()
}

fn download(remote_url: &str, dest: &Path) -> bool {
    kroma_http::Fetch::new()
        .purpose("artwork")
        .max_time(MAX_TIME_SECS)
        .max_bytes(MAX_ARTWORK_BYTES)
        .download(remote_url, dest)
        .is_ok_and(|response| response.ensure_ok().is_ok())
        && dest.exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cached_url_whose_file_is_gone_is_missing_and_a_remote_one_never_is() {
        let dir = kroma_testing::temp_dir("image-missing");
        std::fs::create_dir_all(images_dir(dir.path())).unwrap();
        std::fs::write(images_dir(dir.path()).join("here.webp"), b"x").unwrap();

        assert!(!local_art_missing(dir.path(), "/api/images/here.webp"));
        assert!(local_art_missing(dir.path(), "/api/images/gone.webp"));
        assert!(!local_art_missing(dir.path(), "https://img.example/p.jpg"));
    }
}
