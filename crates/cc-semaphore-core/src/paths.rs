//! Where the published snapshot lives, on the machine writing it AND on
//! every machine reading it. Shared by the daemon (writer) and every
//! frontend (reader) so the path logic exists in exactly one place.
//! See 02_design.md §2.1.

use std::path::PathBuf;

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .expect("HOME must be set")
}

/// `$XDG_RUNTIME_DIR/cc-semaphore`, falling back to `~/.cache/cc-semaphore`
/// when `XDG_RUNTIME_DIR` isn't set.
fn local_base_dir() -> PathBuf {
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(runtime_dir).join("cc-semaphore")
    } else {
        home_dir().join(".cache/cc-semaphore")
    }
}

/// The local snapshot path: written by the daemon on Linux/WSL1, read by
/// the GNOME extension and the Always-on-top window on the same machine.
pub fn local_state_path() -> PathBuf {
    local_base_dir().join("state.json")
}
