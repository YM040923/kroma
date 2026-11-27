use std::ffi::OsString;
use std::io;
use std::path::{Component, Path, PathBuf};

use cap_tempfile::{ambient_authority, cap_std::fs::Dir, TempFile};
use kroma_wasm_host::Preopen;

pub(super) struct DownloadRoots(Vec<(PathBuf, Option<Dir>)>);

impl DownloadRoots {
    pub(super) fn open(roots: &[Preopen]) -> io::Result<Self> {
        let mut dirs = Vec::with_capacity(roots.len());
        for root in roots {
            let dir = if root.writable {
                Some(Dir::open_ambient_dir(&root.host, ambient_authority())?)
            } else {
                None
            };
            dirs.push((PathBuf::from(&root.guest), dir));
        }
        Ok(Self(dirs))
    }

    pub(super) fn destination(&self, guest: &Path) -> io::Result<DownloadTarget> {
        let denied = || io::Error::from(io::ErrorKind::PermissionDenied);
        if !guest.has_root() || guest.components().any(|part| part == Component::ParentDir) {
            return Err(denied());
        }
        // Match the most specific guest mount, even when that mount is read-only.
        let (mount, dir) = self
            .0
            .iter()
            .filter(|(p, _)| guest.starts_with(p))
            .max_by_key(|(p, _)| p.components().count())
            .ok_or_else(denied)?;
        let dir = dir.as_ref().ok_or_else(denied)?;
        let relative = guest.strip_prefix(mount).map_err(|_| denied())?;
        if relative
            .components()
            .any(|p| !matches!(p, Component::Normal(_)))
        {
            return Err(denied());
        }
        let name = relative.file_name().ok_or_else(denied)?.to_os_string();
        let parent = match relative.parent().filter(|p| !p.as_os_str().is_empty()) {
            Some(path) => dir.open_dir(path)?,
            None => dir.try_clone()?,
        };
        match parent.symlink_metadata(&name) {
            Ok(meta) if !meta.is_file() => return Err(denied()),
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
        Ok(DownloadTarget { parent, name })
    }
}

pub(super) struct DownloadTarget {
    parent: Dir,
    name: OsString,
}

impl DownloadTarget {
    pub(super) fn receive(
        &self,
        transfer: impl FnOnce(std::fs::File) -> anyhow::Result<kroma_http::Response>,
    ) -> anyhow::Result<kroma_http::Response> {
        let staged = TempFile::new(&self.parent)?;
        let file = staged.as_file().try_clone()?.into_std();
        let response = transfer(file)?;
        if (200..300).contains(&response.status) {
            staged.replace(&self.name)?;
        }
        Ok(response)
    }
}
