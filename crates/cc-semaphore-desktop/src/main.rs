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
    /// `false` when `snapshot.version` doesn't match the version this
    /// binary was built against — see 02_design.md §2.2.1. A successful
    /// deserialize doesn't by itself mean the format is one we understand:
    /// a version bump can change field *meaning* without changing shape.
    #[serde(rename = "versionSupported")]
    version_supported: bool,
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
    read_snapshot(&state_path).map(|snapshot| {
        let version_supported = snapshot.version == cc_semaphore_core::SNAPSHOT_VERSION;
        SnapshotEvent {
            snapshot,
            home_dir,
            daemon_alive,
            version_supported,
        }
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

/// Registers `tauri-plugin-autostart` on Windows only (01_requirements.md
/// §5.4) — there is nothing to launch-on-login on Linux, which already has
/// its own opt-in mechanism (`cc-semaphored install-service`, §3.7).
#[cfg(target_os = "windows")]
fn register_autostart_plugin(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder.plugin(tauri_plugin_autostart::init(
        tauri_plugin_autostart::MacosLauncher::LaunchAgent,
        None,
    ))
}

#[cfg(not(target_os = "windows"))]
fn register_autostart_plugin(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder
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

    let builder = register_autostart_plugin(
        tauri::Builder::default().invoke_handler(tauri::generate_handler![get_snapshot]),
    );

    builder
        .setup(move |app| {
            let window = app
                .get_webview_window("main")
                .expect("the \"main\" window is declared in tauri.conf.json");

            // Registers this exe to launch on Windows login, unconditionally
            // on every run (idempotent — enable() on an already-enabled
            // registration is a no-op, not an error). No opt-out toggle:
            // running the installer is itself the explicit "set this up"
            // action (01_requirements.md §5.4), and Phase 7's completion
            // criterion is that monitoring is already running after an OS
            // boot with no further user action. Linux's equivalent
            // (`cc-semaphored install-service`) stays an explicit opt-in
            // command instead — asymmetric on purpose, since installing
            // this desktop app on Windows already *is* that explicit step.
            #[cfg(target_os = "windows")]
            {
                use tauri_plugin_autostart::ManagerExt;
                if let Err(e) = app.autolaunch().enable() {
                    eprintln!("cc-semaphore-desktop: failed to register autostart: {e}");
                }
            }

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
                        let version_supported =
                            snapshot.version == cc_semaphore_core::SNAPSHOT_VERSION;
                        let _ = app_handle.emit(
                            "snapshot",
                            SnapshotEvent {
                                snapshot,
                                home_dir: home_dir.clone(),
                                daemon_alive,
                                version_supported,
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
