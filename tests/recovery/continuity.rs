//! Continuity across provider failures and process restarts.

use ctx_agent::{AgentRuntime, RepositoryContext};
use ctx_core::{ProviderError, TaskStatus};
use ctx_integration_tests::{snapshot, test_config};
use ctx_providers::{MockProvider, MockTurn};
use ctx_tools::AutoApprove;
use serde_json::json;
use std::sync::Arc;

#[tokio::test]
async fn task_state_survives_a_provider_failure_and_a_restart() {
    let repo = snapshot("py-service");
    let data = tempfile::tempdir().unwrap();
    let mut cfg = test_config(data.path());
    cfg.validation.commands = vec![format!("{} -m unittest discover -s tests -v", ctx_core::platform::python_interpreter())];
    // First process: edits, then the provider dies.
    let ctx = RepositoryContext::open(repo.path(), cfg.clone()).unwrap();
    let provider = Arc::new(MockProvider::new(vec![
        MockTurn::ToolCalls(vec![("read_file".into(), json!({"path": "svc/pricing.py"}))]),
        MockTurn::ToolCalls(vec![("apply_patch".into(), json!({"path": "svc/pricing.py", "edits": [{"old_text": "return round(price - pct, 2)", "new_text": "return round(price * (100 - pct) / 100, 2)"}]}))]),
        MockTurn::Error(ProviderError::Connection("socket closed".into())),
        MockTurn::Error(ProviderError::Connection("socket closed".into())),
        MockTurn::Error(ProviderError::Connection("socket closed".into())),
        MockTurn::Error(ProviderError::Connection("socket closed".into())),
        MockTurn::Error(ProviderError::Connection("socket closed".into())),
    ]));
    let rt = AgentRuntime::new(ctx.clone(), provider, Arc::new(AutoApprove));
    let session = rt.sessions().start("test", "mock").unwrap();
    let outcome = rt.run(&session, "Fix the discount bug in svc/pricing.py").await.unwrap();
    assert_eq!(outcome.status, TaskStatus::Failed, "{outcome:#?}");
    assert_eq!(outcome.modified_files, vec!["svc/pricing.py"]);
    let session_id = session.id.clone();
    let task_id = outcome.task_id.clone();
    drop(rt);
    drop(ctx);

    // Second process on the same data directory: resume and continue.
    let ctx = RepositoryContext::open(repo.path(), cfg).unwrap();
    let provider = Arc::new(MockProvider::new(vec![
        MockTurn::ToolCalls(vec![("run_validation".into(), json!({"kind": "test"}))]),
        MockTurn::Text("Validation passes; the discount fix is complete.".into()),
    ]));
    let rt = AgentRuntime::new(ctx.clone(), provider.clone(), Arc::new(AutoApprove));
    let session = rt.sessions().resume(&session_id).unwrap();
    let outcome2 = rt.run(&session, "continue: validate the fix").await.unwrap();
    assert_eq!(outcome2.task_id, task_id, "the follow-up continues the interrupted task");
    assert_eq!(outcome2.status, TaskStatus::Completed, "{outcome2:#?}");
    assert_eq!(outcome2.modified_files, vec!["svc/pricing.py"], "modified files persisted across the restart");
    assert!(outcome2.validation.iter().any(|v| v.passed));
    // The resumed request included the earlier conversation (the patch result).
    let first = &provider.requests()[0];
    assert!(first.messages.iter().any(|m| matches!(m, ctx_core::AgentMessage::ToolResult(r) if r.tool_name == "apply_patch")));
    // Episodic memory lets later sessions recall the work.
    let hits = ctx.memory.search("discount pricing", 5, false).unwrap();
    assert!(hits.iter().any(|(m, _)| m.category == ctx_memory::MemoryCategory::Episodic));
}

#[tokio::test]
async fn stale_index_after_external_edit_is_refreshed_before_use() {
    let repo = snapshot("rust-cli");
    let data = tempfile::tempdir().unwrap();
    let ctx = RepositoryContext::open_ephemeral(repo.path(), test_config(data.path())).unwrap();
    ctx.ensure_indexed(None).unwrap();
    // A human edits a file behind the agent's back.
    let path = repo.path().join("src/commands.rs");
    let original = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, format!("// edited externally\n{original}")).unwrap();
    let result = ctx.retrieval.retrieve(ctx_retrieval::RetrievalRequest::new("greet command", 3000)).await.unwrap();
    let snippet = result.context.iter().find(|s| s.source.path == "src/commands.rs").expect("commands.rs retrieved");
    assert!(result.diagnostics.stale_refreshed >= 1);
    let current = std::fs::read(&path).unwrap();
    assert_eq!(snippet.source.file_hash, ctx_core::content_hash(&current), "snippet carries the current file hash");
}
