//! Wires the Windows system tray (02_design.md §6): icon rasterization +
//! caching (icon.rs) driven by the rotation/blink state machine
//! (tray_state.rs), the always-current tooltip, the right-click menu
//! (Show panel / Quit), the left-click popup, and the daemon-liveness
//! indicator (02_design.md §3.9).
//!
//! ## Platform note
//!
//! Tauri's tray backend documents that on Linux, `TrayIconEvent::Click` is
//! never emitted (right-click still opens the context menu, but left click
//! does nothing) — confirmed by reading `tauri-2.11.5/src/tray/mod.rs`
//! rather than assumed. So the popup-on-left-click path here can only be
//! exercised for real on Windows; on Linux this module still renders the
//! icon and serves the right-click menu, which is enough to sanity-check
//! rendering on this dev machine (docs/measurements.md).

use crate::icon::{IconCache, IconPhase, ICON_SIZE};
use crate::tray_state::{Display, TrayState};
use cc_semaphore_core::StateCounts;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, Rect};

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

/// `initial_counts` is read synchronously from whatever snapshot already
/// exists on disk (if any) so the tray's baseline doesn't depend on the
/// webview loading or the watcher firing — the tray must work even if the
/// user never opens a window.
pub fn setup(
    app: &AppHandle,
    state_path: PathBuf,
    initial_counts: StateCounts,
) -> tauri::Result<()> {
    let show_panel = MenuItem::with_id(app, "show-panel", "Show panel", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_panel, &quit])?;

    let mut state = TrayState::new();
    state.on_counts(initial_counts.clone(), now_ms());
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
            s.icons
                .get(
                    initial_counts.running,
                    cc_semaphore_core::SessionState::Running,
                    IconPhase::Normal,
                )
                .clone()
        } else {
            s.icons.offline().clone()
        };
        to_image(rgba)
    };

    let tray = TrayIconBuilder::with_id("cc-semaphore-tray")
        .icon(initial_icon)
        .tooltip(tooltip(initial_alive, &initial_counts))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show-panel" => toggle_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                rect,
                ..
            } = event
            {
                toggle_popup(tray.app_handle(), &rect);
            }
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
            s.state.expire(now);
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

/// Feeds a freshly read snapshot's counts into the tray's blink state
/// machine. Call this from the same place `main.rs` emits the "snapshot"
/// event to the windows; the icon/tooltip themselves are refreshed by the
/// tick thread started in `setup()`, not from here.
pub fn on_snapshot(app: &AppHandle, counts: &StateCounts) {
    let Some(handle) = app.try_state::<TrayHandle>() else {
        return;
    };
    let mut s = handle.shared.lock().expect("tray state mutex");
    s.counts = counts.clone();
    s.state.on_counts(counts.clone(), now_ms());
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

/// Shows/hides the popup window near the tray icon on left click
/// (02_design.md §6.4). Only reachable on platforms where Tauri actually
/// emits `TrayIconEvent::Click` — Windows, per the module doc comment
/// above.
fn toggle_popup(app: &AppHandle, tray_rect: &Rect) {
    let Some(w) = app.get_webview_window("popup") else {
        return;
    };
    if w.is_visible().unwrap_or(false) {
        let _ = w.hide();
        return;
    }
    position_near_tray(&w, tray_rect);
    let _ = w.show();
    let _ = w.set_focus();
}

/// Places the popup's bottom-left corner near the tray icon's position,
/// which keeps it on-screen for the common case of a bottom-right tray
/// (Windows). Exact anchoring depends on real screen/taskbar geometry that
/// can only be confirmed on Windows hardware (03_plan.md Phase 6).
fn position_near_tray(window: &tauri::WebviewWindow, tray_rect: &Rect) {
    let scale = window.scale_factor().unwrap_or(1.0);
    let tray_pos = tray_rect.position.to_physical::<f64>(scale);
    let win_size = window.outer_size().unwrap_or(PhysicalSize::new(360, 480));
    let x = (tray_pos.x - win_size.width as f64).max(0.0) as i32;
    let y = (tray_pos.y - win_size.height as f64).max(0.0) as i32;
    let _ = window.set_position(PhysicalPosition::new(x, y));
}

/// Hides the popup when it loses focus, so clicking elsewhere dismisses it
/// like a native flyout (02_design.md §6.4).
pub fn hide_popup_on_focus_lost(window: &tauri::WebviewWindow) {
    let window = window.clone();
    window.clone().on_window_event(move |event| {
        if let tauri::WindowEvent::Focused(false) = event {
            let _ = window.hide();
        }
    });
}
