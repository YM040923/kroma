//! The Cloud115 module as a component (its `.kmod` entrypoint).
//!
//! It contributes no port routes: playback goes through the `.strm` entries it
//! writes, and the admin page talks to [`kroma_cloud115::routes`].

use kroma_module_runtime::RemoteHost;

kroma_module_runtime::module_main!(
    |_host: &RemoteHost| axum::Router::new(),
    vec![kroma_cloud115::server_module::<RemoteHost>()],
);
