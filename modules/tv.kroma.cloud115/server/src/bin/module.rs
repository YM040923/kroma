//! The Cloud115 module as a component (its `.kmod` entrypoint).
//!
//! It mounts the `cloud115` provider point (`/_port/cloud115/...`, host-token
//! protected by the runtime) for explicitly requested staged uploads,
//! and serves its admin page through
//! [`kroma_cloud115::routes`]. Playback goes through the `.strm` entries the
//! module writes.

use kroma_module_runtime::RemoteHost;

kroma_module_runtime::module_main!(
    |_host: &RemoteHost| kroma_cloud115::provider::routes::<RemoteHost>(),
    vec![kroma_cloud115::server_module::<RemoteHost>()],
);
