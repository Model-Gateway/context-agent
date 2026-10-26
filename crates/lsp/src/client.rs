//! Minimal JSON-RPC-over-stdio LSP client.
//!
//! Scope: initialize/initialized handshake, document sync (`didOpen` /
//! `didChange` with full text / `didClose`), position requests
//! (`definition`, `references`, and raw `completion` / `hover` for the
//! desktop editor), server notifications handed to an optional callback
//! (`publishDiagnostics`), shutdown/exit. Everything is guarded by timeouts
//! so a misbehaving server never blocks the agent.

use crate::service::{Location, SemanticOutcome, SemanticService};
use async_trait::async_trait;
use ctx_core::{CtxError, Result};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{mpsc, oneshot};

/// Receives every notification and request the server sends on its own
/// (`textDocument/publishDiagnostics`, `window/logMessage`, …).
pub type NotificationHandler = Arc<dyn Fn(Value) + Send + Sync>;

pub struct LspClient {
    root: PathBuf,
    language_id: String,
    stdin: tokio::sync::Mutex<ChildStdin>,
    _child: Child,
    next_id: AtomicI64,
    pending: Arc<Mutex<HashMap<i64, oneshot::Sender<Value>>>>,
    /// Absolute path → document version sent with the last `didOpen` / `didChange`.
    opened: Mutex<HashMap<String, i64>>,
    /// The server's `initialize` result capabilities.
    capabilities: Value,
    timeout: Duration,
}

/// Encode a JSON-RPC message with LSP framing.
pub fn frame(message: &Value) -> Vec<u8> {
    let body = message.to_string();
    format!("Content-Length: {}\r\n\r\n{}", body.len(), body).into_bytes()
}

/// Read one framed message from a buffered reader.
pub async fn read_message<R: tokio::io::AsyncRead + Unpin>(reader: &mut BufReader<R>) -> std::io::Result<Option<Value>> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some(v) = trimmed.strip_prefix("Content-Length:") {
            content_length = v.trim().parse().ok();
        }
    }
    let len = content_length.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "missing Content-Length"))?;
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await?;
    serde_json::from_slice(&buf).map(Some).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// The LSP `languageId` of one document: the CSS server parses SCSS and
/// Less only when told so, and shell servers expect `shellscript`.
fn document_language_id<'a>(language: &'a str, path: &Path) -> &'a str {
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    match (language, ext.as_str()) {
        ("css", "scss") => "scss",
        ("css", "sass") => "sass",
        ("css", "less") => "less",
        ("shell", _) => "shellscript",
        _ => language,
    }
}

/// `abs` relative to `root` with forward slashes (the form the rest of the
/// backend and the app use), else `abs` unchanged. Tolerates a verbatim
/// `\\?\` prefix on either side and, on Windows, a drive letter in either case.
pub fn relative_to(root: &Path, abs: &str) -> String {
    let root = ctx_core::platform::simplified(root);
    let path = ctx_core::platform::simplified(Path::new(abs));
    if let Ok(rel) = path.strip_prefix(root) {
        return rel.to_string_lossy().replace('\\', "/");
    }
    if cfg!(windows) {
        let (r, p) = (root.to_string_lossy().replace('/', "\\").to_ascii_lowercase(), path.to_string_lossy().replace('/', "\\"));
        let r = r.trim_end_matches('\\');
        if p.to_ascii_lowercase().starts_with(r) && p[r.len()..].starts_with('\\') {
            return p[r.len() + 1..].replace('\\', "/");
        }
    }
    abs.to_string()
}

/// `path` as a `file://` URI (`file:///C:/repo/a.rs` on Windows,
/// percent-encoded), as language servers expect.
pub fn file_uri(path: &Path) -> String {
    url::Url::from_file_path(path).map(String::from).unwrap_or_else(|_| format!("file://{}", path.to_string_lossy()))
}

/// The local path a server's `file://` URI names (`C:\repo\a.rs` on
/// Windows, decoding `file:///c%3A/…`), else the URI minus its scheme with
/// percent escapes decoded.
pub fn uri_to_path(uri: &str) -> String {
    if let Ok(parsed) = url::Url::parse(uri) {
        if parsed.scheme() == "file" {
            if let Ok(path) = parsed.to_file_path() {
                return path.to_string_lossy().into_owned();
            }
        }
    }
    let raw = uri.strip_prefix("file://").unwrap_or(uri);
    // Servers percent-encode spaces and the like.
    let mut out = Vec::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&raw[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

impl LspClient {
    pub async fn start(executable: &str, args: &[&str], root: &Path, language_id: &str, timeout: Duration) -> Result<Arc<Self>> {
        Self::start_with(executable, args, root, language_id, timeout, None).await
    }

    /// `start` with a callback for the server's own notifications.
    pub async fn start_with(executable: &str, args: &[&str], root: &Path, language_id: &str, timeout: Duration, on_notification: Option<NotificationHandler>) -> Result<Arc<Self>> {
        let mut command = std::process::Command::new(executable);
        ctx_core::platform::hide_console_window(&mut command);
        let mut child = Command::from(command)
            .args(args)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| CtxError::other(format!("failed to start language server {executable}: {e}")))?;
        let stdin = child.stdin.take().ok_or_else(|| CtxError::other("no stdin for language server"))?;
        let stdout = child.stdout.take().ok_or_else(|| CtxError::other("no stdout for language server"))?;
        let pending: Arc<Mutex<HashMap<i64, oneshot::Sender<Value>>>> = Arc::new(Mutex::new(HashMap::new()));
        let pending_reader = pending.clone();
        let (notify_tx, mut notify_rx) = mpsc::channel::<Value>(64);
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            while let Ok(Some(msg)) = read_message(&mut reader).await {
                if let Some(id) = msg.get("id").and_then(|i| i.as_i64()) {
                    if msg.get("method").is_none() {
                        if let Some(tx) = pending_reader.lock().remove(&id) {
                            let _ = tx.send(msg);
                        }
                        continue;
                    }
                }
                let _ = notify_tx.send(msg).await;
            }
        });
        // Hand notifications to the callback (or drain them) so the channel never fills.
        tokio::spawn(async move {
            while let Some(message) = notify_rx.recv().await {
                if let Some(handler) = &on_notification {
                    handler(message);
                }
            }
        });
        let mut client = Self {
            root: root.to_path_buf(),
            language_id: language_id.to_string(),
            stdin: tokio::sync::Mutex::new(stdin),
            _child: child,
            next_id: AtomicI64::new(1),
            pending,
            opened: Mutex::new(Default::default()),
            capabilities: Value::Null,
            timeout,
        };
        let init = json!({
            "processId": std::process::id(),
            "rootUri": file_uri(root),
            "capabilities": {
                "textDocument": {
                    "definition": {"linkSupport": false},
                    "references": {},
                    "synchronization": {"didSave": false, "willSave": false},
                    "completion": {"completionItem": {"snippetSupport": true, "documentationFormat": ["plaintext", "markdown"]}, "contextSupport": true},
                    "hover": {"contentFormat": ["plaintext", "markdown"]},
                    "publishDiagnostics": {"relatedInformation": false}
                }
            },
            "workspaceFolders": [{"uri": file_uri(root), "name": root.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default()}]
        });
        let result = client.request("initialize", init).await?;
        client.capabilities = result.get("capabilities").cloned().unwrap_or(Value::Null);
        let client = Arc::new(client);
        client.notify("initialized", json!({})).await?;
        Ok(client)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Characters after which the server wants a completion request (`.`, `::`, …).
    pub fn completion_trigger_characters(&self) -> Vec<String> {
        self.capabilities
            .pointer("/completionProvider/triggerCharacters")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|c| c.as_str().map(String::from)).collect())
            .unwrap_or_default()
    }

    pub fn absolute(&self, path: &str) -> PathBuf {
        if Path::new(path).is_absolute() { PathBuf::from(path) } else { self.root.join(path) }
    }

    /// Repository-relative form of a `file://` URI the server sent.
    pub fn relative(&self, uri: &str) -> String {
        relative_to(&self.root, &uri_to_path(uri))
    }

    /// `didOpen` with the caller's text (an editor buffer, not the disk), or
    /// `didChange` when the document is already open. Returns the version.
    pub async fn open_document(&self, path: &str, text: &str) -> Result<i64> {
        self.sync_document(path, text).await
    }

    /// Full-text `didChange`; opens the document first when needed.
    pub async fn change_document(&self, path: &str, text: &str) -> Result<i64> {
        self.sync_document(path, text).await
    }

    async fn sync_document(&self, path: &str, text: &str) -> Result<i64> {
        let abs = self.absolute(path);
        let key = abs.to_string_lossy().to_string();
        let version = {
            let mut opened = self.opened.lock();
            match opened.get_mut(&key) {
                Some(v) => {
                    *v += 1;
                    Some(*v)
                }
                None => {
                    opened.insert(key, 1);
                    None
                }
            }
        };
        match version {
            Some(version) => {
                self.notify("textDocument/didChange", json!({"textDocument": {"uri": file_uri(&abs), "version": version}, "contentChanges": [{"text": text}]})).await?;
                Ok(version)
            }
            None => {
                let language_id = document_language_id(&self.language_id, &abs);
                self.notify("textDocument/didOpen", json!({"textDocument": {"uri": file_uri(&abs), "languageId": language_id, "version": 1, "text": text}})).await?;
                Ok(1)
            }
        }
    }

    pub async fn close_document(&self, path: &str) -> Result<()> {
        let abs = self.absolute(path);
        if self.opened.lock().remove(&abs.to_string_lossy().to_string()).is_some() {
            self.notify("textDocument/didClose", json!({"textDocument": {"uri": file_uri(&abs)}})).await?;
        }
        Ok(())
    }

    /// A `textDocument/*` request at a 0-based position, with `extra` merged
    /// into the params; the raw result is returned for the caller to shape.
    pub async fn position_request_raw(&self, method: &str, path: &str, line: usize, character: usize, extra: Value) -> Result<Value> {
        let abs = self.ensure_open(path).await?;
        let mut params = json!({
            "textDocument": {"uri": file_uri(&abs)},
            "position": {"line": line, "character": character}
        });
        if let (Some(obj), Some(extra)) = (params.as_object_mut(), extra.as_object()) {
            for (k, v) in extra {
                obj.insert(k.clone(), v.clone());
            }
        }
        self.request(method, params).await
    }

    async fn send(&self, message: &Value) -> Result<()> {
        let bytes = frame(message);
        let mut stdin = self.stdin.lock().await;
        tokio::time::timeout(self.timeout, stdin.write_all(&bytes)).await.map_err(|_| CtxError::Timeout(self.timeout))?.map_err(CtxError::from)?;
        Ok(())
    }

    pub async fn request(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(id, tx);
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})).await?;
        let response = tokio::time::timeout(self.timeout, rx).await.map_err(|_| {
            self.pending.lock().remove(&id);
            CtxError::Timeout(self.timeout)
        })?;
        let response = response.map_err(|_| CtxError::other("language server closed"))?;
        if let Some(err) = response.get("error") {
            return Err(CtxError::other(format!("language server error for {method}: {err}")));
        }
        Ok(response.get("result").cloned().unwrap_or(Value::Null))
    }

    pub async fn notify(&self, method: &str, params: Value) -> Result<()> {
        self.send(&json!({"jsonrpc": "2.0", "method": method, "params": params})).await
    }

    /// Opens the document from disk unless the caller opened it with its own text.
    async fn ensure_open(&self, path: &str) -> Result<PathBuf> {
        let abs = self.absolute(path);
        if !self.opened.lock().contains_key(&abs.to_string_lossy().to_string()) {
            let text = tokio::fs::read_to_string(&abs).await?;
            self.open_document(path, &text).await?;
        }
        Ok(abs)
    }

    pub fn parse_locations(&self, value: &Value) -> Vec<Location> {
        let items: Vec<&Value> = match value {
            Value::Array(a) => a.iter().collect(),
            Value::Object(_) => vec![value],
            _ => vec![],
        };
        items
            .into_iter()
            .filter_map(|loc| {
                let uri = loc.get("uri").or_else(|| loc.get("targetUri"))?.as_str()?;
                let range = loc.get("range").or_else(|| loc.get("targetSelectionRange"))?;
                let line = range.pointer("/start/line")?.as_u64()? as usize + 1;
                let column = range.pointer("/start/character")?.as_u64()? as usize + 1;
                Some(Location { path: self.relative(uri), line, column })
            })
            .collect()
    }

    async fn position_request(&self, method: &str, path: &str, line: usize, column: usize, extra: Value) -> SemanticOutcome {
        match self.position_request_raw(method, path, line.saturating_sub(1), column.saturating_sub(1), extra).await {
            Ok(v) => SemanticOutcome::Found(self.parse_locations(&v)),
            Err(e) => SemanticOutcome::Unavailable { reason: e.to_string() },
        }
    }

    pub async fn shutdown(&self) {
        let _ = self.request("shutdown", Value::Null).await;
        let _ = self.notify("exit", Value::Null).await;
    }
}

#[async_trait]
impl SemanticService for LspClient {
    async fn definition(&self, path: &str, line: usize, column: usize) -> SemanticOutcome {
        self.position_request("textDocument/definition", path, line, column, json!({})).await
    }

    async fn references(&self, path: &str, line: usize, column: usize) -> SemanticOutcome {
        self.position_request("textDocument/references", path, line, column, json!({"context": {"includeDeclaration": true}})).await
    }

    fn describe(&self) -> String {
        format!("semantic level: language server for {} at {}", self.language_id, self.root.display())
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn file_uris_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let abs = ctx_core::platform::canonicalize(dir.path()).unwrap().join("src dir").join("a.rs");
        let uri = super::file_uri(&abs);
        assert!(uri.starts_with("file:///"), "{uri}");
        assert!(uri.contains("src%20dir"), "{uri}");
        assert_eq!(std::path::PathBuf::from(super::uri_to_path(&uri)), abs);
    }

    #[cfg(windows)]
    #[test]
    fn windows_file_uris() {
        assert_eq!(super::file_uri(std::path::Path::new(r"C:\proj\src\a.rs")), "file:///C:/proj/src/a.rs");
        assert_eq!(super::uri_to_path("file:///C:/proj/src/a.rs"), r"C:\proj\src\a.rs");
        assert_eq!(super::uri_to_path("file:///c%3A/proj/a.rs").to_ascii_lowercase(), r"c:\proj\a.rs");
    }

    use super::*;

    #[tokio::test]
    async fn framing_round_trip() {
        let msg = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"a": "é"}});
        let bytes = frame(&msg);
        let mut reader = BufReader::new(std::io::Cursor::new(bytes));
        let decoded = read_message(&mut reader).await.unwrap().unwrap();
        assert_eq!(decoded, msg);
        assert!(read_message(&mut reader).await.unwrap().is_none());
    }

    /// Drives the client against a tiny fake language server written in
    /// Python, which exists on most developer machines; skipped otherwise.
    #[tokio::test]
    async fn definition_against_fake_server() {
        let python = ctx_core::platform::python_interpreter();
        if crate::catalog::ServerCatalog::which(python).is_none() {
            eprintln!("{python} not available; skipping fake server test");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake_lsp.py");
        std::fs::write(
            &script,
            r#"
import sys, json
def read():
    length = None
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        line = line.decode().strip()
        if not line:
            break
        if line.lower().startswith("content-length:"):
            length = int(line.split(":")[1])
    return json.loads(sys.stdin.buffer.read(length))
def send(obj):
    body = json.dumps(obj).encode()
    sys.stdout.buffer.write(b"Content-Length: %d\r\n\r\n" % len(body) + body)
    sys.stdout.buffer.flush()
while True:
    msg = read()
    if msg is None:
        break
    m = msg.get("method")
    if m == "initialize":
        send({"jsonrpc": "2.0", "id": msg["id"], "result": {"capabilities": {}}})
    elif m == "textDocument/definition":
        uri = msg["params"]["textDocument"]["uri"]
        send({"jsonrpc": "2.0", "id": msg["id"], "result": [{"uri": uri, "range": {"start": {"line": 4, "character": 2}, "end": {"line": 4, "character": 8}}}]})
    elif m == "shutdown":
        send({"jsonrpc": "2.0", "id": msg["id"], "result": None})
    elif m == "exit":
        break
"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("a.py"), "x = 1\n").unwrap();
        let script_str = script.to_string_lossy().to_string();
        let client = LspClient::start(&crate::catalog::ServerCatalog::which(python).unwrap(), &["-I", &script_str], dir.path(), "python", Duration::from_secs(5)).await.unwrap();
        match client.definition("a.py", 1, 1).await {
            SemanticOutcome::Found(locs) => {
                assert_eq!(locs.len(), 1);
                assert_eq!(locs[0].path, "a.py");
                assert_eq!((locs[0].line, locs[0].column), (5, 3));
            }
            other => panic!("unexpected {other:?}"),
        }
        client.shutdown().await;
    }
}
