//! Watches the local snapshot file and sends `()` whenever it may have
//! changed. See 02_design.md §1 (architecture diagram) and §8: Linux uses
//! `FileMonitor`-style event notification (inotify), Windows uses a fixed
//! mtime poll because this app only ever reads a snapshot written by a
//! *different* machine (WSL1) across the DrvFs boundary, where notification
//! delivery is unconfirmed (§8's open item, deferred to Phase 6).

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

const RETRY_DELAY: Duration = Duration::from_secs(5);

#[cfg(target_os = "linux")]
pub fn spawn(state_path: PathBuf) -> Receiver<()> {
    let (tx, rx) = mpsc::channel();
    let watch_dir = state_path
        .parent()
        .expect("state path always has a parent directory")
        .to_path_buf();
    let file_name = state_path
        .file_name()
        .expect("state path always has a file name")
        .to_owned();

    thread::spawn(move || loop {
        match watch_once(&watch_dir, &file_name, &tx) {
            Ok(()) => return, // receiver dropped, nobody's listening
            Err(e) => {
                eprintln!(
                    "cc-semaphore-desktop: inotify watch on {} failed ({e}); retrying in {}s",
                    watch_dir.display(),
                    RETRY_DELAY.as_secs()
                );
                thread::sleep(RETRY_DELAY);
            }
        }
    });
    rx
}

#[cfg(target_os = "linux")]
fn watch_once(
    watch_dir: &std::path::Path,
    file_name: &std::ffi::OsStr,
    tx: &mpsc::Sender<()>,
) -> std::io::Result<()> {
    use inotify::{Inotify, WatchMask};

    // The daemon creates this directory lazily on its first write; wait
    // for it to exist rather than failing outright.
    while !watch_dir.exists() {
        thread::sleep(RETRY_DELAY);
    }

    let mut inotify = Inotify::init()?;
    // CLOSE_WRITE/CREATE cover a direct write; MOVED_TO covers the
    // tmp-file+rename swap the writer actually uses (02_design.md §2.5).
    inotify.watches().add(
        watch_dir,
        WatchMask::CLOSE_WRITE | WatchMask::CREATE | WatchMask::MOVED_TO | WatchMask::DELETE,
    )?;

    let mut buffer = [0; 4096];
    loop {
        let events = inotify.read_events_blocking(&mut buffer)?;
        // The same directory also receives heartbeat.json churn on every
        // daemon tick (cc_semaphore_core::heartbeat); only state.json
        // itself changing should wake this watcher.
        let relevant = events.filter(|e| e.name == Some(file_name)).count() > 0;
        if relevant && tx.send(()).is_err() {
            return Ok(());
        }
    }
}

#[cfg(not(target_os = "linux"))]
pub fn spawn(state_path: PathBuf) -> Receiver<()> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut last_mtime = None;
        loop {
            let mtime = std::fs::metadata(&state_path)
                .and_then(|m| m.modified())
                .ok();
            if mtime != last_mtime {
                last_mtime = mtime;
                if tx.send(()).is_err() {
                    return;
                }
            }
            thread::sleep(Duration::from_millis(500));
        }
    });
    rx
}
