//! Error types shared across the workspace.

use std::time::Duration;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, CtxError>;

#[derive(Debug, Error)]
pub enum CtxError {
    #[error("configuration error: {0}")]
    Config(String),
    #[error("storage error: {0}")]
    Storage(String),
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error("tool error: {0}")]
    Tool(String),
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("stale source: {0}")]
    Stale(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("operation cancelled")]
    Cancelled,
    #[error("timed out after {0:?}")]
    Timeout(Duration),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("index error: {0}")]
    Index(String),
    #[error("{0}")]
    Other(String),
}

impl CtxError {
    pub fn other(msg: impl Into<String>) -> Self {
        CtxError::Other(msg.into())
    }

    pub fn tool(msg: impl Into<String>) -> Self {
        CtxError::Tool(msg.into())
    }

    pub fn storage(msg: impl Into<String>) -> Self {
        CtxError::Storage(msg.into())
    }

    pub fn config(msg: impl Into<String>) -> Self {
        CtxError::Config(msg.into())
    }

    pub fn invalid(msg: impl Into<String>) -> Self {
        CtxError::InvalidArgument(msg.into())
    }

    /// Stable machine-readable code for headless/JSON output (spec §12.3).
    pub fn code(&self) -> &'static str {
        match self {
            CtxError::Config(_) => "config",
            CtxError::Storage(_) => "storage",
            CtxError::Provider(p) => p.code(),
            CtxError::Tool(_) => "tool",
            CtxError::PermissionDenied(_) => "permission_denied",
            CtxError::InvalidArgument(_) => "invalid_argument",
            CtxError::NotFound(_) => "not_found",
            CtxError::Stale(_) => "stale_source",
            CtxError::Conflict(_) => "conflict",
            CtxError::Cancelled => "cancelled",
            CtxError::Timeout(_) => "timeout",
            CtxError::Io(_) => "io",
            CtxError::Serde(_) => "serialization",
            CtxError::Index(_) => "index",
            CtxError::Other(_) => "other",
        }
    }

    /// Stable process exit code for headless mode (spec §12.3).
    pub fn exit_code(&self) -> i32 {
        match self {
            CtxError::Config(_) => 2,
            CtxError::InvalidArgument(_) => 2,
            CtxError::PermissionDenied(_) => 3,
            CtxError::Provider(_) => 4,
            CtxError::Timeout(_) => 5,
            CtxError::Cancelled => 130,
            _ => 1,
        }
    }
}

impl From<toml::de::Error> for CtxError {
    fn from(e: toml::de::Error) -> Self {
        CtxError::Config(e.to_string())
    }
}

/// Errors raised by model providers, normalized across transports (spec §5.8).
#[derive(Debug, Error, Clone)]
pub enum ProviderError {
    #[error("authentication failed: {0}")]
    Auth(String),
    #[error("rate limited{}", retry_after.map(|d| format!(" (retry after {d:?})")).unwrap_or_default())]
    RateLimited { retry_after: Option<Duration> },
    #[error("provider request timed out")]
    Timeout,
    #[error("connection failure: {0}")]
    Connection(String),
    #[error("stream interrupted: {0}")]
    InterruptedStream(String),
    #[error("context length exceeded: {0}")]
    ContextLength(String),
    #[error("invalid provider response: {0}")]
    InvalidResponse(String),
    #[error("model returned malformed tool call: {0}")]
    MalformedToolCall(String),
    #[error("provider api error {status}: {message}")]
    Api { status: u16, code: Option<String>, message: String },
    /// ModelGateway's provider route failed on the provider's side (its
    /// balance, quota, credentials or availability), not because of the
    /// request or the user's own balance: another attempt may be routed to
    /// another provider. `category` is the gateway's `error.details.category`.
    #[error("provider api error {status}: {message}")]
    UpstreamRoute { status: u16, category: String, message: String },
    #[error("capability not supported by provider: {0}")]
    Unsupported(String),
    #[error("retries exhausted after {attempts} attempts: {last}")]
    RetriesExhausted { attempts: u32, last: String },
    #[error("all {attempted} configured models failed; last error: {last}")]
    AllModelsFailed { attempted: u32, last: String },
    /// ModelGateway refused the request (`discount_drop`, HTTP 412) because
    /// every remaining provider is much less discounted than the model's
    /// best offer; resent without the guard header it goes ahead. Discounts
    /// are whole percentages off the official output price.
    #[error("discount dropped: {message}")]
    DiscountDrop { expected: Option<f64>, available: Option<f64>, message: String },
}

impl ProviderError {
    pub fn is_retryable(&self) -> bool {
        match self {
            ProviderError::RateLimited { .. }
            | ProviderError::Timeout
            | ProviderError::Connection(_)
            | ProviderError::InterruptedStream(_)
            | ProviderError::UpstreamRoute { .. } => true,
            // 200: an unclassified error inside an accepted response (the
            // upstream failed mid-request, e.g. "upstream unavailable").
            ProviderError::Api { status, .. } => *status >= 500 || matches!(*status, 200 | 408 | 409),
            _ => false,
        }
    }

    /// Whether another model in the fallback chain should be tried after
    /// this error. Everything except the errors another model would hit
    /// identically or that the runtime handles itself: a prompt that is too
    /// long is compacted and resent, and a malformed tool call is reported
    /// back to the model.
    pub fn triggers_fallback(&self) -> bool {
        !matches!(self, ProviderError::ContextLength(_) | ProviderError::MalformedToolCall(_) | ProviderError::DiscountDrop { .. })
    }

    /// Whether the error is worth repeating the whole step for, after a
    /// pause, once every retry and fallback is spent: transient conditions
    /// and outages, not a rejected key or an unsupported request.
    pub fn is_transient(&self) -> bool {
        match self {
            ProviderError::RetriesExhausted { .. } | ProviderError::AllModelsFailed { .. } | ProviderError::InvalidResponse(_) => true,
            other => other.is_retryable(),
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            ProviderError::Auth(_) => "provider_auth",
            ProviderError::RateLimited { .. } => "provider_rate_limited",
            ProviderError::Timeout => "provider_timeout",
            ProviderError::Connection(_) => "provider_connection",
            ProviderError::InterruptedStream(_) => "provider_stream_interrupted",
            ProviderError::ContextLength(_) => "provider_context_length",
            ProviderError::InvalidResponse(_) => "provider_invalid_response",
            ProviderError::MalformedToolCall(_) => "provider_malformed_tool_call",
            ProviderError::Api { .. } => "provider_api",
            ProviderError::UpstreamRoute { .. } => "provider_upstream_route",
            ProviderError::Unsupported(_) => "provider_unsupported",
            ProviderError::RetriesExhausted { .. } => "provider_retries_exhausted",
            ProviderError::AllModelsFailed { .. } => "provider_all_models_failed",
            ProviderError::DiscountDrop { .. } => "provider_discount_drop",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryable_classification() {
        assert!(ProviderError::RateLimited { retry_after: None }.is_retryable());
        assert!(ProviderError::Api { status: 503, code: None, message: String::new() }.is_retryable());
        assert!(!ProviderError::Api { status: 400, code: None, message: String::new() }.is_retryable());
        assert!(ProviderError::Api { status: 200, code: None, message: "upstream unavailable".into() }.is_retryable());
        assert!(!ProviderError::ContextLength("x".into()).is_retryable());
        let upstream = ProviderError::UpstreamRoute { status: 402, category: "provider_balance".into(), message: String::new() };
        assert!(upstream.is_retryable() && upstream.triggers_fallback() && upstream.is_transient());
    }

    #[test]
    fn fallback_and_transient_classification() {
        assert!(ProviderError::Auth("bad key".into()).triggers_fallback());
        assert!(ProviderError::Api { status: 404, code: None, message: "no such model".into() }.triggers_fallback());
        assert!(ProviderError::RetriesExhausted { attempts: 4, last: "timeout".into() }.triggers_fallback());
        assert!(!ProviderError::ContextLength("x".into()).triggers_fallback());
        assert!(!ProviderError::MalformedToolCall("x".into()).triggers_fallback());
        assert!(!ProviderError::DiscountDrop { expected: Some(98.0), available: Some(0.0), message: String::new() }.triggers_fallback());
        assert!(!ProviderError::DiscountDrop { expected: None, available: None, message: String::new() }.is_transient());
        assert!(ProviderError::AllModelsFailed { attempted: 2, last: "x".into() }.is_transient());
        assert!(ProviderError::Timeout.is_transient());
        assert!(!ProviderError::Auth("x".into()).is_transient());
        assert!(!ProviderError::Api { status: 400, code: None, message: String::new() }.is_transient());
    }

    #[test]
    fn exit_codes_are_stable() {
        assert_eq!(CtxError::Cancelled.exit_code(), 130);
        assert_eq!(CtxError::PermissionDenied("x".into()).exit_code(), 3);
    }
}
