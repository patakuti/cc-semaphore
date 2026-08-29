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
}
