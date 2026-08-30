//! Canonical state colors. See 02_design.md §4.1.
//!
//! These values must stay in sync with `ui/panel.css` and
//! `extensions/cc-semaphore@patakuti/stylesheet.css`; a test in this crate
//! verifies the three agree (02_design.md §12).

pub const RUNNING: &str = "#2ec27e";
pub const WAITING: &str = "#f5c211";
pub const IDLE: &str = "#e01b24";

use crate::types::SessionState;

pub fn color_for(state: SessionState) -> &'static str {
    match state {
        SessionState::Running => RUNNING,
        SessionState::Waiting => WAITING,
        SessionState::Idle => IDLE,
    }
}

/// The same colors as `(r, g, b)` bytes, for consumers that render pixels
/// rather than CSS (the Windows tray icon rasterizer, §6.1).
pub fn rgb_for(state: SessionState) -> (u8, u8, u8) {
    hex_to_rgb(color_for(state))
}

fn hex_to_rgb(hex: &str) -> (u8, u8, u8) {
    let h = hex.trim_start_matches('#');
    let r = u8::from_str_radix(&h[0..2], 16).expect("color_for() always returns valid hex");
    let g = u8::from_str_radix(&h[2..4], 16).expect("color_for() always returns valid hex");
    let b = u8::from_str_radix(&h[4..6], 16).expect("color_for() always returns valid hex");
    (r, g, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GNOME_STYLESHEET: &str =
        include_str!("../../../extensions/cc-semaphore@patakuti/stylesheet.css");
    const UI_PANEL_CSS: &str = include_str!("../../../ui/panel.css");

    #[test]
    fn gnome_stylesheet_uses_the_same_hex_colors() {
        assert!(
            GNOME_STYLESHEET.contains(RUNNING),
            "stylesheet.css .ccs-running must use {RUNNING}"
        );
        assert!(
            GNOME_STYLESHEET.contains(WAITING),
            "stylesheet.css .ccs-waiting must use {WAITING}"
        );
        assert!(
            GNOME_STYLESHEET.contains(IDLE),
            "stylesheet.css .ccs-idle must use {IDLE}"
        );
    }

    #[test]
    fn ui_panel_css_uses_the_same_hex_colors() {
        assert!(
            UI_PANEL_CSS.contains(RUNNING),
            "panel.css must use {RUNNING}"
        );
        assert!(
            UI_PANEL_CSS.contains(WAITING),
            "panel.css must use {WAITING}"
        );
        assert!(UI_PANEL_CSS.contains(IDLE), "panel.css must use {IDLE}");
    }

    #[test]
    fn rgb_for_matches_the_hex_constants() {
        assert_eq!(rgb_for(SessionState::Running), (0x2e, 0xc2, 0x7e));
        assert_eq!(rgb_for(SessionState::Waiting), (0xf5, 0xc2, 0x11));
        assert_eq!(rgb_for(SessionState::Idle), (0xe0, 0x1b, 0x24));
    }
}
