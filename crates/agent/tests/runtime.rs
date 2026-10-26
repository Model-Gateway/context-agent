//! End-to-end runtime tests with the scripted mock provider.

use ctx_agent::{AgentRuntime, RepositoryContext, RunOptions};
use ctx_core::{AgentEvent, AgentMessage, Config, EmbeddingBackend, PermissionProfile, TaskStatus};
use ctx_providers::{MockProvider, MockTurn, ModelProvider};
use ctx_tools::{AutoApprove, DenyAll, ToolRegistry};
use serde_json::json;
use std::sync::Arc;

fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("svc")).unwrap();
    std::fs::create_dir_all(root.join("tests")).unwrap();
    std::fs::write(root.join("svc/__init__.py"), "").unwrap();
    std::fs::write(root.join("svc/pricing.py"), "def apply_discount(price, pct):\n    \"\"\"Apply a percentage discount.\"\"\"\n    return price - pct\n").unwrap();
    std::fs::write(root.join("tests/test_pricing.py"), "import unittest\nfrom svc.pricing import apply_discount\n\nclass T(unittest.TestCase):\n    def test_ten_percent(self):\n        self.assertEqual(apply_discount(200, 10), 180)\n\nif __name__ == '__main__':\n    unittest.main()\n").unwrap();
    std::fs::write(root.join("pyproject.toml"), "[project]\nname = \"svc\"\nversion = \"0.1.0\"\n").unwrap();
    std::fs::write(root.join("README.md"), "# svc\nPricing helpers.\n").unwrap();
    dir
}

fn config(root: &std::path::Path, data: &std::path::Path) -> Config {
    let mut cfg = Config::default();
    cfg.data_dir = data.to_path_buf();
    cfg.permissions.profile = PermissionProfile::WorkspaceAutonomous;
    cfg.embeddings.backend = EmbeddingBackend::Hashing;
    cfg.embeddings.dimension = 64;
    cfg.agent.command_timeout_secs = 60;
    // Opt-in; these tests cover the automatic run.
    cfg.agent.auto_validate = true;
    cfg.validation.commands = vec![format!("{} -m unittest discover -s tests -v", ctx_core::platform::python_interpreter())];
    let _ = root;
    cfg
}

type Events = Arc<parking_lot::Mutex<Vec<AgentEvent>>>;

/// Runtime plus a background drain of its events (a real UI consumes them
/// continuously; a test must not let the channel fill up).
fn runtime(ctx: Arc<RepositoryContext>, provider: Arc<dyn ModelProvider>) -> (AgentRuntime, Events) {
    let (tx, mut rx) = tokio::sync::mpsc::channel(256);
    let events: Events = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let sink = events.clone();
    tokio::spawn(async move {
        while let Some(ev) = rx.recv().await {
            sink.lock().push(ev);
        }
    });
    (AgentRuntime::new(ctx, provider, Arc::new(AutoApprove)).with_event_sink(tx), events)
}

#[tokio::test]
async fn fixes_bug_with_validation_and_recovery() {
    let dir = repo();
    let data = tempfile::tempdir().unwrap();
    let ctx = RepositoryContext::open_ephemeral(dir.path(), config(dir.path(), data.path())).unwrap();
    // Scripted model: a wrong fix first (validation fails → recovery), then
    // the right one, then a final report.
    let provider = Arc::new(MockProvider::new(vec![
        MockTurn::ToolCalls(vec![("read_file".into(), json!({"path": "svc/pricing.py"}))]),
        MockTurn::ToolCalls(vec![("apply_patch".into(), json!({"path": "svc/pricing.py", "edits": [{"old_text": "return price - pct", "new_text": "return price - pct * 3"}]}))]),
        MockTurn::ToolCalls(vec![("run_validation".into(), json!({"kind": "test"}))]),
        MockTurn::ToolCalls(vec![("apply_patch".into(), json!({"path": "svc/pricing.py", "edits": [{"old_text": "return price - pct * 3", "new_text": "return price * (100 - pct) / 100"}]}))]),
        MockTurn::ToolCalls(vec![("run_validation".into(), json!({"kind": "test"}))]),
        MockTurn::Text("Fixed apply_discount to apply a percentage. Validation: python3 -m unittest passed.".into()),
    ]));
    let (rt, events) = runtime(ctx.clone(), provider.clone());
    let session = rt.sessions().start("test", "mock").unwrap();
    let outcome = rt.run(&session, "Fix the discount calculation bug in pricing so the tests pass").await.unwrap();
    assert_eq!(outcome.status, TaskStatus::Completed, "{outcome:#?}");
    assert_eq!(outcome.modified_files, vec!["svc/pricing.py"]);
    assert_eq!(outcome.validation.len(), 2);
    assert!(!outcome.validation[0].passed && outcome.validation[1].passed);
    assert!(outcome.unresolved.is_empty(), "{:?}", outcome.unresolved);
    assert_eq!(outcome.model_calls, 6);
    assert!(std::fs::read_to_string(dir.path().join("svc/pricing.py")).unwrap().contains("(100 - pct)"));

    // The first request carried proactive context for pricing.py.
    let first = &provider.requests()[0];
    let ctx_msg = first.messages.iter().find_map(|m| if let AgentMessage::Context(c) = m { Some(c) } else { None }).expect("context payload");
    assert!(ctx_msg.sections.iter().any(|s| s.title.contains("svc/pricing.py")), "{:?}", ctx_msg.sections.iter().map(|s| s.title.clone()).collect::<Vec<_>>());
    assert!(first.messages[0].is_trusted_instruction());

    // Events: recovery happened and files changed.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let events = events.lock().clone();
    assert!(events.iter().any(|e| matches!(e, AgentEvent::StateChanged { to: ctx_core::ExecutionState::Recovering, .. })));
    assert!(events.iter().any(|e| matches!(e, AgentEvent::FileChanged { .. })));
    assert!(events.iter().any(|e| matches!(e, AgentEvent::TaskCompleted { .. })));
    // Episodic memory recorded and the task persisted with status.
    let episodes = ctx.memory.list(Some(ctx_memory::MemoryCategory::Episodic), None, false, 10).unwrap();
    assert_eq!(episodes.len(), 1);
    assert!(episodes[0].content.contains("svc/pricing.py"));
    let task = ctx.stores.sessions.get_task(outcome.task_id.as_str()).unwrap().unwrap();
    assert_eq!(task.status.as_deref(), Some("completed"));
    assert!(ctx.stores.sessions.events_for_task(outcome.task_id.as_str(), 0, 1000).unwrap().len() > 10);
}

#[tokio::test]
async fn auto_validation_runs_when_model_forgets_and_follow_up_continues_task() {
    let dir = repo();
    let data = tempfile::tempdir().unwrap();
    let ctx = RepositoryContext::open_ephemeral(dir.path(), config(dir.path(), data.path())).unwrap();
    let provider = Arc::new(MockProvider::new(vec![
        MockTurn::ToolCalls(vec![("apply_patch".into(), json!({"path": "svc/pricing.py", "edits": [{"old_text": "return price - pct", "new_text": "return price * (100 - pct) / 100"}]}))]),
        MockTurn::Text("Done.".into()),
        // Second user message ("make it return a float") refines the same task.
        MockTurn::ToolCalls(vec![("apply_patch".into(), json!({"path": "svc/pricing.py", "edits": [{"old_text": "return price * (100 - pct) / 100", "new_text": "return float(price * (100 - pct) / 100)"}]}))]),
        MockTurn::Text("Returned a float.".into()),
    ]));
    let (rt, _events) = runtime(ctx.clone(), provider.clone());
    let session = rt.sessions().start("test", "mock").unwrap();
    let outcome = rt.run(&session, "Fix the discount bug in svc/pricing.py").await.unwrap();
    assert_eq!(outcome.status, TaskStatus::Completed, "{outcome:#?}");
    assert_eq!(outcome.validation.len(), 1, "runtime ran validation automatically");
    assert!(outcome.validation[0].passed);
    let first_task = outcome.task_id.clone();
    let session = ctx.stores.sessions.get_session(&session.id).unwrap().unwrap();
    let outcome2 = rt.run(&session, "Make it return a float").await.unwrap();
    assert_eq!(outcome2.task_id, first_task, "refinement stays on the same task");
    assert_eq!(outcome2.status, TaskStatus::Completed, "{outcome2:#?}");
    assert_eq!(outcome2.validation.len(), 2);
    // Unrelated follow-up starts a new task.
    let provider2 = Arc::new(MockProvider::new(vec![MockTurn::Text("The README describes pricing helpers.".into())]));
    let (rt2, _events2) = runtime(ctx.clone(), provider2);
    let session = ctx.stores.sessions.get_session(&session.id).unwrap().unwrap();
    let outcome3 = rt2.run(&session, "Explain what the README says about the project").await.unwrap();
    assert_ne!(outcome3.task_id, first_task);
    assert_eq!(outcome3.status, TaskStatus::Completed);
    assert!(outcome3.modified_files.is_empty());
}

#[tokio::test]
async fn blocked_after_recovery_budget_and_explore_tools_cannot_edit() {
    let dir = repo();
    let data = tempfile::tempdir().unwrap();
    let mut cfg = config(dir.path(), data.path());
    cfg.agent.max_recovery_cycles = 1;
    let ctx = RepositoryContext::open_ephemeral(dir.path(), cfg).unwrap();
    // Always makes a wrong edit, validates, and eventually reports.
    let provider = Arc::new(MockProvider::new(vec![
        MockTurn::ToolCalls(vec![("apply_patch".into(), json!({"path": "svc/pricing.py", "edits": [{"old_text": "return price - pct", "new_text": "return price - pct - 1"}]}))]),
        MockTurn::ToolCalls(vec![("run_validation".into(), json!({"kind": "test"}))]),
        MockTurn::ToolCalls(vec![("apply_patch".into(), json!({"path": "svc/pricing.py", "edits": [{"old_text": "return price - pct - 1", "new_text": "return price - pct - 2"}]}))]),
        MockTurn::ToolCalls(vec![("run_validation".into(), json!({"kind": "test"}))]),
    ]).with_fallback("I could not make the tests pass; the failure is in test_ten_percent."));
    let (rt, _events) = runtime(ctx.clone(), provider);
    let session = rt.sessions().start("test", "mock").unwrap();
    let outcome = rt.run(&session, "Fix pricing").await.unwrap();
    assert_eq!(outcome.status, TaskStatus::Blocked, "{outcome:#?}");
    assert!(outcome.unresolved.iter().any(|u| u.contains("recovery") || u.contains("validation")));

    let cfg = config(dir.path(), data.path());
    let ctx = RepositoryContext::open_ephemeral(dir.path(), cfg).unwrap();
    let provider = Arc::new(MockProvider::new(vec![
        MockTurn::ToolCalls(vec![("apply_patch".into(), json!({"path": "svc/pricing.py", "edits": [{"old_text": "pct", "new_text": "p"}]}))]),
        MockTurn::Text("I can only inspect here; here is the analysis.".into()),
    ]));
    let (rt, _events) = runtime(ctx.clone(), provider.clone());
    let rt = rt.with_tools(ToolRegistry::explore());
    let session = rt.sessions().start("test", "mock").unwrap();
    let outcome = rt.run(&session, "Fix pricing").await.unwrap();
    assert_eq!(outcome.status, TaskStatus::Completed);
    assert!(outcome.modified_files.is_empty());
    let first = &provider.requests()[0];
    assert!(!first.tools.iter().any(|t| t.name == "apply_patch" || t.name == "execute_command"), "the explore set has no editing or shell tools");
    let second = &provider.requests()[1];
    let denied = second.messages.iter().any(|m| matches!(m, AgentMessage::ToolResult(r) if r.is_error && r.content.contains("unknown tool")));
    assert!(denied, "{second:#?}");
}

#[tokio::test]
async fn mixed_validation_batch_is_not_a_pass_and_unvalidated_edits_persist() {
    let dir = repo();
    let data = tempfile::tempdir().unwrap();
    let mut cfg = config(dir.path(), data.path());
    cfg.validation.commands = vec![format!("{} -m unittest discover -s tests -v", ctx_core::platform::python_interpreter()), "sh -c 'exit 0'".into()];
    cfg.agent.max_recovery_cycles = 1;
    let ctx = RepositoryContext::open_ephemeral(dir.path(), cfg).unwrap();
    // Wrong edit, validate (unittest fails, sh passes), then stop with text:
    // the task must not be reported as completed.
    let provider = Arc::new(MockProvider::new(vec![
        MockTurn::ToolCalls(vec![("apply_patch".into(), json!({"path": "svc/pricing.py", "edits": [{"old_text": "return price - pct", "new_text": "return price - pct - 1"}]}))]),
        MockTurn::ToolCalls(vec![("run_validation".into(), json!({"kind": "test"}))]),
    ]).with_fallback("Giving up."));
    let (rt, _events) = runtime(ctx.clone(), provider);
    let session = rt.sessions().start("test", "mock").unwrap();
    let outcome = rt.run(&session, "Fix pricing").await.unwrap();
    assert_eq!(outcome.status, TaskStatus::Blocked, "{outcome:#?}");

    // A later turn that edits after a pass and stops without validating must
    // re-validate automatically, even though it is a new runtime instance.
    let provider = Arc::new(MockProvider::new(vec![
        MockTurn::ToolCalls(vec![("apply_patch".into(), json!({"path": "svc/pricing.py", "edits": [{"old_text": "return price - pct - 1", "new_text": "return price * (100 - pct) / 100"}]}))]),
        MockTurn::Text("Fixed.".into()),
    ]));
    let (rt, _events) = runtime(ctx.clone(), provider);
    let session = ctx.stores.sessions.get_session(&session.id).unwrap().unwrap();
    let outcome = rt.run(&session, "try again, the tests still fail").await.unwrap();
    assert_eq!(outcome.status, TaskStatus::Completed, "{outcome:#?}");
    assert!(outcome.validation.iter().rev().take(2).all(|v| v.passed));
    assert!(outcome.unresolved.is_empty(), "{:?}", outcome.unresolved);
}

#[tokio::test]
async fn falls_back_to_another_model_and_reports_it() {
    use ctx_core::ProviderError;
    use ctx_providers::FallbackProvider;
    let dir = repo();
    let data = tempfile::tempdir().unwrap();
    let ctx = RepositoryContext::open_ephemeral(dir.path(), config(dir.path(), data.path())).unwrap();
    // The primary is down for the first request and healthy afterwards;
    // the fallback answers the first step.
    let primary = Arc::new(MockProvider::new(vec![
        MockTurn::Error(ProviderError::Api { status: 503, code: None, message: "overloaded".into() }),
        MockTurn::Text("Done: the module needs no change.".into()),
    ]).with_model("primary"));
    let secondary = Arc::new(MockProvider::new(vec![MockTurn::ToolCalls(vec![("read_file".into(), json!({"path": "svc/pricing.py"}))])]).with_model("backup"));
    let chain: Arc<dyn ModelProvider> = Arc::new(FallbackProvider::new(vec![primary.clone(), secondary.clone()], std::time::Duration::from_millis(1)));
    let (rt, events) = runtime(ctx.clone(), chain);
    let session = rt.sessions().start("test", "mock").unwrap();
    let outcome = rt.run(&session, "Check whether pricing needs changes").await.unwrap();
    assert_eq!(outcome.status, TaskStatus::Completed, "{outcome:#?}");
    assert_eq!(secondary.requests().len(), 1);
    assert_eq!(primary.requests().len(), 2);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let warnings: Vec<String> = events.lock().iter().filter_map(|e| if let AgentEvent::Warning { message, .. } = e { Some(message.clone()) } else { None }).collect();
    assert!(warnings.iter().any(|w| w.contains("model primary failed") && w.contains("falling back to backup")), "{warnings:?}");
    assert!(warnings.iter().any(|w| w.contains("served by fallback model backup")), "{warnings:?}");
    assert!(warnings.iter().any(|w| w.contains("primary is answering again")), "{warnings:?}");
    // The request log names the model that answered each step.
    let records = ctx.stores.sessions.model_requests_for_task(&outcome.task_id.to_string()).unwrap();
    let models: Vec<&str> = records.iter().map(|r| r.model.as_str()).collect();
    assert_eq!(models, vec!["backup", "primary"], "{records:?}");
}

#[tokio::test]
async fn transient_failure_of_every_model_repeats_the_step_after_a_pause() {
    use ctx_core::ProviderError;
    let dir = repo();
    let data = tempfile::tempdir().unwrap();
    let mut cfg = config(dir.path(), data.path());
    cfg.agent.max_model_failures = 1;
    let ctx = RepositoryContext::open_ephemeral(dir.path(), cfg).unwrap();
    let provider = Arc::new(MockProvider::new(vec![
        MockTurn::Error(ProviderError::RetriesExhausted { attempts: 4, last: "connection refused".into() }),
        MockTurn::Text("Nothing to do.".into()),
    ]));
    let (rt, events) = runtime(ctx.clone(), provider.clone());
    let session = rt.sessions().start("test", "mock").unwrap();
    let started = std::time::Instant::now();
    let outcome = rt.run(&session, "Say hello").await.unwrap();
    assert_eq!(outcome.status, TaskStatus::Completed, "{outcome:#?}");
    assert!(started.elapsed() >= std::time::Duration::from_secs(5), "the step retry waits before repeating");
    assert_eq!(provider.requests().len(), 2);
    assert_eq!(outcome.model_calls, 2);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let warnings: Vec<String> = events.lock().iter().filter_map(|e| if let AgentEvent::Warning { message, .. } = e { Some(message.clone()) } else { None }).collect();
    assert!(warnings.iter().any(|w| w.contains("retrying the step in 5s (failure 1/1)")), "{warnings:?}");

    // A second transient failure exceeds the budget and fails the task.
    let provider = Arc::new(MockProvider::new(vec![MockTurn::Error(ProviderError::Timeout); 3]));
    let (rt, _events) = runtime(ctx.clone(), provider.clone());
    let session = rt.sessions().start("test", "mock").unwrap();
    let outcome = rt.run(&session, "Say hello").await.unwrap();
    assert_eq!(outcome.status, TaskStatus::Failed, "{outcome:#?}");
    assert_eq!(provider.requests().len(), 2);

    // A rejected key is not transient: no wait, immediate failure.
    let provider = Arc::new(MockProvider::new(vec![MockTurn::Error(ProviderError::Auth("bad key".into())); 3]));
    let (rt, _events) = runtime(ctx.clone(), provider.clone());
    let session = rt.sessions().start("test", "mock").unwrap();
    let started = std::time::Instant::now();
    let outcome = rt.run(&session, "Say hello").await.unwrap();
    assert_eq!(outcome.status, TaskStatus::Failed, "{outcome:#?}");
    assert!(started.elapsed() < std::time::Duration::from_secs(4));
    assert_eq!(provider.requests().len(), 1);
}

#[tokio::test]
async fn compaction_checkpoints_long_conversations() {
    let dir = repo();
    let data = tempfile::tempdir().unwrap();
    let mut cfg = config(dir.path(), data.path());
    cfg.model.context_window = 6_000;
    cfg.model.max_output_tokens = 1_000;
    cfg.context.reserve_tool_tokens = 500;
    cfg.context.reserve_output_tokens = 500;
    cfg.context.keep_recent_turns = 1;
    let ctx = RepositoryContext::open_ephemeral(dir.path(), cfg).unwrap();
    let mut turns = Vec::new();
    for _ in 0..8 {
        turns.push(MockTurn::Mixed("Looking at the file again. ".repeat(60), vec![("read_file".into(), json!({"path": "svc/pricing.py"}))]));
    }
    turns.push(MockTurn::Text("No change needed.".into()));
    let provider = Arc::new(MockProvider::new(turns));
    let (rt, events) = runtime(ctx.clone(), provider.clone());
    let session = rt.sessions().start("test", "mock").unwrap();
    let outcome = rt.run(&session, "Investigate pricing").await.unwrap();
    assert_eq!(outcome.status, TaskStatus::Completed);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let checkpoints = events.lock().iter().filter(|ev| matches!(ev, AgentEvent::Checkpoint { .. })).count();
    assert!(checkpoints >= 1, "expected at least one checkpoint");
    assert!(!ctx.stores.sessions.checkpoints_for_task(outcome.task_id.as_str()).unwrap().is_empty());
    // Later requests carried a checkpoint section instead of the full history.
    let last = provider.requests().last().unwrap().clone();
    let has_checkpoint = last.messages.iter().any(|m| matches!(m, AgentMessage::Context(c) if c.sections.iter().any(|s| s.kind == ctx_core::ContextSectionKind::TaskCheckpoint)));
    assert!(has_checkpoint);
    assert!(last.messages.len() < 30);
}

#[tokio::test]
async fn variant_without_proactive_retrieval_sends_no_context() {
    let dir = repo();
    let data = tempfile::tempdir().unwrap();
    let ctx = RepositoryContext::open_ephemeral(dir.path(), config(dir.path(), data.path())).unwrap();
    let provider = Arc::new(MockProvider::new(vec![MockTurn::Text("ok".into())]));
    let (tx, _rx) = tokio::sync::mpsc::channel(64);
    let rt = AgentRuntime::new(ctx, provider.clone(), Arc::new(AutoApprove)).with_event_sink(tx).with_options(RunOptions { proactive_retrieval: false, memory_context: false, ..Default::default() });
    let session = rt.sessions().start("test", "mock").unwrap();
    rt.run(&session, "Say ok").await.unwrap();
    let first = &provider.requests()[0];
    assert!(!first.messages.iter().any(|m| matches!(m, AgentMessage::Context(c) if c.sections.iter().any(|s| s.kind == ctx_core::ContextSectionKind::RepositorySource))));
}

#[tokio::test]
async fn switching_permission_profile_changes_policy_and_instructions() {
    let dir = repo();
    let data = tempfile::tempdir().unwrap();
    let ctx = RepositoryContext::open_ephemeral(dir.path(), config(dir.path(), data.path())).unwrap();
    let provider = Arc::new(MockProvider::new(vec![
        MockTurn::ToolCalls(vec![("apply_patch".into(), json!({"path": "svc/pricing.py", "edits": [{"old_text": "pct", "new_text": "p"}]}))]),
        MockTurn::Text("The edit was declined; nothing changed.".into()),
    ]));
    let (tx, mut rx) = tokio::sync::mpsc::channel(256);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let rt = AgentRuntime::new(ctx.clone(), provider.clone(), Arc::new(DenyAll)).with_event_sink(tx);
    assert_eq!(rt.permission_profile(), PermissionProfile::WorkspaceAutonomous);
    let rt = rt.with_permission_profile(PermissionProfile::ApprovalRequired);
    assert_eq!(rt.permission_profile(), PermissionProfile::ApprovalRequired);
    let session = rt.sessions().start("test", "mock").unwrap();
    let outcome = rt.run(&session, "Fix pricing").await.unwrap();
    assert!(outcome.modified_files.is_empty(), "{outcome:#?}");
    assert!(std::fs::read_to_string(dir.path().join("svc/pricing.py")).unwrap().contains("return price - pct"));
    let first = &provider.requests()[0];
    assert!(first.tools.iter().any(|t| t.name == "apply_patch"), "the full tool set stays; the policy decides");
    assert!(!first.tools.iter().any(|t| t.name == "spawn_agents"), "no sub-agents below ultra effort");
    let instructions = format!("{:?}", first.messages);
    assert!(instructions.contains("Permission profile: approval_required"), "the model is told the active profile");
    let second = &provider.requests()[1];
    assert!(second.messages.iter().any(|m| matches!(m, AgentMessage::ToolResult(r) if r.is_error && r.content.contains("declined"))), "{second:#?}");
}

/// Ultra effort: the model gets `spawn_agents`, deploys an explore and an
/// implement agent that run in their own sessions, and gets their reports
/// back; the parent task folds in their files and usage, and the UI stream
/// carries `agent_started` / `agent_progress` / `agent_finished`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ultra_effort_deploys_sub_agents() {
    use ctx_core::{FinishReason, ModelRequest, ModelResult, ProviderMetadata, ReasoningEffort, ToolCall, TokenUsage, UsageSource};
    use std::sync::atomic::{AtomicU64, Ordering};

    let dir = repo();
    let data = tempfile::tempdir().unwrap();
    let mut cfg = config(dir.path(), data.path());
    cfg.model.reasoning_effort = Some(ReasoningEffort::Ultra);
    cfg.agent.agent_max_steps = 6;
    let ctx = RepositoryContext::open_ephemeral(dir.path(), cfg).unwrap();

    let counter = Arc::new(AtomicU64::new(0));
    let reply = {
        let counter = counter.clone();
        move |text: Option<&str>, calls: Vec<(&str, serde_json::Value)>| -> ModelResult {
            let tool_calls: Vec<ToolCall> = calls.into_iter().map(|(name, arguments)| ToolCall { id: format!("call_{}", counter.fetch_add(1, Ordering::SeqCst)), name: name.into(), arguments }).collect();
            ModelResult {
                finish_reason: if tool_calls.is_empty() { FinishReason::Stop } else { FinishReason::ToolCalls },
                text: text.map(str::to_string),
                tool_calls,
                usage: Some(TokenUsage { prompt_tokens: 100, completion_tokens: 10, cached_prompt_tokens: None, reasoning_tokens: None, source: UsageSource::Reported }),
                provider_metadata: ProviderMetadata { provider: "mock".into(), model: "mock-model".into(), request_id: None, extra: Default::default() },
            }
        }
    };
    let provider = Arc::new(MockProvider::dynamic(move |request: &ModelRequest| {
        let brief = request.messages.iter().find_map(|m| match m { AgentMessage::User(u) => Some(u.text.clone()), _ => None }).unwrap_or_default();
        let has_tool = |name: &str| request.tools.iter().any(|t| t.name == name);
        let saw_result = |tool: &str| request.messages.iter().any(|m| matches!(m, AgentMessage::ToolResult(r) if r.tool_name == tool));
        // The lead agent: deploy two agents, then report.
        if has_tool("spawn_agents") {
            if saw_result("spawn_agents") {
                return Ok(reply(Some("Both agents reported; the discount is fixed."), vec![]));
            }
            return Ok(reply(Some("Splitting this up."), vec![("spawn_agents", json!({
                "context": "The repository is a tiny pricing library.",
                "agents": [
                    {"name": "pricing-explorer", "mode": "explore", "objective": "EXPLORE: find where discounts are applied."},
                    {"name": "pricing-fixer", "mode": "implement", "objective": "IMPLEMENT: make apply_discount subtract a percentage."}
                ]
            }))]));
        }
        // Sub-agents: identified by their brief.
        if brief.starts_with("EXPLORE") {
            assert!(!has_tool("apply_patch") && !has_tool("execute_command"), "explore agents cannot edit or run commands");
            if saw_result("read_file") {
                return Ok(reply(Some("Discounts are applied in svc/pricing.py:3."), vec![]));
            }
            return Ok(reply(None, vec![("read_file", json!({"path": "svc/pricing.py"}))]));
        }
        if brief.starts_with("IMPLEMENT") {
            assert!(has_tool("apply_patch") && !has_tool("spawn_agents"), "implement agents edit but never spawn");
            if saw_result("apply_patch") {
                return Ok(reply(Some("Changed the formula in svc/pricing.py."), vec![]));
            }
            return Ok(reply(None, vec![("apply_patch", json!({"path": "svc/pricing.py", "edits": [{"old_text": "return price - pct", "new_text": "return price - price * pct / 100"}]}))]));
        }
        panic!("unexpected request: {brief}");
    }));
    let (rt, events) = runtime(ctx.clone(), provider.clone());
    assert!(rt.deploys_agents());
    let session = rt.sessions().start("test", "mock").unwrap();
    let outcome = rt.run(&session, "Fix the discount").await.unwrap();
    assert_eq!(outcome.status, TaskStatus::Completed, "{outcome:#?}");
    assert_eq!(outcome.modified_files, vec!["svc/pricing.py".to_string()], "the agent's edit belongs to the lead task");
    assert!(std::fs::read_to_string(dir.path().join("svc/pricing.py")).unwrap().contains("price * pct / 100"));
    assert!(outcome.usage.prompt_tokens >= 500, "sub-agent usage is folded in: {:?}", outcome.usage);

    let lead_request = &provider.requests()[0];
    assert_eq!(lead_request.reasoning_effort, Some(ReasoningEffort::Ultra));
    let spawn_result = provider.requests().iter().flat_map(|r| r.messages.clone()).find_map(|m| match m { AgentMessage::ToolResult(r) if r.tool_name == "spawn_agents" => Some(r.content), _ => None }).expect("the lead got the reports");
    assert!(spawn_result.contains("## pricing-explorer (explore) — completed") && spawn_result.contains("Discounts are applied in svc/pricing.py:3."), "{spawn_result}");
    assert!(spawn_result.contains("## pricing-fixer (implement) — completed") && spawn_result.contains("Files changed: svc/pricing.py"), "{spawn_result}");

    // Wait for the drain, then inspect the stream.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let events = events.lock();
    let started: Vec<_> = events.iter().filter_map(|e| match e { AgentEvent::AgentStarted { name, mode, agent_task_id, .. } => Some((name.clone(), *mode, agent_task_id.clone())), _ => None }).collect();
    assert_eq!(started.len(), 2, "{started:?}");
    assert!(started.iter().any(|(n, m, _)| n == "pricing-explorer" && *m == ctx_core::AgentMode::Explore));
    assert!(started.iter().any(|(n, m, _)| n == "pricing-fixer" && *m == ctx_core::AgentMode::Implement));
    let finished: Vec<_> = events.iter().filter_map(|e| match e { AgentEvent::AgentFinished { report, .. } => Some(report.clone()), _ => None }).collect();
    assert_eq!(finished.len(), 2);
    assert!(finished.iter().all(|r| r.status == TaskStatus::Completed && r.steps >= 2), "{finished:#?}");
    assert!(events.iter().any(|e| matches!(e, AgentEvent::AgentProgress { inner, .. } if matches!(inner.as_ref(), AgentEvent::ToolCallStarted { name, .. } if name == "read_file"))), "agent tool calls are wrapped");
    assert!(events.iter().any(|e| matches!(e, AgentEvent::AgentProgress { inner, .. } if matches!(inner.as_ref(), AgentEvent::ToolCallOutput { .. }))), "agent tool outputs stream too");
    assert!(events.iter().any(|e| matches!(e, AgentEvent::ToolCallOutput { name, .. } if name == "spawn_agents")), "the lead's own tool outputs stream");
    for ev in events.iter() {
        if let AgentEvent::AgentProgress { task_id, .. } | AgentEvent::AgentStarted { task_id, .. } | AgentEvent::AgentFinished { task_id, .. } = ev {
            assert_eq!(task_id, &outcome.task_id, "wrapped events carry the lead task id");
        }
    }
    // Agent sessions stay out of the user's session list, but their tasks
    // point back at the lead task.
    let store = &ctx.stores.sessions;
    assert_eq!(store.list_sessions(ctx.id.as_str(), 50).unwrap().len(), 1, "agent sessions are hidden");
    let children = store.child_sessions(&session.id).unwrap();
    assert_eq!(children.len(), 2);
    for (_, _, agent_task) in &started {
        let task = store.get_task(agent_task.as_str()).unwrap().expect("agent task exists");
        assert_eq!(task.parent_task_id.as_deref(), Some(outcome.task_id.as_str()));
    }
    // The persisted lead-task event log keeps the wrappers (without deltas).
    let rows = store.events_for_task(outcome.task_id.as_str(), 0, 10_000).unwrap();
    assert!(rows.iter().any(|r| r.kind == "agent_started") && rows.iter().any(|r| r.kind == "agent_finished") && rows.iter().any(|r| r.kind == "agent_progress"), "{:?}", rows.iter().map(|r| r.kind.clone()).collect::<Vec<_>>());
    assert!(!rows.iter().any(|r| r.kind == "tool_call_output"), "outputs are persisted as tool_output rows, not events");
}

/// Codex-style agent tools: the lead deploys an agent in the background,
/// waits for its report, sends it a follow-up (a second turn of the same
/// conversation) and collects that report too.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn background_agents_take_follow_ups() {
    use ctx_core::{FinishReason, ModelRequest, ModelResult, ProviderMetadata, ReasoningEffort, ToolCall, TokenUsage, UsageSource};
    use std::sync::atomic::{AtomicU64, Ordering};

    let dir = repo();
    let data = tempfile::tempdir().unwrap();
    let mut cfg = config(dir.path(), data.path());
    cfg.model.reasoning_effort = Some(ReasoningEffort::Ultra);
    cfg.agent.agent_max_steps = 6;
    let ctx = RepositoryContext::open_ephemeral(dir.path(), cfg).unwrap();

    let counter = Arc::new(AtomicU64::new(0));
    let reply = move |text: Option<&str>, calls: Vec<(&str, serde_json::Value)>| -> ModelResult {
        let tool_calls: Vec<ToolCall> = calls.into_iter().map(|(name, arguments)| ToolCall { id: format!("call_{}", counter.fetch_add(1, Ordering::SeqCst)), name: name.into(), arguments }).collect();
        ModelResult {
            finish_reason: if tool_calls.is_empty() { FinishReason::Stop } else { FinishReason::ToolCalls },
            text: text.map(str::to_string),
            tool_calls,
            usage: Some(TokenUsage { prompt_tokens: 100, completion_tokens: 10, cached_prompt_tokens: None, reasoning_tokens: None, source: UsageSource::Reported }),
            provider_metadata: ProviderMetadata { provider: "mock".into(), model: "mock-model".into(), request_id: None, extra: Default::default() },
        }
    };
    let provider = Arc::new(MockProvider::dynamic(move |request: &ModelRequest| {
        let has_tool = |name: &str| request.tools.iter().any(|t| t.name == name);
        let results = |tool: &str| request.messages.iter().filter_map(|m| match m { AgentMessage::ToolResult(r) if r.tool_name == tool => Some(r.content.clone()), _ => None }).collect::<Vec<_>>();
        if has_tool("spawn_agents") {
            assert!(["send_input", "wait_agents", "read_agent", "close_agent"].iter().all(|t| has_tool(t)), "the lead gets the whole agent tool set");
            let spawned = results("spawn_agents");
            let waits = results("wait_agents");
            return Ok(match (spawned.first(), waits.len(), results("send_input").len()) {
                (None, _, _) => reply(None, vec![("spawn_agents", json!({"background": true, "agents": [{"name": "pricing-explorer", "mode": "explore", "objective": "EXPLORE: find where discounts are applied."}]}))]),
                (Some(_), 0, _) => reply(None, vec![("wait_agents", json!({"all": true}))]),
                (Some(spawn), 1, 0) => {
                    let id = spawn.split("id ").nth(1).and_then(|rest| rest.split_whitespace().next()).expect("the spawn result names the agent id");
                    reply(None, vec![("send_input", json!({"id": id, "message": "FOLLOW-UP: which line exactly?"}))])
                }
                (Some(_), 1, _) => reply(None, vec![("wait_agents", json!({}))]),
                _ => reply(Some("The explorer found it on line 3."), vec![]),
            });
        }
        let last_user = request.messages.iter().rev().find_map(|m| match m { AgentMessage::User(u) if !u.text.starts_with("[Context") => Some(u.text.clone()), _ => None }).unwrap_or_default();
        if last_user.starts_with("FOLLOW-UP") {
            assert!(!results("read_file").is_empty(), "the follow-up turn continues the agent's conversation");
            return Ok(reply(Some("Line 3."), vec![]));
        }
        assert!(last_user.starts_with("EXPLORE"), "unexpected request: {last_user}");
        if results("read_file").is_empty() {
            return Ok(reply(None, vec![("read_file", json!({"path": "svc/pricing.py"}))]));
        }
        Ok(reply(Some("Discounts are applied in svc/pricing.py:3."), vec![]))
    }));
    let (rt, events) = runtime(ctx.clone(), provider.clone());
    let session = rt.sessions().start("test", "mock").unwrap();
    let outcome = rt.run(&session, "Where are discounts applied?").await.unwrap();
    assert_eq!(outcome.status, TaskStatus::Completed, "{outcome:#?}");
    assert!(outcome.usage.prompt_tokens >= 800, "both agent turns are folded in: {:?}", outcome.usage);

    let lead_results = |tool: &str| provider.requests().iter().flat_map(|r| r.messages.clone()).filter_map(|m| match m { AgentMessage::ToolResult(r) if r.tool_name == tool => Some(r.content), _ => None }).collect::<std::collections::BTreeSet<_>>();
    let spawn = lead_results("spawn_agents");
    assert!(spawn.iter().all(|c| c.contains("working in the background")), "{spawn:?}");
    let waits = lead_results("wait_agents");
    assert!(waits.iter().any(|c| c.contains("## pricing-explorer (explore) — completed") && c.contains("svc/pricing.py:3")), "{waits:?}");
    assert!(waits.iter().any(|c| c.contains("Line 3.")), "{waits:?}");
    assert!(lead_results("send_input").iter().all(|c| c.contains("follow-up turn")), "the agent had reported");

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let events = events.lock();
    let started: Vec<_> = events.iter().filter_map(|e| match e { AgentEvent::AgentStarted { agent_id, objective, .. } => Some((agent_id.clone(), objective.clone())), _ => None }).collect();
    assert_eq!(started.len(), 2, "one announcement per turn: {started:?}");
    assert_eq!(started[0].0, started[1].0, "both turns belong to the same agent");
    assert!(started[1].1.starts_with("FOLLOW-UP"));
    assert_eq!(events.iter().filter(|e| matches!(e, AgentEvent::AgentFinished { .. })).count(), 2);
    assert_eq!(ctx.stores.sessions.child_sessions(&session.id).unwrap().len(), 1, "the follow-up reuses the agent's session");
}

/// Agents outlive the lead's turn: a later turn of the same conversation
/// is told about the agent, messages it (its conversation continues),
/// retires it with close_agent, and a third turn finds it closed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn agents_persist_across_lead_turns() {
    use ctx_core::{FinishReason, ModelRequest, ModelResult, ProviderMetadata, ReasoningEffort, ToolCall, TokenUsage, UsageSource};
    use std::sync::atomic::{AtomicU64, Ordering};

    let dir = repo();
    let data = tempfile::tempdir().unwrap();
    let mut cfg = config(dir.path(), data.path());
    cfg.model.reasoning_effort = Some(ReasoningEffort::Ultra);
    cfg.agent.agent_max_steps = 6;
    let ctx = RepositoryContext::open_ephemeral(dir.path(), cfg).unwrap();

    let counter = Arc::new(AtomicU64::new(0));
    let reply = move |text: Option<&str>, calls: Vec<(&str, serde_json::Value)>| -> ModelResult {
        let tool_calls: Vec<ToolCall> = calls.into_iter().map(|(name, arguments)| ToolCall { id: format!("call_{}", counter.fetch_add(1, Ordering::SeqCst)), name: name.into(), arguments }).collect();
        ModelResult {
            finish_reason: if tool_calls.is_empty() { FinishReason::Stop } else { FinishReason::ToolCalls },
            text: text.map(str::to_string),
            tool_calls,
            usage: Some(TokenUsage { prompt_tokens: 100, completion_tokens: 10, cached_prompt_tokens: None, reasoning_tokens: None, source: UsageSource::Reported }),
            provider_metadata: ProviderMetadata { provider: "mock".into(), model: "mock-model".into(), request_id: None, extra: Default::default() },
        }
    };
    let provider = Arc::new(MockProvider::dynamic(move |request: &ModelRequest| {
        // The latest real user message, and the tool results since.
        let start = request.messages.iter().rposition(|m| matches!(m, AgentMessage::User(u) if !u.text.starts_with("[Context"))).unwrap_or(0);
        let last_user = match &request.messages[start] { AgentMessage::User(u) => u.text.clone(), _ => String::new() };
        let results = |tool: &str| request.messages[start..].iter().filter_map(|m| match m { AgentMessage::ToolResult(r) if r.tool_name == tool => Some(r.content.clone()), _ => None }).collect::<Vec<_>>();
        if request.tools.iter().any(|t| t.name == "spawn_agents") {
            return Ok(match last_user.as_str() {
                "FIRST" if results("spawn_agents").is_empty() => reply(None, vec![("spawn_agents", json!({"agents": [{"name": "pricing-explorer", "mode": "explore", "objective": "EXPLORE: find where discounts are applied."}]}))]),
                "SECOND" if results("send_input").is_empty() => {
                    assert!(format!("{:?}", request.messages).contains("take follow-ups through send_input: pricing-explorer [agent-"), "the lead is told about the earlier agent");
                    reply(None, vec![("send_input", json!({"id": "pricing-explorer", "message": "FOLLOW-UP: which line exactly?"}))])
                }
                "SECOND" if results("wait_agents").is_empty() => reply(None, vec![("wait_agents", json!({}))]),
                "SECOND" if results("close_agent").is_empty() => reply(None, vec![("close_agent", json!({"id": "pricing-explorer"}))]),
                "THIRD" if results("send_input").is_empty() => reply(None, vec![("send_input", json!({"id": "pricing-explorer", "message": "FOLLOW-UP: anything else?"}))]),
                _ => reply(Some("Done."), vec![]),
            });
        }
        let agent_user = request.messages.iter().rev().find_map(|m| match m { AgentMessage::User(u) if !u.text.starts_with("[Context") => Some(u.text.clone()), _ => None }).unwrap_or_default();
        let saw_read = request.messages.iter().any(|m| matches!(m, AgentMessage::ToolResult(r) if r.tool_name == "read_file"));
        if agent_user.starts_with("FOLLOW-UP") {
            assert!(saw_read, "the agent's conversation survives the lead's turn");
            return Ok(reply(Some("Line 3."), vec![]));
        }
        Ok(if saw_read { reply(Some("Discounts are applied in svc/pricing.py:3."), vec![]) } else { reply(None, vec![("read_file", json!({"path": "svc/pricing.py"}))]) })
    }));
    let (rt, _events) = runtime(ctx.clone(), provider.clone());
    let session = rt.sessions().start("test", "mock").unwrap();
    for message in ["FIRST", "SECOND", "THIRD"] {
        let session = ctx.stores.sessions.get_session(&session.id).unwrap().unwrap();
        let outcome = rt.run(&session, message).await.unwrap();
        assert_eq!(outcome.status, TaskStatus::Completed, "{message}: {outcome:#?}");
    }
    let lead_results = |tool: &str| provider.requests().iter().flat_map(|r| r.messages.clone()).filter_map(|m| match m { AgentMessage::ToolResult(r) if r.tool_name == tool => Some((r.content, r.is_error)), _ => None }).collect::<Vec<_>>();
    assert!(lead_results("wait_agents").iter().any(|(c, _)| c.contains("Line 3.")), "the follow-up from a later turn reported");
    assert!(lead_results("send_input").iter().any(|(c, error)| *error && c.contains("is closed")), "a closed agent stays closed in later turns");
    let children = ctx.stores.sessions.child_sessions(&session.id).unwrap();
    assert_eq!(children.len(), 1, "one agent across all three turns");
    assert_eq!(children[0].metadata.get("closed").and_then(|c| c.as_bool()), Some(true));
}

#[tokio::test]
async fn below_ultra_there_is_no_spawn_tool_and_agents_never_nest() {
    let dir = repo();
    let data = tempfile::tempdir().unwrap();
    let ctx = RepositoryContext::open_ephemeral(dir.path(), config(dir.path(), data.path())).unwrap();
    let provider = Arc::new(MockProvider::new(vec![MockTurn::Text("Nothing to do.".into())]));
    let (rt, _events) = runtime(ctx.clone(), provider.clone());
    assert!(!rt.deploys_agents());
    let high = rt.with_reasoning_effort(Some(ctx_core::ReasoningEffort::level("high")));
    assert!(!high.deploys_agents());
    let ultra = high.with_reasoning_effort(Some(ctx_core::ReasoningEffort::Ultra));
    assert!(ultra.deploys_agents());
    let auto = ultra.with_reasoning_effort(None);
    assert!(!auto.deploys_agents() && auto.reasoning_effort().is_none(), "auto clears a configured level");
    let session = rt.sessions().start("test", "mock").unwrap();
    rt.run(&session, "Say ok").await.unwrap();
    let first = &provider.requests()[0];
    assert!(!first.tools.iter().any(|t| t.name == "spawn_agents"));
    assert_eq!(first.reasoning_effort, None);
}

/// Sends a steering message the moment the model is first asked, so it is
/// waiting when that (final) answer comes back.
struct SteerOnFirstCall {
    inner: Arc<MockProvider>,
    inbox: ctx_agent::Inbox,
    sent: std::sync::atomic::AtomicBool,
}

#[async_trait::async_trait]
impl ModelProvider for SteerOnFirstCall {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn model(&self) -> &str {
        self.inner.model()
    }
    fn capabilities(&self) -> ctx_providers::EffectiveCapabilities {
        self.inner.capabilities()
    }
    async fn complete(&self, request: &ctx_core::ModelRequest, stream: Option<ctx_providers::StreamSink>, cancel: &ctx_core::CancellationToken) -> ctx_core::Result<ctx_core::ModelResult> {
        if !self.sent.swap(true, std::sync::atomic::Ordering::SeqCst) {
            self.inbox.send(ctx_core::UserMessage { text: "also mention the README".into(), ..Default::default() }).unwrap();
        }
        self.inner.complete(request, stream, cancel).await
    }
    async fn probe(&self, cancel: &ctx_core::CancellationToken) -> ctx_core::Result<ctx_providers::CapabilityProfile> {
        self.inner.probe(cancel).await
    }
    fn apply_profile(&self, profile: &ctx_providers::CapabilityProfile) {
        self.inner.apply_profile(profile)
    }
}

#[tokio::test]
async fn steering_sent_while_the_model_answers_is_read_before_finishing() {
    let dir = repo();
    let data = tempfile::tempdir().unwrap();
    let ctx = RepositoryContext::open_ephemeral(dir.path(), config(dir.path(), data.path())).unwrap();
    let mock = Arc::new(MockProvider::new(vec![MockTurn::Text("Pricing applies a flat discount.".into()), MockTurn::Text("The README describes the pricing helpers.".into())]));
    let inbox = ctx_agent::Inbox::new();
    let provider = Arc::new(SteerOnFirstCall { inner: mock.clone(), inbox: inbox.clone(), sent: false.into() });
    let (rt, events) = runtime(ctx.clone(), provider);
    let rt = rt.with_inbox(inbox.clone());
    let session = rt.sessions().start("test", "mock").unwrap();
    let outcome = rt.run(&session, "How does pricing work?").await.unwrap();
    assert_eq!(outcome.status, TaskStatus::Completed, "{outcome:#?}");
    assert_eq!(outcome.model_calls, 2, "the turn went on to answer the steering");
    let second = &mock.requests()[1];
    assert!(second.messages.iter().any(|m| matches!(m, AgentMessage::User(u) if u.text == "also mention the README")));
    assert!(events.lock().iter().any(|e| matches!(e, AgentEvent::Steered { text, .. } if text == "also mention the README")));
    assert!(inbox.send(ctx_core::UserMessage { text: "late".into(), ..Default::default() }).is_err(), "closed once the run ended");
}

#[tokio::test]
async fn much_worse_gateway_discount_asks_and_continues_without_the_guard() {
    use ctx_core::ProviderError;
    let dir = repo();
    let data = tempfile::tempdir().unwrap();
    let ctx = RepositoryContext::open_ephemeral(dir.path(), config(dir.path(), data.path())).unwrap();
    let drop = || MockTurn::Error(ProviderError::DiscountDrop { expected: Some(98.0), available: Some(0.0), message: "refused".into() });
    let provider = Arc::new(MockProvider::new(vec![drop(), MockTurn::Text("Nothing to do.".into())]));
    let (rt, events) = runtime(ctx.clone(), provider.clone());
    let session = rt.sessions().start("test", "mock").unwrap();
    let outcome = rt.run(&session, "Check whether pricing needs changes").await.unwrap();
    assert_eq!(outcome.status, TaskStatus::Completed, "{outcome:#?}");
    // Guarded until the user agreed; the repeated step goes without it.
    let guards: Vec<Option<u32>> = provider.requests().iter().map(|r| r.max_discount_drop).collect();
    assert_eq!(guards, vec![Some(ctx_core::pricing::DISCOUNT_DROP_POINTS), None]);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let asked: Vec<ctx_core::ApprovalRequest> = events.lock().iter().filter_map(|e| if let AgentEvent::ApprovalRequested { request, .. } = e { Some(request.clone()) } else { None }).collect();
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert_eq!(asked[0].tool, ctx_agent::DISCOUNT_DROP_APPROVAL);
    assert_eq!(asked[0].summary, "Continue mock-model at 0% off instead of 98% off?");

    // Denied: the run is cancelled.
    let provider = Arc::new(MockProvider::new(vec![drop(), MockTurn::Text("Nothing to do.".into())]));
    let rt = AgentRuntime::new(ctx.clone(), provider.clone(), Arc::new(DenyAll));
    let session = rt.sessions().start("test", "mock").unwrap();
    let outcome = rt.run(&session, "Check whether pricing needs changes").await.unwrap();
    assert_eq!(outcome.status, TaskStatus::Cancelled, "{outcome:#?}");
    assert_eq!(provider.requests().len(), 1);
}

#[tokio::test]
async fn always_continue_sends_no_discount_guard() {
    let dir = repo();
    let data = tempfile::tempdir().unwrap();
    let mut cfg = config(dir.path(), data.path());
    cfg.spend.confirm_discount_drop = false;
    let ctx = RepositoryContext::open_ephemeral(dir.path(), cfg).unwrap();
    let provider = Arc::new(MockProvider::new(vec![MockTurn::Text("Nothing to do.".into())]));
    let (rt, _events) = runtime(ctx, provider.clone());
    let session = rt.sessions().start("test", "mock").unwrap();
    rt.run(&session, "Check whether pricing needs changes").await.unwrap();
    assert!(provider.requests().iter().all(|r| r.max_discount_drop.is_none()));
}

/// A gateway that is being updated: its status answers come from a script,
/// then it is online.
struct UpdatingGateway {
    inner: Arc<MockProvider>,
    statuses: parking_lot::Mutex<std::collections::VecDeque<ctx_providers::GatewayStatus>>,
}

#[async_trait::async_trait]
impl ModelProvider for UpdatingGateway {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn model(&self) -> &str {
        self.inner.model()
    }
    fn capabilities(&self) -> ctx_providers::EffectiveCapabilities {
        self.inner.capabilities()
    }
    async fn complete(&self, request: &ctx_core::ModelRequest, stream: Option<ctx_providers::StreamSink>, cancel: &ctx_core::CancellationToken) -> ctx_core::Result<ctx_core::ModelResult> {
        self.inner.complete(request, stream, cancel).await
    }
    async fn probe(&self, cancel: &ctx_core::CancellationToken) -> ctx_core::Result<ctx_providers::CapabilityProfile> {
        self.inner.probe(cancel).await
    }
    fn apply_profile(&self, profile: &ctx_providers::CapabilityProfile) {
        self.inner.apply_profile(profile)
    }
    async fn gateway_status(&self, _cancel: &ctx_core::CancellationToken) -> Option<ctx_providers::GatewayStatus> {
        Some(self.statuses.lock().pop_front().unwrap_or(ctx_providers::GatewayStatus::Online))
    }
}

#[tokio::test]
async fn a_step_failing_during_a_gateway_update_waits_for_it_and_repeats() {
    use ctx_core::ProviderError;
    use ctx_providers::GatewayStatus;
    let dir = repo();
    let data = tempfile::tempdir().unwrap();
    let mut cfg = config(dir.path(), data.path());
    // Any other failure would end the run: the wait is not charged.
    cfg.agent.max_model_failures = 0;
    let ctx = RepositoryContext::open_ephemeral(dir.path(), cfg).unwrap();
    let mock = Arc::new(MockProvider::new(vec![MockTurn::Error(ProviderError::Api { status: 503, code: Some("gateway_updating".into()), message: "updating".into() }), MockTurn::Text("Nothing to do.".into())]));
    let provider = Arc::new(UpdatingGateway { inner: mock.clone(), statuses: parking_lot::Mutex::new([GatewayStatus::Updating].into()) });
    let (rt, events) = runtime(ctx, provider);
    let session = rt.sessions().start("test", "mock").unwrap();
    let outcome = rt.run(&session, "Check whether pricing needs changes").await.unwrap();
    assert_eq!(outcome.status, TaskStatus::Completed, "{outcome:#?}");
    assert_eq!(mock.requests().len(), 2);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let warnings: Vec<String> = events.lock().iter().filter_map(|e| if let AgentEvent::Warning { message, .. } = e { Some(message.clone()) } else { None }).collect();
    assert!(warnings.iter().any(|w| w.starts_with("ModelGateway is being updated")), "{warnings:?}");
    assert!(warnings.iter().any(|w| w.starts_with("ModelGateway is back online")), "{warnings:?}");
}

#[tokio::test]
async fn mentioned_files_reach_the_model_and_count_as_read() {
    let dir = repo();
    let data = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".env"), "SECRET=1\n").unwrap();
    let ctx = RepositoryContext::open_ephemeral(dir.path(), config(dir.path(), data.path())).unwrap();
    let provider = Arc::new(MockProvider::new(vec![MockTurn::Text("Read them.".into())]));
    let (rt, _events) = runtime(ctx, provider.clone());
    let session = rt.sessions().start("test", "mock").unwrap();
    rt.run(&session, "Summarise @README.md and @svc/pricing.py, see @docs/missing.md and @.env").await.unwrap();
    let first = &provider.requests()[0];
    let attached = first.messages.iter().find_map(|m| match m {
        AgentMessage::User(u) if u.text == ctx_agent::runtime::FILE_MENTIONS_NOTE => Some(u.files.clone()),
        _ => None,
    });
    let attached = attached.expect("the mentioned files follow the message");
    assert_eq!(attached.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), vec!["README.md", "svc/pricing.py"]);
    assert!(attached[0].content.contains("Pricing helpers."));
    let instructions = first.messages.iter().find_map(|m| match m {
        AgentMessage::TaskInstructions(t) => Some(t.text.clone()),
        _ => None,
    });
    let instructions = instructions.unwrap();
    assert!(instructions.contains("mentions README.md, svc/pricing.py"), "{instructions}");
    assert!(instructions.contains("not found in the repository: docs/missing.md"), "{instructions}");
    assert!(instructions.contains("not readable under the active permissions") && instructions.contains(".env"), "credential files are refused: {instructions}");
    assert!(ctx_agent::runtime::is_hidden_note(ctx_agent::runtime::FILE_MENTIONS_NOTE));
}
