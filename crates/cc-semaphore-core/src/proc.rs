//! Process liveness checks against `/proc`. See 02_design.md §0.4.
//!
//! Only meaningful on Linux (native Ubuntu and WSL1); the desktop frontend
//! on Windows never calls this, it only reads the published snapshot.

/// Abstracts reading `/proc/<pid>/stat` so the liveness logic can be tested
/// without real processes.
pub trait ProcSource {
    /// Returns the raw contents of `/proc/<pid>/stat`, or `None` if the
    /// process does not exist / the file cannot be read.
    fn read_stat(&self, pid: u32) -> Option<String>;
}

#[cfg(target_os = "linux")]
pub struct RealProcSource;

#[cfg(target_os = "linux")]
impl ProcSource for RealProcSource {
    fn read_stat(&self, pid: u32) -> Option<String> {
        std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()
    }
}

/// Extracts field 22 (`starttime`, in clock ticks) from the contents of
/// `/proc/<pid>/stat`.
///
/// The `comm` field (field 2) is parenthesized and may itself contain
/// spaces or parentheses, so fields cannot be split on whitespace naively.
/// We split on the *last* `)` instead, then count fields from there:
/// state(1) ppid(2) ... starttime is the 20th field after that point.
fn extract_starttime(stat: &str) -> Option<&str> {
    let after_comm = stat.rsplit_once(')')?.1;
    after_comm.split_whitespace().nth(19)
}

/// Returns whether `pid` is alive and, when `proc_start` is available,
/// still the same process that originally wrote it (not a reused pid).
///
/// When `proc_start` is `None` (record predates or lacks the field), this
/// degrades to a plain existence check, per 02_design.md §0.4.
pub fn is_alive(source: &dyn ProcSource, pid: u32, proc_start: Option<&str>) -> bool {
    let Some(stat) = source.read_stat(pid) else {
        return false;
    };
    match proc_start {
        Some(expected) => extract_starttime(&stat).is_some_and(|actual| actual == expected),
        None => true,
    }
}

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

    fn stat_line(comm: &str, starttime: &str) -> String {
        // pid (comm) state ppid pgrp session tty_nr tpgid flags minflt cminflt
        // majflt cmajflt utime stime cutime cstime priority nice num_threads
        // itrealvalue starttime ...
        let mut fields = vec!["S", "1", "1", "1", "0", "-1", "0", "0", "0", "0"];
        fields.extend(["0", "0", "0", "0", "20", "0", "1", "0", "0"]); // ...num_threads, itrealvalue
        fields.push(starttime);
        format!("123 ({comm}) {}", fields.join(" "))
    }

    #[test]
    fn alive_when_starttime_matches() {
        let src = FakeProcSource(HashMap::from([(123, stat_line("node", "18721871"))]));
        assert!(is_alive(&src, 123, Some("18721871")));
    }

    #[test]
    fn dead_when_pid_missing_from_proc() {
        let src = FakeProcSource(HashMap::new());
        assert!(!is_alive(&src, 123, Some("18721871")));
    }

    #[test]
    fn not_alive_when_starttime_differs_pid_reused() {
        let src = FakeProcSource(HashMap::from([(123, stat_line("node", "99999999"))]));
        assert!(!is_alive(&src, 123, Some("18721871")));
    }

    #[test]
    fn degrades_to_existence_check_when_proc_start_missing() {
        let src = FakeProcSource(HashMap::from([(123, stat_line("node", "1"))]));
        assert!(is_alive(&src, 123, None));
    }

    #[test]
    fn tolerates_parens_and_spaces_in_comm() {
        let src = FakeProcSource(HashMap::from([(123, stat_line("weird (name)", "42"))]));
        assert!(is_alive(&src, 123, Some("42")));
    }

    /// Real `/proc/<pid>/stat` line captured on Ubuntu 24.04 during Phase 0
    /// (docs/measurements.md). starttime (field 22) is 18962456.
    #[test]
    fn extracts_starttime_from_real_captured_stat_line() {
        let real = "2122244 (cat) R 2122242 2122244 2122242 0 -1 4194304 95 0 0 0 0 0 0 0 \
                     20 0 1 0 18962456 8642560 433 18446744073709551615 106386784448512 \
                     106386784466097 140734864673040 0 0 0 0 0 0 0 0 0 17 3 0 0 0 0 0 \
                     106386784479888 106386784481384 106387170340864 140734864677431 \
                     140734864677451 140734864677451 140734864682987 0";
        assert_eq!(extract_starttime(real), Some("18962456"));
    }
}
