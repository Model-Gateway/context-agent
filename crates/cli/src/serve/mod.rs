//! `ctx serve`: a JSON-RPC 2.0 server over stdin/stdout for the desktop app
//! (`docs/desktop-protocol.md`). One request per line, one frame per line;
//! nothing but protocol frames is ever written to stdout (logs go to the
//! log file). Every request runs in its own task; one writer task owns
//! stdout so frames never interleave.

use crate::app;
use crate::model_stats::{self, ModelStats};
use crate::Cli;
use ctx_agent::{AgentRuntime, Inbox, RepositoryContext, RunOptions, SessionManager};
use ctx_core::{AgentEvent, AgentMessage, ApprovalDecision, FileInput, ImageInput, UserMessage, ApprovalRequest, CancellationToken, Config, CtxError, LoadOptions, PermissionProfile, ProviderError, RepositoryId, Result, SandboxKind, TaskOutcome, TaskStatus, TokenUsage, UsageSource};
use ctx_indexing::indexer::IndexProgress;
use ctx_indexing::RepositoryDiscovery;
use ctx_providers::{build_provider, build_provider_for_project, load_profile, save_profile, CapabilityProfile, MockProvider, MockTurn, ModelProvider};
use ctx_security::Redactor;
use ctx_storage::{EventRecord, SessionRecord, SessionStore, TaskRecord};
use ctx_tools::{ApprovalHandler, DenyAll, McpManager, RememberingApprovals};
use parking_lot::{Mutex, RwLock};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot, watch, Notify};
use tokio::task::JoinHandle;

mod browser;
mod dto;
mod gateway;
mod gitops;
mod lsp;
mod question;
mod runner;
mod snapshot;
mod spending;
mod suggest;
mod support;
mod worktree;

use browser::*;
use question::*;
use dto::*;
use lsp::*;
use runner::*;
use support::*;
pub(crate) use support::{mcp_doctor_line, write_private_atomic};

const PROTOCOL_VERSION: u32 = 1;
const PROBE_TIMEOUT: Duration = Duration::from_secs(120);
const MODELS_CACHE_TTL: Duration = Duration::from_secs(600);
const DEFAULT_FILE_MAX_BYTES: usize = 262_144;
/// Upper bound for `file/read.max_bytes`, so one response frame stays bounded.
const FILE_MAX_BYTES_CAP: usize = 16 * 1024 * 1024;
/// Queued stdout frames above which event forwarding pauses until the client
/// catches up (responses and `task/finished` are never held back).
const OUTBOX_HIGH_WATER: usize = 16_384;
const OUTBOX_LOW_WATER: usize = 4_096;
/// How long shutdown waits for cancelled runs to report `task/finished`.
const SHUTDOWN_RUN_GRACE: Duration = Duration::from_secs(5);
/// How long `project/open` waits for the cancelled run of a project that was
/// closed while running, so a new runtime never works on the same session
/// while the old one is still unwinding.
const REOPEN_RUN_GRACE: Duration = Duration::from_secs(30);
const DIFF_CAP_BYTES: usize = 2 * 1024 * 1024;
const EVENT_SINK_CAPACITY: usize = 4096;
/// Dotted key prefixes `config/patch` accepts.
const PATCH_PREFIXES: &[&str] = &["model.", "permissions.", "pricing.", "spend.", "agent.", "context.", "validation.", "telemetry.", "embeddings.", "index.", "mcp."];

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const BACKEND_ERROR: i64 = -32000;
const PROJECT_NOT_OPEN: i64 = -32001;
const TASK_RUNNING: i64 = -32002;
const NO_TASK_RUNNING: i64 = -32003;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct RpcError {
    code: i64,
    message: String,
    data: Option<Value>,
}

impl RpcError {
    fn new(code: i64, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), data: None }
    }

    fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(INVALID_PARAMS, message)
    }

    fn project_not_open(id: &str) -> Self {
        Self::new(PROJECT_NOT_OPEN, format!("project {id} is not open"))
    }

    fn task_running() -> Self {
        Self::new(TASK_RUNNING, "a task is already running in this project")
    }

    fn session_task_running() -> Self {
        Self::new(TASK_RUNNING, "a task is already running in this session")
    }

    fn no_task_running() -> Self {
        Self::new(NO_TASK_RUNNING, "no task is running in this project")
    }

    fn to_json(&self) -> Value {
        let mut err = json!({"code": self.code, "message": self.message});
        if let Some(data) = &self.data {
            err["data"] = data.clone();
        }
        err
    }
}

impl From<CtxError> for RpcError {
    fn from(e: CtxError) -> Self {
        Self { code: BACKEND_ERROR, message: e.to_string(), data: Some(json!({"kind": e.code()})) }
    }
}

impl From<serde_json::Error> for RpcError {
    fn from(e: serde_json::Error) -> Self {
        RpcError::from(CtxError::from(e))
    }
}

type RpcResult<T> = std::result::Result<T, RpcError>;

/// The name a project is shown and attributed under: its root folder's name.
fn project_name(root: &Path) -> String {
    root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| root.display().to_string())
}

fn parse<T: DeserializeOwned>(params: Value) -> RpcResult<T> {
    serde_json::from_value(params).map_err(|e| RpcError::invalid_params(format!("invalid params: {e}")))
}

fn to_value<T: Serialize>(value: &T) -> RpcResult<Value> {
    Ok(serde_json::to_value(value)?)
}

/// Run blocking work (SQLite, git, repository walks) off the async threads.
async fn blocking<T, F>(f: F) -> RpcResult<T>
where
    F: FnOnce() -> Result<T> + Send + 'static,
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(f).await {
        Ok(r) => r.map_err(RpcError::from),
        Err(e) => Err(RpcError::from(CtxError::other(format!("background task failed: {e}")))),
    }
}

/// Like [`blocking`] for work that reports protocol-level errors itself.
async fn blocking_rpc<T, F>(f: F) -> RpcResult<T>
where
    F: FnOnce() -> RpcResult<T> + Send + 'static,
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(f).await {
        Ok(r) => r,
        Err(e) => Err(RpcError::from(CtxError::other(format!("background task failed: {e}")))),
    }
}

// ---------------------------------------------------------------------------
// Output frames
// ---------------------------------------------------------------------------

enum Out {
    Line(String),
    Flush(oneshot::Sender<()>),
}

/// The one queue in front of stdout. Frames are never dropped; the queue
/// depth is tracked so the high-volume event stream can wait for the client
/// to catch up (`wait_for_room`) instead of buffering without bound while
/// the app has stopped reading its pipe.
#[derive(Clone)]
struct Outbox {
    tx: mpsc::UnboundedSender<Out>,
    queued: Arc<AtomicUsize>,
    drained: Arc<Notify>,
}

impl Outbox {
    fn new() -> (Self, mpsc::UnboundedReceiver<Out>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Self { tx, queued: Arc::new(AtomicUsize::new(0)), drained: Arc::new(Notify::new()) }, rx)
    }

    fn line(&self, line: String) {
        self.queued.fetch_add(1, Ordering::SeqCst);
        if self.tx.send(Out::Line(line)).is_err() {
            self.queued.fetch_sub(1, Ordering::SeqCst);
        }
    }

    fn flush(&self, done: oneshot::Sender<()>) {
        let _ = self.tx.send(Out::Flush(done));
    }

    fn notify(&self, method: &str, params: Value) {
        let frame = json!({"jsonrpc": "2.0", "method": method, "params": params});
        self.line(frame.to_string());
    }

    /// Whether the client has fallen far behind; advisory frames (index
    /// progress) are skipped while this holds.
    fn is_congested(&self) -> bool {
        self.queued.load(Ordering::SeqCst) >= OUTBOX_HIGH_WATER
    }

    /// Wait until the queue has drained below the high-water mark.
    async fn wait_for_room(&self) {
        loop {
            let notified = self.drained.notified();
            let mut notified = std::pin::pin!(notified);
            notified.as_mut().enable();
            if !self.is_congested() {
                return;
            }
            notified.await;
        }
    }
}

async fn writer_task(mut rx: mpsc::UnboundedReceiver<Out>, queued: Arc<AtomicUsize>, drained: Arc<Notify>) {
    let mut stdout = tokio::io::stdout();
    while let Some(msg) = rx.recv().await {
        match msg {
            Out::Line(mut line) => {
                line.push('\n');
                let written = stdout.write_all(line.as_bytes()).await.is_ok();
                let _ = stdout.flush().await;
                let left = queued.fetch_sub(1, Ordering::SeqCst).saturating_sub(1);
                if left < OUTBOX_LOW_WATER {
                    drained.notify_waiters();
                }
                if !written {
                    break;
                }
            }
            Out::Flush(done) => {
                let _ = stdout.flush().await;
                let _ = done.send(());
            }
        }
    }
    // stdout is gone: nothing drains any more, so release every waiter.
    queued.store(0, Ordering::SeqCst);
    drained.notify_waiters();
}

// ---------------------------------------------------------------------------
// Server state
// ---------------------------------------------------------------------------

/// How the user-level configuration is (re)loaded: the CLI's own options
/// without any repository root, so a repository-level file cannot alter
/// the user configuration the app shows and edits.
struct ConfigSource {
    explicit_path: Option<PathBuf>,
    data_dir: Option<PathBuf>,
    model: Option<String>,
    profile: Option<String>,
}

impl ConfigSource {
    fn from_cli(cli: &Cli) -> Self {
        Self { explicit_path: cli.config.clone(), data_dir: cli.data_dir.clone(), model: cli.model.clone(), profile: cli.profile.clone() }
    }

    fn load(&self) -> Result<Config> {
        self.load_for(None)
    }

    /// The configuration for a project: the same layers as `ctx run`,
    /// including `<root>/.context-agent.toml` (only the tables the core
    /// crate trusts from a repository), plus the CLI overrides.
    fn load_for(&self, repository_root: Option<&Path>) -> Result<Config> {
        let mut config = Config::load(&LoadOptions { explicit_path: self.explicit_path.clone(), repository_root: repository_root.map(Path::to_path_buf), data_dir: self.data_dir.clone() })?;
        if let Some(model) = &self.model {
            config.model.name = model.clone();
        }
        if let Some(profile) = &self.profile {
            config.permissions.profile = PermissionProfile::parse(profile).ok_or_else(|| CtxError::Config(format!("unknown permission profile `{profile}`")))?;
        }
        config.validate()?;
        Ok(config)
    }

    /// A model named explicitly (`--model` / `CTX_MODEL_NAME`) wins over a
    /// workspace's remembered model.
    fn explicit_model(&self) -> bool {
        self.model.is_some() || std::env::var_os("CTX_MODEL_NAME").is_some()
    }
}

struct Server {
    config: RwLock<Config>,
    source: ConfigSource,
    projects: RwLock<HashMap<String, Arc<Project>>>,
    out: Outbox,
    mock: bool,
    /// `/models` listings per base URL.
    models_cache: Mutex<HashMap<String, (Instant, Vec<String>)>>,
    /// Serializes `config/patch` (a read-merge-write of one file).
    patch_lock: tokio::sync::Mutex<()>,
    /// Indexing tasks of projects closed while they were still running: a
    /// reopen waits for them instead of indexing the same store twice.
    orphaned_indexing: Mutex<HashMap<String, JoinHandle<()>>>,
    /// Runs of projects closed while tasks were running: cancelled on close,
    /// awaited (bounded) by a reopen and by shutdown so a second runtime never
    /// continues the same session while the first is still unwinding.
    orphaned_runs: Mutex<HashMap<String, Vec<JoinHandle<()>>>>,
    /// One async lock per repository id, so concurrent `project/open` calls
    /// for one root build a single `RepositoryContext` instead of racing on
    /// the SQLite stores.
    open_locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    /// Process-level redaction for `log` notifications and error strings.
    redactor: Redactor,
    /// `initialize.host`: the machine this server runs on, read once.
    host: HostInfo,
    /// Cancelled on shutdown; every project's token is a child, so probes
    /// and runs stop with the process.
    lifetime: CancellationToken,
    /// Language servers of the desktop editor (serve/lsp.rs).
    lsp: Arc<LspSessions>,
    /// The user's MCP servers, shared by every project's runs (`--mock`
    /// connects none).
    mcp: Arc<McpManager>,
    shutdown: Notify,
}

/// Approval plumbing for one project, keyed by [`approval_key`] (session and
/// request id: two sessions whose providers both number calls `call_0` must
/// never answer each other's prompts). The runtime emits
/// `approval_requested` before the run's `SessionApprovals` files the reply
/// channel, so a decision may arrive before its reply channel exists; it is
/// kept in `early` until the pair shows up. One lock guards all three maps so the two
/// check-then-insert sequences (`pair`, `respond`) cannot interleave and
/// strand a decision in `early` while its sender sits in `pending`.
#[derive(Default)]
pub(crate) struct ApprovalState {
    inner: Mutex<ApprovalInner>,
}

#[derive(Default)]
struct ApprovalInner {
    pending: HashMap<String, oneshot::Sender<ApprovalDecision>>,
    early: HashMap<String, ApprovalDecision>,
    /// Request ids announced by `approval_requested` and not yet resolved.
    announced: HashSet<String>,
}

impl ApprovalState {
    fn announce(&self, id: &str) {
        self.inner.lock().announced.insert(id.to_string());
    }

    /// The reply channel for `id` arrived from the runtime: answer it from
    /// an early decision or park it until the client answers.
    fn pair(&self, id: &str, reply: oneshot::Sender<ApprovalDecision>) {
        let mut inner = self.inner.lock();
        match inner.early.remove(id) {
            Some(decision) => {
                drop(inner);
                let _ = reply.send(decision);
            }
            None => {
                inner.pending.insert(id.to_string(), reply);
            }
        }
    }

    /// The client's decision for `id`; `false` when the id is unknown.
    fn respond(&self, id: &str, decision: ApprovalDecision) -> bool {
        let mut inner = self.inner.lock();
        if let Some(reply) = inner.pending.remove(id) {
            drop(inner);
            let _ = reply.send(decision);
            return true;
        }
        if inner.announced.contains(id) {
            inner.early.insert(id.to_string(), decision);
            return true;
        }
        false
    }

    /// `approval_resolved` for `id`. Only the bookkeeping of the resolved
    /// request is dropped: a sender still in `pending` can only belong to a
    /// *newer* request with the same id (ids such as `approval_call_0`
    /// repeat across steps) whose pair raced ahead of this event, because a
    /// resolved request's sender was consumed by `respond`/`pair` or never
    /// existed (remembered approvals). Dropping it would silently deny that
    /// request.
    fn clear(&self, id: &str) {
        let mut inner = self.inner.lock();
        inner.early.remove(id);
        inner.announced.remove(id);
    }

    fn clear_all(&self) {
        let mut inner = self.inner.lock();
        inner.pending.clear();
        inner.early.clear();
        inner.announced.clear();
    }

    /// Forget one session's requests; a pending one is answered with Deny
    /// (its sender is dropped).
    fn clear_session(&self, session_id: &str) {
        let prefix = approval_key(session_id, "");
        let mut inner = self.inner.lock();
        inner.pending.retain(|k, _| !k.starts_with(&prefix));
        inner.early.retain(|k, _| !k.starts_with(&prefix));
        inner.announced.retain(|k| !k.starts_with(&prefix));
    }

    /// The key of request `id` in whichever session asked it, for clients
    /// that answer without naming the session (a pending request first).
    fn key_for(&self, id: &str) -> Option<String> {
        let suffix = format!("{APPROVAL_KEY_SEPARATOR}{id}");
        let inner = self.inner.lock();
        let found = inner.pending.keys().find(|k| k.ends_with(&suffix)).or_else(|| inner.announced.iter().find(|k| k.ends_with(&suffix)));
        found.cloned()
    }
}

const APPROVAL_KEY_SEPARATOR: char = '\u{1f}';

/// Where [`ApprovalState`] files request `id` of `session_id`.
fn approval_key(session_id: &str, id: &str) -> String {
    format!("{session_id}{APPROVAL_KEY_SEPARATOR}{id}")
}

/// Flips a project's `index_busy` flag back when the indexing task ends,
/// however it ends.
struct IndexBusy(watch::Sender<bool>);

impl Drop for IndexBusy {
    fn drop(&mut self) {
        self.0.send_replace(false);
    }
}

/// One session's run slot.
struct RunningTask {
    session_id: String,
    task_id: Option<String>,
    started_at: ctx_core::Timestamp,
    cancel: CancellationToken,
    /// Where `task/steer` hands messages to the run.
    inbox: Inbox,
    handle: Option<JoinHandle<()>>,
}

struct Project {
    id: String,
    ctx: Arc<RepositoryContext>,
    /// The repository's files for `@` completion (`files/search`).
    files: Arc<ctx_tools::mentions::FileIndex>,
    /// Per-project configuration: the model may differ from the user level
    /// after `models/switch`.
    config: RwLock<Config>,
    provider: RwLock<Arc<dyn ModelProvider>>,
    /// Permission profile for the next runs (`permissions/set`).
    profile: RwLock<PermissionProfile>,
    /// Holds the "approve for session" answers of the project's runs: every
    /// run asks through [`RememberingApprovals::sharing`] it (its own inner
    /// handler is never called), so they survive across runs and sessions.
    remembering: Arc<RememberingApprovals>,
    approvals: Arc<ApprovalState>,
    /// The runs' `browser` requests waiting for the app (serve/browser.rs).
    browser: Arc<BrowserState>,
    /// The runs' `ask_user` questions waiting for the app (serve/question.rs).
    questions: Arc<QuestionState>,
    /// Run slots by session id: one run per session, any number per project.
    running: Mutex<HashMap<String, RunningTask>>,
    indexing: Mutex<Option<JoinHandle<()>>>,
    /// `true` from `start_indexing` until the indexing task has sent
    /// `index/ready`; runs and rebuilds wait on it (the `JoinHandle` above
    /// is only for ownership, so `indexing()` keeps telling the truth).
    index_busy: watch::Sender<bool>,
    index_busy_rx: watch::Receiver<bool>,
    /// Set by `project/close`: a still-running indexing task then keeps
    /// its notifications to itself.
    closed: Arc<AtomicBool>,
    /// Cancelled by `project/close` (and, through its parent, by shutdown):
    /// runs and probes of this project are children of it, so a hung 120 s
    /// probe ends with the project instead of outliving it.
    lifetime: CancellationToken,
    /// Sessions with a `session/delete` in flight; `task/run` refuses them.
    deleting: Mutex<HashSet<String>>,
}

/// Whether a run slot is taken (reserved or still running).
fn slot_taken(run: &RunningTask) -> bool {
    run.handle.as_ref().map(|h| !h.is_finished()).unwrap_or(true)
}

/// Whether any session's slot is taken.
fn any_slot_taken(running: &HashMap<String, RunningTask>) -> bool {
    running.values().any(slot_taken)
}

/// Whether `session_id`'s slot is taken.
fn session_slot_taken(running: &HashMap<String, RunningTask>, session_id: &str) -> bool {
    running.get(session_id).is_some_and(slot_taken)
}

impl Project {
    /// The running tasks, oldest first, dropping slots whose task ended
    /// without clearing them (a panic) so no session gets stuck.
    fn running_tasks(&self) -> Vec<RunningTaskInfo> {
        let mut guard = self.running.lock();
        guard.retain(|_, r| slot_taken(r));
        let mut tasks: Vec<&RunningTask> = guard.values().collect();
        tasks.sort_by_key(|r| r.started_at);
        tasks.into_iter().map(|r| RunningTaskInfo { session_id: r.session_id.clone(), task_id: r.task_id.clone(), started_at: r.started_at.to_rfc3339() }).collect()
    }

    /// Whether any session of the project is running a task.
    fn is_running(&self) -> bool {
        !self.running_tasks().is_empty()
    }

    fn model(&self) -> String {
        self.config.read().model.name.clone()
    }

    fn indexing(&self) -> bool {
        *self.index_busy_rx.borrow()
    }

    /// Wait until no indexing pass is running for this project.
    async fn wait_index_idle(&self) {
        let mut rx = self.index_busy_rx.clone();
        let _ = rx.wait_for(|busy| !*busy).await;
    }
}

// ---------------------------------------------------------------------------
// Entry point and request loop
// ---------------------------------------------------------------------------

pub async fn run(cli: &Cli, mock: bool) -> Result<i32> {
    let source = ConfigSource::from_cli(cli);
    let config = source.load()?;
    // Logs go to the file: stdout carries protocol frames only.
    app::init_logging(cli, &config, true);
    crate::billing::install(&config);
    let (out, out_rx) = Outbox::new();
    let writer = tokio::spawn(writer_task(out_rx, out.queued.clone(), out.drained.clone()));
    let mut redactor = Redactor::new(config.telemetry.redact_secrets);
    for model in config.model.chain() {
        if let Some(key) = ctx_core::resolve_api_key(&config, &model) {
            redactor.add_known_value(key);
        }
    }
    let redactor = redactor.with_environment_secrets();
    // stdio servers without a `cwd` run in the home directory: the server
    // outlives any one project.
    let mcp_cwd = ctx_core::platform::home_dir().filter(|h| h.is_dir()).unwrap_or_else(|| config.data_dir.clone());
    let mcp = if mock { McpManager::disabled() } else { McpManager::new(&config.mcp, mcp_cwd, Some(config.layout())) };
    let server = Arc::new(Server {
        config: RwLock::new(config),
        source,
        projects: RwLock::new(HashMap::new()),
        out,
        mock,
        models_cache: Mutex::new(HashMap::new()),
        patch_lock: tokio::sync::Mutex::new(()),
        orphaned_indexing: Mutex::new(HashMap::new()),
        orphaned_runs: Mutex::new(HashMap::new()),
        open_locks: Mutex::new(HashMap::new()),
        redactor,
        host: host_info(),
        lifetime: CancellationToken::new(),
        shutdown: Notify::new(),
        lsp: Arc::new(LspSessions::default()),
        mcp,
    });
    tracing::info!(mock, "ctx serve started");
    // Every change of an MCP server's state (connected, failed, new tool
    // list) reaches the app as `mcp/status`.
    {
        let server = server.clone();
        let mut changes = server.mcp.subscribe();
        tokio::spawn(async move {
            while changes.changed().await.is_ok() {
                server.notify("mcp/status", json!({"servers": server.mcp.status()}));
            }
        });
    }

    // Raw byte segments rather than `lines()`: a line that is not valid
    // UTF-8 is a parse error for that one frame (`-32700`, id null), never a
    // reason to exit and drop every open project and running task.
    let mut segments = BufReader::new(tokio::io::stdin()).split(b'\n');
    loop {
        tokio::select! {
            segment = segments.next_segment() => match segment {
                Ok(Some(bytes)) => {
                    let mut line = match String::from_utf8(bytes) {
                        Ok(line) => line,
                        Err(e) => {
                            tracing::warn!(error = %e, "stdin line is not valid UTF-8");
                            server.respond(Value::Null, Err(RpcError::new(PARSE_ERROR, format!("parse error: invalid UTF-8: {}", e.utf8_error()))));
                            continue;
                        }
                    };
                    if line.ends_with('\r') {
                        line.pop();
                    }
                    if line.trim().is_empty() {
                        continue;
                    }
                    let server = server.clone();
                    tokio::spawn(async move { server.dispatch_line(line).await });
                }
                Ok(None) => break,
                Err(e) => {
                    tracing::warn!(error = %e, "stdin read failed; shutting down");
                    break;
                }
            },
            _ = server.shutdown.notified() => break,
        }
    }

    // Stop running tasks (and pending probes) cooperatively and make sure
    // every queued frame is on the wire before the process exits.
    server.lifetime.cancel();
    let projects: Vec<Arc<Project>> = server.projects.read().values().cloned().collect();
    let mut handles = Vec::new();
    for p in projects {
        let cancelled = {
            let mut running = p.running.lock();
            for r in running.values_mut() {
                r.cancel.cancel();
                if let Some(handle) = r.handle.take() {
                    handles.push(handle);
                }
            }
            !running.is_empty()
        };
        if cancelled {
            // A run waiting on an approval never observes the token: dropping
            // its reply channel answers it (Deny) so it can report
            // `task/finished` inside the grace period.
            p.approvals.clear_all();
        }
    }
    // Runs of projects closed while running were cancelled by the close and
    // still owe a `task/finished`.
    handles.extend(server.orphaned_runs.lock().drain().flat_map(|(_, h)| h));
    // Give cancelled runs one bounded, shared chance to report `task/finished`.
    let _ = tokio::time::timeout(SHUTDOWN_RUN_GRACE, futures::future::join_all(handles)).await;
    let _ = tokio::time::timeout(Duration::from_secs(3), server.mcp.shutdown()).await;
    let (done_tx, done_rx) = oneshot::channel();
    server.out.flush(done_tx);
    let _ = tokio::time::timeout(Duration::from_secs(2), done_rx).await;
    drop(writer);
    Ok(0)
}

impl Server {
    fn notify(&self, method: &str, params: Value) {
        self.out.notify(method, params);
    }

    /// A `log` notification (and a log-file line). Messages go through the
    /// redactor so keys or `https://user:token@host` URLs never reach the
    /// client or the log.
    fn log(&self, level: &str, message: impl Into<String>) {
        let message = self.redactor.redact(&message.into());
        tracing::info!(level, "{message}");
        self.notify("log", json!({"level": level, "message": message}));
    }

    /// Queue a response. Error messages are redacted here, in one place:
    /// they carry paths, provider bodies and base URLs (`https://user:token@host`)
    /// verbatim, and nothing secret may reach the client or the log.
    fn respond(&self, id: Value, result: RpcResult<Value>) {
        let frame = match result {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err(e) => {
                let mut error = e.to_json();
                self.redactor.redact_json(&mut error);
                json!({"jsonrpc": "2.0", "id": id, "error": error})
            }
        };
        self.out.line(frame.to_string());
    }

    async fn flush(&self) {
        let (tx, rx) = oneshot::channel();
        self.out.flush(tx);
        let _ = tokio::time::timeout(Duration::from_secs(2), rx).await;
    }

    async fn dispatch_line(self: Arc<Self>, line: String) {
        let value: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                self.respond(Value::Null, Err(RpcError::new(PARSE_ERROR, format!("parse error: {e}"))));
                return;
            }
        };
        // Only an *absent* `id` makes a notification: JSON-RPC 2.0 allows
        // (and discourages) `"id": null` for a request, and a client that
        // sends it would otherwise wait forever.
        let id = value.get("id").cloned();
        let method = value.get("method").and_then(|m| m.as_str()).map(str::to_string);
        let is_request = value.is_object() && value.get("jsonrpc").and_then(|v| v.as_str()) == Some("2.0");
        let Some(method) = method.filter(|_| is_request) else {
            self.respond(id.unwrap_or(Value::Null), Err(RpcError::new(INVALID_REQUEST, "invalid request: expected {jsonrpc: \"2.0\", method: string}")));
            return;
        };
        // `params` is by-name only: a positional array would be accepted by
        // serde for some structs and silently mean something else.
        let params = match value.get("params") {
            None | Some(Value::Null) => json!({}),
            Some(object @ Value::Object(_)) => object.clone(),
            Some(_) => {
                if let Some(id) = id {
                    self.respond(id, Err(RpcError::invalid_params("invalid params: expected an object")));
                }
                return;
            }
        };
        tracing::debug!(%method, "request");
        let result = self.handle(&method, params).await;
        if let Some(id) = id {
            self.respond(id, result);
        }
        if method == "shutdown" {
            self.flush().await;
            self.shutdown.notify_one();
        }
    }

    async fn handle(self: &Arc<Self>, method: &str, params: Value) -> RpcResult<Value> {
        match method {
            "initialize" => to_value(&self.config_info()),
            "config/get" => {
                self.reload_config().await?;
                to_value(&self.config_info())
            }
            "config/patch" => self.config_patch(parse(params)?).await,
            "project/open" => self.project_open(parse(params)?).await,
            "project/close" => self.project_close(parse(params)?),
            "project/status" => {
                let p = self.project(&parse::<ProjectParams>(params)?.project_id)?;
                to_value(&self.project_info(&p).await?)
            }
            "index/rebuild" => self.index_rebuild(parse(params)?).await,
            "sessions/list" => self.sessions_list(parse(params)?).await,
            "session/create" => self.session_create(parse(params)?).await,
            "session/rename" => self.session_rename(parse(params)?).await,
            "session/delete" => self.session_delete(parse(params)?).await,
            "session/rewind" => self.session_rewind(parse(params)?).await,
            "session/history" => self.session_history(parse(params)?).await,
            "task/run" => self.task_run(parse(params)?).await,
            "task/steer" => self.task_steer(parse(params)?).await,
            "task/cancel" => self.task_cancel(parse(params)?),
            "approval/respond" => self.approval_respond(parse(params)?),
            "browser/respond" => self.browser_respond(parse(params)?),
            "question/respond" => self.question_respond(parse(params)?),
            "permissions/set" => self.permissions_set(parse(params)?).await,
            "models/list" => self.models_list(parse(params)?).await,
            "models/switch" => self.models_switch(parse(params)?).await,
            "suggest/next" => self.suggest_next(parse(params)?).await,
            "suggest/starters" => self.suggest_starters(parse(params)?).await,
            "git/status" => self.git_status(parse(params)?).await,
            "git/stage" => self.git_stage(parse(params)?).await,
            "git/unstage" => self.git_unstage(parse(params)?).await,
            "git/discard" => self.git_discard(parse(params)?).await,
            "git/commit" => self.git_commit(parse(params)?).await,
            "git/undo_commit" => self.git_undo_commit(parse(params)?).await,
            "git/push" => self.git_push(parse(params)?).await,
            "git/pull" => self.git_pull(parse(params)?).await,
            "git/fetch" => self.git_fetch(parse(params)?).await,
            "git/branches" => self.git_branches(parse(params)?).await,
            "git/checkout" => self.git_checkout(parse(params)?).await,
            "git/log" => self.git_log(parse(params)?).await,
            "git/commit_message" => self.git_commit_message(parse(params)?).await,
            "git/diff" => self.git_diff(parse(params)?).await,
            "worktree/create" => self.worktree_create(parse(params)?).await,
            "worktree/list" => self.worktree_list(parse(params)?).await,
            "worktree/remove" => self.worktree_remove(parse(params)?).await,
            "worktree/merge" => self.worktree_merge(parse(params)?).await,
            "file/read" => self.file_read(parse(params)?).await,
            "files/search" => self.files_search(parse(params)?).await,
            "fs/list" => self.fs_list(parse(params)?).await,
            "file/write" => self.file_write(parse(params)?).await,
            "symbols/find" => self.symbols_find(parse(params)?).await,
            "lsp/status" => self.lsp_status(parse(params)?).await,
            "lsp/open" => self.lsp_open(parse(params)?).await,
            "lsp/change" => self.lsp_change(parse(params)?).await,
            "lsp/close" => self.lsp_close(parse(params)?).await,
            "lsp/completion" => self.lsp_completion(parse(params)?).await,
            "lsp/hover" => self.lsp_hover(parse(params)?).await,
            "lsp/definition" => self.lsp_definition(parse(params)?).await,
            "tasks/list" => self.tasks_list(parse(params)?).await,
            "memory/list" => self.memory_list(parse(params)?).await,
            "doctor/run" => self.doctor_run(parse(params)?).await,
            "gateway/status" => self.gateway_status().await,
            "gateway/device/start" => self.gateway_device_start(parse(params)?).await,
            "gateway/device/poll" => self.gateway_device_poll(parse(params)?).await,
            "gateway/account" => self.gateway_account(parse(params)?).await,
            "gateway/usage" => self.gateway_usage(parse(params)?).await,
            "gateway/signout" => self.gateway_signout(parse(params)?).await,
            "gateway/billing" => self.gateway_billing(parse(params)?).await,
            "gateway/billing/quote" => self.gateway_billing_post("quote", parse(params)?).await,
            "gateway/billing/topup" => self.gateway_billing_post("topup", parse(params)?).await,
            "gateway/billing/subscribe" => self.gateway_billing_post("subscribe", parse(params)?).await,
            "gateway/billing/preference" => self.gateway_billing_preference(parse(params)?).await,
            "gateway/key" => self.gateway_key(parse(params)?).await,
            "gateway/key/update" => self.gateway_key_update(parse(params)?).await,
            "gateway/context_credit/claim" => self.gateway_claim_context_credit(parse(params)?).await,
            "gateway/device/identify" => self.gateway_device_identify(parse(params)?).await,
            "gateway/referral" => self.gateway_referral(parse(params)?).await,
            "gateway/referral/update" => self.gateway_referral_update(parse(params)?).await,
            "gateway/referral/attach" => self.gateway_referral_attach(parse(params)?).await,
            "mcp/status" => Ok(json!({"servers": self.mcp.status()})),
            "mcp/reconnect" => self.mcp_reconnect(parse(params)?).await,
            "mcp/catalog" => Ok(json!({"apps": ctx_tools::mcp::catalog()})),
            "mcp/permissions" => Ok(crate::computer::permissions()),
            "mcp/oauth/start" => self.mcp_oauth_start(parse(params)?).await,
            "mcp/oauth/poll" => self.mcp_oauth_poll(parse(params)?).await,
            "mcp/oauth/account" => self.mcp_oauth_account(parse(params)?),
            "mcp/oauth/signout" => self.mcp_oauth_signout(parse(params)?).await,
            "mcp/install" => self.mcp_install(parse(params)?).await,
            "mcp/uninstall" => self.mcp_uninstall(parse(params)?).await,
            "spend/summary" => self.spend_summary().await,
            "pricing/lookup" => self.pricing_lookup(parse(params)?).await,
            "shutdown" => Ok(json!({})),
            _ => Err(RpcError::new(METHOD_NOT_FOUND, format!("unknown method `{method}`"))),
        }
    }

    fn project(&self, id: &str) -> RpcResult<Arc<Project>> {
        self.projects.read().get(id).cloned().ok_or_else(|| RpcError::project_not_open(id))
    }

    // ----- configuration ----------------------------------------------------

    fn config_info(&self) -> ConfigInfo {
        let c = self.config.read();
        ConfigInfo {
            protocol_version: PROTOCOL_VERSION,
            ctx_version: env!("CARGO_PKG_VERSION"),
            data_dir: c.data_dir.display().to_string(),
            config_file: c.layout().config_file().display().to_string(),
            config_sources: c.sources.iter().map(|p| p.display().to_string()).collect(),
            mock: self.mock,
            model: ModelInfo {
                name: c.model.name.clone(),
                base_url: c.model.base_url.clone(),
                api_key_env: c.model.api_key_env.clone(),
                api_key_set: ctx_core::resolve_api_key(&c, &c.model).is_some(),
                credential: c.model.credential.clone(),
                context_window: c.model.context_window,
                max_output_tokens: c.model.max_output_tokens,
                reasoning_effort: c.model.reasoning_effort.clone(),
            },
            permissions: PermissionsInfo {
                profile: c.permissions.profile.as_str(),
                allowed_commands: c.permissions.allowed_commands.clone(),
                denied_commands: c.permissions.denied_commands.clone(),
                allow_network_commands: c.permissions.allow_network_commands,
            },
            pricing: c.pricing.as_ref().map(|p| PricingInfo { input_per_million: p.input_per_million, cached_input_per_million: p.cached_input_per_million, output_per_million: p.output_per_million, models: p.models.clone() }),
            spend: c.spend.clone(),
            mcp: c.mcp.clone(),
            host: self.host.clone(),
        }
    }

    /// `files/search`: the repository files best matching `query` for `@`
    /// completion (ignore rules applied; the walk is cached for ten seconds).
    /// `fs/list`: a directory of this machine (the desktop app's remote
    /// folder picker and file tree); no project needed.
    async fn fs_list(&self, params: FsListParams) -> RpcResult<Value> {
        let listing = blocking_rpc(move || list_directory(params.path.as_deref(), params.dirs_only)).await?;
        to_value(&listing)
    }

    async fn files_search(&self, params: FilesSearchParams) -> RpcResult<Value> {
        let project = self.project(&params.project_id)?;
        let limit = params.limit.unwrap_or(20).clamp(1, 100);
        let query = params.query;
        let paths = blocking(move || Ok(project.files.search(&query, limit))).await?;
        Ok(json!({"files": paths}))
    }

    /// `mcp/install`: writes the catalog app's server entry (with the
    /// user's input values) through the ordinary patch path, so the server
    /// connects like any other configuration change.
    async fn mcp_install(self: &Arc<Self>, params: McpInstallParams) -> RpcResult<Value> {
        let app = ctx_tools::mcp::catalog_app(&params.id).ok_or_else(|| RpcError::invalid_params(format!("no app `{}` in the catalog", params.id)))?;
        let missing: Vec<String> = app.missing_inputs(&params.inputs).iter().map(|i| i.label.clone()).collect();
        if !missing.is_empty() {
            return Err(RpcError::invalid_params(format!("{} needs: {}", app.name, missing.join(", "))));
        }
        let mut server = app.server(&params.inputs);
        server.exclude_tools = params.exclude_tools.iter().map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
        // Update…: the tool permissions set on the app's page stay.
        if let Some(existing) = self.config.read().mcp.servers.get(&app.id) {
            server.bypass_permissions = existing.bypass_permissions;
            server.permissions = existing.permissions.clone();
        }
        let mut values = serde_json::Map::new();
        values.insert(format!("mcp.servers.{}", app.id), serde_json::to_value(&server).map_err(CtxError::from)?);
        let uninstalled: Vec<String> = self.config.read().mcp.uninstalled.iter().filter(|u| **u != app.id).cloned().collect();
        values.insert("mcp.uninstalled".into(), if uninstalled.is_empty() { Value::Null } else { json!(uninstalled) });
        self.config_patch(ConfigPatchParams { values }).await
    }

    /// `mcp/uninstall`: removes the entry; a default app is remembered as
    /// removed so `Config::load` does not add it back.
    /// The app's sign-in, or `-32602` when it has none.
    fn oauth_of(id: &str) -> RpcResult<ctx_tools::mcp::CatalogOAuth> {
        let app = ctx_tools::mcp::catalog_app(id).ok_or_else(|| RpcError::invalid_params(format!("no app `{id}` in the catalog")))?;
        app.oauth.ok_or_else(|| RpcError::invalid_params(format!("{} has no sign-in; it takes a token", app.name)))
    }

    /// `mcp/oauth/start`: GitHub's device code (`user_code`, `verification_uri`,
    /// `device_code`, `interval`, `expires_in`). The client shows the code and
    /// opens the page, then polls `mcp/oauth/poll`.
    async fn mcp_oauth_start(&self, params: McpOAuthAppParams) -> RpcResult<Value> {
        let oauth = Self::oauth_of(&params.app)?;
        let code = ctx_tools::mcp::oauth::github_device_start(&oauth.scopes).await?;
        to_value(&code)
    }

    /// `mcp/oauth/poll`: `{status: "pending", interval} | {status: "approved",
    /// account} | {status: "denied"} | {status: "expired"}`. On approval the
    /// token is stored (never returned) and an installed app reconnects
    /// with it.
    async fn mcp_oauth_poll(self: &Arc<Self>, params: McpOAuthPollParams) -> RpcResult<Value> {
        let oauth = Self::oauth_of(&params.app)?;
        let outcome = ctx_tools::mcp::oauth::github_device_poll(&self.layout(), &oauth.credential, &params.device_code).await?;
        if matches!(outcome, ctx_tools::mcp::oauth::DevicePoll::Approved { .. }) && self.config.read().mcp.servers.contains_key(&params.app) {
            let server = self.clone();
            let app = params.app.clone();
            tokio::spawn(async move {
                let _ = server.mcp.reconnect(Some(&app)).await;
            });
        }
        to_value(&outcome)
    }

    /// `mcp/oauth/account`: `{account: "octocat" | null}`.
    fn mcp_oauth_account(&self, params: McpOAuthAppParams) -> RpcResult<Value> {
        let oauth = Self::oauth_of(&params.app)?;
        Ok(json!({"account": ctx_tools::mcp::oauth::signed_in_account(&self.layout(), &oauth.credential)}))
    }

    /// `mcp/oauth/signout`: forgets the token; an installed app then fails
    /// to connect with "not signed in" until the user signs in again.
    async fn mcp_oauth_signout(self: &Arc<Self>, params: McpOAuthAppParams) -> RpcResult<Value> {
        let oauth = Self::oauth_of(&params.app)?;
        let removed = ctx_tools::mcp::oauth::sign_out(&self.layout(), &oauth.credential)?;
        if self.config.read().mcp.servers.contains_key(&params.app) {
            let _ = self.mcp.reconnect(Some(&params.app)).await;
        }
        Ok(json!({"signed_out": removed}))
    }

    async fn mcp_uninstall(self: &Arc<Self>, params: McpUninstallParams) -> RpcResult<Value> {
        // Tokens the entry used are forgotten with it.
        let credentials = self.config.read().mcp.servers.get(&params.id).map(ctx_tools::mcp::oauth::referenced_credentials).unwrap_or_default();
        let result = self.mcp_uninstall_entry(&params).await?;
        for name in credentials {
            if let Err(e) = ctx_tools::mcp::oauth::sign_out(&self.layout(), &name) {
                tracing::warn!(credential = %name, error = %e, "could not forget the app's stored credential");
            }
        }
        Ok(result)
    }

    async fn mcp_uninstall_entry(self: &Arc<Self>, params: &McpUninstallParams) -> RpcResult<Value> {
        let mut values = serde_json::Map::new();
        values.insert(format!("mcp.servers.{}", params.id), Value::Null);
        if ctx_core::McpConfig::is_default_app(&params.id) {
            let mut uninstalled = self.config.read().mcp.uninstalled.clone();
            if !uninstalled.contains(&params.id) {
                uninstalled.push(params.id.clone());
            }
            values.insert("mcp.uninstalled".into(), json!(uninstalled));
        }
        self.config_patch(ConfigPatchParams { values }).await
    }

    /// `mcp/reconnect`: reconnects one or every server and answers with
    /// the state afterwards (the `mcp/status` notification follows every
    /// change meanwhile).
    async fn mcp_reconnect(&self, params: McpReconnectParams) -> RpcResult<Value> {
        self.mcp.reconnect(params.server.as_deref()).await?;
        Ok(json!({"servers": self.mcp.status()}))
    }

    async fn reload_config(self: &Arc<Self>) -> RpcResult<()> {
        let server = self.clone();
        let config = blocking(move || server.source.load()).await?;
        *self.config.write() = config;
        Ok(())
    }

    async fn config_patch(self: &Arc<Self>, params: ConfigPatchParams) -> RpcResult<Value> {
        // One patch at a time: two concurrent read-merge-writes would lose
        // one of them.
        let _serialized = self.patch_lock.lock().await;
        let server = self.clone();
        blocking_rpc(move || server.apply_config_patch(params)).await?;
        to_value(&self.config_info())
    }

    /// The whole read-merge-validate-write cycle of `config/patch`, run on a
    /// blocking thread.
    fn apply_config_patch(&self, params: ConfigPatchParams) -> RpcResult<()> {
        let path = self.config.read().layout().config_file();
        let previous = match std::fs::read_to_string(&path) {
            Ok(text) => Some(text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(RpcError::from(CtxError::from(e))),
        };
        let mut table: toml::Table = match &previous {
            Some(text) => text.parse().map_err(|e: toml::de::Error| CtxError::Config(format!("{}: {e}", path.display())))?,
            None => toml::Table::new(),
        };
        let schema = patch_schema().map_err(|e| RpcError::from(CtxError::Config(format!("cannot derive the configuration schema: {e}"))))?;
        let mut model_changed = false;
        let mut model_name_patched = false;
        let mut mcp_changed = false;
        for (key, value) in &params.values {
            let segments: Vec<&str> = key.split('.').collect();
            let allowed = segments.len() >= 2 && segments.iter().all(|s| !s.is_empty()) && PATCH_PREFIXES.iter().any(|prefix| key.starts_with(prefix));
            if !allowed {
                return Err(RpcError::invalid_params(format!("configuration key `{key}` cannot be patched (allowed prefixes: {})", PATCH_PREFIXES.join(" "))));
            }
            // The lenient loader would accept `model.nmae`; a typo must not
            // land in the user's file as a success. Removals are not checked,
            // so junk an older version wrote can still be taken out. Object
            // values are checked leaf by leaf, so `{"model.capabilities":
            // {"streming": false}}` is refused like `model.capabilities.streming`.
            if !value.is_null() {
                check_patch_value(&schema, &segments, value).map_err(RpcError::invalid_params)?;
            }
            model_changed |= segments[0] == "model";
            model_name_patched |= segments.as_slice() == ["model", "name"];
            mcp_changed |= segments[0] == "mcp";
            let converted = match value {
                Value::Null => None,
                other => Some(json_to_toml(other).map_err(|e| RpcError::invalid_params(format!("`{key}`: {e}")))?),
            };
            set_path(&mut table, &segments, converted);
        }
        // Validate the merged file before anything is written.
        let candidate: Config = toml::Value::Table(table.clone()).try_into().map_err(|e: toml::de::Error| RpcError::invalid_params(format!("invalid configuration: {e}")))?;
        candidate.validate().map_err(|e| RpcError::invalid_params(format!("invalid configuration: {e}")))?;
        let text = toml::to_string_pretty(&table).map_err(|e| RpcError::invalid_params(format!("cannot serialize configuration: {e}")))?;
        write_private_atomic(&path, &text).map_err(CtxError::from)?;
        // Reload through the normal layers; roll the file back if the
        // result does not load (e.g. an interaction with an explicit file).
        match self.source.load() {
            Ok(config) => *self.config.write() = config,
            Err(e) => {
                let restore = match &previous {
                    Some(text) => write_private_atomic(&path, text),
                    None => std::fs::remove_file(&path),
                };
                if let Err(restore_err) = restore {
                    tracing::warn!(error = %restore_err, "could not restore the configuration file after a failed patch");
                }
                return Err(RpcError::invalid_params(format!("invalid configuration: {e}")));
            }
        }
        if mcp_changed {
            // Servers reconnect in the background; runs that start
            // meanwhile wait for them.
            let mcp = self.config.read().mcp.clone();
            self.mcp.reconfigure(&mcp);
        }
        if model_changed {
            let model = self.config.read().model.clone();
            let projects: Vec<Arc<Project>> = self.projects.read().values().cloned().collect();
            for project in projects {
                if project.is_running() {
                    continue;
                }
                let mut config = project.config.read().clone();
                let current_name = config.model.name.clone();
                config.model = model.clone();
                if !model_name_patched {
                    // Another `model.*` key changed: keep the model this
                    // project chose with `models/switch`.
                    config.model.name = current_name;
                }
                if !self.mock {
                    match build_provider_for_project(&config, &project_name(&project.ctx.root)) {
                        Ok(provider) => {
                            match load_profile(&config) {
                                Ok(Some(profile)) => {
                                    provider.apply_profile(&profile);
                                    if !profile.is_agent_capable() {
                                        self.log("warn", format!("cached capability profile reports issues: {}", profile.blocking_issues().join("; ")));
                                    }
                                }
                                Ok(None) => {}
                                Err(e) => self.log("warn", format!("could not load the cached capability profile: {e}")),
                            }
                            *project.provider.write() = provider;
                        }
                        Err(e) => {
                            self.log("warn", format!("could not rebuild the provider for {}: {e}", project.ctx.root.display()));
                            continue;
                        }
                    }
                    if model_name_patched {
                        // Keep the remembered model in step, or the next
                        // `project/open` would restore the old one.
                        if let Err(e) = app::remember_model(&config, &project.ctx.root) {
                            self.log("warn", format!("could not remember the model for this workspace: {e}"));
                        }
                    }
                }
                *project.config.write() = config;
            }
        }
        // Prices and spending limits are user-level: every project follows
        // at once (runs read them from `RunOptions::live_config`).
        let (pricing, spend) = { let c = self.config.read(); (c.pricing.clone(), c.spend.clone()) };
        for project in self.projects.read().values() {
            let mut config = project.config.write();
            config.pricing = pricing.clone();
            config.spend = spend.clone();
        }
        Ok(())
    }

    // ----- projects -----------------------------------------------------------

    async fn project_open(self: &Arc<Self>, params: OpenParams) -> RpcResult<Value> {
        let path = PathBuf::from(&params.path);
        // The spec shows `/abs/dir`; a relative path would silently resolve
        // against whatever working directory the app's child inherited.
        if !path.is_absolute() {
            return Err(RpcError::invalid_params(format!("path must be absolute: {}", path.display())));
        }
        if !path.is_dir() {
            return Err(RpcError::from(CtxError::Config(format!("{} is not a directory", path.display()))));
        }
        let exact = params.exact;
        let root = blocking(move || Ok(if exact { ctx_core::platform::canonicalize(&path).unwrap_or(path) } else { RepositoryDiscovery::find_root(&path) })).await?;
        let id = RepositoryId::from_root(&root).to_string();
        // `init_git`: a folder outside any repository gets one, so runs can
        // take the undo snapshots `session/rewind` restores. Never for a
        // project that is already open, and never for the root, a home
        // directory or a top-level folder (`snapshot::git_init_allowed`).
        let mut git_initialized = false;
        if params.init_git && !self.projects.read().contains_key(&id) {
            let probe = root.clone();
            let inside_repository = blocking(move || Ok(RepositoryDiscovery::git_info(&probe).is_some())).await?;
            if !inside_repository {
                if snapshot::git_init_allowed(&root) {
                    match git(&root, &["init", "-q"]).await {
                        Ok(out) if out.status.success() => git_initialized = true,
                        Ok(out) => self.log("warn", format!("could not initialize a git repository in {}: {}", root.display(), git_error_line(&out.stderr))),
                        Err(e) => self.log("warn", format!("could not initialize a git repository in {}: {e}", root.display())),
                    }
                } else {
                    self.log("warn", format!("not initializing a git repository in {}: a home or top-level directory", root.display()));
                }
            }
        }
        // Concurrent opens of one root take turns: the first builds the
        // project, the others find it open. Without this both would open
        // the per-repository SQLite stores at once (`database is locked`).
        let open_lock = self.open_locks.lock().entry(id.clone()).or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))).clone();
        let result = {
            let _opening = open_lock.lock().await;
            self.project_open_locked(&id, root).await
        };
        drop(open_lock);
        // Forget the lock once nobody else is waiting on it.
        {
            let mut locks = self.open_locks.lock();
            if locks.get(&id).map(|l| Arc::strong_count(l) == 1).unwrap_or(false) {
                locks.remove(&id);
            }
        }
        let mut info = result?;
        info.git_initialized = git_initialized;
        to_value(&info)
    }

    /// The body of `project/open` for one repository id, run under that id's
    /// open lock.
    async fn project_open_locked(self: &Arc<Self>, id: &str, root: PathBuf) -> RpcResult<ProjectInfo> {
        let id = id.to_string();
        let already_open = self.projects.read().get(&id).cloned();
        if let Some(existing) = already_open {
            return self.project_info(&existing).await;
        }
        // A project closed while it was still indexing: let that pass finish
        // first, so the same store is not indexed by two tasks at once.
        let orphan = self.orphaned_indexing.lock().remove(&id);
        if let Some(handle) = orphan {
            let _ = handle.await;
        }
        // A project closed while a task was running: its cancelled run may
        // still be unwinding on the same session store. Wait (bounded) so a
        // new runtime never continues the task the old one is cancelling.
        let orphan_runs = self.orphaned_runs.lock().remove(&id).unwrap_or_default();
        if !orphan_runs.is_empty() && tokio::time::timeout(REOPEN_RUN_GRACE, futures::future::join_all(orphan_runs)).await.is_err() {
            tracing::warn!(project = %id, "a cancelled run of the closed project is still unwinding; reopening anyway");
        }
        let server = self.clone();
        let open_root = root.clone();
        let (config, ctx, provider) = blocking_rpc(move || {
            // The project's configuration is loaded through the normal
            // layers, including the repository's own `.context-agent.toml`,
            // exactly as `ctx run` does; the user-level configuration the
            // app shows and edits (`self.config`) never includes that layer.
            let mut config = server.source.load_for(Some(&open_root))?;
            if !server.mock && !server.source.explicit_model() {
                if let Some(model) = app::remembered_model(&config, &open_root) {
                    config.model.name = model;
                }
            }
            let ctx = RepositoryContext::open_exact(&open_root, config.clone())?;
            let provider = server.build_project_provider(&config, &open_root)?;
            Ok((config, ctx, provider))
        })
        .await?;
        let approvals = Arc::new(ApprovalState::default());
        let (index_busy, index_busy_rx) = watch::channel(false);
        let project = Arc::new(Project {
            id: id.clone(),
            files: ctx_tools::mentions::FileIndex::new(ctx.indexer.clone()),
            ctx,
            profile: RwLock::new(config.permissions.profile),
            config: RwLock::new(config),
            provider: RwLock::new(provider),
            remembering: Arc::new(RememberingApprovals::new(Arc::new(DenyAll))),
            approvals,
            browser: Arc::new(BrowserState::default()),
            questions: Arc::new(QuestionState::default()),
            running: Mutex::new(HashMap::new()),
            indexing: Mutex::new(None),
            index_busy,
            index_busy_rx,
            closed: Arc::new(AtomicBool::new(false)),
            lifetime: self.lifetime.child_token(),
            deleting: Mutex::new(HashSet::new()),
        });
        // The open lock guarantees nobody inserted this id meanwhile.
        self.projects.write().insert(id.clone(), project.clone());
        self.start_indexing(&project, false);
        self.project_info(&project).await
    }

    /// The provider a project starts with: cached capability profile
    /// applied when present, never probed here.
    fn build_project_provider(&self, config: &Config, root: &Path) -> RpcResult<Arc<dyn ModelProvider>> {
        if self.mock {
            return Ok(mock_provider());
        }
        let provider = build_provider_for_project(config, &project_name(root))?;
        if let Some(profile) = load_profile(config)? {
            provider.apply_profile(&profile);
            if !profile.is_agent_capable() {
                self.log("warn", format!("cached capability profile reports issues: {}", profile.blocking_issues().join("; ")));
            }
        }
        Ok(provider)
    }

    fn start_indexing(&self, project: &Arc<Project>, rebuilt: bool) {
        let out = self.out.clone();
        let project_id = project.id.clone();
        let ctx = project.ctx.clone();
        let closed = project.closed.clone();
        let busy = IndexBusy(project.index_busy.clone());
        project.index_busy.send_replace(true);
        let handle = tokio::task::spawn_blocking(move || {
            // Dropped when this closure ends (also on a panic): the flag
            // flips back after `index/ready` has been queued.
            let _busy = busy;
            let started = Instant::now();
            let progress_out = out.clone();
            let progress_id = project_id.clone();
            let progress_closed = closed.clone();
            let progress = move |p: IndexProgress| {
                // Progress is advisory: skipped for a closed project and for
                // a client that has fallen far behind.
                if progress_closed.load(Ordering::SeqCst) || progress_out.is_congested() {
                    return;
                }
                progress_out.notify("index/progress", json!({"project_id": progress_id, "done": p.done, "total": p.total}));
            };
            let result = if rebuilt { rebuild_index(&ctx, &progress) } else { ctx.ensure_indexed(Some(&progress)) };
            let payload = match result {
                Ok(r) => json!({"project_id": project_id, "generation": r.generation, "files_indexed": r.files_indexed, "files_removed": r.files_removed, "files_unchanged": r.files_unchanged, "duration_ms": r.duration_ms, "rebuilt": rebuilt, "error": Value::Null}),
                Err(e) => {
                    tracing::warn!(error = %e, "indexing failed");
                    // The error may quote a path or an embedding endpoint.
                    let error = ctx.redactor.redact(&e.to_string());
                    json!({"project_id": project_id, "generation": ctx.stores.index.generation().unwrap_or(0), "files_indexed": 0, "files_removed": 0, "files_unchanged": 0, "duration_ms": started.elapsed().as_millis() as u64, "rebuilt": rebuilt, "error": error})
                }
            };
            if !closed.load(Ordering::SeqCst) {
                out.notify("index/ready", payload);
            }
        });
        *project.indexing.lock() = Some(handle);
    }

    fn project_close(&self, params: ProjectParams) -> RpcResult<Value> {
        let project = self.projects.write().remove(&params.project_id).ok_or_else(|| RpcError::project_not_open(&params.project_id))?;
        project.closed.store(true, Ordering::SeqCst);
        // Ends pending probes and, as their parent, every run token.
        project.lifetime.cancel();
        // The cancel is cooperative and can take as long as the current model
        // call or tool: keep the handle so a reopen and shutdown wait for it.
        let orphan_runs: Vec<JoinHandle<()>> = {
            let mut running = project.running.lock();
            running
                .values_mut()
                .filter_map(|r| {
                    r.cancel.cancel();
                    r.handle.take()
                })
                .filter(|h| !h.is_finished())
                .collect()
        };
        if !orphan_runs.is_empty() {
            let mut orphans = self.orphaned_runs.lock();
            orphans.retain(|_, handles| {
                handles.retain(|h| !h.is_finished());
                !handles.is_empty()
            });
            orphans.entry(project.id.clone()).or_default().extend(orphan_runs);
        }
        project.approvals.clear_all();
        self.lsp.close_project(&project.id);
        // A running indexing pass cannot be interrupted; its notifications
        // are now suppressed and a reopen of the same root waits for it.
        let indexing = project.indexing.lock().take();
        if let Some(handle) = indexing.filter(|h| !h.is_finished()) {
            let mut orphans = self.orphaned_indexing.lock();
            orphans.retain(|_, h| !h.is_finished());
            orphans.insert(project.id.clone(), handle);
        }
        Ok(json!({}))
    }

    async fn project_info(&self, project: &Arc<Project>) -> RpcResult<ProjectInfo> {
        let p = project.clone();
        let worktrees_dir = self.config.read().layout().worktrees_dir();
        let (git, stats, profile, validation, parent) = blocking(move || {
            let git = RepositoryDiscovery::git_info(&p.ctx.root);
            // Only a linked worktree pays for the extra `git rev-parse`.
            let parent = git.as_ref().filter(|g| g.is_worktree).and_then(|_| worktree::parent_root(&p.ctx.root)).map(|parent| WorktreeSummary {
                parent_root: parent.display().to_string(),
                managed: worktree::is_managed(&worktrees_dir, &p.ctx.root),
            });
            let stats = p.ctx.stores.index.stats()?;
            // Before the first index completes the profile would take a
            // walk of its own, holding up `project/open`; it stays empty
            // until then, and `index/ready` has the app ask again.
            let profile = p.ctx.indexed_profile()?;
            let validation = p.ctx.indexed_validation_commands()?;
            Ok((git, stats, profile, validation, parent))
        })
        .await?;
        let root = project.ctx.root.clone();
        let running_tasks = project.running_tasks();
        Ok(ProjectInfo {
            project_id: project.id.clone(),
            root: root.display().to_string(),
            name: project_name(&root),
            git: git.map(|g| GitSummary { branch: g.branch, head: g.head, dirty_files: g.dirty_files.len(), worktree: parent }),
            git_initialized: false,
            index: IndexInfo {
                generation: stats.generation,
                files: stats.files,
                symbols: stats.symbols,
                chunks: stats.chunks,
                relationships: stats.relationships,
                embeddings: stats.embeddings,
                last_full_index_at: stats.last_full_index_at,
                languages: stats.languages.into_iter().map(|(language, files)| LanguageCount { language, files }).collect(),
            },
            indexing: project.indexing(),
            model: project.model(),
            permission_profile: project.profile.read().as_str(),
            sandbox: match project.ctx.policy.sandbox().kind {
                SandboxKind::None => "none",
                SandboxKind::Wrapper => "wrapper",
            },
            validation_commands: validation.into_iter().map(|c| ValidationCommandInfo { command: c.command, kind: format!("{:?}", c.kind).to_lowercase(), cwd: c.cwd, source: c.source }).collect(),
            languages: profile.iter().flat_map(|p| &p.languages).map(|(l, n)| LanguageCount { language: l.clone(), files: *n as u64 }).collect(),
            build_files: profile.iter().flat_map(|p| &p.build_files).map(|b| b.path.clone()).collect(),
            file_count: profile.as_ref().map_or(0, |p| p.file_count),
            running_task: running_tasks.first().cloned(),
            running_tasks,
        })
    }

    async fn index_rebuild(&self, params: ProjectParams) -> RpcResult<Value> {
        let project = self.project(&params.project_id)?;
        loop {
            {
                // Check and start under the lock `task/run` reserves its slot
                // with, so neither a run nor a second rebuild can slip in
                // between: `start_indexing` flips `index_busy` before the lock
                // is released, so two rebuilds dispatched together can never
                // both observe an idle index and run `IndexStore::clear`
                // under each other.
                let running = project.running.lock();
                if any_slot_taken(&running) {
                    return Err(RpcError::task_running());
                }
                if !project.indexing() {
                    self.start_indexing(&project, true);
                    return Ok(json!({}));
                }
            }
            // Let the indexing pass that is still running finish first, then
            // re-check: the pass may have been joined by a run meanwhile.
            project.wait_index_idle().await;
        }
    }

    // ----- sessions -----------------------------------------------------------

    async fn sessions_list(&self, params: SessionsListParams) -> RpcResult<Value> {
        let project = self.project(&params.project_id)?;
        let limit = params.limit;
        let sessions = blocking(move || {
            let store = &project.ctx.stores.sessions;
            store.list_sessions(project.ctx.id.as_str(), limit)?.iter().map(|s| session_summary(store, s)).collect::<Result<Vec<_>>>()
        })
        .await?;
        Ok(json!({"sessions": sessions}))
    }

    async fn session_create(&self, params: SessionCreateParams) -> RpcResult<Value> {
        let project = self.project(&params.project_id)?;
        let model = project.model();
        let title = params.title.filter(|t| !t.trim().is_empty());
        let summary = blocking(move || {
            let manager = SessionManager::new(project.ctx.clone());
            let session = manager.start("desktop", &model)?;
            let store = &project.ctx.stores.sessions;
            if let Some(title) = title {
                set_session_title(store, &session.id, Some(title.trim()))?;
            }
            let session = store.get_session(&session.id)?.ok_or_else(|| CtxError::NotFound(format!("session {}", session.id)))?;
            session_summary(store, &session)
        })
        .await?;
        to_value(&summary)
    }

    async fn session_rename(&self, params: SessionRenameParams) -> RpcResult<Value> {
        let project = self.project(&params.project_id)?;
        let summary = blocking(move || {
            let store = &project.ctx.stores.sessions;
            let session = store.get_session(&params.session_id)?.ok_or_else(|| CtxError::InvalidArgument(format!("unknown session {}", params.session_id)))?;
            let title = params.title.trim();
            set_session_title(store, &session.id, if title.is_empty() { None } else { Some(title) })?;
            let session = store.get_session(&session.id)?.ok_or_else(|| CtxError::NotFound(format!("session {}", session.id)))?;
            session_summary(store, &session)
        })
        .await
        .map_err(unknown_id_is_invalid_params)?;
        to_value(&summary)
    }

    async fn session_delete(&self, params: SessionParams) -> RpcResult<Value> {
        let project = self.project(&params.project_id)?;
        let session_id = params.session_id.clone();
        {
            let running = project.running.lock();
            if session_slot_taken(&running, &session_id) {
                return Err(RpcError::session_task_running());
            }
            // Until the delete has finished, `task/run` refuses this session
            // (checked under the same `running` lock).
            project.deleting.lock().insert(session_id.clone());
        }
        let owner = project.clone();
        let result = blocking(move || {
            let store = &project.ctx.stores.sessions;
            if store.get_session(&params.session_id)?.is_none() {
                return Err(CtxError::InvalidArgument(format!("unknown session {}", params.session_id)));
            }
            // Sub-agent sessions belong to this one and go with it.
            for child in store.child_sessions(&params.session_id)? {
                delete_session(store, &child.id)?;
            }
            delete_session(store, &params.session_id)
        })
        .await;
        owner.deleting.lock().remove(&session_id);
        result.map_err(unknown_id_is_invalid_params)?;
        owner.ctx.activity.forget(&session_id);
        Ok(json!({}))
    }

    /// Remove a user message and everything after it; optionally put the
    /// working tree back to the snapshot taken before it. Refused while a
    /// task of the session runs; `task/run` for the session is refused
    /// meanwhile (the same `deleting` guard as `session/delete`).
    async fn session_rewind(&self, params: RewindParams) -> RpcResult<Value> {
        let project = self.project(&params.project_id)?;
        let session_id = params.session_id.clone();
        {
            let running = project.running.lock();
            if session_slot_taken(&running, &session_id) {
                return Err(RpcError::session_task_running());
            }
            let mut deleting = project.deleting.lock();
            if deleting.contains(&session_id) {
                return Err(RpcError::invalid_params(format!("session {session_id} is being deleted")));
            }
            deleting.insert(session_id.clone());
        }
        let result = self.rewind_guarded(&project, params).await;
        project.deleting.lock().remove(&session_id);
        result
    }

    async fn rewind_guarded(&self, project: &Arc<Project>, params: RewindParams) -> RpcResult<Value> {
        let root = project.ctx.root.clone();
        let store = project.ctx.stores.sessions.clone();
        let (sid, mid) = (params.session_id.clone(), params.message_id);
        // Check the snapshot before anything is deleted: a rewind that
        // cannot restore the files is refused whole, not half done.
        let planned = blocking(move || snapshot::plan(&store, &sid, mid)).await.map_err(unknown_id_is_invalid_params)?;
        // Only the session's own files go back; a snapshot from before
        // runs recorded them restores the whole tree, which would undo
        // whatever another session is doing right now.
        let restore = match (params.restore_files, planned) {
            (false, _) => None,
            (true, Some((snapshot::Snapshot { tree: Some(tree), .. }, files))) => {
                if !snapshot::tree_exists(&root, &tree).await {
                    return Err(RpcError::from(CtxError::NotFound("the file snapshot for this message is no longer in the repository (pruned by git gc); rewind without restoring files".into())));
                }
                if files.is_none() && project.is_running() {
                    return Err(RpcError::from(CtxError::Conflict("this message predates per-session file restores, so restoring would reset the whole working tree while another session is running; wait for it or rewind without restoring files".into())));
                }
                Some((tree, files))
            }
            (true, _) => return Err(RpcError::from(CtxError::NotFound("no file snapshot was taken before this message; rewind without restoring files".into()))),
        };
        let store = project.ctx.stores.sessions.clone();
        let (sid, mid) = (params.session_id.clone(), params.message_id);
        let report = blocking(move || snapshot::rewind(&store, &sid, mid)).await.map_err(unknown_id_is_invalid_params)?;
        // The rewound conversation no longer holds those reads.
        project.ctx.activity.forget(&params.session_id);
        let files = match restore {
            Some((tree, Some(files))) => Some(snapshot::restore_files(&root, &tree, &files).await?),
            Some((tree, None)) => Some(snapshot::restore_tree(&root, &tree).await?),
            None => None,
        };
        let store = project.ctx.stores.sessions.clone();
        let sid = params.session_id.clone();
        let session = blocking(move || {
            let record = store.get_session(&sid)?.ok_or_else(|| CtxError::InvalidArgument(format!("unknown session {sid}")))?;
            session_summary(&store, &record)
        })
        .await?;
        to_value(&snapshot::RewindResult { removed_tasks: report.removed_tasks, removed_messages: report.removed_messages, files, session })
    }

    async fn session_history(&self, params: SessionParams) -> RpcResult<Value> {
        let project = self.project(&params.project_id)?;
        let history = blocking(move || {
            let ctx = &project.ctx;
            let store = &ctx.stores.sessions;
            let session = store.get_session(&params.session_id)?.ok_or_else(|| CtxError::InvalidArgument(format!("unknown session {}", params.session_id)))?;
            let summary = session_summary(store, &session)?;
            let snapshots = snapshot::snapshots_from(&session.metadata);
            let mut turns = Vec::new();
            for task in store.tasks_for_session(&session.id)? {
                let rows = store.events_for_task(&task.id, 0, usize::MAX / 2)?;
                // Call ids repeat inside a continued task (mock ids, providers
                // numbering calls per response), so rows are paired with the
                // tool results positionally, in id order.
                let mut full_lens: HashMap<String, VecDeque<u64>> = HashMap::new();
                for r in rows.iter().filter(|r| r.kind == "tool_output") {
                    if let (Some(call_id), Some(len)) = (r.payload.get("call_id").and_then(|c| c.as_str()), r.payload.get("full_len").and_then(|n| n.as_u64())) {
                        full_lens.entry(call_id.to_string()).or_default().push_back(len);
                    }
                }
                let events: Vec<AgentEvent> = rows.into_iter().filter_map(|r: EventRecord| serde_json::from_value::<AgentEvent>(r.payload).ok()).collect();
                let events = with_agent_tool_outputs(store, events)?;
                let outcome = events.iter().rev().find_map(|e| match e {
                    AgentEvent::TaskCompleted { outcome, .. } => Some(outcome.clone()),
                    _ => None,
                });
                let messages = store
                    .messages_for_task(&task.id, 0)?
                    .into_iter()
                    .filter_map(|r| {
                        let message = serde_json::from_value::<AgentMessage>(r.message).ok()?;
                        let created_at = r.created_at.to_rfc3339();
                        match message {
                            AgentMessage::User(u) if ctx_agent::runtime::is_hidden_note(&u.text) => None,
                            AgentMessage::User(u) => Some(HistoryMessage::User {
                                id: r.id,
                                runtime: ctx_agent::runtime::is_runtime_note(&u.text),
                                snapshot: snapshot::snapshot_for(&snapshots, r.id).map(|s| s.tree.is_some()).unwrap_or(false),
                                text: u.text,
                                images: u.images.into_iter().filter_map(|i| i.path).collect(),
                                files: u.files.into_iter().filter_map(|f| f.path.map(|path| HistoryFile { path, name: f.name })).collect(),
                                created_at,
                            }),
                            AgentMessage::Assistant(a) => Some(HistoryMessage::Assistant { id: r.id, text: a.text, tool_calls: a.tool_calls.into_iter().map(|c| ToolCallInfo { id: c.id, name: c.name, arguments: c.arguments }).collect(), created_at }),
                            AgentMessage::ToolResult(t) => {
                                let full_len = take_full_len(&mut full_lens, &t.call_id).filter(|n| *n > t.content.len() as u64);
                                Some(HistoryMessage::ToolResult { id: r.id, call_id: t.call_id, tool_name: t.tool_name, content: t.content, is_error: t.is_error, full_len, created_at })
                            }
                            _ => None,
                        }
                    })
                    .collect();
                let totals = store.usage_totals(Some(&task.id), None)?;
                let cost_usd = totals.cost();
                turns.push(Turn {
                    task: task_summary(&task, false),
                    messages,
                    events,
                    outcome,
                    usage: UsageInfo { prompt_tokens: totals.prompt_tokens, completion_tokens: totals.completion_tokens, cached_tokens: totals.cached_tokens, requests: totals.requests, cost_usd },
                });
            }
            Ok(SessionHistory { session: summary, turns })
        })
        .await
        .map_err(unknown_id_is_invalid_params)?;
        to_value(&history)
    }

    // ----- tasks --------------------------------------------------------------

    async fn task_run(self: &Arc<Self>, params: TaskRunParams) -> RpcResult<Value> {
        let project = self.project(&params.project_id)?;
        let lookup = project.clone();
        let session_id = params.session_id.clone();
        let session = blocking(move || lookup.ctx.stores.sessions.get_session(&session_id)).await?.ok_or_else(|| RpcError::invalid_params(format!("unknown session {}", params.session_id)))?;
        // A child of the project's token: `project/close` and shutdown end
        // the run without having to find it.
        let cancel = project.lifetime.child_token();
        let options = params.options.unwrap_or_default().into_options();
        let (paths, files) = (params.images, params.files);
        let user = blocking_rpc(move || Ok(UserMessage { text: params.prompt, images: read_images(&paths)?, files: read_files(&files)? })).await?;
        {
            // Reserve the session's run slot and spawn under the same lock
            // (`tokio::spawn` is synchronous): the handle is in the slot
            // before anyone else can look, so a run that ends quickly can
            // never leave its handle in a later run's reservation. Other
            // sessions of the project run alongside.
            let mut running = project.running.lock();
            if session_slot_taken(&running, &session.id) {
                return Err(RpcError::session_task_running());
            }
            if project.deleting.lock().contains(&session.id) {
                return Err(RpcError::invalid_params(format!("session {} is being deleted", session.id)));
            }
            // Open from the start: steering sent while the run waits for
            // the index or the probe reaches the model at its first step.
            let inbox = Inbox::new();
            inbox.open();
            let handle = tokio::spawn(execute_run(self.clone(), project.clone(), session.clone(), user, options, cancel.clone(), inbox.clone()));
            running.insert(session.id.clone(), RunningTask { session_id: session.id.clone(), task_id: None, started_at: ctx_core::now(), cancel, inbox, handle: Some(handle) });
        }
        Ok(json!({"accepted": true}))
    }

    /// Hands a message to the session's running turn; the model reads it
    /// before its next step (the `steered` event). Refused with
    /// `NO_TASK_RUNNING` when the session is idle or its run is ending:
    /// the client then sends it with `task/run` after `task/finished`.
    async fn task_steer(&self, params: TaskSteerParams) -> RpcResult<Value> {
        let project = self.project(&params.project_id)?;
        let (paths, files, prompt) = (params.images, params.files, params.prompt);
        let user = blocking_rpc(move || Ok(UserMessage { text: prompt, images: read_images(&paths)?, files: read_files(&files)? })).await?;
        let running = project.running.lock();
        let run = running.get(&params.session_id).filter(|r| slot_taken(r)).ok_or_else(RpcError::no_task_running)?;
        run.inbox.send(user).map_err(|_| RpcError::no_task_running())?;
        Ok(json!({}))
    }

    fn task_cancel(&self, params: TaskCancelParams) -> RpcResult<Value> {
        let project = self.project(&params.project_id)?;
        let cancelled: Vec<String> = {
            let guard = project.running.lock();
            let live = guard.values().filter(|r| r.handle.as_ref().is_some_and(|h| !h.is_finished()));
            live.filter(|r| params.session_id.as_ref().is_none_or(|s| *s == r.session_id))
                .map(|r| {
                    r.cancel.cancel();
                    r.session_id.clone()
                })
                .collect()
        };
        if cancelled.is_empty() {
            return Err(RpcError::no_task_running());
        }
        // A run parked on an approval awaits its reply channel, which never
        // observes the token. Dropping its pending senders answers it with
        // Deny (`SessionApprovals`); the runtime then sees the cancelled
        // token and ends the run with `task/finished`.
        for session_id in &cancelled {
            project.approvals.clear_session(session_id);
            project.browser.clear_session(session_id);
            project.questions.clear_session(session_id);
        }
        Ok(json!({}))
    }

    /// The app's answer to a `browser/request`: hands the page's reply to
    /// the `browser` tool call waiting for it.
    fn browser_respond(&self, params: BrowserRespondParams) -> RpcResult<Value> {
        let project = self.project(&params.project_id)?;
        let key = approval_key(&params.session_id, &params.request_id);
        let reply = BrowserReply { ok: params.ok, content: params.content, url: params.url, title: params.title, error: params.error };
        if project.browser.respond(&key, reply) {
            return Ok(json!({}));
        }
        Err(RpcError::invalid_params(format!("unknown browser request `{}`", params.request_id)))
    }

    /// The app's answer to a `question/request` of the `ask_user` tool.
    fn question_respond(&self, params: QuestionRespondParams) -> RpcResult<Value> {
        let project = self.project(&params.project_id)?;
        let key = approval_key(&params.session_id, &params.request_id);
        if project.questions.respond(&key, QuestionReply { answers: params.answers, message: params.message }) {
            return Ok(json!({}));
        }
        Err(RpcError::invalid_params(format!("unknown question request `{}`", params.request_id)))
    }

    fn approval_respond(&self, params: ApprovalParams) -> RpcResult<Value> {
        let project = self.project(&params.project_id)?;
        let decision = match params.decision.as_str() {
            "approve" => ApprovalDecision::Approve,
            "deny" => ApprovalDecision::Deny,
            "approve_for_session" => ApprovalDecision::ApproveForSession,
            other => return Err(RpcError::invalid_params(format!("unknown decision `{other}` (expected approve, deny or approve_for_session)"))),
        };
        let key = match &params.session_id {
            Some(session_id) => Some(approval_key(session_id, &params.request_id)),
            None => project.approvals.key_for(&params.request_id),
        };
        if key.is_some_and(|key| project.approvals.respond(&key, decision)) {
            return Ok(json!({}));
        }
        Err(RpcError::invalid_params(format!("unknown approval request `{}`", params.request_id)))
    }

    async fn permissions_set(&self, params: PermissionsParams) -> RpcResult<Value> {
        let project = self.project(&params.project_id)?;
        let profile = PermissionProfile::parse(&params.profile).ok_or_else(|| RpcError::invalid_params(format!("unknown permission profile `{}`", params.profile)))?;
        *project.profile.write() = profile;
        to_value(&self.project_info(&project).await?)
    }

    // ----- models -------------------------------------------------------------

    async fn models_list(&self, params: ModelsListParams) -> RpcResult<Value> {
        let config = match &params.project_id {
            Some(id) => self.project(id)?.config.read().clone(),
            None => self.config.read().clone(),
        };
        let current = config.model.name.clone();
        if self.mock {
            return to_value(&ModelsList { models: with_current_first(vec!["mock-model".to_string()], &current), current, source: "endpoint", error: None, stats: ModelStats::new() });
        }
        let key = config.model.base_url.trim_end_matches('/').to_string();
        if !params.refresh {
            let cached = self.models_cache.lock().get(&key).filter(|(at, _)| at.elapsed() < MODELS_CACHE_TTL).map(|(_, m)| m.clone());
            if let Some(models) = cached {
                let stats = model_stats::load(&config).await;
                return to_value(&ModelsList { models: with_current_first(models, &current), current, source: "cached", error: None, stats });
            }
        }
        // The stats are cached on disk for an hour; `refresh` only refetches the list.
        let (listed, stats) = tokio::join!(ctx_providers::list_models(&config), model_stats::load(&config));
        match listed {
            Ok(mut models) => {
                models.sort();
                models.dedup();
                self.models_cache.lock().insert(key, (Instant::now(), models.clone()));
                to_value(&ModelsList { models: with_current_first(models, &current), current, source: "endpoint", error: None, stats })
            }
            Err(e) => to_value(&ModelsList { models: vec![current.clone()], current, source: "fallback", error: Some(self.redactor.redact(&e.to_string())), stats }),
        }
    }

    async fn models_switch(&self, params: ModelsSwitchParams) -> RpcResult<Value> {
        let project = self.project(&params.project_id)?;
        let model = params.model.trim().to_string();
        if model.is_empty() {
            return Err(RpcError::invalid_params("model must not be empty"));
        }
        // Allowed while a task runs: every run clones the project's provider
        // when it starts (`execute_run`), so the running turn keeps its model
        // and the switch applies to the next `task/run`.
        let mut next = project.config.read().clone();
        next.model.name = model.clone();
        if self.mock {
            *project.config.write() = next;
            return to_value(&json!({"model": model, "capability": CapabilitySummary::mock()}));
        }
        let provider = build_provider_for_project(&next, &project_name(&project.ctx.root))?;
        let cached = {
            let config = next.clone();
            blocking(move || load_profile(&config)).await?
        };
        let profile = match cached {
            Some(profile) if profile.is_agent_capable() => {
                provider.apply_profile(&profile);
                profile
            }
            // No profile, or one that failed validation: probe again, so
            // choosing the model re-checks it instead of reusing the failure.
            // The project's token: `project/close` and shutdown abort the
            // probe instead of leaving the request pending for 120 s.
            _ => self.probe(&next, provider.clone(), &project.lifetime).await?,
        };
        *project.provider.write() = provider;
        *project.config.write() = next.clone();
        let (config, root) = (next.clone(), project.ctx.root.clone());
        if let Err(e) = blocking(move || app::remember_model(&config, &root)).await {
            self.log("warn", format!("could not remember the model for this workspace: {}", e.message));
        }
        to_value(&json!({"model": model, "capability": CapabilitySummary::from_profile(&profile)}))
    }

    /// Probe an endpoint (120 s timeout), save the profile and apply it,
    /// reporting progress the way `ctx run` prints it.
    async fn probe(&self, config: &Config, provider: Arc<dyn ModelProvider>, cancel: &CancellationToken) -> RpcResult<CapabilityProfile> {
        if self.mock {
            self.log("info", "mock provider: no capability probe is performed");
        } else {
            self.log("info", format!("probing provider capabilities for {} at {} …", config.model.name, config.model.base_url));
        }
        let probe = async {
            tokio::select! {
                result = tokio::time::timeout(PROBE_TIMEOUT, provider.probe(cancel)) => result,
                // The provider honours the token itself; this guarantees a
                // prompt return even if it only checks between requests.
                _ = cancel.cancelled() => Ok(Err(CtxError::Cancelled)),
            }
        };
        match probe.await {
            Ok(Ok(profile)) => {
                if !self.mock {
                    let path = save_profile(config, &profile)?;
                    self.log("info", format!("capability profile saved to {}", path.display()));
                }
                provider.apply_profile(&profile);
                if !profile.is_agent_capable() {
                    let issues = profile.blocking_issues().join("; ");
                    self.log("warn", format!("the endpoint does not satisfy the required capabilities: {issues}"));
                    return Err(RpcError::from(CtxError::Provider(ProviderError::Unsupported(format!("the endpoint does not satisfy the required capabilities: {issues}. Run `ctx doctor --probe` for details.")))));
                }
                Ok(profile)
            }
            Ok(Err(e)) => {
                self.log("warn", format!("capability probe failed: {e}"));
                Err(RpcError::from(e))
            }
            Err(_) => {
                self.log("warn", "capability probe timed out after 120 s");
                Err(RpcError::from(CtxError::Timeout(PROBE_TIMEOUT)))
            }
        }
    }

    /// Make sure a project's provider has a capability profile before a run
    /// (probing lazily when none is cached).
    async fn ensure_probed(&self, project: &Arc<Project>, cancel: &CancellationToken) -> RpcResult<()> {
        let config = project.config.read().clone();
        let cached = {
            let config = config.clone();
            blocking(move || load_profile(&config)).await?
        };
        if cached.is_some() {
            return Ok(());
        }
        let provider = project.provider.read().clone();
        self.probe(&config, provider, cancel).await?;
        Ok(())
    }

    // ----- repository inspection ----------------------------------------------

    async fn git_status(&self, params: ProjectParams) -> RpcResult<Value> {
        let project = self.project(&params.project_id)?;
        let root = project.ctx.root.clone();
        let info_root = root.clone();
        let info = blocking(move || Ok(RepositoryDiscovery::git_info(&info_root))).await?;
        let Some(info) = info else {
            return to_value(&GitStatus { branch: None, head: None, head_subject: None, upstream: None, ahead: 0, behind: 0, remotes: Vec::new(), files: Vec::new() });
        };
        let output = git(&root, &["status", "--porcelain=v1", "-z", "--untracked-files=all"]).await?;
        if !output.status.success() {
            return Err(RpcError::from(CtxError::other(format!("git status failed: {}", git_error_line(&output.stderr)))));
        }
        let mut files = parse_porcelain_z(&output.stdout);
        // Line counts against HEAD; on an unborn branch the index diff and
        // the working-tree diff are summed instead. `--no-renames` keeps
        // the paths in step with the porcelain listing.
        let mut numstat = HashMap::new();
        match git(&root, &["diff", "--numstat", "-z", "--no-renames", "HEAD", "--"]).await {
            Ok(out) if out.status.success() => numstat = parse_numstat_z(&out.stdout),
            _ => {
                for args in [&["diff", "--numstat", "-z", "--no-renames", "--cached", "--"][..], &["diff", "--numstat", "-z", "--no-renames", "--"][..]] {
                    if let Ok(out) = git(&root, args).await {
                        if out.status.success() {
                            for (path, counts) in parse_numstat_z(&out.stdout) {
                                let slot = numstat.entry(path).or_insert(Some((0, 0)));
                                *slot = match (*slot, counts) {
                                    (Some((a, r)), Some((b, s))) => Some((a + b, r + s)),
                                    _ => None,
                                };
                            }
                        }
                    }
                }
            }
        }
        let untracked: Vec<PathBuf> = files.iter().filter(|f| f.status == "?" && !numstat.contains_key(&f.path)).map(|f| root.join(&f.path)).collect();
        let counted: Vec<Option<u64>> = blocking(move || Ok(untracked.iter().map(|p| count_text_lines(p)).collect())).await?;
        let mut counted = counted.into_iter();
        for file in &mut files {
            match numstat.get(&file.path) {
                Some(Some((added, removed))) => {
                    file.insertions = Some(*added);
                    file.deletions = Some(*removed);
                }
                Some(None) => {}
                None if file.status == "?" => {
                    if let Some(Some(lines)) = counted.next() {
                        file.insertions = Some(lines);
                        file.deletions = Some(0);
                    }
                }
                None => {}
            }
        }
        // The branch name also on an unborn branch, plus what the sync and
        // push controls need.
        let branch = match info.branch {
            Some(b) => Some(b),
            None => gitops::current_branch(&root).await,
        };
        let (upstream, ahead, behind) = gitops::branch_sync(&root).await;
        let remotes = gitops::remotes(&root).await;
        let head_subject = gitops::head_subject(&root).await;
        to_value(&GitStatus { branch, head: info.head, head_subject, upstream, ahead, behind, remotes, files })
    }

    async fn git_diff(&self, params: GitDiffParams) -> RpcResult<Value> {
        let project = self.project(&params.project_id)?;
        // A plain directory has no diff (like `git/status`, which reports no
        // branch and no files) rather than git's usage text as an error.
        let info_root = project.ctx.root.clone();
        if blocking(move || Ok(RepositoryDiscovery::git_info(&info_root))).await?.is_none() {
            return Ok(json!({"diff": "", "truncated": false}));
        }
        // Pin the format so `diff.external`, colour, `diff.noprefix`,
        // `diff.mnemonicPrefix` or `core.quotepath` in the user's git config
        // cannot change what the app's unified-diff parser receives.
        let mut args: Vec<&str> = vec!["-c", "color.ui=never", "-c", "core.quotepath=false", "-c", "diff.renames=true", "--no-pager", "diff", "--no-ext-diff", "--no-color", "--src-prefix=a/", "--dst-prefix=b/"];
        if params.staged {
            args.push("--cached");
        }
        let path = params.path.as_deref().map(str::trim).filter(|p| !p.is_empty());
        if let Some(p) = path {
            args.push("--");
            args.push(p);
        }
        let output = git(&project.ctx.root, &args).await?;
        if !output.status.success() {
            return Err(RpcError::from(CtxError::other(format!("git diff failed: {}", git_error_line(&output.stderr)))));
        }
        let truncated = output.stdout.len() > DIFF_CAP_BYTES;
        let bytes = if truncated { &output.stdout[..DIFF_CAP_BYTES] } else { &output.stdout[..] };
        let diff = String::from_utf8_lossy(bytes).to_string();
        Ok(json!({"diff": diff, "truncated": truncated}))
    }

    /// The editor's read. Credential files (`.env`, keys) open like any
    /// other: the user asked for them, and the agent's own tools still
    /// refuse them.
    async fn file_read(&self, params: FileReadParams) -> RpcResult<Value> {
        let project = self.project(&params.project_id)?;
        let max_bytes = params.max_bytes.unwrap_or(DEFAULT_FILE_MAX_BYTES).clamp(1, FILE_MAX_BYTES_CAP);
        let content = blocking(move || {
            let paths = project.ctx.policy.paths();
            let absolute = paths.resolve(&params.path)?;
            let relative = paths.relative(&absolute);
            let meta = std::fs::metadata(&absolute).map_err(|e| if e.kind() == std::io::ErrorKind::NotFound { CtxError::NotFound(format!("file {relative}")) } else { CtxError::from(e) })?;
            if meta.is_dir() {
                return Err(CtxError::InvalidArgument(format!("{relative} is a directory")));
            }
            if ctx_indexing::walker::is_binary(&absolute) {
                return Err(CtxError::InvalidArgument(format!("{relative} is a binary file")));
            }
            // Read at most `max_bytes + 1`: the extra byte tells truncation
            // apart without loading a multi-gigabyte file into memory.
            use std::io::Read;
            let mut bytes = Vec::with_capacity((meta.len() as usize).min(max_bytes) + 1);
            std::fs::File::open(&absolute)?.take(max_bytes as u64 + 1).read_to_end(&mut bytes)?;
            let size = meta.len().max(bytes.len() as u64);
            let truncated = bytes.len() > max_bytes;
            let mut cut = bytes.len().min(max_bytes);
            // Never split a UTF-8 sequence.
            while cut > 0 && cut < bytes.len() && (bytes[cut] & 0b1100_0000) == 0b1000_0000 {
                cut -= 1;
            }
            let hash = ctx_core::content_hash(&bytes[..cut]);
            let content = String::from_utf8_lossy(&bytes[..cut]).to_string();
            let language = project.ctx.registry.language_of(&relative).map(|l| l.as_str().to_string()).filter(|l| l != "other");
            Ok(FileContent { path: relative, content, truncated, language, size, hash })
        })
        .await?;
        to_value(&content)
    }

    /// Replaces (or creates) a file inside the workspace roots, atomically.
    /// `expected_hash` guards against clobbering an edit made since the
    /// caller's `file/read`: when it is set and differs from the file's
    /// current hash, nothing is written (`-32000` kind `conflict`). The
    /// index is updated for the path so `symbols/find` sees the new content.
    async fn file_write(&self, params: FileWriteParams) -> RpcResult<Value> {
        let project = self.project(&params.project_id)?;
        let result = blocking(move || {
            let paths = project.ctx.policy.paths();
            let absolute = paths.resolve(&params.path)?;
            let relative = paths.relative(&absolute);
            let existing = match std::fs::metadata(&absolute) {
                Ok(meta) if meta.is_dir() => return Err(CtxError::InvalidArgument(format!("{relative} is a directory"))),
                Ok(meta) => Some(meta),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => return Err(e.into()),
            };
            if let Some(expected) = params.expected_hash.as_deref() {
                let current = match existing {
                    Some(_) => Some(ctx_core::content_hash(&std::fs::read(&absolute)?)),
                    None => None,
                };
                if current.as_deref() != Some(expected) {
                    return Err(CtxError::Conflict(format!("{relative} changed on disk since it was read")));
                }
            }
            let created = existing.is_none();
            ctx_tools::files::atomic_write(&absolute, &params.content)?;
            if let Err(e) = project.ctx.indexer.update_paths(&[absolute]) {
                tracing::warn!(error = %e, path = %relative, "incremental index update failed after file/write");
            }
            let hash = ctx_core::content_hash(params.content.as_bytes());
            let language = project.ctx.registry.language_of(&relative).map(|l| l.as_str().to_string()).filter(|l| l != "other");
            Ok(FileWriteResult { path: relative, hash, size: params.content.len() as u64, language, created })
        })
        .await?;
        to_value(&result)
    }

    /// Symbol definitions from the index by name (the editor's completion
    /// and go-to-definition). `prefix` matches are ordered shortest name first.
    async fn symbols_find(&self, params: SymbolsFindParams) -> RpcResult<Value> {
        let project = self.project(&params.project_id)?;
        let query = params.query.trim().to_string();
        if query.is_empty() {
            return Ok(json!({"symbols": []}));
        }
        let limit = params.limit.clamp(1, 500);
        let mode = match params.mode {
            SymbolsFindMode::Prefix => ctx_storage::SymbolMatchMode::Prefix,
            SymbolsFindMode::Exact => ctx_storage::SymbolMatchMode::Exact,
            SymbolsFindMode::Contains => ctx_storage::SymbolMatchMode::Contains,
        };
        let symbols = blocking(move || {
            let found = project.ctx.stores.index.find_symbols(&query, mode, limit)?;
            Ok(found
                .into_iter()
                .map(|s| SymbolInfo { name: s.name, kind: s.kind, path: s.path, parent: s.parent, line: s.start_line, end_line: s.end_line, signature: s.signature })
                .collect::<Vec<_>>())
        })
        .await?;
        Ok(json!({"symbols": symbols}))
    }

    async fn tasks_list(&self, params: TasksListParams) -> RpcResult<Value> {
        let project = self.project(&params.project_id)?;
        let limit = params.limit;
        let tasks = blocking(move || Ok(project.ctx.stores.sessions.list_tasks(project.ctx.id.as_str(), limit)?.iter().map(|t| task_summary(t, true)).collect::<Vec<_>>())).await?;
        Ok(json!({"tasks": tasks}))
    }

    async fn memory_list(&self, params: MemoryListParams) -> RpcResult<Value> {
        let project = self.project(&params.project_id)?;
        let category = match params.category.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
            Some(c) => Some(ctx_memory::MemoryCategory::parse(c).ok_or_else(|| RpcError::invalid_params(format!("unknown memory category `{c}`")))?),
            None => None,
        };
        let limit = params.limit;
        let memories = blocking(move || {
            Ok(project
                .ctx
                .memory
                .list(category, None, false, limit)?
                .into_iter()
                .map(|m| MemoryInfo {
                    id: m.id.to_string(),
                    category: m.category.as_str().to_string(),
                    verification: m.verification.as_str().to_string(),
                    content: project.ctx.redactor.redact(&m.content),
                    evidence_paths: m.evidence_paths().map(str::to_string).collect(),
                    created_at: m.created_at.to_rfc3339(),
                    invalidated: m.invalidated,
                })
                .collect::<Vec<_>>())
        })
        .await?;
        Ok(json!({"memories": memories}))
    }

    // ----- doctor -------------------------------------------------------------

    async fn doctor_run(&self, params: DoctorParams) -> RpcResult<Value> {
        let project = match &params.project_id {
            Some(id) => Some(self.project(id)?),
            None => None,
        };
        if let Some(p) = &project {
            if params.probe && p.is_running() {
                return Err(RpcError::task_running());
            }
        }
        let config = match &project {
            Some(p) => p.config.read().clone(),
            None => self.config.read().clone(),
        };
        let cached = if self.mock {
            None
        } else {
            let config = config.clone();
            blocking(move || load_profile(&config)).await?
        };
        let profile = if params.probe {
            let provider: Arc<dyn ModelProvider> = match (&project, self.mock) {
                (_, true) => mock_provider(),
                (Some(p), false) => p.provider.read().clone(),
                (None, false) => build_provider(&config)?,
            };
            let cancel = project.as_ref().map(|p| p.lifetime.clone()).unwrap_or_else(|| self.lifetime.clone());
            Some(self.probe(&config, provider, &cancel).await?)
        } else {
            cached
        };
        let mock = self.mock;
        let mcp = self.mcp.status();
        let report = blocking(move || build_doctor_report(&config, project.as_deref(), profile.as_ref(), mock, &mcp)).await?;
        to_value(&report)
    }
}
