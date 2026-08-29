//! Atomic, multi-target, skip-if-unchanged snapshot writing.
//! See 02_design.md §2.1, §2.5.

use cc_semaphore_core::Snapshot;
use std::path::{Path, PathBuf};

pub struct Writer {
    targets: Vec<PathBuf>,
    /// The last snapshot written, serialized with `generatedAt` zeroed out,
    /// so a scan that changes nothing but the timestamp is a no-op write.
    last_comparable: Option<String>,
}

impl Writer {
    pub fn new(targets: Vec<PathBuf>) -> Self {
        Writer {
            targets,
            last_comparable: None,
        }
    }

    /// Writes `snapshot` to every configured target unless it is
    /// semantically identical to the last one written. A failure writing
    /// to one target (e.g. an unmounted `/mnt/c`) is logged and does not
    /// prevent writing to the others, nor does it crash the daemon.
    pub fn write(&mut self, snapshot: &Snapshot) {
        let comparable = comparable_json(snapshot);
        if self.last_comparable.as_deref() == Some(comparable.as_str()) {
            return;
        }
        let full_json = serde_json::to_vec_pretty(snapshot).expect("Snapshot always serializes");
        for target in &self.targets {
            if let Err(e) = write_atomic(target, &full_json) {
                eprintln!(
                    "cc-semaphored: warning: failed to write {}: {e}",
                    target.display()
                );
            }
        }
        self.last_comparable = Some(comparable);
    }
}

fn comparable_json(snapshot: &Snapshot) -> String {
    let mut value = serde_json::to_value(snapshot).expect("Snapshot always serializes");
    value["generatedAt"] = serde_json::json!(0);
    value.to_string()
}

fn write_atomic(target: &Path, contents: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
        }
    }
    let tmp = target.with_extension("json.tmp");
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cc_semaphore_core::{Snapshot, StateCounts};

    fn empty_snapshot(generated_at: i64) -> Snapshot {
        Snapshot {
            version: 1,
            generated_at,
            host: "h".into(),
            counts: StateCounts::default(),
            sessions: vec![],
        }
    }

    #[test]
    fn skips_rewrite_when_only_generated_at_changes() {
        let dir = std::env::temp_dir().join(format!("cc-semaphore-test-{}", std::process::id()));
        let target = dir.join("state.json");
        let mut writer = Writer::new(vec![target.clone()]);

        writer.write(&empty_snapshot(1));
        let first_write_time = std::fs::metadata(&target).unwrap().modified().unwrap();

        std::thread::sleep(std::time::Duration::from_millis(10));
        writer.write(&empty_snapshot(2)); // same content, different generatedAt
        let second_write_time = std::fs::metadata(&target).unwrap().modified().unwrap();

        assert_eq!(
            first_write_time, second_write_time,
            "should not have rewritten the file"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
