//! The tray icon's rotation/interrupt-blink state machine (02_design.md
//! §6.3). Deliberately holds no alert-episode bookkeeping of its own
//! (no fired-at timestamps, no baselines): whether to blink, which kind,
//! and for how long is recomputed fresh on every call from each session's
//! own `since` (when it entered its current state) against the current
//! time. A session counts as "recent" — and therefore blink-worthy — for
//! `ALERT_WINDOW_MS` after its `since`; past that it's just part of the
//! steady rotation like any other. This one predicate, evaluated per
//! session, replaces what used to be a hand-rolled fire/extend/expire
//! state machine over aggregate counts, and fixes every edge case that
//! design ran into (02_design.md §6.3 revision history for the full story):
//! a session that blips into waiting/idle and immediately back out is
//! simply no longer in `sessions` for that state, so it stops blinking
//! right away; a session that was *already* idle before a second one joins
//! it correctly keeps blinking based on the second (recent) one even after
//! the first (stale) one leaves, in either direction (state change or the
//! session ending outright — both just remove it from `sessions`).
//!
//! Testable without any real waiting: `display()` takes the current time as
//! an explicit argument (03_plan.md Phase 6: "先にテストを書いてから実装する").
//!
//! The owner (tray.rs) is expected to call `on_snapshot()` whenever a new
//! snapshot arrives, and `display()` on every tick of its own timer to
//! decide what the icon should currently show.

use cc_semaphore_core::{SessionEntry, SessionState, StateCounts};

pub const ROTATE_INTERVAL_MS: i64 = 2_000;
pub const BLINK_INTERVAL_MS: i64 = 500;
/// How long a session stays "recent" (blink-worthy) after its `since`.
pub const ALERT_WINDOW_MS: i64 = 30_000;

/// What the icon should show right now. Rotation always shows `Normal`;
/// an active alert blinks between `Normal` and `Inverted` every
/// `BLINK_INTERVAL_MS` (icon.rs renders `Inverted` as a solid block with
/// the digit knocked out, never as a blank icon — see icon.rs's doc
/// comment for why).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Display {
    Normal(SessionState, u32),
    Inverted(SessionState, u32),
}

pub struct TrayState {
    sessions: Vec<SessionEntry>,
}

impl TrayState {
    pub fn new() -> Self {
        TrayState {
            sessions: Vec::new(),
        }
    }

    /// Feeds a freshly observed session list. No diffing against the
    /// previous snapshot is needed — see the module doc comment.
    pub fn on_snapshot(&mut self, sessions: Vec<SessionEntry>) {
        self.sessions = sessions;
    }

    /// Whether any session in `state` entered it within `ALERT_WINDOW_MS`
    /// of `now_ms`.
    fn has_recent(&self, state: SessionState, now_ms: i64) -> bool {
        self.sessions
            .iter()
            .any(|s| s.state == state && now_ms - s.since < ALERT_WINDOW_MS)
    }

    /// What to show right now, given the latest `counts` (for the digit
    /// value) and the current time (for which mode is active / rotation
    /// phase / blink phase).
    pub fn display(&self, counts: &StateCounts, now_ms: i64) -> Display {
        // waiting takes priority over idle when both have a recent session
        // (02_design.md §6.3).
        let alert = if self.has_recent(SessionState::Waiting, now_ms) {
            Some((SessionState::Waiting, counts.waiting))
        } else if self.has_recent(SessionState::Idle, now_ms) {
            Some((SessionState::Idle, counts.idle))
        } else {
            None
        };

        match alert {
            Some((state, value)) => {
                let visible = now_ms.rem_euclid(BLINK_INTERVAL_MS * 2) < BLINK_INTERVAL_MS;
                if visible {
                    Display::Normal(state, value)
                } else {
                    Display::Inverted(state, value)
                }
            }
            None => {
                let phase = now_ms.rem_euclid(ROTATE_INTERVAL_MS * 3) / ROTATE_INTERVAL_MS;
                match phase {
                    0 => Display::Normal(SessionState::Running, counts.running),
                    1 => Display::Normal(SessionState::Waiting, counts.waiting),
                    _ => Display::Normal(SessionState::Idle, counts.idle),
                }
            }
        }
    }
}

impl Default for TrayState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(running: u32, waiting: u32, idle: u32) -> StateCounts {
        StateCounts {
            running,
            waiting,
            idle,
        }
    }

    fn session(state: SessionState, since: i64) -> SessionEntry {
        SessionEntry {
            id: None,
            pid: 1,
            name: None,
            cwd: "/".to_string(),
            state,
            raw_status: "x".to_string(),
            waiting_for: None,
            since,
            started_at: since,
        }
    }

    #[test]
    fn no_sessions_rotates() {
        let t = TrayState::new();
        assert_eq!(
            t.display(&counts(1, 0, 0), 0),
            Display::Normal(SessionState::Running, 1)
        );
    }

    #[test]
    fn a_freshly_waiting_session_blinks() {
        let mut t = TrayState::new();
        t.on_snapshot(vec![session(SessionState::Waiting, 1_000)]);
        assert_eq!(
            t.display(&counts(1, 1, 0), 1_000),
            Display::Normal(SessionState::Waiting, 1)
        );
    }

    #[test]
    fn a_freshly_idle_session_blinks() {
        let mut t = TrayState::new();
        t.on_snapshot(vec![session(SessionState::Idle, 1_000)]);
        assert_eq!(
            t.display(&counts(1, 0, 1), 1_000),
            Display::Normal(SessionState::Idle, 1)
        );
    }

    #[test]
    fn a_running_session_never_blinks() {
        let mut t = TrayState::new();
        t.on_snapshot(vec![session(SessionState::Running, 1_000)]);
        assert_eq!(
            t.display(&counts(1, 0, 0), 1_000),
            Display::Normal(SessionState::Running, 1)
        );
    }

    #[test]
    fn simultaneous_waiting_and_idle_prefers_waiting() {
        let mut t = TrayState::new();
        t.on_snapshot(vec![
            session(SessionState::Idle, 1_000),
            session(SessionState::Waiting, 1_000),
        ]);
        assert_eq!(
            t.display(&counts(0, 1, 1), 1_000),
            Display::Normal(SessionState::Waiting, 1)
        );
    }

    #[test]
    fn a_resolved_session_no_longer_blinks() {
        // The original bug: a session blips into idle and immediately back
        // to running. It's simply not idle in the next snapshot anymore, so
        // there's nothing left to blink about — well before ALERT_WINDOW_MS.
        let mut t = TrayState::new();
        t.on_snapshot(vec![session(SessionState::Idle, 1_000)]);
        t.on_snapshot(vec![session(SessionState::Running, 1_000)]);
        assert_eq!(
            t.display(&counts(1, 0, 0), 1_500),
            Display::Normal(SessionState::Running, 1)
        );
    }

    #[test]
    fn a_second_session_in_the_same_state_still_blinks_once_the_first_resolves() {
        // The "1->2->1" case: idle already had a standing session (A) when
        // a second one (B) joined it. Once A resolves back to running, B
        // (still idle, and recent) correctly keeps the alert going.
        let mut t = TrayState::new();
        let a = session(SessionState::Idle, 0);
        let b = session(SessionState::Idle, 1_000);
        t.on_snapshot(vec![a.clone(), b.clone()]);
        // A resolves; B is untouched.
        t.on_snapshot(vec![b]);
        assert_eq!(
            t.display(&counts(1, 0, 1), 1_000),
            Display::Normal(SessionState::Idle, 1)
        );
    }

    #[test]
    fn an_unrelated_session_ending_does_not_silence_a_still_recent_one() {
        // The follow-up case: instead of A *resolving* (state change), A's
        // session just ends outright (its process exits, so it drops out of
        // `sessions` entirely — see snapshot.rs's `is_alive` filter). B is
        // still idle and recent, so the alert must not fall silent.
        let mut t = TrayState::new();
        let a = session(SessionState::Idle, 0);
        let b = session(SessionState::Idle, 1_000);
        t.on_snapshot(vec![a, b.clone()]);
        t.on_snapshot(vec![b]); // A's process ended; it's gone, not just changed
        assert_eq!(
            t.display(&counts(1, 0, 1), 1_000),
            Display::Normal(SessionState::Idle, 1)
        );
    }

    #[test]
    fn a_stale_session_alone_does_not_blink() {
        let mut t = TrayState::new();
        t.on_snapshot(vec![session(SessionState::Idle, 0)]);
        assert_eq!(
            t.display(&counts(1, 0, 1), ALERT_WINDOW_MS),
            Display::Normal(SessionState::Running, 1),
            "a session idle for exactly ALERT_WINDOW_MS is no longer recent"
        );
    }

    #[test]
    fn a_persistently_idle_session_stops_blinking_after_the_window_even_if_still_idle() {
        let mut t = TrayState::new();
        t.on_snapshot(vec![session(SessionState::Idle, 0)]);
        assert_eq!(
            t.display(&counts(1, 0, 1), ALERT_WINDOW_MS - 1_000),
            Display::Normal(SessionState::Idle, 1),
            "still within the window, and in the blink's visible phase"
        );
        assert_eq!(
            t.display(&counts(1, 0, 1), ALERT_WINDOW_MS),
            Display::Normal(SessionState::Running, 1),
            "past the window: back to steady rotation even though still idle"
        );
    }

    #[test]
    fn rotation_cycles_running_waiting_idle_every_2s() {
        let t = TrayState::new();
        let c = counts(1, 2, 3);
        assert_eq!(t.display(&c, 0), Display::Normal(SessionState::Running, 1));
        assert_eq!(
            t.display(&c, 1_999),
            Display::Normal(SessionState::Running, 1)
        );
        assert_eq!(
            t.display(&c, 2_000),
            Display::Normal(SessionState::Waiting, 2)
        );
        assert_eq!(t.display(&c, 4_000), Display::Normal(SessionState::Idle, 3));
        assert_eq!(
            t.display(&c, 6_000),
            Display::Normal(SessionState::Running, 1)
        );
    }

    #[test]
    fn alert_blinks_at_500ms_cadence() {
        let mut t = TrayState::new();
        t.on_snapshot(vec![session(SessionState::Waiting, 0)]);
        assert_eq!(
            t.display(&counts(0, 1, 0), 0),
            Display::Normal(SessionState::Waiting, 1)
        );
        assert_eq!(
            t.display(&counts(0, 1, 0), 499),
            Display::Normal(SessionState::Waiting, 1)
        );
        assert_eq!(
            t.display(&counts(0, 1, 0), 500),
            Display::Inverted(SessionState::Waiting, 1)
        );
        assert_eq!(
            t.display(&counts(0, 1, 0), 999),
            Display::Inverted(SessionState::Waiting, 1)
        );
        assert_eq!(
            t.display(&counts(0, 1, 0), 1_000),
            Display::Normal(SessionState::Waiting, 1)
        );
    }
}
