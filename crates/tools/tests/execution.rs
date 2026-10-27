//! End-to-end tool execution through the registry with policy, approvals,
//! indexing and memory wired in.

use ctx_core::{ApprovalDecision, ApprovalRequest, CancellationToken, IndexConfig, PermissionProfile, PermissionsConfig, SessionId, TaskId, ToolCall};
use ctx_embeddings::{EmbeddingService, HashingEmbedder};
use ctx_indexing::{Indexer, IndexerOptions, RepositoryDiscovery, ValidationCommand};
use ctx_languages::LanguageRegistry;
use ctx_memory::MemoryStore;
use ctx_retrieval::RetrievalEngine;
use ctx_security::{Decision, PathPolicy, PolicyEngine, Redactor};
use ctx_storage::{IndexStore, SessionStore};
use ctx_tools::{ApprovalHandler, AutoApprove, DenyAll, ToolContext, ToolEffect, ToolRegistry};
use parking_lot::RwLock;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

struct Recording {
    seen: parking_lot::Mutex<Vec<ApprovalRequest>>,
    decision: ApprovalDecision,
}

#[async_trait::async_trait]
impl ApprovalHandler for Recording {
    async fn request(&self, request: ApprovalRequest) -> ApprovalDecision {
        self.seen.lock().push(request);
        self.decision
    }
}

fn fixture(profile: PermissionProfile, approvals: Arc<dyn ApprovalHandler>) -> (tempfile::TempDir, ToolContext, ToolRegistry) {
    let dir = tempfile::tempdir().unwrap();
    let root = ctx_core::platform::canonicalize(dir.path()).unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/lib.rs"), "pub fn greet(name: &str) -> String {\n    format!(\"Hello, {name}!\")\n}\n\npub fn shout(name: &str) -> String {\n    greet(name).to_uppercase()\n}\n").unwrap();
    std::fs::write(root.join("README.md"), "# demo\n").unwrap();
    std::fs::write(root.join(".env"), "SECRET=abc\n").unwrap();
    let store = Arc::new(IndexStore::in_memory().unwrap());
    let embeddings = Arc::new(EmbeddingService::load(store.clone(), Arc::new(HashingEmbedder::new(64))).unwrap());
    let indexer = Arc::new(Indexer::new(&root, &IndexConfig::default(), IndexerOptions::default(), store, Arc::new(LanguageRegistry::with_defaults()), Some(embeddings)).unwrap());
    indexer.full_index(None).unwrap();
    let memory = MemoryStore::new(Arc::new(SessionStore::in_memory().unwrap()), ctx_core::RepositoryId::from_root(&root));
    let retrieval = Arc::new(RetrievalEngine::new(indexer.clone(), Some(memory.clone())));
    let paths = Arc::new(PathPolicy::new(vec![root.clone()], &IndexConfig::default().credential_patterns).unwrap());
    let policy = Arc::new(PolicyEngine::new(PermissionsConfig { profile, allowed_commands: vec!["echo".into()], ..Default::default() }, paths));
    let ctx = ToolContext {
        root: root.clone(),
        task_id: TaskId::new(),
        session_id: SessionId::new(),
        policy,
        approvals,
        indexer: Some(indexer),
        retrieval: Some(retrieval),
        memory: Some(memory),
        events: None,
        validation_commands: vec![ValidationCommand { command: "sh -c 'exit 0'".into(), kind: ctx_indexing::ValidationKind::Test, cwd: ".".into(), source: "test".into(), system: ctx_indexing::BuildSystem::Make }],
        command_timeout: Duration::from_secs(5),
        redactor: Arc::new(Redactor::default()),
        cancel: CancellationToken::new(),
        shell: ctx_core::platform::default_shell_program(),
        max_output_chars: 4_000,
        context_explanation: Arc::new(RwLock::new(None)),
        working_set: Arc::new(RwLock::new(Vec::new())),
        web_search: ctx_core::WebSearchConfig { provider: ctx_core::WebSearchProvider::Duckduckgo, ..Default::default() },
        gateway: None,
        web_fetch: ctx_core::WebFetchConfig::default(),
        files: ctx_tools::SessionFiles::detached(),
    };
    (dir, ctx, ToolRegistry::standard())
}

fn call(name: &str, args: serde_json::Value) -> ToolCall {
    ToolCall { id: format!("call_{name}"), name: name.into(), arguments: args }
}

#[tokio::test]
async fn read_search_and_symbols_work() {
    let (_dir, ctx, tools) = fixture(PermissionProfile::ApprovalRequired, Arc::new(DenyAll));
    let r = tools.execute(&call("read_file", json!({"path": "src/lib.rs", "start_line": 1, "end_line": 3})), &ctx).await;
    assert!(!r.output.is_error, "{}", r.output.content);
    assert!(r.output.content.contains("  1 | pub fn greet"));
    assert!(r.output.content.contains("hash sha256:"));
    assert!(r.output.effects.iter().any(|e| matches!(e, ToolEffect::SourceRead { .. })));
    assert_eq!(ctx.working_set.read().as_slice(), &["src/lib.rs".to_string()]);

    let r = tools.execute(&call("search_code", json!({"query": "shout greeting uppercase"})), &ctx).await;
    assert!(r.output.content.contains("src/lib.rs"), "{}", r.output.content);
    let r = tools.execute(&call("search_code", json!({"query": "Hello, {name}!", "mode": "text"})), &ctx).await;
    assert!(r.output.content.contains("src/lib.rs:2:"), "{}", r.output.content);
    let r = tools.execute(&call("find_symbol", json!({"name": "shout"})), &ctx).await;
    assert!(r.output.content.contains("src/lib.rs:5-7 [function] shout"), "{}", r.output.content);
    let r = tools.execute(&call("find_references", json!({"name": "greet"})), &ctx).await;
    assert!(r.output.content.contains("src/lib.rs:6:"), "{}", r.output.content);
    let r = tools.execute(&call("list_files", json!({"pattern": "*.rs"})), &ctx).await;
    assert!(r.output.content.contains("src/lib.rs") && !r.output.content.contains("README"));
}

#[tokio::test]
async fn reversed_or_empty_read_ranges_do_not_panic() {
    let (dir, ctx, tools) = fixture(PermissionProfile::ApprovalRequired, Arc::new(DenyAll));
    let r = tools.execute(&call("read_file", json!({"path": "src/lib.rs", "start_line": 5, "end_line": 2})), &ctx).await;
    assert!(r.output.is_error && r.output.content.contains("before start_line"), "{}", r.output.content);
    std::fs::write(dir.path().join("src/empty.rs"), "").unwrap();
    let r = tools.execute(&call("read_file", json!({"path": "src/empty.rs"})), &ctx).await;
    assert!(!r.output.is_error, "{}", r.output.content);
    let r = tools.execute(&call("read_file", json!({"path": "src/lib.rs", "start_line": 7, "end_line": 7})), &ctx).await;
    assert!(r.output.content.contains("  7 |"), "{}", r.output.content);
}

#[tokio::test]
async fn policy_denies_and_credential_paths_are_protected() {
    let (_dir, ctx, tools) = fixture(PermissionProfile::ApprovalRequired, Arc::new(DenyAll));
    let r = tools.execute(&call("apply_patch", json!({"path": "src/lib.rs", "edits": [{"old_text": "Hello", "new_text": "Hi"}]})), &ctx).await;
    assert!(r.output.is_error && !r.executed);
    assert!(matches!(r.decision, Decision::RequireApproval { .. }));
    // The explore tool set (sub-agents that only investigate) has no editing
    // tools at all, whatever the profile.
    let explore = ToolRegistry::explore();
    let r = explore.execute(&call("apply_patch", json!({"path": "src/lib.rs", "edits": [{"old_text": "Hello", "new_text": "Hi"}]})), &ctx).await;
    assert!(r.output.content.contains("unknown tool") && !r.executed, "{}", r.output.content);
    let r = explore.execute(&call("execute_command", json!({"command": "echo hi"})), &ctx).await;
    assert!(r.output.content.contains("unknown tool") && !r.executed, "{}", r.output.content);
    assert!(!explore.names().contains(&"write_file") && explore.names().contains(&"read_file"));
    assert!(explore.names().contains(&"web_search"));
    // A declined web search never reaches the network, and the prompt shows
    // the query with credentials redacted.
    let r = tools.execute(&call("web_search", json!({"query": "why does OPENAI_API_KEY=sk-proj-abcdefghijklmnopqrstuvwxyz0123456789 fail"})), &ctx).await;
    assert!(r.output.is_error && !r.executed, "{}", r.output.content);
    assert!(matches!(r.decision, Decision::RequireApproval { .. }));
    assert!(!r.output.content.contains("sk-proj-abcdefghijklmnopqrstuvwxyz0123456789"), "{}", r.output.content);
    // web_fetch never sends a URL carrying a credential or a non-http scheme.
    let r = tools.execute(&call("web_fetch", json!({"url": "https://example.com/?key=sk-proj-abcdefghijklmnopqrstuvwxyz0123456789"})), &ctx).await;
    assert!(r.output.is_error && !r.executed && r.output.content.contains("credential"), "{}", r.output.content);
    let r = tools.execute(&call("web_fetch", json!({"url": "file:///etc/passwd"})), &ctx).await;
    assert!(r.output.is_error && !r.executed, "{}", r.output.content);
    let r = tools.execute(&call("read_file", json!({"path": ".env"})), &ctx).await;
    assert!(r.output.is_error && r.output.content.contains("credential"));
    let r = tools.execute(&call("read_file", json!({"path": "../../etc/passwd"})), &ctx).await;
    assert!(r.output.is_error && !r.executed);
    let r = tools.execute(&call("execute_command", json!({"command": "rm -rf src"})), &ctx).await;
    assert!(r.output.is_error && !r.executed);
    let r = tools.execute(&call("bogus_tool", json!({})), &ctx).await;
    assert!(r.output.content.contains("unknown tool"));
    let r = tools.execute(&ToolCall { id: "m".into(), name: "read_file".into(), arguments: json!({"__malformed_arguments": "{bad", "__reason": "eof"}) }, &ctx).await;
    assert!(r.output.content.contains("not valid JSON"));
}

#[tokio::test]
async fn approvals_gate_edits_and_commands() {
    let recorder = Arc::new(Recording { seen: parking_lot::Mutex::new(vec![]), decision: ApprovalDecision::Approve });
    let (dir, ctx, tools) = fixture(PermissionProfile::ApprovalRequired, recorder.clone());
    tools.execute(&call("read_file", json!({"path": "src/lib.rs"})), &ctx).await;
    let r = tools.execute(&call("apply_patch", json!({"path": "src/lib.rs", "edits": [{"old_text": "Hello, {name}!", "new_text": "Hi, {name}!"}]})), &ctx).await;
    assert!(!r.output.is_error, "{}", r.output.content);
    assert_eq!(r.approval, Some(ApprovalDecision::Approve));
    assert!(r.output.content.contains("-    format!(\"Hello, {name}!\")"));
    assert!(std::fs::read_to_string(dir.path().join("src/lib.rs")).unwrap().contains("Hi, {name}!"));
    assert!(ctx.indexer.as_ref().unwrap().store().search_lexical("\"hi\"", 5).unwrap().iter().any(|h| h.chunk.path == "src/lib.rs"), "index updated after edit");
    let seen = recorder.seen.lock();
    assert_eq!(seen.len(), 1);
    assert!(seen[0].details.contains("+    format!(\"Hi, {name}!\")"), "approval shows the diff: {}", seen[0].details);
    drop(seen);

    // Allowed prefix skips approval; unknown command asks; denial is reported.
    let r = tools.execute(&call("execute_command", json!({"command": "echo hello"})), &ctx).await;
    assert!(r.output.content.contains("hello") && r.approval.is_none());
    let denier = Arc::new(Recording { seen: parking_lot::Mutex::new(vec![]), decision: ApprovalDecision::Deny });
    let (_dir2, ctx2, tools2) = fixture(PermissionProfile::ApprovalRequired, denier);
    let r = tools2.execute(&call("write_file", json!({"path": "src/new.rs", "content": "// x\n"})), &ctx2).await;
    assert!(r.output.is_error && r.output.content.contains("declined"));
    assert!(!ctx2.root.join("src/new.rs").exists());
}

#[tokio::test]
async fn stale_patches_are_rejected_and_commands_time_out() {
    let (dir, ctx, tools) = fixture(PermissionProfile::WorkspaceAutonomous, Arc::new(AutoApprove));
    // An edit of a file the session never read is refused, and so is
    // overwriting one.
    let r = tools.execute(&call("apply_patch", json!({"path": "src/lib.rs", "edits": [{"old_text": "Hello", "new_text": "Hi"}]})), &ctx).await;
    assert!(r.output.is_error && r.output.content.contains("has not been read"), "{}", r.output.content);
    let r = tools.execute(&call("write_file", json!({"path": "src/lib.rs", "content": "x", "overwrite": true})), &ctx).await;
    assert!(r.output.is_error && r.output.content.contains("has not been read"), "{}", r.output.content);
    let r = tools.execute(&call("read_file", json!({"path": "src/lib.rs"})), &ctx).await;
    let hash = r.output.content.split("hash ").nth(1).unwrap().split(|c: char| c == ',' || c.is_whitespace()).next().unwrap().to_string();
    // Someone else edits the file: the edit is refused with or without the hash.
    std::fs::write(dir.path().join("src/lib.rs"), "pub fn greet(name: &str) -> String {\n    format!(\"Hello, {name}!!\")\n}\n").unwrap();
    let r = tools.execute(&call("apply_patch", json!({"path": "src/lib.rs", "edits": [{"old_text": "Hello", "new_text": "Hi"}], "expected_file_hash": hash})), &ctx).await;
    assert!(r.output.is_error && r.output.content.contains("changed since you last read it"), "{}", r.output.content);
    let r = tools.execute(&call("apply_patch", json!({"path": "src/lib.rs", "edits": [{"old_text": "Hello", "new_text": "Hi"}]})), &ctx).await;
    assert!(r.output.is_error && r.output.content.contains("changed since you last read it"), "{}", r.output.content);
    tools.execute(&call("read_file", json!({"path": "src/lib.rs"})), &ctx).await;
    let r = tools.execute(&call("apply_patch", json!({"path": "src/lib.rs", "edits": [{"old_text": "Hello, {name}!\")", "new_text": "x"}]})), &ctx).await;
    assert!(r.output.is_error && r.output.content.contains("not found"), "{}", r.output.content);

    let r = tools.execute(&call("execute_command", json!({"command": "sleep 5", "timeout_secs": 1})), &ctx).await;
    assert!(r.output.is_error && r.output.content.contains("TIMED OUT"));
    let r = tools.execute(&call("run_validation", json!({"kind": "test"})), &ctx).await;
    assert!(!r.output.is_error);
    assert!(r.output.effects.iter().any(|e| matches!(e, ToolEffect::Validation { record } if record.passed)));
    let r = tools.execute(&call("run_validation", json!({"command": "sh -c 'echo failing; exit 3'"})), &ctx).await;
    assert!(r.output.content.contains("FAILED (exit 3"), "{}", r.output.content);
    let r = tools.execute(&call("write_file", json!({"path": "src/new.rs", "content": "pub fn x() {}\n"})), &ctx).await;
    assert!(!r.output.is_error);
    let r = tools.execute(&call("write_file", json!({"path": "src/new.rs", "content": "y"})), &ctx).await;
    assert!(r.output.is_error && r.output.content.contains("already exists"));
    let profile = RepositoryDiscovery::profile(&ctx.root, &[], ctx.indexer.as_ref().unwrap().registry()).unwrap();
    assert_eq!(profile.file_count, 0);
}

#[tokio::test]
async fn backgrounded_processes_never_wedge_a_command() {
    let (_dir, ctx, tools) = fixture(PermissionProfile::WorkspaceAutonomous, Arc::new(AutoApprove));
    // `a && b > log &` backgrounds the whole list in a subshell. bash keeps
    // that subshell, and with it the output pipe, open; dash execs `b` in
    // its place and releases the pipe. Either way the call returns once the
    // shell exits.
    let started = std::time::Instant::now();
    let r = tools.execute(&call("execute_command", json!({"command": "echo ready && sleep 30 > /dev/null 2>&1 &", "timeout_secs": 20})), &ctx).await;
    assert!(started.elapsed() < Duration::from_secs(10), "took {:?}", started.elapsed());
    assert!(!r.output.is_error && r.output.content.contains("ready"), "{}", r.output.content);

    // A background process that inherits the pipe holds it open in any
    // shell: the call still returns, and says the process keeps running.
    let started = std::time::Instant::now();
    let r = tools.execute(&call("execute_command", json!({"command": "echo ready && sleep 30 &", "timeout_secs": 20})), &ctx).await;
    assert!(started.elapsed() < Duration::from_secs(10), "took {:?}", started.elapsed());
    assert!(!r.output.is_error && r.output.content.contains("ready") && r.output.content.contains("still running"), "{}", r.output.content);

    // A timeout kills the whole process group, background children too.
    let started = std::time::Instant::now();
    let r = tools.execute(&call("execute_command", json!({"command": "sleep 30 & sleep 30", "timeout_secs": 1})), &ctx).await;
    assert!(started.elapsed() < Duration::from_secs(10), "took {:?}", started.elapsed());
    assert!(r.output.is_error && r.output.content.contains("TIMED OUT"), "{}", r.output.content);

    // Stop interrupts a running command.
    let cancel = ctx.cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        cancel.cancel();
    });
    let started = std::time::Instant::now();
    let r = tools.execute(&call("execute_command", json!({"command": "sleep 30", "timeout_secs": 60})), &ctx).await;
    assert!(started.elapsed() < Duration::from_secs(10), "took {:?}", started.elapsed());
    assert!(r.output.is_error && r.output.content.contains("cancelled"), "{}", r.output.content);
}

#[tokio::test]
async fn powershell_runs_beside_bash_without_its_quoting() {
    let (_dir, ctx, tools) = fixture(PermissionProfile::WorkspaceAutonomous, Arc::new(AutoApprove));
    // What broke when the script went through bash first: a `$` variable
    // and PowerShell's doubled single quote.
    let r = tools.execute(&call("execute_command", json!({"command": "$x = 'it''s'; Write-Output \"[$x]\"", "shell": "powershell"})), &ctx).await;
    if ctx_core::platform::powershell_alongside().is_none() {
        // Offered only on Windows next to a POSIX shell; elsewhere the schema has no `shell`.
        assert!(r.output.is_error, "{}", r.output.content);
        return;
    }
    assert!(!r.output.is_error && r.output.content.contains("[it's]"), "{}", r.output.content);
    let r = tools.execute(&call("execute_command", json!({"command": "exit 3", "shell": "powershell"})), &ctx).await;
    assert!(r.output.content.contains("(exit 3,"), "{}", r.output.content);
}

#[tokio::test]
async fn validation_override_is_gated_like_any_command() {
    let (_dir, ctx, tools) = fixture(PermissionProfile::WorkspaceAutonomous, Arc::new(DenyAll));
    // Detected validation command: allowed without a prompt.
    let r = tools.execute(&call("run_validation", json!({"kind": "test"})), &ctx).await;
    assert!(r.executed && !r.output.is_error, "{}", r.output.content);
    // A model-supplied unrecognised command needs approval (denied here).
    let r = tools.execute(&call("run_validation", json!({"command": "python3 -c 'print(1)'"})), &ctx).await;
    assert!(!r.executed, "{}", r.output.content);
    assert!(matches!(r.decision, Decision::RequireApproval { .. }), "{:?}", r.decision);
    // The same override that equals a detected command is still validation.
    let r = tools.execute(&call("run_validation", json!({"command": "sh -c 'exit 0'"})), &ctx).await;
    assert!(r.executed, "{}", r.output.content);
}

#[tokio::test]
async fn memory_and_context_inspection() {
    let (_dir, ctx, tools) = fixture(PermissionProfile::WorkspaceAutonomous, Arc::new(DenyAll));
    let memory = ctx.memory.clone().unwrap();
    memory.remember(ctx_memory::MemoryRecord::new(memory.repository_id().clone(), ctx_memory::MemoryCategory::ProjectKnowledge, "greet lives in src/lib.rs", ctx_memory::VerificationStatus::SourceDerived)).unwrap();
    let r = tools.execute(&call("retrieve_memory", json!({"query": "greet"})), &ctx).await;
    assert!(r.output.content.contains("greet lives in src/lib.rs"), "{}", r.output.content);
    let r = tools.execute(&call("inspect_context", json!({"what": "freshness"})), &ctx).await;
    assert!(r.output.content.contains("Working-set freshness"));
    let r = tools.execute(&call("inspect_context", json!({})), &ctx).await;
    assert!(r.output.content.contains("No context explanation"));
}
