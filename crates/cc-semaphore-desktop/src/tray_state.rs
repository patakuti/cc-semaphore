//! The tray icon's rotation/interrupt-blink state machine (02_design.md
//! §6.3). A pure state machine that takes the current time as an explicit
//! argument rather than owning a timer, so it can be tested without any
//! real waiting (03_plan.md Phase 6: "先にテストを書いてから実装する").
//!
//! The owner (tray.rs) is expected to call `on_counts()` whenever a new
//! snapshot arrives, and `display()` (after `expire()`) on every tick of
//! its own timer to decide what the icon should currently show.

use cc_semaphore_core::{SessionState, StateCounts};

pub const ROTATE_INTERVAL_MS: i64 = 2_000;
pub const BLINK_INTERVAL_MS: i64 = 500;
pub const ALERT_DURATION_MS: i64 = 30_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AlertKind {
    Waiting,
    Idle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Rotate,
    Alert { kind: AlertKind, deadline_ms: i64 },
}

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
    prev_counts: Option<StateCounts>,
    mode: Mode,
}

impl TrayState {
    pub fn new() -> Self {
        TrayState {
            prev_counts: None,
            mode: Mode::Rotate,
        }
    }

    /// Feeds a newly observed `counts`. The very first call only
    /// establishes a baseline and never fires an alert, so a fresh app
    /// launch doesn't blink over sessions that already existed
    /// (02_design.md §6.3, "起動直後の誤爆防止").
    pub fn on_counts(&mut self, counts: StateCounts, now_ms: i64) {
        if let Some(prev) = self.prev_counts.clone() {
            // idle checked before waiting: if both increase in the same
            // snapshot, waiting (checked/fired last) wins, per §6.3.
            if counts.idle > prev.idle {
                self.fire(AlertKind::Idle, now_ms);
            }
            if counts.waiting > prev.waiting {
                self.fire(AlertKind::Waiting, now_ms);
            }
        }
        self.prev_counts = Some(counts);
    }

    /// Firing always sets a fresh 30s deadline for `kind`, regardless of
    /// whether an alert was already active: extending the same kind and
    /// switching to a different kind both reduce to "adopt this kind, timer
    /// resets to now+30s" (02_design.md §6.3).
    fn fire(&mut self, kind: AlertKind, now_ms: i64) {
        self.mode = Mode::Alert {
            kind,
            deadline_ms: now_ms + ALERT_DURATION_MS,
        };
    }

    /// Reverts an expired alert back to rotation. Must be called with the
    /// current time before `display()` for that to reflect expiry.
    pub fn expire(&mut self, now_ms: i64) {
        if let Mode::Alert { deadline_ms, .. } = self.mode {
            if now_ms >= deadline_ms {
                self.mode = Mode::Rotate;
            }
        }
    }

    /// What to show right now, given the latest `counts` (for the digit
    /// value) and the current time (for rotation phase / blink phase).
    pub fn display(&self, counts: &StateCounts, now_ms: i64) -> Display {
        match self.mode {
            Mode::Rotate => {
                let phase = now_ms.rem_euclid(ROTATE_INTERVAL_MS * 3) / ROTATE_INTERVAL_MS;
                match phase {
                    0 => Display::Normal(SessionState::Running, counts.running),
                    1 => Display::Normal(SessionState::Waiting, counts.waiting),
                    _ => Display::Normal(SessionState::Idle, counts.idle),
                }
            }
            Mode::Alert { kind, .. } => {
                let (state, value) = match kind {
                    AlertKind::Waiting => (SessionState::Waiting, counts.waiting),
                    AlertKind::Idle => (SessionState::Idle, counts.idle),
                };
                let visible = now_ms.rem_euclid(BLINK_INTERVAL_MS * 2) < BLINK_INTERVAL_MS;
                if visible {
                    Display::Normal(state, value)
                } else {
                    Display::Inverted(state, value)
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

    #[test]
    fn first_snapshot_is_a_baseline_and_never_alerts() {
        let mut t = TrayState::new();
        t.on_counts(counts(0, 3, 3), 0);
        assert_eq!(t.mode, Mode::Rotate);
    }

    #[test]
    fn waiting_increase_fires_an_alert() {
        let mut t = TrayState::new();
        t.on_counts(counts(1, 0, 0), 0);
        t.on_counts(counts(1, 1, 0), 1_000);
        assert_eq!(
            t.display(&counts(1, 1, 0), 1_000),
            Display::Normal(SessionState::Waiting, 1)
        );
    }

    #[test]
    fn idle_increase_fires_an_alert() {
        let mut t = TrayState::new();
        t.on_counts(counts(1, 0, 0), 0);
        t.on_counts(counts(1, 0, 1), 1_000);
        assert_eq!(
            t.display(&counts(1, 0, 1), 1_000),
            Display::Normal(SessionState::Idle, 1)
        );
    }

    #[test]
    fn running_increase_does_not_fire() {
        let mut t = TrayState::new();
        t.on_counts(counts(1, 0, 0), 0);
        t.on_counts(counts(2, 0, 0), 1_000);
        assert_eq!(t.mode, Mode::Rotate);
    }

    #[test]
    fn simultaneous_waiting_and_idle_increase_prefers_waiting() {
        let mut t = TrayState::new();
        t.on_counts(counts(0, 0, 0), 0);
        t.on_counts(counts(0, 1, 1), 1_000);
        assert_eq!(
            t.display(&counts(0, 1, 1), 1_000),
            Display::Normal(SessionState::Waiting, 1)
        );
    }

    #[test]
    fn same_kind_increase_extends_the_deadline() {
        let mut t = TrayState::new();
        t.on_counts(counts(0, 0, 0), 0);
        t.on_counts(counts(0, 1, 0), 1_000); // deadline would be 31_000
        t.on_counts(counts(0, 2, 0), 20_000); // extends to 50_000
        t.expire(31_000); // the original deadline: must NOT have expired
        assert_eq!(
            t.display(&counts(0, 2, 0), 31_000),
            Display::Normal(SessionState::Waiting, 2),
            "extension should keep the alert active past the original deadline"
        );
    }

    #[test]
    fn different_kind_increase_switches_over() {
        let mut t = TrayState::new();
        t.on_counts(counts(0, 0, 0), 0);
        t.on_counts(counts(0, 0, 1), 1_000); // idle alert, deadline 31_000
        t.on_counts(counts(0, 1, 1), 2_000); // waiting increases -> switch over
        assert_eq!(
            t.display(&counts(0, 1, 1), 2_000),
            Display::Normal(SessionState::Waiting, 1)
        );
    }

    #[test]
    fn alert_reverts_to_rotation_after_its_deadline() {
        let mut t = TrayState::new();
        t.on_counts(counts(1, 0, 0), 0);
        t.on_counts(counts(1, 1, 0), 1_000); // deadline 31_000
        t.expire(30_999);
        assert_eq!(
            t.mode,
            Mode::Alert {
                kind: AlertKind::Waiting,
                deadline_ms: 31_000
            }
        );
        t.expire(31_000);
        assert_eq!(t.mode, Mode::Rotate);
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
        t.on_counts(counts(0, 0, 0), 0);
        t.on_counts(counts(0, 1, 0), 0);
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
