//! Scripted mock provider for tests and offline evaluation.

use crate::capability::{CapabilityProfile, EffectiveCapabilities, ProbeStatus};
use crate::provider::{ModelProvider, StreamSink};
use async_trait::async_trait;
use ctx_core::{CancellationToken, FinishReason, ModelRequest, ModelResult, ProviderError, ProviderMetadata, Result, StreamEvent, TokenUsage, ToolCall};
use parking_lot::Mutex;
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::Arc;

type DynamicTurn = Arc<dyn Fn(&ModelRequest) -> Result<ModelResult> + Send + Sync>;

/// One scripted model response.
#[derive(Clone)]
pub enum MockTurn {
    Text(String),
    ToolCalls(Vec<(String, Value)>),
    /// Text and tool calls together.
    Mixed(String, Vec<(String, Value)>),
    Error(ProviderError),
    Dynamic(DynamicTurn),
}

impl std::fmt::Debug for MockTurn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MockTurn::Text(t) => write!(f, "Text({t:?})"),
            MockTurn::ToolCalls(c) => write!(f, "ToolCalls({c:?})"),
            MockTurn::Mixed(t, c) => write!(f, "Mixed({t:?}, {c:?})"),
            MockTurn::Error(e) => write!(f, "Error({e:?})"),
            MockTurn::Dynamic(_) => write!(f, "Dynamic(..)"),
        }
    }
}

pub struct MockProvider {
    turns: Mutex<VecDeque<MockTurn>>,
    requests: Mutex<Vec<ModelRequest>>,
    model: String,
    /// When the script is exhausted, return this text instead of failing.
    fallback: Option<String>,
    call_counter: Mutex<u64>,
}

impl MockProvider {
    pub fn new(turns: Vec<MockTurn>) -> Self {
        Self { turns: Mutex::new(turns.into()), requests: Mutex::new(Vec::new()), model: "mock-model".into(), fallback: None, call_counter: Mutex::new(0) }
    }

    pub fn with_fallback(mut self, text: impl Into<String>) -> Self {
        self.fallback = Some(text.into());
        self
    }

    /// Report a different model name (to tell providers of a chain apart).
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    pub fn dynamic(f: impl Fn(&ModelRequest) -> Result<ModelResult> + Send + Sync + 'static) -> Self {
        let mut p = Self::new(vec![]);
        p.fallback = None;
        p.turns = Mutex::new(VecDeque::from(vec![MockTurn::Dynamic(Arc::new(f))]));
        p
    }

    pub fn push(&self, turn: MockTurn) {
        self.turns.lock().push_back(turn);
    }

    pub fn requests(&self) -> Vec<ModelRequest> {
        self.requests.lock().clone()
    }

    pub fn remaining(&self) -> usize {
        self.turns.lock().len()
    }

    fn metadata(&self) -> ProviderMetadata {
        ProviderMetadata { provider: "mock".into(), model: self.model.clone(), request_id: None, extra: Default::default() }
    }

    fn result_from(&self, text: Option<String>, calls: Vec<(String, Value)>, request: &ModelRequest) -> ModelResult {
        let mut counter = self.call_counter.lock();
        let tool_calls: Vec<ToolCall> = calls
            .into_iter()
            .map(|(name, arguments)| {
                *counter += 1;
                ToolCall { id: format!("mock_call_{}", *counter), name, arguments }
            })
            .collect();
        let prompt_tokens: u64 = request.messages.iter().map(|m| ctx_core::estimate_tokens(&m.text_for_estimate()) as u64).sum();
        let completion_tokens = text.as_deref().map(|t| ctx_core::estimate_tokens(t) as u64).unwrap_or(0) + tool_calls.len() as u64 * 20;
        ModelResult {
            finish_reason: if tool_calls.is_empty() { FinishReason::Stop } else { FinishReason::ToolCalls },
            text,
            tool_calls,
            usage: Some(TokenUsage { prompt_tokens, completion_tokens, cached_prompt_tokens: None, reasoning_tokens: None, source: ctx_core::UsageSource::Reported }),
            provider_metadata: self.metadata(),
        }
    }
}

#[async_trait]
impl ModelProvider for MockProvider {
    fn name(&self) -> &str {
        "mock"
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn capabilities(&self) -> EffectiveCapabilities {
        EffectiveCapabilities::default()
    }

    async fn complete(&self, request: &ModelRequest, stream: Option<StreamSink>, cancel: &CancellationToken) -> Result<ModelResult> {
        if cancel.is_cancelled() {
            return Err(ctx_core::CtxError::Cancelled);
        }
        self.requests.lock().push(request.clone());
        let turn = {
            let mut turns = self.turns.lock();
            match turns.front() {
                Some(MockTurn::Dynamic(_)) => turns.front().cloned(),
                _ => turns.pop_front(),
            }
        };
        let result = match turn {
            Some(MockTurn::Text(t)) => self.result_from(Some(t), vec![], request),
            Some(MockTurn::ToolCalls(calls)) => self.result_from(None, calls, request),
            Some(MockTurn::Mixed(t, calls)) => self.result_from(Some(t), calls, request),
            Some(MockTurn::Error(e)) => return Err(e.into()),
            Some(MockTurn::Dynamic(f)) => f(request)?,
            None => match &self.fallback {
                Some(text) => self.result_from(Some(text.clone()), vec![], request),
                None => return Err(ProviderError::InvalidResponse("mock script exhausted".into()).into()),
            },
        };
        if let Some(sink) = stream {
            if let Some(text) = &result.text {
                for word in text.split_inclusive(' ') {
                    let _ = sink.send(StreamEvent::TextDelta { text: word.to_string() }).await;
                }
            }
            for (i, call) in result.tool_calls.iter().enumerate() {
                let _ = sink.send(StreamEvent::ToolCallStarted { index: i, id: call.id.clone(), name: call.name.clone() }).await;
                let _ = sink.send(StreamEvent::ToolCallArgumentsDelta { index: i, fragment: call.arguments.to_string() }).await;
            }
            if let Some(u) = result.usage {
                let _ = sink.send(StreamEvent::Usage { usage: u }).await;
            }
            let _ = sink.send(StreamEvent::Finished { reason: result.finish_reason }).await;
        }
        Ok(result)
    }

    async fn probe(&self, _cancel: &CancellationToken) -> Result<CapabilityProfile> {
        let mut p = CapabilityProfile::new("mock", "mock://", &self.model);
        p.probed_at = Some(ctx_core::now());
        p.connectivity = ProbeStatus::Supported;
        p.chat_completion = ProbeStatus::Supported;
        p.tool_calling = ProbeStatus::Supported;
        p.tool_result_continuation = ProbeStatus::Supported;
        p.streaming = ProbeStatus::Supported;
        p.stream_usage = ProbeStatus::Supported;
        p.parallel_tool_calls = ProbeStatus::Supported;
        p.usage_reporting = ProbeStatus::Supported;
        Ok(p)
    }

    fn apply_profile(&self, _profile: &CapabilityProfile) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use ctx_core::AgentMessage;
    use serde_json::json;

    #[tokio::test]
    async fn scripted_turns_and_streaming() {
        let p = MockProvider::new(vec![MockTurn::ToolCalls(vec![("read_file".into(), json!({"path": "a"}))]), MockTurn::Text("done".into())]);
        let req = ModelRequest::new(vec![AgentMessage::user("hi")], vec![]);
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        let r1 = p.complete(&req, Some(tx), &CancellationToken::new()).await.unwrap();
        assert_eq!(r1.tool_calls.len(), 1);
        assert_eq!(r1.finish_reason, FinishReason::ToolCalls);
        let mut saw_start = false;
        while let Ok(ev) = rx.try_recv() {
            if matches!(ev, StreamEvent::ToolCallStarted { .. }) {
                saw_start = true;
            }
        }
        assert!(saw_start);
        let r2 = p.complete(&req, None, &CancellationToken::new()).await.unwrap();
        assert_eq!(r2.text.as_deref(), Some("done"));
        assert!(p.complete(&req, None, &CancellationToken::new()).await.is_err());
        assert_eq!(p.requests().len(), 3);
    }
}
