//! JSON-RPC 2.0 over newline-delimited frames: the port of `JSONRPC.swift`
//! (`RPCConnection`, docs/desktop-protocol.md "Framing").
//!
//! `RpcConnection` owns the request ids, the pending map and the write side
//! of the child's stdin. Every outgoing line goes through one writer task
//! behind an `mpsc` channel so frames never interleave; `BackendProcess`
//! (`mod.rs`) hands complete stdout lines to [`RpcConnection::receive`] in the
//! order the child wrote them. A response resolves its pending request; a
//! notification is emitted to the frontend as the `backend:notification`
//! event (`{ method, params }`), still in frame order.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;
use tokio::process::ChildStdin;
use tokio::sync::{mpsc, oneshot};

use super::{debug, lock};

/// The event every notification frame is emitted as (`{ method, params }`).
pub const NOTIFICATION_EVENT: &str = "backend:notification";

/// `BackendError` as the frontend sees it: a rejected `rpc_request` carries
/// this as a JSON string. `code` / `message` come from the backend's error
/// response and `kind` is its `data.kind` (`CtxError::code()`); errors raised
/// by the shell itself use [`RpcError::LOCAL_CODE`] and one of the `KIND_*`
/// kinds (`.transport`, `.notRunning`, `.decoding` in Swift).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

impl RpcError {
    /// `code` of every error raised by the shell rather than by the backend.
    pub const LOCAL_CODE: i64 = -1;
    /// `BackendError.transport`: the child could not be reached or went away.
    pub const KIND_TRANSPORT: &'static str = "transport";
    /// `BackendError.notRunning`: no child at all.
    pub const KIND_NOT_RUNNING: &'static str = "not_running";
    /// `BackendError.decoding`: a request could not be encoded.
    pub const KIND_DECODING: &'static str = "decoding";

    fn local(kind: &str, message: impl Into<String>) -> Self {
        Self {
            code: Self::LOCAL_CODE,
            message: message.into(),
            kind: Some(kind.to_owned()),
        }
    }

    /// `BackendError.transport(message)`.
    pub fn transport(message: impl Into<String>) -> Self {
        Self::local(Self::KIND_TRANSPORT, message)
    }

    /// `BackendError.notRunning`.
    pub fn not_running() -> Self {
        Self::local(Self::KIND_NOT_RUNNING, "The backend is not running.")
    }

    /// `BackendError.decoding(message)`.
    pub fn decoding(message: impl Into<String>) -> Self {
        Self::local(Self::KIND_DECODING, message)
    }

    /// The JSON the frontend's `BackendError` is rebuilt from.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| {
            format!(
                "{{\"code\":{},\"message\":{:?},\"kind\":\"{}\"}}",
                self.code,
                self.message,
                Self::KIND_TRANSPORT
            )
        })
    }
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for RpcError {}

impl From<RpcError> for String {
    fn from(error: RpcError) -> Self {
        error.to_json()
    }
}

/// One decoded line from the backend (`Frame` in Swift): a response carries
/// `id` plus `result` or `error`; a notification carries `method` and no `id`.
#[derive(Deserialize)]
struct Frame {
    #[serde(default)]
    id: Option<Value>,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    params: Option<Value>,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<ErrorBody>,
}

/// `RPCErrorBody`: `data.kind` is the backend's `CtxError::code()` when present.
#[derive(Deserialize)]
struct ErrorBody {
    code: i64,
    message: String,
    #[serde(default)]
    data: Option<Value>,
}

impl ErrorBody {
    fn kind(&self) -> Option<String> {
        self.data
            .as_ref()
            .and_then(|data| data.get("kind"))
            .and_then(Value::as_str)
            .map(str::to_owned)
    }
}

/// What we write (`RPCRequest`); `params` is omitted entirely when absent.
#[derive(Serialize)]
struct Request<'a> {
    jsonrpc: &'static str,
    id: u64,
    method: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<&'a Value>,
}

/// The `backend:notification` payload (`RPCNotification` without `raw`).
#[derive(Debug, Clone, Serialize)]
pub struct Notification {
    pub method: String,
    pub params: Value,
}

/// Resolves one pending request with its result or error.
pub type PendingSender = oneshot::Sender<Result<Value, RpcError>>;
/// Request id → the caller waiting for the response.
pub type PendingMap = Arc<Mutex<HashMap<u64, PendingSender>>>;

/// One line for the writer task; `id` lets a failed write fail its request.
pub struct Outgoing {
    pub id: u64,
    pub bytes: Vec<u8>,
}

/// Serialises every write to the child and matches responses to callers.
pub struct RpcConnection {
    app: AppHandle,
    next_id: AtomicU64,
    pending: PendingMap,
    writer: mpsc::UnboundedSender<Outgoing>,
    /// Set by [`RpcConnection::close`]: refuses new requests afterwards.
    closed: Mutex<Option<RpcError>>,
}

impl RpcConnection {
    /// Creates the connection and its writer task over the child's stdin.
    pub fn new(app: AppHandle, stdin: ChildStdin) -> Arc<Self> {
        let (writer, receiver) = mpsc::unbounded_channel();
        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        tauri::async_runtime::spawn(writer_task(stdin, receiver, Arc::clone(&pending)));
        Arc::new(Self {
            app,
            next_id: AtomicU64::new(1),
            pending,
            writer,
            closed: Mutex::new(None),
        })
    }

    /// Requests still waiting for a response.
    pub fn pending_count(&self) -> usize {
        lock(&self.pending).len()
    }

    /// True once [`RpcConnection::close`] ran (the child exited).
    pub fn is_closed(&self) -> bool {
        lock(&self.closed).is_some()
    }

    /// Sends a request and waits for its response. Fails with the close
    /// error once the child has exited, or with a transport error when the
    /// line could not be written.
    pub async fn call(&self, method: &str, params: Option<Value>) -> Result<Value, RpcError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = Request {
            jsonrpc: "2.0",
            id,
            method,
            params: params.as_ref(),
        };
        let mut bytes = serde_json::to_vec(&request)
            .map_err(|error| RpcError::decoding(format!("could not encode {method}: {error}")))?;
        bytes.push(b'\n');

        let (sender, receiver) = oneshot::channel();
        {
            // Lock order everywhere: `closed`, then `pending`. Holding `closed`
            // while registering means `close()` can never miss this request.
            let closed = lock(&self.closed);
            if let Some(error) = closed.as_ref() {
                return Err(error.clone());
            }
            lock(&self.pending).insert(id, sender);
            if self.writer.send(Outgoing { id, bytes }).is_err() {
                lock(&self.pending).remove(&id);
                return Err(RpcError::transport("write to ctx failed: stdin is closed"));
            }
        }

        match receiver.await {
            Ok(outcome) => outcome,
            Err(_) => Err(RpcError::transport("request dropped")),
        }
    }

    /// Fails one pending request (the caller gave up on it).
    pub fn cancel(&self, id: u64) {
        if let Some(sender) = lock(&self.pending).remove(&id) {
            let _ = sender.send(Err(RpcError::transport("request cancelled")));
        }
    }

    /// One complete stdout line (without its LF), already UTF-8 decoded.
    pub fn receive(&self, line: &str) {
        let frame: Frame = match serde_json::from_str(line) {
            Ok(frame) => frame,
            Err(error) => {
                debug(|| format!("dropped undecodable frame: {error} — {}", prefix(line, 200)));
                return;
            }
        };
        if let Some(id) = frame.id.as_ref().and_then(Value::as_u64) {
            let Some(sender) = lock(&self.pending).remove(&id) else {
                debug(|| format!("response for unknown id {id}"));
                return;
            };
            let outcome = match frame.error {
                Some(error) => Err(RpcError {
                    code: error.code,
                    message: error.message.clone(),
                    kind: error.kind(),
                }),
                None => Ok(frame.result.unwrap_or(Value::Null)),
            };
            let _ = sender.send(outcome);
        } else if let Some(method) = frame.method {
            let payload = Notification {
                method,
                params: frame.params.unwrap_or(Value::Null),
            };
            if let Err(error) = self.app.emit(NOTIFICATION_EVENT, payload) {
                debug(|| format!("could not emit notification: {error}"));
            }
        } else {
            debug(|| format!("frame with neither id nor method: {}", prefix(line, 200)));
        }
    }

    /// Fails every pending request and refuses new ones (process exit).
    pub fn close(&self, error: RpcError) {
        let mut closed = lock(&self.closed);
        *closed = Some(error.clone());
        let waiting: Vec<PendingSender> = lock(&self.pending).drain().map(|(_, sender)| sender).collect();
        drop(closed);
        for sender in waiting {
            let _ = sender.send(Err(error.clone()));
        }
    }
}

/// The single writer: lines reach the child's stdin one after another, each
/// flushed. A failed write (EPIPE after the child died) fails its request and
/// every request queued behind it; later calls fail at the channel.
async fn writer_task(mut stdin: ChildStdin, mut receiver: mpsc::UnboundedReceiver<Outgoing>, pending: PendingMap) {
    while let Some(outgoing) = receiver.recv().await {
        let written = match stdin.write_all(&outgoing.bytes).await {
            Ok(()) => stdin.flush().await,
            Err(error) => Err(error),
        };
        if let Err(error) = written {
            debug(|| format!("write to ctx failed: {error}"));
            let message = format!("write to ctx failed: {error}");
            fail(&pending, outgoing.id, &message);
            receiver.close();
            while let Ok(queued) = receiver.try_recv() {
                fail(&pending, queued.id, &message);
            }
            break;
        }
    }
    // Dropping `stdin` closes our end of the pipe (`closeStdin()` in Swift).
}

fn fail(pending: &PendingMap, id: u64, message: &str) {
    if let Some(sender) = lock(pending).remove(&id) {
        let _ = sender.send(Err(RpcError::transport(message)));
    }
}

fn prefix(text: &str, count: usize) -> String {
    text.chars().take(count).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_errors_serialize_with_their_kind() {
        let error = RpcError::transport("ctx exited (1)");
        let json: Value = serde_json::from_str(&error.to_json()).unwrap();
        assert_eq!(json["code"], -1);
        assert_eq!(json["message"], "ctx exited (1)");
        assert_eq!(json["kind"], "transport");
    }

    #[test]
    fn backend_error_kind_comes_from_data() {
        let frame: Frame = serde_json::from_str(
            r#"{"jsonrpc":"2.0","id":3,"error":{"code":-32000,"message":"no key","data":{"kind":"provider_auth"}}}"#,
        )
        .unwrap();
        assert_eq!(frame.id.unwrap().as_u64(), Some(3));
        assert_eq!(frame.error.unwrap().kind().as_deref(), Some("provider_auth"));
    }

    #[test]
    fn request_omits_absent_params() {
        let request = Request { jsonrpc: "2.0", id: 1, method: "initialize", params: None };
        assert_eq!(
            serde_json::to_string(&request).unwrap(),
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#
        );
    }
}
