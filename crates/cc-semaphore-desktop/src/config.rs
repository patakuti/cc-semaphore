//! Minimal config-file support for cc-semaphore-desktop. Shares
//! `~/.config/cc-semaphore/config.json` on Linux / `%APPDATA%\cc-semaphore\
//! config.json` on Windows with cc-semaphored (02_design.md §10), but only
//! reads the one field this binary actually needs. The daemon-only fields
//! (`inotifyDebounceMs`, `wsl1PollIntervalMs`, etc.) are irrelevant to a
//! frontend and deliberately not duplicated here — see `cc-semaphore-
//! daemon/src/config.rs` for those. The tray/GNOME display timings
//! (rotation, blink) are intentionally fixed constants, not config-file
//! parameters (02_design.md §10, 2026-09-03 revision): only this one
//! backend-polling-latency parameter was judged worth exposing.

use serde::Deserialize;
use std::path::PathBuf;

pub const DEFAULT_WINDOWS_POLL_INTERVAL_MS: u64 = 500;

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct RawConfig {
    #[serde(rename = "windowsPollIntervalMs")]
    windows_poll_interval_ms: Option<u64>,
}

#[cfg(target_os = "windows")]
fn path() -> PathBuf {
    let app_data = std::env::var_os("APPDATA").expect("APPDATA must be set on Windows");
    PathBuf::from(app_data).join("cc-semaphore/config.json")
}

#[cfg(not(target_os = "windows"))]
fn path() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME must be set");
    PathBuf::from(home).join(".config/cc-semaphore/config.json")
}

fn parse_windows_poll_interval_ms(bytes: &[u8]) -> Option<u64> {
    serde_json::from_slice::<RawConfig>(bytes)
        .ok()?
        .windows_poll_interval_ms
}

/// Reads `windowsPollIntervalMs` from the shared config file. Falls back
/// to `DEFAULT_WINDOWS_POLL_INTERVAL_MS` for a missing file, unparseable
/// JSON, or a missing/invalid field — a bad config must never prevent the
/// app from starting (same tolerance as the daemon's own `config::load`).
pub fn windows_poll_interval_ms() -> u64 {
    std::fs::read(path())
        .ok()
        .and_then(|bytes| parse_windows_poll_interval_ms(&bytes))
        .unwrap_or(DEFAULT_WINDOWS_POLL_INTERVAL_MS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_configured_value() {
        assert_eq!(
            parse_windows_poll_interval_ms(br#"{"windowsPollIntervalMs": 250}"#),
            Some(250)
        );
    }

    #[test]
    fn ignores_unrelated_fields() {
        assert_eq!(
            parse_windows_poll_interval_ms(br#"{"inotifyDebounceMs": 150}"#),
            None
        );
    }

    #[test]
    fn tolerates_invalid_json() {
        assert_eq!(parse_windows_poll_interval_ms(b"not json"), None);
    }
}
