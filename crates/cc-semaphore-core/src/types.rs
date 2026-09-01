use serde::{Deserialize, Serialize};

/// Raw record as written by Claude Code to `~/.claude/sessions/<pid>.json`.
///
/// Unknown fields are ignored on purpose: this file is owned by Claude Code,
/// not by cc-semaphore, and new fields may appear in future CLI versions.
#[derive(Debug, Clone, Deserialize)]
pub struct SessionRecord {
    pub pid: u32,
    #[serde(rename = "sessionId")]
    pub session_id: Option<String>,
    pub cwd: Option<String>,
    #[serde(rename = "startedAt")]
    pub started_at: Option<i64>,
    #[serde(rename = "procStart")]
    pub proc_start: Option<String>,
    pub kind: Option<String>,
    pub name: Option<String>,
    pub status: Option<String>,
    #[serde(rename = "statusUpdatedAt")]
    pub status_updated_at: Option<i64>,
    #[serde(rename = "waitingFor")]
    pub waiting_for: Option<String>,
}

/// The three states cc-semaphore distinguishes. See 01_requirements.md §3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionState {
    Running,
    Waiting,
    Idle,
}

impl SessionState {
    /// Sort key: sessions that need attention sort first.
    /// waiting(0) < idle(1) < running(2), per 02_design.md §2.4.
    pub fn sort_rank(self) -> u8 {
        match self {
            SessionState::Waiting => 0,
            SessionState::Idle => 1,
            SessionState::Running => 2,
        }
    }
}

/// One session as it appears in the published snapshot. See 02_design.md §2.2.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEntry {
    pub id: Option<String>,
    pub pid: u32,
    pub name: Option<String>,
    pub cwd: String,
    pub state: SessionState,
    #[serde(rename = "rawStatus")]
    pub raw_status: String,
    #[serde(rename = "waitingFor", skip_serializing_if = "Option::is_none")]
    pub waiting_for: Option<String>,
    /// `statusUpdatedAt` from the raw record, epoch ms. Elapsed time is
    /// derived from this at render time (02_design.md §0.5).
    pub since: i64,
    #[serde(rename = "startedAt")]
    pub started_at: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StateCounts {
    pub running: u32,
    pub waiting: u32,
    pub idle: u32,
}

/// The backend↔frontend contract file. See 02_design.md §2.2.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub version: u32,
    #[serde(rename = "generatedAt")]
    pub generated_at: i64,
    pub host: String,
    pub counts: StateCounts,
    pub sessions: Vec<SessionEntry>,
}

pub const SNAPSHOT_VERSION: u32 = 1;
