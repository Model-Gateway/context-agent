//! A chain of providers tried in order: when one fails, the next answers.
//!
//! Each provider in the chain keeps its own bounded retry (see
//! [`crate::RetryPolicy`]); this layer decides what happens once a provider
//! has given up. A provider that failed over is *demoted* for a cooldown:
//! requests in that window go straight to the next healthy provider instead
//! of paying the full retry budget again, and when the cooldown ends the
//! demoted provider is tried first again, so the chain recovers to the
//! configured order on its own. Streaming goes to whichever provider is
//! tried first; after a provider streamed content and then failed, the
//! rest of the chain answers without streaming so the UI never sees the
//! same text twice.

use crate::capability::{CapabilityProfile, EffectiveCapabilities};
use crate::provider::{GatewayStatus, ModelProvider, ProviderNotice, StreamSink};
use async_trait::async_trait;
use ctx_core::{CancellationToken, CtxError, FinishReason, ModelRequest, ModelResult, ProviderError, Result, StreamEvent};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct FallbackProvider {
    providers: Vec<Arc<dyn ModelProvider>>,
    cooldown: Duration,
    /// Per provider: until when it is demoted, if it is.
    demoted_until: Mutex<Vec<Option<Instant>>>,
    notices: Mutex<Vec<ProviderNotice>>,
}

impl FallbackProvider {
    /// `providers` in the order they should be tried; the first is primary.
    pub fn new(providers: Vec<Arc<dyn ModelProvider>>, cooldown: Duration) -> Self {
        assert!(!providers.is_empty(), "a fallback chain needs at least one provider");
        let n = providers.len();
        Self { providers, cooldown, demoted_until: Mutex::new(vec![None; n]), notices: Mutex::new(Vec::new()) }
    }

    fn primary(&self) -> &Arc<dyn ModelProvider> {
        &self.providers[0]
    }

    /// Indices in the order to try them now: healthy providers in
    /// configured order, then demoted ones in configured order (so a
    /// request still reaches every provider when all are demoted).
    fn order(&self) -> Vec<usize> {
        let now = Instant::now();
        let demoted = self.demoted_until.lock();
        let healthy = (0..self.providers.len()).filter(|i| !matches!(demoted[*i], Some(until) if until > now));
        let cooling = (0..self.providers.len()).filter(|i| matches!(demoted[*i], Some(until) if until > now));
        healthy.chain(cooling).collect()
    }

    fn demote(&self, index: usize) {
        self.demoted_until.lock()[index] = Some(Instant::now() + self.cooldown);
    }

    /// Clear a demotion; true when the provider was demoted.
    fn restore(&self, index: usize) -> bool {
        self.demoted_until.lock()[index].take().is_some()
    }

    /// Models currently demoted, for diagnostics.
    pub fn demoted_models(&self) -> Vec<String> {
        let now = Instant::now();
        let demoted = self.demoted_until.lock();
        self.providers.iter().enumerate().filter(|(i, _)| matches!(demoted[*i], Some(until) if until > now)).map(|(_, p)| p.model().to_string()).collect()
    }

    fn note(&self, notice: ProviderNotice) {
        tracing::warn!(%notice, "provider chain");
        self.notices.lock().push(notice);
    }
}

/// Forwards stream events to the real sink while counting content deltas,
/// so the chain knows whether a failed provider already showed text.
fn counting_sink(real: &StreamSink) -> (StreamSink, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<StreamEvent>(256);
    let count = Arc::new(AtomicUsize::new(0));
    let seen = count.clone();
    let real = real.clone();
    let forwarder = tokio::spawn(async move {
        while let Some(ev) = rx.recv().await {
            if matches!(ev, StreamEvent::TextDelta { .. } | StreamEvent::ReasoningDelta { .. } | StreamEvent::ToolCallStarted { .. }) {
                seen.fetch_add(1, Ordering::SeqCst);
            }
            if real.send(ev).await.is_err() {
                break;
            }
        }
    });
    (tx, count, forwarder)
}

#[async_trait]
impl ModelProvider for FallbackProvider {
    fn name(&self) -> &str {
        self.primary().name()
    }

    fn model(&self) -> &str {
        self.primary().model()
    }

    fn capabilities(&self) -> EffectiveCapabilities {
        self.primary().capabilities()
    }

    async fn complete(&self, request: &ModelRequest, stream: Option<StreamSink>, cancel: &CancellationToken) -> Result<ModelResult> {
        let order = self.order();
        let mut stream = stream;
        let mut last_error: Option<ProviderError> = None;
        for (position, index) in order.iter().copied().enumerate() {
            if cancel.is_cancelled() {
                return Err(CtxError::Cancelled);
            }
            let provider = &self.providers[index];
            let (sink, streamed, forwarder) = match &stream {
                Some(real) => {
                    let (tx, count, task) = counting_sink(real);
                    (Some(tx), Some(count), Some(task))
                }
                None => (None, None, None),
            };
            let outcome = provider.complete(request, sink, cancel).await;
            if let Some(task) = forwarder {
                let _ = task.await;
            }
            match outcome {
                Ok(result) => {
                    if self.restore(index) {
                        self.note(ProviderNotice::Recovered { model: provider.model().to_string() });
                    }
                    if index != 0 {
                        self.note(ProviderNotice::ServedByFallback { model: provider.model().to_string() });
                    }
                    return Ok(result);
                }
                Err(CtxError::Provider(err)) if err.triggers_fallback() && position + 1 < order.len() => {
                    self.demote(index);
                    let next = &self.providers[order[position + 1]];
                    self.note(ProviderNotice::FailedOver { from: provider.model().to_string(), to: next.model().to_string(), error: err.to_string() });
                    // Content already shown must not be streamed again by
                    // the next provider; tell the UI it is superseded.
                    if streamed.map(|c| c.load(Ordering::SeqCst) > 0).unwrap_or(false) {
                        if let Some(real) = stream.take() {
                            let _ = real.send(StreamEvent::Finished { reason: FinishReason::Interrupted }).await;
                        }
                    }
                    last_error = Some(err);
                }
                Err(CtxError::Provider(err)) if err.triggers_fallback() => {
                    self.demote(index);
                    last_error = Some(err);
                }
                Err(other) => return Err(other),
            }
        }
        let last = last_error.map(|e| e.to_string()).unwrap_or_else(|| "no provider configured".into());
        Err(ProviderError::AllModelsFailed { attempted: order.len() as u32, last }.into())
    }

    async fn probe(&self, cancel: &CancellationToken) -> Result<CapabilityProfile> {
        self.primary().probe(cancel).await
    }

    fn apply_profile(&self, profile: &CapabilityProfile) {
        self.primary().apply_profile(profile);
    }

    fn drain_notices(&self) -> Vec<ProviderNotice> {
        let mut out: Vec<ProviderNotice> = std::mem::take(&mut *self.notices.lock());
        for p in &self.providers {
            out.extend(p.drain_notices());
        }
        out
    }

    fn models(&self) -> Vec<String> {
        self.providers.iter().flat_map(|p| p.models()).collect()
    }

    /// The worst status among the chain's gateways: one updating gateway
    /// is worth waiting for even when others answer.
    async fn gateway_status(&self, cancel: &CancellationToken) -> Option<GatewayStatus> {
        let mut out = None;
        for p in &self.providers {
            match p.gateway_status(cancel).await {
                Some(GatewayStatus::Updating) => return Some(GatewayStatus::Updating),
                Some(GatewayStatus::Unreachable) => out = Some(GatewayStatus::Unreachable),
                Some(GatewayStatus::Online) if out.is_none() => out = Some(GatewayStatus::Online),
                _ => {}
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::{MockProvider, MockTurn};
    use ctx_core::AgentMessage;

    fn req() -> ModelRequest {
        ModelRequest::new(vec![AgentMessage::user("hi")], vec![])
    }

    fn failing(model: &str, err: ProviderError, times: usize) -> Arc<MockProvider> {
        Arc::new(MockProvider::new(vec![MockTurn::Error(err); times]).with_model(model).with_fallback(format!("{model} ok")))
    }

    #[tokio::test]
    async fn falls_back_when_primary_fails_and_reports_it() {
        let primary = failing("a", ProviderError::Api { status: 503, code: None, message: "down".into() }, 1);
        let secondary = Arc::new(MockProvider::new(vec![]).with_model("b").with_fallback("b ok"));
        let chain = FallbackProvider::new(vec![primary.clone(), secondary.clone()], Duration::from_secs(60));
        let out = chain.complete(&req(), None, &CancellationToken::new()).await.unwrap();
        assert_eq!(out.text.as_deref(), Some("b ok"));
        assert_eq!(out.provider_metadata.model, "b");
        let notices = chain.drain_notices();
        assert!(matches!(&notices[0], ProviderNotice::FailedOver { from, to, .. } if from == "a" && to == "b"), "{notices:?}");
        assert!(matches!(&notices[1], ProviderNotice::ServedByFallback { model } if model == "b"));
        assert!(chain.drain_notices().is_empty());
        assert_eq!(chain.demoted_models(), vec!["a".to_string()]);
        // While demoted, the primary is skipped outright.
        let out = chain.complete(&req(), None, &CancellationToken::new()).await.unwrap();
        assert_eq!(out.provider_metadata.model, "b");
        assert_eq!(primary.requests().len(), 1);
        assert_eq!(secondary.requests().len(), 2);
        assert_eq!(chain.model(), "a");
        assert_eq!(chain.models(), vec!["a".to_string(), "b".to_string()]);
    }

    #[tokio::test]
    async fn primary_is_tried_again_after_the_cooldown() {
        let primary = failing("a", ProviderError::Timeout, 1);
        let secondary = Arc::new(MockProvider::new(vec![]).with_model("b").with_fallback("b ok"));
        let chain = FallbackProvider::new(vec![primary.clone(), secondary], Duration::from_millis(20));
        assert_eq!(chain.complete(&req(), None, &CancellationToken::new()).await.unwrap().provider_metadata.model, "b");
        tokio::time::sleep(Duration::from_millis(40)).await;
        let out = chain.complete(&req(), None, &CancellationToken::new()).await.unwrap();
        assert_eq!(out.provider_metadata.model, "a");
        assert_eq!(out.text.as_deref(), Some("a ok"));
        let notices = chain.drain_notices();
        assert!(notices.iter().any(|n| matches!(n, ProviderNotice::Recovered { model } if model == "a")), "{notices:?}");
        assert!(chain.demoted_models().is_empty());
    }

    #[tokio::test]
    async fn all_failing_yields_one_aggregate_error_and_still_tries_demoted_ones() {
        let a = Arc::new(MockProvider::new(vec![MockTurn::Error(ProviderError::Connection("refused".into())); 4]).with_model("a"));
        let b = Arc::new(MockProvider::new(vec![MockTurn::Error(ProviderError::Auth("bad key".into())); 4]).with_model("b"));
        let chain = FallbackProvider::new(vec![a.clone(), b.clone()], Duration::from_secs(60));
        let err = chain.complete(&req(), None, &CancellationToken::new()).await.unwrap_err();
        assert!(matches!(err, CtxError::Provider(ProviderError::AllModelsFailed { attempted: 2, ref last }) if last.contains("bad key")), "{err}");
        assert_eq!(chain.demoted_models().len(), 2);
        // Both demoted: the next request still goes through both, in order.
        let err = chain.complete(&req(), None, &CancellationToken::new()).await.unwrap_err();
        assert!(matches!(err, CtxError::Provider(ProviderError::AllModelsFailed { .. })));
        assert_eq!(a.requests().len(), 2);
        assert_eq!(b.requests().len(), 2);
    }

    #[tokio::test]
    async fn context_length_and_cancellation_do_not_fall_back() {
        let a = Arc::new(MockProvider::new(vec![MockTurn::Error(ProviderError::ContextLength("too long".into()))]).with_model("a"));
        let b = Arc::new(MockProvider::new(vec![]).with_model("b").with_fallback("b ok"));
        let chain = FallbackProvider::new(vec![a, b.clone()], Duration::from_secs(60));
        let err = chain.complete(&req(), None, &CancellationToken::new()).await.unwrap_err();
        assert!(matches!(err, CtxError::Provider(ProviderError::ContextLength(_))));
        assert!(b.requests().is_empty());
        assert!(chain.demoted_models().is_empty());
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert!(matches!(chain.complete(&req(), None, &cancel).await.unwrap_err(), CtxError::Cancelled));
        assert!(b.requests().is_empty());
    }

    #[tokio::test]
    async fn streaming_moves_to_the_fallback_when_nothing_was_shown() {
        let a = Arc::new(MockProvider::new(vec![MockTurn::Error(ProviderError::Timeout)]).with_model("a"));
        let b = Arc::new(MockProvider::new(vec![]).with_model("b").with_fallback("hello from b"));
        let chain = FallbackProvider::new(vec![a, b], Duration::from_secs(60));
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        let out = chain.complete(&req(), Some(tx), &CancellationToken::new()).await.unwrap();
        assert_eq!(out.text.as_deref(), Some("hello from b"));
        let mut streamed = String::new();
        while let Ok(ev) = rx.try_recv() {
            if let StreamEvent::TextDelta { text } = ev {
                streamed.push_str(&text);
            }
        }
        assert_eq!(streamed, "hello from b");
    }

    #[tokio::test]
    async fn streamed_content_is_not_repeated_by_the_fallback() {
        // The primary streams a partial answer, then fails.
        let a = Arc::new(MockProvider::dynamic(|_| Err(ProviderError::InterruptedStream("cut".into()).into())).with_model("a"));
        let streaming_then_failing: Arc<dyn ModelProvider> = Arc::new(StreamThenFail { inner: a });
        let b = Arc::new(MockProvider::new(vec![]).with_model("b").with_fallback("full answer"));
        let chain = FallbackProvider::new(vec![streaming_then_failing, b], Duration::from_secs(60));
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        let out = chain.complete(&req(), Some(tx), &CancellationToken::new()).await.unwrap();
        assert_eq!(out.text.as_deref(), Some("full answer"));
        let mut events = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            events.push(ev);
        }
        let text: String = events.iter().filter_map(|e| if let StreamEvent::TextDelta { text } = e { Some(text.as_str()) } else { None }).collect();
        assert_eq!(text, "partial ", "the fallback answer must not stream on top of the partial one");
        assert!(events.iter().any(|e| matches!(e, StreamEvent::Finished { reason: FinishReason::Interrupted })));
    }

    /// Sends one text delta, then delegates (and fails).
    struct StreamThenFail {
        inner: Arc<MockProvider>,
    }

    #[async_trait]
    impl ModelProvider for StreamThenFail {
        fn name(&self) -> &str {
            "mock"
        }
        fn model(&self) -> &str {
            self.inner.model()
        }
        fn capabilities(&self) -> EffectiveCapabilities {
            EffectiveCapabilities::default()
        }
        async fn complete(&self, request: &ModelRequest, stream: Option<StreamSink>, cancel: &CancellationToken) -> Result<ModelResult> {
            if let Some(s) = &stream {
                let _ = s.send(StreamEvent::TextDelta { text: "partial ".into() }).await;
            }
            self.inner.complete(request, None, cancel).await
        }
        async fn probe(&self, cancel: &CancellationToken) -> Result<CapabilityProfile> {
            self.inner.probe(cancel).await
        }
        fn apply_profile(&self, _profile: &CapabilityProfile) {}
    }
}
