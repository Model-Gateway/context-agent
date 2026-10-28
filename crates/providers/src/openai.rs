//! OpenAI-compatible Chat Completions transport (spec §5.2–§5.8).

use crate::capability::{CapabilityProfile, EffectiveCapabilities, ProbeStatus};
use crate::provider::{GatewayStatus, ModelProvider, StreamSink};
use crate::retry::RetryPolicy;
use crate::sse::{SseEvent, SseParser};
use async_trait::async_trait;
use ctx_core::{
    AgentMessage, CancellationToken, Config, CtxError, FinishReason, ModelCapabilities, ModelConfig, ModelRequest, ModelResult,
    ProviderError, ProviderMetadata, Result, StreamEvent, TokenUsage, ToolCall, ToolChoice, ToolDefinition, UsageSource,
};
use futures::StreamExt;
use parking_lot::RwLock;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::time::Duration;

pub const PROVIDER_NAME: &str = "openai_compatible";
/// Set by ModelGateway (new-api) on every reply.
const GATEWAY_REQUEST_ID_HEADER: &str = "x-oneapi-request-id";
/// ModelGateway's opt-in guard against much worse discounts (see
/// `ModelRequest::max_discount_drop`) and the two discounts it reports
/// when it refuses.
const DISCOUNT_DROP_HEADER: &str = "x-modelgateway-max-discount-drop";
const EXPECTED_DISCOUNT_HEADER: &str = "x-modelgateway-expected-discount";
const AVAILABLE_DISCOUNT_HEADER: &str = "x-modelgateway-available-discount";
/// ModelGateway's proxy marks the 503 it answers with while the gateway is
/// being updated with `updating` in this header.
const GATEWAY_STATUS_HEADER: &str = "x-modelgateway-status";

pub struct OpenAiCompatibleProvider {
    client: reqwest::Client,
    config: ModelConfig,
    api_key: Option<String>,
    retry: RetryPolicy,
    configured: ModelCapabilities,
    profile: RwLock<CapabilityProfile>,
}

/// Accumulates streamed tool-call fragments by index.
#[derive(Default, Debug)]
struct PartialToolCall {
    id: String,
    name: String,
    arguments: String,
}

impl OpenAiCompatibleProvider {
    pub fn from_config(config: &Config) -> Result<Self> {
        Self::new(config.model.clone())
    }

    /// A provider whose key comes from `config.api_key_env`. Callers with a
    /// `Config` use `ctx_providers::build_provider`, which also honours
    /// `model.credential`.
    pub fn new(config: ModelConfig) -> Result<Self> {
        let api_key = config.api_key();
        Self::with_api_key(config, api_key)
    }

    /// A provider authenticating with `api_key` (none sends no Authorization header).
    pub fn with_api_key(config: ModelConfig, api_key: Option<String>) -> Result<Self> {
        let mut headers = reqwest::header::HeaderMap::new();
        for (k, v) in &config.extra_headers {
            let name = reqwest::header::HeaderName::from_bytes(k.as_bytes()).map_err(|e| CtxError::Config(format!("invalid header {k}: {e}")))?;
            let value = reqwest::header::HeaderValue::from_str(v).map_err(|e| CtxError::Config(format!("invalid header value for {k}: {e}")))?;
            headers.insert(name, value);
        }
        // The request timeout bounds non-streaming requests as a whole; for
        // streams it is applied as an idle (read) timeout so a long but
        // healthy stream is never cut off.
        let client = reqwest::Client::builder()
            .read_timeout(Duration::from_secs(config.request_timeout_secs.max(5)))
            .connect_timeout(Duration::from_secs(20))
            .default_headers(headers)
            .user_agent(concat!("context-agent/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| CtxError::Config(format!("failed to build http client: {e}")))?;
        let api_key = api_key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty());
        let retry = RetryPolicy::from_model(&config);
        let profile = CapabilityProfile::new(&config.fingerprint(), &config.base_url, &config.name);
        Ok(Self { client, configured: config.capabilities.clone(), config, api_key, retry, profile: RwLock::new(profile) })
    }

    pub fn config(&self) -> &ModelConfig {
        &self.config
    }

    pub fn profile(&self) -> CapabilityProfile {
        self.profile.read().clone()
    }

    /// Model identifiers from the endpoint's `/models` listing, sorted.
    pub async fn list_models(&self) -> Result<Vec<String>> {
        let resp = self.authorized(self.client.get(self.endpoint("models"))).send().await.map_err(|e| CtxError::other(format!("GET /models failed: {e}")))?;
        if !resp.status().is_success() {
            return Err(CtxError::other(format!("/models returned HTTP {}", resp.status())));
        }
        let body: Value = resp.json().await.map_err(|e| CtxError::other(format!("unreadable /models response: {e}")))?;
        let mut ids: Vec<String> = body.get("data").and_then(|d| d.as_array()).map(|arr| arr.iter().filter_map(|m| m.get("id").and_then(|i| i.as_str()).map(|s| s.to_string())).collect()).unwrap_or_default();
        ids.sort();
        ids.dedup();
        Ok(ids)
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}/{}", self.config.base_url.trim_end_matches('/'), path.trim_start_matches('/'))
    }

    /// Whether this endpoint is ModelGateway: signed in with its stored
    /// credential, or a modelgateway.co host.
    fn is_modelgateway(&self) -> bool {
        if self.signed_in_to_modelgateway() {
            return true;
        }
        let host = self.config.base_url.split("://").nth(1).unwrap_or("").split(['/', ':']).next().unwrap_or("").to_ascii_lowercase();
        host == "modelgateway.co" || host.ends_with(".modelgateway.co")
    }

    /// Whether requests use the ModelGateway credential from sign-in. The
    /// gateway's discount features (the lower-discount guard) are only for
    /// signed-in users, not a modelgateway host used with your own key.
    fn signed_in_to_modelgateway(&self) -> bool {
        self.config.credential.as_deref().map(str::trim) == Some("modelgateway")
    }

    fn authorized(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.api_key {
            Some(key) => req.bearer_auth(key),
            None => req,
        }
    }

    // ----- request translation -------------------------------------------

    /// Translate internal messages to chat-completions messages while
    /// preserving trust boundaries: only policy/task instructions become
    /// system (or developer) messages; context payloads are user-role data.
    pub fn translate_messages(messages: &[AgentMessage], caps: &EffectiveCapabilities) -> Vec<Value> {
        let mut out = Vec::with_capacity(messages.len());
        for m in messages {
            match m {
                AgentMessage::Policy(p) => out.push(json!({"role": "system", "content": p.text})),
                AgentMessage::TaskInstructions(t) => {
                    let role = if caps.developer_role { "developer" } else { "system" };
                    out.push(json!({"role": role, "content": t.text}));
                }
                AgentMessage::Context(c) => {
                    if !c.is_empty() {
                        out.push(json!({"role": "user", "content": c.render()}));
                    }
                }
                AgentMessage::User(u) if u.images.is_empty() => out.push(json!({"role": "user", "content": u.text_with_files()})),
                AgentMessage::User(u) => {
                    let mut parts = Vec::with_capacity(u.images.len() + 1);
                    let text = u.text_with_files();
                    if !text.is_empty() {
                        parts.push(json!({"type": "text", "text": text}));
                    }
                    parts.extend(u.images.iter().map(|image| json!({"type": "image_url", "image_url": {"url": image.data_url()}})));
                    out.push(json!({"role": "user", "content": parts}));
                }
                // A turn with neither text nor tool calls (a reply cut off
                // while reasoning) carries nothing, and strict servers reject
                // an assistant message without content.
                AgentMessage::Assistant(a) if a.tool_calls.is_empty() && a.text.as_deref().is_none_or(str::is_empty) => {}
                AgentMessage::Assistant(a) => {
                    let mut msg = json!({"role": "assistant"});
                    msg["content"] = match &a.text {
                        Some(t) if !t.is_empty() => Value::String(t.clone()),
                        _ => Value::Null,
                    };
                    if !a.tool_calls.is_empty() {
                        msg["tool_calls"] = Value::Array(
                            a.tool_calls
                                .iter()
                                .map(|tc| {
                                    json!({
                                        "id": tc.id,
                                        "type": "function",
                                        "function": {"name": tc.name, "arguments": serialize_arguments(&tc.arguments)}
                                    })
                                })
                                .collect(),
                        );
                    }
                    out.push(msg);
                }
                AgentMessage::ToolResult(r) => {
                    out.push(json!({"role": "tool", "tool_call_id": r.call_id, "content": r.content}));
                }
            }
        }
        out
    }

    fn translate_tools(tools: &[ToolDefinition]) -> Vec<Value> {
        tools
            .iter()
            .map(|t| json!({"type": "function", "function": {"name": t.name, "description": t.description, "parameters": t.parameters}}))
            .collect()
    }

    /// The `reasoning_effort` sent with `request`: the request decides (the
    /// runtime always sets the effort in force). A model the effort
    /// catalogue lists values for gets the chosen one, `ultra` its
    /// strongest; any other model only with the configured opt-in, `ultra`
    /// going out as `high`.
    fn reasoning_effort_value(&self, request: &ModelRequest, caps: &EffectiveCapabilities) -> Option<String> {
        let effort = request.reasoning_effort.as_ref()?;
        let listed = caps.catalog_effort.then(|| ctx_core::effort_levels(&self.config.name, &self.config.base_url)).flatten();
        listed.and_then(|levels| effort.wire_value_for(&levels)).or_else(|| caps.reasoning_controls.then(|| effort.wire_value().to_string()))
    }

    /// Whether the catalogue lists effort values for this model.
    fn catalog_lists_effort(&self) -> bool {
        ctx_core::effort_levels(&self.config.name, &self.config.base_url).is_some_and(|levels| !levels.is_empty())
    }

    pub fn build_body(&self, request: &ModelRequest, stream: bool, caps: &EffectiveCapabilities) -> Value {
        let mut body = json!({
            "model": self.config.name,
            "messages": Self::translate_messages(&request.messages, caps),
        });
        if caps.tool_calling && !request.tools.is_empty() {
            body["tools"] = Value::Array(Self::translate_tools(&request.tools));
            body["tool_choice"] = match request.tool_choice {
                ToolChoice::Auto => json!("auto"),
                ToolChoice::None => json!("none"),
                ToolChoice::Required => json!("required"),
            };
            if let Some(parallel) = request.parallel_tool_calls {
                if caps.parallel_tool_calls {
                    body["parallel_tool_calls"] = json!(parallel);
                }
            }
        }
        let max_out = request.max_output_tokens.unwrap_or(self.config.max_output_tokens);
        body[caps.max_tokens_field.as_str()] = json!(max_out);
        if let Some(t) = request.temperature.or(self.config.temperature) {
            body["temperature"] = json!(t);
        }
        if let Some(effort) = self.reasoning_effort_value(request, caps) {
            body["reasoning_effort"] = json!(effort);
        }
        if caps.prompt_caching {
            if let Some(key) = &request.cache.prompt_cache_key {
                body["prompt_cache_key"] = json!(key);
            }
        }
        if stream {
            body["stream"] = json!(true);
            if caps.stream_usage {
                body["stream_options"] = json!({"include_usage": true});
            }
        }
        body
    }

    // ----- response parsing ----------------------------------------------

    fn parse_usage(usage: &Value) -> Option<TokenUsage> {
        let prompt = usage.get("prompt_tokens")?.as_u64()?;
        let completion = usage.get("completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
        let cached = usage.pointer("/prompt_tokens_details/cached_tokens").and_then(|v| v.as_u64());
        let reasoning = usage.pointer("/completion_tokens_details/reasoning_tokens").and_then(|v| v.as_u64());
        Some(TokenUsage { prompt_tokens: prompt, completion_tokens: completion, cached_prompt_tokens: cached, reasoning_tokens: reasoning, source: UsageSource::Reported })
    }

    fn parse_finish(reason: Option<&str>, has_tool_calls: bool) -> FinishReason {
        match reason {
            Some("stop") | Some("end_turn") => {
                if has_tool_calls {
                    FinishReason::ToolCalls
                } else {
                    FinishReason::Stop
                }
            }
            Some("tool_calls") | Some("function_call") => FinishReason::ToolCalls,
            Some("length") | Some("max_tokens") => FinishReason::Length,
            Some("content_filter") => FinishReason::ContentFilter,
            None if has_tool_calls => FinishReason::ToolCalls,
            None => FinishReason::Unknown,
            Some(_) => FinishReason::Unknown,
        }
    }

    fn metadata(&self, id: Option<&str>, extra: BTreeMap<String, Value>) -> ProviderMetadata {
        ProviderMetadata { provider: PROVIDER_NAME.into(), model: self.config.name.clone(), request_id: id.map(|s| s.to_string()), extra }
    }

    fn parse_non_streaming(&self, body: &Value) -> Result<ModelResult> {
        let choice = body
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|c| c.first())
            .ok_or_else(|| ProviderError::InvalidResponse("response has no choices".into()))?;
        let message = choice.get("message").ok_or_else(|| ProviderError::InvalidResponse("choice has no message".into()))?;
        let text = message.get("content").and_then(|c| c.as_str()).map(|s| s.to_string()).filter(|s| !s.is_empty());
        let mut tool_calls = Vec::new();
        if let Some(calls) = message.get("tool_calls").and_then(|c| c.as_array()) {
            for (i, call) in calls.iter().enumerate() {
                let id = call.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()).unwrap_or_else(|| format!("call_{i}"));
                let name = call.pointer("/function/name").and_then(|v| v.as_str()).unwrap_or_default().to_string();
                let raw_args = call.pointer("/function/arguments").cloned().unwrap_or(Value::String("{}".into()));
                tool_calls.push(ToolCall { id, name, arguments: parse_arguments(&raw_args) });
            }
        }
        let finish = Self::parse_finish(choice.get("finish_reason").and_then(|v| v.as_str()), !tool_calls.is_empty());
        let usage = body.get("usage").and_then(Self::parse_usage);
        let mut extra = BTreeMap::new();
        if let Some(fp) = body.get("system_fingerprint").filter(|v| !v.is_null()) {
            extra.insert("system_fingerprint".into(), fp.clone());
        }
        Ok(ModelResult { text, tool_calls, finish_reason: finish, usage, provider_metadata: self.metadata(body.get("id").and_then(|v| v.as_str()), extra) })
    }

    async fn map_error_response(status: reqwest::StatusCode, retry_after: Option<Duration>, body: String) -> ProviderError {
        let parsed: Option<Value> = serde_json::from_str(&body).ok();
        let message = parsed
            .as_ref()
            .and_then(|v| v.pointer("/error/message").or_else(|| v.get("message")).or_else(|| v.get("detail")))
            .and_then(|m| m.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| body.chars().take(500).collect());
        let code = parsed.as_ref().and_then(|v| v.pointer("/error/code")).and_then(|c| c.as_str()).map(|s| s.to_string());
        // A rate limit keeps its own variant for the Retry-After hint.
        if let Some(category) = Self::upstream_route_failure(parsed.as_ref().and_then(|v| v.pointer("/error/details"))).filter(|_| status.as_u16() != 429) {
            return ProviderError::UpstreamRoute { status: status.as_u16(), category, message };
        }
        let lower = message.to_ascii_lowercase();
        match status.as_u16() {
            401 | 403 => ProviderError::Auth(message),
            429 => ProviderError::RateLimited { retry_after },
            400 | 413 | 422
                if lower.contains("context length")
                    || lower.contains("context_length")
                    || lower.contains("maximum context")
                    || lower.contains("too many tokens")
                    || lower.contains("context window")
                    || lower.contains("prompt is too long")
                    || lower.contains("exceeds the limit")
                    || code.as_deref() == Some("context_length_exceeded") =>
            {
                ProviderError::ContextLength(message)
            }
            s => ProviderError::Api { status: s, code, message },
        }
    }

    /// ModelGateway's `error.details` for a failure on the provider route it
    /// picked: an exhausted provider balance or quota, rejected provider
    /// credentials or access, or anything it marks retryable. Another
    /// attempt may take another route, so these are retried rather than
    /// treated like the user's own billing or a bad request. Returns the
    /// category.
    fn upstream_route_failure(details: Option<&Value>) -> Option<String> {
        let details = details?;
        if details.get("source").and_then(|s| s.as_str()) != Some("upstream") {
            return None;
        }
        let category = details.get("category").and_then(|c| c.as_str()).unwrap_or("provider_error");
        let retryable = details.get("retryable").and_then(|r| r.as_bool()).unwrap_or(false);
        (retryable || matches!(category, "provider_balance" | "provider_quota" | "provider_authentication" | "provider_access")).then(|| category.to_string())
    }

    /// An error object inside a 200 response (a non-streaming body or an SSE
    /// chunk): the upstream failed after the request was accepted. Its
    /// `code` (an HTTP status or a name) or `type` classifies it like an
    /// error response would be; without either it stays `Api { status: 200 }`,
    /// which counts as a transient upstream failure.
    fn in_band_error(err: &Value, fallback: &str) -> ProviderError {
        let message = err.get("message").and_then(|m| m.as_str()).unwrap_or(fallback).to_string();
        let code = err.get("code").and_then(|c| c.as_str().map(str::to_string).or_else(|| c.as_u64().map(|n| n.to_string())));
        let status = code.as_deref().and_then(|c| c.parse::<u16>().ok()).filter(|s| (400..600).contains(s));
        if let Some(category) = Self::upstream_route_failure(err.get("details")).filter(|_| status != Some(429)) {
            return ProviderError::UpstreamRoute { status: status.unwrap_or(200), category, message };
        }
        let name =format!("{} {}", code.as_deref().unwrap_or(""), err.get("type").and_then(|t| t.as_str()).unwrap_or("")).to_ascii_lowercase();
        match status {
            Some(401 | 403) => ProviderError::Auth(message),
            Some(429) => ProviderError::RateLimited { retry_after: None },
            Some(s) => ProviderError::Api { status: s, code, message },
            None if name.contains("context_length") => ProviderError::ContextLength(message),
            None if name.contains("rate_limit") => ProviderError::RateLimited { retry_after: None },
            None if name.contains("invalid_api_key") || name.contains("authentication") || name.contains("permission") => ProviderError::Auth(message),
            None if name.contains("invalid_request") => ProviderError::Api { status: 400, code, message },
            None => ProviderError::Api { status: 200, code, message },
        }
    }

    async fn send(&self, body: &Value, max_discount_drop: Option<u32>, cancel: &CancellationToken) -> Result<reqwest::Response> {
        let streaming = body.get("stream").and_then(|s| s.as_bool()).unwrap_or(false);
        let mut req = self.authorized(self.client.post(self.endpoint("chat/completions")).json(body));
        if !streaming {
            req = req.timeout(Duration::from_secs(self.config.request_timeout_secs.max(5)));
        }
        if let Some(points) = max_discount_drop.filter(|_| self.signed_in_to_modelgateway()) {
            req = req.header(DISCOUNT_DROP_HEADER, points.to_string());
        }
        let response = tokio::select! {
            _ = cancel.cancelled() => return Err(CtxError::Cancelled),
            r = req.send() => r,
        };
        let response = response.map_err(|e| {
            if e.is_timeout() {
                ProviderError::Timeout
            } else if e.is_connect() {
                ProviderError::Connection(e.to_string())
            } else {
                ProviderError::Connection(e.to_string())
            }
        })?;
        let status = response.status();
        if !status.is_success() {
            let retry_after = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .map(Duration::from_secs);
            let discount = |name: &str| response.headers().get(name).and_then(|v| v.to_str().ok()).and_then(|s| s.trim().parse::<f64>().ok());
            let (expected, available) = (discount(EXPECTED_DISCOUNT_HEADER), discount(AVAILABLE_DISCOUNT_HEADER));
            let text = response.text().await.unwrap_or_default();
            return Err(match Self::map_error_response(status, retry_after, text).await {
                ProviderError::Api { status: 412, code: Some(code), message } if code == "discount_drop" => {
                    let message = match (expected, available) {
                        (Some(e), Some(a)) => format!("{} is {e}% off at best, but the providers left are {a}% off", self.config.name),
                        _ => message,
                    };
                    ProviderError::DiscountDrop { expected, available, message }
                }
                other => other,
            }
            .into());
        }
        Ok(response)
    }

    async fn complete_once(&self, request: &ModelRequest, stream: Option<&StreamSink>, caps: &EffectiveCapabilities, cancel: &CancellationToken) -> Result<ModelResult> {
        let use_stream = stream.is_some() && caps.streaming;
        let body = self.build_body(request, use_stream, caps);
        tracing::debug!(model = %self.config.name, stream = use_stream, messages = request.messages.len(), tools = request.tools.len(), "sending chat completion");
        let response = self.send(&body, request.max_discount_drop, cancel).await?;
        // ModelGateway's id for the request, to look up what it billed.
        let gateway_id = response.headers().get(GATEWAY_REQUEST_ID_HEADER).and_then(|v| v.to_str().ok()).map(str::to_string);
        let mut result = if use_stream {
            self.consume_stream(response, stream.unwrap(), cancel).await
        } else {
            let text = tokio::select! {
                _ = cancel.cancelled() => return Err(CtxError::Cancelled),
                t = response.text() => t.map_err(|e| ProviderError::Connection(e.to_string()))?,
            };
            let value: Value = serde_json::from_str(&text).map_err(|e| ProviderError::InvalidResponse(format!("invalid JSON: {e}")))?;
            if let Some(err) = value.get("error").filter(|e| !e.is_null()) {
                return Err(Self::in_band_error(err, "unknown provider error").into());
            }
            self.parse_non_streaming(&value)
        }?;
        if let Some(id) = gateway_id.filter(|id| !id.is_empty()) {
            result.provider_metadata.extra.insert(ctx_core::pricing::GATEWAY_REQUEST_ID.into(), Value::String(id));
        }
        Ok(result)
    }

    async fn consume_stream(&self, response: reqwest::Response, sink: &StreamSink, cancel: &CancellationToken) -> Result<ModelResult> {
        let mut parser = SseParser::new();
        let mut byte_stream = response.bytes_stream();
        let mut text = String::new();
        let mut partial_calls: BTreeMap<usize, PartialToolCall> = BTreeMap::new();
        let mut finish: Option<FinishReason> = None;
        let mut raw_finish: Option<String> = None;
        let mut usage: Option<TokenUsage> = None;
        let mut response_id: Option<String> = None;
        let mut done = false;
        // Bytes of an incomplete UTF-8 sequence carried to the next chunk.
        let mut pending: Vec<u8> = Vec::new();

        loop {
            let next = tokio::select! {
                _ = cancel.cancelled() => {
                    let _ = sink.send(StreamEvent::Finished { reason: FinishReason::Cancelled }).await;
                    return Err(CtxError::Cancelled);
                }
                chunk = byte_stream.next() => chunk,
            };
            let Some(chunk) = next else { break };
            let bytes = match chunk {
                Ok(bytes) => bytes,
                // Broken off after content arrived: keep it and finish as
                // interrupted so the runtime resumes from there. Before any
                // content the request is simply retried.
                Err(e) if !text.is_empty() || !partial_calls.is_empty() => {
                    tracing::warn!(error = %e, "response stream broke off; keeping the partial response");
                    break;
                }
                Err(e) => return Err(ProviderError::InterruptedStream(e.to_string()).into()),
            };
            // Decode only complete UTF-8 sequences; carry a split multi-byte
            // character over to the next chunk instead of corrupting it.
            pending.extend_from_slice(&bytes);
            let valid_up_to = match std::str::from_utf8(&pending) {
                Ok(_) => pending.len(),
                Err(e) => e.valid_up_to(),
            };
            let decoded: String = String::from_utf8_lossy(&pending[..valid_up_to]).into_owned();
            pending.drain(..valid_up_to);
            if pending.len() > 4 {
                // Not a split character but genuinely invalid bytes: drop them.
                pending.clear();
            }
            let events = parser.feed(&decoded);
            for ev in events {
                match ev {
                    SseEvent::Done => {
                        done = true;
                    }
                    SseEvent::Data(data) => {
                        let value: Value = match serde_json::from_str(&data) {
                            Ok(v) => v,
                            Err(e) => {
                                tracing::warn!(error = %e, "skipping malformed SSE chunk");
                                continue;
                            }
                        };
                        if let Some(err) = value.get("error").filter(|e| !e.is_null()) {
                            return Err(Self::in_band_error(err, "stream error").into());
                        }
                        if response_id.is_none() {
                            response_id = value.get("id").and_then(|v| v.as_str()).map(|s| s.to_string());
                        }
                        if let Some(u) = value.get("usage").filter(|u| !u.is_null()).and_then(Self::parse_usage) {
                            usage = Some(u);
                            let _ = sink.send(StreamEvent::Usage { usage: u }).await;
                        }
                        let Some(choice) = value.get("choices").and_then(|c| c.as_array()).and_then(|c| c.first()) else { continue };
                        if let Some(reason) = choice.get("finish_reason").and_then(|v| v.as_str()) {
                            raw_finish = Some(reason.to_string());
                        }
                        let Some(delta) = choice.get("delta") else { continue };
                        // DeepSeek/Kimi-style servers use `reasoning_content`,
                        // OpenRouter-style gateways `reasoning`.
                        if let Some(reasoning) = delta.get("reasoning_content").or_else(|| delta.get("reasoning")).and_then(|c| c.as_str()) {
                            if !reasoning.is_empty() {
                                let _ = sink.send(StreamEvent::ReasoningDelta { text: reasoning.to_string() }).await;
                            }
                        }
                        if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                            if !content.is_empty() {
                                text.push_str(content);
                                let _ = sink.send(StreamEvent::TextDelta { text: content.to_string() }).await;
                            }
                        }
                        if let Some(calls) = delta.get("tool_calls").and_then(|c| c.as_array()) {
                            for call in calls {
                                // Servers that omit `index` send fragments for the
                                // current call; a fragment with a new id starts one.
                                let index = match call.get("index").and_then(|i| i.as_u64()) {
                                    Some(i) => i as usize,
                                    None if call.get("id").and_then(|v| v.as_str()).map(|id| !id.is_empty()).unwrap_or(false) && partial_calls.values().any(|p| !p.id.is_empty()) => partial_calls.len(),
                                    None => partial_calls.keys().next_back().copied().unwrap_or(0),
                                };
                                let entry = partial_calls.entry(index).or_default();
                                let mut started = false;
                                if let Some(id) = call.get("id").and_then(|v| v.as_str()) {
                                    if entry.id.is_empty() {
                                        entry.id = id.to_string();
                                        started = true;
                                    }
                                }
                                if let Some(name) = call.pointer("/function/name").and_then(|v| v.as_str()) {
                                    if entry.name.is_empty() {
                                        entry.name = name.to_string();
                                        started = true;
                                    }
                                }
                                if started && !entry.name.is_empty() {
                                    let _ = sink
                                        .send(StreamEvent::ToolCallStarted { index, id: entry.id.clone(), name: entry.name.clone() })
                                        .await;
                                }
                                if let Some(args) = call.pointer("/function/arguments").and_then(|v| v.as_str()) {
                                    if !args.is_empty() {
                                        entry.arguments.push_str(args);
                                        let _ = sink.send(StreamEvent::ToolCallArgumentsDelta { index, fragment: args.to_string() }).await;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if done {
                break;
            }
        }
        if let Some(SseEvent::Data(data)) = parser.finish() {
            if let Ok(value) = serde_json::from_str::<Value>(&data) {
                if let Some(u) = value.get("usage").and_then(Self::parse_usage) {
                    usage = Some(u);
                }
            }
        }
        if !done && raw_finish.is_none() {
            if text.is_empty() && partial_calls.is_empty() {
                return Err(ProviderError::InterruptedStream("stream ended before any content or [DONE]".into()).into());
            }
            finish = Some(FinishReason::Interrupted);
        }
        let finish = finish.unwrap_or_else(|| Self::parse_finish(raw_finish.as_deref(), !partial_calls.is_empty()));
        self.finalize_stream(text, partial_calls, Some(finish), usage, response_id, sink).await
    }

    async fn finalize_stream(
        &self,
        text: String,
        partial_calls: BTreeMap<usize, PartialToolCall>,
        finish: Option<FinishReason>,
        usage: Option<TokenUsage>,
        response_id: Option<String>,
        sink: &StreamSink,
    ) -> Result<ModelResult> {
        let finish = finish.unwrap_or(FinishReason::Unknown);
        let mut tool_calls = Vec::new();
        for (index, partial) in partial_calls {
            // A stream that broke off mid-arguments leaves that call
            // incomplete; drop it, the model makes it again on resuming.
            if finish == FinishReason::Interrupted && !serde_json::from_str::<Value>(partial.arguments.trim()).is_ok_and(|v| v.is_object()) {
                continue;
            }
            let id = if partial.id.is_empty() { format!("call_{index}") } else { partial.id };
            let raw = Value::String(if partial.arguments.trim().is_empty() { "{}".into() } else { partial.arguments });
            tool_calls.push(ToolCall { id, name: partial.name, arguments: parse_arguments(&raw) });
        }
        // A model that was cut off mid-arguments produces malformed JSON; the
        // tool layer reports that back to the model instead of executing.
        let _ = sink.send(StreamEvent::Finished { reason: finish }).await;
        Ok(ModelResult {
            text: if text.is_empty() { None } else { Some(text) },
            tool_calls,
            finish_reason: finish,
            usage,
            provider_metadata: self.metadata(response_id.as_deref(), BTreeMap::new()),
        })
    }

    // ----- probing --------------------------------------------------------

    async fn probe_models(&self, cancel: &CancellationToken) -> (ProbeStatus, ProbeStatus) {
        let req = self.authorized(self.client.get(self.endpoint("models")));
        let resp = tokio::select! {
            _ = cancel.cancelled() => return (ProbeStatus::Failed { error: "cancelled".into() }, ProbeStatus::Unknown),
            r = req.send() => r,
        };
        match resp {
            Ok(r) if r.status().is_success() => {
                let listed = match r.json::<Value>().await {
                    Ok(v) => {
                        let ids: Vec<String> = v
                            .get("data")
                            .and_then(|d| d.as_array())
                            .map(|arr| arr.iter().filter_map(|m| m.get("id").and_then(|i| i.as_str()).map(|s| s.to_string())).collect())
                            .unwrap_or_default();
                        if ids.is_empty() {
                            ProbeStatus::Unknown
                        } else if ids.iter().any(|id| id == &self.config.name) {
                            ProbeStatus::Supported
                        } else {
                            ProbeStatus::Unsupported { reason: format!("model `{}` not in /models listing ({} models listed)", self.config.name, ids.len()) }
                        }
                    }
                    Err(_) => ProbeStatus::Unknown,
                };
                (ProbeStatus::Supported, listed)
            }
            Ok(r) if r.status().as_u16() == 401 || r.status().as_u16() == 403 => {
                (ProbeStatus::Failed { error: format!("authentication rejected (HTTP {})", r.status()) }, ProbeStatus::Unknown)
            }
            Ok(r) => (ProbeStatus::Supported, ProbeStatus::Unsupported { reason: format!("/models returned HTTP {}", r.status()) }),
            Err(e) => (ProbeStatus::Failed { error: e.to_string() }, ProbeStatus::Unknown),
        }
    }

    fn probe_tool() -> ToolDefinition {
        ToolDefinition {
            name: "echo_number".into(),
            description: "Echo the given number back. Call this tool exactly as instructed.".into(),
            parameters: json!({"type": "object", "properties": {"value": {"type": "integer", "description": "The number to echo"}}, "required": ["value"], "additionalProperties": false}),
        }
    }
}

fn serialize_arguments(args: &Value) -> String {
    match args {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Parse tool-call arguments. Providers send a JSON string; some send an
/// object directly. Malformed JSON is preserved under a sentinel key so the
/// tool layer can report it to the model (spec §5.6).
pub fn parse_arguments(raw: &Value) -> Value {
    match raw {
        Value::Object(_) => raw.clone(),
        Value::Null => json!({}),
        Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                return json!({});
            }
            match serde_json::from_str::<Value>(trimmed) {
                Ok(v @ Value::Object(_)) => v,
                Ok(other) => json!({"__malformed_arguments": other.to_string(), "__reason": "arguments must be a JSON object"}),
                Err(e) => {
                    // Common repair: concatenated objects from parallel calls
                    // merged into one stream, or trailing garbage.
                    if let Some(repaired) = repair_json_object(trimmed) {
                        return repaired;
                    }
                    json!({"__malformed_arguments": s, "__reason": e.to_string()})
                }
            }
        }
        other => json!({"__malformed_arguments": other.to_string(), "__reason": "unexpected argument type"}),
    }
}

fn repair_json_object(text: &str) -> Option<Value> {
    // Take the first balanced top-level object.
    let start = text.find('{')?;
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escape = false;
    for (i, c) in text[start..].char_indices() {
        if in_str {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    let candidate = &text[start..start + i + 1];
                    return serde_json::from_str::<Value>(candidate).ok().filter(|v| v.is_object());
                }
            }
            _ => {}
        }
    }
    None
}

#[async_trait]
impl ModelProvider for OpenAiCompatibleProvider {
    fn name(&self) -> &str {
        PROVIDER_NAME
    }

    fn model(&self) -> &str {
        &self.config.name
    }

    fn capabilities(&self) -> EffectiveCapabilities {
        self.profile.read().effective(&self.configured)
    }

    async fn complete(&self, request: &ModelRequest, stream: Option<StreamSink>, cancel: &CancellationToken) -> Result<ModelResult> {
        if self.api_key.is_none() && (!self.config.api_key_env.is_empty() || self.config.credential.is_some()) && !self.config.base_url.contains("localhost") && !self.config.base_url.contains("127.0.0.1") {
            tracing::warn!(source = %ctx_core::api_key_source(&self.config), "no API key found for the configured source");
        }
        let caps = self.capabilities();
        let stream_ref = stream.as_ref();
        let caps_ref = &caps;
        let result = self
            .retry
            .run(cancel, |attempt| async move {
                // Only stream on the first attempt; a retried stream would
                // duplicate deltas in the UI. Tell the UI that whatever was
                // streamed before the retry is superseded.
                let sink = if attempt == 0 {
                    stream_ref
                } else {
                    if let Some(s) = stream_ref {
                        let _ = s.send(StreamEvent::Finished { reason: FinishReason::Interrupted }).await;
                    }
                    None
                };
                self.complete_once(request, sink, caps_ref, cancel).await
            })
            .await;
        match result {
            Err(CtxError::Provider(ProviderError::Api { status: 400, message, .. })) if message.contains("max_completion_tokens") && caps.max_tokens_field == "max_tokens" => {
                // Adapt to providers that renamed the field.
                self.profile.write().max_tokens_field = "max_completion_tokens".into();
                let caps = self.capabilities();
                self.complete_once(request, stream_ref, &caps, cancel).await
            }
            Err(CtxError::Provider(ProviderError::Api { status: 400, message, .. })) if message.contains("reasoning") && self.reasoning_effort_value(request, &caps).is_some() => {
                // The endpoint rejected `reasoning_effort`: stop sending it.
                tracing::warn!(model = %self.config.name, "endpoint rejected reasoning_effort; sending requests without it");
                self.profile.write().reasoning_controls = ProbeStatus::Unsupported { reason: message };
                let caps = self.capabilities();
                self.complete_once(request, stream_ref, &caps, cancel).await
            }
            other => other,
        }
    }

    async fn probe(&self, cancel: &CancellationToken) -> Result<CapabilityProfile> {
        let mut profile = CapabilityProfile::new(&self.config.fingerprint(), &self.config.base_url, &self.config.name);
        profile.probed_at = Some(ctx_core::now());
        let (connectivity, listed) = self.probe_models(cancel).await;
        profile.connectivity = connectivity;
        profile.model_listed = listed;
        let mut caps = EffectiveCapabilities { streaming: false, prompt_caching: false, ..Default::default() };

        // 1. Basic completion (also detects the max_tokens field name).
        let basic = ModelRequest { max_output_tokens: Some(256), ..ModelRequest::new(vec![AgentMessage::user("Reply with the single word OK.")], vec![]) };
        let mut basic_result = self.complete_once(&basic, None, &caps, cancel).await;
        if let Err(CtxError::Provider(ProviderError::Api { status: 400, message, .. })) = &basic_result {
            if message.contains("max_completion_tokens") {
                caps.max_tokens_field = "max_completion_tokens".into();
                profile.max_tokens_field = caps.max_tokens_field.clone();
                profile.notes.push("provider requires max_completion_tokens".into());
                basic_result = self.complete_once(&basic, None, &caps, cancel).await;
            }
        }
        match &basic_result {
            Ok(r) => {
                profile.chat_completion = ProbeStatus::Supported;
                profile.connectivity = ProbeStatus::Supported;
                profile.usage_reporting = match r.usage {
                    Some(_) => ProbeStatus::Supported,
                    None => ProbeStatus::Unsupported { reason: "no usage field in response".into() },
                };
                profile.cached_token_reporting = match r.usage.and_then(|u| u.cached_prompt_tokens) {
                    Some(_) => ProbeStatus::Supported,
                    None => ProbeStatus::Unknown,
                };
            }
            Err(e) => {
                profile.chat_completion = ProbeStatus::Failed { error: e.to_string() };
                if let CtxError::Provider(ProviderError::Auth(_)) = e {
                    profile.connectivity = ProbeStatus::Failed { error: e.to_string() };
                }
                // Without basic completion nothing else can be probed.
                return Ok(profile);
            }
        }

        // 2. Tool call generation.
        let tool = Self::probe_tool();
        let tool_req = ModelRequest {
            max_output_tokens: Some(512),
            tool_choice: ToolChoice::Required,
            ..ModelRequest::new(vec![AgentMessage::user("Call the echo_number tool with value 7. Do not answer in text.")], vec![tool.clone()])
        };
        let mut assistant_call: Option<ToolCall> = None;
        match self.complete_once(&tool_req, None, &caps, cancel).await {
            Ok(r) if r.tool_calls.iter().any(|c| c.name == "echo_number") => {
                let call = r.tool_calls.iter().find(|c| c.name == "echo_number").unwrap().clone();
                let args_ok = call.arguments.get("value").and_then(|v| v.as_i64()) == Some(7) || call.arguments.get("value").is_some();
                profile.tool_calling = if args_ok {
                    ProbeStatus::Supported
                } else {
                    ProbeStatus::Unsupported { reason: format!("tool arguments not parseable: {}", call.arguments) }
                };
                assistant_call = Some(call);
            }
            Ok(r) => {
                profile.tool_calling = ProbeStatus::Unsupported { reason: format!("model answered in text instead of calling the tool (finish={:?})", r.finish_reason) };
            }
            Err(e) => {
                // Some servers reject tool_choice=required; retry with auto.
                let retry = ModelRequest { tool_choice: ToolChoice::Auto, ..tool_req.clone() };
                match self.complete_once(&retry, None, &caps, cancel).await {
                    Ok(r) if r.tool_calls.iter().any(|c| c.name == "echo_number") => {
                        let call = r.tool_calls.into_iter().find(|c| c.name == "echo_number").unwrap();
                        if call.arguments.get("value").is_some() {
                            profile.tool_calling = ProbeStatus::Supported;
                            profile.notes.push("tool_choice=required rejected; auto works".into());
                            assistant_call = Some(call);
                        } else {
                            profile.tool_calling = ProbeStatus::Unsupported { reason: format!("tool arguments not parseable: {}", call.arguments) };
                        }
                    }
                    Ok(_) => profile.tool_calling = ProbeStatus::Unsupported { reason: format!("tool call request failed ({e}) and auto mode produced no call") },
                    Err(e2) => profile.tool_calling = ProbeStatus::Failed { error: e2.to_string() },
                }
            }
        }

        // 3. Tool-result continuation.
        if let Some(call) = assistant_call.clone() {
            let cont = ModelRequest {
                max_output_tokens: Some(256),
                ..ModelRequest::new(
                    vec![
                        AgentMessage::user("Call the echo_number tool with value 7. Do not answer in text."),
                        AgentMessage::Assistant(ctx_core::AssistantMessage { text: None, tool_calls: vec![call.clone()], provider_metadata: None }),
                        AgentMessage::tool_result(call.id.clone(), "echo_number", "7", false),
                        AgentMessage::user("What number did the tool return? Answer with just the number."),
                    ],
                    vec![tool.clone()],
                )
            };
            profile.tool_result_continuation = match self.complete_once(&cont, None, &caps, cancel).await {
                Ok(_) => ProbeStatus::Supported,
                Err(e) => ProbeStatus::Failed { error: e.to_string() },
            };
        } else {
            profile.tool_result_continuation = ProbeStatus::Unsupported { reason: "no tool call to continue from".into() };
        }

        // 4. Parallel tool calls (informational).
        if profile.tool_calling.is_supported() {
            let par = ModelRequest {
                max_output_tokens: Some(512),
                parallel_tool_calls: Some(true),
                ..ModelRequest::new(vec![AgentMessage::user("Call echo_number twice in the same response: once with value 1 and once with value 2.")], vec![tool.clone()])
            };
            profile.parallel_tool_calls = match self.complete_once(&par, None, &caps, cancel).await {
                Ok(r) if r.tool_calls.len() >= 2 => ProbeStatus::Supported,
                Ok(r) => ProbeStatus::Unsupported { reason: format!("model produced {} tool call(s) when asked for two", r.tool_calls.len()) },
                Err(e) => ProbeStatus::Failed { error: e.to_string() },
            };
        }

        // 5. Streaming (with and without stream_options).
        let stream_caps = EffectiveCapabilities { streaming: true, stream_usage: true, max_tokens_field: caps.max_tokens_field.clone(), ..Default::default() };
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        // Probe requests use generous output caps (256-512 tokens): a model
        // that is cut off before finishing an answer or a tool call would be
        // misreported as lacking the capability.
        // Drain concurrently: a chatty server must not fill the channel and
        // stall the probe.
        let drain = tokio::spawn(async move {
            let mut deltas = 0usize;
            while let Some(ev) = rx.recv().await {
                if matches!(ev, StreamEvent::TextDelta { .. }) {
                    deltas += 1;
                }
            }
            deltas
        });
        // A generous limit: some models spend their first output tokens on
        // internal work before any visible text, and a 16-token cap can end
        // the stream before a single content delta arrives.
        let stream_req = ModelRequest { max_output_tokens: Some(256), ..ModelRequest::new(vec![AgentMessage::user("Reply with the single word OK.")], vec![]) };
        match self.complete_once(&stream_req, Some(&tx), &stream_caps, cancel).await {
            Ok(r) => {
                drop(tx);
                let deltas = drain.await.unwrap_or(0);
                // Streaming works when the SSE stream delivered content or
                // ended cleanly with a finish reason.
                let clean_finish = matches!(r.finish_reason, FinishReason::Stop | FinishReason::Length | FinishReason::ToolCalls);
                profile.streaming = if deltas > 0 || r.text.is_some() || clean_finish { ProbeStatus::Supported } else { ProbeStatus::Unsupported { reason: format!("stream ended without content (finish reason {:?})", r.finish_reason) } };
                profile.stream_usage = if r.usage.is_some() { ProbeStatus::Supported } else { ProbeStatus::Unsupported { reason: "no usage in final stream chunk".into() } };
            }
            Err(e) => {
                drop(tx);
                let _ = drain.await;
                // Retry without stream_options in case it was rejected.
                let no_usage = EffectiveCapabilities { stream_usage: false, ..stream_caps.clone() };
                let (tx2, _rx2) = tokio::sync::mpsc::channel(64);
                match self.complete_once(&stream_req, Some(&tx2), &no_usage, cancel).await {
                    Ok(_) => {
                        profile.streaming = ProbeStatus::Supported;
                        profile.stream_usage = ProbeStatus::Unsupported { reason: format!("stream_options rejected: {e}") };
                    }
                    Err(e2) => profile.streaming = ProbeStatus::Failed { error: e2.to_string() },
                }
            }
        }

        // 6. Reasoning controls (configured, or listed by the effort catalogue).
        if self.configured.reasoning_controls || self.catalog_lists_effort() {
            let caps_r = EffectiveCapabilities { reasoning_controls: true, max_tokens_field: caps.max_tokens_field.clone(), streaming: false, ..Default::default() };
            let req = ModelRequest { reasoning_effort: Some(ctx_core::ReasoningEffort::level("low")), max_output_tokens: Some(256), ..ModelRequest::new(vec![AgentMessage::user("Reply OK.")], vec![]) };
            profile.reasoning_controls = match self.complete_once(&req, None, &caps_r, cancel).await {
                Ok(_) => ProbeStatus::Supported,
                Err(e) => ProbeStatus::Unsupported { reason: e.to_string() },
            };
        }

        // 7. Developer role (optional).
        if self.configured.developer_role {
            let caps_d = EffectiveCapabilities { developer_role: true, max_tokens_field: caps.max_tokens_field.clone(), streaming: false, ..Default::default() };
            let req = ModelRequest { max_output_tokens: Some(256), ..ModelRequest::new(vec![AgentMessage::task_instructions("Reply OK."), AgentMessage::user("Go.")], vec![]) };
            profile.developer_role = match self.complete_once(&req, None, &caps_d, cancel).await {
                Ok(_) => ProbeStatus::Supported,
                Err(e) => ProbeStatus::Unsupported { reason: e.to_string() },
            };
        }

        *self.profile.write() = profile.clone();
        Ok(profile)
    }

    fn apply_profile(&self, profile: &CapabilityProfile) {
        *self.profile.write() = profile.clone();
    }

    /// ModelGateway's `/api/status` on the endpoint's origin: while an
    /// update replaces the gateway, its proxy answers it with a marked 503.
    async fn gateway_status(&self, cancel: &CancellationToken) -> Option<GatewayStatus> {
        if !self.is_modelgateway() {
            return None;
        }
        let mut url = reqwest::Url::parse(&self.config.base_url).ok()?;
        url.set_path("/api/status");
        url.set_query(None);
        let response = tokio::select! {
            _ = cancel.cancelled() => return None,
            r = self.client.get(url).timeout(Duration::from_secs(10)).send() => r,
        };
        Some(match response {
            Ok(r) if r.headers().get(GATEWAY_STATUS_HEADER).and_then(|v| v.to_str().ok()).is_some_and(|v| v.trim().eq_ignore_ascii_case("updating")) => GatewayStatus::Updating,
            Ok(r) if r.status().is_success() => GatewayStatus::Online,
            _ => GatewayStatus::Unreachable,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ctx_core::{AssistantMessage, ContextPayload, ContextSection, ContextSectionKind, TrustLevel};

    fn provider() -> OpenAiCompatibleProvider {
        let mut cfg = ModelConfig::default();
        cfg.api_key_env = String::new();
        OpenAiCompatibleProvider::new(cfg).unwrap()
    }

    struct Efforts;

    impl ctx_core::EffortLookup for Efforts {
        fn effort_levels(&self, model: &str, _base_url: &str) -> Option<Vec<String>> {
            match model {
                "listed-effort-model" => Some(["low", "medium", "high", "xhigh", "max"].map(String::from).to_vec()),
                "listed-toggle-model" => Some(vec![]),
                _ => None,
            }
        }
    }

    #[test]
    fn reasoning_effort_follows_the_catalogue() {
        use ctx_core::ReasoningEffort;
        ctx_core::install_effort_lookup(std::sync::Arc::new(Efforts));
        let (high, ultra) = (ReasoningEffort::level("high"), ReasoningEffort::Ultra);
        let sent = |model: &str, effort: &ReasoningEffort, caps: &EffectiveCapabilities| {
            let mut cfg = ModelConfig::default();
            cfg.api_key_env = String::new();
            cfg.name = model.into();
            let provider = OpenAiCompatibleProvider::new(cfg).unwrap();
            let request = ModelRequest { reasoning_effort: Some(effort.clone()), ..ModelRequest::new(vec![AgentMessage::user("hi")], vec![]) };
            provider.build_body(&request, false, caps).get("reasoning_effort").and_then(Value::as_str).map(str::to_string)
        };
        let auto = EffectiveCapabilities::default();
        assert_eq!(sent("listed-effort-model", &ultra, &auto).as_deref(), Some("max"), "no opt-in needed for a listed model");
        assert_eq!(sent("listed-effort-model", &high, &auto).as_deref(), Some("high"));
        assert_eq!(sent("listed-toggle-model", &ultra, &auto), None);
        assert_eq!(sent("unlisted-model", &ultra, &auto), None);
        let opted_in = EffectiveCapabilities { reasoning_controls: true, ..Default::default() };
        assert_eq!(sent("unlisted-model", &ultra, &opted_in).as_deref(), Some("high"));
        let rejected = EffectiveCapabilities { catalog_effort: false, ..Default::default() };
        assert_eq!(sent("listed-effort-model", &ultra, &rejected), None, "an endpoint that rejected the field");
    }

    #[test]
    fn user_images_become_content_parts() {
        let image = ctx_core::ImageInput::from_bytes(b"\x89PNG\r\n\x1a\nrest", None).unwrap();
        let messages = vec![AgentMessage::User(ctx_core::UserMessage { text: "what is this?".into(), images: vec![image], files: vec![] })];
        let out = OpenAiCompatibleProvider::translate_messages(&messages, &EffectiveCapabilities::default());
        let parts = out[0]["content"].as_array().unwrap();
        assert_eq!(parts[0], json!({"type": "text", "text": "what is this?"}));
        assert_eq!(parts[1]["type"], "image_url");
        assert!(parts[1]["image_url"]["url"].as_str().unwrap().starts_with("data:image/png;base64,"));
    }

    #[test]
    fn translation_preserves_roles_and_tool_links() {
        let messages = vec![
            AgentMessage::policy("policy"),
            AgentMessage::task_instructions("task"),
            AgentMessage::Context(ContextPayload {
                sections: vec![ContextSection { kind: ContextSectionKind::RepositorySource, trust: TrustLevel::Untrusted, title: "a.rs".into(), body: "fn a(){}".into(), rationale: None }],
            }),
            AgentMessage::user("do it"),
            AgentMessage::Assistant(AssistantMessage {
                text: None,
                tool_calls: vec![ToolCall { id: "c1".into(), name: "read_file".into(), arguments: json!({"path": "a.rs"}) }],
                provider_metadata: None,
            }),
            AgentMessage::tool_result("c1", "read_file", "contents", false),
        ];
        let caps = EffectiveCapabilities::default();
        let out = OpenAiCompatibleProvider::translate_messages(&messages, &caps);
        assert_eq!(out[0]["role"], "system");
        assert_eq!(out[1]["role"], "system");
        assert_eq!(out[2]["role"], "user");
        assert!(out[2]["content"].as_str().unwrap().contains("not instructions"));
        assert_eq!(out[4]["tool_calls"][0]["id"], "c1");
        assert_eq!(out[4]["tool_calls"][0]["function"]["arguments"], "{\"path\":\"a.rs\"}");
        assert_eq!(out[5]["role"], "tool");
        assert_eq!(out[5]["tool_call_id"], "c1");
        let dev = EffectiveCapabilities { developer_role: true, ..Default::default() };
        let out = OpenAiCompatibleProvider::translate_messages(&messages, &dev);
        assert_eq!(out[1]["role"], "developer");
    }

    #[test]
    fn empty_assistant_turns_are_not_sent() {
        // Sessions saved before the runtime stopped recording replies cut off
        // while reasoning still hold these turns.
        let empty = |text: Option<&str>| AgentMessage::Assistant(AssistantMessage { text: text.map(str::to_string), tool_calls: vec![], provider_metadata: None });
        let messages = vec![AgentMessage::user("do it"), empty(None), AgentMessage::user("go on"), empty(Some("")), AgentMessage::assistant_text("done")];
        let out = OpenAiCompatibleProvider::translate_messages(&messages, &EffectiveCapabilities::default());
        let roles: Vec<&str> = out.iter().map(|m| m["role"].as_str().unwrap()).collect();
        assert_eq!(roles, ["user", "user", "assistant"]);
        assert_eq!(out[2]["content"], "done");
    }

    #[test]
    fn body_respects_capabilities() {
        let p = provider();
        let req = ModelRequest { parallel_tool_calls: Some(false), ..ModelRequest::new(vec![AgentMessage::user("x")], vec![OpenAiCompatibleProvider::probe_tool()]) };
        let caps = EffectiveCapabilities::default();
        let body = p.build_body(&req, true, &caps);
        assert!(body.get("tools").is_some());
        assert_eq!(body["parallel_tool_calls"], false);
        assert_eq!(body["stream"], true);
        assert!(body.get("stream_options").is_some());
        assert!(body.get("max_tokens").is_some());
        let no_tools = EffectiveCapabilities { tool_calling: false, stream_usage: false, max_tokens_field: "max_completion_tokens".into(), ..Default::default() };
        let body = p.build_body(&req, true, &no_tools);
        assert!(body.get("tools").is_none());
        assert!(body.get("stream_options").is_none());
        assert!(body.get("max_completion_tokens").is_some());
    }

    #[test]
    fn parses_non_streaming_response() {
        let p = provider();
        let body = json!({
            "id": "resp_1",
            "choices": [{"finish_reason": "tool_calls", "message": {"content": null, "tool_calls": [
                {"id": "call_a", "type": "function", "function": {"name": "read_file", "arguments": "{\"path\": \"x.rs\"}"}},
                {"id": "call_b", "type": "function", "function": {"name": "search_code", "arguments": "{bad json"}}
            ]}}],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "prompt_tokens_details": {"cached_tokens": 4}}
        });
        let r = p.parse_non_streaming(&body).unwrap();
        assert_eq!(r.finish_reason, FinishReason::ToolCalls);
        assert_eq!(r.tool_calls.len(), 2);
        assert_eq!(r.tool_calls[0].arguments["path"], "x.rs");
        assert!(r.tool_calls[1].arguments.get("__malformed_arguments").is_some());
        let usage = r.usage.unwrap();
        assert_eq!(usage.cached_prompt_tokens, Some(4));
        assert_eq!(usage.source, UsageSource::Reported);
        assert_eq!(r.provider_metadata.request_id.as_deref(), Some("resp_1"));
    }

    #[test]
    fn utf8_split_detection() {
        let bytes = "caf\u{e9}".as_bytes();
        let (head, tail) = bytes.split_at(4); // splits the two-byte é
        let valid = std::str::from_utf8(head).map(|_| head.len()).unwrap_or_else(|e| e.valid_up_to());
        assert_eq!(valid, 3);
        let mut pending = head[valid..].to_vec();
        pending.extend_from_slice(tail);
        assert_eq!(std::str::from_utf8(&pending).unwrap(), "\u{e9}");
    }

    #[test]
    fn argument_repair() {
        assert_eq!(parse_arguments(&json!("{\"a\":1}{\"b\":2}"))["a"], 1);
        assert_eq!(parse_arguments(&json!(""))["__malformed_arguments"], Value::Null);
        assert_eq!(parse_arguments(&json!({"x": 1}))["x"], 1);
        assert!(parse_arguments(&json!("[1,2]")).get("__malformed_arguments").is_some());
    }

    #[tokio::test]
    async fn error_mapping() {
        let e = OpenAiCompatibleProvider::map_error_response(
            reqwest::StatusCode::BAD_REQUEST,
            None,
            "{\"error\":{\"message\":\"This model's maximum context length is 8192 tokens\",\"code\":\"context_length_exceeded\"}}".into(),
        )
        .await;
        assert!(matches!(e, ProviderError::ContextLength(_)));
        let e = OpenAiCompatibleProvider::map_error_response(reqwest::StatusCode::TOO_MANY_REQUESTS, Some(Duration::from_secs(2)), "{}".into()).await;
        assert!(matches!(e, ProviderError::RateLimited { retry_after: Some(_) }));
        let e = OpenAiCompatibleProvider::map_error_response(reqwest::StatusCode::UNAUTHORIZED, None, "nope".into()).await;
        assert!(matches!(e, ProviderError::Auth(_)));
    }

    #[tokio::test]
    async fn gateway_upstream_route_failures_are_retried() {
        let body = |source: &str, category: &str| json!({"error": {"message": "Upstream provider balance is insufficient.", "type": "new_api_error", "details": {"source": source, "category": category, "retryable": false}}}).to_string();
        let e = OpenAiCompatibleProvider::map_error_response(reqwest::StatusCode::PAYMENT_REQUIRED, None, body("upstream", "provider_balance")).await;
        assert!(matches!(&e, ProviderError::UpstreamRoute { status: 402, category, .. } if category == "provider_balance") && e.is_retryable());
        assert!(e.to_string().starts_with("provider api error 402: "));
        let e = OpenAiCompatibleProvider::map_error_response(reqwest::StatusCode::UNAUTHORIZED, None, body("upstream", "provider_authentication")).await;
        assert!(matches!(e, ProviderError::UpstreamRoute { status: 401, .. }));
        // The user's own gateway balance and request-shaped upstream errors are not retried.
        let e = OpenAiCompatibleProvider::map_error_response(reqwest::StatusCode::PAYMENT_REQUIRED, None, body("account", "account_balance")).await;
        assert!(matches!(e, ProviderError::Api { status: 402, .. }) && !e.is_transient());
        let e = OpenAiCompatibleProvider::map_error_response(reqwest::StatusCode::BAD_REQUEST, None, body("upstream", "provider_request_rejected")).await;
        assert!(matches!(e, ProviderError::Api { status: 400, .. }) && !e.is_transient());
        let e = OpenAiCompatibleProvider::in_band_error(&json!({"message": "quota", "code": 429, "details": {"source": "upstream", "category": "provider_quota"}}), "stream error");
        assert!(matches!(e, ProviderError::RateLimited { .. }));
        let e = OpenAiCompatibleProvider::in_band_error(&json!({"message": "quota", "details": {"source": "upstream", "category": "provider_quota"}}), "stream error");
        assert!(matches!(e, ProviderError::UpstreamRoute { status: 200, .. }));
    }

    #[test]
    fn in_band_error_mapping() {
        let e = OpenAiCompatibleProvider::in_band_error(&json!({"message": "Upstream provider unavailable."}), "stream error");
        assert!(matches!(e, ProviderError::Api { status: 200, .. }) && e.is_transient());
        let e = OpenAiCompatibleProvider::in_band_error(&json!({"message": "busy", "code": 503}), "stream error");
        assert!(matches!(e, ProviderError::Api { status: 503, .. }));
        let e = OpenAiCompatibleProvider::in_band_error(&json!({"message": "slow down", "code": "rate_limit_exceeded"}), "stream error");
        assert!(matches!(e, ProviderError::RateLimited { .. }));
        let e = OpenAiCompatibleProvider::in_band_error(&json!({"message": "too long", "code": "context_length_exceeded"}), "stream error");
        assert!(matches!(e, ProviderError::ContextLength(_)));
        let e = OpenAiCompatibleProvider::in_band_error(&json!({"message": "bad key", "type": "authentication_error"}), "stream error");
        assert!(matches!(e, ProviderError::Auth(_)) && !e.is_transient());
        let e = OpenAiCompatibleProvider::in_band_error(&json!({"message": "bad field", "type": "invalid_request_error"}), "stream error");
        assert!(matches!(e, ProviderError::Api { status: 400, .. }) && !e.is_transient());
    }
}
