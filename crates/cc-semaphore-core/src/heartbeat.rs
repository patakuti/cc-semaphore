//! Daemon-liveness heartbeat. See 02_design.md §3.9.
//!
//! `state.json` is only rewritten when its content changes (§2.5), so its
//! own `generatedAt`/mtime cannot distinguish "daemon alive but nothing
//! changed" from "daemon has stopped": a long-idle daemon legitimately
//! leaves the file untouched for as long as sessions stay unchanged.
//!
//! `heartbeat.json` fixes that by being rewritten on *every* scan tick
//! regardless of content, in the same directory next to `state.json`, on
//! every write target (so it also reaches Windows over the WSL1 bridge).
//! Readers only need to check its mtime freshness, not its content.

use std::path::{Path, PathBuf};

/// A reader treats the daemon as alive if its heartbeat is younger than
/// this. Generous relative to every write cadence in the system (Linux
/// liveness tick 5s, WSL1 poll 1s, Windows mtime poll 500ms) to absorb
/// scheduling jitter without false positives.
pub const STALE_AFTER_MS: i64 = 15_000;

/// The heartbeat path that sits alongside a given `state.json` target.
pub fn heartbeat_path(state_path: &Path) -> PathBuf {
    state_path.with_file_name("heartbeat.json")
}

/// Whether a heartbeat last touched at `heartbeat_mtime_ms` (epoch ms)
/// counts as fresh at `now_ms`.
pub fn is_fresh(heartbeat_mtime_ms: i64, now_ms: i64) -> bool {
    now_ms.saturating_sub(heartbeat_mtime_ms) < STALE_AFTER_MS
}

/// Convenience wrapper every frontend uses: stats `heartbeat_path`'s mtime
/// and checks it's fresh right now. Any I/O error (missing file, clock
/// issues, ...) is treated as "not alive" — there's no live signal to
/// trust either way.
pub fn daemon_alive(state_path: &Path) -> bool {
    let Ok(now_ms) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) else {
        return false;
    };
    let Ok(metadata) = std::fs::metadata(heartbeat_path(state_path)) else {
        return false;
    };
    let Ok(modified) = metadata.modified() else {
        return false;
    };
    let Ok(since_epoch) = modified.duration_since(std::time::UNIX_EPOCH) else {
        return false;
    };
    is_fresh(since_epoch.as_millis() as i64, now_ms.as_millis() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heartbeat_path_is_a_sibling_of_state_json() {
        let p = heartbeat_path(Path::new("/run/user/1000/cc-semaphore/state.json"));
        assert_eq!(p, Path::new("/run/user/1000/cc-semaphore/heartbeat.json"));
    }

    #[test]
    fn fresh_just_now() {
        assert!(is_fresh(1_000, 1_000));
    }

    #[test]
    fn fresh_just_under_the_threshold() {
        assert!(is_fresh(0, STALE_AFTER_MS - 1));
    }

    #[test]
    fn stale_at_the_threshold() {
        assert!(!is_fresh(0, STALE_AFTER_MS));
    }

    #[test]
    fn stale_long_after() {
        assert!(!is_fresh(0, STALE_AFTER_MS * 10));
    }
}
