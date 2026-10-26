//! Configuration (spec §5.3, §11.1, §13.1).
//!
//! Resolution order, later layers override earlier ones:
//! 1. built-in defaults
//! 2. `~/.context-agent/config.toml`
//! 3. `<repository>/.context-agent.toml`
//! 4. an explicit `--config` file
//! 5. `CTX_*` environment variables

use crate::error::{CtxError, Result};
use crate::paths::{default_data_dir, DataLayout};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub model: ModelConfig,
    pub permissions: PermissionsConfig,
    pub index: IndexConfig,
    pub embeddings: EmbeddingConfig,
    pub context: ContextConfig,
    pub agent: AgentConfig,
    pub validation: ValidationConfig,
    pub telemetry: TelemetryConfig,
    pub web_search: WebSearchConfig,
    pub web_fetch: WebFetchConfig,
    pub mcp: McpConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pricing: Option<PricingConfig>,
    pub spend: crate::pricing::SpendConfig,
    /// Resolved at load time; not read from files.
    #[serde(skip)]
    pub data_dir: PathBuf,
    /// Files that contributed to this configuration, for diagnostics.
    #[serde(skip)]
    pub sources: Vec<PathBuf>,
    /// Top-level tables a repository-level file tried to set but which are
    /// only honoured from trusted (user-owned) configuration.
    #[serde(skip)]
    pub ignored_repository_keys: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            model: ModelConfig::default(),
            permissions: PermissionsConfig::default(),
            index: IndexConfig::default(),
            embeddings: EmbeddingConfig::default(),
            context: ContextConfig::default(),
            agent: AgentConfig::default(),
            validation: ValidationConfig::default(),
            telemetry: TelemetryConfig::default(),
            web_search: WebSearchConfig::default(),
            web_fetch: WebFetchConfig::default(),
            mcp: McpConfig::default(),
            pricing: None,
            spend: crate::pricing::SpendConfig::default(),
            data_dir: default_data_dir(),
            sources: Vec::new(),
            ignored_repository_keys: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    #[default]
    ChatCompletions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PromptCaching {
    #[default]
    Auto,
    Enabled,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelCapabilities {
    pub streaming: bool,
    pub tool_calling: bool,
    pub parallel_tool_calls: bool,
    pub prompt_caching: PromptCaching,
    pub reasoning_controls: bool,
    /// Whether the provider honours the `developer` role. When false, trusted
    /// task instructions are mapped into `system`.
    pub developer_role: bool,
}

impl Default for ModelCapabilities {
    fn default() -> Self {
        Self {
            streaming: true,
            tool_calling: true,
            parallel_tool_calls: true,
            prompt_caching: PromptCaching::Auto,
            reasoning_controls: false,
            developer_role: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelConfig {
    pub name: String,
    pub base_url: String,
    pub api_key_env: String,
    pub transport: Transport,
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub request_timeout_secs: u64,
    /// Attempts after the first for one request to one model, on transient
    /// errors (rate limits, timeouts, connection failures, 5xx, broken
    /// streams).
    pub max_retries: u32,
    /// First retry delay; doubles per attempt with jitter.
    pub retry_base_delay_ms: u64,
    /// Longest wait between two attempts, also capping a `Retry-After` the
    /// endpoint asks for.
    pub retry_max_delay_ms: u64,
    /// How long a model that failed over to a fallback stays demoted before
    /// it is tried first again.
    pub fallback_cooldown_secs: u64,
    /// Models tried, in order, when this one fails. Each entry inherits
    /// every unset field from this `[model]` table, so a bare `name` means
    /// "the same endpoint, another model".
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fallbacks: Vec<FallbackModel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<crate::model::ReasoningEffort>,
    pub extra_headers: BTreeMap<String, String>,
    /// When set, every request carries this header with the name of the
    /// repository the run belongs to (ASCII-sanitised), so a usage gateway
    /// can break spending down per project. `None` sends nothing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_header: Option<String>,
    /// A stored credential (`<data_dir>/credentials.toml`, see
    /// `credentials.rs`) whose key is used instead of `api_key_env`: how a
    /// signed-in ModelGateway account authenticates without an environment
    /// variable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential: Option<String>,
    pub capabilities: ModelCapabilities,
}

/// One entry of `model.fallbacks`: a model to use when the ones before it
/// fail. Unset fields come from the primary `[model]` table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct FallbackModel {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_timeout_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<crate::model::ReasoningEffort>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra_headers: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<ModelCapabilities>,
}

impl FallbackModel {
    /// A fallback on the primary endpoint, by model name alone.
    pub fn named(name: impl Into<String>) -> Self {
        Self { name: name.into(), ..Default::default() }
    }

    /// The complete model configuration this entry stands for.
    pub fn resolve(&self, primary: &ModelConfig) -> ModelConfig {
        ModelConfig {
            name: self.name.clone(),
            base_url: self.base_url.clone().unwrap_or_else(|| primary.base_url.clone()),
            api_key_env: self.api_key_env.clone().unwrap_or_else(|| primary.api_key_env.clone()),
            transport: primary.transport,
            context_window: self.context_window.unwrap_or(primary.context_window),
            max_output_tokens: self.max_output_tokens.unwrap_or(primary.max_output_tokens),
            request_timeout_secs: self.request_timeout_secs.unwrap_or(primary.request_timeout_secs),
            max_retries: self.max_retries.unwrap_or(primary.max_retries),
            retry_base_delay_ms: primary.retry_base_delay_ms,
            retry_max_delay_ms: primary.retry_max_delay_ms,
            fallback_cooldown_secs: primary.fallback_cooldown_secs,
            fallbacks: Vec::new(),
            temperature: self.temperature.or(primary.temperature),
            reasoning_effort: self.reasoning_effort.clone().or_else(|| primary.reasoning_effort.clone()),
            extra_headers: self.extra_headers.clone().unwrap_or_else(|| primary.extra_headers.clone()),
            project_header: primary.project_header.clone(),
            // A fallback on the same endpoint with the same key variable
            // shares the primary's signed-in credential; one with its own
            // endpoint or variable authenticates on its own.
            credential: self.credential.clone().or_else(|| if self.base_url.is_none() && self.api_key_env.is_none() { primary.credential.clone() } else { None }),
            capabilities: self.capabilities.clone().unwrap_or_else(|| primary.capabilities.clone()),
        }
    }
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            name: "gpt-4.1".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            api_key_env: "OPENAI_API_KEY".to_string(),
            transport: Transport::ChatCompletions,
            context_window: 128_000,
            max_output_tokens: 16_000,
            request_timeout_secs: 180,
            max_retries: 3,
            retry_base_delay_ms: 500,
            retry_max_delay_ms: 20_000,
            fallback_cooldown_secs: 60,
            fallbacks: Vec::new(),
            temperature: None,
            reasoning_effort: None,
            extra_headers: BTreeMap::new(),
            project_header: None,
            credential: None,
            capabilities: ModelCapabilities::default(),
        }
    }
}

impl ModelConfig {
    /// This model's configuration with the project header filled in for
    /// `project` (see `project_header`); unchanged when no header is
    /// configured. The value is reduced to printable ASCII and 64 bytes so
    /// it is always a valid header value.
    pub fn for_project(&self, project: &str) -> ModelConfig {
        let Some(name) = self.project_header.as_deref().map(str::trim).filter(|n| !n.is_empty()) else {
            return self.clone();
        };
        let value = sanitize_header_value(project);
        if value.is_empty() {
            return self.clone();
        }
        let mut config = self.clone();
        config.extra_headers.insert(name.to_string(), value.clone());
        for fallback in &mut config.fallbacks {
            if let Some(headers) = fallback.extra_headers.as_mut() {
                headers.insert(name.to_string(), value.clone());
            }
        }
        config
    }
}

/// Printable ASCII only (anything else becomes `_`), trimmed, at most 64
/// bytes: what an HTTP header value may carry.
pub fn sanitize_header_value(value: &str) -> String {
    let mut out: String = value.trim().chars().map(|c| if (' '..='~').contains(&c) { c } else { '_' }).collect();
    out.truncate(64);
    out.trim().to_string()
}

impl ModelConfig {
    /// Read the API key from the configured environment variable.
    pub fn api_key(&self) -> Option<String> {
        if self.api_key_env.is_empty() {
            return None;
        }
        std::env::var(&self.api_key_env).ok().map(|k| k.trim().to_string()).filter(|k| !k.is_empty())
    }

    /// Stable fingerprint of the endpoint+model used to cache capability
    /// profiles.
    pub fn fingerprint(&self) -> String {
        let text = format!("{}|{}|{:?}", self.base_url.trim_end_matches('/'), self.name, self.transport);
        let hash = crate::source::content_hash(text.as_bytes());
        hash.trim_start_matches("sha256:")[..16].to_string()
    }

    /// Identity of the endpoint+model pair, for deduplicating a chain.
    fn identity(&self) -> (String, String) {
        (self.base_url.trim_end_matches('/').to_string(), self.name.clone())
    }

    /// The primary model followed by its resolved fallbacks, in the order
    /// they are tried. A fallback that names the primary again (or an
    /// earlier fallback) is dropped, so switching the primary to a model
    /// from the fallback list never tries it twice.
    pub fn chain(&self) -> Vec<ModelConfig> {
        let mut out = vec![ModelConfig { fallbacks: Vec::new(), ..self.clone() }];
        for fb in &self.fallbacks {
            let resolved = fb.resolve(self);
            if out.iter().all(|m| m.identity() != resolved.identity()) {
                out.push(resolved);
            }
        }
        out
    }

    /// Short human-readable description of the chain, e.g.
    /// `gpt-4.1 → gpt-4.1-mini → claude-sonnet-5-5 (https://other/v1)`.
    pub fn chain_summary(&self) -> String {
        let primary_url = self.base_url.trim_end_matches('/');
        self.chain()
            .iter()
            .map(|m| if m.base_url.trim_end_matches('/') == primary_url { m.name.clone() } else { format!("{} ({})", m.name, m.base_url) })
            .collect::<Vec<_>>()
            .join(" → ")
    }
}

/// What the agent may do without asking. Three tiers: ask, run freely
/// inside the workspace, or never ask at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PermissionProfile {
    #[default]
    ApprovalRequired,
    WorkspaceAutonomous,
    /// Never prompts. Denied command prefixes, credential files and paths
    /// outside the workspace stay blocked.
    BypassPermissions,
}

impl PermissionProfile {
    pub const ALL: [PermissionProfile; 3] = [PermissionProfile::ApprovalRequired, PermissionProfile::WorkspaceAutonomous, PermissionProfile::BypassPermissions];

    pub fn as_str(&self) -> &'static str {
        match self {
            PermissionProfile::ApprovalRequired => "approval_required",
            PermissionProfile::WorkspaceAutonomous => "workspace_autonomous",
            PermissionProfile::BypassPermissions => "bypass_permissions",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "approval_required" | "approval" => Some(PermissionProfile::ApprovalRequired),
            "workspace_autonomous" | "workspace" => Some(PermissionProfile::WorkspaceAutonomous),
            "bypass_permissions" | "bypass" => Some(PermissionProfile::BypassPermissions),
            _ => None,
        }
    }
}

/// Developer-mode command wrapping. There is no user-facing sandbox mode:
/// this exists so tests and developers can run every shell command through
/// a wrapper (a container runner, a tracing shim) by configuring
/// `[permissions.sandbox]` explicitly. It never changes what is allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SandboxKind {
    #[default]
    None,
    /// A user-provided wrapper command (e.g. a container runner) that the
    /// runtime prefixes to every shell command.
    Wrapper,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SandboxConfig {
    pub kind: SandboxKind,
    /// Wrapper executable and arguments; the command is appended.
    pub wrapper: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PermissionsConfig {
    pub profile: PermissionProfile,
    /// Command prefixes always allowed without approval (e.g. `cargo test`).
    pub allowed_commands: Vec<String>,
    /// Command prefixes never allowed.
    pub denied_commands: Vec<String>,
    pub allow_network_commands: bool,
    /// Additional writable roots outside the repository.
    pub extra_workspace_roots: Vec<PathBuf>,
    pub sandbox: SandboxConfig,
}

impl Default for PermissionsConfig {
    fn default() -> Self {
        Self {
            profile: PermissionProfile::ApprovalRequired,
            allowed_commands: Vec::new(),
            denied_commands: Vec::new(),
            allow_network_commands: false,
            extra_workspace_roots: Vec::new(),
            sandbox: SandboxConfig::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct IndexConfig {
    /// Additional glob exclusions on top of `.gitignore`.
    pub exclude: Vec<String>,
    pub max_file_bytes: u64,
    pub watch: bool,
    pub include_docs: bool,
    pub index_hidden: bool,
    /// Globs that are never indexed (credential material).
    pub credential_patterns: Vec<String>,
    /// Maximum bytes per chunk before a symbol is subdivided.
    pub max_chunk_bytes: usize,
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            exclude: vec![
                "**/node_modules/**".into(),
                "**/target/**".into(),
                "**/dist/**".into(),
                "**/build/**".into(),
                "**/.git/**".into(),
                "**/vendor/**".into(),
                "**/__pycache__/**".into(),
                "**/.venv/**".into(),
                "**/venv/**".into(),
                "**/*.min.js".into(),
                "**/*.lock".into(),
                "**/package-lock.json".into(),
                "**/yarn.lock".into(),
                "**/pnpm-lock.yaml".into(),
                "**/Cargo.lock".into(),
            ],
            max_file_bytes: 1_048_576,
            watch: true,
            include_docs: true,
            index_hidden: false,
            credential_patterns: vec![
                "**/.env".into(),
                "**/.env.*".into(),
                "**/*.pem".into(),
                "**/*.key".into(),
                "**/*.p12".into(),
                "**/*.pfx".into(),
                "**/id_rsa*".into(),
                "**/id_ed25519*".into(),
                "**/.netrc".into(),
                "**/credentials".into(),
                "**/credentials.json".into(),
                "**/secrets.*".into(),
                "**/*.secret".into(),
            ],
            max_chunk_bytes: 6_000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddingBackend {
    /// Deterministic feature-hashing embedder; no model download required.
    #[default]
    Hashing,
    /// Local ONNX model (requires a model path).
    Onnx,
    /// Disable semantic retrieval.
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EmbeddingConfig {
    pub backend: EmbeddingBackend,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_path: Option<PathBuf>,
    pub dimension: usize,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self { backend: EmbeddingBackend::Hashing, model_path: None, dimension: 256 }
    }
}

/// Token budget range for one task category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetRange {
    pub min: usize,
    pub max: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ContextConfig {
    pub localized_edit: BudgetRange,
    pub multi_file_change: BudgetRange,
    pub complex_feature: BudgetRange,
    pub investigation: BudgetRange,
    /// Tokens reserved for the model's output.
    pub reserve_output_tokens: usize,
    /// Tokens reserved for tool responses in the same request cycle.
    pub reserve_tool_tokens: usize,
    /// Fraction of the context window at which compaction is triggered.
    pub compaction_threshold: f32,
    /// Maximum characters of a single tool result kept in active context.
    pub max_tool_output_chars: usize,
    /// Number of most recent conversation turns always retained.
    pub keep_recent_turns: usize,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            localized_edit: BudgetRange { min: 2_000, max: 6_000 },
            multi_file_change: BudgetRange { min: 6_000, max: 16_000 },
            complex_feature: BudgetRange { min: 12_000, max: 32_000 },
            investigation: BudgetRange { min: 4_000, max: 24_000 },
            reserve_output_tokens: 8_000,
            reserve_tool_tokens: 12_000,
            compaction_threshold: 0.75,
            max_tool_output_chars: 24_000,
            keep_recent_turns: 6,
        }
    }
}

impl ContextConfig {
    pub fn budget_for(&self, category: crate::TaskCategory) -> BudgetRange {
        match category {
            crate::TaskCategory::LocalizedEdit => self.localized_edit,
            crate::TaskCategory::MultiFileChange => self.multi_file_change,
            crate::TaskCategory::ComplexFeature => self.complex_feature,
            crate::TaskCategory::Investigation => self.investigation,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentConfig {
    /// Step budget of a task; 0 means no limit (the stall and tool-failure
    /// guards still stop a run that is going nowhere).
    pub max_steps: u32,
    pub max_recovery_cycles: u32,
    pub max_consecutive_tool_failures: u32,
    pub command_timeout_secs: u64,
    pub auto_validate: bool,
    /// Steps without new information or edits before the agent is considered
    /// stuck.
    pub stall_threshold: u32,
    /// Sub-agents the Ultra effort tier may run at the same time.
    pub max_parallel_agents: usize,
    /// Sub-agents one `spawn_agents` call may deploy.
    pub max_agents_per_call: usize,
    /// Step budget of each sub-agent; 0 means no limit.
    pub agent_max_steps: u32,
    /// Model calls that may fail outright (every retry and fallback spent)
    /// in one task before it fails. Each failure waits with backoff and
    /// repeats the step.
    pub max_model_failures: u32,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_steps: 0,
            max_recovery_cycles: 3,
            max_consecutive_tool_failures: 5,
            command_timeout_secs: 600,
            auto_validate: false,
            stall_threshold: 8,
            max_parallel_agents: 4,
            max_agents_per_call: 6,
            agent_max_steps: 0,
            max_model_failures: 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ValidationConfig {
    /// Explicit commands override auto-detection.
    pub commands: Vec<String>,
    pub auto_detect: bool,
    pub timeout_secs: u64,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self { commands: Vec::new(), auto_detect: true, timeout_secs: 900 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TelemetryConfig {
    pub retention_days: u32,
    pub redact_secrets: bool,
    pub log_level: String,
    /// Persist full tool output in the event log (otherwise truncated).
    pub store_full_tool_output: bool,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self { retention_days: 30, redact_secrets: true, log_level: "info".into(), store_full_tool_output: true }
    }
}

/// Search backend for the `web_search` tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WebSearchProvider {
    /// The signed-in ModelGateway account, else Brave when
    /// `BRAVE_SEARCH_API_KEY` is set, else Tavily when `TAVILY_API_KEY` is
    /// set, else DuckDuckGo (no key).
    #[default]
    Auto,
    /// ModelGateway's `/v1/web/search`, billed to the signed-in account.
    Gateway,
    Brave,
    Tavily,
    /// DuckDuckGo's HTML results page; needs no key but is rate limited.
    Duckduckgo,
}

impl WebSearchProvider {
    pub fn as_str(&self) -> &'static str {
        match self {
            WebSearchProvider::Auto => "auto",
            WebSearchProvider::Gateway => "modelgateway",
            WebSearchProvider::Brave => "brave",
            WebSearchProvider::Tavily => "tavily",
            WebSearchProvider::Duckduckgo => "duckduckgo",
        }
    }

    /// Environment variable holding this provider's key, if it takes one.
    pub fn default_key_env(&self) -> Option<&'static str> {
        match self {
            WebSearchProvider::Brave => Some("BRAVE_SEARCH_API_KEY"),
            WebSearchProvider::Tavily => Some("TAVILY_API_KEY"),
            WebSearchProvider::Auto | WebSearchProvider::Gateway | WebSearchProvider::Duckduckgo => None,
        }
    }
}

/// Where one search goes: a concrete provider with what it needs to
/// authenticate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchBackend {
    Gateway(crate::credentials::GatewayAccess),
    Brave { api_key: String },
    Tavily { api_key: String },
    Duckduckgo,
}

impl SearchBackend {
    pub fn provider(&self) -> WebSearchProvider {
        match self {
            SearchBackend::Gateway(_) => WebSearchProvider::Gateway,
            SearchBackend::Brave { .. } => WebSearchProvider::Brave,
            SearchBackend::Tavily { .. } => WebSearchProvider::Tavily,
            SearchBackend::Duckduckgo => WebSearchProvider::Duckduckgo,
        }
    }
}

/// The `web_search` tool. Only honoured from user configuration: a
/// repository cannot turn it on or point it at another key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WebSearchConfig {
    pub enabled: bool,
    pub provider: WebSearchProvider,
    /// Overrides the default key variable of an explicit `provider`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    /// Results returned when the model does not ask for a count.
    pub max_results: usize,
    pub timeout_secs: u64,
}

impl Default for WebSearchConfig {
    fn default() -> Self {
        Self { enabled: true, provider: WebSearchProvider::Auto, api_key_env: None, max_results: 8, timeout_secs: 20 }
    }
}

fn env_key(name: &str) -> Option<String> {
    std::env::var(name).ok().map(|k| k.trim().to_string()).filter(|k| !k.is_empty())
}

impl WebSearchConfig {
    /// The backend for the next search. `gateway` is the signed-in
    /// ModelGateway account, if any. An explicit provider that cannot
    /// authenticate is an error, not a silent fallback.
    pub fn resolve(&self, gateway: Option<&crate::credentials::GatewayAccess>) -> Result<SearchBackend> {
        match self.provider {
            WebSearchProvider::Auto => {
                if let Some(access) = gateway {
                    return Ok(SearchBackend::Gateway(access.clone()));
                }
                Ok(Self::keyed_or_duckduckgo())
            }
            WebSearchProvider::Gateway => gateway
                .cloned()
                .map(SearchBackend::Gateway)
                .ok_or_else(|| CtxError::Config("web_search.provider = \"modelgateway\" needs a ModelGateway sign-in (Account in the app)".into())),
            WebSearchProvider::Duckduckgo => Ok(SearchBackend::Duckduckgo),
            provider @ (WebSearchProvider::Brave | WebSearchProvider::Tavily) => {
                let env = self.api_key_env.as_deref().or(provider.default_key_env()).unwrap_or_default();
                let Some(api_key) = env_key(env) else {
                    return Err(CtxError::Config(format!("web_search.provider = \"{}\" needs an API key in ${env}", provider.as_str())));
                };
                Ok(if provider == WebSearchProvider::Brave { SearchBackend::Brave { api_key } } else { SearchBackend::Tavily { api_key } })
            }
        }
    }

    /// What `auto` uses without a gateway account, and after the gateway
    /// turns out not to offer search: Brave, else Tavily, else DuckDuckGo.
    pub fn keyed_or_duckduckgo() -> SearchBackend {
        if let Some(api_key) = env_key("BRAVE_SEARCH_API_KEY") {
            return SearchBackend::Brave { api_key };
        }
        if let Some(api_key) = env_key("TAVILY_API_KEY") {
            return SearchBackend::Tavily { api_key };
        }
        SearchBackend::Duckduckgo
    }
}

/// The `web_fetch` tool. User configuration only, like `[web_search]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WebFetchConfig {
    pub enabled: bool,
    pub timeout_secs: u64,
    /// Larger responses are cut off at this many bytes.
    pub max_bytes: usize,
}

impl Default for WebFetchConfig {
    fn default() -> Self {
        Self { enabled: true, timeout_secs: 30, max_bytes: 5 * 1024 * 1024 }
    }
}

/// `[mcp]`: Model Context Protocol servers whose tools the agent may call,
/// next to the built-in ones. User configuration only: a server is a
/// command this machine runs, or a URL this machine sends tool arguments
/// to, so a repository's `.context-agent.toml` can never add one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct McpConfig {
    /// `false` keeps every server unconnected and hands the model no MCP tools.
    pub enabled: bool,
    /// How long one server may take to start, initialize and list its tools.
    pub startup_timeout_secs: u64,
    /// How long one tool call may take.
    pub call_timeout_secs: u64,
    /// Servers by name; the name prefixes the server's tools (`mcp__<server>__<tool>`).
    pub servers: BTreeMap<String, McpServerConfig>,
    /// Built-in apps installed by default ([`DEFAULT_MCP_APPS`]) that the
    /// user removed; `load` would otherwise add them back.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub uninstalled: Vec<String>,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self { enabled: true, startup_timeout_secs: 30, call_timeout_secs: 120, servers: BTreeMap::new(), uninstalled: Vec::new() }
    }
}

/// Whether a tool-name pattern matches `name`: exact, or with `*` standing
/// for any run of characters (`create_*`, `*_issue`, `*`).
pub fn tool_pattern_matches(pattern: &str, name: &str) -> bool {
    if !pattern.contains('*') {
        return pattern == name;
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if !name.starts_with(first) || !name[first.len()..].ends_with(last) || name.len() < first.len() + last.len() {
        return false;
    }
    let mut rest = &name[first.len()..name.len() - last.len()];
    for middle in &parts[1..parts.len() - 1] {
        match rest.find(middle) {
            Some(i) => rest = &rest[i + middle.len()..],
            None => return false,
        }
    }
    true
}

/// MCP servers the `ctx` binary itself provides (`ctx mcp-serve <name>`),
/// configured with `transport = "builtin"` and `builtin = "<name>"`.
pub const BUILTIN_MCP_SERVERS: &[&str] = &["computer-use"];

/// Built-in apps every installation starts with: present in `mcp.servers`
/// after `load` unless the user removed them (`mcp.uninstalled`). Computer
/// Use drives the screen through Core Graphics, so only macOS gets it.
#[cfg(target_os = "macos")]
pub const DEFAULT_MCP_APPS: &[&str] = &["computer-use"];
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_MCP_APPS: &[&str] = &[];

impl McpConfig {
    /// Servers that are configured and switched on (none when the whole
    /// table is off).
    pub fn active_servers(&self) -> impl Iterator<Item = (&String, &McpServerConfig)> {
        self.servers.iter().filter(move |(_, s)| self.enabled && s.enabled)
    }

    /// Adds the default apps the user never removed.
    pub fn apply_defaults(&mut self) {
        for name in DEFAULT_MCP_APPS {
            if !self.servers.contains_key(*name) && !self.uninstalled.iter().any(|u| u == name) {
                self.servers.insert(name.to_string(), McpServerConfig::builtin(name));
            }
        }
    }

    /// Whether `name` is a default app (installed unless removed).
    pub fn is_default_app(name: &str) -> bool {
        DEFAULT_MCP_APPS.contains(&name)
    }
}

/// How a client reaches an MCP server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum McpTransport {
    /// A child process speaking JSON-RPC over its stdin and stdout.
    #[default]
    Stdio,
    /// Streamable HTTP: JSON-RPC POSTed to one URL, answers as JSON or SSE.
    Http,
    /// A server the `ctx` binary provides itself (`builtin` names it), run
    /// as a child process over stdio.
    Builtin,
}

impl McpTransport {
    pub fn as_str(&self) -> &'static str {
        match self {
            McpTransport::Stdio => "stdio",
            McpTransport::Http => "http",
            McpTransport::Builtin => "builtin",
        }
    }
}

/// One `[mcp.servers.<name>]` entry. `${VAR}` in `command`, `args`, `env`,
/// `cwd`, `url` and `headers` is replaced with that environment variable
/// when the server starts, so keys stay out of the file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct McpServerConfig {
    pub enabled: bool,
    pub transport: McpTransport,
    /// stdio: the program to run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// stdio: environment added to the process (on top of ctx's own).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// stdio: working directory (default: the repository in the CLI, the
    /// home directory in `ctx serve`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// http: the server's MCP endpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// http: headers sent with every request (`Authorization`, …).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    /// Only these of the server's tools are offered to the model (all when
    /// empty).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
    /// Tools never offered to the model, even when `tools` allows them.
    /// Both lists accept `*` wildcards (`create_*`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub exclude_tools: Vec<String>,
    /// builtin: which of [`BUILTIN_MCP_SERVERS`] this is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub builtin: Option<String>,
    /// The catalog app this entry was installed from (`None` for a server
    /// the user configured by hand).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    /// Every tool of the server runs without asking, whatever its
    /// annotations; a tool set to `ask` in `permissions` still asks.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub bypass_permissions: bool,
    /// Per-tool overrides by the server's own tool name: `allow` runs
    /// without asking, `ask` always asks (except under the bypass profile).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub permissions: BTreeMap<String, McpToolPermission>,
}

/// One MCP tool's permission override (`[mcp.servers.<name>.permissions]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpToolPermission {
    /// Runs without asking, even when the server marks it destructive.
    Allow,
    /// Asks every time, even when the server marks it read-only.
    Ask,
}

impl Default for McpServerConfig {
    fn default() -> Self {
        Self { enabled: true, transport: McpTransport::Stdio, command: None, args: Vec::new(), env: BTreeMap::new(), cwd: None, url: None, headers: BTreeMap::new(), tools: Vec::new(), exclude_tools: Vec::new(), builtin: None, app: None, bypass_permissions: false, permissions: BTreeMap::new() }
    }
}

impl McpServerConfig {
    /// A stdio server.
    pub fn stdio(command: impl Into<String>, args: Vec<String>) -> Self {
        Self { command: Some(command.into()), args, ..Self::default() }
    }

    /// A Streamable HTTP server.
    pub fn http(url: impl Into<String>) -> Self {
        Self { transport: McpTransport::Http, url: Some(url.into()), ..Self::default() }
    }

    /// One of the servers `ctx` provides itself.
    pub fn builtin(name: &str) -> Self {
        Self { transport: McpTransport::Builtin, builtin: Some(name.to_string()), app: Some(name.to_string()), ..Self::default() }
    }

    /// Whether a name may head a `[mcp.servers.<name>]` table: letters,
    /// digits, `_` and `-`, so it fits in a tool name.
    pub fn valid_name(name: &str) -> bool {
        !name.is_empty() && name.len() <= 32 && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    }

    /// Whether the tool named `name` is offered to the model: allowed by
    /// `tools` (all when empty) and not matched by `exclude_tools`.
    pub fn tool_allowed(&self, name: &str) -> bool {
        (self.tools.is_empty() || self.tools.iter().any(|p| tool_pattern_matches(p, name))) && !self.exclude_tools.iter().any(|p| tool_pattern_matches(p, name))
    }

    /// The override for the tool named `name`: its own entry in
    /// `permissions`, else `allow` when the server bypasses permissions.
    pub fn tool_permission(&self, name: &str) -> Option<McpToolPermission> {
        self.permissions.get(name).copied().or(self.bypass_permissions.then_some(McpToolPermission::Allow))
    }

    /// Whether `other` is this entry with only `bypass_permissions` or
    /// `permissions` changed, which needs no reconnect.
    pub fn differs_only_in_permissions(&self, other: &McpServerConfig) -> bool {
        McpServerConfig { bypass_permissions: other.bypass_permissions, permissions: other.permissions.clone(), ..self.clone() } == *other
    }

    /// The command line or URL, for listings.
    pub fn target(&self) -> String {
        match self.transport {
            McpTransport::Stdio => std::iter::once(self.command.clone().unwrap_or_default()).chain(self.args.iter().cloned()).collect::<Vec<_>>().join(" "),
            McpTransport::Http => self.url.clone().unwrap_or_default(),
            McpTransport::Builtin => format!("built into ctx ({})", self.builtin.clone().unwrap_or_default()),
        }
    }

    pub fn validate(&self, name: &str) -> Result<()> {
        if !Self::valid_name(name) {
            return Err(CtxError::Config(format!("mcp.servers.{name}: server names use letters, digits, `_` and `-` only (at most 32 characters)")));
        }
        match self.transport {
            McpTransport::Stdio => {
                if self.command.as_deref().map(str::trim).unwrap_or_default().is_empty() {
                    return Err(CtxError::Config(format!("mcp.servers.{name}: a stdio server needs a `command`")));
                }
            }
            McpTransport::Http => {
                let url = self.url.as_deref().map(str::trim).unwrap_or_default();
                if !(url.starts_with("http://") || url.starts_with("https://")) {
                    return Err(CtxError::Config(format!("mcp.servers.{name}: an http server needs a `url` starting with http:// or https://")));
                }
            }
            McpTransport::Builtin => {
                let builtin = self.builtin.as_deref().unwrap_or_default();
                if !BUILTIN_MCP_SERVERS.contains(&builtin) {
                    return Err(CtxError::Config(format!("mcp.servers.{name}: `builtin` must be one of {}", BUILTIN_MCP_SERVERS.join(", "))));
                }
            }
        }
        Ok(())
    }
}

/// `[pricing]`: the user's own prices (USD per million tokens). `models`
/// prices one model; the flat prices are the fallback for models with no
/// other price (see `crate::pricing`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PricingConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_per_million: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_input_per_million: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_per_million: Option<f64>,
    pub models: BTreeMap<String, crate::pricing::ModelPrice>,
}

impl PricingConfig {
    /// The flat prices, when input and output are both set.
    pub fn flat(&self) -> Option<crate::pricing::ModelPrice> {
        Some(crate::pricing::ModelPrice { input: self.input_per_million?, output: self.output_per_million?, cache_read: self.cached_input_per_million })
    }
}

/// Options controlling configuration resolution.
#[derive(Debug, Clone, Default)]
pub struct LoadOptions {
    pub explicit_path: Option<PathBuf>,
    pub repository_root: Option<PathBuf>,
    pub data_dir: Option<PathBuf>,
}

impl Config {
    pub const REPO_CONFIG_FILE: &'static str = ".context-agent.toml";

    /// Tables a repository-level `.context-agent.toml` may set. Everything
    /// touching the model endpoint, credentials, permissions, telemetry or
    /// pricing comes only from the user's own configuration, so cloning an
    /// untrusted repository can never escalate the agent's authority.
    pub const REPOSITORY_ALLOWED_TABLES: &'static [&'static str] = &["index", "context", "validation", "agent", "embeddings"];

    pub fn layout(&self) -> DataLayout {
        DataLayout::new(self.data_dir.clone())
    }

    /// Resolve configuration from all layers.
    pub fn load(opts: &LoadOptions) -> Result<Config> {
        let data_dir = opts.data_dir.clone().unwrap_or_else(default_data_dir);
        let layout = DataLayout::new(data_dir.clone());

        let mut table = toml::Table::new();
        let mut sources = Vec::new();
        let mut ignored_repository_keys = Vec::new();

        let mut layer_paths = vec![(layout.config_file(), true)];
        if let Some(root) = &opts.repository_root {
            layer_paths.push((root.join(Self::REPO_CONFIG_FILE), false));
        }
        if let Some(p) = &opts.explicit_path {
            if !p.exists() {
                return Err(CtxError::Config(format!("config file not found: {}", p.display())));
            }
            layer_paths.push((p.clone(), true));
        }

        for (path, trusted) in layer_paths {
            if path.is_file() {
                let text = std::fs::read_to_string(&path)?;
                let mut layer: toml::Table = text
                    .parse()
                    .map_err(|e: toml::de::Error| CtxError::Config(format!("{}: {}", path.display(), e)))?;
                if !trusted {
                    let keys: Vec<String> = layer.keys().cloned().collect();
                    for key in keys {
                        if !Self::REPOSITORY_ALLOWED_TABLES.contains(&key.as_str()) {
                            layer.remove(&key);
                            ignored_repository_keys.push(key);
                        }
                    }
                }
                merge_tables(&mut table, layer);
                sources.push(path);
            }
        }

        apply_env_overrides(&mut table);

        let mut config: Config = toml::Value::Table(table)
            .try_into()
            .map_err(|e: toml::de::Error| CtxError::Config(e.to_string()))?;
        config.data_dir = data_dir;
        config.sources = sources;
        config.ignored_repository_keys = ignored_repository_keys;
        config.mcp.apply_defaults();
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        if self.model.base_url.trim().is_empty() {
            return Err(CtxError::Config("model.base_url must not be empty".into()));
        }
        if self.model.name.trim().is_empty() {
            return Err(CtxError::Config("model.name must not be empty".into()));
        }
        if self.model.max_output_tokens >= self.model.context_window {
            return Err(CtxError::Config(
                "model.max_output_tokens must be smaller than model.context_window".into(),
            ));
        }
        if self.model.retry_base_delay_ms == 0 || self.model.retry_max_delay_ms < self.model.retry_base_delay_ms {
            return Err(CtxError::Config("model.retry_base_delay_ms must be positive and no larger than model.retry_max_delay_ms".into()));
        }
        for (i, fb) in self.model.fallbacks.iter().enumerate() {
            if fb.name.trim().is_empty() {
                return Err(CtxError::Config(format!("model.fallbacks[{i}].name must not be empty")));
            }
            let resolved = fb.resolve(&self.model);
            if resolved.base_url.trim().is_empty() {
                return Err(CtxError::Config(format!("model.fallbacks[{i}].base_url must not be empty")));
            }
            if resolved.max_output_tokens >= resolved.context_window {
                return Err(CtxError::Config(format!("model.fallbacks[{i}] ({}): max_output_tokens must be smaller than context_window", fb.name)));
            }
        }
        if !(0.1..=0.98).contains(&self.context.compaction_threshold) {
            return Err(CtxError::Config("context.compaction_threshold must be within 0.1..=0.98".into()));
        }
        if self.embeddings.backend == EmbeddingBackend::Onnx && self.embeddings.model_path.is_none() {
            return Err(CtxError::Config("embeddings.model_path is required for the onnx backend".into()));
        }
        for (name, range) in [
            ("localized_edit", self.context.localized_edit),
            ("multi_file_change", self.context.multi_file_change),
            ("complex_feature", self.context.complex_feature),
            ("investigation", self.context.investigation),
        ] {
            if range.min > range.max {
                return Err(CtxError::Config(format!("context.{name}: min ({}) exceeds max ({})", range.min, range.max)));
            }
        }
        if self.context.keep_recent_turns == 0 {
            return Err(CtxError::Config("context.keep_recent_turns must be at least 1".into()));
        }
        if self.agent.max_parallel_agents == 0 || self.agent.max_agents_per_call == 0 {
            return Err(CtxError::Config("agent.max_parallel_agents and agent.max_agents_per_call must be at least 1".into()));
        }
        if self.web_search.max_results == 0 || self.web_search.timeout_secs == 0 {
            return Err(CtxError::Config("web_search.max_results and web_search.timeout_secs must be at least 1".into()));
        }
        if self.web_fetch.max_bytes == 0 || self.web_fetch.timeout_secs == 0 {
            return Err(CtxError::Config("web_fetch.max_bytes and web_fetch.timeout_secs must be at least 1".into()));
        }
        if self.mcp.startup_timeout_secs == 0 || self.mcp.call_timeout_secs == 0 {
            return Err(CtxError::Config("mcp.startup_timeout_secs and mcp.call_timeout_secs must be at least 1".into()));
        }
        for (name, server) in &self.mcp.servers {
            server.validate(name)?;
        }
        let usd = |v: f64| v.is_finite() && v >= 0.0;
        if !self.spend.max_usd.map_or(true, usd) || !self.spend.models.values().all(|v| usd(*v)) {
            return Err(CtxError::Config("spend limits must be zero or more dollars".into()));
        }
        if let Some(p) = &self.pricing {
            let prices = p.models.values().flat_map(|m| [Some(m.input), Some(m.output), m.cache_read]).chain([p.input_per_million, p.cached_input_per_million, p.output_per_million]);
            if !prices.flatten().all(usd) {
                return Err(CtxError::Config("prices must be zero or more dollars per million tokens".into()));
            }
        }
        Ok(())
    }

    /// Serialize the default configuration as a commented template.
    pub fn template() -> String {
        let defaults = Config::default();
        let body = toml::to_string_pretty(&defaults).unwrap_or_default();
        format!(
            "# Context configuration.\n# Values here override built-in defaults; a repository-level\n# `.context-agent.toml` and CTX_* environment variables override this file.\n\n{body}\n\
             # Models to try, in order, when the primary model fails (after its\n\
             # retries). Unset fields inherit from [model]; a bare `name` is\n\
             # another model on the same endpoint. See docs/configuration.md.\n\
             #\n\
             # [[model.fallbacks]]\n\
             # name = \"gpt-4.1-mini\"\n\
             #\n\
             # [[model.fallbacks]]\n\
             # name = \"claude-sonnet-5-5\"\n\
             # base_url = \"https://api.anthropic.com/v1\"\n\
             # api_key_env = \"ANTHROPIC_API_KEY\"\n\
             \n\
             # Model Context Protocol servers whose tools the model may call\n\
             # (as `mcp__<server>__<tool>`). `${{VAR}}` reads an environment\n\
             # variable when the server starts. `ctx mcp add` writes these.\n\
             #\n\
             # [mcp.servers.github]\n\
             # command = \"npx\"\n\
             # args = [\"-y\", \"@modelcontextprotocol/server-github\"]\n\
             # env = {{ GITHUB_PERSONAL_ACCESS_TOKEN = \"${{GITHUB_TOKEN}}\" }}\n\
             #\n\
             # [mcp.servers.docs]\n\
             # transport = \"http\"\n\
             # url = \"https://mcp.example.com/mcp\"\n\
             # headers = {{ Authorization = \"Bearer ${{DOCS_TOKEN}}\" }}\n"
        )
    }

    pub fn write_template(path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, Config::template())?;
        Ok(())
    }
}

/// Deep-merge `over` into `base`: tables merge recursively, other values
/// replace.
pub fn merge_tables(base: &mut toml::Table, over: toml::Table) {
    for (key, value) in over {
        match (base.get_mut(&key), value) {
            (Some(toml::Value::Table(existing)), toml::Value::Table(incoming)) => {
                merge_tables(existing, incoming);
            }
            (_, value) => {
                base.insert(key, value);
            }
        }
    }
}

fn set_nested(table: &mut toml::Table, path: &[&str], value: toml::Value) {
    if path.len() == 1 {
        table.insert(path[0].to_string(), value);
        return;
    }
    let entry = table
        .entry(path[0].to_string())
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    if !entry.is_table() {
        *entry = toml::Value::Table(toml::Table::new());
    }
    set_nested(entry.as_table_mut().unwrap(), &path[1..], value);
}

fn apply_env_overrides(table: &mut toml::Table) {
    let string_overrides: &[(&str, &[&str])] = &[
        ("CTX_MODEL_NAME", &["model", "name"]),
        ("CTX_MODEL_BASE_URL", &["model", "base_url"]),
        ("CTX_MODEL_API_KEY_ENV", &["model", "api_key_env"]),
        ("CTX_PERMISSION_PROFILE", &["permissions", "profile"]),
        ("CTX_LOG_LEVEL", &["telemetry", "log_level"]),
    ];
    for (env, path) in string_overrides {
        if let Ok(v) = std::env::var(env) {
            if !v.is_empty() {
                set_nested(table, path, toml::Value::String(v));
            }
        }
    }
    let int_overrides: &[(&str, &[&str])] = &[
        ("CTX_MODEL_CONTEXT_WINDOW", &["model", "context_window"]),
        ("CTX_MODEL_MAX_OUTPUT_TOKENS", &["model", "max_output_tokens"]),
        ("CTX_MODEL_MAX_RETRIES", &["model", "max_retries"]),
        ("CTX_MODEL_FALLBACK_COOLDOWN_SECS", &["model", "fallback_cooldown_secs"]),
        ("CTX_AGENT_MAX_STEPS", &["agent", "max_steps"]),
        ("CTX_AGENT_MAX_MODEL_FAILURES", &["agent", "max_model_failures"]),
    ];
    for (env, path) in int_overrides {
        if let Ok(v) = std::env::var(env) {
            if let Ok(n) = v.parse::<i64>() {
                set_nested(table, path, toml::Value::Integer(n));
            }
        }
    }
    // `CTX_MODEL_FALLBACKS="model-a,model-b"`: fallbacks on the primary
    // endpoint, by name. Replaces a configured list; an empty value clears it.
    if let Ok(v) = std::env::var("CTX_MODEL_FALLBACKS") {
        let entries: Vec<toml::Value> = v
            .split(',')
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(|n| {
                let mut t = toml::Table::new();
                t.insert("name".into(), toml::Value::String(n.to_string()));
                toml::Value::Table(t)
            })
            .collect();
        set_nested(table, &["model", "fallbacks"], toml::Value::Array(entries));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_validate() {
        Config::default().validate().unwrap();
    }

    #[test]
    fn template_round_trips() {
        let text = Config::template();
        let parsed: Config = toml::from_str(&text).unwrap();
        assert_eq!(parsed.model.context_window, 128_000);
        assert_eq!(parsed.permissions.profile, PermissionProfile::ApprovalRequired);
    }

    #[test]
    fn mcp_tool_permissions_parse_and_resolve() {
        let text = r#"
[mcp.servers.gh]
transport = "http"
url = "https://example.com/mcp"
bypass_permissions = true
permissions = { delete_repo = "ask", list_issues = "allow" }
"#;
        let cfg: Config = toml::from_str(text).unwrap();
        let server = &cfg.mcp.servers["gh"];
        assert_eq!(server.tool_permission("delete_repo"), Some(McpToolPermission::Ask), "a tool's own setting wins over the server's bypass");
        assert_eq!(server.tool_permission("create_issue"), Some(McpToolPermission::Allow), "bypass allows the rest");
        let plain = McpServerConfig::http("https://example.com/mcp");
        assert_eq!(plain.tool_permission("create_issue"), None);
        assert!(plain.differs_only_in_permissions(server), "only the permission keys differ: no reconnect");
        assert!(!McpServerConfig::http("https://other.example/mcp").differs_only_in_permissions(server));
    }

    #[test]
    fn fallbacks_inherit_from_the_primary_and_dedupe() {
        let text = r#"
[model]
name = "primary"
base_url = "https://a.example/v1"
api_key_env = "A_KEY"
context_window = 100000
max_output_tokens = 8000
max_retries = 5

[[model.fallbacks]]
name = "second"

[[model.fallbacks]]
name = "third"
base_url = "https://b.example/v1/"
api_key_env = "B_KEY"
context_window = 200000
max_retries = 1

[[model.fallbacks]]
name = "primary"
"#;
        let cfg: Config = toml::from_str(text).unwrap();
        cfg.validate().unwrap();
        let chain = cfg.model.chain();
        assert_eq!(chain.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), vec!["primary", "second", "third"]);
        assert!(chain[0].fallbacks.is_empty());
        assert_eq!(chain[1].base_url, "https://a.example/v1");
        assert_eq!(chain[1].api_key_env, "A_KEY");
        assert_eq!(chain[1].max_retries, 5);
        assert_eq!(chain[1].context_window, 100_000);
        assert_eq!(chain[2].api_key_env, "B_KEY");
        assert_eq!(chain[2].context_window, 200_000);
        assert_eq!(chain[2].max_output_tokens, 8000);
        assert_eq!(chain[2].max_retries, 1);
        assert_ne!(chain[2].fingerprint(), chain[1].fingerprint());
        assert_eq!(cfg.model.chain_summary(), "primary → second → third (https://b.example/v1/)");

        let mut bad = cfg.clone();
        bad.model.fallbacks.push(FallbackModel { name: "tiny".into(), context_window: Some(4000), ..Default::default() });
        assert!(bad.validate().unwrap_err().to_string().contains("tiny"));
        let mut bad = cfg.clone();
        bad.model.retry_max_delay_ms = 10;
        assert!(bad.validate().is_err());
    }

    #[test]
    fn env_sets_fallbacks_by_name() {
        let mut table = toml::Table::new();
        std::env::set_var("CTX_MODEL_FALLBACKS", " alpha, ,beta ");
        std::env::set_var("CTX_MODEL_MAX_RETRIES", "7");
        apply_env_overrides(&mut table);
        std::env::remove_var("CTX_MODEL_FALLBACKS");
        std::env::remove_var("CTX_MODEL_MAX_RETRIES");
        let cfg: Config = table.try_into().unwrap();
        assert_eq!(cfg.model.fallbacks, vec![FallbackModel::named("alpha"), FallbackModel::named("beta")]);
        assert_eq!(cfg.model.max_retries, 7);
        assert_eq!(cfg.model.chain().len(), 3);
    }

    #[test]
    fn layered_loading_merges_nested_tables() {
        let data = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        std::fs::write(
            data.path().join("config.toml"),
            "[model]\nname = \"global-model\"\ncontext_window = 64000\nmax_output_tokens = 4000\n[model.capabilities]\nstreaming = false\n",
        )
        .unwrap();
        std::fs::write(
            repo.path().join(Config::REPO_CONFIG_FILE),
            "[model]\nname = \"repo-model\"\n[index]\nmax_file_bytes = 4096\n[context]\nkeep_recent_turns = 3\n",
        )
        .unwrap();
        let cfg = Config::load(&LoadOptions {
            explicit_path: None,
            repository_root: Some(repo.path().to_path_buf()),
            data_dir: Some(data.path().to_path_buf()),
        })
        .unwrap();
        // Trusted layer values survive; nested tables merge; the repository
        // layer may tune indexing/context but not the model.
        assert_eq!(cfg.model.name, "global-model");
        assert_eq!(cfg.model.context_window, 64000);
        assert!(!cfg.model.capabilities.streaming);
        assert!(cfg.model.capabilities.tool_calling);
        assert_eq!(cfg.index.max_file_bytes, 4096);
        assert_eq!(cfg.context.keep_recent_turns, 3);
        assert_eq!(cfg.ignored_repository_keys, vec!["model".to_string()]);
        assert_eq!(cfg.sources.len(), 2);
    }

    #[test]
    fn repository_config_cannot_escalate() {
        let data = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        std::fs::write(
            repo.path().join(Config::REPO_CONFIG_FILE),
            "[model]\nbase_url = \"https://attacker.example/v1\"\n[permissions]\nprofile = \"bypass_permissions\"\n[validation]\ncommands = [\"make test\"]\n[agent]\nmax_steps = 5\n",
        )
        .unwrap();
        let cfg = Config::load(&LoadOptions { explicit_path: None, repository_root: Some(repo.path().to_path_buf()), data_dir: Some(data.path().to_path_buf()) }).unwrap();
        assert_eq!(cfg.model.base_url, ModelConfig::default().base_url);
        assert_eq!(cfg.permissions.profile, PermissionProfile::ApprovalRequired);
        assert_eq!(cfg.validation.commands, vec!["make test".to_string()]);
        assert_eq!(cfg.agent.max_steps, 5);
        assert_eq!(cfg.ignored_repository_keys, vec!["model".to_string(), "permissions".to_string()]);
    }

    #[test]
    fn invalid_output_budget_is_rejected() {
        let mut cfg = Config::default();
        cfg.model.max_output_tokens = cfg.model.context_window;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn pricing_accounts_for_cached_tokens() {
        let pricing = PricingConfig { input_per_million: Some(10.0), cached_input_per_million: Some(1.0), output_per_million: Some(30.0), models: BTreeMap::new() };
        let usage = crate::model::TokenUsage {
            prompt_tokens: 1_000_000,
            completion_tokens: 0,
            cached_prompt_tokens: Some(500_000),
            reasoning_tokens: None,
            source: crate::model::UsageSource::Reported,
        };
        assert!((pricing.flat().unwrap().cost(&usage) - 5.5).abs() < 1e-9);
    }

    #[test]
    fn tool_patterns_and_filters() {
        assert!(tool_pattern_matches("create_*", "create_issue"));
        assert!(tool_pattern_matches("*_issue", "update_issue"));
        assert!(tool_pattern_matches("get_*_files", "get_pull_request_files"));
        assert!(tool_pattern_matches("*", "anything"));
        assert!(!tool_pattern_matches("create_*", "update_issue"));
        assert!(!tool_pattern_matches("a*a", "a"));
        let mut server = McpServerConfig::stdio("x", vec![]);
        server.exclude_tools = vec!["delete_*".into()];
        assert!(server.tool_allowed("read_graph") && !server.tool_allowed("delete_entities"));
        server.tools = vec!["read_*".into(), "delete_entities".into()];
        assert!(server.tool_allowed("read_graph") && !server.tool_allowed("search_nodes") && !server.tool_allowed("delete_entities"), "exclusions win");
    }

}
