// Prevent an extra console window on Windows in release builds; harmless
// elsewhere.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod geometry;
mod watcher;

use cc_semaphore_core::Snapshot;
use serde::Serialize;
use std::thread;
use std::time::Duration;
use tauri::{Emitter, Manager};

/// The event payload emitted to the webview on every change. See
/// 02_design.md §7.3 and ui/tauri-bridge.js.
#[derive(Serialize, Clone)]
struct SnapshotEvent {
    snapshot: Snapshot,
    #[serde(rename = "homeDir")]
    home_dir: Option<String>,
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
    read_snapshot(&cc_semaphore_core::local_state_path())
        .map(|snapshot| SnapshotEvent { snapshot, home_dir })
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_snapshot])
        .setup(|app| {
            let window = app
                .get_webview_window("main")
                .expect("the \"main\" window is declared in tauri.conf.json");

            geometry::restore(&window);
            let _ = window.show();

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

            let home_dir = std::env::var("HOME").ok();
            let state_path = cc_semaphore_core::local_state_path();
            let rx = watcher::spawn(state_path.clone());
            let app_handle = app.handle().clone();
            thread::spawn(move || {
                // The initial state is fetched by the webview itself via
                // the get_snapshot command; this loop only re-emits on
                // every subsequent change the watcher reports.
                while rx.recv().is_ok() {
                    if let Some(snapshot) = read_snapshot(&state_path) {
                        let _ = app_handle.emit(
                            "snapshot",
                            SnapshotEvent {
                                snapshot,
                                home_dir: home_dir.clone(),
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
