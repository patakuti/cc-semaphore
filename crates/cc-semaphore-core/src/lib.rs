//! Shared types and logic for cc-semaphore: parsing Claude Code's session
//! files, mapping raw status to the three-state model, process liveness
//! checks, and building the published snapshot.
//!
//! See `02_design.md` in the project root for the full design rationale.

pub mod colors;
pub mod format;
pub mod proc;
pub mod snapshot;
pub mod status;
pub mod types;

pub use proc::{is_alive, ProcSource};
pub use snapshot::{build_snapshot, parse_session_record, DEFAULT_INCLUDE_KINDS};
pub use status::map_status;
pub use types::{SessionEntry, SessionRecord, SessionState, Snapshot, StateCounts};
