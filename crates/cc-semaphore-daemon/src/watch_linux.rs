//! Linux inotify watcher. Runs on its own thread and sends a `()` on the
//! returned channel whenever something in `sessions_dir` changes.
//! See 02_design.md §3.3; mask corrected per docs/measurements.md #5
//! (Claude Code writes `<pid>.json` in place — CLOSE_WRITE, not rename).

use inotify::{Inotify, WatchMask};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

const RETRY_DELAY: Duration = Duration::from_secs(5);

pub fn spawn(sessions_dir: PathBuf) -> Receiver<()> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || loop {
        match watch_once(&sessions_dir, &tx) {
            // Receiver was dropped: nobody's listening anymore, stop.
            Ok(()) => return,
            Err(e) => {
                eprintln!(
                    "cc-semaphored: inotify watch on {} failed ({e}); retrying in {}s",
                    sessions_dir.display(),
                    RETRY_DELAY.as_secs()
                );
                thread::sleep(RETRY_DELAY);
            }
        }
    });
    rx
}

fn watch_once(sessions_dir: &std::path::Path, tx: &mpsc::Sender<()>) -> std::io::Result<()> {
    let mut inotify = Inotify::init()?;
    inotify.watches().add(
        sessions_dir,
        WatchMask::CLOSE_WRITE | WatchMask::CREATE | WatchMask::DELETE,
    )?;

    let mut buffer = [0; 4096];
    loop {
        let events = inotify.read_events_blocking(&mut buffer)?;
        if events.count() > 0 && tx.send(()).is_err() {
            return Ok(()); // receiver gone: exit quietly, no need to retry
        }
    }
}
