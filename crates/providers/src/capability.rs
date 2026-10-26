//! Provider capability profiles (spec §5.4).

use ctx_core::{ModelCapabilities, PromptCaching, Timestamp};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ProbeStatus {
    #[default]
    Unknown,
    Supported,
    Unsupported { reason: String },
    Failed { error: String },
}

impl ProbeStatus {
    pub fn is_supported(&self) -> bool {
        matches!(self, ProbeStatus::Supported)
    }

    pub fn is_known_unsupported(&self) -> bool {
        matches!(self, ProbeStatus::Unsupported { .. } | ProbeStatus::Failed { .. })
    }

    pub fn symbol(&self) -> &'static str {
        match self {
            ProbeStatus::Unknown => "?",
            ProbeStatus::Supported => "ok",
            ProbeStatus::Unsupported { .. } => "no",
            ProbeStatus::Failed { .. } => "fail",
        }
    }

    pub fn detail(&self) -> String {
        match self {
            ProbeStatus::Unknown => "not probed".into(),
            ProbeStatus::Supported => "supported".into(),
            ProbeStatus::Unsupported { reason } => format!("unsupported: {reason}"),
            ProbeStatus::Failed { error } => format!("failed: {error}"),
        }
    }
}

/// Persisted results of compatibility checks for one endpoint+model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CapabilityProfile {
    pub fingerprint: String,
    pub base_url: String,
    pub model: String,
    pub probed_at: Option<Timestamp>,
    pub connectivity: ProbeStatus,
    pub model_listed: ProbeStatus,
    pub chat_completion: ProbeStatus,
    pub tool_calling: ProbeStatus,
    pub tool_result_continuation: ProbeStatus,
    pub streaming: ProbeStatus,
    pub stream_usage: ProbeStatus,
    pub parallel_tool_calls: ProbeStatus,
    pub usage_reporting: ProbeStatus,
    pub cached_token_reporting: ProbeStatus,
    pub reasoning_controls: ProbeStatus,
    pub developer_role: ProbeStatus,
    /// `max_tokens` or `max_completion_tokens`.
    pub max_tokens_field: String,
    pub notes: Vec<String>,
}

impl CapabilityProfile {
    pub fn new(fingerprint: &str, base_url: &str, model: &str) -> Self {
        Self {
            fingerprint: fingerprint.to_string(),
            base_url: base_url.to_string(),
            model: model.to_string(),
            max_tokens_field: "max_tokens".to_string(),
            ..Default::default()
        }
    }

    /// Whether the endpoint satisfies the required capabilities for an
    /// autonomous agent (spec §5.2).
    pub fn is_agent_capable(&self) -> bool {
        self.chat_completion.is_supported() && self.tool_calling.is_supported() && self.tool_result_continuation.is_supported()
    }

    pub fn blocking_issues(&self) -> Vec<String> {
        let mut out = Vec::new();
        if !self.chat_completion.is_supported() {
            out.push(format!("chat completion: {}", self.chat_completion.detail()));
        }
        if !self.tool_calling.is_supported() {
            out.push(format!("tool calling: {}", self.tool_calling.detail()));
        }
        if !self.tool_result_continuation.is_supported() {
            out.push(format!("tool-result continuation: {}", self.tool_result_continuation.detail()));
        }
        out
    }

    /// Combine configured capabilities with probe outcomes: configuration
    /// may disable a feature; a probe that proved a feature unsupported
    /// disables it; unknown probes defer to configuration.
    pub fn effective(&self, configured: &ModelCapabilities) -> EffectiveCapabilities {
        let combine = |cfg: bool, probe: &ProbeStatus| cfg && !probe.is_known_unsupported();
        EffectiveCapabilities {
            streaming: combine(configured.streaming, &self.streaming),
            stream_usage: !self.stream_usage.is_known_unsupported(),
            tool_calling: combine(configured.tool_calling, &self.tool_calling),
            parallel_tool_calls: combine(configured.parallel_tool_calls, &self.parallel_tool_calls),
            prompt_caching: match configured.prompt_caching {
                PromptCaching::Disabled => false,
                PromptCaching::Enabled => true,
                PromptCaching::Auto => !self.cached_token_reporting.is_known_unsupported(),
            },
            reasoning_controls: combine(configured.reasoning_controls, &self.reasoning_controls),
            catalog_effort: !self.reasoning_controls.is_known_unsupported(),
            developer_role: combine(configured.developer_role, &self.developer_role),
            max_tokens_field: self.max_tokens_field.clone(),
        }
    }
}

/// Capabilities the adapter will actually use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectiveCapabilities {
    pub streaming: bool,
    pub stream_usage: bool,
    pub tool_calling: bool,
    pub parallel_tool_calls: bool,
    pub prompt_caching: bool,
    /// `reasoning_effort` may be sent to any model (configured opt-in).
    pub reasoning_controls: bool,
    /// `reasoning_effort` may be sent to a model the effort catalogue lists
    /// effort values for; off once the endpoint rejected the field.
    pub catalog_effort: bool,
    pub developer_role: bool,
    pub max_tokens_field: String,
}

impl Default for EffectiveCapabilities {
    fn default() -> Self {
        Self {
            streaming: true,
            stream_usage: true,
            tool_calling: true,
            parallel_tool_calls: true,
            prompt_caching: true,
            reasoning_controls: false,
            catalog_effort: true,
            developer_role: false,
            max_tokens_field: "max_tokens".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_results_override_configuration() {
        let mut profile = CapabilityProfile::new("fp", "http://x", "m");
        profile.streaming = ProbeStatus::Unsupported { reason: "no sse".into() };
        profile.tool_calling = ProbeStatus::Supported;
        let configured = ModelCapabilities::default();
        let eff = profile.effective(&configured);
        assert!(!eff.streaming);
        assert!(eff.tool_calling);
        let mut disabled = configured.clone();
        disabled.tool_calling = false;
        assert!(!profile.effective(&disabled).tool_calling);
    }

    #[test]
    fn agent_capable_requires_tools() {
        let mut profile = CapabilityProfile::new("fp", "http://x", "m");
        assert!(!profile.is_agent_capable());
        profile.chat_completion = ProbeStatus::Supported;
        profile.tool_calling = ProbeStatus::Supported;
        profile.tool_result_continuation = ProbeStatus::Supported;
        assert!(profile.is_agent_capable());
    }
}
