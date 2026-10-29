//! Shared contracts for Context.
//!
//! Every other crate depends on this one. It holds the provider-independent
//! message model, the normalized model response contract, configuration,
//! identifiers, source references, token estimation, and the event types the
//! runtime streams to user interfaces. Nothing here performs I/O against
//! repositories, databases, or model providers.

pub mod config;
pub mod credentials;
pub mod error;
pub mod fallback_index;
pub mod events;
pub mod ids;
pub mod message;
pub mod model;
pub mod paths;
pub mod platform;
pub mod pricing;
pub mod source;
pub mod tokens;

pub use config::*;
pub use credentials::*;
pub use error::*;
pub use events::*;
pub use ids::*;
pub use message::*;
pub use model::*;
pub use pricing::{ModelPrice, PriceSource, SpendConfig, SpendPeriod};
pub use source::*;
pub use tokens::*;

pub use tokio_util::sync::CancellationToken;

/// Categories used to size retrieval and context budgets (spec §7.11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskCategory {
    #[default]
    LocalizedEdit,
    MultiFileChange,
    ComplexFeature,
    Investigation,
}

impl TaskCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskCategory::LocalizedEdit => "localized_edit",
            TaskCategory::MultiFileChange => "multi_file_change",
            TaskCategory::ComplexFeature => "complex_feature",
            TaskCategory::Investigation => "investigation",
        }
    }
}

/// Runtime execution states (spec §10.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExecutionState {
    Idle,
    Preparing,
    Investigating,
    Implementing,
    Validating,
    Recovering,
    Completed,
    Blocked,
    Cancelled,
    Failed,
}

impl ExecutionState {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            ExecutionState::Completed
                | ExecutionState::Blocked
                | ExecutionState::Cancelled
                | ExecutionState::Failed
        )
    }

    /// Whether a transition from `self` to `next` is permitted by the state
    /// machine in spec §10.2. Terminal states never transition.
    pub fn can_transition_to(&self, next: ExecutionState) -> bool {
        use ExecutionState::*;
        if self.is_terminal() {
            return false;
        }
        match (self, next) {
            (_, Cancelled) | (_, Failed) | (_, Blocked) => true,
            (Idle, Preparing) => true,
            (Preparing, Investigating) | (Preparing, Implementing) => true,
            (Investigating, Implementing) | (Investigating, Validating) | (Investigating, Completed) => true,
            (Implementing, Validating) | (Implementing, Investigating) | (Implementing, Completed) => true,
            (Validating, Completed) | (Validating, Recovering) | (Validating, Implementing) => true,
            (Recovering, Investigating) | (Recovering, Implementing) => true,
            _ => false,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ExecutionState::Idle => "IDLE",
            ExecutionState::Preparing => "PREPARING",
            ExecutionState::Investigating => "INVESTIGATING",
            ExecutionState::Implementing => "IMPLEMENTING",
            ExecutionState::Validating => "VALIDATING",
            ExecutionState::Recovering => "RECOVERING",
            ExecutionState::Completed => "COMPLETED",
            ExecutionState::Blocked => "BLOCKED",
            ExecutionState::Cancelled => "CANCELLED",
            ExecutionState::Failed => "FAILED",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_states_do_not_transition() {
        assert!(!ExecutionState::Completed.can_transition_to(ExecutionState::Preparing));
        assert!(!ExecutionState::Failed.can_transition_to(ExecutionState::Idle));
    }

    #[test]
    fn recovery_loops_back_to_investigation() {
        assert!(ExecutionState::Validating.can_transition_to(ExecutionState::Recovering));
        assert!(ExecutionState::Recovering.can_transition_to(ExecutionState::Investigating));
        assert!(!ExecutionState::Recovering.can_transition_to(ExecutionState::Completed));
    }
}
