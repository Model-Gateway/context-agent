//! Autonomous agent runtime (spec §10).
//!
//! [`RepositoryContext`] wires storage, indexing, retrieval, memory, policy
//! and telemetry for one repository. [`SessionManager`] resolves each user
//! message to a task (continue / refine / new / earlier / reverse) and keeps
//! the persisted conversation. [`AgentRuntime`] runs the bounded
//! plan–implement–validate–recover loop with streaming events, compaction,
//! automatic validation and an honest completion report. Under the Ultra
//! effort tier the model may deploy bounded sub-agents (`agents`).

pub mod agents;
pub mod inbox;
pub mod repository;
pub mod runtime;
pub mod session;
pub mod validation;

pub use inbox::Inbox;
pub use repository::RepositoryContext;
pub use runtime::{AgentRuntime, RunOptions, DISCOUNT_DROP_APPROVAL};
pub use session::{SessionManager, TaskResolution};
pub use validation::validation_commands;

/// Fingerprint of the stable system policy in use (for reports).
pub fn policy_hash() -> String {
    ctx_context::policy_hash()
}
