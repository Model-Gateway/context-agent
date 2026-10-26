//! The Streamable HTTP transport (MCP 2025-03-26 and later): every message
//! is a POST to one URL; the server answers with JSON, or with a
//! `text/event-stream` that carries the answer among other messages.

use super::protocol::{is_server_request, notice_for, response_result, NoticeSink, Transport};
use async_trait::async_trait;
use ctx_core::{CtxError, Result};
use futures::StreamExt;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

pub struct HttpTransport {
    client: reqwest::Client,
    url: String,
    headers: Vec<(String, String)>,
    session: Mutex<Option<String>>,
    protocol_version: Mutex<Option<String>>,
    next_id: AtomicU64,
    notices: NoticeSink,
}

impl HttpTransport {
    pub fn new(url: &str, headers: Vec<(String, String)>, notices: NoticeSink) -> Result<Self> {
        let client = reqwest::Client::builder()
            // Streams stay open for as long as a tool call runs; per-call
            // limits are applied by the caller.
            .connect_timeout(Duration::from_secs(20))
            .build()
            .map_err(|e| CtxError::tool(format!("could not build the HTTP client: {e}")))?;
        Ok(Self { client, url: url.to_string(), headers, session: Mutex::new(None), protocol_version: Mutex::new(None), next_id: AtomicU64::new(1), notices })
    }

    fn post(&self, body: &Value) -> reqwest::RequestBuilder {
        let mut request = self.client.post(&self.url).header("Accept", "application/json, text/event-stream").header("Content-Type", "application/json").json(body);
        for (name, value) in &self.headers {
            request = request.header(name, value);
        }
        if let Some(session) = self.session.lock().clone() {
            request = request.header("Mcp-Session-Id", session);
        }
        if let Some(version) = self.protocol_version.lock().clone() {
            request = request.header("MCP-Protocol-Version", version);
        }
        request
    }

    fn remember_session(&self, response: &reqwest::Response) {
        if let Some(session) = response.headers().get("mcp-session-id").and_then(|v| v.to_str().ok()) {
            *self.session.lock() = Some(session.to_string());
        }
    }

    /// Handles a message that is not the answer we wait for.
    fn aside(&self, message: &Value) {
        if is_server_request(message) {
            tracing::debug!(url = %self.url, "ignoring a server request over HTTP: {}", message.get("method").and_then(|m| m.as_str()).unwrap_or(""));
        } else if message.get("method").is_some() {
            if let Some(notice) = notice_for(message) {
                (self.notices)(notice);
            }
        }
    }
}

fn status_error(status: reqwest::StatusCode, body: &str) -> CtxError {
    let excerpt = body.chars().take(300).collect::<String>();
    let hint = match status.as_u16() {
        401 | 403 => " (check the server's headers / credentials)",
        404 => " (not an MCP endpoint, or the session expired)",
        405 => " (the server does not accept POST here; the legacy HTTP+SSE transport is not supported)",
        _ => "",
    };
    CtxError::tool(format!("HTTP {status}{hint}: {excerpt}"))
}

#[async_trait]
impl Transport for HttpTransport {
    async fn request(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let body = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let response = self.post(&body).send().await.map_err(|e| CtxError::tool(format!("request to {} failed: {e}", self.url)))?;
        self.remember_session(&response);
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(status_error(status, &text));
        }
        let content_type = response.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_ascii_lowercase();
        if content_type.starts_with("text/event-stream") {
            let mut stream = response.bytes_stream();
            let mut buffer = String::new();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|e| CtxError::tool(format!("the event stream from {} broke: {e}", self.url)))?;
                buffer.push_str(&String::from_utf8_lossy(&chunk));
                for data in drain_sse_events(&mut buffer) {
                    let message: Value = match serde_json::from_str(&data) {
                        Ok(m) => m,
                        Err(_) => continue,
                    };
                    if message.get("id").and_then(|i| i.as_u64()) == Some(id) && message.get("method").is_none() {
                        return response_result(&message);
                    }
                    self.aside(&message);
                }
            }
            return Err(CtxError::tool(format!("the event stream from {} ended before the answer to {method}", self.url)));
        }
        let text = response.text().await.map_err(|e| CtxError::tool(format!("the response from {} could not be read: {e}", self.url)))?;
        let message: Value = serde_json::from_str(&text).map_err(|e| CtxError::tool(format!("{} answered with invalid JSON: {e}", self.url)))?;
        // A server may batch; find our answer.
        if let Some(batch) = message.as_array() {
            for item in batch {
                if item.get("id").and_then(|i| i.as_u64()) == Some(id) && item.get("method").is_none() {
                    return response_result(item);
                }
                self.aside(item);
            }
            return Err(CtxError::tool(format!("{} answered without a response to {method}", self.url)));
        }
        response_result(&message)
    }

    async fn notify(&self, method: &str, params: Value) -> Result<()> {
        let body = json!({"jsonrpc": "2.0", "method": method, "params": params});
        let response = self.post(&body).send().await.map_err(|e| CtxError::tool(format!("request to {} failed: {e}", self.url)))?;
        self.remember_session(&response);
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(status_error(status, &text));
        }
        Ok(())
    }

    fn set_protocol_version(&self, version: &str) {
        *self.protocol_version.lock() = Some(version.to_string());
    }

    async fn close(&self) {
        let session = self.session.lock().take();
        if let Some(session) = session {
            let mut request = self.client.delete(&self.url).header("Mcp-Session-Id", session).timeout(Duration::from_secs(5));
            for (name, value) in &self.headers {
                request = request.header(name, value);
            }
            let _ = request.send().await;
        }
    }

    fn describe(&self) -> String {
        self.url.clone()
    }
}

/// Removes every complete server-sent event from `buffer` and returns
/// their `data` payloads (multi-line data joined with newlines; comments,
/// `event:` and `id:` fields dropped).
pub fn drain_sse_events(buffer: &mut String) -> Vec<String> {
    let mut events = Vec::new();
    loop {
        let normalized = buffer.replace("\r\n", "\n");
        let Some(end) = normalized.find("\n\n") else {
            *buffer = normalized;
            break;
        };
        let block = normalized[..end].to_string();
        *buffer = normalized[end + 2..].to_string();
        let data: Vec<&str> = block.lines().filter_map(|line| line.strip_prefix("data:")).map(|d| d.strip_prefix(' ').unwrap_or(d)).collect();
        if !data.is_empty() {
            events.push(data.join("\n"));
        }
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_events_are_split_and_joined() {
        let mut buffer = String::from(": comment\nevent: message\ndata: {\"a\":\ndata: 1}\n\ndata: second\r\n\r\ndata: partial");
        let events = drain_sse_events(&mut buffer);
        assert_eq!(events, vec!["{\"a\":\n1}".to_string(), "second".to_string()]);
        assert_eq!(buffer, "data: partial");
        buffer.push_str("\n\n");
        assert_eq!(drain_sse_events(&mut buffer), vec!["partial".to_string()]);
        assert!(buffer.is_empty());
    }
}
