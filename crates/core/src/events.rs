//! Events streamed from the runtime to user interfaces and the event log.

use crate::ids::{CheckpointId, TaskId};
use crate::model::TokenUsage;
use crate::ExecutionState;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    ReadOnly,
    Write,
    Execute,
    Network,
    Destructive,
}

impl RiskLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            RiskLevel::ReadOnly => "read_only",
            RiskLevel::Write => "write",
            RiskLevel::Execute => "execute",
            RiskLevel::Network => "network",
            RiskLevel::Destructive => "destructive",
        }
    }
}

/// A request for the user to approve an action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: String,
    pub tool: String,
    pub risk: RiskLevel,
    /// One-line summary, e.g. `run: cargo test`.
    pub summary: String,
    /// Full details (command text, patch preview).
    pub details: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Approve,
    Deny,
    /// Approve this and identical future actions in the session.
    ApproveForSession,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileChangeKind {
    Created,
    Modified,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationRecord {
    pub command: String,
    pub passed: bool,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub summary: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Completed,
    Blocked,
    Failed,
    Cancelled,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Completed => "completed",
            TaskStatus::Blocked => "blocked",
            TaskStatus::Failed => "failed",
            TaskStatus::Cancelled => "cancelled",
        }
    }
}

/// Final report for a task (spec §10.7): only claims validation passed when
/// a tool reported it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskOutcome {
    pub task_id: TaskId,
    pub status: TaskStatus,
    pub summary: String,
    pub modified_files: Vec<String>,
    pub validation: Vec<ValidationRecord>,
    pub unresolved: Vec<String>,
    pub steps: u32,
    pub model_calls: u32,
    pub tool_calls: u32,
    pub usage: TokenUsage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    pub duration_ms: u64,
}

/// What a sub-agent is allowed to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AgentMode {
    /// Read, search and inspect only: no edits, no shell.
    #[default]
    Explore,
    /// The full tool set under the parent's permission profile.
    Implement,
}

impl AgentMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentMode::Explore => "explore",
            AgentMode::Implement => "implement",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "explore" | "read" | "investigate" | "research" => Some(AgentMode::Explore),
            "implement" | "edit" | "write" | "build" => Some(AgentMode::Implement),
            _ => None,
        }
    }
}

/// The report of one finished sub-agent (spec §10.8).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentReport {
    pub agent_id: String,
    pub name: String,
    pub mode: AgentMode,
    pub status: TaskStatus,
    /// The agent's final message.
    pub summary: String,
    pub modified_files: Vec<String>,
    pub unresolved: Vec<String>,
    pub steps: u32,
    pub tool_calls: u32,
    pub usage: TokenUsage,
    /// USD; `None` when no request had a price.
    #[serde(default)]
    pub cost_usd: Option<f64>,
    pub duration_ms: u64,
}

/// Summary statistics about a compiled context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ContextStats {
    pub estimated_prompt_tokens: usize,
    pub stable_prefix_tokens: usize,
    pub repository_snippets: usize,
    pub repository_tokens: usize,
    pub memory_records: usize,
    pub memory_tokens: usize,
    pub conversation_messages: usize,
    pub conversation_tokens: usize,
    pub retrieval_budget_tokens: usize,
    pub context_window: usize,
    pub compacted: bool,
}

/// One message of a model request as `AgentEvent::PromptSent` shows it:
/// `kind` is the message's role in the context (`policy`,
/// `task_instructions`, `context`, `user`, `assistant`, `tool_result`) and
/// `text` everything the model reads of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptMessage {
    pub kind: String,
    pub text: String,
    #[serde(default)]
    pub estimated_tokens: usize,
    /// Images sent with a user message (their bytes are left out).
    #[serde(default)]
    pub images: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum AgentEvent {
    TaskStarted { task_id: TaskId, objective: String, continued: bool },
    StateChanged { task_id: TaskId, from: ExecutionState, to: ExecutionState },
    ContextCompiled { task_id: TaskId, stats: ContextStats },
    ModelRequestStarted { task_id: TaskId, step: u32, estimated_prompt_tokens: usize },
    /// The full prompt of the run's first model request, with the names of
    /// the tools offered (`RunOptions::debug_prompt`). Live only.
    PromptSent { task_id: TaskId, step: u32, messages: Vec<PromptMessage>, tools: Vec<String> },
    ModelTextDelta { task_id: TaskId, text: String },
    ModelReasoningDelta { task_id: TaskId, text: String },
    ModelResponse { task_id: TaskId, text: Option<String>, tool_call_count: usize, usage: Option<TokenUsage> },
    ToolCallStarted { task_id: TaskId, call_id: String, name: String, arguments: serde_json::Value },
    ToolCallFinished { task_id: TaskId, call_id: String, name: String, success: bool, summary: String, duration_ms: u64 },
    /// The (truncated, redacted) text the model saw for a finished call.
    /// Live only: the raw `tool_output` row is the persisted copy.
    ToolCallOutput { task_id: TaskId, call_id: String, name: String, content: String, full_len: usize, is_error: bool },
    ApprovalRequested { task_id: TaskId, request: ApprovalRequest },
    ApprovalResolved { task_id: TaskId, request_id: String, decision: ApprovalDecision },
    ValidationStarted { task_id: TaskId, command: String },
    ValidationFinished { task_id: TaskId, record: ValidationRecord },
    FileChanged { task_id: TaskId, path: String, kind: FileChangeKind },
    Checkpoint { task_id: TaskId, checkpoint_id: CheckpointId, reason: String },
    Warning { task_id: TaskId, message: String },
    Usage { task_id: TaskId, cumulative: TokenUsage, cost_usd: Option<f64> },
    /// A message sent to the running turn (steering) was handed to the
    /// model; it is in the conversation from here on.
    Steered { task_id: TaskId, text: String, images: usize, files: usize },
    /// A sub-agent was deployed by the Ultra effort tier. `agent_task_id`
    /// is the task the agent's own events carry.
    AgentStarted { task_id: TaskId, agent_id: String, agent_task_id: TaskId, name: String, mode: AgentMode, objective: String },
    /// One of a running sub-agent's own events, wrapped so the parent task's
    /// stream stays in order. Persisted when the inner event is.
    AgentProgress { task_id: TaskId, agent_id: String, inner: Box<AgentEvent> },
    AgentFinished { task_id: TaskId, agent_id: String, report: AgentReport },
    TaskCompleted { task_id: TaskId, outcome: TaskOutcome },
}

impl AgentEvent {
    pub fn task_id(&self) -> &TaskId {
        match self {
            AgentEvent::TaskStarted { task_id, .. }
            | AgentEvent::StateChanged { task_id, .. }
            | AgentEvent::ContextCompiled { task_id, .. }
            | AgentEvent::ModelRequestStarted { task_id, .. }
            | AgentEvent::PromptSent { task_id, .. }
            | AgentEvent::ModelTextDelta { task_id, .. }
            | AgentEvent::ModelReasoningDelta { task_id, .. }
            | AgentEvent::ModelResponse { task_id, .. }
            | AgentEvent::ToolCallStarted { task_id, .. }
            | AgentEvent::ToolCallFinished { task_id, .. }
            | AgentEvent::ToolCallOutput { task_id, .. }
            | AgentEvent::ApprovalRequested { task_id, .. }
            | AgentEvent::ApprovalResolved { task_id, .. }
            | AgentEvent::ValidationStarted { task_id, .. }
            | AgentEvent::ValidationFinished { task_id, .. }
            | AgentEvent::FileChanged { task_id, .. }
            | AgentEvent::Checkpoint { task_id, .. }
            | AgentEvent::Warning { task_id, .. }
            | AgentEvent::Usage { task_id, .. }
            | AgentEvent::Steered { task_id, .. }
            | AgentEvent::AgentStarted { task_id, .. }
            | AgentEvent::AgentProgress { task_id, .. }
            | AgentEvent::AgentFinished { task_id, .. }
            | AgentEvent::TaskCompleted { task_id, .. } => task_id,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            AgentEvent::TaskStarted { .. } => "task_started",
            AgentEvent::StateChanged { .. } => "state_changed",
            AgentEvent::ContextCompiled { .. } => "context_compiled",
            AgentEvent::ModelRequestStarted { .. } => "model_request_started",
            AgentEvent::PromptSent { .. } => "prompt_sent",
            AgentEvent::ModelTextDelta { .. } => "model_text_delta",
            AgentEvent::ModelReasoningDelta { .. } => "model_reasoning_delta",
            AgentEvent::ModelResponse { .. } => "model_response",
            AgentEvent::ToolCallStarted { .. } => "tool_call_started",
            AgentEvent::ToolCallFinished { .. } => "tool_call_finished",
            AgentEvent::ToolCallOutput { .. } => "tool_call_output",
            AgentEvent::ApprovalRequested { .. } => "approval_requested",
            AgentEvent::ApprovalResolved { .. } => "approval_resolved",
            AgentEvent::ValidationStarted { .. } => "validation_started",
            AgentEvent::ValidationFinished { .. } => "validation_finished",
            AgentEvent::FileChanged { .. } => "file_changed",
            AgentEvent::Checkpoint { .. } => "checkpoint",
            AgentEvent::Warning { .. } => "warning",
            AgentEvent::Usage { .. } => "usage",
            AgentEvent::Steered { .. } => "steered",
            AgentEvent::AgentStarted { .. } => "agent_started",
            AgentEvent::AgentProgress { .. } => "agent_progress",
            AgentEvent::AgentFinished { .. } => "agent_finished",
            AgentEvent::TaskCompleted { .. } => "task_completed",
        }
    }

    /// Text deltas are high-volume and not persisted to the event log; tool
    /// outputs are persisted as raw `tool_output` rows instead, and a full
    /// prompt is a debugging view of what the log already has.
    pub fn is_persistent(&self) -> bool {
        match self {
            AgentEvent::ModelTextDelta { .. } | AgentEvent::ModelReasoningDelta { .. } | AgentEvent::ToolCallOutput { .. } | AgentEvent::PromptSent { .. } => false,
            AgentEvent::AgentProgress { inner, .. } => inner.is_persistent(),
            _ => true,
        }
    }

    /// The innermost event: a sub-agent's event unwrapped from its
    /// `AgentProgress` envelope(s), or `self`.
    pub fn innermost(&self) -> &AgentEvent {
        match self {
            AgentEvent::AgentProgress { inner, .. } => inner.innermost(),
            other => other,
        }
    }
}
