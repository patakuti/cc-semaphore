//! Wires the Windows system tray (02_design.md §6): icon rasterization +
//! caching (icon.rs) driven by the rotation/blink state machine
//! (tray_state.rs), the always-current tooltip, the menu (Show panel /
//! Quit, on both left- and right-click), and the daemon-liveness indicator
//! (02_design.md §3.9).
//!
//! There used to also be a separate left-click popup window (`popup.html`)
//! showing its own copy of the session list. Removed (user feedback,
//! 2026-09-05): confusing to have two different-looking session lists
//! (that popup's plain color-coded text vs. the always-on-top panel's
//! badges/collapse), and it would sometimes appear to show a stale/old
//! view. Left-click now shows the same menu as right-click instead
//! (`show_menu_on_left_click(true)`, confirmed against
//! `tauri-2.11.5/src/tray/mod.rs` — this is actually Tauri's own default,
//! which this module had previously turned off specifically to make room
//! for the now-removed popup).

use crate::icon::{IconCache, IconPhase, ICON_SIZE};
use crate::tray_state::{Display, TrayState};
use cc_semaphore_core::{SessionEntry, StateCounts};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager};

const TICK_MS: u64 = 500;

struct Shared {
    state: TrayState,
    counts: StateCounts,
    icons: IconCache,
    daemon_alive: bool,
}

/// Holds everything `on_snapshot` needs to update the tray from outside
/// this module, stashed as Tauri managed state. The `TrayIcon` itself
/// isn't here: it's only ever driven from the tick thread in `setup()`,
/// which owns its own clone.
struct TrayHandle {
    shared: Arc<Mutex<Shared>>,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock is after 1970")
        .as_millis() as i64
}

fn tooltip(daemon_alive: bool, counts: &StateCounts) -> String {
    if daemon_alive {
        format!(
            "running:{} waiting:{} idle:{}",
            counts.running, counts.waiting, counts.idle
        )
    } else {
        "cc-semaphore: daemon not running".to_string()
    }
}

fn to_image(rgba: Vec<u8>) -> Image<'static> {
    Image::new_owned(rgba, ICON_SIZE, ICON_SIZE)
}

/// `initial_counts`/`initial_sessions` are read synchronously from whatever
/// snapshot already exists on disk (if any) so the tray's baseline doesn't
/// depend on the webview loading or the watcher firing — the tray must work
/// even if the user never opens a window.
pub fn setup(
    app: &AppHandle,
    state_path: PathBuf,
    initial_counts: StateCounts,
    initial_sessions: Vec<SessionEntry>,
) -> tauri::Result<()> {
    let show_panel = MenuItem::with_id(app, "show-panel", "Show panel", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_panel, &quit])?;

    let mut state = TrayState::new();
    state.on_snapshot(initial_sessions);
    let initial_alive = cc_semaphore_core::heartbeat::daemon_alive(&state_path);
    let shared = Arc::new(Mutex::new(Shared {
        state,
        counts: initial_counts.clone(),
        icons: IconCache::new(),
        daemon_alive: initial_alive,
    }));

    let initial_icon = {
        let mut s = shared.lock().expect("tray state mutex");
        let rgba = if initial_alive {
            match s.state.display(&initial_counts, now_ms()) {
                Display::Normal(state, value) => {
                    s.icons.get(value, state, IconPhase::Normal).clone()
                }
                Display::Inverted(state, value) => {
                    s.icons.get(value, state, IconPhase::Inverted).clone()
                }
                Display::Empty => s.icons.empty().clone(),
            }
        } else {
            s.icons.offline().clone()
        };
        to_image(rgba)
    };

    let tray = TrayIconBuilder::with_id("cc-semaphore-tray")
        .icon(initial_icon)
        .tooltip(tooltip(initial_alive, &initial_counts))
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show-panel" => toggle_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;

    app.manage(TrayHandle {
        shared: Arc::clone(&shared),
    });

    let app_handle = app.clone();
    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(TICK_MS));
        let now = now_ms();
        let alive = cc_semaphore_core::heartbeat::daemon_alive(&state_path);
        let (rgba, tip, alive_changed) = {
            let mut s = shared.lock().expect("tray state mutex");
            let alive_changed = alive != s.daemon_alive;
            s.daemon_alive = alive;
            let counts = s.counts.clone();
            let rgba = if !alive {
                s.icons.offline().clone()
            } else {
                match s.state.display(&counts, now) {
                    Display::Normal(state, value) => {
                        s.icons.get(value, state, IconPhase::Normal).clone()
                    }
                    Display::Inverted(state, value) => {
                        s.icons.get(value, state, IconPhase::Inverted).clone()
                    }
                    Display::Empty => s.icons.empty().clone(),
                }
            };
            (rgba, tooltip(alive, &counts), alive_changed)
        };
        let _ = tray.set_icon(Some(to_image(rgba)));
        let _ = tray.set_tooltip(Some(tip));
        if alive_changed {
            let _ = app_handle.emit("daemon-status", DaemonStatus { alive });
        }
    });

    Ok(())
}

#[derive(serde::Serialize, Clone)]
struct DaemonStatus {
    alive: bool,
}

/// Feeds a freshly read snapshot's counts and sessions into the tray's
/// blink state machine. Call this from the same place `main.rs` emits the
/// "snapshot" event to the windows; the icon/tooltip themselves are
/// refreshed by the tick thread started in `setup()`, not from here.
pub fn on_snapshot(app: &AppHandle, counts: &StateCounts, sessions: &[SessionEntry]) {
    let Some(handle) = app.try_state::<TrayHandle>() else {
        return;
    };
    let mut s = handle.shared.lock().expect("tray state mutex");
    s.counts = counts.clone();
    s.state.on_snapshot(sessions.to_vec());
}

fn toggle_main_window(app: &AppHandle) {
    let Some(w) = app.get_webview_window("main") else {
        return;
    };
    if w.is_visible().unwrap_or(false) {
        let _ = w.hide();
    } else {
        let _ = w.show();
        let _ = w.set_focus();
    }
}
