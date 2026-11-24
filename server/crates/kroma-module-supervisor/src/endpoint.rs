//! Where a running module answers, and where the core answers it back.
//!
//! On Unix a module listens on a Unix-domain socket in its run directory, which
//! needs no port and no firewall rule. Windows has no usable `tokio` Unix-socket
//! listener, so there a module listens on a loopback TCP port instead. The
//! [`Endpoint`] carries whichever of the two the current platform uses, and
//! `url` builds the `kroma_http::Loopback` base for it either way.

use std::path::PathBuf;

use anyhow::Context;

use super::Supervisor;

/// Where a running module answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// The Unix socket a module listens on (Unix only).
    pub socket: PathBuf,
    /// The loopback port a module listens on (Windows only).
    pub port: u16,
}

impl Endpoint {
    /// `path`, with its query, on this module, as a URL [`kroma_http::Loopback`]
    /// reaches.
    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base())
    }

    /// The `kroma_http::Loopback` base for this module.
    fn base(&self) -> String {
        #[cfg(unix)]
        {
            kroma_http::loopback::unix_url(&self.socket)
        }
        #[cfg(not(unix))]
        {
            format!("http://127.0.0.1:{}", self.port)
        }
    }
}

impl Supervisor {
    /// Where the core serves its API for modules.
    pub fn host_socket(&self) -> PathBuf {
        self.run.host_socket()
    }

    /// [`host_socket`](Self::host_socket), ready to bind: its directory closed
    /// to every other user, and a socket a previous run left behind removed.
    pub fn prepare_host_socket(&self) -> std::io::Result<PathBuf> {
        self.run.prepare_host()
    }

    /// The `kroma_http::Loopback` base the core's own API is reached on. On Unix
    /// that is the host socket; on Windows it is `host_port`.
    pub fn host_base(&self) -> String {
        #[cfg(unix)]
        {
            kroma_http::loopback::unix_url(&self.host_socket())
        }
        #[cfg(not(unix))]
        {
            format!("http://127.0.0.1:{}", self.host_port())
        }
    }

    pub(crate) fn assign_endpoint(&self, id: &str) -> anyhow::Result<Endpoint> {
        #[cfg(unix)]
        {
            let socket = self
                .run
                .prepare_module(id)
                .with_context(|| format!("prepare the socket '{id}' serves on"))?;
            Ok(Endpoint {
                socket,
                port: 0,
            })
        }
        #[cfg(not(unix))]
        {
            // The module gets its own loopback port; nothing is bound here — the
            // supervisor binds it when it starts the component (see spawn_component).
            let _ = id;
            let port = self.reserve_port()?;
            Ok(Endpoint {
                socket: PathBuf::new(),
                port,
            })
        }
    }

    pub(crate) fn peer_base(&self, peer: &str, endpoint: &Endpoint, via_core: bool) -> String {
        if via_core {
            return format!("{}/api/module/{peer}", self.host_base());
        }
        endpoint.url("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_endpoint_is_a_base_a_route_is_appended_to() {
        #[cfg(unix)]
        {
            let endpoint = Endpoint {
                socket: PathBuf::from("/data/run/modules/a.b/module.sock"),
                port: 0,
            };
            let url = endpoint.url("/_port/x");
            assert_eq!(
                url,
                format!(
                    "{}/_port/x",
                    kroma_http::loopback::unix_url(Path::new("/data/run/modules/a.b/module.sock"))
                )
            );
        }
        #[cfg(not(unix))]
        {
            let endpoint = Endpoint {
                socket: PathBuf::new(),
                port: 40401,
            };
            assert_eq!(endpoint.url("/_port/x"), "http://127.0.0.1:40401/_port/x");
        }
    }
}
