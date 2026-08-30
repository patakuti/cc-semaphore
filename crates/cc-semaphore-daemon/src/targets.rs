//! Resolves where the snapshot gets written. See 02_design.md §2.1, §8.

use crate::config::Config;
use crate::env;
use std::path::PathBuf;

pub use cc_semaphore_core::local_state_path;

pub fn lock_path() -> PathBuf {
    // Sibling of the state file, in the same per-user runtime directory.
    local_state_path().with_file_name("daemon.lock")
}

/// Resolves the WSL1→Windows write target per 02_design.md §8's priority
/// order. Returns `None` (with a one-time warning) if nothing resolves.
fn windows_state_path(config: &Config) -> Option<PathBuf> {
    if let Some(dir) = &config.windows_state_dir {
        return Some(dir.join("state.json"));
    }
    if let Ok(dir) = std::env::var("CC_SEMAPHORE_WIN_STATE_DIR") {
        return Some(PathBuf::from(dir).join("state.json"));
    }
    if let Ok(user) = std::env::var("USER") {
        let guess = PathBuf::from(format!("/mnt/c/Users/{user}/AppData/Local/cc-semaphore"));
        if guess.parent().is_some_and(|p| p.exists()) {
            return Some(guess.join("state.json"));
        }
    }
    None
}

/// All paths the snapshot should be written to for the current
/// environment: always the local path, plus the Windows path when running
/// under WSL and it resolves.
pub fn resolve(config: &Config) -> Vec<PathBuf> {
    let mut targets = vec![local_state_path()];
    if env::is_wsl() {
        match windows_state_path(config) {
            Some(p) => targets.push(p),
            None => eprintln!(
                "cc-semaphored: warning: running under WSL but no Windows state directory \
                 could be resolved (checked config windowsStateDir, \
                 $CC_SEMAPHORE_WIN_STATE_DIR, /mnt/c/Users/$USER/AppData/Local/cc-semaphore); \
                 writing to the local path only"
            ),
        }
    }
    targets
}
