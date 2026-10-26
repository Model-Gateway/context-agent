//! The stdio transport: an MCP server as a child process, one JSON-RPC
//! message per line on its stdin and stdout.

use super::protocol::{answer_server_request, is_server_request, notice_for, response_result, NoticeSink, ServerNotice, Transport};
use async_trait::async_trait;
use ctx_core::{CtxError, Result};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::oneshot;

/// Lines of the server's stderr kept for error messages.
const STDERR_TAIL: usize = 12;

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value>>>>>;

pub struct StdioTransport {
    stdin: Arc<tokio::sync::Mutex<ChildStdin>>,
    pending: Pending,
    next_id: AtomicU64,
    child: Mutex<Option<Child>>,
    closed: Arc<AtomicBool>,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
    description: String,
    reader: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl StdioTransport {
    /// Start `command args…` in `cwd` with `env` added to this process's
    /// environment, and begin reading its stdout.
    pub async fn spawn(command: &str, args: &[String], env: &BTreeMap<String, String>, cwd: &Path, notices: NoticeSink) -> Result<Self> {
        let path = augmented_path();
        // `npx` is `npx.cmd` on Windows, which `Command::new` would not find
        // (it only appends `.exe`); resolve against the PATH the server gets.
        let program = if cfg!(windows) { ctx_core::platform::find_executable_in(command, Some(&path)).map(|p| p.into_os_string()).unwrap_or_else(|| command.into()) } else { command.into() };
        let mut std_cmd = std::process::Command::new(program);
        ctx_core::platform::hide_console_window(&mut std_cmd);
        let mut cmd = Command::from(std_cmd);
        cmd.args(args).envs(env).env("PATH", &path).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
        if cwd.is_dir() {
            cmd.current_dir(cwd);
        }
        let description = std::iter::once(command.to_string()).chain(args.iter().cloned()).collect::<Vec<_>>().join(" ");
        let mut child = cmd.spawn().map_err(|e| CtxError::tool(format!("could not start `{description}`: {e}")))?;
        let stdin = child.stdin.take().ok_or_else(|| CtxError::tool("the server's stdin is not available"))?;
        let stdout = child.stdout.take().ok_or_else(|| CtxError::tool("the server's stdout is not available"))?;
        let stderr = child.stderr.take();
        let stdin = Arc::new(tokio::sync::Mutex::new(stdin));
        let pending: Pending = Arc::default();
        let closed = Arc::new(AtomicBool::new(false));
        let stderr_tail: Arc<Mutex<VecDeque<String>>> = Arc::default();
        if let Some(stderr) = stderr {
            let tail = stderr_tail.clone();
            let name = description.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::debug!(server = %name, "mcp stderr: {line}");
                    let mut tail = tail.lock();
                    if tail.len() == STDERR_TAIL {
                        tail.pop_front();
                    }
                    tail.push_back(line);
                }
            });
        }
        let reader = tokio::spawn(read_loop(stdout, stdin.clone(), pending.clone(), closed.clone(), stderr_tail.clone(), notices, description.clone()));
        Ok(Self { stdin, pending, next_id: AtomicU64::new(1), child: Mutex::new(Some(child)), closed, stderr_tail, description, reader: Mutex::new(Some(reader)) })
    }

    fn stderr_excerpt(&self) -> String {
        let tail = self.stderr_tail.lock();
        if tail.is_empty() {
            String::new()
        } else {
            format!("; the server printed: {}", tail.iter().cloned().collect::<Vec<_>>().join(" | ").chars().take(600).collect::<String>())
        }
    }

    async fn write(&self, message: &Value) -> Result<()> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(CtxError::tool(format!("the server exited{}", self.stderr_excerpt())));
        }
        let mut line = serde_json::to_string(message)?;
        line.push('\n');
        let mut stdin = self.stdin.lock().await;
        stdin.write_all(line.as_bytes()).await.map_err(|e| CtxError::tool(format!("could not write to the server: {e}{}", self.stderr_excerpt())))?;
        stdin.flush().await.map_err(|e| CtxError::tool(format!("could not write to the server: {e}")))?;
        Ok(())
    }
}

/// Reads the server's stdout until it closes: answers go to their waiting
/// request, server requests are answered, notifications become notices.
async fn read_loop(stdout: tokio::process::ChildStdout, stdin: Arc<tokio::sync::Mutex<ChildStdin>>, pending: Pending, closed: Arc<AtomicBool>, stderr_tail: Arc<Mutex<VecDeque<String>>>, notices: NoticeSink, name: String) {
    let mut lines = BufReader::new(stdout).lines();
    loop {
        let line = match lines.next_line().await {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(e) => {
                tracing::debug!(server = %name, error = %e, "mcp stdout read failed");
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let message: Value = match serde_json::from_str(&line) {
            Ok(m) => m,
            Err(_) => {
                // Servers that print banners to stdout break the protocol;
                // keep the line for the error message and carry on.
                tracing::debug!(server = %name, "mcp stdout is not JSON: {}", line.chars().take(200).collect::<String>());
                let mut tail = stderr_tail.lock();
                if tail.len() == STDERR_TAIL {
                    tail.pop_front();
                }
                tail.push_back(line);
                continue;
            }
        };
        dispatch(message, &stdin, &pending, &notices).await;
    }
    closed.store(true, Ordering::SeqCst);
    let waiting: Vec<oneshot::Sender<Result<Value>>> = pending.lock().drain().map(|(_, tx)| tx).collect();
    let excerpt = {
        let tail = stderr_tail.lock();
        if tail.is_empty() { String::new() } else { format!(": {}", tail.iter().cloned().collect::<Vec<_>>().join(" | ").chars().take(600).collect::<String>()) }
    };
    for tx in waiting {
        let _ = tx.send(Err(CtxError::tool(format!("the server exited before answering{excerpt}"))));
    }
    notices(ServerNotice::Closed(format!("the process exited{excerpt}")));
}

async fn dispatch(message: Value, stdin: &Arc<tokio::sync::Mutex<ChildStdin>>, pending: &Pending, notices: &NoticeSink) {
    if is_server_request(&message) {
        let answer = answer_server_request(&message);
        if let Ok(mut line) = serde_json::to_string(&answer) {
            line.push('\n');
            let mut stdin = stdin.lock().await;
            let _ = stdin.write_all(line.as_bytes()).await;
            let _ = stdin.flush().await;
        }
        return;
    }
    if message.get("method").is_some() {
        if let Some(notice) = notice_for(&message) {
            notices(notice);
        }
        return;
    }
    let id = message.get("id").and_then(|id| id.as_u64());
    let Some(id) = id else {
        tracing::debug!("mcp message without a usable id: {}", message.to_string().chars().take(200).collect::<String>());
        return;
    };
    if let Some(tx) = pending.lock().remove(&id) {
        let _ = tx.send(response_result(&message));
    }
}

#[async_trait]
impl Transport for StdioTransport {
    async fn request(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(id, tx);
        if let Err(e) = self.write(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})).await {
            self.pending.lock().remove(&id);
            return Err(e);
        }
        match rx.await {
            Ok(result) => result,
            Err(_) => Err(CtxError::tool(format!("the server exited before answering{}", self.stderr_excerpt()))),
        }
    }

    async fn notify(&self, method: &str, params: Value) -> Result<()> {
        self.write(&json!({"jsonrpc": "2.0", "method": method, "params": params})).await
    }

    async fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        // Closing stdin asks a well-behaved server to exit; the kill after
        // a short grace period is for the others.
        let child = self.child.lock().take();
        if let Some(mut child) = child {
            {
                let mut stdin = self.stdin.lock().await;
                let _ = stdin.shutdown().await;
            }
            let exited = tokio::time::timeout(std::time::Duration::from_secs(2), child.wait()).await;
            if exited.is_err() {
                let _ = child.kill().await;
            }
        }
        if let Some(reader) = self.reader.lock().take() {
            reader.abort();
        }
    }

    fn describe(&self) -> String {
        format!("`{}`", self.description)
    }
}

/// This process's `PATH` with the usual tool directories appended: the
/// desktop app's backend inherits a GUI environment that lacks Homebrew,
/// Cargo and npm directories, which is where MCP servers live.
pub fn augmented_path() -> std::ffi::OsString {
    ctx_core::platform::path_with_extras(std::env::var_os("PATH").as_deref(), &ctx_core::platform::tool_directories())
}
