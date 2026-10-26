//! Interactive terminal session (spec §12.2).

use crate::account::{self, Reply, Request};
use crate::app::{self, App};
use crate::gateway::{self, DeviceCode, GatewayError};
use crate::model_stats::{self, ModelStats};
use crate::models_dev::{self, ModelsDevIndex};
use crate::resume::{self, ResumePicker};
use crate::slash::{self, Suggestion};
use crate::transcript::theme::*;
use crate::transcript::{self, fmt_secs, fmt_tokens, spinner, Entry, Transcript};
use crate::Cli;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind};
use ctx_agent::{AgentRuntime, Inbox};
use ctx_core::{AgentEvent, ApprovalDecision, ApprovalRequest, CancellationToken, Config, ContextStats, CredentialStore, CtxError, PermissionProfile, ReasoningEffort, Result, TaskOutcome, TokenUsage, UserMessage};
use ctx_providers::ModelProvider;
use ctx_storage::{SessionRecord, SessionStore};
use ctx_tools::ChannelApprovals;
use futures::StreamExt;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, oneshot};

/// What a key press asks the event loop to do.
enum Action {
    Run(String),
    /// Hand a message to the running turn (Enter while a task runs).
    Steer(String),
    /// Run a message after the current turn (⌘/ctrl/alt-Enter or tab while a task runs).
    Queue(String),
    ListModels,
    SwitchModel(String),
    SetMode(PermissionProfile),
    SetEffort(Option<ReasoningEffort>),
    NewSession,
    /// `/resume`: list the earlier conversations, narrowed by the filter.
    ListSessions(String),
    /// `/mcp`: the MCP servers' state and tools.
    McpStatus,
    Resume(SessionRecord),
    SignIn,
    SignOut,
    Account(Request),
    ConfirmPlan,
}

/// The `/model` popup: models from the endpoint's `/models` listing,
/// narrowed by a typed filter.
struct ModelPicker {
    models: Vec<String>,
    filter: String,
    selected: usize,
    loading: bool,
    error: Option<String>,
}

impl ModelPicker {
    fn loading() -> Self {
        Self { models: Vec::new(), filter: String::new(), selected: 0, loading: true, error: None }
    }

    fn loaded(&mut self, result: Result<Vec<String>>, current: &str) {
        self.loading = false;
        match result {
            Ok(models) => self.models = models,
            Err(e) => self.error = Some(e.to_string()),
        }
        if !self.models.iter().any(|m| m == current) {
            self.models.insert(0, current.to_string());
        }
        self.selected = self.matches().iter().position(|m| *m == current).unwrap_or(0);
    }

    fn matches(&self) -> Vec<&str> {
        let needle = self.filter.to_lowercase();
        self.models.iter().map(String::as_str).filter(|m| m.to_lowercase().contains(&needle)).collect()
    }

    /// The highlighted model, or the filter text as typed when nothing
    /// listed matches (endpoints do not always list every model they serve).
    fn choice(&self) -> Option<String> {
        self.matches().get(self.selected).map(|m| m.to_string()).or_else(|| Some(self.filter.trim().to_string()).filter(|t| !t.is_empty()))
    }
}

/// The agents view (`/agents`, or ↓ on an empty prompt): every sub-agent
/// deployed in this session, and one agent's own conversation.
struct AgentsView {
    /// Index into `Transcript::agents`.
    selected: usize,
    /// The agent whose conversation is open; `None` shows the list.
    open: Option<String>,
    /// First row of the open conversation shown; kept at the bottom while `follow`.
    scroll: usize,
    follow: bool,
}

impl AgentsView {
    /// Starts on the first agent still running, else the latest one.
    fn new(transcript: &Transcript) -> Self {
        let agents: Vec<&transcript::AgentView> = transcript.agents().collect();
        let selected = agents.iter().position(|a| a.is_running()).unwrap_or(agents.len().saturating_sub(1));
        Self { selected, open: None, scroll: 0, follow: true }
    }

    fn open(&mut self, transcript: &Transcript, index: usize) {
        if let Some(agent) = transcript.agents().nth(index) {
            self.selected = index;
            self.open = Some(agent.agent_id.clone());
            self.scroll = 0;
            self.follow = true;
        }
    }

    fn scroll_by(&mut self, rows: isize) {
        if rows < 0 {
            self.follow = false;
            self.scroll = self.scroll.saturating_sub(rows.unsigned_abs());
        } else {
            self.scroll = self.scroll.saturating_add(rows as usize);
        }
    }
}

/// The ↑/↓ prompt history menu.
struct HistoryMenu {
    /// Index into `State::history`.
    selected: usize,
    /// What was typed before the menu opened; Esc or ↓ past the newest
    /// entry brings it back.
    draft: String,
}

struct State {
    transcript: Transcript,
    tool_log: Vec<String>,
    input: String,
    state: String,
    step: u32,
    task_started: Option<Instant>,
    usage: TokenUsage,
    cost: Option<f64>,
    index_generation: u64,
    /// Startup index progress; `None` once the index is ready.
    indexing: Option<String>,
    running: bool,
    model: String,
    profile: PermissionProfile,
    /// Reasoning effort in force (`None` = the model decides).
    effort: Option<ReasoningEffort>,
    /// Models from the last `/model` listing, for argument completion.
    models_cache: Vec<String>,
    /// ModelGateway's savings and 24 h error rates (`model_stats`, disk-cached for an hour).
    model_stats: ModelStats,
    /// models.dev metadata for the picker's detail card; loaded on the first `/model`.
    models_dev: Option<ModelsDevIndex>,
    /// The endpoint's base URL, which picks the models.dev provider first.
    base_url: String,
    /// Installed apps, for `@` mention completion.
    mcp: Option<Arc<ctx_tools::McpManager>>,
    /// The repository's files, for `@` mention completion.
    files: Option<Arc<ctx_tools::mentions::FileIndex>>,
    /// Highlighted slash-command suggestion.
    completion: usize,
    /// Esc hides suggestions until the input changes.
    completion_dismissed: bool,
    /// Prompts typed in this repository, oldest first.
    history: Vec<String>,
    history_menu: Option<HistoryMenu>,
    agents_view: Option<AgentsView>,
    /// Prompts to run once the current turn ends, oldest first.
    queued: Vec<String>,
    /// Prompts sent to the running turn that the model has not seen yet.
    steering: Vec<String>,
    last_context: Option<ContextStats>,
    pending: Option<(ApprovalRequest, oneshot::Sender<ApprovalDecision>)>,
    picker: Option<ModelPicker>,
    resume_picker: Option<ResumePicker>,
    /// Model being loaded or probed by a `/model` switch.
    switching: Option<String>,
    /// First transcript row shown; kept at the bottom while `follow`.
    scroll: usize,
    follow: bool,
    /// Conversation rows visible in the last frame (one page).
    page: usize,
    show_thinking: bool,
    show_log: bool,
    session: SessionRecord,
    last_outcome: Option<TaskOutcome>,
    quit: bool,
}

impl State {
    fn notice(&mut self, text: impl Into<String>) {
        self.transcript.push(Entry::Notice(text.into()));
    }

    fn error(&mut self, text: impl Into<String>) {
        self.transcript.push(Entry::Error(text.into()));
    }

    fn suggestions(&self) -> Vec<Suggestion> {
        if self.completion_dismissed || self.picker.is_some() || self.resume_picker.is_some() || self.pending.is_some() || self.history_menu.is_some() || self.agents_view.is_some() {
            return Vec::new();
        }
        if !self.input.starts_with('/') {
            let Some((_, partial)) = slash::mention_partial(&self.input) else { return Vec::new() };
            let files = self.files.as_ref().map(|f| f.search(partial.trim_start_matches('"'), 8)).unwrap_or_default();
            return slash::mention_suggestions(&self.input, &self.mention_candidates(), &files);
        }
        slash::suggestions(&self.input, &self.models_cache, self.profile, &self.model, self.effort.as_ref(), self.effort_levels().as_deref())
    }

    /// The installed apps as `@` completions, with their state.
    fn mention_candidates(&self) -> Vec<slash::MentionCandidate> {
        use ctx_tools::McpServerState;
        let Some(mcp) = &self.mcp else { return Vec::new() };
        mcp.status()
            .into_iter()
            .map(|s| {
                let state = match s.state {
                    McpServerState::Connected => format!("{} tools", s.tools.iter().filter(|t| t.enabled).count()),
                    McpServerState::Connecting => "connecting".into(),
                    McpServerState::Disabled => "paused".into(),
                    McpServerState::Failed => "failed".into(),
                };
                let about = s.app.as_deref().and_then(ctx_tools::mcp::catalog_app).map(|a| a.tagline).unwrap_or_else(|| s.target.clone());
                slash::MentionCandidate { name: s.name, state, about }
            })
            .collect()
    }

    /// The current model's effort values (models.dev), `None` when unknown.
    fn effort_levels(&self) -> Option<Vec<String>> {
        ctx_core::effort_levels(&self.model, &self.base_url)
    }

    /// The input changed: start suggestions afresh; editing a recalled
    /// prompt keeps the text and closes the history menu.
    fn edited(&mut self) {
        self.completion = 0;
        self.completion_dismissed = false;
        self.history_menu = None;
    }

    fn remember_input(&mut self, text: &str) {
        if !text.is_empty() && self.history.last().map(String::as_str) != Some(text) {
            self.history.push(text.to_string());
        }
    }

    /// ↑: open the history menu on the newest prompt, or step to an older one.
    fn history_back(&mut self) {
        let selected = match &self.history_menu {
            Some(menu) => menu.selected.saturating_sub(1),
            None if self.history.is_empty() => return,
            None => {
                let draft = std::mem::take(&mut self.input);
                self.history_menu = Some(HistoryMenu { selected: self.history.len() - 1, draft });
                self.history.len() - 1
            }
        };
        self.show_history_entry(selected);
    }

    /// ↓: step to a newer prompt; past the newest, restore the draft.
    fn history_forward(&mut self) {
        let Some(menu) = &self.history_menu else { return };
        if menu.selected + 1 >= self.history.len() {
            self.close_history(true);
        } else {
            let next = menu.selected + 1;
            self.show_history_entry(next);
        }
    }

    fn show_history_entry(&mut self, index: usize) {
        if let Some(menu) = &mut self.history_menu {
            menu.selected = index;
        }
        self.input = self.history[index].clone();
    }

    /// Close the menu, restoring what was typed before it opened if asked.
    fn close_history(&mut self, restore_draft: bool) {
        if let Some(menu) = self.history_menu.take() {
            if restore_draft {
                self.input = menu.draft;
            }
        }
    }

    /// Mouse-wheel scrolling of the conversation (or of the agents view).
    fn scroll_by(&mut self, rows: isize) {
        if let Some(view) = &mut self.agents_view {
            if view.open.is_some() {
                view.scroll_by(rows);
            } else {
                let last = self.transcript.agents().count().saturating_sub(1);
                view.selected = view.selected.saturating_add_signed(rows.signum()).min(last);
            }
            return;
        }
        if rows < 0 {
            self.follow = false;
            self.scroll = self.scroll.saturating_sub(rows.unsigned_abs());
        } else {
            self.scroll = self.scroll.saturating_add(rows as usize);
        }
    }

    /// Switches to `session` with an empty conversation.
    fn reset_conversation(&mut self, session: SessionRecord) {
        self.transcript = Transcript::default();
        self.tool_log.clear();
        self.state = "idle".into();
        self.step = 0;
        self.task_started = None;
        self.usage = TokenUsage::estimated(0, 0);
        self.cost = None;
        self.last_context = None;
        self.last_outcome = None;
        self.scroll = 0;
        self.follow = true;
        self.agents_view = None;
        self.session = session;
    }

    /// Switches to `session` and lays its conversation out again from the
    /// stored events, task by task: prompts, replies, tool calls, sub-agents
    /// and outcomes. A continued task's `task_started` carries the task's
    /// objective, so prompts come from the stored user messages, in order.
    fn load_session(&mut self, store: &SessionStore, session: SessionRecord) -> Result<()> {
        self.reset_conversation(session);
        for task in store.tasks_for_session(&self.session.id)? {
            let mut prompts: std::collections::VecDeque<String> = store
                .messages_for_task(&task.id, 0)?
                .into_iter()
                .filter_map(|r| match serde_json::from_value::<ctx_core::AgentMessage>(r.message).ok()? {
                    ctx_core::AgentMessage::User(u) if !ctx_agent::runtime::is_runtime_note(&u.text) && !ctx_agent::runtime::is_hidden_note(&u.text) => Some(u.text),
                    _ => None,
                })
                .collect();
            for row in store.events_for_task(&task.id, 0, usize::MAX >> 1)? {
                let Ok(event) = serde_json::from_value::<AgentEvent>(row.payload) else { continue };
                match &event {
                    AgentEvent::TaskStarted { objective, .. } => {
                        let prompt = prompts.pop_front().unwrap_or_else(|| objective.clone());
                        self.transcript.push(Entry::User(prompt));
                    }
                    AgentEvent::Steered { text, .. } => {
                        if let Some(i) = prompts.iter().position(|p| p == text) {
                            prompts.remove(i);
                        }
                    }
                    _ => {}
                }
                self.apply(event);
            }
        }
        // A task the process never finished (it was killed) is over now.
        self.transcript.interrupt_running();
        let totals = store.usage_totals(None, Some(&self.session.id))?;
        self.usage = TokenUsage::estimated(totals.prompt_tokens, totals.completion_tokens);
        self.cost = totals.cost();
        self.state = "idle".into();
        self.step = 0;
        self.task_started = None;
        self.tool_log.clear();
        self.follow = true;
        Ok(())
    }

    fn log(&mut self, text: impl Into<String>) {
        self.tool_log.push(text.into());
        if self.tool_log.len() > 400 {
            self.tool_log.remove(0);
        }
    }


    fn apply(&mut self, ev: AgentEvent) {
        match ev {
            AgentEvent::TaskStarted { objective, continued, .. } => {
                self.state = "preparing".into();
                self.step = 0;
                self.task_started = Some(Instant::now());
                self.log(format!("task {}: {}", if continued { "continued" } else { "started" }, objective.chars().take(60).collect::<String>()));
            }
            AgentEvent::StateChanged { to, .. } => self.state = to.as_str().to_lowercase(),
            AgentEvent::ContextCompiled { stats, .. } => {
                self.log(format!("context ~{} tok · {} snippets · {} memories{}", stats.estimated_prompt_tokens, stats.repository_snippets, stats.memory_records, if stats.compacted { " · compacted" } else { "" }));
                self.last_context = Some(stats);
            }
            AgentEvent::ModelRequestStarted { step, .. } => {
                self.step = step;
                self.transcript.model_request_started();
            }
            // Only the desktop app asks for the full prompt.
            AgentEvent::PromptSent { .. } => {}
            AgentEvent::ModelReasoningDelta { text, .. } => self.transcript.reasoning_delta(&text),
            AgentEvent::ModelTextDelta { text, .. } => self.transcript.text_delta(&text),
            AgentEvent::ModelResponse { text, .. } => self.transcript.model_response(text),
            AgentEvent::ToolCallStarted { call_id, name, arguments, .. } => {
                self.log(format!("→ {name} {}", arguments.to_string().chars().take(90).collect::<String>()));
                self.transcript.tool_started(call_id, name, &arguments);
            }
            AgentEvent::ToolCallFinished { call_id, success, summary, duration_ms, .. } => {
                self.log(format!("  {} {} ({duration_ms} ms)", if success { crate::transcript::glyphs().ok } else { crate::transcript::glyphs().fail }, summary.chars().take(90).collect::<String>()));
                self.transcript.tool_finished(&call_id, success, summary, duration_ms);
            }
            AgentEvent::ValidationFinished { record, .. } => {
                self.transcript.push(Entry::Validation { command: record.command, passed: record.passed, summary: record.summary });
            }
            AgentEvent::FileChanged { path, kind, .. } => self.log(format!("  file {kind:?}: {path}")),
            AgentEvent::Checkpoint { reason, .. } => self.log(format!("checkpoint: {reason}")),
            AgentEvent::Warning { message, .. } => self.log(format!("note: {message}")),
            AgentEvent::Usage { cumulative, cost_usd, .. } => {
                self.usage = cumulative;
                self.cost = cost_usd;
            }
            AgentEvent::Steered { text, .. } => {
                if let Some(i) = self.steering.iter().position(|p| *p == text) {
                    self.steering.remove(i);
                }
                self.transcript.push(Entry::User(text));
                self.follow = true;
            }
            AgentEvent::TaskCompleted { outcome, .. } => {
                self.state = outcome.status.as_str().into();
                self.transcript.task_finished(&outcome);
                self.last_outcome = Some(outcome);
            }
            AgentEvent::AgentStarted { agent_id, name, mode, objective, .. } => {
                self.log(format!("{} agent {name} ({}) started: {}", crate::transcript::glyphs().agents, mode.as_str(), objective.lines().next().unwrap_or("").chars().take(60).collect::<String>()));
                self.transcript.agent_started(agent_id, name, mode.as_str().to_string(), objective);
            }
            AgentEvent::AgentProgress { agent_id, inner, .. } => {
                match inner.as_ref() {
                    AgentEvent::ToolCallStarted { name, arguments, .. } => self.log(format!("  [{}] → {name} {}", agent_name(&self.transcript, &agent_id), arguments.to_string().chars().take(70).collect::<String>())),
                    AgentEvent::ToolCallFinished { success, summary, duration_ms, .. } => self.log(format!("  [{}]   {} {} ({duration_ms} ms)", agent_name(&self.transcript, &agent_id), if *success { crate::transcript::glyphs().ok } else { crate::transcript::glyphs().fail }, summary.chars().take(70).collect::<String>())),
                    AgentEvent::FileChanged { path, kind, .. } => self.log(format!("  [{}]   file {kind:?}: {path}", agent_name(&self.transcript, &agent_id))),
                    AgentEvent::Warning { message, .. } => self.log(format!("  [{}]   note: {message}", agent_name(&self.transcript, &agent_id))),
                    _ => {}
                }
                self.transcript.agent_event(&agent_id, &inner);
            }
            AgentEvent::AgentFinished { agent_id, report, .. } => {
                self.log(format!("{} agent {} {}: {} steps, {} tool calls, {} ms", crate::transcript::glyphs().agents, report.name, report.status.as_str(), report.steps, report.tool_calls, report.duration_ms));
                self.transcript.agent_finished(&agent_id, &report);
            }
            AgentEvent::ToolCallOutput { .. } | AgentEvent::ApprovalRequested { .. } | AgentEvent::ApprovalResolved { .. } | AgentEvent::ValidationStarted { .. } => {}
        }
    }
}

/// The display name of a sub-agent in the transcript (its id until the
/// `agent_started` event has been applied).
fn agent_name(transcript: &Transcript, agent_id: &str) -> String {
    transcript
        .entries
        .iter()
        .rev()
        .find_map(|e| match e {
            Entry::Agents(agents) => agents.iter().find(|a| a.agent_id == agent_id).map(|a| a.name.clone()),
            _ => None,
        })
        .unwrap_or_else(|| agent_id.to_string())
}

pub async fn run(cli: &Cli) -> Result<i32> {
    let App { mut config, ctx } = app::open(cli, true)?;
    crate::billing::install(&config);
    let provider = app::provider(&config, true, false).await?;
    // Bring the index up to date behind the UI. A task submitted meanwhile
    // waits for it inside the runtime, which reconciles before each task.
    let (progress_tx, mut progress_rx) = mpsc::channel::<(usize, usize)>(64);
    let index_ctx = ctx.clone();
    let mut startup_index = Some(tokio::task::spawn_blocking(move || {
        index_ctx.ensure_indexed(Some(&|p| {
            let _ = progress_tx.try_send((p.done, p.total));
        }))
    }));
    let (approvals, mut approval_rx) = ChannelApprovals::new();
    let (tx, mut rx) = mpsc::channel::<AgentEvent>(4096);
    let cancel = CancellationToken::new();
    let inbox = Inbox::new();
    let mcp = app::mcp_manager(&config, &ctx.root);
    let mut runtime = Arc::new(AgentRuntime::new(ctx.clone(), provider, Arc::new(approvals)).with_event_sink(tx).with_cancel(cancel.clone()).with_inbox(inbox.clone()).with_mcp(mcp.clone()));
    // Like `claude`: a new conversation unless --continue or --resume asks
    // for an earlier one; `--resume` without an id opens the picker.
    let store = &ctx.stores.sessions;
    let mut open_picker = false;
    let resumed = match cli.resume.as_deref().map(str::trim) {
        Some("") => {
            open_picker = true;
            None
        }
        Some(id) => Some(resume::find(store, ctx.id.as_str(), id)?.ok_or_else(|| CtxError::InvalidArgument(format!("no conversation `{id}` in this repository (or the id matches several); `ctx --resume` lists them")))?),
        None if cli.continue_session => runtime.sessions().resume_latest()?,
        None => None,
    };
    let session = match resumed.clone() {
        Some(s) => s,
        None => fresh_session(&runtime, store, ctx.id.as_str(), &config.model.name, None)?,
    };
    let (index_tx, mut index_rx) = mpsc::channel::<u64>(16);
    let _watcher = if config.index.watch {
        match ctx_indexing::IndexWatcher::start(ctx.indexer.clone(), std::time::Duration::from_secs(300), move |report| {
            let _ = index_tx.try_send(report.generation);
        }) {
            Ok(w) => Some(w),
            Err(e) => {
                tracing::warn!(error = %e, "file watcher unavailable");
                None
            }
        }
    } else {
        None
    };

    let mut state = State {
        mcp: Some(mcp.clone()),
        files: Some(ctx_tools::mentions::FileIndex::new(ctx.indexer.clone())),
        transcript: Transcript::default(),
        tool_log: Vec::new(),
        input: String::new(),
        state: "idle".into(),
        step: 0,
        task_started: None,
        usage: TokenUsage::estimated(0, 0),
        cost: None,
        index_generation: ctx.stores.index.generation().unwrap_or(0),
        indexing: Some("checking index…".into()),
        running: false,
        model: config.model.name.clone(),
        profile: config.permissions.profile,
        effort: config.model.reasoning_effort.clone(),
        models_cache: Vec::new(),
        model_stats: ModelStats::new(),
        models_dev: None,
        base_url: config.model.base_url.clone(),
        completion: 0,
        completion_dismissed: false,
        history: load_history(&ctx),
        history_menu: None,
        agents_view: None,
        queued: Vec::new(),
        steering: Vec::new(),
        last_context: None,
        pending: None,
        picker: None,
        resume_picker: None,
        switching: None,
        scroll: 0,
        follow: true,
        page: 10,
        show_thinking: false,
        show_log: false,
        session: session.clone(),
        last_outcome: None,
        quit: false,
    };
    let mut model_note = if app::remembered_model(&config, &ctx.root).as_deref() == Some(config.model.name.as_str()) && cli.model.is_none() { format!(" · {} (this workspace's model)", config.model.name) } else { String::new() };
    if app::remembered_profile(&config, &ctx.root) == Some(config.permissions.profile) && cli.profile.is_none() {
        model_note.push_str(&format!(" · {} mode (this workspace's)", slash::mode_name(config.permissions.profile)));
    }
    if let Some(effort) = &config.model.reasoning_effort {
        model_note.push_str(&format!(" · {} effort{}", effort.as_str(), if effort.deploys_agents() { " (sub-agents enabled)" } else { "" }));
    }
    let banner = format!("Context · session {}{model_note}\n{}\nEnter runs a task (while one runs: steers it; tab queues) · /resume opens an earlier conversation · /help for commands · ctrl-c cancels, twice to quit", &session.id[..8], ctx.root.display());
    match resumed {
        Some(s) => resume_session(&mut state, store, s),
        None => state.notice(banner),
    }
    if open_picker {
        match resume::list(store, ctx.id.as_str(), Some(&session.id)) {
            Ok(sessions) if sessions.is_empty() => state.notice("no earlier conversations in this repository to resume"),
            Ok(sessions) => state.resume_picker = Some(ResumePicker::new(sessions, "")),
            Err(e) => state.error(format!("could not list the conversations: {e}")),
        }
    }

    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        set_mouse_reporting(false);
        set_key_disambiguation(false);
        ratatui::restore();
        original_hook(info);
    }));
    let mut terminal = ratatui::init();
    set_mouse_reporting(true);
    set_key_disambiguation(true);
    let mut term_events = EventStream::new();
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(80));
    let mut running_task: Option<tokio::task::JoinHandle<Result<TaskOutcome>>> = None;
    let mut models_task: Option<tokio::task::JoinHandle<Result<Vec<String>>>> = None;
    // For the status line's savings; refreshed with every `/model` listing.
    let mut stats_task: Option<tokio::task::JoinHandle<ModelStats>> = {
        let config = config.clone();
        Some(tokio::spawn(async move { model_stats::load(&config).await }))
    };
    let mut models_dev_task: Option<tokio::task::JoinHandle<Option<ModelsDevIndex>>> = None;
    let mut switch_task = None;
    // `/signin`: the device code once the gateway has issued one (shown
    // again by a second `/signin`), and the handshake, which ends with the
    // approved account's name; `/signout`: revocation, then the credential
    // as it was (to put the old endpoint back). Either one ends by
    // rewriting the user configuration file and rebuilding the provider.
    let (code_tx, mut code_rx) = mpsc::channel::<DeviceCode>(1);
    let mut signin_code: Option<DeviceCode> = None;
    let mut signin_task: Option<tokio::task::JoinHandle<std::result::Result<Option<String>, String>>> = None;
    let mut signout_task: Option<tokio::task::JoinHandle<(std::result::Result<bool, GatewayError>, Option<ctx_core::StoredCredential>)>> = None;
    let mut endpoint_task: Option<tokio::task::JoinHandle<(Config, Result<Arc<dyn ModelProvider>>, String)>> = None;
    // `/usage` and `/billing`: one gateway call at a time, and the plan a
    // `/billing plan <id> balance` priced, until `/billing confirm`.
    let mut account_task: Option<tokio::task::JoinHandle<std::result::Result<Reply, String>>> = None;
    let mut pending_plan: Option<i64> = None;
    let mut ctrl_c_armed = false;

    loop {
        terminal.draw(|f| draw(f, &mut state)).map_err(|e| CtxError::other(e.to_string()))?;
        if state.quit {
            break;
        }
        tokio::select! {
            ev = term_events.next() => {
                let Some(Ok(ev)) = ev else { break };
                if let Event::Mouse(mouse) = ev {
                    match mouse.kind {
                        MouseEventKind::ScrollUp => state.scroll_by(-3),
                        MouseEventKind::ScrollDown => state.scroll_by(3),
                        _ => {}
                    }
                }
                if let Event::Key(key) = ev {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }
                    let key = normalize_key(key);
                    match handle_key(&mut state, key, &cancel, &mut ctrl_c_armed) {
                        Some(Action::Run(prompt)) if !state.running => running_task = Some(start_run(&mut state, &runtime, prompt)),
                        Some(Action::Steer(prompt)) => match inbox.send(UserMessage { text: prompt.clone(), ..Default::default() }) {
                            Ok(()) => state.steering.push(prompt),
                            // The turn is wrapping up: the prompt runs right after it.
                            Err(_) => state.queued.push(prompt),
                        },
                        Some(Action::Queue(prompt)) => state.queued.push(prompt),
                        Some(Action::ListModels) => {
                            if stats_task.is_none() {
                                let config = config.clone();
                                stats_task = Some(tokio::spawn(async move { model_stats::load(&config).await }));
                            }
                            if state.models_dev.is_none() && models_dev_task.is_none() {
                                let config = config.clone();
                                models_dev_task = Some(tokio::spawn(async move { models_dev::load(&config).await }));
                            }
                            let config = config.clone();
                            models_task = Some(tokio::spawn(async move { ctx_providers::list_models(&config).await }));
                        }
                        Some(Action::SetMode(profile)) => {
                            runtime = Arc::new(runtime.with_permission_profile(profile));
                            state.profile = profile;
                            let when = if state.running { " (from the next task)" } else { "" };
                            let text = format!("approval mode: {} — {}{when}", slash::mode_name(profile), slash::mode_about(profile));
                            match app::remember_profile(&config, &ctx.root, profile) {
                                Ok(true) => state.notice(format!("{text}\nremembered for this workspace")),
                                Ok(false) => {
                                    let next_time = app::remembered_profile(&config, &ctx.root).unwrap_or(config.permissions.profile);
                                    state.transcript.push(Entry::Warning(format!("{text}. Edits, deletes and commands now run without asking. Bypass is not remembered: this workspace opens in {} mode next time.", slash::mode_name(next_time))));
                                }
                                Err(e) => {
                                    state.log(format!("could not remember the approval mode: {e}"));
                                    state.notice(text);
                                }
                            }
                        }
                        Some(Action::SetEffort(effort)) => {
                            runtime = Arc::new(runtime.with_reasoning_effort(effort.clone()));
                            let when = if state.running { " (from the next task)" } else { "" };
                            let text = format!("reasoning effort: {} — {}{when}", slash::effort_name(effort.as_ref()), slash::effort_about(effort.as_ref()));
                            let remembered = app::remember_effort(&config, &ctx.root, effort.clone());
                            state.effort = effort;
                            match remembered {
                                Ok(()) => state.notice(format!("{text}\nremembered for this workspace")),
                                Err(e) => {
                                    state.log(format!("could not remember the reasoning effort: {e}"));
                                    state.notice(text);
                                }
                            }
                        }
                        Some(Action::NewSession) => match fresh_session(&runtime, &ctx.stores.sessions, ctx.id.as_str(), &state.model, Some(&state.session.id)) {
                            Ok(session) => {
                                let id = session.id[..8].to_string();
                                state.reset_conversation(session);
                                state.notice(format!("new session {id} · /resume goes back to an earlier one"));
                            }
                            Err(e) => state.error(format!("could not start a new session: {e}")),
                        },
                        Some(Action::ListSessions(filter)) => match resume::list(&ctx.stores.sessions, ctx.id.as_str(), Some(&state.session.id)) {
                            Ok(sessions) if sessions.is_empty() => state.notice("no earlier conversations in this repository"),
                            Ok(sessions) => state.resume_picker = Some(ResumePicker::new(sessions, &filter)),
                            Err(e) => state.error(format!("could not list the conversations: {e}")),
                        },
                        Some(Action::McpStatus) => state.notice(mcp_summary(&mcp)),
                        Some(Action::Resume(session)) => resume_session(&mut state, &ctx.stores.sessions, session),
                        Some(Action::SignIn | Action::SignOut) if endpoint_task.is_some() || switch_task.is_some() => state.notice("wait for the model switch in progress to finish"),
                        Some(Action::SignIn) => {
                            if let Some(code) = &signin_code {
                                open_in_browser(code.verification_uri_complete.as_deref().unwrap_or(&code.verification_uri));
                                state.notice(signin_prompt(code));
                            } else if signin_task.is_some() || signout_task.is_some() {
                                state.notice("ModelGateway is busy; try again in a moment");
                            } else {
                                match gateway::stored(&config.layout()) {
                                    Ok(Some(credential)) if config.model.credential.as_deref() == Some(CredentialStore::MODELGATEWAY) => {
                                        state.notice(format!("already signed in to ModelGateway{}; /signout to sign out", credential.account.map(|a| format!(" as {a}")).unwrap_or_default()));
                                    }
                                    // Signed in elsewhere (the desktop app) but this
                                    // configuration does not route through it yet.
                                    Ok(Some(credential)) => signin_task = Some(tokio::spawn(async move { Ok(credential.account) })),
                                    Ok(None) => {
                                        state.notice("signing in to ModelGateway…");
                                        let layout = config.layout();
                                        let code_tx = code_tx.clone();
                                        signin_task = Some(tokio::spawn(async move { device_sign_in(layout, code_tx).await }));
                                    }
                                    Err(e) => state.error(format!("could not read the stored credentials: {e}")),
                                }
                            }
                        }
                        Some(Action::SignOut) => {
                            if let Some(task) = signin_task.take() {
                                task.abort();
                                signin_code = None;
                                state.notice("sign-in cancelled");
                            } else if signout_task.is_some() {
                                state.notice("already signing out");
                            } else {
                                let layout = config.layout();
                                match gateway::stored(&layout) {
                                    Ok(None) if config.model.credential.as_deref() != Some(CredentialStore::MODELGATEWAY) => state.notice("not signed in to ModelGateway"),
                                    Ok(credential) => {
                                        state.notice("signing out of ModelGateway…");
                                        signout_task = Some(tokio::spawn(async move { (gateway::sign_out(&layout, None).await, credential) }));
                                    }
                                    Err(e) => state.error(format!("could not read the stored credentials: {e}")),
                                }
                            }
                        }
                        Some(Action::Account(_) | Action::ConfirmPlan) if account_task.is_some() => state.notice("still waiting for ModelGateway's last answer"),
                        Some(Action::ConfirmPlan) => match pending_plan.take() {
                            Some(id) => {
                                let layout = config.layout();
                                account_task = Some(tokio::spawn(account::run(layout, Request::Plan { id, from_balance: true, confirmed: true })));
                            }
                            None => state.notice("nothing to confirm; /billing plan <id> balance prices a plan first"),
                        },
                        Some(Action::Account(request)) => {
                            if !matches!(request, Request::Usage { .. }) && pending_plan.take().is_some() {
                                state.notice("plan purchase cancelled");
                            }
                            let layout = config.layout();
                            account_task = Some(tokio::spawn(account::run(layout, request)));
                        }
                        Some(Action::SwitchModel(name)) => {
                            state.log(format!("switching to {name}: loading its capability profile (probing if none is cached)"));
                            state.switching = Some(name.clone());
                            let mut next = config.clone();
                            next.model.name = name;
                            switch_task = Some(tokio::spawn(async move {
                                let provider = app::provider(&next, true, true).await;
                                (next, provider)
                            }));
                        }
                        _ => {}
                    }
                }
            }
            Some(ev) = rx.recv() => state.apply(ev),
            Some((request, reply)) = approval_rx.recv() => state.pending = Some((request, reply)),
            Some((done, total)) = progress_rx.recv() => state.indexing = Some(format!("indexing {done}/{total}")),
            res = finished(&mut startup_index) => {
                startup_index = None;
                state.indexing = None;
                match res.unwrap_or_else(|e| Err(CtxError::other(e.to_string()))) {
                    Ok(r) => {
                        state.index_generation = r.generation;
                        state.log(format!("index ready: {} indexed, {} removed, {} unchanged ({} ms)", r.files_indexed, r.files_removed, r.files_unchanged, r.duration_ms));
                    }
                    Err(e) => state.error(format!("indexing failed: {e}; retrieval will fall back to text tools")),
                }
            }
            Some(gen) = index_rx.recv() => {
                state.index_generation = gen;
                state.log(format!("index updated to generation {gen}"));
            }
            res = finished(&mut running_task) => {
                running_task = None;
                state.running = false;
                ctrl_c_armed = false;
                let ran = matches!(res, Ok(Ok(_)));
                match res {
                    Ok(Ok(_)) => {}
                    Ok(Err(e)) => state.error(format!("error: {e}")),
                    Err(e) => state.error(format!("task panicked: {e}")),
                }
                if let Ok(Some(s)) = ctx.stores.sessions.get_session(&state.session.id) {
                    state.session = s;
                }
                // Steering the turn never read (it was cancelled, failed or
                // ran out of steps) goes to the front of the queue.
                state.steering.clear();
                let missed: Vec<String> = inbox.take().into_iter().map(|m| m.text).collect();
                state.queued.splice(0..0, missed);
                if ran && !state.queued.is_empty() && !cancel.is_cancelled() && state.switching.is_none() {
                    let prompt = state.queued.remove(0);
                    running_task = Some(start_run(&mut state, &runtime, prompt));
                }
            }
            res = finished(&mut models_task) => {
                models_task = None;
                if let Ok(Ok(models)) = &res {
                    state.models_cache = models.clone();
                }
                if let Some(picker) = state.picker.as_mut() {
                    picker.loaded(res.unwrap_or_else(|e| Err(CtxError::other(e.to_string()))), &state.model);
                }
            }
            res = finished(&mut stats_task) => {
                stats_task = None;
                if let Ok(stats) = res {
                    state.model_stats = stats;
                }
            }
            res = finished(&mut models_dev_task) => {
                models_dev_task = None;
                if let Ok(Some(index)) = res {
                    state.models_dev = Some(index);
                }
            }
            Some(code) = code_rx.recv() => {
                open_in_browser(code.verification_uri_complete.as_deref().unwrap_or(&code.verification_uri));
                state.notice(signin_prompt(&code));
                signin_code = Some(code);
            }
            res = finished(&mut signin_task) => {
                signin_task = None;
                signin_code = None;
                match res {
                    Ok(Ok(account)) => match gateway::use_gateway_endpoint(&config, gateway::DEFAULT_ORIGIN).map_err(|e| e.to_string()).and_then(|()| app::load_config(cli).map_err(|e| e.to_string())) {
                        Ok(next) => {
                            let who = account.map(|a| format!(" as {a}")).unwrap_or_default();
                            let mut note = format!("signed in to ModelGateway{who} · model requests now go to {}", next.model.base_url);
                            if next.model.credential.as_deref() != Some(CredentialStore::MODELGATEWAY) {
                                note = format!("signed in to ModelGateway{who}, but a repository or --config file sets the model endpoint, so requests still go to {}", next.model.base_url);
                            }
                            if state.running {
                                note.push_str(" (from the next task)");
                            }
                            state.log(format!("loading {} at {} (probing if no capability profile is cached)", next.model.name, next.model.base_url));
                            endpoint_task = Some(tokio::spawn(async move {
                                let provider = app::provider(&next, true, true).await;
                                (next, provider, note)
                            }));
                        }
                        Err(e) => state.error(format!("signed in to ModelGateway, but the model endpoint could not be switched: {e}")),
                    },
                    Ok(Err(e)) => state.error(format!("ModelGateway sign-in failed: {e}")),
                    Err(e) if e.is_cancelled() => {}
                    Err(e) => state.error(format!("ModelGateway sign-in failed: {e}")),
                }
            }
            res = finished(&mut signout_task) => {
                signout_task = None;
                let (revoked, credential) = match res {
                    Ok(done) => done,
                    Err(e) => (Err(GatewayError::Transport(e.to_string())), None),
                };
                let mut note = "signed out of ModelGateway".to_string();
                match revoked {
                    Ok(true) => {}
                    Ok(false) if credential.is_none() => {}
                    Ok(false) => note.push_str("; the device key could not be revoked on the gateway"),
                    Err(e) => note.push_str(&format!("; the device key could not be revoked on the gateway ({e}) and may still be active there")),
                }
                let origin = credential.as_ref().and_then(|c| c.origin.as_deref()).and_then(gateway::origin).unwrap_or_else(|| gateway::DEFAULT_ORIGIN.to_string());
                let restore = credential.and_then(|c| c.restore_base_url);
                match gateway::restore_endpoint(&config, &origin, restore).map_err(|e| e.to_string()).and_then(|()| app::load_config(cli).map_err(|e| e.to_string())) {
                    Ok(next) => {
                        note.push_str(&format!(" · model requests now go to {}", next.model.base_url));
                        if state.running {
                            note.push_str(" (from the next task)");
                        }
                        endpoint_task = Some(tokio::spawn(async move {
                            let provider = app::provider(&next, true, true).await;
                            (next, provider, note)
                        }));
                    }
                    Err(e) => state.error(format!("{note}, but the previous model endpoint could not be restored: {e}")),
                }
            }
            res = finished(&mut account_task) => {
                account_task = None;
                match res {
                    Ok(Ok(Reply::Text(text))) => state.notice(text),
                    Ok(Ok(Reply::ConfirmPlan { id, text })) => {
                        pending_plan = Some(id);
                        state.notice(text);
                    }
                    Ok(Err(e)) => state.error(e),
                    Err(e) => state.error(format!("ModelGateway request failed: {e}")),
                }
            }
            res = finished(&mut endpoint_task) => {
                endpoint_task = None;
                match res {
                    Ok((next, provider, note)) => {
                        let name = next.model.name.clone();
                        let base_url = next.model.base_url.clone();
                        // The file is rewritten either way; follow it so `/model`
                        // lists the new endpoint's models.
                        config = next;
                        state.base_url = base_url.clone();
                        state.models_cache.clear();
                        match provider {
                            Ok(provider) => {
                                runtime = Arc::new(runtime.with_provider(provider));
                                state.model = name;
                                state.notice(note);
                            }
                            Err(e) => {
                                state.notice(note);
                                state.error(format!("{name} could not be loaded at {base_url}: {e}; /model picks another"));
                            }
                        }
                    }
                    Err(e) => state.error(format!("could not switch the model endpoint: {e}")),
                }
            }
            res = finished(&mut switch_task) => {
                switch_task = None;
                let name = state.switching.take().unwrap_or_default();
                match res {
                    Ok((next, Ok(provider))) => {
                        runtime = Arc::new(runtime.with_provider(provider));
                        config = next;
                        state.base_url = config.model.base_url.clone();
                        let remembered = match app::remember_model(&config, &ctx.root) {
                            Ok(()) => " (remembered for this workspace)",
                            Err(e) => {
                                state.log(format!("could not remember the model: {e}"));
                                ""
                            }
                        };
                        state.notice(format!("model switched from {} to {name}{remembered}", state.model));
                        state.model = name;
                    }
                    Ok((_, Err(e))) => state.error(format!("could not switch to {name}: {e}; still using {}", state.model)),
                    Err(e) => state.error(format!("could not switch to {name}: {e}; still using {}", state.model)),
                }
            }
            _ = tick.tick() => {}
        }
    }
    set_mouse_reporting(false);
    set_key_disambiguation(false);
    ratatui::restore();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(3), mcp.shutdown()).await;
    Ok(state.last_outcome.as_ref().map(|o| app::exit_code_for(o.status)).unwrap_or(0))
}

/// `/mcp`: one line per configured server, then its tools.
fn mcp_summary(mcp: &ctx_tools::McpManager) -> String {
    use ctx_tools::McpServerState;
    let config = mcp.config();
    if config.servers.is_empty() {
        return "no MCP servers configured; `ctx mcp add <name> -- <command>` or `ctx mcp add <name> --url <url>` adds one (see docs/configuration.md)".into();
    }
    let mut lines = Vec::new();
    if !config.enabled {
        lines.push("mcp.enabled = false: no server is connected".to_string());
    }
    for status in mcp.status() {
        let (line, _) = crate::serve::mcp_doctor_line(&status);
        lines.push(format!("{}: {line}", status.name));
        if status.state == McpServerState::Connected {
            for tool in &status.tools {
                let marks = match (tool.read_only, tool.destructive) {
                    (true, _) => " (read-only)",
                    (false, true) => " (destructive)",
                    _ => "",
                };
                lines.push(format!("  {}{marks}", tool.qualified_name));
            }
        }
    }
    lines.join("\n")
}

/// Resolves when the task in `handle` finishes; never while there is none.
async fn finished<T>(handle: &mut Option<tokio::task::JoinHandle<T>>) -> std::result::Result<T, tokio::task::JoinError> {
    match handle.as_mut() {
        Some(h) => h.await,
        None => std::future::pending().await,
    }
}

fn handle_key(state: &mut State, key: KeyEvent, cancel: &CancellationToken, ctrl_c_armed: &mut bool) -> Option<Action> {
    if let Some((_, _)) = &state.pending {
        let decision = match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => Some(ApprovalDecision::Approve),
            KeyCode::Char('a') | KeyCode::Char('A') => Some(ApprovalDecision::ApproveForSession),
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => Some(ApprovalDecision::Deny),
            _ => None,
        };
        if let Some(d) = decision {
            if let Some((req, reply)) = state.pending.take() {
                state.log(format!("approval {:?}: {}", d, req.summary.chars().take(60).collect::<String>()));
                let _ = reply.send(d);
            }
        }
        return None;
    }
    if state.picker.is_some() {
        return handle_picker_key(state, key);
    }
    if state.resume_picker.is_some() {
        return handle_resume_key(state, key);
    }
    if state.agents_view.is_some() && !(key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::CONTROL) {
        handle_agents_key(state, key);
        return None;
    }
    if state.history_menu.is_some() {
        match key.code {
            KeyCode::Up => {
                state.history_back();
                return None;
            }
            KeyCode::Down => {
                state.history_forward();
                return None;
            }
            KeyCode::Esc => {
                state.close_history(true);
                return None;
            }
            // Enter runs the recalled prompt; anything else edits it.
            _ => state.close_history(false),
        }
    }
    let suggestions = state.suggestions();
    if !suggestions.is_empty() {
        let n = suggestions.len();
        let chosen = &suggestions[state.completion.min(n - 1)];
        match key.code {
            KeyCode::Up => {
                state.completion = (state.completion + n - 1) % n;
                return None;
            }
            KeyCode::Down => {
                state.completion = (state.completion + 1) % n;
                return None;
            }
            KeyCode::Tab => {
                state.input = chosen.replacement.clone();
                if chosen.takes_args {
                    state.input.push(' ');
                }
                state.edited();
                return None;
            }
            KeyCode::Esc => {
                state.completion_dismissed = true;
                return None;
            }
            // Enter completes an `@` mention (the message is not finished)…
            KeyCode::Enter if chosen.label.starts_with('@') => {
                state.input = chosen.replacement.clone();
                state.edited();
                return None;
            }
            // …and runs the highlighted slash command.
            KeyCode::Enter => state.input = chosen.replacement.clone(),
            _ => {}
        }
    }
    match (key.code, key.modifiers) {
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
            if state.running {
                if *ctrl_c_armed {
                    state.quit = true;
                } else {
                    *ctrl_c_armed = true;
                    cancel.cancel();
                    state.notice("cancelling the current task (ctrl-c again to quit)");
                }
            } else {
                state.quit = true;
            }
        }
        (KeyCode::Esc, _) if !state.running => state.quit = true,
        // While a task runs, Enter steers it and ⌘/ctrl/alt-Enter or tab
        // (most macOS terminals keep ⌘-Enter) queue the prompt for after it.
        (KeyCode::Enter, m) | (KeyCode::Tab, m) => {
            let prompt = state.input.trim().to_string();
            if prompt.starts_with('/') {
                state.remember_input(&prompt);
                state.input.clear();
                state.edited();
                return slash_command(state, &prompt);
            }
            if !prompt.is_empty() && state.running {
                state.input.clear();
                state.edited();
                state.remember_input(&prompt);
                let queue = key.code == KeyCode::Tab || m.intersects(KeyModifiers::SUPER | KeyModifiers::CONTROL | KeyModifiers::ALT);
                return Some(if queue { Action::Queue(prompt) } else { Action::Steer(prompt) });
            }
            if !prompt.is_empty() && !state.running {
                if state.switching.is_some() {
                    state.notice("wait for the model switch to finish before running a task");
                    return None;
                }
                state.input.clear();
                if cancel.is_cancelled() {
                    state.notice("the session was cancelled; restart ctx to run more tasks");
                    return None;
                }
                state.remember_input(&prompt);
                return Some(Action::Run(prompt));
            }
        }
        (KeyCode::Backspace, _) => {
            state.input.pop();
            state.edited();
        }
        (KeyCode::BackTab, _) => return Some(Action::SetMode(slash::next_mode(state.profile))),
        (KeyCode::Up, _) => state.history_back(),
        (KeyCode::Down, _) if state.input.is_empty() && state.transcript.agents().next().is_some() => state.agents_view = Some(AgentsView::new(&state.transcript)),
        (KeyCode::PageUp, _) => state.scroll_by(-(state.page.saturating_sub(2).max(1) as isize)),
        (KeyCode::PageDown, _) => state.scroll_by(state.page.saturating_sub(2).max(1) as isize),
        (KeyCode::Home, _) => {
            state.follow = false;
            state.scroll = 0;
        }
        (KeyCode::End, _) => state.follow = true,
        (KeyCode::Char('l'), KeyModifiers::CONTROL) => state.tool_log.clear(),
        (KeyCode::Char('o'), KeyModifiers::CONTROL) => state.show_thinking = !state.show_thinking,
        (KeyCode::Char('t'), KeyModifiers::CONTROL) => state.show_log = !state.show_log,
        (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) => {
            state.input.push(c);
            state.edited();
        }
        _ => {}
    }
    None
}

/// Shows `prompt` as the user's turn and runs it.
fn start_run(state: &mut State, runtime: &Arc<AgentRuntime>, prompt: String) -> tokio::task::JoinHandle<Result<TaskOutcome>> {
    state.running = true;
    state.transcript.push(Entry::User(prompt.clone()));
    state.follow = true;
    let rt = runtime.clone();
    let session = state.session.clone();
    tokio::spawn(async move { rt.run(&session, &prompt).await })
}

fn slash_command(state: &mut State, line: &str) -> Option<Action> {
    let (command, arg) = line.split_once(char::is_whitespace).map(|(c, a)| (c, a.trim())).unwrap_or((line, ""));
    match command {
        "/new" if state.running => state.notice("wait for the current task to finish before starting a new session"),
        "/new" => return Some(Action::NewSession),
        "/model" if state.running => state.notice("wait for the current task to finish before switching models"),
        "/model" if state.switching.is_some() => state.notice("a model switch is already in progress"),
        "/model" if arg.is_empty() => {
            state.picker = Some(ModelPicker::loading());
            return Some(Action::ListModels);
        }
        "/model" if arg == state.model => state.notice(format!("already using {arg}")),
        "/model" => return Some(Action::SwitchModel(arg.to_string())),
        "/approval" if arg.is_empty() => {
            let mut text = format!("approval mode: {} — {}\n", slash::mode_name(state.profile), slash::mode_about(state.profile));
            for (name, _, about) in slash::MODES {
                text.push_str(&format!("\n  {name:<11}{about}"));
            }
            text.push_str("\n\n/approval <mode> or shift-tab to change it (bypass only by name)");
            state.notice(text);
        }
        "/approval" => match slash::parse_mode(arg) {
            Some(p) if p == state.profile => state.notice(format!("already in {} mode", slash::mode_name(p))),
            Some(p) => return Some(Action::SetMode(p)),
            None => state.error(format!("unknown approval mode `{arg}`; use ask, auto or bypass")),
        },
        "/effort" if arg.is_empty() => {
            let mut text = format!("reasoning effort: {} — {}\n", slash::effort_name(state.effort.as_ref()), slash::effort_about(state.effort.as_ref()));
            for effort in slash::effort_choices(state.effort_levels().as_deref()) {
                text.push_str(&format!("\n  {:<11}{}", slash::effort_name(effort.as_ref()), slash::effort_about(effort.as_ref())));
            }
            text.push_str("\n\n/effort <level> to change it; ultra lets the model deploy sub-agents");
            state.notice(text);
        }
        "/effort" => {
            let choices = slash::effort_choices(state.effort_levels().as_deref());
            let names = || choices.iter().map(|e| slash::effort_name(e.as_ref())).collect::<Vec<_>>().join(", ");
            match slash::parse_effort(arg) {
                Some(e) if e == state.effort => state.notice(format!("already at {} effort", slash::effort_name(e.as_ref()))),
                Some(e) if choices.contains(&e) => return Some(Action::SetEffort(e)),
                Some(_) => state.error(format!("{} does not take `{arg}`; use {}", state.model, names())),
                None => state.error(format!("unknown reasoning effort `{arg}`; use {}", names())),
            }
        }
        "/signin" => return Some(Action::SignIn),
        "/signout" => return Some(Action::SignOut),
        "/usage" => match account::parse_usage(arg) {
            Ok(request) => return Some(Action::Account(request)),
            Err(e) => state.error(e),
        },
        "/billing" if arg.eq_ignore_ascii_case("confirm") => return Some(Action::ConfirmPlan),
        "/billing" => match account::parse_billing(arg) {
            Ok(request) => return Some(Action::Account(request)),
            Err(e) => state.error(e),
        },
        "/agents" if state.transcript.agents().next().is_none() => state.notice("no sub-agents in this session yet; at /effort ultra the model deploys them for work that splits into parallel pieces"),
        "/agents" => state.agents_view = Some(AgentsView::new(&state.transcript)),
        "/resume" if state.running => state.notice("wait for the current task to finish before resuming another conversation"),
        "/resume" => return Some(Action::ListSessions(arg.to_string())),
        "/mcp" => return Some(Action::McpStatus),
        "/help" => state.notice(slash::help()),
        _ => state.error(format!("unknown command {command}; /help lists them")),
    }
    None
}

/// The device flow: a code from the gateway (sent to the UI, which opens
/// the browser), then polling until the browser approves, denies or the
/// code expires. On approval the key is in the credential store.
async fn device_sign_in(layout: ctx_core::paths::DataLayout, codes: mpsc::Sender<DeviceCode>) -> std::result::Result<Option<String>, String> {
    let origin = gateway::DEFAULT_ORIGIN;
    let start = gateway::device_start(origin, None).await.map_err(|e| e.to_string())?;
    let code: DeviceCode = serde_json::from_value(start).map_err(|e| format!("unexpected reply from the gateway ({e})"))?;
    let deadline = Instant::now() + std::time::Duration::from_secs(code.expires_in.unwrap_or(600));
    let mut interval = code.interval.unwrap_or(5).max(1);
    let device_code = code.device_code.clone();
    let _ = codes.send(code).await;
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
        if Instant::now() >= deadline {
            return Err("the code expired; /signin to try again".into());
        }
        let poll = gateway::device_poll(&layout, origin, &device_code).await.map_err(|e| e.to_string())?;
        match poll.get("status").and_then(serde_json::Value::as_str) {
            Some("approved") => return Ok(gateway::account_name(&poll)),
            Some("denied") => return Err("the request was denied in the browser".into()),
            Some("expired") => return Err("the code expired; /signin to try again".into()),
            _ => interval = poll.get("interval").and_then(serde_json::Value::as_u64).unwrap_or(interval).max(1),
        }
    }
}

fn signin_prompt(code: &DeviceCode) -> String {
    format!(
        "approve this device in your browser to sign in to ModelGateway

  code  {}
  page  {}

waiting for approval… /signout cancels",
        code.user_code,
        code.verification_uri_complete.as_deref().unwrap_or(&code.verification_uri)
    )
}

/// Opens `url` in the default browser, quietly (the terminal is the TUI's).
/// Evens out what the Windows console reports so the key bindings work the
/// same everywhere: AltGr arrives as Ctrl+Alt (so `@ { } [ ] \ | ~ €` on
/// German, French, Nordic… layouts would be dropped as Ctrl chords), and
/// Ctrl+letter follows Caps Lock and Shift (`Char('C')` + CONTROL). A no-op
/// on other platforms, whose terminals already send plain characters.
pub(crate) fn normalize_key(mut key: KeyEvent) -> KeyEvent {
    if !cfg!(windows) {
        return key;
    }
    if let KeyCode::Char(c) = key.code {
        if key.modifiers.contains(KeyModifiers::CONTROL | KeyModifiers::ALT) {
            key.modifiers.remove(KeyModifiers::CONTROL | KeyModifiers::ALT);
        } else if key.modifiers.contains(KeyModifiers::CONTROL) && c.is_ascii_alphabetic() {
            key.code = KeyCode::Char(c.to_ascii_lowercase());
            key.modifiers.remove(KeyModifiers::SHIFT);
        }
    }
    key
}

pub(crate) fn open_in_browser(url: &str) {
    let _ = ctx_core::platform::open_url(url);
}

/// The agents view's keys: the list picks an agent, the open conversation
/// scrolls; tab steps to the next agent and esc goes back a level.
fn handle_agents_key(state: &mut State, key: KeyEvent) {
    let Some(view) = state.agents_view.as_mut() else { return };
    let count = state.transcript.agents().count();
    let page = state.page.saturating_sub(2).max(1) as isize;
    match (&view.open, key.code) {
        (_, KeyCode::Char('o')) if key.modifiers == KeyModifiers::CONTROL => state.show_thinking = !state.show_thinking,
        (None, KeyCode::Esc | KeyCode::Left | KeyCode::Char('q')) => state.agents_view = None,
        (None, KeyCode::Up | KeyCode::Char('k')) => view.selected = view.selected.saturating_sub(1),
        (None, KeyCode::Down | KeyCode::Char('j')) => view.selected = (view.selected + 1).min(count.saturating_sub(1)),
        (None, KeyCode::Home) => view.selected = 0,
        (None, KeyCode::End) => view.selected = count.saturating_sub(1),
        (None, KeyCode::Enter | KeyCode::Right) => view.open(&state.transcript, view.selected),
        (Some(_), KeyCode::Esc | KeyCode::Left | KeyCode::Char('q')) => view.open = None,
        (Some(_), KeyCode::Tab) if count > 0 => view.open(&state.transcript, (view.selected + 1) % count),
        (Some(_), KeyCode::BackTab) if count > 0 => view.open(&state.transcript, (view.selected + count - 1) % count),
        (Some(_), KeyCode::Up | KeyCode::Char('k')) => view.scroll_by(-1),
        (Some(_), KeyCode::Down | KeyCode::Char('j')) => view.scroll_by(1),
        (Some(_), KeyCode::PageUp) => view.scroll_by(-page),
        (Some(_), KeyCode::PageDown | KeyCode::Char(' ')) => view.scroll_by(page),
        (Some(_), KeyCode::Home) => {
            view.follow = false;
            view.scroll = 0;
        }
        (Some(_), KeyCode::End) => view.follow = true,
        _ => {}
    }
}

fn handle_resume_key(state: &mut State, key: KeyEvent) -> Option<Action> {
    let picker = state.resume_picker.as_mut()?;
    let count = picker.matches().len();
    match (key.code, key.modifiers) {
        (KeyCode::Esc, _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => state.resume_picker = None,
        (KeyCode::Up, _) => picker.selected = picker.selected.saturating_sub(1),
        (KeyCode::Down, _) => picker.selected = (picker.selected + 1).min(count.saturating_sub(1)),
        (KeyCode::PageUp, _) => picker.selected = picker.selected.saturating_sub(10),
        (KeyCode::PageDown, _) => picker.selected = (picker.selected + 10).min(count.saturating_sub(1)),
        (KeyCode::Enter, _) => {
            let choice = picker.choice()?;
            state.resume_picker = None;
            return Some(Action::Resume(choice));
        }
        (KeyCode::Backspace, _) => {
            picker.filter.pop();
            picker.selected = 0;
        }
        (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) => {
            picker.filter.push(c);
            picker.selected = 0;
        }
        _ => {}
    }
    None
}

/// A session for a new conversation: the latest interactive one when
/// nothing was ever run in it (so starting ctx over and over leaves one
/// empty session behind, not one per start), else a new one.
fn fresh_session(runtime: &AgentRuntime, store: &SessionStore, repository_id: &str, model: &str, current: Option<&str>) -> Result<SessionRecord> {
    for s in store.list_sessions(repository_id, 20)? {
        if s.mode == "interactive" && Some(s.id.as_str()) != current && store.tasks_for_session(&s.id)?.is_empty() {
            return Ok(s);
        }
    }
    runtime.sessions().start("interactive", model)
}

fn resume_session(state: &mut State, store: &SessionStore, session: SessionRecord) {
    let id = session.id[..8.min(session.id.len())].to_string();
    match state.load_session(store, session) {
        Ok(()) => {
            let prompts = state.transcript.entries.iter().filter(|e| matches!(e, Entry::User(_))).count();
            state.notice(format!("resumed session {id} · {prompts} prompt{} · the model picks up where it left off", if prompts == 1 { "" } else { "s" }));
        }
        Err(e) => state.error(format!("could not load session {id}: {e}")),
    }
}

fn handle_picker_key(state: &mut State, key: KeyEvent) -> Option<Action> {
    let picker = state.picker.as_mut()?;
    let count = picker.matches().len();
    match (key.code, key.modifiers) {
        (KeyCode::Esc, _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => state.picker = None,
        (KeyCode::Up, _) => picker.selected = picker.selected.saturating_sub(1),
        (KeyCode::Down, _) => picker.selected = (picker.selected + 1).min(count.saturating_sub(1)),
        (KeyCode::PageUp, _) => picker.selected = picker.selected.saturating_sub(10),
        (KeyCode::PageDown, _) => picker.selected = (picker.selected + 10).min(count.saturating_sub(1)),
        (KeyCode::Enter, _) => {
            let choice = picker.choice()?;
            state.picker = None;
            if choice == state.model {
                return None;
            }
            return Some(Action::SwitchModel(choice));
        }
        (KeyCode::Backspace, _) => {
            picker.filter.pop();
            picker.selected = 0;
        }
        (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) => {
            picker.filter.push(c);
            picker.selected = 0;
        }
        _ => {}
    }
    None
}

/// Ask the terminal to report mouse buttons and the wheel (xterm mode 1000,
/// SGR encoding 1006) so the wheel scrolls the conversation while ↑/↓ stay
/// free for prompt history. Motion is not requested. While this is on,
/// selecting text needs the terminal's bypass modifier (Option-drag in
/// iTerm2 and Terminal.app, Shift-drag in most others).
fn set_mouse_reporting(on: bool) {
    use std::io::Write;
    let mut out = std::io::stdout();
    // The Windows console only produces mouse events when its input mode
    // has ENABLE_MOUSE_INPUT (and QuickEdit off), which is what crossterm's
    // capture commands set; escape sequences alone are not enough there.
    #[cfg(windows)]
    {
        let _ = if on { crossterm::execute!(out, crossterm::event::EnableMouseCapture) } else { crossterm::execute!(out, crossterm::event::DisableMouseCapture) };
    }
    #[cfg(not(windows))]
    {
        let _ = out.write_all(if on { b"\x1b[?1000h\x1b[?1006h" } else { b"\x1b[?1006l\x1b[?1000l" });
    }
    let _ = out.flush();
}

static KEYS_DISAMBIGUATED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Where the terminal speaks the kitty keyboard protocol, ask it to report
/// modified keys, so ⌘-Enter (when the terminal does not keep it for
/// itself) and ctrl-Enter arrive as such and queue instead of steering.
fn set_key_disambiguation(on: bool) {
    use crossterm::event::{KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags};
    use std::sync::atomic::Ordering;
    let mut out = std::io::stdout();
    if on {
        if crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false) && crossterm::execute!(out, PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)).is_ok() {
            KEYS_DISAMBIGUATED.store(true, Ordering::SeqCst);
        }
    } else if KEYS_DISAMBIGUATED.swap(false, Ordering::SeqCst) {
        let _ = crossterm::execute!(out, PopKeyboardEnhancementFlags);
    }
}

/// Prompt history for this repository, oldest first: what was typed in
/// earlier sessions, consecutive repeats collapsed.
fn load_history(ctx: &ctx_agent::RepositoryContext) -> Vec<String> {
    let mut history: Vec<String> = Vec::new();
    for text in ctx.stores.sessions.recent_user_messages(500).unwrap_or_default().into_iter().rev() {
        let text = text.trim().to_string();
        if !text.is_empty() && history.last() != Some(&text) {
            history.push(text);
        }
    }
    history
}

fn draw(f: &mut Frame, state: &mut State) {
    let area = f.area();
    let input_rows = input_rows(state, area.width.saturating_sub(4).max(10) as usize);
    let input_height = input_rows.len().min(6) as u16 + 2;
    let [main, input, status] = Layout::vertical([Constraint::Min(3), Constraint::Length(input_height), Constraint::Length(1)]).areas(area);
    let (conv, side) = if state.show_log && main.width >= 100 {
        let [conv, side] = Layout::horizontal([Constraint::Min(40), Constraint::Length(main.width * 3 / 10)]).areas(main);
        (conv, Some(side))
    } else {
        (main, None)
    };

    // Conversation: borderless, one column of padding on each side.
    let conv = Rect { x: conv.x + 1, width: conv.width.saturating_sub(2), ..conv };
    state.page = conv.height as usize;
    if state.agents_view.is_some() {
        draw_agents(f, state, conv);
    } else {
        let lines = state.transcript.render(conv.width as usize, state.show_thinking);
        draw_scrolled(f, &lines, &mut state.scroll, &mut state.follow, conv);
    }

    if let Some(side) = side {
        let block = Block::default().borders(Borders::LEFT).border_style(Style::default().fg(FAINT)).title(Span::styled(" events ", Style::default().fg(DIM)));
        let inner = block.inner(side);
        let start = state.tool_log.len().saturating_sub(inner.height as usize);
        let rows: Vec<Line> = state.tool_log[start..].iter().map(|l| Line::styled(format!(" {l}"), Style::default().fg(DIM))).collect();
        f.render_widget(block, side);
        f.render_widget(Paragraph::new(rows), inner);
    }

    // Input: grows with its content; the title shows what a running task is doing.
    let mut block = Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(Style::default().fg(FAINT));
    if let (true, Some(started)) = (state.running, state.task_started) {
        block = block.title(Line::from(vec![
            Span::styled(format!(" {} ", spinner(started)), Style::default().fg(THINK)),
            Span::styled(format!("{} ", state.state), Style::default().fg(THINK)),
            Span::styled(format!("· step {} · {} · ctrl-c to cancel ", state.step, fmt_secs(started.elapsed())), Style::default().fg(DIM)),
        ]));
    } else if state.switching.is_some() {
        block = block.title(Span::styled(" switching model… ", Style::default().fg(WARN)));
    }
    if let Some(view) = &state.agents_view {
        let hint = if view.open.is_some() { " ↑↓ scroll · tab next agent · ctrl-o thinking · esc back to agents " } else { " ↑↓ choose · enter opens · esc back to the conversation " };
        block = block.title_bottom(Span::styled(hint, Style::default().fg(DIM)));
    }
    let suggestions = state.suggestions();
    if !suggestions.is_empty() && !state.running {
        block = block.title(Span::styled(" tab completes · ↑↓ choose · esc hides ", Style::default().fg(DIM)));
    }
    if let Some(menu) = &state.history_menu {
        block = block.title(Span::styled(format!(" history {}/{} · ↑↓ browse · enter runs · esc restores ", menu.selected + 1, state.history.len()), Style::default().fg(DIM)));
    }
    let inner = block.inner(input);
    let skip = input_rows.len().saturating_sub(inner.height as usize);
    let shown: Vec<Line> = input_rows.into_iter().skip(skip).collect();
    if state.picker.is_none() && state.resume_picker.is_none() && state.pending.is_none() && state.agents_view.is_none() {
        if let Some(last) = shown.last() {
            let row = (shown.len() - 1) as u16;
            let col = if state.input.is_empty() { 2 } else { last.width() as u16 };
            f.set_cursor_position((inner.x + col.min(inner.width.saturating_sub(1)), inner.y + row));
        }
    }
    f.render_widget(block, input);
    f.render_widget(Paragraph::new(shown), inner);
    if !suggestions.is_empty() {
        draw_suggestions(f, &suggestions, state.completion.min(suggestions.len() - 1), input);
    }
    if let Some(menu) = &state.history_menu {
        draw_history(f, &state.history, menu.selected, input);
    }

    // Status line: model and mode on the left, usage and index on the right.
    let mut left = vec![Span::raw(" "), Span::styled(state.model.clone(), Style::default().fg(ACCENT))];
    if let Some(savings) = state.model_stats.get(&state.model).and_then(|s| s.savings_label()) {
        left.push(Span::styled(format!(" {savings}"), Style::default().fg(OK)));
    }
    if let Some(next) = &state.switching {
        left.push(Span::styled(format!(" → {next}"), Style::default().fg(WARN)));
    }
    let mode_color = match state.profile {
        PermissionProfile::ApprovalRequired => DIM,
        PermissionProfile::WorkspaceAutonomous => WARN,
        PermissionProfile::BypassPermissions => ERR,
    };
    left.push(Span::styled(" · ", Style::default().fg(DIM)));
    left.push(Span::styled(format!("{} mode", slash::mode_name(state.profile)), Style::default().fg(mode_color)));
    if let Some(effort) = &state.effort {
        left.push(Span::styled(" · ", Style::default().fg(DIM)));
        left.push(Span::styled(format!("{} effort", effort.as_str()), Style::default().fg(if effort.deploys_agents() { THINK } else { DIM })));
    }
    let mut right: Vec<String> = Vec::new();
    if let Some(c) = &state.last_context {
        right.push(format!("ctx {}/{}", fmt_tokens(c.estimated_prompt_tokens as u64), fmt_tokens(c.context_window as u64)));
    }
    let tokens = state.usage.prompt_tokens + state.usage.completion_tokens;
    if tokens > 0 {
        right.push(format!("{} tok", fmt_tokens(tokens)));
    }
    if let Some(cost) = state.cost {
        right.push(format!("${cost:.4}"));
    }
    right.push(state.indexing.clone().unwrap_or_else(|| format!("index gen {}", state.index_generation)));
    f.render_widget(Paragraph::new(Line::from(left)), status);
    f.render_widget(Paragraph::new(Span::styled(format!("{} ", right.join(" · ")), Style::default().fg(DIM))).alignment(Alignment::Right), status);

    if let Some((req, _)) = &state.pending {
        let area = centered(f.area(), 70, 60);
        f.render_widget(Clear, area);
        let mut text = vec![
            Line::from(vec![Span::styled(req.tool.clone(), Style::default().add_modifier(Modifier::BOLD)), Span::styled(format!("  {} risk", req.risk.as_str()), Style::default().fg(WARN))]),
            Line::raw(req.summary.clone()),
            Line::styled(req.reason.clone(), Style::default().fg(DIM)),
            Line::raw(""),
        ];
        for l in req.details.lines().take((area.height as usize).saturating_sub(8)) {
            text.push(Line::styled(l.to_string(), Style::default().fg(CODE)));
        }
        text.push(Line::raw(""));
        let key = |k: &str| Span::styled(k.to_string(), Style::default().fg(ACCENT).add_modifier(Modifier::BOLD));
        let label = |l: &str| Span::styled(l.to_string(), Style::default().fg(DIM));
        text.push(Line::from(vec![key("y"), label(" approve   "), key("n"), label(" deny   "), key("a"), label(" approve for this session")]));
        let block = Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(Style::default().fg(WARN)).title(Span::styled(" approval required ", Style::default().fg(WARN).add_modifier(Modifier::BOLD)));
        f.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }).block(block), area);
    } else if let Some(picker) = &state.picker {
        draw_picker(f, picker, &state.model, &state.model_stats, state.models_dev.as_ref(), &state.base_url);
    } else if let Some(picker) = &state.resume_picker {
        resume::draw_picker(f, picker);
    }
}

/// `lines` in `area` from row `scroll`, kept at the bottom while `follow`,
/// with a where-you-are hint once scrolled away from it.
fn draw_scrolled(f: &mut Frame, lines: &[Line<'static>], scroll: &mut usize, follow: &mut bool, area: Rect) {
    let height = area.height as usize;
    let max_scroll = lines.len().saturating_sub(height);
    if *follow || *scroll >= max_scroll {
        *scroll = max_scroll;
        *follow = true;
    }
    let visible: Vec<Line> = lines[*scroll..(*scroll + height).min(lines.len())].to_vec();
    f.render_widget(Paragraph::new(visible), area);
    if !*follow {
        let hint = format!(" ↑ {} above · ↓ {} more · end to follow ", *scroll, max_scroll - *scroll);
        let w = (hint.chars().count() as u16).min(area.width);
        let at = Rect { x: area.right().saturating_sub(w), y: area.bottom().saturating_sub(1), width: w, height: 1 };
        f.render_widget(Paragraph::new(hint).style(Style::default().fg(DIM).bg(SELECTED_BG)), at);
    }
}

/// The agents view in place of the conversation: the list of every agent
/// (grouped by the `spawn_agents` call that deployed them), or the open
/// agent's own conversation under a one-row header.
fn draw_agents(f: &mut Frame, state: &mut State, area: Rect) {
    let State { transcript, agents_view, show_thinking, .. } = state;
    let Some(view) = agents_view.as_mut() else { return };
    let width = area.width as usize;
    let agents: Vec<&transcript::AgentView> = transcript.agents().collect();
    view.selected = view.selected.min(agents.len().saturating_sub(1));
    let bold = Style::default().add_modifier(Modifier::BOLD);

    if let Some(agent) = view.open.as_ref().and_then(|id| agents.iter().find(|a| &a.agent_id == id)) {
        let (icon, color) = agent.icon();
        let mut head = vec![Span::styled(format!("{icon} "), Style::default().fg(color)), Span::styled(agent.name.clone(), bold)];
        head.extend(agent.stats());
        let position = format!("  {}/{}", view.selected + 1, agents.len());
        head.push(Span::styled(position, Style::default().fg(FAINT)));
        let [header, body] = Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).areas(area);
        f.render_widget(Paragraph::new(Line::from(head)), header);
        let lines = agent.log.render(width, *show_thinking);
        draw_scrolled(f, &lines, &mut view.scroll, &mut view.follow, body);
        return;
    }
    view.open = None;

    let running = agents.iter().filter(|a| a.is_running()).count();
    let mut lines = vec![
        Line::from(vec![
            Span::styled(format!("{} ", crate::transcript::glyphs().agents), Style::default().fg(THINK)),
            Span::styled("Agents", bold),
            Span::styled(format!("  {}{}", agents.len(), if running > 0 { format!(" · {running} running") } else { String::new() }), Style::default().fg(DIM)),
        ]),
    ];
    let mut selected_row = 0;
    let mut index = 0;
    let groups: Vec<&Vec<transcript::AgentView>> = transcript.entries.iter().filter_map(|e| if let Entry::Agents(a) = e { Some(a) } else { None }).collect();
    for (g, group) in groups.iter().enumerate() {
        lines.push(Line::raw(""));
        if groups.len() > 1 {
            lines.push(Line::styled(format!("  deployment {} · {} agent{}", g + 1, group.len(), if group.len() == 1 { "" } else { "s" }), Style::default().fg(FAINT)));
        }
        for agent in group.iter() {
            let on = index == view.selected;
            if on {
                selected_row = lines.len();
            }
            let (icon, color) = agent.icon();
            let marker = if on { Span::styled("› ", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)) } else { Span::raw("  ") };
            let name = if on { bold.fg(ACCENT) } else { bold };
            let mut row = vec![marker, Span::styled(format!("{icon} "), Style::default().fg(color)), Span::styled(agent.name.clone(), name)];
            row.extend(agent.stats());
            let activity = transcript::truncate_end(&agent.activity().split_whitespace().collect::<Vec<_>>().join(" "), width.saturating_sub(7));
            let style = if agent.is_running() { Style::default().fg(DIM).add_modifier(Modifier::ITALIC) } else { Style::default().fg(DIM) };
            let detail = Line::from(vec![Span::raw("    "), Span::styled("⎿ ", Style::default().fg(FAINT)), Span::styled(activity, style)]);
            let (row, detail) = (Line::from(row), detail);
            if on {
                lines.push(row.style(Style::default().bg(SELECTED_BG)));
                lines.push(detail.style(Style::default().bg(SELECTED_BG)));
            } else {
                lines.push(row);
                lines.push(detail);
            }
            index += 1;
        }
    }
    // Keep the selected agent's two rows on screen.
    let height = area.height as usize;
    let offset = (selected_row + 2).saturating_sub(height);
    let visible: Vec<Line> = lines.into_iter().skip(offset).take(height).collect();
    f.render_widget(Paragraph::new(visible), area);
}

/// Slash-command and `@` mention suggestions, just above the input box.
fn draw_suggestions(f: &mut Frame, suggestions: &[Suggestion], selected: usize, input: Rect) {
    let rows = suggestions.len().min(8).min(input.y as usize);
    if rows == 0 {
        return;
    }
    let area = Rect { x: input.x + 1, y: input.y - rows as u16, width: input.width.saturating_sub(2), height: rows as u16 };
    let label_w = suggestions.iter().map(|s| s.label.chars().count() + if s.hint.is_empty() { 0 } else { s.hint.chars().count() + 1 }).max().unwrap_or(0).min(36);
    let offset = selected.saturating_sub(rows - 1);
    let lines: Vec<Line> = suggestions
        .iter()
        .enumerate()
        .skip(offset)
        .take(rows)
        .map(|(i, s)| {
            let on = i == selected;
            let name = if on { Style::default().fg(ACCENT).add_modifier(Modifier::BOLD) } else { Style::default() };
            let pad = label_w.saturating_sub(s.label.chars().count() + if s.hint.is_empty() { 0 } else { s.hint.chars().count() + 1 });
            let mut spans = vec![Span::raw(" "), Span::styled(s.label.clone(), name)];
            if !s.hint.is_empty() {
                spans.push(Span::styled(format!(" {}", s.hint), Style::default().fg(DIM)));
            }
            spans.push(Span::raw(" ".repeat(pad + 3)));
            let used = 1 + label_w + 3;
            let about: String = s.about.chars().take((area.width as usize).saturating_sub(used + 1)).collect();
            spans.push(Span::styled(about, Style::default().fg(DIM)));
            let line = Line::from(spans);
            if on {
                line.style(Style::default().bg(SELECTED_BG))
            } else {
                line
            }
        })
        .collect();
    f.render_widget(Clear, area);
    f.render_widget(Paragraph::new(lines), area);
}

/// The history menu, just above the input box: newest at the bottom, the
/// selected prompt highlighted.
fn draw_history(f: &mut Frame, history: &[String], selected: usize, input: Rect) {
    let rows = history.len().min(8).min(input.y as usize);
    if rows == 0 {
        return;
    }
    let area = Rect { x: input.x + 1, y: input.y - rows as u16, width: input.width.saturating_sub(2), height: rows as u16 };
    let last = (selected + rows / 2).clamp(rows - 1, history.len() - 1);
    let first = last + 1 - rows;
    let lines: Vec<Line> = (first..=last)
        .map(|i| {
            let on = i == selected;
            let one_line = history[i].split_whitespace().collect::<Vec<_>>().join(" ");
            let text: String = if one_line.chars().count() + 2 > area.width as usize {
                one_line.chars().take((area.width as usize).saturating_sub(3)).collect::<String>() + "…"
            } else {
                one_line
            };
            let style = if on { Style::default().fg(ACCENT).add_modifier(Modifier::BOLD) } else { Style::default().fg(DIM) };
            let line = Line::from(vec![Span::raw(" "), Span::styled(text, style)]);
            if on {
                line.style(Style::default().bg(SELECTED_BG))
            } else {
                line
            }
        })
        .collect();
    f.render_widget(Clear, area);
    f.render_widget(Paragraph::new(lines), area);
}

/// The input box's rows: `› ` then the typed text, word-wrapped.
fn input_rows(state: &State, width: usize) -> Vec<Line<'static>> {
    let mut rows = pending_rows(state, width);
    let prompt = vec![Span::styled("› ", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))];
    if state.input.is_empty() {
        let hint = match (state.running, state.queued.is_empty()) {
            (false, _) => "describe a task, or /help",
            (true, true) => "enter steers this task · tab queues for after it",
            (true, false) => "enter steers · tab queues",
        };
        rows.push(Line::from(vec![prompt[0].clone(), Span::styled(hint, Style::default().fg(DIM))]));
        return rows;
    }
    rows.extend(transcript::wrap(vec![Span::raw(state.input.clone())], width, prompt, vec![Span::raw("  ")]));
    rows
}

/// One row per prompt waiting on the running turn: steering the model has
/// not read yet, then the queue; at most three, then a count.
fn pending_rows(state: &State, width: usize) -> Vec<Line<'static>> {
    let items: Vec<(&str, &String)> = state.steering.iter().map(|p| ("steering", p)).chain(state.queued.iter().map(|p| ("queued", p))).collect();
    let mut rows: Vec<Line<'static>> = items
        .iter()
        .take(3)
        .map(|(label, text)| {
            let first = text.lines().next().unwrap_or("");
            let room = width.saturating_sub(label.len() + 3).max(8);
            let clipped = if first.chars().count() > room || text.lines().nth(1).is_some() { format!("{}…", first.chars().take(room.saturating_sub(1)).collect::<String>()) } else { first.to_string() };
            let color = if *label == "steering" { THINK } else { DIM };
            Line::from(vec![Span::styled(format!("↳ {label} "), Style::default().fg(color)), Span::styled(clipped, Style::default().fg(DIM))])
        })
        .collect();
    if items.len() > 3 {
        rows.push(Line::styled(format!("  +{} more", items.len() - 3), Style::default().fg(DIM)));
    }
    rows
}

fn draw_picker(f: &mut Frame, picker: &ModelPicker, current: &str, stats: &ModelStats, models_dev: Option<&ModelsDevIndex>, base_url: &str) {
    let full = f.area();
    let mut area = centered(full, 60, 70);
    let matches = picker.matches();
    // The highlighted model's detail card sits to the right of the picker:
    // the pair is centred together, and the picker narrows (down to 40
    // columns) before the card is dropped.
    let card = matches.get(picker.selected).and_then(|id| {
        let info = models_dev.and_then(|index| index.info(id, base_url));
        model_card(id, info.as_ref(), stats.get(*id), CARD_WIDTH as usize - 4)
    });
    let card = card.filter(|_| full.width >= 40 + CARD_WIDTH + 1);
    if card.is_some() {
        area.width = area.width.min(full.width - CARD_WIDTH - 1);
        area.x = full.x + (full.width - area.width - CARD_WIDTH - 1) / 2;
    }
    f.render_widget(Clear, area);
    let title = if picker.loading { " switch model ".to_string() } else { format!(" switch model · {} of {} ", matches.len(), picker.models.len()) };
    let block = Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(Style::default().fg(ACCENT)).title(title);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let [header, list, footer] = Layout::vertical([Constraint::Length(2), Constraint::Min(1), Constraint::Length(1)]).areas(inner);

    let dim = Style::default().fg(DIM);
    f.render_widget(Paragraph::new(Line::from(vec![Span::styled("filter ", dim), Span::raw(picker.filter.as_str())])), header);
    f.set_cursor_position((header.x + 7 + picker.filter.chars().count() as u16, header.y));

    let mut lines: Vec<Line> = Vec::new();
    if picker.loading {
        lines.push(Line::styled("loading models…", dim));
    } else if let Some(e) = &picker.error {
        lines.push(Line::styled(format!("could not list models: {e}"), Style::default().fg(ERR)));
    }
    if !picker.loading && matches.is_empty() && !picker.filter.trim().is_empty() {
        lines.push(Line::styled(format!("no listed model matches; Enter switches to `{}` as typed", picker.filter.trim()), dim));
    }
    let rows = (list.height as usize).saturating_sub(lines.len()).max(1);
    let offset = picker.selected.saturating_sub(rows - 1);
    let selected_y = list.y + (lines.len() + picker.selected - offset) as u16;
    for (i, model) in matches.iter().enumerate().skip(offset).take(rows) {
        let style = if i == picker.selected { Style::default().fg(ACCENT).bg(SELECTED_BG).add_modifier(Modifier::BOLD) } else { Style::default() };
        let mut spans = vec![Span::styled(format!(" {model} "), style)];
        if *model == current {
            spans.push(Span::styled("  current", dim));
        } else if let Some(savings) = stats.get(*model).and_then(|s| s.savings_label()) {
            spans.push(Span::styled(format!("  {savings}"), Style::default().fg(OK)));
        }
        lines.push(Line::from(spans));
    }
    f.render_widget(Paragraph::new(lines), list);
    f.render_widget(Paragraph::new(Line::styled("↑↓ select · Enter switch · Esc cancel", dim)), footer);

    if let Some(card) = card {
        // Tops line up with the highlighted row (its border with the row), kept on screen.
        let height = (card.len() as u16 + 2).min(full.height);
        let y = selected_y.saturating_sub(1).min(full.bottom() - height).max(full.y);
        let card_area = Rect { x: area.right() + 1, y, width: CARD_WIDTH, height };
        f.render_widget(Clear, card_area);
        let block = Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(dim).padding(ratatui::widgets::Padding::horizontal(1));
        f.render_widget(Paragraph::new(card).block(block), card_area);
    }
}

/// Columns the model detail card takes beside the picker, borders included.
const CARD_WIDTH: u16 = 40;

/// The `/model` picker's detail card, like the desktop app's hover card:
/// name, lab and id, models.dev's description (3 rows at most), context /
/// output / knowledge, capability chips and price, then ModelGateway's 24 h
/// error rate and savings. `None` when there is nothing to show beyond the id.
fn model_card(id: &str, info: Option<&models_dev::ModelInfo>, stat: Option<&model_stats::ModelStat>, width: usize) -> Option<Vec<Line<'static>>> {
    let gateway = stat.filter(|s| s.error_rate_label().is_some() || s.savings_label().is_some());
    if info.is_none() && gateway.is_none() {
        return None;
    }
    let dim = Style::default().fg(DIM);
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let clip = |s: &str| -> String {
        if s.chars().count() <= width {
            s.to_string()
        } else {
            s.chars().take(width.saturating_sub(1)).collect::<String>() + "…"
        }
    };
    // Label row over value row, columns two spaces apart.
    let columns = |cols: Vec<(&str, Vec<Span<'static>>)>| -> Vec<Line<'static>> {
        let (mut labels, mut values) = (Vec::new(), Vec::new());
        for (label, value) in cols {
            let w = label.chars().count().max(value.iter().map(|s| s.width()).sum()) + 2;
            labels.push(Span::styled(format!("{label:<w$}"), dim));
            let used: usize = value.iter().map(|s| s.width()).sum();
            values.extend(value);
            values.push(Span::raw(" ".repeat(w - used)));
        }
        vec![Line::from(labels), Line::from(values)]
    };

    let mut out: Vec<Line<'static>> = Vec::new();
    match info {
        Some(info) => {
            out.push(Line::styled(clip(&info.name), bold));
            out.push(Line::styled(clip(&format!("{} · {}", info.lab_name, info.id)), dim));
            if let Some(description) = &info.description {
                let mut rows = transcript::wrap(vec![Span::styled(description.clone(), dim)], width, vec![], vec![]);
                if rows.len() > 3 {
                    rows.truncate(3);
                    let last: String = rows[2].spans.iter().map(|s| s.content.as_ref()).collect();
                    rows[2] = Line::styled(clip(&format!("{}…", last.trim_end())), dim);
                }
                out.push(Line::raw(""));
                out.extend(rows);
            }
            let mut limits = Vec::new();
            if let Some(c) = info.context_window {
                limits.push(("Context", vec![Span::styled(models_dev::fmt_limit(c), bold)]));
            }
            if let Some(o) = info.max_output {
                limits.push(("Output", vec![Span::styled(models_dev::fmt_limit(o), bold)]));
            }
            if let Some(k) = &info.knowledge {
                limits.push(("Knowledge", vec![Span::styled(k.clone(), bold)]));
            }
            if !limits.is_empty() {
                out.push(Line::raw(""));
                out.extend(columns(limits));
            }
            // Agent runs need tool calling, so its absence is the one warning chip.
            let chip = Style::default().bg(SELECTED_BG);
            let mut chips = vec![if info.tool_call { ("Tools", chip) } else { ("⚠ No tool calling", chip.fg(WARN)) }];
            for (on, label) in [(info.reasoning, "Reasoning"), (info.vision, "Vision"), (info.open_weights, "Open weights")] {
                if on {
                    chips.push((label, chip));
                }
            }
            out.push(Line::raw(""));
            let mut row: Vec<Span<'static>> = Vec::new();
            let mut used = 0;
            for (label, style) in chips {
                let w = label.chars().count() + 2;
                if used > 0 && used + 1 + w > width {
                    out.push(Line::from(std::mem::take(&mut row)));
                    used = 0;
                }
                if used > 0 {
                    row.push(Span::raw(" "));
                    used += 1;
                }
                row.push(Span::styled(format!(" {label} "), style));
                used += w;
            }
            out.push(Line::from(row));
            if let Some(cost) = info.cost_line() {
                out.push(Line::styled(clip(&cost), dim));
            }
        }
        None => out.push(Line::styled(clip(id), bold)),
    }
    if let Some(stat) = gateway {
        // Green under 5%, amber under 20%.
        let mut cols = Vec::new();
        if let (Some(rate), Some(label)) = (stat.error_rate_24h, stat.error_rate_label()) {
            let tint = if rate < 5.0 { OK } else if rate < 20.0 { WARN } else { ERR };
            cols.push(("Errors (24h)", vec![Span::styled("● ", Style::default().fg(tint)), Span::styled(label, bold)]));
        }
        if let Some(savings) = stat.savings_label() {
            cols.push(("Cost", vec![Span::styled(savings, bold.fg(OK))]));
        }
        out.push(Line::raw(""));
        out.extend(columns(cols));
    }
    Some(out)
}

fn centered(area: Rect, pct_x: u16, pct_y: u16) -> Rect {
    let [_, mid, _] = Layout::vertical([Constraint::Percentage((100 - pct_y) / 2), Constraint::Percentage(pct_y), Constraint::Percentage((100 - pct_y) / 2)]).areas(area);
    let [_, cell, _] = Layout::horizontal([Constraint::Percentage((100 - pct_x) / 2), Constraint::Percentage(pct_x), Constraint::Percentage((100 - pct_x) / 2)]).areas(mid);
    cell
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picker(models: &[&str], current: &str) -> ModelPicker {
        let mut p = ModelPicker::loading();
        p.loaded(Ok(models.iter().map(|m| m.to_string()).collect()), current);
        p
    }

    #[test]
    fn picker_starts_on_the_current_model_and_filters_case_insensitively() {
        let mut p = picker(&["gpt-4.1", "gpt-4.1-mini", "o3"], "gpt-4.1-mini");
        assert_eq!(p.choice().as_deref(), Some("gpt-4.1-mini"));
        p.filter = "O3".into();
        p.selected = 0;
        assert_eq!(p.matches(), vec!["o3"]);
        assert_eq!(p.choice().as_deref(), Some("o3"));
    }

    #[test]
    fn picker_keeps_an_unlisted_current_model_and_accepts_typed_names() {
        let mut p = picker(&["a", "b"], "local-model");
        assert_eq!(p.models[0], "local-model");
        p.filter = " my-custom ".into();
        assert!(p.matches().is_empty());
        assert_eq!(p.choice().as_deref(), Some("my-custom"));

        let mut failed = ModelPicker::loading();
        failed.loaded(Err(CtxError::other("HTTP 404")), "m");
        assert_eq!(failed.models, vec!["m".to_string()]);
        assert!(failed.error.as_deref().unwrap().contains("404"));
    }

    #[test]
    fn picker_renders_matches_and_marks_the_current_model() {
        let mut p = picker(&["gpt-4.1", "gpt-4.1-mini", "o3"], "gpt-4.1");
        p.filter = "gpt".into();
        p.selected = 1;
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 20)).unwrap();
        let stats = ModelStats::from([("gpt-4.1-mini".to_string(), model_stats::ModelStat { savings_percent: Some(98.0), error_rate_24h: None })]);
        terminal.draw(|f| draw_picker(f, &p, "gpt-4.1", &stats, None, "")).unwrap();
        let screen: String = terminal.backend().buffer().content().chunks(80).map(|row| row.iter().map(|c| c.symbol()).collect::<String>() + "\n").collect();
        println!("{screen}");
        assert!(screen.contains("filter gpt"));
        assert!(screen.contains("gpt-4.1   current"));
        assert!(screen.contains("gpt-4.1-mini   98% off"));
        assert!(screen.contains("gpt-4.1-mini"));
        assert!(!screen.contains("o3"));
        assert!(screen.contains("switch model · 2 of 3"));
        assert!(screen.contains("Esc cancel"));
    }

    #[test]
    fn picker_shows_the_highlighted_models_card_to_the_right() {
        let api = br#"{"openai": {"name": "OpenAI", "models": {"gpt-4.1": {"id": "gpt-4.1", "name": "GPT-4.1",
            "description": "Long-lived GPT workhorse for coding, instruction following, and production apps", "tool_call": true,
            "modalities": {"input": ["text", "image"]}, "limit": {"context": 1047576, "output": 32768}, "knowledge": "2024-04", "cost": {"input": 2, "output": 8}}}}}"#;
        let index = ModelsDevIndex::parse(api).unwrap();
        let p = picker(&["gpt-4.1", "o3"], "o3");
        let mut p = p;
        p.selected = 0;
        let stats = ModelStats::from([("gpt-4.1".to_string(), model_stats::ModelStat { savings_percent: Some(98.0), error_rate_24h: Some(1.5) })]);
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 30)).unwrap();
        terminal.draw(|f| draw_picker(f, &p, "o3", &stats, Some(&index), "")).unwrap();
        let screen: String = terminal.backend().buffer().content().chunks(120).map(|row| row.iter().map(|c| c.symbol()).collect::<String>() + "\n").collect();
        println!("{screen}");
        for text in ["GPT-4.1", "OpenAI · gpt-4.1", "Long-lived GPT workhorse", "Context  Output  Knowledge", "1M       32K     2024-04", " Tools ", " Vision ", "$2 in · $8 out per M", "Errors (24h)  Cost", "● 1.5%        98% off"] {
            assert!(screen.contains(text), "missing {text:?}");
        }
        // A model with neither models.dev data nor gateway numbers has no card.
        p.selected = 1;
        terminal.draw(|f| draw_picker(f, &p, "o3", &stats, Some(&index), "")).unwrap();
        let screen: String = terminal.backend().buffer().content().chunks(120).map(|row| row.iter().map(|c| c.symbol()).collect::<String>() + "\n").collect();
        assert!(!screen.contains("Context"));
    }

    fn state_with_history(history: &[&str]) -> State {
        let now = chrono::Utc::now();
        let session = SessionRecord { id: "6f3a9c2e-0000".into(), repository_id: "r".into(), mode: "interactive".into(), model: "m".into(), active_task_id: None, created_at: now, updated_at: now, metadata: serde_json::json!({}) };
        State {
            mcp: None,
            files: None,
            transcript: Transcript::default(), tool_log: vec![], input: String::new(), state: "idle".into(), step: 0, task_started: None, usage: TokenUsage::estimated(0, 0), cost: None,
            index_generation: 0, indexing: None, running: false, model: "m".into(), profile: PermissionProfile::ApprovalRequired, effort: None, models_cache: vec![], model_stats: ModelStats::new(), models_dev: None, base_url: String::new(), completion: 0, completion_dismissed: false,
            history: history.iter().map(|h| h.to_string()).collect(), history_menu: None, agents_view: None, queued: vec![], steering: vec![], last_context: None, pending: None, picker: None, resume_picker: None, switching: None, scroll: 0, follow: true, page: 10,
            show_thinking: false, show_log: false, session, last_outcome: None, quit: false,
        }
    }

    fn press(state: &mut State, code: KeyCode) -> Option<Action> {
        handle_key(state, KeyEvent::new(code, KeyModifiers::NONE), &CancellationToken::new(), &mut false)
    }

    #[test]
    fn arrows_browse_earlier_prompts_and_restore_the_draft() {
        let mut s = state_with_history(&["fix the tests", "/approval auto", "add a changelog entry"]);
        s.input = "half-typed".into();
        press(&mut s, KeyCode::Up);
        assert_eq!(s.input, "add a changelog entry");
        assert!(s.suggestions().is_empty(), "the history menu owns the arrows");
        press(&mut s, KeyCode::Up);
        press(&mut s, KeyCode::Up);
        press(&mut s, KeyCode::Up);
        assert_eq!(s.input, "fix the tests", "stops at the oldest");
        press(&mut s, KeyCode::Down);
        assert_eq!(s.input, "/approval auto");
        press(&mut s, KeyCode::Down);
        press(&mut s, KeyCode::Down);
        assert_eq!(s.input, "half-typed", "past the newest brings back the draft");
        assert!(s.history_menu.is_none());

        press(&mut s, KeyCode::Up);
        press(&mut s, KeyCode::Esc);
        assert_eq!(s.input, "half-typed", "esc restores the draft");
        assert!(!s.quit, "esc closes the menu instead of quitting");
    }

    #[test]
    fn recalled_prompts_can_be_run_or_edited_and_are_recorded_once() {
        let mut s = state_with_history(&["fix the tests"]);
        press(&mut s, KeyCode::Up);
        press(&mut s, KeyCode::Char('!'));
        assert_eq!(s.input, "fix the tests!", "typing edits the recalled text");
        assert!(s.history_menu.is_none());
        assert!(matches!(press(&mut s, KeyCode::Enter), Some(Action::Run(p)) if p == "fix the tests!"));
        press(&mut s, KeyCode::Up);
        assert!(matches!(press(&mut s, KeyCode::Enter), Some(Action::Run(p)) if p == "fix the tests!"));
        assert_eq!(s.history, vec!["fix the tests".to_string(), "fix the tests!".to_string()], "repeats are not stored twice");
        let mut empty = state_with_history(&[]);
        press(&mut empty, KeyCode::Up);
        assert!(empty.history_menu.is_none() && empty.input.is_empty());
    }

    fn screen(state: &mut State, w: u16, h: u16) -> String {
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| draw(f, state)).unwrap();
        terminal.backend().buffer().content().chunks(w as usize).map(|row| row.iter().map(|c| c.symbol()).collect::<String>() + "\n").collect()
    }

    #[test]
    fn agents_view_lists_agents_and_opens_each_ones_conversation() {
        let task = || ctx_core::TaskId::from_string("a");
        let mut s = state_with_history(&[]);
        s.notice("hello");
        assert!(matches!(slash_command(&mut s, "/agents"), None) && s.agents_view.is_none(), "nothing to show yet");
        s.transcript.agent_started("agent-1".into(), "auth-explorer".into(), "explore".into(), "Find the login flow".into());
        s.transcript.agent_started("agent-2".into(), "api-implementer".into(), "implement".into(), "Add the endpoint".into());
        s.transcript.agent_event("agent-1", &AgentEvent::ToolCallStarted { task_id: task(), call_id: "c1".into(), name: "read_file".into(), arguments: serde_json::json!({"path": "src/auth.rs"}) });
        s.transcript.agent_event("agent-1", &AgentEvent::ToolCallFinished { task_id: task(), call_id: "c1".into(), name: "read_file".into(), success: true, summary: "40 lines".into(), duration_ms: 3 });
        s.transcript.agent_event("agent-1", &AgentEvent::ModelTextDelta { task_id: task(), text: "The handler checks the session cookie".into() });
        s.transcript.agent_event("agent-1", &AgentEvent::Usage { task_id: task(), cumulative: TokenUsage::estimated(12_000, 300), cost_usd: None });
        let report = ctx_core::AgentReport { agent_id: "agent-2".into(), name: "api-implementer".into(), mode: ctx_core::AgentMode::Implement, status: ctx_core::TaskStatus::Completed, summary: "Added `POST /login`.".into(), modified_files: vec!["src/api.rs".into()], unresolved: vec![], steps: 4, tool_calls: 6, usage: TokenUsage::estimated(5000, 100), cost_usd: None, duration_ms: 9100 };
        s.transcript.agent_finished("agent-2", &report);

        // ↓ on an empty prompt opens the list on the agent still running.
        press(&mut s, KeyCode::Down);
        assert_eq!(s.agents_view.as_ref().map(|v| v.selected), Some(0));
        let list = screen(&mut s, 100, 24);
        println!("{list}");
        assert!(list.contains("Agents  2 · 1 running"), "{list}");
        assert!(list.contains("auth-explorer · explore · 1 tool · 12.3k tokens"), "{list}");
        assert!(list.contains("⎿ The handler checks the session cookie"), "{list}");
        assert!(list.contains("✓ api-implementer · implement · completed · 4 steps · 6 tool calls · 9.1s · 5.1k tokens"), "{list}");
        assert!(list.contains("⎿ Added `POST /login`."), "{list}");
        assert!(list.contains("enter opens"), "{list}");
        assert!(!list.contains("hello"), "the view replaces the conversation");

        // Enter opens the agent's own conversation: brief, tools, text.
        press(&mut s, KeyCode::Enter);
        let open = screen(&mut s, 100, 24);
        println!("{open}");
        assert!(open.contains("auth-explorer · explore") && open.contains("1/2"), "{open}");
        assert!(open.contains("Find the login flow"), "{open}");
        assert!(open.contains("Read src/auth.rs"), "{open}");
        assert!(open.contains("The handler checks the session cookie"), "{open}");
        // Tab steps to the next agent; esc goes back to the list, then out.
        press(&mut s, KeyCode::Tab);
        assert_eq!(s.agents_view.as_ref().and_then(|v| v.open.clone()).as_deref(), Some("agent-2"));
        assert!(screen(&mut s, 100, 24).contains("Add the endpoint"));
        press(&mut s, KeyCode::Esc);
        assert!(s.agents_view.as_ref().is_some_and(|v| v.open.is_none()));
        press(&mut s, KeyCode::Esc);
        assert!(s.agents_view.is_none() && !s.quit, "esc closes the view before it quits");
        assert!(screen(&mut s, 100, 24).contains("hello"));
    }

    /// A store with one empty session and one with a task run twice
    /// (the second prompt continuing it), its last tool call never finished.
    fn store_with_history() -> (SessionStore, SessionRecord) {
        let store = SessionStore::in_memory().unwrap();
        let now = chrono::Utc::now();
        let session = |id: &str| SessionRecord { id: id.into(), repository_id: "r".into(), mode: "interactive".into(), model: "gpt-4.1".into(), active_task_id: None, created_at: now, updated_at: now, metadata: serde_json::json!({}) };
        store.create_session(&session("00000000-empty")).unwrap();
        let s = session("6f3a9c2e-work");
        store.create_session(&s).unwrap();
        let task = ctx_storage::TaskRecord { id: "t1".into(), session_id: s.id.clone(), repository_id: "r".into(), objective: "fix the login bug".into(), category: "localized_edit".into(), state: "idle".into(), status: Some("completed".into()), parent_task_id: None, summary: None, state_json: serde_json::json!({}), created_at: now, updated_at: now, completed_at: None };
        store.create_task(&task).unwrap();
        let user = |text: &str| serde_json::to_value(ctx_core::AgentMessage::User(UserMessage { text: text.into(), ..Default::default() })).unwrap();
        let event = |e: AgentEvent| store.append_event(Some(&s.id), Some("t1"), e.name(), &serde_json::to_value(&e).unwrap()).unwrap();
        let t = || ctx_core::TaskId::from_string("t1");
        store.append_message(&s.id, Some("t1"), &user("fix the login bug")).unwrap();
        event(AgentEvent::TaskStarted { task_id: t(), objective: "fix the login bug".into(), continued: false });
        event(AgentEvent::ToolCallStarted { task_id: t(), call_id: "c1".into(), name: "read_file".into(), arguments: serde_json::json!({"path": "src/auth.rs"}) });
        event(AgentEvent::ToolCallFinished { task_id: t(), call_id: "c1".into(), name: "read_file".into(), success: true, summary: "40 lines".into(), duration_ms: 3 });
        event(AgentEvent::ModelResponse { task_id: t(), text: Some("Fixed the redirect in `src/auth.rs`.".into()), tool_call_count: 0, usage: None });
        store.append_message(&s.id, Some("t1"), &user("also add a test")).unwrap();
        event(AgentEvent::TaskStarted { task_id: t(), objective: "fix the login bug".into(), continued: true });
        event(AgentEvent::ToolCallStarted { task_id: t(), call_id: "c2".into(), name: "execute_command".into(), arguments: serde_json::json!({"command": "cargo test"}) });
        (store, s)
    }

    #[test]
    fn resuming_replays_the_conversation_from_stored_events() {
        let (store, session) = store_with_history();
        let mut s = state_with_history(&[]);
        s.notice("from the old session");
        resume_session(&mut s, &store, session.clone());
        assert_eq!(s.session.id, session.id);
        let rows: Vec<String> = s.transcript.render(100, false).iter().map(|l| l.spans.iter().map(|sp| sp.content.as_ref()).collect()).collect();
        println!("{rows:#?}");
        let at = |text: &str| rows.iter().position(|r| r.contains(text)).unwrap_or_else(|| panic!("missing {text:?}: {rows:#?}"));
        assert!(!rows.iter().any(|r| r.contains("from the old session")), "the old conversation is gone");
        assert!(at("› fix the login bug") < at("Read src/auth.rs"));
        assert!(at("Read src/auth.rs") < at("Fixed the redirect"));
        assert!(at("Fixed the redirect") < at("› also add a test"), "a continued task's prompt is the message, not the objective");
        assert!(at("› also add a test") < at("cargo test"));
        assert!(at("interrupted") > at("cargo test"), "the unfinished call is closed");
        assert!(at("resumed session 6f3a9c2e · 2 prompts") > at("cargo test"));

        // The empty session is left out of the list, and the open one too.
        let list = resume::list(&store, "r", None).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].title.as_str(), list[0].prompts, list[0].tasks), ("fix the login bug", 2, 1));
        assert!(resume::list(&store, "r", Some(&session.id)).unwrap().is_empty());
        assert_eq!(resume::find(&store, "r", "6f3a").unwrap().map(|s| s.id), Some(session.id.clone()));
        assert!(resume::find(&store, "r", "zzz").unwrap().is_none());
    }

    #[test]
    fn resume_picker_lists_filters_and_resumes() {
        let (store, session) = store_with_history();
        let mut s = state_with_history(&[]);
        assert!(matches!(slash_command(&mut s, "/resume"), Some(Action::ListSessions(f)) if f.is_empty()));
        s.resume_picker = Some(ResumePicker::new(resume::list(&store, "r", None).unwrap(), ""));
        let shown = screen(&mut s, 100, 24);
        println!("{shown}");
        assert!(shown.contains("resume a conversation · 1 of 1"), "{shown}");
        assert!(shown.contains("› fix the login bug") && shown.contains("just now · 2 prompts"), "{shown}");
        assert!(shown.contains("6f3a9c2e · gpt-4.1 · 1 task · completed"), "{shown}");
        press(&mut s, KeyCode::Char('q'));
        assert!(screen(&mut s, 100, 24).contains("no conversation matches"));
        press(&mut s, KeyCode::Backspace);
        assert!(matches!(press(&mut s, KeyCode::Enter), Some(Action::Resume(r)) if r.id == session.id));
        assert!(s.resume_picker.is_none());
        s.running = true;
        assert!(slash_command(&mut s, "/resume").is_none(), "not while a task runs");
    }

    #[test]
    fn while_a_task_runs_enter_steers_and_tab_or_cmd_enter_queues() {
        let mut s = state_with_history(&[]);
        s.running = true;
        s.input = "also the docs".into();
        assert!(matches!(press(&mut s, KeyCode::Enter), Some(Action::Steer(p)) if p == "also the docs"));
        assert!(s.input.is_empty());
        s.input = "then commit".into();
        assert!(matches!(press(&mut s, KeyCode::Tab), Some(Action::Queue(p)) if p == "then commit"));
        s.input = "and push".into();
        let cmd_enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::SUPER);
        assert!(matches!(handle_key(&mut s, cmd_enter, &CancellationToken::new(), &mut false), Some(Action::Queue(p)) if p == "and push"));
        s.running = false;
        s.input = "next".into();
        assert!(matches!(press(&mut s, KeyCode::Tab), Some(Action::Run(p)) if p == "next"), "idle: tab just runs");
    }
}
