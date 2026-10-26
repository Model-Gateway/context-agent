//! The provider trait.

use crate::capability::{CapabilityProfile, EffectiveCapabilities};
use async_trait::async_trait;
use ctx_core::{CancellationToken, ModelRequest, ModelResult, Result, StreamEvent};
use tokio::sync::mpsc;

/// Receives streaming events while a completion is in flight.
pub type StreamSink = mpsc::Sender<StreamEvent>;

/// Something a provider did on its own that the user should hear about:
/// a request served by a fallback model, or the primary model recovering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderNotice {
    /// `from` failed with `error` (after its own retries) and `to` was tried
    /// next.
    FailedOver { from: String, to: String, error: String },
    /// `model` served the request although it is not the primary model.
    ServedByFallback { model: String },
    /// A previously demoted model answered again.
    Recovered { model: String },
}

impl std::fmt::Display for ProviderNotice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderNotice::FailedOver { from, to, error } => write!(f, "model {from} failed ({error}); falling back to {to}"),
            ProviderNotice::ServedByFallback { model } => write!(f, "request served by fallback model {model}"),
            ProviderNotice::Recovered { model } => write!(f, "model {model} is answering again"),
        }
    }
}

/// What a hosted gateway in front of the model reports about itself, so a
/// failed step can wait out a planned update instead of failing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatewayStatus {
    /// Answering normally.
    Online,
    /// Being updated: requests fail until the new version is up.
    Updating,
    /// No answer at all (a connection error or an unmarked proxy error).
    Unreachable,
}

#[async_trait]
pub trait ModelProvider: Send + Sync {
    /// Transport identifier, e.g. `openai_compatible` or `mock`.
    fn name(&self) -> &str;

    /// Model identifier sent to the provider.
    fn model(&self) -> &str;

    /// Capabilities in effect (configuration combined with probe results).
    fn capabilities(&self) -> EffectiveCapabilities;

    /// Run one completion. When `stream` is provided and the provider
    /// supports streaming, incremental events are sent to it; the full
    /// normalized result is always returned.
    async fn complete(&self, request: &ModelRequest, stream: Option<StreamSink>, cancel: &CancellationToken) -> Result<ModelResult>;

    /// Run capability checks against the live endpoint (spec §5.4).
    async fn probe(&self, cancel: &CancellationToken) -> Result<CapabilityProfile>;

    /// Replace the active capability profile (after probing or loading).
    fn apply_profile(&self, profile: &CapabilityProfile);

    /// Notices accumulated since the last call, oldest first. The runtime
    /// drains them after each completion and reports them as warnings.
    fn drain_notices(&self) -> Vec<ProviderNotice> {
        Vec::new()
    }

    /// Model identifiers this provider may answer with, primary first.
    fn models(&self) -> Vec<String> {
        vec![self.model().to_string()]
    }

    /// The status of the gateway this provider routes through, when it is
    /// one that reports it (ModelGateway); `None` for any other endpoint.
    async fn gateway_status(&self, _cancel: &CancellationToken) -> Option<GatewayStatus> {
        None
    }
}
