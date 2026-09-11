//! Session state-transition triggers (`triggers.json`). See 02_design.md §3.10.

use cc_semaphore_core::{SessionEntry, SessionState};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Instant;

const DEFAULT_DEBOUNCE_MS: u64 = 2000;

#[derive(Debug, Clone, Deserialize)]
pub struct Action {
    #[serde(rename = "type")]
    pub kind: String,
    pub command: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct RawTriggers {
    #[serde(rename = "onWaiting")]
    on_waiting: Vec<Action>,
    #[serde(rename = "onIdle")]
    on_idle: Vec<Action>,
    #[serde(rename = "onRunning")]
    on_running: Vec<Action>,
    #[serde(rename = "debounceMs")]
    debounce_ms: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct Triggers {
    on_waiting: Vec<Action>,
    on_idle: Vec<Action>,
    on_running: Vec<Action>,
    debounce_ms: u64,
}

impl Triggers {
    fn disabled() -> Self {
        Triggers {
            on_waiting: vec![],
            on_idle: vec![],
            on_running: vec![],
            debounce_ms: DEFAULT_DEBOUNCE_MS,
        }
    }

    fn actions_for(&self, state: SessionState) -> &[Action] {
        match state {
            SessionState::Waiting => &self.on_waiting,
            SessionState::Idle => &self.on_idle,
            SessionState::Running => &self.on_running,
        }
    }
}

pub fn triggers_path() -> PathBuf {
    crate::env::home_dir().join(".config/cc-semaphore/triggers.json")
}

/// Loads `triggers.json` if present. A missing file means "triggers
/// disabled" (empty action lists) — same treatment as `config::load()`
/// for a missing `config.json`. An unparseable file is reported and also
/// treated as disabled, rather than preventing the daemon from starting.
pub fn load() -> Triggers {
    let path = triggers_path();
    let Ok(contents) = std::fs::read_to_string(&path) else {
        eprintln!(
            "cc-semaphored: trigger: {} not found; triggers disabled",
            path.display()
        );
        return Triggers::disabled();
    };
    match serde_json::from_str::<RawTriggers>(&contents) {
        Ok(raw) => {
            let triggers = Triggers {
                on_waiting: raw.on_waiting,
                on_idle: raw.on_idle,
                on_running: raw.on_running,
                debounce_ms: raw.debounce_ms.unwrap_or(DEFAULT_DEBOUNCE_MS),
            };
            eprintln!(
                "cc-semaphored: trigger: loaded {} (onWaiting={}, onIdle={}, onRunning={}, debounceMs={})",
                path.display(),
                triggers.on_waiting.len(),
                triggers.on_idle.len(),
                triggers.on_running.len(),
                triggers.debounce_ms,
            );
            triggers
        }
        Err(e) => {
            eprintln!(
                "cc-semaphored: warning: {} is not valid JSON ({e}); triggers disabled",
                path.display()
            );
            Triggers::disabled()
        }
    }
}

struct Tracked {
    last_fired: SessionState,
    pending: Option<(SessionState, Instant)>,
}

/// Detects per-session state transitions across scans and fires the
/// matching trigger actions once a transition has held stable for
/// `debounceMs`. See 02_design.md §3.10.2 for the algorithm this
/// implements.
pub struct Engine {
    triggers: Triggers,
    tracked: HashMap<u32, Tracked>,
}

impl Engine {
    pub fn new(triggers: Triggers) -> Self {
        Engine {
            triggers,
            tracked: HashMap::new(),
        }
    }

    /// Feeds one scan's sessions through the transition detector, running
    /// (asynchronously) any actions whose transition just stabilized.
    pub fn observe(&mut self, sessions: &[SessionEntry], now: Instant) {
        let mut seen = HashSet::with_capacity(sessions.len());
        for session in sessions {
            seen.insert(session.pid);
            match self.tracked.get_mut(&session.pid) {
                None => {
                    // First observation of this PID: nothing to transition
                    // from, so this does not count as a transition (avoids
                    // a notification storm for sessions already in flight
                    // when the daemon starts).
                    eprintln!(
                        "cc-semaphored: trigger: pid={} first observed in state {:?} (baseline, not a transition)",
                        session.pid, session.state
                    );
                    self.tracked.insert(
                        session.pid,
                        Tracked {
                            last_fired: session.state,
                            pending: None,
                        },
                    );
                }
                Some(entry) => Self::advance(entry, &self.triggers, session, now),
            }
        }
        self.tracked.retain(|pid, _| seen.contains(pid));
    }

    fn advance(entry: &mut Tracked, triggers: &Triggers, session: &SessionEntry, now: Instant) {
        if session.state == entry.last_fired {
            if entry.pending.is_some() {
                eprintln!(
                    "cc-semaphored: trigger: pid={} reverted to {:?} before the pending transition stabilized; cancelled",
                    session.pid, session.state
                );
            }
            entry.pending = None; // flipped back before it ever fired
            return;
        }
        let already_pending_this_state =
            matches!(entry.pending, Some((s, _)) if s == session.state);
        if !already_pending_this_state {
            eprintln!(
                "cc-semaphored: trigger: pid={} {:?} -> {:?} observed; waiting {}ms for it to stabilize",
                session.pid, entry.last_fired, session.state, triggers.debounce_ms
            );
            entry.pending = Some((session.state, now));
            return;
        }
        let Some((_, since)) = entry.pending else {
            return;
        };
        let elapsed_ms = now.duration_since(since).as_millis() as u64;
        if elapsed_ms >= triggers.debounce_ms {
            let actions = triggers.actions_for(session.state);
            eprintln!(
                "cc-semaphored: trigger: pid={} {:?} -> {:?} stabilized after {}ms; firing {} action(s)",
                session.pid,
                entry.last_fired,
                session.state,
                elapsed_ms,
                actions.len()
            );
            run_all(actions, session);
            entry.last_fired = session.state;
            entry.pending = None;
        }
    }
}

fn run_all(actions: &[Action], session: &SessionEntry) {
    if actions.is_empty() {
        eprintln!(
            "cc-semaphored: trigger: pid={} state={:?}: 0 actions configured for this event (check triggers.json)",
            session.pid, session.state
        );
    }
    for action in actions {
        if action.kind != "command" {
            // v1 only supports "command" (01_requirements.md §4.6);
            // unknown types are ignored rather than treated as an error.
            eprintln!(
                "cc-semaphored: warning: trigger action type {:?} is not supported (only \"command\" is); skipping",
                action.kind
            );
            continue;
        }
        run_command(substitute(&action.command, session));
    }
}

fn state_name(state: SessionState) -> &'static str {
    match state {
        SessionState::Running => "running",
        SessionState::Waiting => "waiting",
        SessionState::Idle => "idle",
    }
}

/// Wraps `value` in single quotes for safe interpolation into a `sh -c`
/// string, escaping any embedded single quotes.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn substitute(template: &str, session: &SessionEntry) -> String {
    template
        .replace(
            "{{name}}",
            &shell_quote(session.name.as_deref().unwrap_or("")),
        )
        .replace("{{cwd}}", &shell_quote(&session.cwd))
        .replace("{{pid}}", &session.pid.to_string())
        .replace("{{state}}", state_name(session.state))
        .replace(
            "{{waitingFor}}",
            &shell_quote(session.waiting_for.as_deref().unwrap_or("")),
        )
}

/// Runs `command` via `/bin/sh -c` on a dedicated thread that also waits
/// on the child, so the caller (the scan loop) is never blocked and the
/// child never becomes a zombie (02_design.md §3.10.3).
fn run_command(command: String) {
    eprintln!("cc-semaphored: trigger: running: {command}");
    std::thread::spawn(move || {
        match Command::new("/bin/sh")
            .arg("-c")
            .arg(&command)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(mut child) => match child.wait() {
                Ok(status) if status.success() => {
                    eprintln!("cc-semaphored: trigger: command exited successfully: {command}");
                }
                Ok(status) => {
                    eprintln!(
                        "cc-semaphored: warning: trigger command exited with {status}: {command}"
                    );
                }
                Err(e) => {
                    eprintln!("cc-semaphored: warning: failed to wait on trigger command: {e}");
                }
            },
            Err(e) => {
                eprintln!(
                    "cc-semaphored: warning: failed to spawn trigger command ({e}): {command}"
                );
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn session(pid: u32, state: SessionState) -> SessionEntry {
        SessionEntry {
            id: None,
            pid,
            name: Some("test".to_string()),
            cwd: "/tmp".to_string(),
            state,
            raw_status: "x".to_string(),
            waiting_for: None,
            since: 0,
            started_at: 0,
        }
    }

    fn triggers_with_debounce(debounce_ms: u64) -> Triggers {
        Triggers {
            on_waiting: vec![Action {
                kind: "command".to_string(),
                command: "true".to_string(),
            }],
            on_idle: vec![],
            on_running: vec![],
            debounce_ms,
        }
    }

    fn fired_states(engine: &Engine) -> HashMap<u32, SessionState> {
        engine
            .tracked
            .iter()
            .map(|(pid, t)| (*pid, t.last_fired))
            .collect()
    }

    #[test]
    fn first_observation_does_not_fire() {
        let mut engine = Engine::new(triggers_with_debounce(0));
        engine.observe(&[session(1, SessionState::Waiting)], Instant::now());
        // last_fired is seeded to the observed state without going through
        // `advance`, so nothing was "fired" — verified indirectly: a second
        // observation with the same state does not re-fire (no pending set).
        assert_eq!(fired_states(&engine)[&1], SessionState::Waiting);
    }

    #[test]
    fn does_not_fire_before_debounce_elapses() {
        let mut engine = Engine::new(triggers_with_debounce(1_000_000));
        let t0 = Instant::now();
        engine.observe(&[session(1, SessionState::Running)], t0);
        engine.observe(
            &[session(1, SessionState::Waiting)],
            t0 + Duration::from_millis(1),
        );
        // Debounce (1_000_000ms) has not elapsed: still tracked as running.
        assert_eq!(fired_states(&engine)[&1], SessionState::Running);
    }

    #[test]
    fn fires_after_debounce_elapses() {
        let mut engine = Engine::new(triggers_with_debounce(10));
        let t0 = Instant::now();
        engine.observe(&[session(1, SessionState::Running)], t0);
        engine.observe(
            &[session(1, SessionState::Waiting)],
            t0 + Duration::from_millis(1),
        );
        engine.observe(
            &[session(1, SessionState::Waiting)],
            t0 + Duration::from_millis(20),
        );
        assert_eq!(fired_states(&engine)[&1], SessionState::Waiting);
    }

    #[test]
    fn flip_flop_cancels_pending_transition() {
        let mut engine = Engine::new(triggers_with_debounce(10));
        let t0 = Instant::now();
        engine.observe(&[session(1, SessionState::Waiting)], t0);
        engine.observe(
            &[session(1, SessionState::Running)],
            t0 + Duration::from_millis(1),
        );
        // Flips back before the debounce for `running` elapses.
        engine.observe(
            &[session(1, SessionState::Waiting)],
            t0 + Duration::from_millis(2),
        );
        engine.observe(
            &[session(1, SessionState::Waiting)],
            t0 + Duration::from_millis(50),
        );
        // Never left `waiting`, so it never re-fired for `waiting` either.
        assert_eq!(fired_states(&engine)[&1], SessionState::Waiting);
    }

    #[test]
    fn dropped_pid_is_untracked() {
        let mut engine = Engine::new(triggers_with_debounce(0));
        let t0 = Instant::now();
        engine.observe(&[session(1, SessionState::Running)], t0);
        assert!(engine.tracked.contains_key(&1));
        engine.observe(&[], t0);
        assert!(!engine.tracked.contains_key(&1));
    }

    #[test]
    fn substitute_replaces_placeholders_and_escapes_quotes() {
        let mut s = session(1, SessionState::Waiting);
        s.name = Some("it's a test".to_string());
        s.cwd = "/home/x".to_string();
        s.waiting_for = Some("permission".to_string());
        let out = substitute("echo {{name}} {{cwd}} {{pid}} {{state}} {{waitingFor}}", &s);
        assert_eq!(
            out,
            "echo 'it'\\''s a test' '/home/x' 1 waiting 'permission'"
        );
    }
}
