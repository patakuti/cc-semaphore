//! Configuration file and defaults. See 02_design.md §10.

use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct RawConfig {
    #[serde(rename = "includeKinds")]
    include_kinds: Option<Vec<String>>,
    #[serde(rename = "inotifyDebounceMs")]
    inotify_debounce_ms: Option<u64>,
    #[serde(rename = "livenessTickSecs")]
    liveness_tick_secs: Option<u64>,
    #[serde(rename = "wsl1PollIntervalMs")]
    wsl1_poll_interval_ms: Option<u64>,
    #[serde(rename = "windowsStateDir")]
    windows_state_dir: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub include_kinds: Vec<String>,
    pub inotify_debounce_ms: u64,
    pub liveness_tick_secs: u64,
    pub wsl1_poll_interval_ms: u64,
    /// Explicit override for the WSL1→Windows write target. See
    /// `resolve_windows_state_dir` for the full priority order when this
    /// is `None`.
    pub windows_state_dir: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            include_kinds: vec!["interactive".to_string()],
            inotify_debounce_ms: 150,
            liveness_tick_secs: 5,
            wsl1_poll_interval_ms: 1000,
            windows_state_dir: None,
        }
    }
}

pub fn config_path() -> PathBuf {
    crate::env::home_dir().join(".config/cc-semaphore/config.json")
}

/// Loads config from `~/.config/cc-semaphore/config.json` if present,
/// overlaying it onto the defaults. A missing file is not an error; an
/// unparseable one is reported but falls back to defaults rather than
/// preventing the daemon from starting.
pub fn load() -> Config {
    let mut config = Config::default();
    let path = config_path();
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return config;
    };
    match serde_json::from_str::<RawConfig>(&contents) {
        Ok(raw) => {
            if let Some(v) = raw.include_kinds {
                config.include_kinds = v;
            }
            if let Some(v) = raw.inotify_debounce_ms {
                config.inotify_debounce_ms = v;
            }
            if let Some(v) = raw.liveness_tick_secs {
                config.liveness_tick_secs = v;
            }
            if let Some(v) = raw.wsl1_poll_interval_ms {
                config.wsl1_poll_interval_ms = v;
            }
            if let Some(v) = raw.windows_state_dir {
                config.windows_state_dir = Some(PathBuf::from(v));
            }
        }
        Err(e) => {
            eprintln!(
                "cc-semaphored: warning: {} is not valid JSON ({e}); using defaults",
                path.display()
            );
        }
    }
    config
}
