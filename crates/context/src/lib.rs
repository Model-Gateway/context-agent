//! Context compiler (spec §8).
//!
//! Builds each model invocation from logical layers — stable policy and
//! tool definitions, dynamic trusted instructions, repository context,
//! memory context, the active conversation, and the latest request — with
//! a token budget, retention classes, safe compaction into task checkpoints,
//! and full inspection output.

pub mod budget;
pub mod compaction;
pub mod compiler;
pub mod explain;
pub mod policy;
pub mod retention;

pub use budget::ContextBudget;
pub use compaction::{compact, TaskCheckpoint};
pub use compiler::{CompileInput, CompiledContext, ContextCompiler};
pub use explain::{ContextExplanation, ExcludedExplanation, MemoryExplanation, SourceExplanation};
pub use policy::{SYSTEM_POLICY, policy_hash};
pub use retention::{prune_conversation, PruneStats, RetentionClass};
