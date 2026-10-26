//! Model-agnostic inference layer (spec §5).
//!
//! The runtime talks to [`ModelProvider`] only. The OpenAI-compatible
//! adapter translates the internal [`ctx_core::AgentMessage`] representation
//! into chat-completions requests, parses streaming and non-streaming
//! responses into [`ctx_core::ModelResult`], and normalizes errors. The
//! capability profile records what a given endpoint actually supports.

pub mod capability;
pub mod fallback;
pub mod mock;
pub mod openai;
pub mod provider;
pub mod retry;
pub mod sse;

pub use capability::{CapabilityProfile, EffectiveCapabilities, ProbeStatus};
pub use fallback::FallbackProvider;
pub use mock::{MockProvider, MockTurn};
pub use openai::OpenAiCompatibleProvider;
pub use provider::{GatewayStatus, ModelProvider, ProviderNotice, StreamSink};
pub use retry::RetryPolicy;

use ctx_core::{resolve_api_key, Config, CtxError, ModelConfig, Result};
use std::sync::Arc;
use std::time::Duration;

/// Build the configured provider: the primary model, wrapped in a
/// [`FallbackProvider`] when `model.fallbacks` names others to try when it
/// fails. Each fallback gets its own cached capability profile when one
/// exists; the caller applies the primary's profile as before.
pub fn build_provider(config: &Config) -> Result<Arc<dyn ModelProvider>> {
    let chain = config.model.chain();
    let mut providers: Vec<Arc<dyn ModelProvider>> = Vec::with_capacity(chain.len());
    for (i, model) in chain.iter().enumerate() {
        let provider = build_single(model, resolve_api_key(config, model))?;
        if i > 0 {
            if let Some(profile) = load_profile_for(config, model)? {
                provider.apply_profile(&profile);
            }
        }
        providers.push(provider);
    }
    if providers.len() == 1 {
        return Ok(providers.pop().unwrap());
    }
    Ok(Arc::new(FallbackProvider::new(providers, Duration::from_secs(config.model.fallback_cooldown_secs))))
}

/// [`build_provider`] for a run inside the repository `project`: when
/// `model.project_header` is set, the primary model and every fallback send
/// that header with the (sanitised) project name.
pub fn build_provider_for_project(config: &Config, project: &str) -> Result<Arc<dyn ModelProvider>> {
    if config.model.project_header.is_none() {
        return build_provider(config);
    }
    let mut scoped = config.clone();
    scoped.model = config.model.for_project(project);
    build_provider(&scoped)
}

/// One transport for one model. Only the OpenAI-compatible transport is
/// implemented; the match keeps the extension point explicit.
/// `api_key` is the resolved key (stored credential, else environment).
fn build_single(model: &ModelConfig, api_key: Option<String>) -> Result<Arc<dyn ModelProvider>> {
    match model.transport {
        ctx_core::Transport::ChatCompletions => Ok(Arc::new(OpenAiCompatibleProvider::with_api_key(model.clone(), api_key)?)),
    }
}

/// Models the configured endpoint lists under `/models`.
pub async fn list_models(config: &Config) -> Result<Vec<String>> {
    match config.model.transport {
        ctx_core::Transport::ChatCompletions => OpenAiCompatibleProvider::with_api_key(config.model.clone(), resolve_api_key(config, &config.model))?.list_models().await,
    }
}

/// Load a persisted capability profile for the configured model, if any.
pub fn load_profile(config: &Config) -> Result<Option<CapabilityProfile>> {
    load_profile_for(config, &config.model)
}

/// Load the persisted capability profile of any model (a fallback, say).
pub fn load_profile_for(config: &Config, model: &ModelConfig) -> Result<Option<CapabilityProfile>> {
    let path = config.layout().providers_dir().join(format!("{}.json", model.fingerprint()));
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path)?;
    let profile: CapabilityProfile = serde_json::from_str(&text).map_err(|e| CtxError::Config(format!("corrupt capability profile {}: {e}", path.display())))?;
    Ok(Some(profile))
}

pub fn save_profile(config: &Config, profile: &CapabilityProfile) -> Result<std::path::PathBuf> {
    let dir = config.layout().providers_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", config.model.fingerprint()));
    std::fs::write(&path, serde_json::to_string_pretty(profile)?)?;
    Ok(path)
}
