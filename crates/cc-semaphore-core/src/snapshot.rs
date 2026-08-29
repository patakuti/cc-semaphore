use crate::proc::{is_alive, ProcSource};
use crate::status::map_status;
use crate::types::{
    SessionEntry, SessionRecord, SessionState, Snapshot, StateCounts, SNAPSHOT_VERSION,
};

/// Parses one `<pid>.json` file's contents. Returns `None` for anything
/// that isn't valid JSON matching the expected shape; unknown fields are
/// ignored (see `SessionRecord`). Per 02_design.md §3.5, a parse failure is
/// not fatal for the caller: it just means this record is skipped and
/// picked up on the next scan (Claude Code writes the file in place,
/// non-atomically — see docs/measurements.md #5).
pub fn parse_session_record(json: &str) -> Option<SessionRecord> {
    serde_json::from_str(json).ok()
}

fn to_session_entry(record: &SessionRecord, include_kinds: &[&str]) -> Option<SessionEntry> {
    let kind = record.kind.as_deref()?;
    if !include_kinds.contains(&kind) {
        return None;
    }
    let raw_status = record.status.clone()?;
    let state = map_status(&raw_status)?;
    let cwd = record.cwd.clone()?;
    let since = record.status_updated_at?;
    let started_at = record.started_at.unwrap_or(since);

    Some(SessionEntry {
        id: record.session_id.clone(),
        pid: record.pid,
        name: record.name.clone(),
        cwd,
        state,
        raw_status,
        waiting_for: if state == SessionState::Waiting {
            record.waiting_for.clone()
        } else {
            None
        },
        since,
        started_at,
    })
}

/// Builds the published snapshot from a set of already-parsed session
/// records. Applies the filter (02_design.md §2.3), sort order (§2.4), and
/// aggregate counts.
///
/// `include_kinds` defaults to `["interactive"]` per §2.3; callers pass it
/// explicitly so tests (and future config-driven overrides) don't depend on
/// a hidden default.
pub fn build_snapshot(
    records: &[SessionRecord],
    proc: &dyn ProcSource,
    include_kinds: &[&str],
    now_ms: i64,
    host: &str,
) -> Snapshot {
    let mut sessions: Vec<SessionEntry> = records
        .iter()
        .filter(|r| is_alive(proc, r.pid, r.proc_start.as_deref()))
        .filter_map(|r| to_session_entry(r, include_kinds))
        .collect();

    sessions.sort_by(|a, b| {
        a.state
            .sort_rank()
            .cmp(&b.state.sort_rank())
            .then(a.since.cmp(&b.since))
    });

    let mut counts = StateCounts::default();
    for s in &sessions {
        match s.state {
            SessionState::Running => counts.running += 1,
            SessionState::Waiting => counts.waiting += 1,
            SessionState::Idle => counts.idle += 1,
        }
    }

    Snapshot {
        version: SNAPSHOT_VERSION,
        generated_at: now_ms,
        host: host.to_string(),
        counts,
        sessions,
    }
}

pub const DEFAULT_INCLUDE_KINDS: &[&str] = &["interactive"];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct FakeProcSource(HashMap<u32, String>);

    impl ProcSource for FakeProcSource {
        fn read_stat(&self, pid: u32) -> Option<String> {
            self.0.get(&pid).cloned()
        }
    }

    /// Builds a `/proc/<pid>/stat` line whose starttime (field 22) is
    /// `starttime`, so it matches a given record's `procStart` and
    /// `is_alive` treats it as alive. Field layout mirrors `proc.rs`'s own
    /// test helper.
    fn stat_with_starttime(starttime: &str) -> String {
        let mut fields = vec!["S", "1", "1", "1", "0", "-1", "0", "0", "0", "0"];
        fields.extend(["0", "0", "0", "0", "20", "0", "1", "0", "0"]);
        fields.push(starttime);
        format!("1 (x) {}", fields.join(" "))
    }

    #[test]
    fn parses_real_captured_interactive_record() {
        let json = include_str!("../tests/fixtures/session_running_interactive.json");
        let record = parse_session_record(json).expect("valid fixture must parse");
        assert_eq!(record.pid, 2071740);
        assert_eq!(record.status.as_deref(), Some("busy"));
        assert_eq!(record.kind.as_deref(), Some("interactive"));
        assert_eq!(record.proc_start.as_deref(), Some("18721871"));
    }

    #[test]
    fn parses_real_captured_bg_record() {
        let json = include_str!("../tests/fixtures/session_bg_idle.json");
        let record = parse_session_record(json).expect("valid fixture must parse");
        assert_eq!(record.pid, 2114799);
        assert_eq!(record.kind.as_deref(), Some("bg"));
    }

    #[test]
    fn build_snapshot_excludes_bg_kind_by_default() {
        let json = include_str!("../tests/fixtures/session_bg_idle.json");
        let record = parse_session_record(json).unwrap();
        let proc = FakeProcSource(HashMap::from([(
            record.pid,
            stat_with_starttime(record.proc_start.as_deref().unwrap()),
        )]));
        let snap = build_snapshot(&[record], &proc, DEFAULT_INCLUDE_KINDS, 0, "test-host");
        assert!(snap.sessions.is_empty());
    }

    #[test]
    fn build_snapshot_includes_interactive_running_session() {
        let json = include_str!("../tests/fixtures/session_running_interactive.json");
        let record = parse_session_record(json).unwrap();
        let proc = FakeProcSource(HashMap::from([(
            record.pid,
            stat_with_starttime(record.proc_start.as_deref().unwrap()),
        )]));
        let snap = build_snapshot(&[record], &proc, DEFAULT_INCLUDE_KINDS, 0, "test-host");
        assert_eq!(snap.sessions.len(), 1);
        assert_eq!(snap.sessions[0].state, SessionState::Running);
        assert_eq!(snap.counts.running, 1);
        assert_eq!(snap.counts.waiting, 0);
        assert_eq!(snap.counts.idle, 0);
    }

    #[test]
    fn build_snapshot_excludes_dead_process() {
        let json = include_str!("../tests/fixtures/session_running_interactive.json");
        let record = parse_session_record(json).unwrap();
        let proc = FakeProcSource(HashMap::new()); // no pid registered => dead
        let snap = build_snapshot(&[record], &proc, DEFAULT_INCLUDE_KINDS, 0, "test-host");
        assert!(snap.sessions.is_empty());
    }

    #[test]
    fn build_snapshot_excludes_unknown_status_instead_of_defaulting() {
        let json = include_str!("../tests/fixtures/session_unknown_status.json");
        let record = parse_session_record(json).unwrap();
        let proc = FakeProcSource(HashMap::from([(
            record.pid,
            stat_with_starttime(record.proc_start.as_deref().unwrap()),
        )]));
        let snap = build_snapshot(&[record], &proc, DEFAULT_INCLUDE_KINDS, 0, "test-host");
        assert!(snap.sessions.is_empty());
    }

    #[test]
    fn sort_order_is_waiting_then_idle_then_running_by_since_ascending() {
        let mk = |pid: u32, status: &str, since: i64| SessionRecord {
            pid,
            session_id: None,
            cwd: Some("/tmp".into()),
            started_at: Some(since),
            proc_start: None,
            kind: Some("interactive".into()),
            name: None,
            status: Some(status.into()),
            status_updated_at: Some(since),
            waiting_for: None,
        };
        let records = vec![
            mk(1, "busy", 300),
            mk(2, "idle", 200),
            mk(3, "waiting", 400),
            mk(4, "waiting", 100),
        ];
        let proc = FakeProcSource(
            records
                .iter()
                .map(|r| (r.pid, stat_with_starttime("1")))
                .collect::<HashMap<_, _>>(),
        );
        let snap = build_snapshot(&records, &proc, DEFAULT_INCLUDE_KINDS, 0, "h");
        let pids: Vec<u32> = snap.sessions.iter().map(|s| s.pid).collect();
        assert_eq!(pids, vec![4, 3, 2, 1]); // waiting(earliest first), then idle, then running
    }

    #[test]
    fn waiting_for_is_only_populated_for_waiting_state() {
        let mk = |pid: u32, status: &str| SessionRecord {
            pid,
            session_id: None,
            cwd: Some("/tmp".into()),
            started_at: Some(1),
            proc_start: None,
            kind: Some("interactive".into()),
            name: None,
            status: Some(status.into()),
            status_updated_at: Some(1),
            waiting_for: Some("input needed".into()),
        };
        let records = vec![mk(1, "waiting"), mk(2, "busy")];
        let proc = FakeProcSource(HashMap::from([
            (1, stat_with_starttime("1")),
            (2, stat_with_starttime("1")),
        ]));
        let snap = build_snapshot(&records, &proc, DEFAULT_INCLUDE_KINDS, 0, "h");
        let waiting_entry = snap.sessions.iter().find(|s| s.pid == 1).unwrap();
        let busy_entry = snap.sessions.iter().find(|s| s.pid == 2).unwrap();
        assert_eq!(waiting_entry.waiting_for.as_deref(), Some("input needed"));
        assert_eq!(busy_entry.waiting_for, None);
    }
}
