//! Environment detection. See 02_design.md §3.2.

/// True if running under WSL (1 or 2). Distinguishing WSL1 from WSL2 has
/// not been verified on real Windows hardware yet (03_plan.md Phase 0-C,
/// deferred). Until that's done, any "microsoft" match is treated as
/// WSL1-style (polling), which is the conservative choice: polling always
/// works, whereas trusting inotify on an environment we haven't verified
/// would not (01_requirements.md §4.2).
pub fn is_wsl() -> bool {
    std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .map(|s| s.to_lowercase().contains("microsoft"))
        .unwrap_or(false)
}

pub fn home_dir() -> std::path::PathBuf {
    std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .expect("HOME must be set")
}

pub fn sessions_dir() -> std::path::PathBuf {
    home_dir().join(".claude/sessions")
}
