//! Sub-agents (spec §10.8): the tools the Ultra effort tier gives the lead
//! model to deploy sub-agents and talk to them (after Codex's
//! collaboration tools), and the plumbing that runs each agent as a
//! bounded [`AgentRuntime`] of its own and streams its events back through
//! the parent task.
//!
//! - `spawn_agents` deploys agents and waits for their reports, or with
//!   `background` returns their ids at once while they work.
//! - `send_input` messages an agent: a working agent reads it before its
//!   next step (`interrupt` stops the current turn first), one that has
//!   reported takes it up as a follow-up turn of its own conversation.
//! - `wait_agents` waits until agents report (any or all, with a timeout).
//! - `read_agent` reads an agent's status, recent activity and latest
//!   report without waiting; without an id it lists every agent.
//! - `close_agent` stops an agent and frees its slot.
//!
//! An agent is a fresh conversation in its own session (mode `agent`,
//! hidden from session listings, a child of the lead's session) with only
//! the brief the lead agent wrote; its follow-up turns continue that
//! conversation. `explore` agents get the inspection tool set and cannot
//! edit or run commands; `implement` agents get the full set under the
//! parent's permission profile, so every approval the user would see from
//! the lead agent is still asked. Agents never spawn agents.
//!
//! Agents outlive the lead's turn: each run restores the agents of its
//! session from their sessions, so the lead can message one it deployed
//! turns ago. Only their turns are bound to a run: the ones still working
//! when it ends are stopped (and can be messaged again later), and
//! `close_agent` retires an agent for good (`metadata.closed`).

use crate::inbox::Inbox;
use crate::runtime::{AgentRuntime, RunOptions};
use async_trait::async_trait;
use ctx_core::{AgentEvent, AgentMode, AgentReport, CancellationToken, CtxError, Result, SessionId, TaskId, TaskStatus, TokenUsage, UsageSource, UserMessage};
use ctx_security::{Action, ActionKind};
use ctx_storage::SessionStore;
use ctx_telemetry::EventLog;
use ctx_tools::{Tool, ToolContext, ToolEffect, ToolOutput, ToolRegistry};
use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch, Semaphore};
use tokio::task::JoinHandle;

pub const SPAWN_TOOL: &str = "spawn_agents";
pub const SEND_TOOL: &str = "send_input";
pub const WAIT_TOOL: &str = "wait_agents";
pub const READ_TOOL: &str = "read_agent";
pub const CLOSE_TOOL: &str = "close_agent";
/// The lead agent's whole sub-agent tool set.
pub const AGENT_TOOLS: [&str; 5] = [SPAWN_TOOL, SEND_TOOL, WAIT_TOOL, READ_TOOL, CLOSE_TOOL];

/// `wait_agents` waits this long unless told otherwise, and never longer
/// than [`MAX_WAIT`].
const DEFAULT_WAIT: Duration = Duration::from_secs(300);
const MAX_WAIT: Duration = Duration::from_secs(1800);
/// Lines of recent activity `read_agent` keeps per agent.
const ACTIVITY_LINES: usize = 12;
/// How long closing an agent waits for its turn to wind down.
const CLOSE_WAIT: Duration = Duration::from_secs(10);

/// The agents of one lead run and everything needed to run them.
pub(crate) struct AgentHost {
    /// A clone of the lead runtime (shared repository, provider, policy and
    /// remembered approvals); each agent turn is a child of it.
    parent: AgentRuntime,
    sink: Option<mpsc::Sender<AgentEvent>>,
    events: EventLog,
    store: Arc<SessionStore>,
    model: String,
    max_per_call: usize,
    max_steps: u32,
    /// The lead run's cancellation; every agent turn is a child of it.
    cancel: CancellationToken,
    /// Agent turns that may work at the same time.
    slots: Arc<Semaphore>,
    agents: Mutex<Vec<Arc<Agent>>>,
    /// Bumped whenever an agent's phase changes, for `wait_agents`.
    changed: watch::Sender<u64>,
}

struct Agent {
    id: String,
    name: String,
    mode: AgentMode,
    session_id: String,
    parent_session: SessionId,
    parent_task: TaskId,
    /// Messages for the working turn; open while the agent is queued or
    /// running.
    inbox: Inbox,
    state: Mutex<AgentState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Phase {
    /// Waiting for a free slot.
    #[default]
    Queued,
    Running,
    /// Reported; a message starts its next turn.
    Idle,
    Closed,
}

impl Phase {
    fn settled(self) -> bool {
        matches!(self, Phase::Idle | Phase::Closed)
    }
}

#[derive(Default)]
struct AgentState {
    phase: Phase,
    turns: u32,
    /// The current turn's cancellation.
    cancel: Option<CancellationToken>,
    /// Messages for the turn after the current one.
    pending: Vec<String>,
    /// Reports the lead agent has not been handed yet.
    unreported: Vec<AgentReport>,
    latest: Option<AgentReport>,
    /// The current turn's recent tool calls and messages, and its latest
    /// model text.
    activity: VecDeque<String>,
    text: String,
    tool_calls: u32,
    task: Option<JoinHandle<()>>,
}

impl AgentState {
    fn status(&self) -> &'static str {
        match self.phase {
            Phase::Queued => "queued",
            Phase::Running => "running",
            Phase::Idle => self.latest.as_ref().map(|r| r.status.as_str()).unwrap_or("idle"),
            Phase::Closed => "closed",
        }
    }
}

impl Agent {
    fn report(&self, status: TaskStatus, summary: String, started: Instant) -> AgentReport {
        AgentReport {
            agent_id: self.id.clone(),
            name: self.name.clone(),
            mode: self.mode,
            status,
            summary,
            modified_files: Vec::new(),
            unresolved: Vec::new(),
            steps: 0,
            tool_calls: 0,
            usage: TokenUsage::estimated(0, 0),
            cost_usd: None,
            duration_ms: started.elapsed().as_millis() as u64,
        }
    }

    /// Keeps the recent activity `read_agent` shows.
    fn observe(&self, event: &AgentEvent) {
        let mut s = self.state.lock();
        let line = match event {
            AgentEvent::ToolCallStarted { name, arguments, .. } => {
                s.tool_calls += 1;
                format!("→ {name} {}", clip(&arguments.to_string(), 100))
            }
            AgentEvent::ToolCallFinished { success, summary, .. } => format!("  {} {}", if *success { "✓" } else { "✗" }, clip(summary, 100)),
            AgentEvent::Steered { text, .. } => format!("← message from the lead: {}", clip(text, 100)),
            AgentEvent::ModelResponse { text: Some(text), .. } if !text.trim().is_empty() => {
                s.text = text.trim().to_string();
                return;
            }
            _ => return,
        };
        if s.activity.len() == ACTIVITY_LINES {
            s.activity.pop_front();
        }
        s.activity.push_back(line);
    }
}

impl AgentHost {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(parent: AgentRuntime, sink: Option<mpsc::Sender<AgentEvent>>, events: EventLog, store: Arc<SessionStore>, model: String, max_parallel: usize, max_per_call: usize, max_steps: u32) -> Self {
        let cancel = parent.cancel_token();
        Self { parent, sink, events, store, model, max_per_call, max_steps, cancel, slots: Arc::new(Semaphore::new(max_parallel.max(1))), agents: Mutex::new(Vec::new()), changed: watch::channel(0).0 }
    }

    /// The lead agent's tools for deploying and talking to agents.
    pub(crate) fn tools(self: &Arc<Self>) -> Vec<Arc<dyn Tool>> {
        vec![Arc::new(SpawnAgents { host: self.clone() }), Arc::new(SendInput { host: self.clone() }), Arc::new(WaitAgents { host: self.clone() }), Arc::new(ReadAgent { host: self.clone() }), Arc::new(CloseAgent { host: self.clone() })]
    }

    fn notify(&self) {
        self.changed.send_modify(|n| *n += 1);
    }

    /// `name [id]` of every agent still queued or running.
    pub(crate) fn working(&self) -> Vec<String> {
        self.agents.lock().iter().filter(|a| !a.state.lock().phase.settled()).map(|a| format!("{} [{}]", a.name, a.id)).collect()
    }

    /// `name [id] (mode, status)` of every agent waiting for a message,
    /// earlier turns' included.
    pub(crate) fn idle(&self) -> Vec<String> {
        self.agents
            .lock()
            .iter()
            .filter_map(|a| {
                let s = a.state.lock();
                (s.phase == Phase::Idle).then(|| format!("{} [{}] ({}, {})", a.name, a.id, a.mode.as_str(), s.status()))
            })
            .collect()
    }

    /// Loads the agents earlier turns of `session` deployed: idle (or
    /// closed), with their latest report, and now working for `task`.
    pub(crate) fn restore(&self, session: &SessionId, task: &TaskId) {
        let children = match self.store.child_sessions(session.as_str()) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(error = %e, "could not load the session's sub-agents");
                return;
            }
        };
        let mut agents = self.agents.lock();
        for child in children {
            let meta = &child.metadata;
            let text = |key: &str| meta.get(key).and_then(|v| v.as_str()).map(str::to_string);
            let (Some(id), Some(name)) = (text("agent_id"), text("name")) else { continue };
            if agents.iter().any(|a| a.id == id) {
                continue;
            }
            let mode = text("mode").as_deref().and_then(AgentMode::parse).unwrap_or_default();
            let agent = Agent { id, name, mode, session_id: child.id.clone(), parent_session: session.clone(), parent_task: task.clone(), inbox: Inbox::new(), state: Mutex::new(AgentState::default()) };
            let task = child.active_task_id.as_deref().and_then(|t| self.store.get_task(t).ok().flatten());
            {
                let mut s = agent.state.lock();
                s.phase = if meta.get("closed").and_then(|v| v.as_bool()).unwrap_or(false) { Phase::Closed } else { Phase::Idle };
                s.latest = task.map(|t| AgentReport {
                    status: t.status.as_deref().and_then(parse_status).unwrap_or(TaskStatus::Cancelled),
                    ..agent.report(TaskStatus::Cancelled, t.summary.unwrap_or_default(), Instant::now())
                });
            }
            agents.push(Arc::new(agent));
        }
    }

    /// Stops every agent when the lead's run ends and hands back the
    /// reports the lead never collected (their usage still counts).
    pub(crate) async fn shutdown(&self) -> Vec<AgentReport> {
        let agents = self.agents.lock().clone();
        let mut tasks = Vec::new();
        for agent in &agents {
            let mut s = agent.state.lock();
            s.phase = Phase::Closed;
            if let Some(c) = &s.cancel {
                c.cancel();
            }
            tasks.extend(s.task.take());
        }
        for task in tasks {
            let _ = tokio::time::timeout(CLOSE_WAIT, task).await;
        }
        self.notify();
        agents.iter().flat_map(|a| std::mem::take(&mut a.state.lock().unreported)).collect()
    }

    /// An agent by id, or by name when only one has it.
    fn find(&self, key: &str) -> Result<Arc<Agent>> {
        let key = key.trim();
        let agents = self.agents.lock();
        if let Some(a) = agents.iter().find(|a| a.id == key) {
            return Ok(a.clone());
        }
        let named: Vec<_> = agents.iter().filter(|a| a.name.eq_ignore_ascii_case(key)).collect();
        if named.len() == 1 {
            return Ok(named[0].clone());
        }
        let known = agents.iter().map(|a| format!("{} [{}]", a.name, a.id)).collect::<Vec<_>>().join(", ");
        Err(CtxError::InvalidArgument(if named.is_empty() {
            format!("no agent `{key}` in this run (agents: {})", if known.is_empty() { "none" } else { &known })
        } else {
            format!("several agents are named `{key}`; use the id ({known})")
        }))
    }

    /// Agents worth waiting for: working, or with reports not collected.
    fn awaitable(&self) -> Vec<Arc<Agent>> {
        self.agents
            .lock()
            .iter()
            .filter(|a| {
                let s = a.state.lock();
                !s.phase.settled() || !s.unreported.is_empty()
            })
            .cloned()
            .collect()
    }

    /// A new agent in its own session, working on `brief` at once.
    fn spawn(self: &Arc<Self>, name: String, mode: AgentMode, brief: String, ctx: &ToolContext) -> Result<Arc<Agent>> {
        let id = format!("agent-{}", TaskId::new().short());
        let parent_session = SessionId::from_string(ctx.session_id.as_str());
        let metadata = json!({"parent_session": parent_session.as_str(), "parent_task": ctx.task_id.as_str(), "agent_id": id, "name": name, "mode": mode.as_str()});
        let session = self.parent.sessions().start_with_metadata(SessionStore::AGENT_MODE, &self.model, metadata).map_err(|e| CtxError::tool(format!("could not start the session of agent {name}: {e}")))?;
        let agent = Arc::new(Agent { id, name, mode, session_id: session.id, parent_session, parent_task: ctx.task_id.clone(), inbox: Inbox::new(), state: Mutex::new(AgentState::default()) });
        self.agents.lock().push(agent.clone());
        self.start(&agent, &mut agent.state.lock(), brief);
        Ok(agent)
    }

    /// Starts the agent's next turns with `message`; `s` is its locked
    /// state, settled or new.
    fn start(self: &Arc<Self>, agent: &Arc<Agent>, s: &mut AgentState, message: String) {
        // Messages sent while the turn waits for a slot reach its first step.
        agent.inbox.open();
        s.phase = Phase::Queued;
        s.task = Some(tokio::spawn(agent_loop(self.clone(), agent.clone(), message)));
        self.notify();
    }

    /// Hands `message` to the agent; says what became of it.
    fn send(self: &Arc<Self>, agent: &Arc<Agent>, message: String, interrupt: bool) -> Result<&'static str> {
        const FOLLOW_UP: &str = "it had reported, so your message starts a follow-up turn of its conversation; wait_agents collects the new report";
        let mut s = agent.state.lock();
        match s.phase {
            Phase::Closed => return Err(CtxError::InvalidArgument(format!("{} [{}] is closed", agent.name, agent.id))),
            Phase::Idle => {
                self.start(agent, &mut s, message);
                return Ok(FOLLOW_UP);
            }
            Phase::Running if interrupt => {
                s.pending.push(message);
                if let Some(c) = &s.cancel {
                    c.cancel();
                }
                return Ok("its current turn was stopped; it takes up your message next");
            }
            Phase::Queued | Phase::Running => {}
        }
        drop(s);
        let Err(refused) = agent.inbox.send(UserMessage { text: message, ..Default::default() }) else {
            return Ok("delivered; it reads your message before its next step");
        };
        // The turn ended in the meantime.
        let mut s = agent.state.lock();
        match s.phase {
            Phase::Closed => Err(CtxError::InvalidArgument(format!("{} [{}] is closed", agent.name, agent.id))),
            Phase::Idle => {
                self.start(agent, &mut s, refused.text);
                Ok(FOLLOW_UP)
            }
            Phase::Queued | Phase::Running => {
                s.pending.push(refused.text);
                Ok("its turn is ending; it takes up your message in its next turn")
            }
        }
    }

    /// Waits until every one of `agents` has settled, or with `all` false
    /// until one has a report the lead has not collected. `false` when
    /// `timeout` ran out first.
    async fn wait(&self, agents: &[Arc<Agent>], all: bool, timeout: Option<Duration>, cancel: &CancellationToken) -> Result<bool> {
        let mut changed = self.changed.subscribe();
        let deadline = timeout.map(|t| tokio::time::Instant::now() + t);
        loop {
            let (working, fresh) = agents.iter().fold((0, 0), |(w, f), a| {
                let s = a.state.lock();
                (w + usize::from(!s.phase.settled()), f + usize::from(s.phase.settled() && !s.unreported.is_empty()))
            });
            if working == 0 || (!all && fresh > 0) {
                return Ok(true);
            }
            tokio::select! {
                _ = changed.changed() => {}
                _ = cancel.cancelled() => return Err(CtxError::Cancelled),
                _ = sleep_until(deadline) => return Ok(false),
            }
        }
    }

    /// Stops the agent for good (later turns restore it closed) and waits
    /// briefly for its turn to wind down.
    async fn close(&self, agent: &Agent) {
        match self.store.get_session(&agent.session_id) {
            Ok(Some(mut session)) => {
                if let Some(meta) = session.metadata.as_object_mut() {
                    meta.insert("closed".into(), Value::Bool(true));
                }
                if let Err(e) = self.store.set_session_metadata(&session.id, &session.metadata) {
                    tracing::warn!(error = %e, "could not record that the agent was closed");
                }
            }
            Ok(None) => {}
            Err(e) => tracing::warn!(error = %e, "could not record that the agent was closed"),
        }
        let task = {
            let mut s = agent.state.lock();
            s.phase = Phase::Closed;
            s.pending.clear();
            if let Some(c) = &s.cancel {
                c.cancel();
            }
            s.task.take()
        };
        agent.inbox.close();
        if let Some(task) = task {
            let _ = tokio::time::timeout(CLOSE_WAIT, task).await;
        }
        self.notify();
    }
}

async fn sleep_until(deadline: Option<tokio::time::Instant>) {
    match deadline {
        Some(d) => tokio::time::sleep_until(d).await,
        None => std::future::pending().await,
    }
}

/// An agent's turns: the first message, then the messages that arrived
/// for a next turn, until it reports with nothing waiting or is closed.
async fn agent_loop(host: Arc<AgentHost>, agent: Arc<Agent>, mut message: String) {
    loop {
        let cancel = host.cancel.child_token();
        {
            let mut s = agent.state.lock();
            if s.phase == Phase::Closed {
                cancel.cancel();
            }
            s.cancel = Some(cancel.clone());
        }
        let started = Instant::now();
        let report = tokio::select! {
            permit = host.slots.clone().acquire_owned() => {
                let _permit = permit;
                {
                    let mut s = agent.state.lock();
                    if s.phase != Phase::Closed {
                        s.phase = Phase::Running;
                    }
                    s.turns += 1;
                    s.activity.clear();
                    s.text.clear();
                    s.tool_calls = 0;
                }
                host.notify();
                let report = run_turn(&host, &agent, &message, cancel).await;
                emit(&host, &agent.parent_session, AgentEvent::AgentFinished { task_id: agent.parent_task.clone(), agent_id: agent.id.clone(), report: report.clone() }).await;
                report
            }
            _ = cancel.cancelled() => agent.report(TaskStatus::Cancelled, "stopped before it started".into(), started),
        };
        // Messages the turn never read join the next one.
        agent.inbox.close();
        let unread = agent.inbox.take();
        let next = {
            let mut s = agent.state.lock();
            s.cancel = None;
            s.latest = Some(report.clone());
            s.unreported.push(report);
            s.pending.extend(unread.into_iter().map(|m| m.text));
            if s.phase == Phase::Closed {
                None
            } else if s.pending.is_empty() {
                s.phase = Phase::Idle;
                None
            } else {
                s.phase = Phase::Queued;
                agent.inbox.open();
                Some(std::mem::take(&mut s.pending).join("\n\n"))
            }
        };
        host.notify();
        match next {
            Some(m) => message = m,
            None => return,
        }
    }
}

/// Runs one turn of the agent: a child runtime on the agent's session,
/// its events wrapped into the parent's stream, and a report at the end.
async fn run_turn(host: &Arc<AgentHost>, agent: &Arc<Agent>, message: &str, cancel: CancellationToken) -> AgentReport {
    let started = Instant::now();
    let session = match host.store.get_session(&agent.session_id) {
        Ok(Some(s)) => s,
        Ok(None) => return agent.report(TaskStatus::Failed, "the agent's session is gone".into(), started),
        Err(e) => return agent.report(TaskStatus::Failed, format!("could not load the agent's session: {e}"), started),
    };
    let tools = match agent.mode {
        AgentMode::Explore => ToolRegistry::explore(),
        AgentMode::Implement => ToolRegistry::standard(),
    };
    let (tx, rx) = mpsc::channel::<AgentEvent>(256);
    let options = RunOptions {
        max_steps: Some(host.max_steps),
        auto_validate: false,
        reconcile_index: false,
        // The prompt view shows the lead agent's prompt only.
        debug_prompt: false,
        instructions: Some(format!(
            "You are the sub-agent `{}` ({}) deployed by a lead agent, working inside its task. Follow only your brief and the lead agent's messages, which may arrive while you work. {} \
             Do not ask the user questions: decide, or report what you could not decide. Finish with a concise report the lead agent can act on: \
             what you found or did, the files you changed, what you verified, and open questions.",
            agent.name,
            agent.mode.as_str(),
            match agent.mode {
                AgentMode::Explore => "You can read, search and inspect, but not edit files or run commands; report findings with exact file paths and line references.",
                AgentMode::Implement => "Make the change your brief asks for and verify it where you can.",
            }
        )),
        ..host.parent.options().clone()
    };
    let child = host.parent.child(tools, tx, cancel, options).with_inbox(agent.inbox.clone());
    let forwarder = tokio::spawn(forward(host.clone(), agent.clone(), rx, message.to_string()));
    let outcome = child.run(&session, message).await;
    drop(child);
    let _ = forwarder.await;
    match outcome {
        Ok(o) => AgentReport {
            agent_id: agent.id.clone(),
            name: agent.name.clone(),
            mode: agent.mode,
            status: o.status,
            summary: o.summary,
            modified_files: o.modified_files,
            unresolved: o.unresolved,
            steps: o.steps,
            tool_calls: o.tool_calls,
            usage: o.usage,
            cost_usd: o.cost_usd,
            duration_ms: started.elapsed().as_millis() as u64,
        },
        Err(CtxError::Cancelled) => agent.report(TaskStatus::Cancelled, "cancelled".into(), started),
        Err(e) => agent.report(TaskStatus::Failed, format!("runtime error: {e}"), started),
    }
}

/// Wrap the child's events into the parent task's stream. The first event
/// of a turn (the child's `task_started`) carries the agent's task id,
/// which the `agent_started` announcement needs.
async fn forward(host: Arc<AgentHost>, agent: Arc<Agent>, mut rx: mpsc::Receiver<AgentEvent>, objective: String) {
    let mut announced = false;
    while let Some(ev) = rx.recv().await {
        if !announced {
            announced = true;
            let agent_task_id = ev.task_id().clone();
            if let Ok(Some(mut task)) = host.store.get_task(agent_task_id.as_str()) {
                task.parent_task_id = Some(agent.parent_task.to_string());
                let _ = host.store.update_task(&task);
            }
            emit(&host, &agent.parent_session, AgentEvent::AgentStarted { task_id: agent.parent_task.clone(), agent_id: agent.id.clone(), agent_task_id, name: agent.name.clone(), mode: agent.mode, objective: objective.clone() }).await;
        }
        agent.observe(&ev);
        emit(&host, &agent.parent_session, AgentEvent::AgentProgress { task_id: agent.parent_task.clone(), agent_id: agent.id.clone(), inner: Box::new(ev) }).await;
    }
}

async fn emit(host: &AgentHost, session: &SessionId, event: AgentEvent) {
    if event.is_persistent() {
        if let Err(e) = host.events.record(session, &event) {
            tracing::warn!(error = %e, "failed to persist agent event");
        }
    }
    if let Some(sink) = &host.sink {
        // Deltas are best-effort, everything else waits for the UI.
        if event.is_persistent() {
            let _ = sink.send(event).await;
        } else {
            let _ = sink.try_send(event);
        }
    }
}

/// The tool result on `agents`: every report the lead has not been handed
/// yet (the effects fold their usage, files and open questions into the
/// lead task), then what each agent still working is doing. `detail`
/// lists a working agent's recent activity and repeats a settled agent's
/// latest report.
fn report_on(agents: &[Arc<Agent>], detail: bool) -> (String, Vec<ToolEffect>) {
    let mut content = String::new();
    let mut effects = Vec::new();
    for agent in agents {
        let mut s = agent.state.lock();
        let fresh = std::mem::take(&mut s.unreported);
        for report in &fresh {
            push_report(&mut content, report);
        }
        let turn = if s.turns > 1 { format!(" · turn {}", s.turns) } else { String::new() };
        if !s.phase.settled() {
            content.push_str(&format!("## {} ({}) — {}{turn} · {} tool calls so far · id {}\n", agent.name, agent.mode.as_str(), s.status(), s.tool_calls, agent.id));
            if detail {
                if !s.activity.is_empty() {
                    content.push_str("Recent activity:\n");
                    for line in &s.activity {
                        content.push_str(line);
                        content.push('\n');
                    }
                }
                if !s.text.is_empty() {
                    content.push_str(&format!("Latest message: {}\n", clip(&s.text, 1500)));
                }
            }
            if !s.pending.is_empty() {
                content.push_str(&format!("{} message(s) wait for its next turn.\n", s.pending.len()));
            }
            content.push('\n');
        } else if fresh.is_empty() {
            match (&s.latest, detail) {
                (Some(report), true) => push_report(&mut content, report),
                _ => content.push_str(&format!("## {} ({}) — {}{turn} · id {}\n{}\n\n", agent.name, agent.mode.as_str(), s.status(), agent.id, if s.latest.is_some() { "Its report was already handed to you." } else { "It never reported." })),
            }
        }
        drop(s);
        for report in fresh {
            for path in &report.modified_files {
                effects.push(ToolEffect::FileModified { path: path.clone() });
            }
            effects.push(ToolEffect::Agent { report });
        }
    }
    (content.trim_end().to_string(), effects)
}

fn push_report(content: &mut String, report: &AgentReport) {
    content.push_str(&format!("## {} ({}) — {} in {} · {} steps · {} tool calls · id {}\n", report.name, report.mode.as_str(), report.status.as_str(), fmt_ms(report.duration_ms), report.steps, report.tool_calls, report.agent_id));
    let summary = report.summary.trim();
    content.push_str(if summary.is_empty() { "(no report)" } else { summary });
    content.push('\n');
    if !report.modified_files.is_empty() {
        content.push_str(&format!("Files changed: {}\n", report.modified_files.join(", ")));
    }
    if !report.unresolved.is_empty() {
        content.push_str(&format!("Unresolved: {}\n", report.unresolved.join("; ")));
    }
    content.push('\n');
}

fn output(content: String, effects: Vec<ToolEffect>, summary: String) -> ToolOutput {
    let mut out = ToolOutput::ok(content, summary);
    out.effects = effects;
    out
}

fn reports(effects: &[ToolEffect]) -> impl Iterator<Item = &AgentReport> {
    effects.iter().filter_map(|e| match e {
        ToolEffect::Agent { report } => Some(report),
        _ => None,
    })
}

fn delegate(tool: &str, agents: usize, summary: String, details: String) -> Action {
    Action { tool: tool.into(), kind: ActionKind::Delegate { agents }, summary, details }
}

fn parse<T: serde::de::DeserializeOwned>(tool: &str, args: &Value) -> Result<T> {
    serde_json::from_value(args.clone()).map_err(|e| CtxError::InvalidArgument(format!("{tool}: {e}")))
}

fn parse_status(s: &str) -> Option<TaskStatus> {
    [TaskStatus::Completed, TaskStatus::Blocked, TaskStatus::Failed, TaskStatus::Cancelled].into_iter().find(|t| t.as_str() == s)
}

fn clip(text: &str, max: usize) -> String {
    let text = text.trim();
    match text.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &text[..i]),
        None => text.to_string(),
    }
}

fn fmt_ms(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        format!("{}m {}s", ms / 60_000, (ms % 60_000) / 1000)
    }
}

/// A sub-agent's usage folded into the parent's cumulative counters.
pub(crate) fn add_usage(total: &mut TokenUsage, usage: &TokenUsage) {
    total.prompt_tokens += usage.prompt_tokens;
    total.completion_tokens += usage.completion_tokens;
    total.cached_prompt_tokens = Some(total.cached_prompt_tokens.unwrap_or(0) + usage.cached_prompt_tokens.unwrap_or(0));
    if usage.source == UsageSource::Estimated {
        total.source = UsageSource::Estimated;
    }
}

// ----- spawn_agents --------------------------------------------------------

#[derive(Debug, Deserialize)]
struct AgentSpec {
    name: String,
    #[serde(default)]
    mode: Option<String>,
    objective: String,
}

#[derive(Debug, Deserialize)]
struct SpawnArgs {
    agents: Vec<AgentSpec>,
    #[serde(default)]
    context: Option<String>,
    #[serde(default)]
    background: bool,
}

struct SpawnAgents {
    host: Arc<AgentHost>,
}

#[async_trait]
impl Tool for SpawnAgents {
    fn name(&self) -> &str {
        SPAWN_TOOL
    }

    fn description(&self) -> &str {
        "Deploy sub-agents that work in parallel, each on an independent piece of the task. \
         Each agent starts fresh with nothing but its brief, so write complete, self-contained briefs: the goal, what you already know, \
         the files or areas to look at, and what done looks like. `explore` agents only read, search and inspect (use them to investigate \
         several areas at once); `implement` agents can edit files and run commands under the current permissions. Never give two \
         `implement` agents the same files. Use this when the task splits into pieces that can be worked on at the same time; \
         do the work yourself when it is small or sequential. By default it waits and returns every agent's final report, files changed \
         and open questions. With `background: true` it returns the agents' ids at once so you can keep working: then collect their reports \
         with wait_agents, check on them with read_agent, steer them or ask follow-ups with send_input, and stop them with close_agent."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "agents": {
                    "type": "array",
                    "description": "The agents to deploy (at least one).",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string", "description": "Short role label shown to the user, e.g. `auth-explorer` or `api-implementer`."},
                            "mode": {"type": "string", "enum": ["explore", "implement"], "description": "`explore`: read, search and inspect only. `implement`: full tool set under the current permissions."},
                            "objective": {"type": "string", "description": "The complete, self-contained brief for this agent."}
                        },
                        "required": ["name", "mode", "objective"],
                        "additionalProperties": false
                    }
                },
                "context": {"type": "string", "description": "Background shared by every agent, prepended to each brief."},
                "background": {"type": "boolean", "description": "Return the agents' ids at once instead of waiting for their reports (default false)."}
            },
            "required": ["agents"],
            "additionalProperties": false
        })
    }

    fn action(&self, args: &Value, _ctx: &ToolContext) -> Result<Action> {
        let parsed: SpawnArgs = parse(SPAWN_TOOL, args)?;
        if parsed.agents.is_empty() {
            return Err(CtxError::InvalidArgument("spawn_agents: at least one agent is required".into()));
        }
        if parsed.agents.len() > self.host.max_per_call {
            return Err(CtxError::InvalidArgument(format!("spawn_agents: at most {} agents per call (asked for {})", self.host.max_per_call, parsed.agents.len())));
        }
        for a in &parsed.agents {
            if a.name.trim().is_empty() || a.objective.trim().is_empty() {
                return Err(CtxError::InvalidArgument("spawn_agents: every agent needs a name and an objective".into()));
            }
            if let Some(m) = &a.mode {
                if AgentMode::parse(m).is_none() {
                    return Err(CtxError::InvalidArgument(format!("spawn_agents: unknown mode `{m}` for {} (explore or implement)", a.name)));
                }
            }
        }
        let names: Vec<String> = parsed.agents.iter().map(|a| format!("{} ({})", a.name.trim(), a.mode.as_deref().and_then(AgentMode::parse).unwrap_or_default().as_str())).collect();
        Ok(delegate(SPAWN_TOOL, parsed.agents.len(), format!("deploy {} agent(s): {}", parsed.agents.len(), names.join(", ")), parsed.agents.iter().map(|a| format!("{}: {}", a.name.trim(), a.objective.trim())).collect::<Vec<_>>().join("\n\n")))
    }

    async fn invoke(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let parsed: SpawnArgs = parse(SPAWN_TOOL, &args)?;
        let context = parsed.context.map(|c| c.trim().to_string()).filter(|c| !c.is_empty());
        let mut agents = Vec::new();
        for spec in parsed.agents {
            let brief = match &context {
                Some(c) => format!("{}\n\nBackground from the lead agent:\n{c}", spec.objective.trim()),
                None => spec.objective.trim().to_string(),
            };
            let mode = spec.mode.as_deref().and_then(AgentMode::parse).unwrap_or_default();
            agents.push(self.host.spawn(spec.name.trim().to_string(), mode, brief, ctx)?);
        }
        if parsed.background {
            let list = agents.iter().map(|a| format!("- {} ({}) · id {}", a.name, a.mode.as_str(), a.id)).collect::<Vec<_>>().join("\n");
            let content = format!("Deployed {} agent(s), working in the background:\n{list}\nCollect their reports with wait_agents before you finish; read_agent shows what they are doing, send_input messages them (in later turns too) and close_agent retires them.", agents.len());
            return Ok(ToolOutput::ok(content, format!("{} agent(s) deployed in the background", agents.len())));
        }
        self.host.wait(&agents, true, None, &ctx.cancel).await?;
        let (content, effects) = report_on(&agents, false);
        let count = reports(&effects).count();
        let failed = reports(&effects).any(|r| matches!(r.status, TaskStatus::Failed));
        Ok(output(content, effects, format!("{count} agent(s) reported{}", if failed { " (some failed)" } else { "" })))
    }
}

// ----- send_input ----------------------------------------------------------

#[derive(Debug, Deserialize)]
struct SendArgs {
    id: String,
    message: String,
    #[serde(default)]
    interrupt: bool,
}

struct SendInput {
    host: Arc<AgentHost>,
}

#[async_trait]
impl Tool for SendInput {
    fn name(&self) -> &str {
        SEND_TOOL
    }

    fn description(&self) -> &str {
        "Send a message to one of your sub-agents. A working agent reads it before its next step (to redirect it, add information or narrow \
         its brief); with `interrupt: true` its current turn stops first and it takes up your message at once. An agent that has already \
         reported keeps its conversation, so a message to it starts a follow-up turn (ask it to dig further or fix what it got wrong), \
         including agents you deployed in earlier turns; collect that turn's report with wait_agents. Returns at once."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {"type": "string", "description": "The agent's id (or its name when unique)."},
                "message": {"type": "string", "description": "What to tell the agent."},
                "interrupt": {"type": "boolean", "description": "Stop the agent's current turn first (default false)."}
            },
            "required": ["id", "message"],
            "additionalProperties": false
        })
    }

    fn action(&self, args: &Value, _ctx: &ToolContext) -> Result<Action> {
        let parsed: SendArgs = parse(SEND_TOOL, args)?;
        if parsed.message.trim().is_empty() {
            return Err(CtxError::InvalidArgument("send_input: the message is empty".into()));
        }
        Ok(delegate(SEND_TOOL, 1, format!("message agent {}", parsed.id.trim()), parsed.message.trim().to_string()))
    }

    async fn invoke(&self, args: Value, _ctx: &ToolContext) -> Result<ToolOutput> {
        let parsed: SendArgs = parse(SEND_TOOL, &args)?;
        let agent = self.host.find(&parsed.id)?;
        let what = self.host.send(&agent, parsed.message.trim().to_string(), parsed.interrupt)?;
        Ok(ToolOutput::ok(format!("{} [{}]: {what}.", agent.name, agent.id), format!("messaged {}", agent.name)))
    }
}

// ----- wait_agents ---------------------------------------------------------

#[derive(Debug, Deserialize)]
struct WaitArgs {
    #[serde(default)]
    ids: Vec<String>,
    #[serde(default)]
    all: bool,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

struct WaitAgents {
    host: Arc<AgentHost>,
}

#[async_trait]
impl Tool for WaitAgents {
    fn name(&self) -> &str {
        WAIT_TOOL
    }

    fn description(&self) -> &str {
        "Wait for sub-agents to report and return their reports. By default it waits for the agents still working (or with reports you \
         have not collected) and returns as soon as one of them reports; with `all: true` it waits for every one. It gives up after \
         `timeout_ms` (default 5 minutes, at most 30) and says which agents are still working; wait again, check on them with read_agent \
         or stop them with close_agent."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "ids": {"type": "array", "items": {"type": "string"}, "description": "The agents to wait for (ids, or names when unique); default: every agent still working or with reports not yet collected."},
                "all": {"type": "boolean", "description": "Wait for every listed agent instead of the first to report (default false)."},
                "timeout_ms": {"type": "integer", "minimum": 1, "description": "How long to wait at most, in milliseconds (default 300000)."}
            },
            "additionalProperties": false
        })
    }

    fn action(&self, args: &Value, _ctx: &ToolContext) -> Result<Action> {
        let parsed: WaitArgs = parse(WAIT_TOOL, args)?;
        let who = if parsed.ids.is_empty() { "working agents".to_string() } else { parsed.ids.join(", ") };
        Ok(delegate(WAIT_TOOL, parsed.ids.len(), format!("wait for {who}"), String::new()))
    }

    async fn invoke(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let parsed: WaitArgs = parse(WAIT_TOOL, &args)?;
        let agents = if parsed.ids.is_empty() { self.host.awaitable() } else { parsed.ids.iter().map(|id| self.host.find(id)).collect::<Result<Vec<_>>>()? };
        if agents.is_empty() {
            return Ok(ToolOutput::ok("No agent is working and every report has been collected.", "no agents to wait for"));
        }
        let timeout = parsed.timeout_ms.map(Duration::from_millis).unwrap_or(DEFAULT_WAIT).min(MAX_WAIT);
        let settled = self.host.wait(&agents, parsed.all, Some(timeout), &ctx.cancel).await?;
        let (mut content, effects) = report_on(&agents, false);
        let count = reports(&effects).count();
        if !settled {
            content = format!("Timed out after {} with no new report.\n\n{content}", fmt_ms(timeout.as_millis() as u64));
        }
        Ok(output(content, effects, if settled { format!("{count} report(s)") } else { format!("timed out; {count} report(s)") }))
    }
}

// ----- read_agent ----------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ReadArgs {
    #[serde(default)]
    id: Option<String>,
}

struct ReadAgent {
    host: Arc<AgentHost>,
}

#[async_trait]
impl Tool for ReadAgent {
    fn name(&self) -> &str {
        READ_TOOL
    }

    fn description(&self) -> &str {
        "Check on a sub-agent without waiting: its status, its recent tool calls and latest message while it works, or its latest report \
         once it has reported. Without an id, lists every agent of this conversation (earlier turns' included) with its status and id."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {"type": "string", "description": "The agent's id (or its name when unique); omit to list every agent."}
            },
            "additionalProperties": false
        })
    }

    fn action(&self, args: &Value, _ctx: &ToolContext) -> Result<Action> {
        let parsed: ReadArgs = parse(READ_TOOL, args)?;
        Ok(delegate(READ_TOOL, 0, parsed.id.map(|id| format!("check on agent {}", id.trim())).unwrap_or_else(|| "list agents".into()), String::new()))
    }

    async fn invoke(&self, args: Value, _ctx: &ToolContext) -> Result<ToolOutput> {
        let parsed: ReadArgs = parse(READ_TOOL, &args)?;
        let (agents, detail) = match parsed.id.as_deref().map(str::trim).filter(|id| !id.is_empty()) {
            Some(id) => (vec![self.host.find(id)?], true),
            None => (self.host.agents.lock().clone(), false),
        };
        if agents.is_empty() {
            return Ok(ToolOutput::ok("No agents have been deployed in this run.", "no agents"));
        }
        let summary = match agents.as_slice() {
            [one] => format!("{} {}", one.name, one.state.lock().status()),
            many => format!("{} agent(s)", many.len()),
        };
        let (content, effects) = report_on(&agents, detail);
        Ok(output(content, effects, summary))
    }
}

// ----- close_agent ---------------------------------------------------------

#[derive(Debug, Deserialize)]
struct CloseArgs {
    id: String,
}

struct CloseAgent {
    host: Arc<AgentHost>,
}

#[async_trait]
impl Tool for CloseAgent {
    fn name(&self) -> &str {
        CLOSE_TOOL
    }

    fn description(&self) -> &str {
        "Retire a sub-agent you no longer need (its brief turned out wrong, another agent already covered it, or it is stuck): it stops, \
         frees its slot and takes no further messages, in this turn or later ones. Returns whatever it reported."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {"type": "string", "description": "The agent's id (or its name when unique)."}
            },
            "required": ["id"],
            "additionalProperties": false
        })
    }

    fn action(&self, args: &Value, _ctx: &ToolContext) -> Result<Action> {
        let parsed: CloseArgs = parse(CLOSE_TOOL, args)?;
        Ok(delegate(CLOSE_TOOL, 1, format!("stop agent {}", parsed.id.trim()), String::new()))
    }

    async fn invoke(&self, args: Value, _ctx: &ToolContext) -> Result<ToolOutput> {
        let parsed: CloseArgs = parse(CLOSE_TOOL, &args)?;
        let agent = self.host.find(&parsed.id)?;
        self.host.close(&agent).await;
        let (content, effects) = report_on(std::slice::from_ref(&agent), false);
        Ok(output(content, effects, format!("closed {}", agent.name)))
    }
}
