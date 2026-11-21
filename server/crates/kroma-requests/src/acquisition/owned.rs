//! What the library already holds for a logical unit. This is the one check
//! every provider and the request flow must share: a cloud replica counts as
//! owned exactly like a local file.

use std::path::Path;

use anyhow::Result;

use kroma_module_host::HostStorage;

use crate::db;
use crate::model::{
    LocationState, MediaLocation, MediaUnitKind, MediaUnitRef, OwnedState, PROVIDER_LOCAL,
};

pub fn locations<S: HostStorage>(state: &S, unit: &MediaUnitRef) -> Result<Vec<MediaLocation>> {
    if !unit.valid() {
        anyhow::bail!("invalid media unit identity");
    }
    let conn = state.db().get()?;
    db::list_media_locations(&conn, &unit.key())
}

/// Whether bytes exist locally and/or in a cloud location. A catalog row with
/// no file is not local; a cloud row only counts while it is `present`.
pub fn owned<S: HostStorage>(state: &S, unit: &MediaUnitRef) -> Result<OwnedState> {
    if !unit.valid() {
        anyhow::bail!("invalid media unit identity");
    }
    let conn = state.db().get()?;
    let local_files = match unit.kind {
        MediaUnitKind::Movie => db::movie_files_by_tmdb(&conn, unit.tmdb_id, None)?,
        MediaUnitKind::Episode => {
            let Some(show_id) = db::show_by_tmdb(&conn, unit.tmdb_id)? else {
                return Ok(OwnedState {
                    local: false,
                    cloud: cloud_present(&conn, unit)?,
                });
            };
            db::episode_files(&conn, &show_id, unit.season, unit.episode, None)?
        }
    };
    let (local_file, strm_file) = classify_files(local_files);
    let cloud = cloud_present(&conn, unit)? || strm_file;
    Ok(OwnedState {
        local: local_file,
        cloud,
    })
}

fn classify_files(files: Vec<db::LibraryFile>) -> (bool, bool) {
    let mut local = false;
    let mut strm = false;
    for file in files {
        let path = Path::new(&file.path);
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        if extension.eq_ignore_ascii_case("strm") {
            strm |= path.is_file();
        } else {
            local |= path.is_file();
        }
    }
    (local, strm)
}

fn cloud_present(conn: &crate::db::PooledConn, unit: &MediaUnitRef) -> Result<bool> {
    Ok(db::list_media_locations(conn, &unit.key())?
        .into_iter()
        .any(|location| {
            location.provider != PROVIDER_LOCAL && location.state == LocationState::Present
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &Path) -> db::LibraryFile {
        db::LibraryFile {
            path: path.to_string_lossy().into_owned(),
            item_id: "item".into(),
            edition: None,
            spans_episodes: false,
        }
    }

    #[test]
    fn existing_strm_is_a_cloud_copy_not_a_local_video() {
        let dir = kroma_testing::temp_dir("acquisition-owned-strm");
        let video = dir.path().join("film.mkv");
        let strm = dir.path().join("film.strm");
        std::fs::write(&video, b"video").unwrap();
        std::fs::write(&strm, "https://115.example/video").unwrap();

        assert_eq!(
            classify_files(vec![file(&video), file(&strm)]),
            (true, true)
        );
    }

    #[test]
    fn missing_strm_does_not_count_as_owned() {
        let dir = kroma_testing::temp_dir("acquisition-owned-missing-strm");
        let strm = dir.path().join("film.strm");

        assert_eq!(classify_files(vec![file(&strm)]), (false, false));
    }
}
