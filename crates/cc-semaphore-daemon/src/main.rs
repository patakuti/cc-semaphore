mod config;
mod env;
mod lock;
mod scan;
mod systemd;
mod targets;
mod watch_linux;
mod writer;
mod wsl1_autostart;

use cc_semaphore_core::{SessionState, Snapshot};
use lock::DaemonLock;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::thread;
use std::time::Duration;
use writer::Writer;

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("daemon") => cmd_daemon(),
        Some("once") => cmd_once(),
        Some("watch") => cmd_watch(),
        Some("install-service") => {
            if let Err(e) = systemd::install() {
                eprintln!("cc-semaphored: {e}");
                std::process::exit(1);
            }
        }
        Some("install-wsl1-autostart") => {
            if let Err(e) = wsl1_autostart::install() {
                eprintln!("cc-semaphored: {e}");
                std::process::exit(1);
            }
        }
        other => {
            eprintln!(
                "cc-semaphored: unknown command {other:?}\n\
                 Usage: cc-semaphored <daemon|once|watch|install-service|install-wsl1-autostart>"
            );
            std::process::exit(2);
        }
    }
}

/// Blocks until it's time to rescan: either a change was reported on `rx`
/// (in which case it drains further changes for up to `debounce` of quiet
/// before returning, coalescing a burst into one rescan), or `tick` elapses
/// with no changes at all (the periodic liveness check — see
/// 02_design.md §0.5 on why this is required, not just an optimization).
fn wait_for_wake(rx: &Receiver<()>, tick: Duration, debounce: Duration) {
    match rx.recv_timeout(tick) {
        Ok(()) => loop {
            match rx.recv_timeout(debounce) {
                Ok(()) => continue,
                Err(RecvTimeoutError::Timeout) => return,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        },
        Err(RecvTimeoutError::Timeout) => {}
        Err(RecvTimeoutError::Disconnected) => {
            // Watcher thread died unexpectedly; avoid busy-looping.
            thread::sleep(tick);
        }
    }
}

fn cmd_daemon() {
    let cfg = config::load();
    let _lock = match DaemonLock::acquire(&targets::lock_path()) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("cc-semaphored: {e}");
            std::process::exit(1);
        }
    };

    let sessions_dir = env::sessions_dir();
    let mut writer = Writer::new(targets::resolve(&cfg));

    // Scan once immediately so the snapshot reflects reality from the
    // moment the daemon starts, rather than only after the first change
    // event or the first liveness tick (up to `liveness_tick_secs` later).
    writer.write(&scan::scan(&sessions_dir, &cfg.include_kinds));

    if env::is_wsl() {
        let interval = Duration::from_millis(cfg.wsl1_poll_interval_ms);
        loop {
            thread::sleep(interval);
            writer.write(&scan::scan(&sessions_dir, &cfg.include_kinds));
        }
    } else {
        let rx = watch_linux::spawn(sessions_dir.clone());
        let tick = Duration::from_secs(cfg.liveness_tick_secs);
        let debounce = Duration::from_millis(cfg.inotify_debounce_ms);
        loop {
            wait_for_wake(&rx, tick, debounce);
            writer.write(&scan::scan(&sessions_dir, &cfg.include_kinds));
        }
    }
}

fn cmd_once() {
    let cfg = config::load();
    let snapshot = scan::scan(&env::sessions_dir(), &cfg.include_kinds);
    println!("{}", serde_json::to_string_pretty(&snapshot).unwrap());
}

fn cmd_watch() {
    let cfg = config::load();
    let sessions_dir = env::sessions_dir();

    let shared = std::sync::Arc::new(std::sync::Mutex::new(scan::scan(
        &sessions_dir,
        &cfg.include_kinds,
    )));

    {
        let shared = std::sync::Arc::clone(&shared);
        let sessions_dir = sessions_dir.clone();
        let include_kinds = cfg.include_kinds.clone();
        let is_wsl = env::is_wsl();
        let poll = Duration::from_millis(cfg.wsl1_poll_interval_ms);
        let tick = Duration::from_secs(cfg.liveness_tick_secs);
        let debounce = Duration::from_millis(cfg.inotify_debounce_ms);
        thread::spawn(move || {
            if is_wsl {
                loop {
                    let snap = scan::scan(&sessions_dir, &include_kinds);
                    *shared.lock().unwrap() = snap;
                    thread::sleep(poll);
                }
            } else {
                let rx = watch_linux::spawn(sessions_dir.clone());
                loop {
                    wait_for_wake(&rx, tick, debounce);
                    let snap = scan::scan(&sessions_dir, &include_kinds);
                    *shared.lock().unwrap() = snap;
                }
            }
        });
    }

    loop {
        render_table(&shared.lock().unwrap());
        thread::sleep(Duration::from_secs(1));
    }
}

fn render_table(snapshot: &Snapshot) {
    print!("\x1B[2J\x1B[H"); // clear screen, move cursor to top-left
    println!(
        "cc-semaphore watch — running:{} waiting:{} idle:{}\n",
        snapshot.counts.running, snapshot.counts.waiting, snapshot.counts.idle
    );
    if snapshot.sessions.is_empty() {
        println!("(no sessions)");
        return;
    }
    let now = scan::now_ms();
    for s in &snapshot.sessions {
        let (color, label) = match s.state {
            SessionState::Running => ("\x1B[32m", "running"),
            SessionState::Waiting => ("\x1B[33m", "waiting"),
            SessionState::Idle => ("\x1B[31m", "idle"),
        };
        let elapsed = cc_semaphore_core::format::elapsed_since_ms(s.since, now);
        let waiting_for = s
            .waiting_for
            .as_deref()
            .map(|w| format!(" ({w})"))
            .unwrap_or_default();
        println!(
            "{color}{label:7}\x1B[0m {:<7} {:<24} {:<40} {elapsed}{waiting_for}",
            s.pid,
            s.name.as_deref().unwrap_or("-"),
            s.cwd,
        );
    }
}
