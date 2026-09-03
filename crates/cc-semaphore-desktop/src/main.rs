// Prevent an extra console window on Windows in release builds; harmless
// elsewhere.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod geometry;
mod icon;
mod tray;
mod tray_state;
mod watcher;

use cc_semaphore_core::Snapshot;
use serde::Serialize;
use std::thread;
use std::time::Duration;
use tauri::{Emitter, Manager};

/// The event payload emitted to the webview on every change. See
/// 02_design.md §7.3 and ui/tauri-bridge.js. `daemon_alive` reflects the
/// heartbeat at the moment this payload was built (02_design.md §3.9); it
/// only covers the "loaded/changed while already down" case; the
/// "went down with no further changes" case is covered separately by the
/// tray's periodic "daemon-status" event (tray.rs), since nothing here
/// re-fires on its own when the daemon simply stops.
#[derive(Serialize, Clone)]
struct SnapshotEvent {
    snapshot: Snapshot,
    #[serde(rename = "homeDir")]
    home_dir: Option<String>,
    #[serde(rename = "daemonAlive")]
    daemon_alive: bool,
}

/// Reads and parses the snapshot file, tolerating a torn read (the writer
/// on this machine renames atomically, but a Windows frontend reading a
/// snapshot bridged from WSL1 over DrvFs cannot assume that — 02_design.md
/// §2.5). Retries once after 50ms; if it still fails, returns `None` and
/// the caller keeps showing the last good snapshot.
fn read_snapshot(path: &std::path::Path) -> Option<Snapshot> {
    for attempt in 0..2 {
        match std::fs::read(path) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(snapshot) => return Some(snapshot),
                Err(_) if attempt == 0 => thread::sleep(Duration::from_millis(50)),
                Err(_) => return None,
            },
            Err(_) if attempt == 0 => thread::sleep(Duration::from_millis(50)),
            Err(_) => return None,
        }
    }
    None
}

/// Called once by ui/tauri-bridge.js right after it attaches its `snapshot`
/// event listener, to fetch whatever is already on disk. Without this,
/// the startup emit below can race the webview's module load and be lost
/// forever: the writer skips re-writing state.json when nothing changed
/// (02_design.md §2.5), so no later file event would ever re-trigger it.
#[tauri::command]
fn get_snapshot() -> Option<SnapshotEvent> {
    let home_dir = std::env::var("HOME").ok();
    let state_path = cc_semaphore_core::local_state_path();
    let daemon_alive = cc_semaphore_core::heartbeat::daemon_alive(&state_path);
    read_snapshot(&state_path).map(|snapshot| SnapshotEvent {
        snapshot,
        home_dir,
        daemon_alive,
    })
}

fn read_counts_and_sessions(
    state_path: &std::path::Path,
) -> (
    cc_semaphore_core::StateCounts,
    Vec<cc_semaphore_core::SessionEntry>,
) {
    match read_snapshot(state_path) {
        Some(s) => (s.counts, s.sessions),
        None => (cc_semaphore_core::StateCounts::default(), Vec::new()),
    }
}

fn main() {
    // `--panel`: show the always-on-top window immediately on launch,
    // rather than leaving it hidden until "Show panel" from the tray menu
    // (02_design.md §7.2). The tray icon still starts up as usual — this
    // is meant for launching the panel outside of the tray flow (a desktop
    // shortcut/launcher running `cc-semaphore-desktop --panel`), not a
    // trayless mode, so quitting still works the normal way (tray menu's
    // "Quit").
    let show_panel_on_launch = std::env::args().any(|arg| arg == "--panel");

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_snapshot])
        .setup(move |app| {
            let window = app
                .get_webview_window("main")
                .expect("the \"main\" window is declared in tauri.conf.json");

            // The panel starts hidden by default (user feedback: it's a
            // detail view, not something to see on every launch) and is
            // toggled via the tray's "Show panel" menu item
            // (tray::toggle_main_window). Still restore its last geometry
            // so it opens where the user left it once shown.
            geometry::restore(&window);

            if show_panel_on_launch {
                let _ = window.show();
                let _ = window.set_focus();
            }

            {
                let event_window = window.clone();
                window.on_window_event(move |event| {
                    if matches!(
                        event,
                        tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_)
                    ) {
                        geometry::save(&event_window);
                    }
                });
            }

            if let Some(popup) = app.get_webview_window("popup") {
                tray::hide_popup_on_focus_lost(&popup);
            }

            let state_path = cc_semaphore_core::local_state_path();
            let (initial_counts, initial_sessions) = read_counts_and_sessions(&state_path);
            tray::setup(
                app.handle(),
                state_path.clone(),
                initial_counts,
                initial_sessions,
            )?;

            let home_dir = std::env::var("HOME").ok();
            let rx = watcher::spawn(state_path.clone(), config::windows_poll_interval_ms());
            let app_handle = app.handle().clone();
            thread::spawn(move || {
                // The initial state is fetched by the webview itself via
                // the get_snapshot command, and the tray's baseline is
                // fetched synchronously in tray::setup() above; this loop
                // only reacts to every subsequent change the watcher
                // reports.
                while rx.recv().is_ok() {
                    if let Some(snapshot) = read_snapshot(&state_path) {
                        tray::on_snapshot(&app_handle, &snapshot.counts, &snapshot.sessions);
                        let daemon_alive = cc_semaphore_core::heartbeat::daemon_alive(&state_path);
                        let _ = app_handle.emit(
                            "snapshot",
                            SnapshotEvent {
                                snapshot,
                                home_dir: home_dir.clone(),
                                daemon_alive,
                            },
                        );
                    }
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running cc-semaphore-desktop");
}
