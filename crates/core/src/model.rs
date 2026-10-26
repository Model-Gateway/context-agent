//! Normalized model request/response contract (spec §5.6).

use crate::message::{AgentMessage, ToolCall, ToolDefinition};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    ToolCalls,
    Length,
    ContentFilter,
    Cancelled,
    Interrupted,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageSource {
    /// Reported by the provider in the response.
    Reported,
    /// Estimated locally with a heuristic tokenizer.
    Estimated,
}

/// Token accounting; `source` distinguishes reported from estimated values
/// (spec §14.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    #[serde(default)]
    pub cached_prompt_tokens: Option<u64>,
    #[serde(default)]
    pub reasoning_tokens: Option<u64>,
    pub source: UsageSource,
}

impl TokenUsage {
    pub fn estimated(prompt_tokens: u64, completion_tokens: u64) -> Self {
        Self {
            prompt_tokens,
            completion_tokens,
            cached_prompt_tokens: None,
            reasoning_tokens: None,
            source: UsageSource::Estimated,
        }
    }

    pub fn total(&self) -> u64 {
        self.prompt_tokens + self.completion_tokens
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ProviderMetadata {
    pub provider: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// Provider-specific opaque values. Only the originating adapter may
    /// reinterpret these.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// Normalized result of one model invocation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelResult {
    pub text: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    pub finish_reason: FinishReason,
    #[serde(default)]
    pub usage: Option<TokenUsage>,
    pub provider_metadata: ProviderMetadata,
}

impl ModelResult {
    pub fn has_tool_calls(&self) -> bool {
        !self.tool_calls.is_empty()
    }
}

/// How hard the model is asked to think: one of the values the model
/// accepts (models.dev's `reasoning_options`, e.g. `low` … `xhigh`, `max`),
/// sent as is, or `Ultra`, the Context tier: the model's strongest value,
/// and the runtime lets the model deploy sub-agents that investigate and
/// implement in parallel.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ReasoningEffort {
    Level(String),
    Ultra,
}

impl Default for ReasoningEffort {
    fn default() -> Self {
        ReasoningEffort::level("medium")
    }
}

impl ReasoningEffort {
    /// The values offered for a model whose own values are unknown.
    pub const STANDARD: [&'static str; 3] = ["low", "medium", "high"];

    pub fn level(value: &str) -> Self {
        ReasoningEffort::Level(value.to_string())
    }

    pub fn as_str(&self) -> &str {
        match self {
            ReasoningEffort::Level(value) => value,
            ReasoningEffort::Ultra => "ultra",
        }
    }

    /// `ultra`, or any effort value (a lowercase word: letters, digits,
    /// `-`, `_`).
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().to_ascii_lowercase();
        match s.as_str() {
            "ultra" => Some(ReasoningEffort::Ultra),
            "med" => Some(ReasoningEffort::level("medium")),
            v if v.starts_with(|c: char| c.is_ascii_lowercase()) && v.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_') => Some(ReasoningEffort::Level(s)),
            _ => None,
        }
    }

    /// The value sent to an endpoint whose model's effort values are
    /// unknown: a level as is; `ultra` is a Context tier, not an API value,
    /// so it goes out as the strongest setting every endpoint accepts.
    pub fn wire_value(&self) -> &str {
        match self {
            ReasoningEffort::Level(value) => value,
            ReasoningEffort::Ultra => "high",
        }
    }

    /// The value sent to a model that accepts the effort values `levels`:
    /// `ultra` takes the strongest, a level itself when listed. A level the
    /// model does not list (picked for another model) falls back to the
    /// nearest listed one on [`EFFORT_LADDER`], the stronger on a tie, never
    /// `none`; `None` when there is none.
    pub fn wire_value_for(&self, levels: &[String]) -> Option<String> {
        let rank = |v: &str| EFFORT_LADDER.iter().position(|l| l.eq_ignore_ascii_case(v));
        let ranked: Vec<(usize, &String)> = levels.iter().filter_map(|v| Some((rank(v)?, v))).filter(|(r, _)| *r > 0).collect();
        let pick = match self {
            // Unknown names sort after the ladder: models.dev lists values weakest first.
            ReasoningEffort::Ultra => ranked.iter().max_by_key(|(r, _)| *r).map(|(_, v)| *v).or_else(|| levels.iter().rfind(|v| rank(v).is_none())),
            ReasoningEffort::Level(value) => levels.iter().find(|l| l.eq_ignore_ascii_case(value)).or_else(|| {
                let target = rank(value)?;
                ranked.iter().min_by_key(|(r, _)| (r.abs_diff(target), std::cmp::Reverse(*r))).map(|(_, v)| *v)
            }),
        };
        pick.cloned()
    }

    /// Whether this tier lets the model deploy sub-agents.
    pub fn deploys_agents(&self) -> bool {
        matches!(self, ReasoningEffort::Ultra)
    }
}

impl Serialize for ReasoningEffort {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ReasoningEffort {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        ReasoningEffort::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("unknown reasoning effort `{s}` (a model's effort value such as low, high or max, or ultra)")))
    }
}

/// Effort values endpoints accept, weakest first (models.dev's vocabulary).
pub const EFFORT_LADDER: [&str; 7] = ["none", "minimal", "low", "medium", "high", "xhigh", "max"];

/// A catalogue of the reasoning-effort values each model accepts.
pub trait EffortLookup: Send + Sync {
    /// The effort values `model` as served from `base_url` accepts, weakest
    /// first: empty when the catalogue lists the model without effort
    /// control, `None` when it does not know.
    fn effort_levels(&self, model: &str, base_url: &str) -> Option<Vec<String>>;
}

static EFFORT_LOOKUP: RwLock<Option<Arc<dyn EffortLookup>>> = RwLock::new(None);

/// Installs (or replaces) the process-wide effort catalogue (the frontends
/// install models.dev's).
pub fn install_effort_lookup(lookup: Arc<dyn EffortLookup>) {
    *EFFORT_LOOKUP.write().unwrap_or_else(|e| e.into_inner()) = Some(lookup);
}

/// [`EffortLookup::effort_levels`] of the installed catalogue.
pub fn effort_levels(model: &str, base_url: &str) -> Option<Vec<String>> {
    let lookup = EFFORT_LOOKUP.read().unwrap_or_else(|e| e.into_inner()).clone();
    lookup?.effort_levels(model, base_url)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolChoice {
    #[default]
    Auto,
    None,
    Required,
}

/// Cache guidance for adapters that support provider-side prompt caching.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CacheHint {
    /// Number of leading messages forming the stable prefix.
    pub stable_prefix_len: usize,
    /// Optional stable key used by providers with keyed caches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,
}

/// A fully compiled request to a model provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelRequest {
    pub messages: Vec<AgentMessage>,
    #[serde(default)]
    pub tools: Vec<ToolDefinition>,
    #[serde(default)]
    pub tool_choice: ToolChoice,
    #[serde(default)]
    pub parallel_tool_calls: Option<bool>,
    #[serde(default)]
    pub max_output_tokens: Option<u32>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub reasoning_effort: Option<ReasoningEffort>,
    #[serde(default)]
    pub cache: CacheHint,
    /// Free-form metadata for logging; never sent to providers.
    #[serde(default)]
    pub tags: BTreeMap<String, String>,
    /// Ask ModelGateway to refuse (`ProviderError::DiscountDrop`) rather
    /// than route to a provider discounted this many percentage points
    /// below the model's best offer. Ignored by other endpoints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_discount_drop: Option<u32>,
}

impl ModelRequest {
    pub fn new(messages: Vec<AgentMessage>, tools: Vec<ToolDefinition>) -> Self {
        Self {
            messages,
            tools,
            tool_choice: ToolChoice::Auto,
            parallel_tool_calls: None,
            max_output_tokens: None,
            temperature: None,
            reasoning_effort: None,
            cache: CacheHint::default(),
            tags: BTreeMap::new(),
            max_discount_drop: None,
        }
    }
}

/// Incremental events emitted while streaming a model response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    TextDelta { text: String },
    /// Reasoning ("thinking") text some endpoints stream alongside the
    /// answer; display only, never sent back to the model.
    ReasoningDelta { text: String },
    ToolCallStarted { index: usize, id: String, name: String },
    ToolCallArgumentsDelta { index: usize, fragment: String },
    Usage { usage: TokenUsage },
    Finished { reason: FinishReason },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn levels(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn efforts_are_the_models_own_values_or_ultra() {
        let pick = |effort: &str, values: &[&str]| ReasoningEffort::parse(effort).unwrap().wire_value_for(&levels(values));
        let opus = ["low", "medium", "high", "xhigh", "max"];
        assert_eq!(["low", "xhigh", "max", "ultra"].map(|e| pick(e, &opus)), ["low", "xhigh", "max", "max"].map(|v| Some(v.to_string())));
        assert_eq!(pick("ultra", &["none", "minimal", "low", "medium", "high"]).as_deref(), Some("high"));
        assert_eq!(pick("ultra", &["low", "medium", "high", "xhigh"]).as_deref(), Some("xhigh"));
        assert_eq!(pick("none", &["none", "low", "high"]).as_deref(), Some("none"), "a listed value goes out as is");
        // A value picked for another model: the nearest listed, the stronger on a tie; `none` never.
        assert_eq!(pick("medium", &["low", "high", "max"]).as_deref(), Some("high"));
        assert_eq!(pick("low", &["none", "minimal", "high"]).as_deref(), Some("minimal"));
        assert_eq!(pick("xhigh", &["none", "low", "medium"]).as_deref(), Some("medium"));
        assert_eq!(pick("low", &["none"]), None);
        assert_eq!(pick("ultra", &[]), None);
        assert_eq!(pick("ultra", &["low", "turbo"]).as_deref(), Some("low"), "known values outrank unknown ones");
        assert_eq!(pick("ultra", &["fast", "deep"]).as_deref(), Some("deep"));
    }

    #[test]
    fn efforts_parse_and_serialize_as_their_value() {
        assert_eq!(ReasoningEffort::parse(" XHigh "), Some(ReasoningEffort::level("xhigh")));
        assert_eq!(ReasoningEffort::parse("max"), Some(ReasoningEffort::level("max")), "max is a model's value, not Ultra");
        assert_eq!(ReasoningEffort::parse("Ultra"), Some(ReasoningEffort::Ultra));
        assert_eq!(ReasoningEffort::parse("very hard"), None);
        assert_eq!(ReasoningEffort::parse(""), None);
        assert_eq!(serde_json::to_value(ReasoningEffort::level("xhigh")).unwrap(), "xhigh");
        assert_eq!(serde_json::from_value::<ReasoningEffort>("ultra".into()).unwrap(), ReasoningEffort::Ultra);
        assert!(serde_json::from_value::<ReasoningEffort>("very hard".into()).is_err());
    }
}
