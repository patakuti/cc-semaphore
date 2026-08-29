//! Reads `~/.claude/sessions/*.json` and turns it into a `Snapshot`.
//! See 02_design.md §2.3, §3.5.

use cc_semaphore_core::proc::RealProcSource;
use cc_semaphore_core::{build_snapshot, parse_session_record, SessionRecord, Snapshot};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// A candidate session file's name is all-digits followed by `.json`
/// (excludes the `<pid>.<hash>.key` files, which we never touch).
fn is_session_filename(name: &str) -> bool {
    name.strip_suffix(".json")
        .is_some_and(|stem| !stem.is_empty() && stem.bytes().all(|b| b.is_ascii_digit()))
}

fn read_records(sessions_dir: &Path) -> Vec<SessionRecord> {
    let Ok(entries) = std::fs::read_dir(sessions_dir) else {
        return vec![];
    };
    entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_str().is_some_and(is_session_filename))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|json| parse_session_record(&json))
        .collect()
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown-host".to_string())
}

pub fn scan(sessions_dir: &Path, include_kinds: &[String]) -> Snapshot {
    let records = read_records(sessions_dir);
    let include_kinds: Vec<&str> = include_kinds.iter().map(String::as_str).collect();
    build_snapshot(
        &records,
        &RealProcSource,
        &include_kinds,
        now_ms(),
        &hostname(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_filename_matches_digits_dot_json_only() {
        assert!(is_session_filename("2071740.json"));
        assert!(!is_session_filename(
            "2071740.65e14189526f2a0c7801f7e29c4567.key"
        ));
        assert!(!is_session_filename("notapid.json"));
        assert!(!is_session_filename(".json"));
        assert!(!is_session_filename("2071740.json.tmp"));
    }
}
