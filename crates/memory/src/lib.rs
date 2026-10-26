//! Persistent memory (spec §9).
//!
//! Memory lives outside the model conversation in four categories: task
//! memory, project knowledge, episodic memory, and execution history (the
//! event log, owned by `ctx-telemetry`). Every record carries evidence and a
//! verification status; unverified hypotheses are never promoted to trusted
//! project facts automatically.

pub mod boundary;
pub mod keywords;
pub mod store;
pub mod taskstate;
pub mod types;

pub use boundary::{classify_boundary, BoundaryDecision, TaskBoundary, TaskSummary};
pub use keywords::extract_keywords;
pub use store::MemoryStore;
pub use taskstate::{PlanStep, PlanStepStatus, TaskState};
pub use types::*;
