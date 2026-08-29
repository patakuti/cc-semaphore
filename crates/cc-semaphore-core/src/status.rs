use crate::types::SessionState;

/// Maps a raw Claude Code `status` value to one of the three states.
///
/// Mapping confirmed against Claude Code 2.1.251 (02_design.md §0.3):
/// `busy`/`shell` → running, `waiting` → waiting, `idle` → idle.
/// Anything else (including a missing/unknown value) returns `None` so the
/// session is excluded from the list rather than shown with a wrong color.
///
/// `shell` was not observed being emitted in practice (docs/measurements.md
/// #3) but is mapped defensively since Claude Code's own validator still
/// accepts it.
pub fn map_status(raw: &str) -> Option<SessionState> {
    match raw {
        "busy" | "shell" => Some(SessionState::Running),
        "waiting" => Some(SessionState::Waiting),
        "idle" => Some(SessionState::Idle),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_known_values() {
        assert_eq!(map_status("busy"), Some(SessionState::Running));
        assert_eq!(map_status("shell"), Some(SessionState::Running));
        assert_eq!(map_status("waiting"), Some(SessionState::Waiting));
        assert_eq!(map_status("idle"), Some(SessionState::Idle));
    }

    #[test]
    fn rejects_unknown_values_instead_of_defaulting() {
        assert_eq!(map_status(""), None);
        assert_eq!(map_status("working"), None);
        assert_eq!(map_status("BUSY"), None);
    }
}
