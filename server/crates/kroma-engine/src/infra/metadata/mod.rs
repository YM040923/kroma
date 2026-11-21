//! Optional movie/show metadata enrichment via TMDB, enabled by a key in
//! `KROMA_TMDB_API_KEY`; inert without one. Shells out to `curl` rather than
//! pulling an HTTP/TLS dependency, as [`crate::infra::probe`] does `ffprobe`.

mod cache;
mod client;
pub mod fanart;
mod find;
pub mod person;
mod search;
mod videos;

use kroma_requests::tmdb::common;

pub use cache::Cache;
pub use client::{
    images, lookup, lookup_all, lookup_all_by_id, season_images, ArtImage, ArtLists, Target,
};
pub(crate) use find::{find_id, Catalogue};
pub use kroma_requests::tmdb::{
    discover, season_episodes, season_episodes_multi, EpisodeArt, SeasonData,
};
pub use videos::{title_videos, TitleVideos};
