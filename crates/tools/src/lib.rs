//! Tools (spec §10.3, §10.4).
//!
//! Every tool declares a JSON schema, maps its arguments to a security
//! [`Action`](ctx_security::Action) for policy evaluation, and reports
//! side effects so the runtime can track modified files, validation
//! results, and the task working set without trusting the model's own
//! account of what happened.

pub mod activity;
pub mod approval;
pub mod files;
pub mod inspect;
pub mod mcp;
pub mod mentions;
pub mod registry;
pub mod schema;
pub mod search;
pub mod shell;
pub mod web;

pub use activity::{ProjectActivity, RunGuard, SessionFiles};
pub use approval::{ApprovalHandler, AutoApprove, ChannelApprovals, DenyAll, RememberingApprovals};
pub use mcp::{McpManager, McpServerState, McpServerStatus, McpToolInfo};
pub use registry::{Tool, ToolContext, ToolEffect, ToolExecution, ToolOutput, ToolRegistry};
