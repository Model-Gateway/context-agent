//! Retrieval data contracts (spec §7.9, §7.12).

use ctx_core::{SourceRef, TaskCategory, VerifiedSourceSnippet};
use ctx_memory::MemoryRecord;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StrategyKind {
    Path,
    Lexical,
    Symbol,
    Semantic,
    Memory,
    Expansion,
    WorkingSet,
}

impl StrategyKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            StrategyKind::Path => "path",
            StrategyKind::Lexical => "lexical",
            StrategyKind::Symbol => "symbol",
            StrategyKind::Semantic => "semantic",
            StrategyKind::Memory => "memory",
            StrategyKind::Expansion => "expansion",
            StrategyKind::WorkingSet => "working_set",
        }
    }
}

/// One strategy's vote for a candidate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Signal {
    pub strategy: StrategyKind,
    /// 1-based rank within that strategy's result list.
    pub rank: usize,
    /// Strategy-native score (BM25, cosine, match quality).
    pub score: f64,
    pub detail: String,
}

/// A candidate source range before verification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceCandidate {
    pub chunk_id: Option<String>,
    pub path: String,
    pub symbol: Option<String>,
    pub symbol_kind: Option<String>,
    pub language: Option<String>,
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: usize,
    pub end_line: usize,
    pub content_hash: String,
    pub file_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    pub signals: Vec<Signal>,
    pub fused_score: f64,
    pub reasons: Vec<String>,
}

impl SourceCandidate {
    pub fn source_ref(&self) -> SourceRef {
        SourceRef {
            path: self.path.clone(),
            start_byte: self.start_byte,
            end_byte: self.end_byte,
            start_line: self.start_line,
            end_line: self.end_line,
            content_hash: self.content_hash.clone(),
            file_hash: self.file_hash.clone(),
        }
    }

    pub fn key(&self) -> String {
        format!("{}:{}:{}", self.path, self.start_byte, self.end_byte)
    }

    pub fn has_strategy(&self, kind: StrategyKind) -> bool {
        self.signals.iter().any(|s| s.strategy == kind)
    }

    pub fn strategies(&self) -> Vec<StrategyKind> {
        let mut v: Vec<StrategyKind> = self.signals.iter().map(|s| s.strategy).collect();
        v.sort();
        v.dedup();
        v
    }

    pub fn rationale(&self) -> String {
        let mut parts: Vec<String> = self.signals.iter().map(|s| format!("{}#{}", s.strategy.as_str(), s.rank)).collect();
        parts.extend(self.reasons.iter().cloned());
        parts.join(", ")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalConfidence {
    High,
    Medium,
    Low,
}

impl RetrievalConfidence {
    pub fn as_str(&self) -> &'static str {
        match self {
            RetrievalConfidence::High => "high",
            RetrievalConfidence::Medium => "medium",
            RetrievalConfidence::Low => "low",
        }
    }

    /// Guidance injected into the context for the model (spec §7.9).
    pub fn guidance(&self) -> &'static str {
        match self {
            RetrievalConfidence::High => "Retrieval confidence is HIGH: the snippets below are very likely the right locations. You may proceed to implement after confirming the exact current text of any range you edit.",
            RetrievalConfidence::Medium => "Retrieval confidence is MEDIUM: several plausible locations are included. Inspect the most relevant ones (and their dependencies) before editing.",
            RetrievalConfidence::Low => "Retrieval confidence is LOW: treat the snippets as leads only. Investigate with search_code, find_symbol and read_file before making changes.",
        }
    }
}

/// Progressive retrieval stages (spec §7.10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalStage {
    #[default]
    Initial,
    Focused,
    Expanded,
    Recovery,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct StrategyDiagnostics {
    pub strategy: String,
    pub hits: usize,
    pub duration_ms: u64,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExcludedCandidate {
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub symbol: Option<String>,
    pub fused_score: f64,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct RetrievalDiagnostics {
    pub strategies: Vec<StrategyDiagnostics>,
    pub candidates_before_fusion: usize,
    pub candidates_after_fusion: usize,
    pub expanded: usize,
    pub stale_refreshed: usize,
    pub dropped_missing: usize,
    pub excluded: Vec<ExcludedCandidate>,
    pub budget_tokens: usize,
    pub used_tokens: usize,
    pub total_ms: u64,
    pub stage: RetrievalStage,
    pub category: TaskCategory,
    pub confidence_rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalResult {
    /// All ranked candidates after fusion (content omitted).
    pub candidates: Vec<SourceCandidate>,
    /// Verified snippets selected under the budget, in rank order.
    pub context: Vec<VerifiedSourceSnippet>,
    pub memories: Vec<(MemoryRecord, f64)>,
    pub confidence: RetrievalConfidence,
    pub index_generation: u64,
    pub diagnostics: RetrievalDiagnostics,
    pub analysis: crate::query::QueryAnalysis,
}

impl RetrievalResult {
    pub fn empty(analysis: crate::query::QueryAnalysis, generation: u64) -> Self {
        Self { candidates: vec![], context: vec![], memories: vec![], confidence: RetrievalConfidence::Low, index_generation: generation, diagnostics: RetrievalDiagnostics::default(), analysis }
    }

    pub fn paths(&self) -> Vec<String> {
        let mut v: Vec<String> = self.context.iter().map(|s| s.source.path.clone()).collect();
        v.dedup();
        v
    }
}
