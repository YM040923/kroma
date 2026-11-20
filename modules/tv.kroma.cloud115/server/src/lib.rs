//! KROMA's 115 drive module.
//!
//! The drive is mirrored into a library the server already scans: a `.strm` per
//! film, and beside it a `.info.json` holding what the server cannot learn for
//! itself (its ffprobe accepts only the `file` protocol, so a URL is unreadable
//! to it). Playback answers with a redirect to the drive's own link, so the bytes
//! never pass through this machine.
//!
//! The drive counts API calls, and its risk control weighs writes more heavily
//! than reads. Everything here is built around that: one global pacer for every
//! call, a cooldown on the answers that mean "slow down", a per-run budget, and
//! nothing that writes to the account unless an operator turns it on.

#![allow(clippy::result_large_err)]

pub mod client;
pub mod crypto;
pub mod events;
pub mod login;
pub mod oss;
pub mod play;
pub mod pacer;
pub mod probe;
pub mod replace;
pub mod routes;
pub mod settings;
pub mod store;
pub mod sync;
pub mod transfer;
pub mod upload;
pub mod watch;

mod module;

pub use module::{server_module, Cloud115Module, MODULE_ID};
pub use routes::routes;

use kroma_module_sdk::EmbeddedModule;

/// What the host reads to know this module exists: its id, title and kind.
pub const MODULE: EmbeddedModule = kroma_module_sdk::embedded_module!();
