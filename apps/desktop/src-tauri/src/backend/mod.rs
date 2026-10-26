//! The `ctx serve` child: the port of `BackendProcess.swift` (spawn with
//! piped stdio, a strict-LF stdout line reader that scans only new bytes, a
//! 500-line stderr ring, an awaitable exit signal) plus the lifecycle half of
//! `BackendClient.swift` (`start`, `stop`, `handleExit`). The typed protocol
//! API, `initialize` and the protocol-version check live in the frontend
//! (`src/backend/client.ts`), which drives this module through the
//! `backend_*` and `rpc_request` commands registered in `lib.rs`.
//!
//! Tasks per child: one stdout reader (lines → [`rpc::RpcConnection::receive`]),
//! one stderr reader (lines → ring + `backend:stderr`), one writer (inside
//! `RpcConnection`) and one exit watcher that owns the `Child`, answers
//! terminate / kill requests, drains stdout first, fails every pending request
//! and emits `backend:exited` unless the exit was requested by `stop`.

pub mod environment;
pub mod locator;
pub mod rpc;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::{ExitStatus, Stdio};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::io::AsyncReadExt;
use tokio::process::{Child, ChildStderr, ChildStdout, Command};
use tokio::sync::{mpsc, watch};

use crate::selftest::LaunchOptions;
use rpc::{RpcConnection, RpcError};

/// `BackendProcess.stderrRingCapacity`.
pub const STDERR_RING_CAPACITY: usize = 500;
/// Emitted once per unrequested exit: `{ code, tail }`.
pub const EXITED_EVENT: &str = "backend:exited";
/// Emitted for every batch of stderr lines: `{ lines }`.
pub const STDERR_EVENT: &str = "backend:stderr";
/// `CTX_MOCK_PACE_MS` for mock children: 20 ms per streamed delta so
/// streaming text and running tool rows can be seen.
pub const MOCK_PACE_MS: &str = "20";
/// How many stderr lines `backend:exited` and start failures carry.
pub const STDERR_TAIL_LINES: usize = 40;

const STDOUT_DRAIN_TIMEOUT: Duration = Duration::from_secs(3);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
const TERMINATE_TIMEOUT: Duration = Duration::from_secs(1);
const REAP_TIMEOUT: Duration = Duration::from_secs(2);
const READ_CHUNK: usize = 64 * 1024;

/// `BackendLog.isEnabled`: debug logging for the backend layer only, silent
/// unless `CTX_DESKTOP_DEBUG` is set in the app's environment.
pub fn debug_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("CTX_DESKTOP_DEBUG").is_some())
}

/// `BackendLog.debug`: the message is only built when logging is on.
pub fn debug(message: impl FnOnce() -> String) {
    if debug_enabled() {
        eprintln!("[backend] {}", message());
    }
}

/// Locks a std mutex, recovering from poisoning (the guarded data is plain state).
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

// MARK: - LineSplitter

/// Splits a byte stream strictly on 0x0A (`LineSplitter` in Swift). U+2028,
/// NEL and friends never end a line. Only the new bytes are searched:
/// rescanning the buffered part on every read made a multi-megabyte frame (a
/// long session's history) cost seconds and held up every frame behind it.
#[derive(Default)]
pub struct LineSplitter {
    /// The unfinished line: never holds an LF between calls.
    buffer: Vec<u8>,
}

impl LineSplitter {
    /// Appends bytes and returns every complete line (without the LF).
    pub fn append(&mut self, data: &[u8]) -> Vec<Vec<u8>> {
        let mut lines = Vec::new();
        let mut offset = 0;
        while offset < data.len() {
            match memchr::memchr(b'\n', &data[offset..]) {
                Some(relative) => {
                    let end = offset + relative;
                    if self.buffer.is_empty() {
                        lines.push(data[offset..end].to_vec());
                    } else {
                        self.buffer.extend_from_slice(&data[offset..end]);
                        lines.push(std::mem::take(&mut self.buffer));
                    }
                    offset = end + 1;
                }
                None => {
                    self.buffer.extend_from_slice(&data[offset..]);
                    break;
                }
            }
        }
        lines
    }

    /// Whatever is left without a trailing LF (EOF).
    pub fn drain(&mut self) -> Option<Vec<u8>> {
        if self.buffer.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut self.buffer))
        }
    }
}

// MARK: - StderrRing

/// The last [`STDERR_RING_CAPACITY`] stderr lines of one child.
#[derive(Default)]
pub struct StderrRing {
    lines: VecDeque<String>,
}

impl StderrRing {
    /// `recordStderr`: appends, dropping the oldest lines past the capacity.
    pub fn push(&mut self, lines: impl IntoIterator<Item = String>) {
        self.lines.extend(lines);
        while self.lines.len() > STDERR_RING_CAPACITY {
            self.lines.pop_front();
        }
    }

    /// `stderrTail(lines:)`: the last `count` lines, joined with LF.
    pub fn tail(&self, count: usize) -> String {
        let skip = self.lines.len().saturating_sub(count);
        self.lines.iter().skip(skip).cloned().collect::<Vec<_>>().join("\n")
    }

    /// `stderrLines`: everything buffered.
    pub fn lines(&self) -> Vec<String> {
        self.lines.iter().cloned().collect()
    }
}

// MARK: - Wire shapes

/// `BackendClient.State`, as `backend_state` returns it:
/// `{ state: "stopped"|"starting"|"running"|"failed", pid?, detail? }`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum BackendStatus {
    #[default]
    Stopped,
    Starting,
    Running {
        pid: u32,
    },
    Failed {
        detail: String,
    },
}

/// Why `backend_start` failed, in the shape `BackendFailure` (AppState.swift)
/// wants; rejected as a JSON string tagged by `kind`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StartFailure {
    /// No executable `ctx` on the search list (`.notFound(searched:)`).
    NotFound { searched: Vec<String> },
    /// The child was gone before `backend_start` returned (`.exited(code:tail:)`).
    Exited { code: i32, tail: String },
    /// The child could not be launched (`.transport`).
    Transport { message: String },
}

impl StartFailure {
    /// The JSON the frontend's `BackendFailure` is rebuilt from.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| {
            "{\"kind\":\"transport\",\"message\":\"backend start failed\"}".to_owned()
        })
    }
}

impl From<StartFailure> for String {
    fn from(failure: StartFailure) -> Self {
        failure.to_json()
    }
}

/// What `backend_start` resolves with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StartResult {
    pub pid: u32,
    /// The binary that was launched.
    pub binary: String,
    /// Every path the locator tried, in order, up to and including `binary`.
    pub searched: Vec<String>,
}

/// `backend_start`'s arguments (`BackendClient.start` + `AppState.startBackend`).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct StartOptions {
    /// `preferences.backendPathOverride` (or `--binary`); tried first.
    pub binary_override: Option<String>,
    /// `--data-dir` for the child (`CTX_DATA_DIR` in the macOS app).
    pub data_dir: Option<String>,
    /// `ctx serve --mock` with `CTX_MOCK_PACE_MS=20`.
    pub mock: bool,
    /// The variable the API key is exported under (`model.api_key_env`);
    /// filled from the secret store when the process does not export it.
    pub api_key_env: Option<String>,
}

#[derive(Clone, Serialize)]
struct ExitedPayload {
    code: i32,
    tail: String,
}

#[derive(Clone, Serialize)]
struct StderrPayload {
    lines: Vec<String>,
}

// MARK: - ChildHandle

/// What the exit watcher accepts from `stop`.
enum Signal {
    /// SIGTERM on Unix; a plain kill on Windows, which has no equivalent.
    Terminate,
    /// SIGKILL / `TerminateProcess`.
    Kill,
}

/// The managed state's view of a running child; the exit watcher task owns
/// the `Child` itself. `exit` is `ExitSignal` in Swift: fires once, any
/// number of waiters.
pub struct ChildHandle {
    pid: u32,
    control: mpsc::UnboundedSender<Signal>,
    exit: watch::Receiver<Option<i32>>,
}

impl ChildHandle {
    /// `processIdentifier`.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// `exitCode`: set once the process has terminated (the signal number for
    /// uncaught signals, like `Process.terminationStatus`).
    pub fn exit_code(&self) -> Option<i32> {
        *self.exit.borrow()
    }

    /// `waitForExit(timeout:)`: the exit code, or `None` after `timeout`.
    pub async fn wait_for_exit(&self, timeout: Duration) -> Option<i32> {
        if let Some(code) = self.exit_code() {
            return Some(code);
        }
        let mut receiver = self.exit.clone();
        let code = match tokio::time::timeout(timeout, receiver.wait_for(|code| code.is_some())).await {
            Ok(Ok(code)) => *code,
            _ => None,
        };
        code
    }

    /// `terminate()`: SIGTERM (a kill on Windows).
    pub fn terminate(&self) {
        let _ = self.control.send(Signal::Terminate);
    }

    /// `forceKill()`: SIGKILL.
    pub fn kill(&self) {
        let _ = self.control.send(Signal::Kill);
    }
}

// MARK: - Backend (managed state)

#[derive(Default)]
struct Inner {
    status: BackendStatus,
    child: Option<Arc<ChildHandle>>,
    connection: Option<Arc<RpcConnection>>,
    /// Last 500 stderr lines of the current (or last) child.
    stderr: Arc<Mutex<StderrRing>>,
    /// Bumped by every `start`; an exit watcher whose generation is stale
    /// belongs to a child `stop` or a restart already tore down.
    generation: u64,
    /// True from `stop()` until the next `start()`: suppresses `backend:exited`.
    stop_requested: bool,
    binary: Option<PathBuf>,
    mock: bool,
}

/// The one `ctx serve` child and its connection, held by Tauri as managed
/// state (`BackendClient` in Swift). Every public method takes the inner
/// lock briefly; nothing awaits the child while holding it except `stop`,
/// which is what callers expect to wait for.
#[derive(Default)]
pub struct Backend {
    inner: tokio::sync::Mutex<Inner>,
}

impl Backend {
    /// Locate → environment → spawn (`BackendClient.start` up to the point
    /// where the macOS app sends `initialize`; the frontend does that through
    /// `rpc_request`). A running child is stopped first, so this doubles as
    /// the restart path.
    pub async fn start(&self, app: &AppHandle, options: StartOptions) -> Result<StartResult, StartFailure> {
        let mut inner = self.inner.lock().await;
        if inner.child.is_some() {
            Self::stop_locked(&mut inner).await;
        }
        inner.stop_requested = false;
        inner.generation += 1;
        let generation = inner.generation;
        inner.status = BackendStatus::Starting;
        inner.stderr = Arc::new(Mutex::new(StderrRing::default()));
        inner.mock = options.mock;
        inner.binary = None;

        // Locate: the explicit override, else `--binary` from the launch options, else the search list.
        let override_path = options
            .binary_override
            .as_deref()
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(str::to_owned)
            .or_else(|| LaunchOptions::current().binary.clone());
        let located = locator::locate(override_path.as_deref());
        let searched: Vec<String> = located.searched.iter().map(|path| path.display().to_string()).collect();
        let Some(binary) = located.found else {
            inner.status = BackendStatus::Failed {
                detail: "ctx not found".to_owned(),
            };
            return Err(StartFailure::NotFound { searched });
        };
        inner.binary = Some(binary.clone());

        // Environment: no API key lookup for the mock, like `AppState.startBackend`.
        let api_key_env = if options.mock { None } else { options.api_key_env.as_deref() };
        let mut environment = environment::build(api_key_env).await;
        if options.mock && environment.get("CTX_MOCK_PACE_MS").is_none_or(String::is_empty) {
            environment.insert("CTX_MOCK_PACE_MS".to_owned(), MOCK_PACE_MS.to_owned());
        }

        let mut arguments = vec!["serve".to_owned()];
        if options.mock {
            arguments.push("--mock".to_owned());
        }
        if let Some(dir) = options.data_dir.as_deref().map(str::trim).filter(|dir| !dir.is_empty()) {
            arguments.push("--data-dir".to_owned());
            arguments.push(locator::expand_tilde(dir).display().to_string());
        }

        let mut command = Command::new(&binary);
        command
            .args(&arguments)
            .env_clear()
            .envs(&environment)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        configure_platform(&mut command);

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                let message = format!("Could not launch {}: {error}", binary.display());
                inner.status = BackendStatus::Failed {
                    detail: message.clone(),
                };
                return Err(StartFailure::Transport { message });
            }
        };
        let pid = child.id().unwrap_or(0);
        let (Some(stdin), Some(stdout), Some(stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take()) else {
            let _ = child.start_kill();
            let message = format!("Could not attach to {}: its pipes are missing", binary.display());
            inner.status = BackendStatus::Failed {
                detail: message.clone(),
            };
            return Err(StartFailure::Transport { message });
        };

        let connection = RpcConnection::new(app.clone(), stdin);
        let (control, control_receiver) = mpsc::unbounded_channel();
        let (exit_sender, exit_receiver) = watch::channel(None);
        let handle = Arc::new(ChildHandle {
            pid,
            control,
            exit: exit_receiver,
        });
        let stderr_ring = Arc::clone(&inner.stderr);
        let stdout_task = tauri::async_runtime::spawn(read_stdout(stdout, Arc::clone(&connection)));
        tauri::async_runtime::spawn(read_stderr(app.clone(), stderr, Arc::clone(&stderr_ring)));
        tauri::async_runtime::spawn(watch_exit(ExitWatch {
            app: app.clone(),
            child,
            control: control_receiver,
            exit: exit_sender,
            stdout_task,
            connection: Arc::clone(&connection),
            stderr: stderr_ring,
            generation,
        }));

        inner.child = Some(handle);
        inner.connection = Some(connection);
        inner.status = BackendStatus::Running { pid };
        debug(|| format!("launched {} {} pid {pid}", binary.display(), arguments.join(" ")));
        Ok(StartResult {
            pid,
            binary: binary.display().to_string(),
            searched,
        })
    }

    /// `shutdown` → wait ≤ 2 s → SIGTERM → 1 s → SIGKILL (on Windows:
    /// `shutdown` → 2 s → kill). Never fails; the exit that follows is not
    /// reported as `backend:exited`.
    pub async fn stop(&self) {
        let mut inner = self.inner.lock().await;
        Self::stop_locked(&mut inner).await;
    }

    async fn stop_locked(inner: &mut Inner) {
        inner.stop_requested = true;
        let child = inner.child.take();
        let connection = inner.connection.take();
        inner.status = BackendStatus::Stopped;
        let Some(child) = child else { return };
        if child.exit_code().is_some() {
            return;
        }
        match connection {
            Some(connection) => {
                let shutdown = tauri::async_runtime::spawn(async move {
                    let _ = connection.call("shutdown", None).await;
                });
                let exited = child.wait_for_exit(SHUTDOWN_TIMEOUT).await.is_some();
                shutdown.abort();
                if !exited {
                    Self::escalate(&child).await;
                }
            }
            None => Self::escalate(&child).await,
        }
        let _ = child.wait_for_exit(REAP_TIMEOUT).await;
    }

    async fn escalate(child: &ChildHandle) {
        child.terminate();
        if child.wait_for_exit(TERMINATE_TIMEOUT).await.is_none() {
            child.kill();
        }
    }

    /// `handleExit`: whether a `backend:exited` event should be emitted.
    async fn handle_exit(&self, generation: u64, code: i32) -> bool {
        let mut inner = self.inner.lock().await;
        if inner.generation != generation {
            return false; // already torn down by stop(), or a newer child
        }
        inner.child = None;
        inner.connection = None;
        if inner.stop_requested {
            inner.status = BackendStatus::Stopped;
            return false;
        }
        inner.status = BackendStatus::Failed {
            detail: format!("ctx exited ({code})"),
        };
        true
    }

    /// `BackendClient.state`.
    pub async fn status(&self) -> BackendStatus {
        self.inner.lock().await.status.clone()
    }

    /// `BackendClient.stderrLog`: the last 500 stderr lines.
    pub async fn stderr_lines(&self) -> Vec<String> {
        let ring = Arc::clone(&self.inner.lock().await.stderr);
        let lines = lock(&ring).lines();
        lines
    }

    /// `BackendClient.binaryURL`.
    pub async fn binary(&self) -> Option<PathBuf> {
        self.inner.lock().await.binary.clone()
    }

    /// `BackendClient.isMock`.
    pub async fn is_mock(&self) -> bool {
        self.inner.lock().await.mock
    }

    /// `BackendClient.call`: a raw request to the running child.
    pub async fn request(&self, method: &str, params: Option<Value>) -> Result<Value, RpcError> {
        let connection = self.inner.lock().await.connection.clone();
        let connection = connection.ok_or_else(RpcError::not_running)?;
        connection.call(method, params).await
    }
}

// MARK: - Child tasks

/// The stdout reader: strict-LF lines, UTF-8 decoded, in order.
async fn read_stdout(mut stdout: ChildStdout, connection: Arc<RpcConnection>) {
    let mut splitter = LineSplitter::default();
    let mut chunk = vec![0u8; READ_CHUNK];
    loop {
        let read = match stdout.read(&mut chunk).await {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) => {
                debug(|| format!("stdout read failed: {error}"));
                break;
            }
        };
        for line in splitter.append(&chunk[..read]) {
            match String::from_utf8(line) {
                Ok(text) => connection.receive(&text),
                Err(error) => debug(|| format!("dropped non-UTF-8 stdout line ({} bytes)", error.as_bytes().len())),
            }
        }
    }
    if let Some(rest) = splitter.drain() {
        debug(|| format!("stdout EOF with {} unframed bytes dropped", rest.len()));
    }
}

/// The stderr reader: lines into the ring and out as `backend:stderr`.
async fn read_stderr(app: AppHandle, mut stderr: ChildStderr, ring: Arc<Mutex<StderrRing>>) {
    let mut splitter = LineSplitter::default();
    let mut chunk = vec![0u8; READ_CHUNK];
    loop {
        let read = match stderr.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        let lines: Vec<String> = splitter
            .append(&chunk[..read])
            .into_iter()
            .map(|line| String::from_utf8_lossy(&line).into_owned())
            .collect();
        record_stderr(&app, &ring, lines);
    }
    if let Some(rest) = splitter.drain() {
        record_stderr(&app, &ring, vec![String::from_utf8_lossy(&rest).into_owned()]);
    }
}

fn record_stderr(app: &AppHandle, ring: &Mutex<StderrRing>, lines: Vec<String>) {
    if lines.is_empty() {
        return;
    }
    lock(ring).push(lines.iter().cloned());
    let _ = app.emit(STDERR_EVENT, StderrPayload { lines });
}

struct ExitWatch {
    app: AppHandle,
    child: Child,
    control: mpsc::UnboundedReceiver<Signal>,
    exit: watch::Sender<Option<i32>>,
    stdout_task: JoinHandle<()>,
    connection: Arc<RpcConnection>,
    stderr: Arc<Mutex<StderrRing>>,
    generation: u64,
}

/// Owns the child: waits for it (answering terminate / kill requests
/// meanwhile), then — stdout first, so every line the child wrote reaches
/// the connection before pending calls fail — closes the connection and
/// reports the exit.
async fn watch_exit(watch: ExitWatch) {
    let ExitWatch {
        app,
        mut child,
        mut control,
        exit,
        mut stdout_task,
        connection,
        stderr,
        generation,
    } = watch;

    let mut control_open = true;
    let code = loop {
        tokio::select! {
            status = child.wait() => break exit_code(status),
            signal = control.recv(), if control_open => match signal {
                Some(Signal::Terminate) => match child.id() {
                    Some(pid) if terminate_pid(pid) => {}
                    _ => {
                        let _ = child.start_kill();
                    }
                },
                Some(Signal::Kill) => {
                    let _ = child.start_kill();
                }
                None => control_open = false,
            },
        }
    };
    debug(|| format!("ctx terminated: status {code}"));
    let _ = exit.send(Some(code));

    if tokio::time::timeout(STDOUT_DRAIN_TIMEOUT, &mut stdout_task).await.is_err() {
        debug(|| "stdout still open 3 s after exit; closing it".to_owned());
        stdout_task.abort();
    }
    connection.close(RpcError::transport(format!("ctx exited ({code})")));
    let tail = lock(&stderr).tail(STDERR_TAIL_LINES);

    let should_emit = app.state::<Backend>().handle_exit(generation, code).await;
    if should_emit {
        let _ = app.emit(EXITED_EVENT, ExitedPayload { code, tail });
    }
}

/// `Process.terminationStatus`: the exit code, or the signal number.
fn exit_code(status: std::io::Result<ExitStatus>) -> i32 {
    match status {
        Ok(status) => status.code().or_else(|| signal_number(&status)).unwrap_or(-1),
        Err(_) => -1,
    }
}

#[cfg(unix)]
fn signal_number(status: &ExitStatus) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;
    status.signal()
}

#[cfg(not(unix))]
fn signal_number(_status: &ExitStatus) -> Option<i32> {
    None
}

/// Sends SIGTERM; returns false where that is impossible (Windows), so the
/// caller falls back to a kill.
#[cfg(unix)]
pub(crate) fn terminate_pid(pid: u32) -> bool {
    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    const SIGTERM: i32 = 15;
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    // SAFETY: kill(2) takes two plain integers and has no memory-safety preconditions.
    unsafe { kill(pid, SIGTERM) == 0 }
}

#[cfg(not(unix))]
pub(crate) fn terminate_pid(_pid: u32) -> bool {
    false
}

/// Windows: `CREATE_NO_WINDOW`, so the console child never flashes a window.
#[cfg(windows)]
fn configure_platform(command: &mut Command) {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn configure_platform(_command: &mut Command) {}

// MARK: - Commands

/// `backend_start({ binary_override?, data_dir?, mock, api_key_env? })`
/// → `{ pid, binary, searched }`, or a `BackendFailure`-shaped JSON error.
#[tauri::command(rename_all = "snake_case")]
pub async fn backend_start(
    app: AppHandle,
    backend: State<'_, Backend>,
    binary_override: Option<String>,
    data_dir: Option<String>,
    mock: Option<bool>,
    api_key_env: Option<String>,
) -> Result<StartResult, String> {
    let options = StartOptions {
        binary_override,
        data_dir,
        mock: mock.unwrap_or(false),
        api_key_env,
    };
    backend.start(&app, options).await.map_err(String::from)
}

/// `backend_stop()`: `shutdown` → wait ≤ 2 s → kill.
#[tauri::command]
pub async fn backend_stop(backend: State<'_, Backend>) -> Result<(), String> {
    backend.stop().await;
    Ok(())
}

/// `backend_state()` → `{ state, pid?, detail? }`.
#[tauri::command]
pub async fn backend_state(backend: State<'_, Backend>) -> Result<BackendStatus, String> {
    Ok(backend.status().await)
}

/// `backend_stderr()` → the last 500 stderr lines.
#[tauri::command]
pub async fn backend_stderr(backend: State<'_, Backend>) -> Result<Vec<String>, String> {
    Ok(backend.stderr_lines().await)
}

/// `rpc_request({ method, params? })` → the response's `result`; rejects
/// with `{ code, message, kind? }` as a JSON string.
#[tauri::command]
pub async fn rpc_request(backend: State<'_, Backend>, method: String, params: Option<Value>) -> Result<Value, String> {
    backend.request(&method, params).await.map_err(String::from)
}

/// `backend_candidates()` → the locator's search list, for the failure panel.
#[tauri::command(rename_all = "snake_case")]
pub fn backend_candidates(binary_override: Option<String>) -> Result<Vec<String>, String> {
    let override_path = binary_override.or_else(|| LaunchOptions::current().binary.clone());
    Ok(locator::candidates(override_path.as_deref())
        .iter()
        .map(|path| path.display().to_string())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitter_only_splits_on_lf() {
        let mut splitter = LineSplitter::default();
        assert_eq!(splitter.append(b"ab"), Vec::<Vec<u8>>::new());
        assert_eq!(
            splitter.append("c\nd\u{2028}e\nf".as_bytes()),
            vec![b"abc".to_vec(), "d\u{2028}e".as_bytes().to_vec()]
        );
        assert_eq!(splitter.drain(), Some(b"f".to_vec()));
        assert_eq!(splitter.drain(), None);
    }

    #[test]
    fn splitter_handles_consecutive_newlines() {
        let mut splitter = LineSplitter::default();
        assert_eq!(splitter.append(b"\n\nx\n"), vec![Vec::new(), Vec::new(), b"x".to_vec()]);
        assert_eq!(splitter.drain(), None);
    }

    #[test]
    fn ring_keeps_the_last_lines() {
        let mut ring = StderrRing::default();
        ring.push((0..(STDERR_RING_CAPACITY + 5)).map(|i| i.to_string()));
        assert_eq!(ring.lines().len(), STDERR_RING_CAPACITY);
        assert_eq!(ring.lines()[0], "5");
        assert_eq!(ring.tail(2), format!("{}\n{}", STDERR_RING_CAPACITY + 3, STDERR_RING_CAPACITY + 4));
    }

    #[test]
    fn status_and_failure_serialize_as_the_contract_says() {
        let running = serde_json::to_value(BackendStatus::Running { pid: 7 }).unwrap();
        assert_eq!(running, serde_json::json!({ "state": "running", "pid": 7 }));
        let failure = serde_json::to_value(StartFailure::NotFound { searched: vec!["/a".into()] }).unwrap();
        assert_eq!(failure, serde_json::json!({ "kind": "not_found", "searched": ["/a"] }));
    }
}
