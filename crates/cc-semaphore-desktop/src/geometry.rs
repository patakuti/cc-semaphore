//! Persists the window's position and size across restarts, per
//! 02_design.md §7.2 (`~/.config/cc-semaphore/window.json`).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{PhysicalPosition, PhysicalSize, WebviewWindow};

#[derive(Serialize, Deserialize)]
struct Geometry {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

fn path() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME must be set");
    PathBuf::from(home).join(".config/cc-semaphore/window.json")
}

/// Applies the saved geometry to `window`, if any was saved. The window is
/// created hidden (`visible: false` in tauri.conf.json) precisely so this
/// can run before the first paint, avoiding a visible jump.
pub fn restore(window: &WebviewWindow) {
    let Ok(bytes) = std::fs::read(path()) else {
        return;
    };
    let Ok(g) = serde_json::from_slice::<Geometry>(&bytes) else {
        return;
    };
    let _ = window.set_position(PhysicalPosition::new(g.x, g.y));
    let _ = window.set_size(PhysicalSize::new(g.width, g.height));
}

pub fn save(window: &WebviewWindow) {
    let (Ok(pos), Ok(size)) = (window.outer_position(), window.inner_size()) else {
        return;
    };
    let g = Geometry {
        x: pos.x,
        y: pos.y,
        width: size.width,
        height: size.height,
    };
    let target = path();
    if let Some(parent) = target.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(&g) {
        let _ = std::fs::write(target, bytes);
    }
}
