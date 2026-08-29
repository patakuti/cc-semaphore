//! Single-instance guard for `cc-semaphored daemon`. See 02_design.md §3.7.

use cc_semaphore_core::proc::{is_alive, RealProcSource};
use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

pub struct DaemonLock {
    path: PathBuf,
}

impl DaemonLock {
    /// Acquires the lock at `path`, creating it exclusively (`O_EXCL`) with
    /// this process's pid as its contents. If a lock file already exists
    /// but the pid inside it is no longer alive (the previous daemon
    /// crashed), the stale lock is removed and acquisition is retried once.
    pub fn acquire(path: &Path) -> Result<Self, String> {
        Self::try_acquire(path, true)
    }

    fn try_acquire(path: &Path, retry_if_stale: bool) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        match OpenOptions::new().write(true).create_new(true).open(path) {
            Ok(mut f) => {
                write!(f, "{}", std::process::id()).map_err(|e| e.to_string())?;
                Ok(DaemonLock {
                    path: path.to_path_buf(),
                })
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                if retry_if_stale && is_stale(path) {
                    let _ = std::fs::remove_file(path);
                    return Self::try_acquire(path, false);
                }
                Err(format!(
                    "cc-semaphored is already running (lock held: {})",
                    path.display()
                ))
            }
            Err(e) => Err(e.to_string()),
        }
    }
}

fn is_stale(lock_path: &Path) -> bool {
    let Ok(contents) = std::fs::read_to_string(lock_path) else {
        return true; // unreadable lock file: treat as stale, safe to replace
    };
    match contents.trim().parse::<u32>() {
        Ok(pid) => !is_alive(&RealProcSource, pid, None),
        Err(_) => true,
    }
}

impl Drop for DaemonLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
