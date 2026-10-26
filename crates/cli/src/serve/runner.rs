//! One `task/run`: the per-run `AgentRuntime`, event forwarding, approval plumbing and the mock script.

use super::*;
use async_trait::async_trait;
use ctx_core::{FinishReason, ModelRequest, ModelResult, ProviderMetadata, ReasoningEffort, StreamEvent, ToolCall};
use ctx_providers::{EffectiveCapabilities, StreamSink};
use std::sync::atomic::AtomicU64;

// ---------------------------------------------------------------------------
// Runs and event forwarding
// ---------------------------------------------------------------------------

/// Environment variable that paces the mock: the gap between two streamed
/// deltas in milliseconds (the pause before each reply is six times that).
/// Unset or `0` keeps the script instant.
const MOCK_PACE_ENV: &str = "CTX_MOCK_PACE_MS";
/// How long a run waits for a background indexing pass before starting
/// without it: long enough for the usual reconcile, short enough that a
/// first index of a large folder does not hold up the first message.
const INDEX_WAIT_GRACE: Duration = Duration::from_secs(2);

/// The scripted provider used by `--mock`, built fresh for every run.
/// Instant by default (`ctx serve --mock` from a terminal, the tests);
/// paced through `CTX_MOCK_PACE_MS`, which the desktop app sets so the
/// streaming text, the running tool rows and the status bar are visible.
pub(super) fn mock_provider() -> Arc<dyn ModelProvider> {
    mock_provider_for(None)
}

/// [`mock_provider`] for a run at `effort`: under Ultra the script deploys
/// two sub-agents (an explorer that reads the README, an implementer that
/// only inspects git status) so the agent views can be exercised offline.
/// Still no file is ever changed.
pub(super) fn mock_provider_for(effort: Option<ReasoningEffort>) -> Arc<dyn ModelProvider> {
    let script = if effort.is_some_and(|e| e.deploys_agents()) { ultra_mock_script() } else { plain_mock_script() };
    let pace_ms = std::env::var(MOCK_PACE_ENV).ok().and_then(|v| v.trim().parse::<u64>().ok()).unwrap_or(0);
    if pace_ms == 0 {
        return Arc::new(script);
    }
    let delta_gap = Duration::from_millis(pace_ms.min(1_000));
    Arc::new(PacedProvider { inner: script, delta_gap, turn_gap: delta_gap * 6 })
}

fn plain_mock_script() -> MockProvider {
    MockProvider::new(vec![
        MockTurn::Mixed("I'll take a quick look at the repository before answering.".into(), vec![("list_files".into(), json!({"pattern": "*.md", "limit": 20}))]),
        MockTurn::ToolCalls(vec![("read_file".into(), json!({"path": "README.md", "start_line": 1, "end_line": 40}))]),
        MockTurn::Text("Here is what I found: the repository has a README describing the project and a small set of source files. I only read files; nothing was changed.".into()),
    ])
    .with_fallback("Nothing further to do; no files were changed.")
}

/// The lead and both agents share one provider, so the script answers by
/// looking at the request: who is asking (the brief) and what it has seen.
fn ultra_mock_script() -> MockProvider {
    let counter = Arc::new(AtomicU64::new(0));
    MockProvider::dynamic(move |request: &ModelRequest| {
        let reply = |text: Option<&str>, calls: Vec<(&str, Value)>| -> ModelResult {
            let tool_calls: Vec<ToolCall> = calls.into_iter().map(|(name, arguments)| ToolCall { id: format!("mock_call_{}", counter.fetch_add(1, Ordering::SeqCst)), name: name.into(), arguments }).collect();
            let prompt_tokens: u64 = request.messages.iter().map(|m| ctx_core::estimate_tokens(&m.text_for_estimate()) as u64).sum();
            let completion_tokens = text.map(|t| ctx_core::estimate_tokens(t) as u64).unwrap_or(0) + tool_calls.len() as u64 * 20;
            ModelResult {
                finish_reason: if tool_calls.is_empty() { FinishReason::Stop } else { FinishReason::ToolCalls },
                text: text.map(str::to_string),
                tool_calls,
                usage: Some(TokenUsage { prompt_tokens, completion_tokens, cached_prompt_tokens: None, reasoning_tokens: None, source: UsageSource::Reported }),
                provider_metadata: ProviderMetadata { provider: "mock".into(), model: "mock-model".into(), request_id: None, extra: Default::default() },
            }
        };
        let brief = request.messages.iter().find_map(|m| match m { AgentMessage::User(u) => Some(u.text.clone()), _ => None }).unwrap_or_default();
        let has_tool = |name: &str| request.tools.iter().any(|t| t.name == name);
        let saw_result = |tool: &str| request.messages.iter().any(|m| matches!(m, AgentMessage::ToolResult(r) if r.tool_name == tool));
        if has_tool("spawn_agents") {
            if saw_result("spawn_agents") {
                return Ok(reply(Some("Both agents have reported. The README describes the project and the working tree has local changes; I only read files, nothing was changed."), vec![]));
            }
            if saw_result("list_files") {
                return Ok(reply(Some("Two independent questions here, so I'll deploy an explorer for the docs and an implementer to check the working tree."), vec![("spawn_agents", json!({
                    "context": "Offline mock run: read only, change nothing.",
                    "agents": [
                        {"name": "docs-explorer", "mode": "explore", "objective": "MOCK-EXPLORE: read the README and summarize what the project is."},
                        {"name": "tree-checker", "mode": "implement", "objective": "MOCK-IMPLEMENT: report the git status of the working tree without changing anything."}
                    ]
                }))]));
            }
            return Ok(reply(Some("I'll take a quick look at the repository before splitting the work up."), vec![("list_files", json!({"pattern": "*.md", "limit": 20}))]));
        }
        if brief.starts_with("MOCK-EXPLORE") {
            if saw_result("read_file") {
                return Ok(reply(Some("The README introduces the project and its layout; nothing else of note in the first 40 lines."), vec![]));
            }
            return Ok(reply(Some("Reading the README first."), vec![("read_file", json!({"path": "README.md", "start_line": 1, "end_line": 40}))]));
        }
        if brief.starts_with("MOCK-IMPLEMENT") {
            if saw_result("git_status") {
                return Ok(reply(Some("Checked the working tree with git status; I changed nothing."), vec![]));
            }
            return Ok(reply(Some("Checking the working tree."), vec![("git_status", json!({}))]));
        }
        Ok(reply(Some("Nothing further to do; no files were changed."), vec![]))
    })
}

/// Paces the mock like a real endpoint: a short pause before each reply and a
/// small gap between streamed deltas. The unpaced script finishes a whole
/// run in ~100 ms, which never lets a client show streaming text, a running
/// tool row or its status bar; at 20 ms a run takes about 1.4 s.
/// Everything else (results, usage, probe) is the wrapped mock's.
struct PacedProvider {
    inner: MockProvider,
    delta_gap: Duration,
    turn_gap: Duration,
}

#[async_trait]
impl ModelProvider for PacedProvider {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn model(&self) -> &str {
        self.inner.model()
    }

    fn capabilities(&self) -> EffectiveCapabilities {
        self.inner.capabilities()
    }

    async fn complete(&self, request: &ModelRequest, stream: Option<StreamSink>, cancel: &CancellationToken) -> Result<ModelResult> {
        tokio::select! {
            _ = tokio::time::sleep(self.turn_gap) => {}
            _ = cancel.cancelled() => return Err(CtxError::Cancelled),
        }
        let Some(sink) = stream else {
            return self.inner.complete(request, None, cancel).await;
        };
        // The mock pushes its whole stream at once; forward it with gaps
        // after text deltas and only return once everything reached the sink,
        // so the deltas stay ordered before the runtime's `model_response`.
        let (tx, mut rx) = mpsc::channel::<StreamEvent>(256);
        let gap = self.delta_gap;
        let forwarder = tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                let paced = matches!(event, StreamEvent::TextDelta { .. } | StreamEvent::ReasoningDelta { .. });
                if sink.send(event).await.is_err() {
                    break;
                }
                if paced {
                    tokio::time::sleep(gap).await;
                }
            }
        });
        let result = self.inner.complete(request, Some(tx), cancel).await;
        let _ = forwarder.await;
        result
    }

    async fn probe(&self, cancel: &CancellationToken) -> Result<CapabilityProfile> {
        self.inner.probe(cancel).await
    }

    fn apply_profile(&self, profile: &CapabilityProfile) {
        self.inner.apply_profile(profile)
    }
}

/// One run's approvals: each request is filed under the run's session and
/// waits for the client's decision; `ApprovalState::pair` does the
/// check-and-insert under one lock so a decision that raced ahead is never
/// lost. A dropped sender (cancel, close) denies.
pub(super) struct SessionApprovals {
    pub(super) session_id: String,
    pub(super) state: Arc<ApprovalState>,
}

#[async_trait]
impl ApprovalHandler for SessionApprovals {
    async fn request(&self, request: ApprovalRequest) -> ApprovalDecision {
        let (reply, decision) = oneshot::channel();
        self.state.pair(&approval_key(&self.session_id, &request.id), reply);
        decision.await.unwrap_or(ApprovalDecision::Deny)
    }
}

/// One `task/run`: wait (briefly) for background indexing, make sure the provider is
/// probed, run the agent loop with a fresh runtime, then report
/// `task/finished` exactly once — also when the runtime, or anything
/// around it, panics.
pub(super) async fn execute_run(server: Arc<Server>, project: Arc<Project>, session: SessionRecord, user: UserMessage, options: RunOptions, cancel: CancellationToken, inbox: Inbox) {
    let session_id = session.id.clone();
    let project_id = project.id.clone();
    let run = async {
        // The index wait and the probe honour `task/cancel` (and
        // `project/close` / shutdown, whose tokens are this one's parents).
        // A pass still running after the grace keeps going in the
        // background: retrieval uses what is indexed so far and the text
        // tools cover the rest.
        tokio::select! {
            _ = tokio::time::timeout(INDEX_WAIT_GRACE, project.wait_index_idle()) => {}
            _ = cancel.cancelled() => {}
        }
        if cancel.is_cancelled() {
            return Ok((Err(CtxError::Cancelled), None, 0, None));
        }
        let provider: Arc<dyn ModelProvider> = if server.mock {
            mock_provider_for(project.config.read().model.reasoning_effort.clone())
        } else {
            tokio::select! {
                probed = server.ensure_probed(&project, &cancel) => probed?,
                _ = cancel.cancelled() => return Ok((Err(CtxError::Cancelled), None, 0, None)),
            }
            project.provider.read().clone()
        };
        if cancel.is_cancelled() {
            return Ok((Err(CtxError::Cancelled), None, 0, None));
        }
        let store = project.ctx.stores.sessions.clone();
        let requests_before = blocking(move || model_requests_high_water(&store)).await?;
        // The undo point: the working tree before this run, recorded against
        // the session (keyed by the conversation id the run will write
        // after) so `session/rewind` can put it back.
        let store = project.ctx.stores.sessions.clone();
        let sid = session_id.clone();
        let after_message_id = blocking(move || snapshot::last_session_message_id(&store, &sid)).await?;
        let tree = snapshot::take_tree(&project.ctx.root).await;
        let undo_point = (after_message_id, tree.is_some());
        let store = project.ctx.stores.sessions.clone();
        let sid = session_id.clone();
        blocking(move || snapshot::record(&store, &sid, after_message_id, tree.as_deref())).await?;
        let (tx, rx) = mpsc::channel::<AgentEvent>(EVENT_SINK_CAPACITY);
        let forwarder = tokio::spawn(forward_events(server.clone(), project.clone(), session_id.clone(), rx));
        let profile = *project.profile.read();
        // The project's live effort: `config/patch` updates the project's
        // configuration while the repository context keeps its own copy.
        let effort = project.config.read().model.reasoning_effort.clone();
        // A run that started during a background pass leaves the index to
        // it: its own reconcile would wait on the indexer, or walk twice.
        let reconcile_index = options.reconcile_index && !project.indexing();
        let options = RunOptions { live_config: Some(Arc::new(project.config.read().clone())), reconcile_index, ..options };
        let approvals: Arc<dyn ApprovalHandler> = Arc::new(project.remembering.sharing(Arc::new(SessionApprovals { session_id: session_id.clone(), state: project.approvals.clone() })));
        // The desktop client has a browser the agent may drive; the TUI's
        // runtimes never see this tool. Sub-agents keep the standard sets.
        let browser = Arc::new(BrowserTool { server: server.clone(), state: project.browser.clone(), project_id: project_id.clone(), session_id: session_id.clone(), current_url: Mutex::new(None) });
        // The app can also put questions to the user (`ask_user`).
        let ask = Arc::new(AskUserTool { server: server.clone(), state: project.questions.clone(), project_id: project_id.clone(), session_id: session_id.clone() });
        let tools = ctx_tools::ToolRegistry::standard().with_tool(browser).with_tool(ask);
        let base = AgentRuntime::new(project.ctx.clone(), provider, approvals).with_event_sink(tx).with_cancel(cancel.clone()).with_inbox(inbox).with_options(options).with_tools(tools).with_mcp(server.mcp.clone());
        let runtime = base.with_permission_profile(profile).with_reasoning_effort(effort);
        drop(base);
        // The agent loop runs in its own task: a panic inside the runtime,
        // a tool or storage must still end in `task/finished`.
        let agent = tokio::spawn({
            let session = session.clone();
            async move { runtime.run_message(&session, user).await }
        });
        let result = match agent.await {
            Ok(result) => result,
            Err(join) => {
                tracing::error!(error = %join, "agent run did not complete");
                Err(CtxError::other(if join.is_panic() { "agent run panicked" } else { "agent run was aborted" }))
            }
        };
        let seen_task = forwarder.await.ok().flatten();
        Ok::<_, RpcError>((result, seen_task, requests_before, Some(undo_point)))
    };
    // A panic anywhere in the body above (high-water queries, runtime
    // construction, the forwarder join) must still end in `task/finished`
    // and release the slot, not leave the session stuck until the next
    // `running_tasks()` notices a finished handle.
    let run = match futures::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(run)).await {
        Ok(run) => run,
        Err(_) => {
            tracing::error!("run task panicked outside the agent loop");
            Err(RpcError::from(CtxError::other("agent run panicked")))
        }
    };

    let (task_id, status, error, error_kind, undo_point) = match run {
        Ok((Ok(outcome), _, requests_before, undo_point)) => {
            let (error, kind) = if outcome.status == TaskStatus::Failed {
                let store = project.ctx.stores.sessions.clone();
                let task = outcome.task_id.to_string();
                match tokio::task::spawn_blocking(move || last_model_error(&store, &task, requests_before)).await.unwrap_or((None, None)) {
                    (Some(message), kind) => (Some(message), kind),
                    // Failures that are not provider errors (storage, tools,
                    // checkpoints) only leave the runtime's summary.
                    (None, _) => {
                        let message = outcome.summary.trim_start_matches("runtime error: ").trim().to_string();
                        let kind = error_kind_from_message(&message).to_string();
                        (Some(message), Some(kind))
                    }
                }
            } else {
                (None, None)
            };
            (Some(outcome.task_id.to_string()), Some(outcome.status.as_str().to_string()), error, kind, undo_point)
        }
        Ok((Err(e), seen_task, _, undo_point)) => (seen_task, None, Some(e.to_string()), Some(e.code().to_string()), undo_point),
        Err(e) => {
            let kind = e.data.as_ref().and_then(|d| d.get("kind")).and_then(|k| k.as_str()).map(str::to_string);
            (None, None, Some(e.message), kind, None)
        }
    };
    let error = error.map(|e| project.ctx.redactor.redact(&e));
    // The conversation id the prompt became (the handle `session/rewind`
    // takes) and whether a file snapshot exists for it.
    let (message_id, has_snapshot) = match undo_point {
        Some((after, has_snapshot)) => {
            // The files the run wrote (its sub-agents' too): all a rewind
            // past this message puts back, so other sessions' work survives.
            let writes = project.ctx.activity.run_writes(&session_id);
            let store = project.ctx.stores.sessions.clone();
            let sid = session_id.clone();
            let id = tokio::task::spawn_blocking(move || {
                if let Err(e) = snapshot::record_files(&store, &sid, after, &writes) {
                    tracing::warn!(error = %e, "could not record the files the run wrote");
                }
                snapshot::first_user_message_after(&store, &sid, after)
            })
            .await
            .ok()
            .and_then(|r| r.ok())
            .flatten();
            (id, has_snapshot && id.is_some())
        }
        None => (None, false),
    };
    // The frame is queued and the slot released under one lock: every
    // request handler checks the slots under the same lock, so a request a
    // client sends the moment it reads `task/finished` already finds the
    // session idle, and a `task/run` accepted afterwards is always ordered
    // after the frame on the wire.
    {
        let mut slots = project.running.lock();
        server.notify("task/finished", json!({"project_id": project_id, "session_id": session_id, "task_id": task_id, "status": status, "error": error, "error_kind": error_kind, "message_id": message_id, "snapshot": has_snapshot}));
        project.approvals.clear_session(&session_id);
        project.browser.clear_session(&session_id);
        project.questions.clear_session(&session_id);
        slots.remove(&session_id);
    }
}

/// Redact and send one `event` notification, waiting for the stdout queue
/// to drain when the client has fallen behind.
async fn emit_event(server: &Server, project: &Project, session_id: &str, mut event: Value) {
    project.ctx.redactor.redact_json(&mut event);
    server.out.wait_for_room().await;
    server.notify("event", json!({"project_id": project.id, "session_id": session_id, "event": event}));
}

/// Forward runtime events as `event` notifications and keep the approval
/// bookkeeping in step (also for a sub-agent's approvals, which arrive
/// wrapped in `agent_progress`). Returns the task id the run worked on.
pub(super) async fn forward_events(server: Arc<Server>, project: Arc<Project>, session_id: String, mut rx: mpsc::Receiver<AgentEvent>) -> Option<String> {
    let mut task_id: Option<String> = None;
    while let Some(ev) = rx.recv().await {
        let tid = ev.task_id().as_str().to_string();
        if task_id.as_deref() != Some(tid.as_str()) {
            task_id = Some(tid.clone());
            if let Some(r) = project.running.lock().get_mut(&session_id) {
                r.task_id = Some(tid.clone());
            }
        }
        match ev.innermost() {
            AgentEvent::ApprovalRequested { request, .. } => project.approvals.announce(&approval_key(&session_id, &request.id)),
            AgentEvent::ApprovalResolved { request_id, .. } => project.approvals.clear(&approval_key(&session_id, request_id)),
            _ => {}
        }
        match serde_json::to_value(&ev) {
            Ok(v) => emit_event(&server, &project, &session_id, v).await,
            Err(e) => tracing::warn!(error = %e, "could not serialize event"),
        }
    }
    task_id
}

/// The highest `model_requests` id in the store, so a failed run only ever
/// reports an error recorded by this run (a continued task keeps the rows
/// of its earlier runs).
pub(super) fn model_requests_high_water(store: &SessionStore) -> Result<i64> {
    store.database().with(|c| c.query_row("SELECT COALESCE(MAX(id), 0) FROM model_requests", [], |r| r.get::<_, i64>(0)))
}

/// A failed run records the provider error in its last `model_requests` row.
pub(super) fn last_model_error(store: &SessionStore, task_id: &str, after_id: i64) -> (Option<String>, Option<String>) {
    let row = store.database().with(|c| {
        let mut stmt = c.prepare("SELECT error FROM model_requests WHERE task_id = ?1 AND id > ?2 AND error IS NOT NULL ORDER BY id DESC LIMIT 1")?;
        let mut rows = stmt.query_map((task_id, after_id), |r| r.get::<_, String>(0))?;
        rows.next().transpose()
    });
    match row {
        Ok(Some(message)) => {
            let kind = error_kind_from_message(&message).to_string();
            (Some(message), Some(kind))
        }
        _ => (None, None),
    }
}

/// `error_kind` for a failure message: the provider error codes by their
/// display prefixes, then the other `CtxError` codes (a runtime summary is
/// `runtime error: <CtxError>`), else `other`.
pub(super) fn error_kind_from_message(message: &str) -> &'static str {
    let mut text = message.trim_start();
    // "retries exhausted after N attempts: <last error>" wraps the cause.
    if text.starts_with("retries exhausted") {
        if let Some((_, last)) = text.split_once(": ") {
            text = last.trim_start();
        }
    }
    const PREFIXES: &[(&str, &str)] = &[
        ("authentication failed", "provider_auth"),
        ("rate limited", "provider_rate_limited"),
        ("provider request timed out", "provider_timeout"),
        ("connection failure", "provider_connection"),
        ("stream interrupted", "provider_stream_interrupted"),
        ("context length exceeded", "provider_context_length"),
        ("invalid provider response", "provider_invalid_response"),
        ("model returned malformed tool call", "provider_malformed_tool_call"),
        ("capability not supported", "provider_unsupported"),
        ("provider api error", "provider_api"),
        ("discount dropped", "provider_discount_drop"),
        ("configuration error", "config"),
        ("storage error", "storage"),
        ("tool error", "tool"),
        ("permission denied", "permission_denied"),
        ("invalid argument", "invalid_argument"),
        ("not found", "not_found"),
        ("stale source", "stale_source"),
        ("conflict", "conflict"),
        ("operation cancelled", "cancelled"),
        ("timed out after", "timeout"),
        ("io error", "io"),
        ("serialization error", "serialization"),
        ("index error", "index"),
    ];
    for (prefix, kind) in PREFIXES {
        if text.starts_with(prefix) {
            return kind;
        }
    }
    if text.starts_with("provider") {
        "provider_api"
    } else {
        "other"
    }
}

pub(super) fn rebuild_index(ctx: &RepositoryContext, progress: &dyn Fn(IndexProgress)) -> Result<ctx_indexing::IndexReport> {
    ctx.stores.index.clear()?;
    if let Some(e) = &ctx.embeddings {
        e.clear()?;
    }
    // Generation is 0 again, so this is a full index plus the usual
    // profile/worktree bookkeeping.
    ctx.ensure_indexed(Some(progress))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ctx_core::RiskLevel;

    fn request(id: &str) -> ApprovalRequest {
        ApprovalRequest { id: id.to_string(), tool: "execute_command".into(), risk: RiskLevel::Execute, summary: "run: cargo test".into(), details: "cargo test".into(), reason: "test".into() }
    }

    /// The runtime announces a request before the pair reaches the receiver
    /// and the client answers immediately: whichever side wins the race,
    /// every oneshot must resolve with the client's decision.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn approval_decisions_are_never_lost_in_the_race() {
        let state = Arc::new(ApprovalState::default());
        let approvals = Arc::new(SessionApprovals { session_id: "s1".into(), state: state.clone() });
        for i in 0..1000 {
            let request_id = format!("approval_call_{i}");
            let id = approval_key("s1", &request_id);
            state.announce(&id);
            let send = {
                let approvals = approvals.clone();
                let req = request(&request_id);
                tokio::spawn(async move { approvals.request(req).await })
            };
            let respond = {
                let state = state.clone();
                let id = id.clone();
                tokio::spawn(async move {
                    if i % 2 == 0 {
                        tokio::task::yield_now().await;
                    }
                    assert!(state.respond(&id, ApprovalDecision::Approve), "{id} was announced, so the decision must be accepted");
                })
            };
            respond.await.unwrap();
            let decision = tokio::time::timeout(Duration::from_secs(2), send).await.unwrap_or_else(|_| panic!("decision for {id} was lost")).unwrap();
            assert_eq!(decision, ApprovalDecision::Approve);
            state.clear(&id);
        }
        assert!(!state.respond("approval_unknown", ApprovalDecision::Deny), "unknown ids are refused");
    }

    /// Request ids repeat across steps (`approval_call_0` for the first
    /// call of every response). When the forwarder lags, the pair of a
    /// later request with the same id can reach the server before the
    /// `approval_resolved` of the earlier one is processed: that stale
    /// `clear` must not drop the new reply channel, which would deny the
    /// request without the user ever seeing it.
    #[tokio::test]
    async fn a_stale_resolution_does_not_drop_a_newer_pending_request() {
        let state = ApprovalState::default();
        let id = "approval_call_0";
        // Step 1: announced, paired, answered by the client.
        state.announce(id);
        let (tx1, rx1) = oneshot::channel();
        state.pair(id, tx1);
        assert!(state.respond(id, ApprovalDecision::Approve));
        assert_eq!(rx1.await.unwrap(), ApprovalDecision::Approve);
        // Step 2's pair arrives before the forwarder processed step 1's
        // `approval_resolved`, then that resolution is processed, then step
        // 2's `approval_requested` reaches the forwarder.
        let (tx2, rx2) = oneshot::channel();
        state.pair(id, tx2);
        state.clear(id);
        state.announce(id);
        assert!(state.respond(id, ApprovalDecision::Deny), "the step-2 request is still known");
        assert_eq!(rx2.await.expect("the step-2 reply channel survived the stale clear"), ApprovalDecision::Deny);
        // The resolution still forgets the client-side bookkeeping.
        state.clear(id);
        assert!(!state.respond(id, ApprovalDecision::Approve), "a resolved id is unknown again");
        // An early decision for a request that is then resolved does not leak
        // into the next request with the same id.
        state.announce(id);
        assert!(state.respond(id, ApprovalDecision::Approve));
        state.clear(id);
        let (tx3, mut rx3) = oneshot::channel();
        state.pair(id, tx3);
        assert!(rx3.try_recv().is_err(), "no stale early decision was applied");
        state.clear_all();
        assert!(rx3.try_recv().is_err());
    }

    /// `task/cancel` while a request is pending: the runtime awaits the
    /// reply channel, which never sees the token, so the cancel drops the
    /// session's pending senders and `SessionApprovals` answers with Deny.
    /// Another session's prompt, with the same request id, stays open.
    #[tokio::test]
    async fn clearing_a_session_drops_only_its_reply_channels() {
        let state = ApprovalState::default();
        let (mine, theirs) = (approval_key("s1", "approval_call_1"), approval_key("s2", "approval_call_1"));
        state.announce(&mine);
        state.announce(&theirs);
        let (tx, rx) = oneshot::channel::<ApprovalDecision>();
        state.pair(&mine, tx);
        let (other_tx, other_rx) = oneshot::channel::<ApprovalDecision>();
        state.pair(&theirs, other_tx);
        state.clear_session("s1");
        assert!(rx.await.is_err(), "the sender was dropped, so SessionApprovals returns Deny");
        assert!(!state.respond(&mine, ApprovalDecision::Approve), "a cancelled request is unknown");
        assert_eq!(state.key_for("approval_call_1").as_deref(), Some(theirs.as_str()), "a client that names no session reaches the one still asking");
        assert!(state.respond(&theirs, ApprovalDecision::Approve));
        assert_eq!(other_rx.await.unwrap(), ApprovalDecision::Approve);
        state.clear_all();
    }

    #[test]
    fn error_kinds_cover_every_provider_and_runtime_prefix() {
        assert_eq!(error_kind_from_message("stream interrupted: eof"), "provider_stream_interrupted");
        assert_eq!(error_kind_from_message("invalid provider response: no choices"), "provider_invalid_response");
        assert_eq!(error_kind_from_message("model returned malformed tool call: bad json"), "provider_malformed_tool_call");
        assert_eq!(error_kind_from_message("discount dropped: 98% off expected, 0% off available"), "provider_discount_drop");
        assert_eq!(error_kind_from_message("storage error: database is locked"), "storage");
        assert_eq!(error_kind_from_message("tool error: boom"), "tool");
        assert_eq!(error_kind_from_message("io error: disk full"), "io");
        assert_eq!(error_kind_from_message("agent run panicked"), "other");
        assert_eq!(error_kind_from_message("retries exhausted after 3 attempts: stream interrupted: eof"), "provider_stream_interrupted");
    }
}
