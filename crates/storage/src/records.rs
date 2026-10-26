//! Row types.

use ctx_core::{Timestamp};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileRecord {
    pub path: String,
    pub language: Option<String>,
    /// `source`, `doc`, `config`, `test`, `build`
    pub kind: String,
    pub size: u64,
    pub hash: String,
    pub mtime: Option<i64>,
    pub generation: u64,
    pub indexed_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SymbolRecord {
    pub id: Option<i64>,
    pub path: String,
    pub name: String,
    pub kind: String,
    pub parent: Option<String>,
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: usize,
    pub end_line: usize,
    pub signature: Option<String>,
    pub generation: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChunkRecord {
    pub id: String,
    pub path: String,
    pub language: Option<String>,
    pub symbol: Option<String>,
    pub symbol_kind: Option<String>,
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: usize,
    pub end_line: usize,
    pub content_hash: String,
    pub file_hash: String,
    pub content: String,
    /// Normalized identifier tokens (camelCase/snake_case split) for lexical
    /// search; not stored in `chunks`, only in the FTS index.
    #[serde(default)]
    pub tokens: String,
    pub generation: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RelationshipRecord {
    pub id: Option<i64>,
    pub from_path: String,
    pub from_symbol: Option<String>,
    /// `import`, `export`, `call`, `reference`, `extends`, `implements`,
    /// `test_of`, `style_of`, `config_of`, `module`
    pub kind: String,
    /// Raw target text (module specifier, symbol name).
    pub target: String,
    pub to_path: Option<String>,
    pub to_symbol: Option<String>,
    pub confidence: f64,
    /// `parser`, `lsp`, `heuristic`
    pub source: String,
    pub generation: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmbeddingRecord {
    pub chunk_id: String,
    pub content_hash: String,
    pub model_id: String,
    pub vector: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: String,
    pub repository_id: String,
    pub mode: String,
    pub model: String,
    pub active_task_id: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskRecord {
    pub id: String,
    pub session_id: String,
    pub repository_id: String,
    pub objective: String,
    pub category: String,
    pub state: String,
    pub status: Option<String>,
    pub parent_task_id: Option<String>,
    pub summary: Option<String>,
    pub state_json: serde_json::Value,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub completed_at: Option<Timestamp>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventRecord {
    pub id: i64,
    pub session_id: Option<String>,
    pub task_id: Option<String>,
    pub kind: String,
    pub payload: serde_json::Value,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationRecord {
    pub id: i64,
    pub session_id: String,
    pub task_id: Option<String>,
    pub message: serde_json::Value,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryRow {
    pub id: String,
    pub repository_id: String,
    pub task_id: Option<String>,
    pub category: String,
    pub content: String,
    pub evidence: serde_json::Value,
    pub verification: String,
    pub tags: String,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub invalidated: bool,
    pub invalidation_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckpointRecord {
    pub id: String,
    pub task_id: String,
    pub session_id: Option<String>,
    pub reason: String,
    pub summary: serde_json::Value,
    pub event_seq: i64,
    pub conversation_seq: i64,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelRequestRecord {
    pub id: Option<i64>,
    pub task_id: Option<String>,
    pub session_id: Option<String>,
    pub step: u32,
    pub provider: String,
    pub model: String,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub cached_tokens: Option<u64>,
    pub usage_source: Option<String>,
    pub latency_ms: u64,
    pub finish_reason: Option<String>,
    pub error: Option<String>,
    pub created_at: Timestamp,
    /// USD; `None` when no price was known for the model.
    #[serde(default)]
    pub cost_usd: Option<f64>,
    /// ModelGateway's `X-Oneapi-Request-Id`, for its billed amount.
    #[serde(default)]
    pub gateway_request_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationRunRecord {
    pub id: Option<i64>,
    pub task_id: String,
    pub command: String,
    pub passed: bool,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub summary: String,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetricRecord {
    pub task_id: Option<String>,
    pub name: String,
    pub value: f64,
    pub unit: String,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct UsageTotals {
    pub requests: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cached_tokens: u64,
    pub reported_requests: u64,
    pub estimated_requests: u64,
    pub total_latency_ms: u64,
    pub errors: u64,
    /// Sum of the priced requests' costs (USD).
    #[serde(default)]
    pub cost_usd: f64,
    /// Requests with a cost; zero means the cost is unknown, not free.
    #[serde(default)]
    pub priced_requests: u64,
}

impl UsageTotals {
    /// The cost when any request was priced.
    pub fn cost(&self) -> Option<f64> {
        (self.priced_requests > 0).then_some(self.cost_usd)
    }
}

/// Spend of one model over a period (`SessionStore::spend_by_model`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelSpend {
    pub model: String,
    pub cost_usd: f64,
    pub requests: u64,
    /// Requests without a known price (their cost is missing from `cost_usd`).
    pub unpriced_requests: u64,
}
