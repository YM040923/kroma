//! The module's configuration, as a run reads it.
//!
//! The stored half lives in [`crate::store`]; this is the resolved view a run
//! and a route use. The cookie is a credential, so the admin page never echoes
//! it back: it reports whether one is stored, never the value.

use serde::{Deserialize, Serialize};

/// One drive folder mirrored into one local folder.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Library {
    /// The drive folder, for example `/电影`.
    pub remote: String,
    /// Where it lands: an absolute path under a storage root, for example
    /// `/media/115/电影`. A relative path from an older config is anchored at
    /// the first library folder (or `/media`).
    pub local: String,
}

/// The knobs that decide how hard this module leans on the drive.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct Options {
    /// Minimum gap between two API calls, in milliseconds. The drive counts
    /// calls; this is the brake.
    pub interval_ms: u64,
    /// How long a signed link is reused when the link carries no deadline.
    pub link_ttl_secs: u64,
    /// How many entries one listing asks for. The drive caps it at 1150; asking
    /// for the cap lowers the call count, which is what gets counted.
    pub page_size: u32,
    /// Stop a run after this many API calls, leaving the rest for the next one.
    /// A library that needs more than this is a library to split.
    pub max_calls_per_run: u32,
    /// Warn (and skip the descent) when a single folder holds more entries than
    /// this. A folder with a thousand files in it is a folder that gets an
    /// account flagged.
    pub max_entries_per_folder: u32,
    /// Describe entries into `.info.json` while mirroring.
    pub probe_enabled: bool,
    /// Tidy the drive folder against the naming templates. Writes to the
    /// account, so it is off unless an operator says so.
    pub organize_enabled: bool,
    /// Seconds between two write calls while tidying.
    pub write_pause_ms: u64,
    /// Poll 115's life events so a change made elsewhere (Symedia organising,
    /// a phone upload) produces its `.strm` within about a minute.
    pub watch_enabled: bool,
    /// Floor for two life-event polls; the job ticks every minute.
    pub watch_interval_secs: u64,
    /// Leave ISO/Blu-ray images out of the mirror: a browser cannot decode one,
    /// and this server cannot remux a URL.
    pub skip_iso: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            interval_ms: 1500,
            link_ttl_secs: 20 * 60,
            page_size: 1150,
            max_calls_per_run: 800,
            max_entries_per_folder: 2000,
            probe_enabled: true,
            organize_enabled: false,
            write_pause_ms: 4000,
            watch_enabled: true,
            watch_interval_secs: 60,
            skip_iso: true,
        }
    }
}

impl Options {
    /// Clamp whatever the settings hold into a shape a run can use. A hand-edited
    /// settings row is a real possibility, and a zero interval would hammer the
    /// drive.
    pub fn sane(mut self) -> Self {
        self.interval_ms = self.interval_ms.clamp(500, 60_000);
        self.link_ttl_secs = self.link_ttl_secs.clamp(60, 4 * 3600);
        self.page_size = self.page_size.clamp(100, 1150);
        self.max_calls_per_run = self.max_calls_per_run.clamp(50, 20_000);
        self.max_entries_per_folder = self.max_entries_per_folder.clamp(200, 20_000);
        self.write_pause_ms = self.write_pause_ms.clamp(1000, 60_000);
        self.watch_interval_secs = self.watch_interval_secs.clamp(30, 3600);
        self
    }
}

/// Everything a run reads, resolved once.
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub cookie: String,
    /// The origin players reach this server at, used to build the URL each
    /// `.strm` names. Written by the first manual sync (the browser knows it),
    /// editable in the settings, required for a scheduled run.
    pub base_url: String,
    pub libraries: Vec<Library>,
    pub options: Options,
}

impl Config {
    pub fn load() -> Self {
        let stored = crate::store::load();
        Self {
            cookie: stored.cookie.trim().to_string(),
            base_url: stored.base_url.trim().trim_end_matches('/').to_string(),
            libraries: stored.libraries,
            options: stored.options.sane(),
        }
    }

    /// What can run: a cookie, an origin to point players at, and a mapping with
    /// both sides named.
    pub fn runnable(&self) -> bool {
        !self.cookie.is_empty()
            && !self.base_url.is_empty()
            && self
                .libraries
                .iter()
                .any(|l| !l.remote.is_empty() && !l.local.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_are_clamped_into_a_shape_a_run_can_use() {
        let wild = Options {
            interval_ms: 0,
            page_size: 99999,
            max_calls_per_run: 0,
            ..Options::default()
        }
        .sane();
        assert!(wild.interval_ms >= 500, "a zero interval would hammer the drive");
        assert!(wild.page_size <= 1150);
        assert!(wild.max_calls_per_run >= 50);
    }

    #[test]
    fn a_config_without_a_cookie_or_a_library_cannot_run() {
        let mut config = Config::default();
        assert!(!config.runnable());
        config.cookie = "UID=1".into();
        assert!(!config.runnable(), "a cookie alone mirrors nothing");
        config.libraries = vec![Library {
            remote: "/电影".into(),
            local: "/media/115/电影".into(),
        }];
        assert!(!config.runnable(), "without an origin a .strm names nothing");
        config.base_url = "http://192.168.1.10:8096".into();
        assert!(config.runnable());
    }
}
