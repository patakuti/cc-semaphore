//! Where *this* machine's own frontend should read the published
//! snapshot from. Shared by the daemon (the Linux/WSL1 writer) and every
//! same-machine reader (GNOME extension, the Always-on-top window) so the
//! path logic exists in exactly one place. See 02_design.md §2.1, §8.
//!
//! On Windows there is no local daemon — the snapshot arrives bridged
//! from WSL1 into `%LOCALAPPDATA%\cc-semaphore\state.json` (02_design.md
//! §8), so that's the path a native Windows build of this function
//! resolves to instead.

use std::path::PathBuf;

#[cfg(not(target_os = "windows"))]
fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .expect("HOME must be set")
}

/// `$XDG_RUNTIME_DIR/cc-semaphore`, falling back to `~/.cache/cc-semaphore`
/// when `XDG_RUNTIME_DIR` isn't set.
#[cfg(not(target_os = "windows"))]
fn local_base_dir() -> PathBuf {
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(runtime_dir).join("cc-semaphore")
    } else {
        home_dir().join(".cache/cc-semaphore")
    }
}

#[cfg(not(target_os = "windows"))]
pub fn local_state_path() -> PathBuf {
    local_base_dir().join("state.json")
}

/// `%LOCALAPPDATA%\cc-semaphore\state.json` — the same default target the
/// WSL1 daemon writes to (`cc-semaphore-daemon/src/targets.rs`). Windows
/// always sets `LOCALAPPDATA` natively, so unlike the WSL1 side this
/// never needs to guess a username or fall back to a config file.
#[cfg(target_os = "windows")]
pub fn local_state_path() -> PathBuf {
    let local_app_data =
        std::env::var_os("LOCALAPPDATA").expect("LOCALAPPDATA must be set on Windows");
    PathBuf::from(local_app_data)
        .join("cc-semaphore")
        .join("state.json")
}
