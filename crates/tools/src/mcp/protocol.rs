//! The Model Context Protocol client: JSON-RPC 2.0 messages over a
//! [`Transport`], the `initialize` handshake, `tools/list` and
//! `tools/call`, and the rendering of a call's content for the model.

use async_trait::async_trait;
use ctx_core::{CtxError, ImageInput, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

/// The protocol revision this client speaks first; a server may answer
/// with an older one it supports instead.
pub const PROTOCOL_VERSION: &str = "2025-06-18";

/// Revisions this client knows well enough to talk to.
pub const SUPPORTED_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

/// Something the server sent on its own: a transport hands these to its
/// owner, which is never blocked on them.
#[derive(Debug, Clone)]
pub enum ServerNotice {
    /// `notifications/tools/list_changed`: the tool list should be fetched again.
    ToolsChanged,
    /// `notifications/message`: a log line from the server.
    Log { level: String, message: String },
    /// The connection ended (the process exited, the stream broke).
    Closed(String),
}

pub type NoticeSink = Arc<dyn Fn(ServerNotice) + Send + Sync>;

/// One way of exchanging JSON-RPC messages with a server.
#[async_trait]
pub trait Transport: Send + Sync {
    /// Send a request and wait for its response. A JSON-RPC error answer is
    /// an `Err`; so is a transport that closed before it answered.
    async fn request(&self, method: &str, params: Value) -> Result<Value>;
    /// Send a notification (no answer expected).
    async fn notify(&self, method: &str, params: Value) -> Result<()>;
    /// Record the protocol version negotiated by `initialize` (the HTTP
    /// transport sends it with every later request).
    fn set_protocol_version(&self, _version: &str) {}
    /// End the connection; idempotent.
    async fn close(&self);
    /// A short description for errors ("`npx …`", a URL).
    fn describe(&self) -> String;
}

/// Turns a raw JSON-RPC message into "what the server answered": the
/// `result`, or the error as a [`CtxError`].
pub fn response_result(message: &Value) -> Result<Value> {
    if let Some(error) = message.get("error") {
        let code = error.get("code").and_then(|c| c.as_i64()).unwrap_or_default();
        let text = error.get("message").and_then(|m| m.as_str()).unwrap_or("unknown error");
        let data = error.get("data").filter(|d| !d.is_null()).map(|d| format!(" ({})", d.to_string().chars().take(300).collect::<String>())).unwrap_or_default();
        return Err(CtxError::tool(format!("{text} (JSON-RPC error {code}){data}")));
    }
    Ok(message.get("result").cloned().unwrap_or(Value::Null))
}

/// Whether a message is a server → client request (it names a method and
/// carries an id), as opposed to a response or a notification.
pub fn is_server_request(message: &Value) -> bool {
    message.get("method").is_some() && message.get("id").map(|id| !id.is_null()).unwrap_or(false)
}

/// The answer to a server → client request this client handles on its
/// own: `ping` and `roots/list`. Anything else is "method not found".
pub fn answer_server_request(message: &Value) -> Value {
    let id = message.get("id").cloned().unwrap_or(Value::Null);
    match message.get("method").and_then(|m| m.as_str()).unwrap_or_default() {
        "ping" => json!({"jsonrpc": "2.0", "id": id, "result": {}}),
        "roots/list" => json!({"jsonrpc": "2.0", "id": id, "result": {"roots": []}}),
        method => json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": format!("{method} is not supported by this client")}}),
    }
}

/// Maps a notification from the server onto a [`ServerNotice`] (`None` for
/// the ones this client ignores, such as progress).
pub fn notice_for(message: &Value) -> Option<ServerNotice> {
    match message.get("method").and_then(|m| m.as_str())? {
        "notifications/tools/list_changed" => Some(ServerNotice::ToolsChanged),
        "notifications/message" => {
            let params = message.get("params")?;
            let level = params.get("level").and_then(|l| l.as_str()).unwrap_or("info").to_string();
            let data = params.get("data").cloned().unwrap_or(Value::Null);
            let message = match data {
                Value::String(s) => s,
                Value::Null => String::new(),
                other => other.to_string(),
            };
            Some(ServerNotice::Log { level, message })
        }
        _ => None,
    }
}

/// `tools/list` entry.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct McpToolSpec {
    pub name: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(rename = "inputSchema", default)]
    pub input_schema: Value,
    #[serde(default)]
    pub annotations: ToolAnnotations,
}

impl McpToolSpec {
    pub fn read_only(&self) -> bool {
        self.annotations.read_only_hint.unwrap_or(false)
    }

    /// Destructive unless the server says read-only, or says otherwise:
    /// the specification's default for `destructiveHint` is `true`, but a
    /// server that annotates nothing at all is treated as "unknown", not
    /// destructive, so its tools ask under the ask profile like any other
    /// side effect rather than under every profile.
    pub fn destructive(&self) -> bool {
        !self.read_only() && self.annotations.destructive_hint == Some(true)
    }
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct ToolAnnotations {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(rename = "readOnlyHint", default)]
    pub read_only_hint: Option<bool>,
    #[serde(rename = "destructiveHint", default)]
    pub destructive_hint: Option<bool>,
    #[serde(rename = "idempotentHint", default)]
    pub idempotent_hint: Option<bool>,
    #[serde(rename = "openWorldHint", default)]
    pub open_world_hint: Option<bool>,
}

/// `initialize` → `serverInfo`.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct ServerInfo {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
}

/// What `tools/call` came back with, rendered for the model.
#[derive(Debug, Clone, PartialEq)]
pub struct McpCallResult {
    pub text: String,
    pub is_error: bool,
    /// Image blocks, decoded for a follow-up message to the model.
    pub images: Vec<ImageInput>,
}

/// One connected server.
pub struct McpClient {
    transport: Box<dyn Transport>,
    pub server: ServerInfo,
    pub protocol_version: String,
    /// The server's own usage notes, if it sent any.
    pub instructions: Option<String>,
    /// Whether the server declared the `tools` capability.
    pub has_tools: bool,
}

impl McpClient {
    /// The `initialize` handshake on an open transport.
    pub async fn connect(transport: Box<dyn Transport>) -> Result<Self> {
        let params = json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "roots": { "listChanged": false } },
            "clientInfo": { "name": "context-agent", "version": env!("CARGO_PKG_VERSION") }
        });
        let result = transport.request("initialize", params).await.map_err(|e| CtxError::tool(format!("initialize failed: {e}")))?;
        let protocol_version = result.get("protocolVersion").and_then(|v| v.as_str()).unwrap_or(PROTOCOL_VERSION).to_string();
        if !SUPPORTED_VERSIONS.contains(&protocol_version.as_str()) {
            tracing::warn!(version = %protocol_version, "MCP server answered with an unknown protocol version; continuing");
        }
        transport.set_protocol_version(&protocol_version);
        let server: ServerInfo = result.get("serverInfo").cloned().map(serde_json::from_value).transpose().map_err(|e| CtxError::tool(format!("initialize returned an unreadable serverInfo: {e}")))?.unwrap_or_default();
        let has_tools = result.pointer("/capabilities/tools").is_some();
        let instructions = result.get("instructions").and_then(|i| i.as_str()).map(str::trim).filter(|i| !i.is_empty()).map(str::to_string);
        transport.notify("notifications/initialized", json!({})).await?;
        Ok(Self { transport, server, protocol_version, instructions, has_tools })
    }

    /// Every tool the server offers (all pages).
    pub async fn list_tools(&self) -> Result<Vec<McpToolSpec>> {
        if !self.has_tools {
            return Ok(Vec::new());
        }
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..64 {
            let params = match &cursor {
                Some(c) => json!({"cursor": c}),
                None => json!({}),
            };
            let result = self.transport.request("tools/list", params).await.map_err(|e| CtxError::tool(format!("tools/list failed: {e}")))?;
            let page: Vec<McpToolSpec> = result.get("tools").cloned().map(serde_json::from_value).transpose().map_err(|e| CtxError::tool(format!("tools/list returned an unreadable tool: {e}")))?.unwrap_or_default();
            tools.extend(page);
            cursor = result.get("nextCursor").and_then(|c| c.as_str()).filter(|c| !c.is_empty()).map(str::to_string);
            if cursor.is_none() {
                break;
            }
        }
        Ok(tools)
    }

    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<McpCallResult> {
        let arguments = if arguments.is_object() { arguments } else { json!({}) };
        let result = self.transport.request("tools/call", json!({"name": name, "arguments": arguments})).await?;
        Ok(render_result(&result))
    }

    pub async fn close(&self) {
        self.transport.close().await;
    }

    pub fn describe(&self) -> String {
        self.transport.describe()
    }
}

/// The `tools/call` result as text: text blocks as they are, other blocks
/// described, `structuredContent` as JSON when there is nothing else.
pub fn render_result(result: &Value) -> McpCallResult {
    let is_error = result.get("isError").and_then(|e| e.as_bool()).unwrap_or(false);
    let mut parts: Vec<String> = Vec::new();
    let mut images = Vec::new();
    for block in result.get("content").and_then(|c| c.as_array()).into_iter().flatten() {
        let kind = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
        match kind {
            "text" => parts.push(block.get("text").and_then(|t| t.as_str()).unwrap_or_default().to_string()),
            "image" if matches!(block.get("mimeType").and_then(|m| m.as_str()), Some("image/png" | "image/jpeg" | "image/gif" | "image/webp")) && block.get("data").and_then(|d| d.as_str()).is_some() => {
                let mime = block.get("mimeType").and_then(|m| m.as_str()).unwrap_or_default();
                let data = block.get("data").and_then(|d| d.as_str()).unwrap_or_default();
                images.push(ImageInput { media_type: mime.to_string(), data: data.to_string(), path: None });
                parts.push(format!("[image {} of {mime}, shown in the next message]", images.len()));
            }
            "image" | "audio" => {
                let mime = block.get("mimeType").and_then(|m| m.as_str()).unwrap_or("application/octet-stream");
                let bytes = block.get("data").and_then(|d| d.as_str()).map(|d| d.len() * 3 / 4).unwrap_or(0);
                parts.push(format!("[{kind} {mime}, about {bytes} bytes; binary content is not shown]"));
            }
            "resource" => {
                let resource = block.get("resource").cloned().unwrap_or(Value::Null);
                let uri = resource.get("uri").and_then(|u| u.as_str()).unwrap_or("");
                match resource.get("text").and_then(|t| t.as_str()) {
                    Some(text) => parts.push(format!("[resource {uri}]\n{text}")),
                    None => {
                        let mime = resource.get("mimeType").and_then(|m| m.as_str()).unwrap_or("application/octet-stream");
                        parts.push(format!("[resource {uri} ({mime}); binary content is not shown]"));
                    }
                }
            }
            "resource_link" => {
                let uri = block.get("uri").and_then(|u| u.as_str()).unwrap_or("");
                let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let description = block.get("description").and_then(|d| d.as_str()).map(|d| format!(": {d}")).unwrap_or_default();
                parts.push(format!("[resource link {name} {uri}{description}]"));
            }
            other => parts.push(format!("[{other} content]")),
        }
    }
    if parts.iter().all(|p| p.trim().is_empty()) {
        if let Some(structured) = result.get("structuredContent").filter(|s| !s.is_null()) {
            parts = vec![serde_json::to_string_pretty(structured).unwrap_or_else(|_| structured.to_string())];
        }
    }
    let mut text = parts.join("\n");
    if text.trim().is_empty() {
        text = if is_error { "the tool reported an error without details".into() } else { "(the tool returned no content)".into() };
    }
    McpCallResult { text, is_error, images }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_text_binary_and_structured_content() {
        let r = render_result(&json!({"content": [
            {"type": "text", "text": "hello"},
            {"type": "image", "data": "AAAA", "mimeType": "image/bmp"},
            {"type": "resource", "resource": {"uri": "file:///a.txt", "text": "body"}},
            {"type": "resource_link", "uri": "https://x/y", "name": "y", "description": "a page"}
        ]}));
        assert!(!r.is_error);
        assert_eq!(r.text, "hello\n[image image/bmp, about 3 bytes; binary content is not shown]\n[resource file:///a.txt]\nbody\n[resource link y https://x/y: a page]");
        let shot = render_result(&json!({"content": [{"type": "image", "data": "iVBORw0KGgo=", "mimeType": "image/png"}]}));
        assert_eq!(shot.images.len(), 1);
        assert_eq!(shot.images[0].media_type, "image/png");
        assert!(shot.text.contains("shown in the next message"));
        let structured = render_result(&json!({"content": [], "structuredContent": {"n": 1}}));
        assert_eq!(structured.text, "{\n  \"n\": 1\n}");
        let error = render_result(&json!({"isError": true, "content": [{"type": "text", "text": "boom"}]}));
        assert!(error.is_error && error.text == "boom");
        assert_eq!(render_result(&json!({})).text, "(the tool returned no content)");
    }

    #[test]
    fn jsonrpc_errors_become_tool_errors() {
        let err = response_result(&json!({"jsonrpc": "2.0", "id": 1, "error": {"code": -32602, "message": "bad", "data": {"field": "x"}}})).unwrap_err().to_string();
        assert!(err.contains("bad") && err.contains("-32602") && err.contains("field"), "{err}");
        assert_eq!(response_result(&json!({"jsonrpc": "2.0", "id": 1, "result": {"ok": true}})).unwrap(), json!({"ok": true}));
    }

    #[test]
    fn server_requests_and_notices_are_recognised() {
        let ping = json!({"jsonrpc": "2.0", "id": 7, "method": "ping"});
        assert!(is_server_request(&ping));
        assert_eq!(answer_server_request(&ping), json!({"jsonrpc": "2.0", "id": 7, "result": {}}));
        assert_eq!(answer_server_request(&json!({"jsonrpc": "2.0", "id": 8, "method": "sampling/createMessage"}))["error"]["code"], json!(-32601));
        assert!(!is_server_request(&json!({"jsonrpc": "2.0", "method": "notifications/tools/list_changed"})));
        assert!(matches!(notice_for(&json!({"method": "notifications/tools/list_changed"})), Some(ServerNotice::ToolsChanged)));
        assert!(matches!(notice_for(&json!({"method": "notifications/message", "params": {"level": "warning", "data": "careful"}})), Some(ServerNotice::Log { message, .. }) if message == "careful"));
        assert!(notice_for(&json!({"method": "notifications/progress", "params": {}})).is_none());
    }

    #[test]
    fn annotations_drive_read_only_and_destructive() {
        let spec: McpToolSpec = serde_json::from_value(json!({"name": "a", "inputSchema": {"type": "object"}, "annotations": {"readOnlyHint": true, "destructiveHint": true}})).unwrap();
        assert!(spec.read_only() && !spec.destructive(), "read-only wins");
        let spec: McpToolSpec = serde_json::from_value(json!({"name": "b", "annotations": {"destructiveHint": true}})).unwrap();
        assert!(!spec.read_only() && spec.destructive());
        let spec: McpToolSpec = serde_json::from_value(json!({"name": "c"})).unwrap();
        assert!(!spec.read_only() && !spec.destructive(), "nothing annotated: neither");
    }
}
