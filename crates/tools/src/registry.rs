//! Tool trait, execution context, and registry.

use crate::approval::ApprovalHandler;
use crate::schema;
use async_trait::async_trait;
use ctx_core::{AgentReport, ApprovalDecision, ApprovalRequest, CancellationToken, CtxError, Result, SessionId, SourceRef, TaskId, ToolCall, ToolDefinition, ValidationRecord, GatewayAccess, WebFetchConfig, WebSearchConfig};
use ctx_indexing::{Indexer, ValidationCommand};
use ctx_memory::MemoryStore;
use ctx_retrieval::RetrievalEngine;
use ctx_security::{Action, Decision, PolicyEngine, Redactor};
use ctx_telemetry::EventLog;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Observable side effects of a tool invocation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "effect", rename_all = "snake_case")]
pub enum ToolEffect {
    FileRead { path: String },
    SourceRead { source: SourceRef },
    FileCreated { path: String },
    FileModified { path: String },
    FileDeleted { path: String },
    CommandRun { command: String, exit_code: Option<i32>, duration_ms: u64 },
    Validation { record: ValidationRecord },
    Search { query: String, hits: usize },
    /// A sub-agent deployed by the Ultra effort tier finished; the runtime
    /// folds its usage and modified files into the parent task.
    Agent { report: AgentReport },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolOutput {
    /// Content returned to the model (already truncated/redacted).
    pub content: String,
    pub is_error: bool,
    /// One-line summary for the UI and event log.
    pub summary: String,
    #[serde(default)]
    pub effects: Vec<ToolEffect>,
    /// Length of the untruncated content.
    #[serde(default)]
    pub full_len: usize,
    /// Images the tool returned (screenshots); the runtime shows them to
    /// the model in a message after the tool results, since tool results
    /// themselves carry text only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<ctx_core::ImageInput>,
}

impl ToolOutput {
    pub fn ok(content: impl Into<String>, summary: impl Into<String>) -> Self {
        let content = content.into();
        let full_len = content.len();
        Self { content, is_error: false, summary: summary.into(), effects: Vec::new(), full_len, images: Vec::new() }
    }

    pub fn error(message: impl Into<String>) -> Self {
        let message = message.into();
        let full_len = message.len();
        Self { content: message.clone(), is_error: true, summary: message.lines().next().unwrap_or("").chars().take(120).collect(), effects: Vec::new(), full_len, images: Vec::new() }
    }

    pub fn with_images(mut self, images: Vec<ctx_core::ImageInput>) -> Self {
        self.images = images;
        self
    }

    pub fn with_effect(mut self, effect: ToolEffect) -> Self {
        self.effects.push(effect);
        self
    }
}

/// Everything a tool needs at invocation time.
pub struct ToolContext {
    pub root: PathBuf,
    pub task_id: TaskId,
    pub session_id: SessionId,
    pub policy: Arc<PolicyEngine>,
    pub approvals: Arc<dyn ApprovalHandler>,
    pub indexer: Option<Arc<Indexer>>,
    pub retrieval: Option<Arc<RetrievalEngine>>,
    pub memory: Option<MemoryStore>,
    pub events: Option<EventLog>,
    pub validation_commands: Vec<ValidationCommand>,
    pub command_timeout: Duration,
    pub redactor: Arc<Redactor>,
    pub cancel: CancellationToken,
    pub shell: String,
    /// Maximum characters of tool output returned to the model.
    pub max_output_chars: usize,
    /// Latest context explanation, for `inspect_context`.
    pub context_explanation: Arc<RwLock<Option<ctx_context::ContextExplanation>>>,
    /// Files in the active task's working set (for freshness inspection).
    pub working_set: Arc<RwLock<Vec<String>>>,
    /// Provider settings for `web_search`.
    pub web_search: WebSearchConfig,
    /// The signed-in ModelGateway account, which `web_search` prefers.
    pub gateway: Option<GatewayAccess>,
    pub web_fetch: WebFetchConfig,
    /// What this session has seen and written, shared with the other
    /// sessions of the project (stale-edit checks).
    pub files: crate::activity::SessionFiles,
}

impl ToolContext {
    pub fn relative(&self, abs: &std::path::Path) -> String {
        self.policy.paths().relative(abs)
    }

    /// Keep head and tail of oversized output; the full text goes to the
    /// event log.
    pub fn truncate(&self, text: String) -> (String, usize) {
        let full = text.len();
        if full <= self.max_output_chars {
            return (text, full);
        }
        let head = self.max_output_chars * 2 / 3;
        let tail = self.max_output_chars - head;
        let mut head_end = head;
        while head_end > 0 && !text.is_char_boundary(head_end) {
            head_end -= 1;
        }
        let mut tail_start = full - tail;
        while tail_start < full && !text.is_char_boundary(tail_start) {
            tail_start += 1;
        }
        (format!("{}\n… [{} characters omitted; the full output is recorded in the task event log] …\n{}", &text[..head_end], tail_start - head_end, &text[tail_start..]), full)
    }
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> Value;
    /// Map validated arguments to a security action for policy evaluation.
    fn action(&self, args: &Value, ctx: &ToolContext) -> Result<Action>;
    async fn invoke(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput>;

    fn definition(&self) -> ToolDefinition {
        ToolDefinition { name: self.name().to_string(), description: self.description().to_string(), parameters: self.parameters() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolExecution {
    pub call_id: String,
    pub tool: String,
    pub output: ToolOutput,
    pub decision: Decision,
    pub approval: Option<ApprovalDecision>,
    pub duration_ms: u64,
    pub executed: bool,
}

pub struct ToolRegistry {
    tools: Vec<Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new(tools: Vec<Arc<dyn Tool>>) -> Self {
        Self { tools }
    }

    /// All built-in tools (spec §10.3).
    pub fn standard() -> Self {
        Self::new(vec![
            Arc::new(crate::files::ReadFile),
            Arc::new(crate::search::SearchCode),
            Arc::new(crate::search::FindSymbol),
            Arc::new(crate::search::FindReferences),
            Arc::new(crate::files::ListFiles),
            Arc::new(crate::files::ApplyPatch),
            Arc::new(crate::files::WriteFile),
            Arc::new(crate::shell::ExecuteCommand),
            Arc::new(crate::shell::RunValidation),
            Arc::new(crate::shell::GitStatus),
            Arc::new(crate::shell::GitDiff),
            Arc::new(crate::inspect::RetrieveMemory),
            Arc::new(crate::inspect::InspectContext),
            Arc::new(crate::web::WebSearch),
            Arc::new(crate::web::WebFetch),
        ])
    }

    /// The inspection subset for an `explore` sub-agent: reading, searching
    /// (code and web), fetching pages and git inspection, no edits and no
    /// shell. Being read-only is a
    /// property of the tool set, not of a permission profile.
    pub fn explore() -> Self {
        Self::new(vec![
            Arc::new(crate::files::ReadFile),
            Arc::new(crate::search::SearchCode),
            Arc::new(crate::search::FindSymbol),
            Arc::new(crate::search::FindReferences),
            Arc::new(crate::files::ListFiles),
            Arc::new(crate::shell::GitStatus),
            Arc::new(crate::shell::GitDiff),
            Arc::new(crate::inspect::RetrieveMemory),
            Arc::new(crate::inspect::InspectContext),
            Arc::new(crate::web::WebSearch),
            Arc::new(crate::web::WebFetch),
        ])
    }

    /// This registry plus `tool` (replacing any tool of the same name).
    pub fn with_tool(&self, tool: Arc<dyn Tool>) -> Self {
        let mut tools: Vec<Arc<dyn Tool>> = self.tools.iter().filter(|t| t.name() != tool.name()).cloned().collect();
        tools.push(tool);
        Self::new(tools)
    }

    /// This registry without the web tools `config` turns off.
    pub fn for_config(&self, config: &ctx_core::Config) -> Self {
        let mut tools = Self::new(self.tools.clone());
        if !config.web_search.enabled {
            tools = tools.without("web_search");
        }
        if !config.web_fetch.enabled {
            tools = tools.without("web_fetch");
        }
        tools
    }

    /// This registry without the named tool.
    pub fn without(&self, name: &str) -> Self {
        Self::new(self.tools.iter().filter(|t| t.name() != name).cloned().collect())
    }

    pub fn tools(&self) -> &[Arc<dyn Tool>] {
        &self.tools
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.iter().find(|t| t.name() == name).cloned()
    }

    pub fn names(&self) -> Vec<&str> {
        self.tools.iter().map(|t| t.name()).collect()
    }

    /// Deterministically ordered definitions (stable prefix for caching).
    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools.iter().map(|t| t.definition()).collect()
    }

    /// Validate, authorize, and run one tool call. Never panics; every
    /// failure becomes an error output for the model.
    pub async fn execute(&self, call: &ToolCall, ctx: &ToolContext) -> ToolExecution {
        let started = Instant::now();
        let mut exec = ToolExecution { call_id: call.id.clone(), tool: call.name.clone(), output: ToolOutput::error(""), decision: Decision::Allow, approval: None, duration_ms: 0, executed: false };
        let Some(tool) = self.get(&call.name) else {
            exec.output = ToolOutput::error(format!("unknown tool `{}`. Available tools: {}", call.name, self.names().join(", ")));
            exec.duration_ms = started.elapsed().as_millis() as u64;
            return exec;
        };
        if let Some(raw) = call.arguments.get("__malformed_arguments") {
            let reason = call.arguments.get("__reason").and_then(|r| r.as_str()).unwrap_or("invalid JSON");
            exec.output = ToolOutput::error(format!("the arguments for `{}` were not valid JSON ({reason}). Raw arguments: {}. Resend the call with a valid JSON object.", call.name, raw));
            exec.duration_ms = started.elapsed().as_millis() as u64;
            return exec;
        }
        if let Err(e) = schema::validate(&tool.parameters(), &call.arguments) {
            exec.output = ToolOutput::error(format!("invalid arguments for `{}`: {e}", call.name));
            exec.duration_ms = started.elapsed().as_millis() as u64;
            return exec;
        }
        let action = match tool.action(&call.arguments, ctx) {
            Ok(a) => a,
            Err(e) => {
                exec.output = ToolOutput::error(format!("`{}` rejected: {e}", call.name));
                exec.duration_ms = started.elapsed().as_millis() as u64;
                return exec;
            }
        };
        let decision = ctx.policy.evaluate(&action);
        exec.decision = decision.clone();
        match decision {
            Decision::Allow => {}
            Decision::Deny { reason } => {
                exec.output = ToolOutput::error(format!("`{}` denied by the active permission profile ({}): {reason}. Choose a different approach or ask the user.", call.name, ctx.policy.profile().as_str()));
                exec.duration_ms = started.elapsed().as_millis() as u64;
                return exec;
            }
            Decision::RequireApproval { reason, risk } => {
                let request = ApprovalRequest { id: format!("approval_{}", call.id), tool: call.name.clone(), risk, summary: action.summary.clone(), details: action.details.clone(), reason };
                let approval = ctx.approvals.request(request).await;
                exec.approval = Some(approval);
                if approval == ApprovalDecision::Deny {
                    exec.output = ToolOutput::error(format!("the user declined `{}`: {}. Do not retry the same action; adjust your approach or explain what you need.", call.name, action.summary));
                    exec.duration_ms = started.elapsed().as_millis() as u64;
                    return exec;
                }
            }
        }
        if ctx.cancel.is_cancelled() {
            exec.output = ToolOutput::error("cancelled before execution");
            exec.duration_ms = started.elapsed().as_millis() as u64;
            return exec;
        }
        exec.executed = true;
        // A bug inside one tool must not take down the whole run: a panic is
        // reported to the model as a tool error. Cancelling always wins, so
        // a tool stuck on I/O can never wedge the run past the user's Stop.
        let invoked = {
            use futures::FutureExt;
            tokio::select! {
                r = std::panic::AssertUnwindSafe(tool.invoke(call.arguments.clone(), ctx)).catch_unwind() => r,
                _ = ctx.cancel.cancelled() => Ok(Err(CtxError::Cancelled)),
            }
        };
        let invoked = match invoked {
            Ok(result) => result,
            Err(panic) => {
                let detail = panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_else(|| "unknown panic".into());
                tracing::error!(tool = %call.name, %detail, "tool panicked");
                Err(CtxError::tool(format!("internal error in {}: {detail}", call.name)))
            }
        };
        exec.output = match invoked {
            Ok(mut out) => {
                let redacted = ctx.redactor.redact_credentials_only(&out.content);
                let (truncated, full) = ctx.truncate(redacted);
                out.content = truncated;
                out.full_len = full;
                out
            }
            Err(CtxError::Cancelled) => ToolOutput::error("cancelled"),
            Err(e) => ToolOutput::error(format!("`{}` failed: {e}", call.name)),
        };
        exec.duration_ms = started.elapsed().as_millis() as u64;
        exec
    }
}

/// Render a unified diff for display.
pub fn unified_diff(path: &str, old: &str, new: &str) -> String {
    let diff = similar::TextDiff::from_lines(old, new);
    diff.unified_diff().context_radius(3).header(&format!("a/{path}"), &format!("b/{path}")).to_string()
}
