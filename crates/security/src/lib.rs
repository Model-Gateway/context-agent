//! Security, permissions, and sandboxing (spec §11).
//!
//! * [`PathPolicy`] resolves and validates filesystem paths against allowed
//!   workspace roots and credential-file patterns.
//! * [`CommandClassifier`] categorizes shell commands by risk.
//! * [`PolicyEngine`] combines a permission profile with the above to decide
//!   whether an action is allowed, denied, or needs approval.
//! * [`Redactor`] removes credential material from text before it is
//!   logged, stored, or displayed.
//! * [`SandboxStatus`] describes the developer-mode command wrapper, if any.

pub mod commands;
pub mod paths;
pub mod policy;
pub mod redact;
pub mod sandbox;

pub use commands::{CommandClass, CommandClassifier};
pub use paths::PathPolicy;
pub use policy::{Action, ActionKind, Decision, PolicyEngine};
pub use redact::Redactor;
pub use sandbox::{SandboxStatus, WrappedCommand};
