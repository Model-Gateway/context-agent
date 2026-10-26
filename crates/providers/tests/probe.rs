//! The capability probe and the live transport against a fake
//! OpenAI-compatible server that answers dynamically.

use ctx_core::{AgentMessage, CancellationToken, ModelConfig, ModelRequest, ToolDefinition};
use ctx_providers::{ModelProvider, OpenAiCompatibleProvider, ProbeStatus};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Behaviour switches for the fake server.
#[derive(Clone, Copy, Default)]
struct Quirks {
    /// Reject `max_tokens` with the OpenAI "use max_completion_tokens" error.
    needs_max_completion_tokens: bool,
    /// Reject `stream_options` (older servers).
    rejects_stream_options: bool,
    /// Never emit tool calls (a chat-only model).
    no_tools: bool,
}

async fn fake_server(quirks: Quirks) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let requests = Arc::new(AtomicUsize::new(0));
    let counter = requests.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else { break };
            let counter = counter.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut tmp = [0u8; 8192];
                let mut header_end = None;
                let mut content_length = 0usize;
                loop {
                    let n = match stream.read(&mut tmp).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    buf.extend_from_slice(&tmp[..n]);
                    if header_end.is_none() {
                        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            header_end = Some(pos + 4);
                            let headers = String::from_utf8_lossy(&buf[..pos]).to_lowercase();
                            content_length = headers.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse().ok()).unwrap_or(0);
                        }
                    }
                    if let Some(h) = header_end {
                        if buf.len() >= h + content_length {
                            break;
                        }
                    }
                }
                let head = String::from_utf8_lossy(&buf[..header_end.unwrap_or(buf.len())]).to_string();
                let authorized = head.to_lowercase().contains("authorization: bearer test-key");
                let body: Value = header_end.map(|h| serde_json::from_slice(&buf[h..]).unwrap_or(Value::Null)).unwrap_or(Value::Null);
                counter.fetch_add(1, Ordering::SeqCst);
                let (status, content_type, payload) = respond(&head, &body, authorized, quirks);
                let _ = stream
                    .write_all(format!("HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}", payload.len()).as_bytes())
                    .await;
                let _ = stream.shutdown().await;
            });
        }
    });
    (format!("http://{addr}/v1"), requests)
}

fn respond(head: &str, body: &Value, authorized: bool, quirks: Quirks) -> (&'static str, &'static str, String) {
    if !authorized {
        return ("401 Unauthorized", "application/json", json!({"error": {"message": "invalid api key"}}).to_string());
    }
    if head.starts_with("GET /v1/models") {
        return ("200 OK", "application/json", json!({"data": [{"id": "fake-model"}, {"id": "other"}]}).to_string());
    }
    if quirks.needs_max_completion_tokens && body.get("max_tokens").is_some() {
        return ("400 Bad Request", "application/json", json!({"error": {"message": "Unsupported parameter: 'max_tokens' is not supported with this model. Use 'max_completion_tokens' instead.", "code": "unsupported_parameter"}}).to_string());
    }
    if quirks.rejects_stream_options && body.get("stream_options").is_some() {
        return ("400 Bad Request", "application/json", json!({"error": {"message": "Unrecognized request argument supplied: stream_options"}}).to_string());
    }
    let messages = body["messages"].as_array().cloned().unwrap_or_default();
    let last_user = messages.iter().rev().find(|m| m["role"] == "user").and_then(|m| m["content"].as_str()).unwrap_or("").to_string();
    let has_tool_result = messages.iter().any(|m| m["role"] == "tool");
    let tools_offered = body.get("tools").map(|t| !t.as_array().map(|a| a.is_empty()).unwrap_or(true)).unwrap_or(false);
    let wants_tool = tools_offered && !quirks.no_tools && !has_tool_result && last_user.contains("echo_number");
    let two = last_user.contains("twice");
    let stream = body["stream"].as_bool().unwrap_or(false);
    let tool_calls: Vec<Value> = if wants_tool {
        let values = if two { vec![1, 2] } else { vec![7] };
        values.iter().enumerate().map(|(i, v)| json!({"id": format!("call_{i}"), "type": "function", "function": {"name": "echo_number", "arguments": format!("{{\"value\": {v}}}")}})).collect()
    } else {
        vec![]
    };
    let text = if has_tool_result { "7" } else { "OK" };
    let usage = json!({"prompt_tokens": 42, "completion_tokens": 3, "prompt_tokens_details": {"cached_tokens": 10}});
    if stream {
        let mut chunks = Vec::new();
        if tool_calls.is_empty() {
            for piece in ["O", "K"] {
                chunks.push(json!({"id": "s1", "choices": [{"index": 0, "delta": {"content": piece}, "finish_reason": null}]}));
            }
        } else {
            for (i, call) in tool_calls.iter().enumerate() {
                chunks.push(json!({"id": "s1", "choices": [{"index": 0, "delta": {"tool_calls": [{"index": i, "id": call["id"], "type": "function", "function": {"name": "echo_number", "arguments": ""}}]}, "finish_reason": null}]}));
                chunks.push(json!({"id": "s1", "choices": [{"index": 0, "delta": {"tool_calls": [{"index": i, "function": {"arguments": call["function"]["arguments"]}}]}, "finish_reason": null}]}));
            }
        }
        chunks.push(json!({"id": "s1", "choices": [{"index": 0, "delta": {}, "finish_reason": if tool_calls.is_empty() { "stop" } else { "tool_calls" }}]}));
        if body.get("stream_options").is_some() {
            chunks.push(json!({"id": "s1", "choices": [], "usage": usage}));
        }
        let mut payload: String = chunks.iter().map(|c| format!("data: {c}\n\n")).collect();
        payload.push_str("data: [DONE]\n\n");
        return ("200 OK", "text/event-stream", payload);
    }
    let message = if tool_calls.is_empty() { json!({"role": "assistant", "content": text}) } else { json!({"role": "assistant", "content": null, "tool_calls": tool_calls}) };
    ("200 OK", "application/json", json!({"id": "r1", "choices": [{"index": 0, "message": message, "finish_reason": if wants_tool { "tool_calls" } else { "stop" }}], "usage": usage}).to_string())
}

fn config(base_url: &str) -> ModelConfig {
    std::env::set_var("PROBE_TEST_KEY", "test-key");
    ModelConfig { name: "fake-model".into(), base_url: base_url.to_string(), api_key_env: "PROBE_TEST_KEY".into(), request_timeout_secs: 10, max_retries: 1, ..Default::default() }
}

#[tokio::test]
async fn probe_reports_a_fully_capable_endpoint() {
    let (url, requests) = fake_server(Quirks::default()).await;
    let provider = OpenAiCompatibleProvider::new(config(&url)).unwrap();
    let profile = provider.probe(&CancellationToken::new()).await.unwrap();
    assert!(profile.is_agent_capable(), "{profile:#?}");
    assert_eq!(profile.model_listed, ProbeStatus::Supported);
    assert_eq!(profile.tool_calling, ProbeStatus::Supported);
    assert_eq!(profile.tool_result_continuation, ProbeStatus::Supported);
    assert_eq!(profile.parallel_tool_calls, ProbeStatus::Supported);
    assert_eq!(profile.streaming, ProbeStatus::Supported);
    assert_eq!(profile.stream_usage, ProbeStatus::Supported);
    assert_eq!(profile.usage_reporting, ProbeStatus::Supported);
    assert_eq!(profile.cached_token_reporting, ProbeStatus::Supported);
    assert_eq!(profile.max_tokens_field, "max_tokens");
    assert!(requests.load(Ordering::SeqCst) >= 6);
    let caps = provider.capabilities();
    assert!(caps.streaming && caps.tool_calling && caps.parallel_tool_calls && caps.prompt_caching);

    // A real streamed completion through the same transport.
    let (tx, mut rx) = tokio::sync::mpsc::channel(32);
    let req = ModelRequest::new(vec![AgentMessage::user("Reply with the single word OK.")], vec![]);
    let result = provider.complete(&req, Some(tx), &CancellationToken::new()).await.unwrap();
    assert_eq!(result.text.as_deref(), Some("OK"));
    assert_eq!(result.usage.unwrap().cached_prompt_tokens, Some(10));
    let mut deltas = 0;
    while let Ok(ev) = rx.try_recv() {
        if matches!(ev, ctx_core::StreamEvent::TextDelta { .. }) {
            deltas += 1;
        }
    }
    assert_eq!(deltas, 2);
}

#[tokio::test]
async fn probe_adapts_to_quirky_servers() {
    let (url, _) = fake_server(Quirks { needs_max_completion_tokens: true, rejects_stream_options: true, no_tools: false }).await;
    let provider = OpenAiCompatibleProvider::new(config(&url)).unwrap();
    let profile = provider.probe(&CancellationToken::new()).await.unwrap();
    assert!(profile.is_agent_capable(), "{profile:#?}");
    assert_eq!(profile.max_tokens_field, "max_completion_tokens");
    assert_eq!(profile.streaming, ProbeStatus::Supported);
    assert!(matches!(profile.stream_usage, ProbeStatus::Unsupported { .. }), "{:?}", profile.stream_usage);
    assert!(profile.notes.iter().any(|n| n.contains("max_completion_tokens")));
    // Subsequent requests use the adapted field and skip stream_options.
    let req = ModelRequest::new(vec![AgentMessage::user("hi")], vec![ToolDefinition { name: "t".into(), description: "d".into(), parameters: json!({"type": "object"}) }]);
    let result = provider.complete(&req, None, &CancellationToken::new()).await.unwrap();
    assert_eq!(result.text.as_deref(), Some("OK"));
}

#[tokio::test]
async fn probe_flags_chat_only_models_and_bad_keys() {
    let (url, _) = fake_server(Quirks { no_tools: true, ..Default::default() }).await;
    let provider = OpenAiCompatibleProvider::new(config(&url)).unwrap();
    let profile = provider.probe(&CancellationToken::new()).await.unwrap();
    assert!(!profile.is_agent_capable());
    assert!(matches!(profile.tool_calling, ProbeStatus::Unsupported { .. }), "{:?}", profile.tool_calling);
    assert!(profile.blocking_issues().iter().any(|i| i.contains("tool calling")));

    std::env::set_var("PROBE_BAD_KEY", "wrong");
    let mut cfg = config(&url);
    cfg.api_key_env = "PROBE_BAD_KEY".into();
    let provider = OpenAiCompatibleProvider::new(cfg).unwrap();
    let profile = provider.probe(&CancellationToken::new()).await.unwrap();
    assert!(matches!(profile.chat_completion, ProbeStatus::Failed { .. }));
    assert!(matches!(profile.connectivity, ProbeStatus::Failed { .. }), "{:?}", profile.connectivity);
}

#[tokio::test]
async fn list_models_returns_sorted_ids_and_surfaces_auth_errors() {
    let (url, _) = fake_server(Quirks::default()).await;
    let provider = OpenAiCompatibleProvider::new(config(&url)).unwrap();
    assert_eq!(provider.list_models().await.unwrap(), vec!["fake-model".to_string(), "other".to_string()]);

    std::env::set_var("PROBE_BAD_KEY", "wrong");
    let mut cfg = config(&url);
    cfg.api_key_env = "PROBE_BAD_KEY".into();
    let err = OpenAiCompatibleProvider::new(cfg).unwrap().list_models().await.unwrap_err();
    assert!(err.to_string().contains("401"), "{err}");
}

/// Answers every request with `response` (a raw HTTP response head and body).
async fn canned_server(response: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf).await;
            let _ = stream.write_all(response.as_bytes()).await;
            let _ = stream.shutdown().await;
        }
    });
    format!("http://{addr}/v1")
}

#[tokio::test]
async fn gateway_status_reads_the_update_mark() {
    use ctx_providers::GatewayStatus;
    let cancel = CancellationToken::new();
    let gateway = |url: &str| {
        let cfg = ModelConfig { credential: Some("modelgateway".into()), ..config(url) };
        OpenAiCompatibleProvider::with_api_key(cfg, Some("test-key".into())).unwrap()
    };
    let updating = canned_server("HTTP/1.1 503 Service Unavailable\r\nX-ModelGateway-Status: updating\r\nRetry-After: 5\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
    assert_eq!(gateway(&updating).gateway_status(&cancel).await, Some(GatewayStatus::Updating));
    let online = canned_server("HTTP/1.1 200 OK\r\nContent-Length: 16\r\nConnection: close\r\n\r\n{\"success\":true}").await;
    assert_eq!(gateway(&online).gateway_status(&cancel).await, Some(GatewayStatus::Online));
    let down = canned_server("HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
    assert_eq!(gateway(&down).gateway_status(&cancel).await, Some(GatewayStatus::Unreachable));
    // Any other endpoint reports nothing.
    let other = OpenAiCompatibleProvider::new(config(&online)).unwrap();
    assert_eq!(other.gateway_status(&cancel).await, None);
}
