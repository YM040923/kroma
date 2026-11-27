//! Windows console-window suppression for spawned child processes.
//!
//! The distributable exe is built with `windows_subsystem = "windows"` (GUI
//! subsystem), so the main process holds no console. Every console child it
//! spawns (ffmpeg/ffprobe/cwebp/curl/cmd) would otherwise allocate a fresh
//! console window — during a library scan that is hundreds of flashing black
//! boxes. `CREATE_NO_WINDOW` on the spawn suppresses it; on other platforms the
//! flag does not exist and these helpers just build a normal command.

use std::process::Command;

/// `CREATE_NO_WINDOW`: start the child with no console at all.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Drop-in for `Command::new` on a console tool: identical everywhere except
/// Windows, where the child is started without a console window.
#[inline]
pub fn new(program: impl AsRef<std::ffi::OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// `CREATE_NO_WINDOW` as a flag, for a caller that builds its own
/// `tokio::process::Command` and cannot route through [`new`].
#[inline]
pub fn no_window_flag() -> u32 {
    #[cfg(windows)]
    {
        CREATE_NO_WINDOW
    }
    #[cfg(not(windows))]
    {
        0
    }
}