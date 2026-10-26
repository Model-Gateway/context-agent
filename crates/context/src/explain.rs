//! Context inspection output (spec §8.8).

use ctx_core::{ContextStats, TaskCategory};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceExplanation {
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub symbol: Option<String>,
    pub score: f64,
    pub strategies: Vec<String>,
    pub reasons: Vec<String>,
    pub tokens: usize,
    pub refreshed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExcludedExplanation {
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub symbol: Option<String>,
    pub score: f64,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryExplanation {
    pub id: String,
    pub category: String,
    pub verification: String,
    pub score: f64,
    pub tokens: usize,
    pub preview: String,
    pub included: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ContextExplanation {
    pub task_category: Option<TaskCategory>,
    pub retrieval_confidence: Option<String>,
    pub confidence_rationale: Option<String>,
    pub query_summary: Option<String>,
    pub selected_sources: Vec<SourceExplanation>,
    pub excluded: Vec<ExcludedExplanation>,
    pub memories: Vec<MemoryExplanation>,
    pub budget: crate::budget::ContextBudget,
    pub stats: ContextStats,
    pub stable_prefix_messages: usize,
    pub prompt_cache_key: Option<String>,
    pub policy_hash: String,
    pub compacted: bool,
    pub checkpoint_id: Option<String>,
    pub pruned_tool_results: usize,
    pub truncated_tool_results: usize,
    pub index_generation: u64,
    pub stale_refreshed: usize,
    pub dropped_missing: usize,
    pub strategy_timings: Vec<(String, usize, u64)>,
    pub warnings: Vec<String>,
}

impl ContextExplanation {
    /// `ctx context explain`
    pub fn render_explain(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("Context explanation (policy {}, index generation {})\n", self.policy_hash, self.index_generation));
        if let Some(c) = &self.task_category {
            s.push_str(&format!("Task category: {}\n", c.as_str()));
        }
        if let Some(q) = &self.query_summary {
            s.push_str(&format!("Query analysis: {q}\n"));
        }
        if let Some(c) = &self.retrieval_confidence {
            s.push_str(&format!("Retrieval confidence: {c} ({})\n", self.confidence_rationale.clone().unwrap_or_default()));
        }
        s.push_str(&format!(
            "Budget: window {} | reserved output {} | reserved tools {} | stable prefix {} | instructions {} | conversation {} | retrieval {} | memory {}\n",
            self.budget.model_window, self.budget.reserve_output, self.budget.reserve_tools, self.budget.stable_prefix, self.budget.instructions, self.budget.conversation, self.budget.retrieval, self.budget.memory
        ));
        s.push_str(&format!("Estimated prompt tokens: {} (repository {} in {} snippets, memory {} in {} records, conversation {} in {} messages)\n", self.stats.estimated_prompt_tokens, self.stats.repository_tokens, self.stats.repository_snippets, self.stats.memory_tokens, self.stats.memory_records, self.stats.conversation_tokens, self.stats.conversation_messages));
        s.push_str(&format!("Cache policy: stable prefix of {} message(s){}\n", self.stable_prefix_messages, self.prompt_cache_key.as_ref().map(|k| format!(", prompt_cache_key {k}")).unwrap_or_default()));
        s.push_str(&format!("Compaction: {}{}; {} tool result(s) elided, {} truncated\n", if self.compacted { "active" } else { "none" }, self.checkpoint_id.as_ref().map(|c| format!(" (checkpoint {c})")).unwrap_or_default(), self.pruned_tool_results, self.truncated_tool_results));
        if !self.strategy_timings.is_empty() {
            s.push_str("Retrieval strategies: ");
            s.push_str(&self.strategy_timings.iter().map(|(n, hits, ms)| format!("{n}={hits} hits/{ms}ms")).collect::<Vec<_>>().join(", "));
            s.push('\n');
        }
        if self.stale_refreshed > 0 || self.dropped_missing > 0 {
            s.push_str(&format!("Index freshness: {} stale snippet(s) refreshed from disk, {} dropped as missing\n", self.stale_refreshed, self.dropped_missing));
        }
        s.push_str("\nSelected sources:\n");
        if self.selected_sources.is_empty() {
            s.push_str("  (none)\n");
        }
        for src in &self.selected_sources {
            s.push_str(&format!(
                "  {}:{}-{}{} score={:.4} tokens={} via [{}]{}{}\n",
                src.path,
                src.start_line,
                src.end_line,
                src.symbol.as_ref().map(|y| format!(" ({y})")).unwrap_or_default(),
                src.score,
                src.tokens,
                src.strategies.join(", "),
                if src.reasons.is_empty() { String::new() } else { format!(" — {}", src.reasons.join("; ")) },
                if src.refreshed { " [refreshed]" } else { "" }
            ));
        }
        if !self.excluded.is_empty() {
            s.push_str("\nExcluded candidates:\n");
            for e in self.excluded.iter().take(25) {
                s.push_str(&format!("  {}:{}-{}{} score={:.4} — {}\n", e.path, e.start_line, e.end_line, e.symbol.as_ref().map(|y| format!(" ({y})")).unwrap_or_default(), e.score, e.reason));
            }
            if self.excluded.len() > 25 {
                s.push_str(&format!("  … {} more\n", self.excluded.len() - 25));
            }
        }
        if !self.memories.is_empty() {
            s.push_str("\nMemories:\n");
            for m in &self.memories {
                s.push_str(&format!("  [{}] {} {} score={:.3} tokens={} — {}\n", if m.included { "x" } else { " " }, m.category, m.verification, m.score, m.tokens, m.preview));
            }
        }
        if !self.warnings.is_empty() {
            s.push_str("\nWarnings:\n");
            for w in &self.warnings {
                s.push_str(&format!("  - {w}\n"));
            }
        }
        s
    }

    /// `ctx context sources`
    pub fn render_sources(&self) -> String {
        let mut s = String::new();
        for src in &self.selected_sources {
            s.push_str(&format!("{}:{}-{}\t{}\t{}\n", src.path, src.start_line, src.end_line, src.symbol.clone().unwrap_or_default(), src.strategies.join("+")));
        }
        s
    }

    /// `ctx context stats`
    pub fn render_stats(&self) -> String {
        format!(
            "prompt_tokens_estimate={}\nstable_prefix_tokens={}\nrepository_snippets={}\nrepository_tokens={}\nmemory_records={}\nmemory_tokens={}\nconversation_messages={}\nconversation_tokens={}\nretrieval_budget={}\ncontext_window={}\ncompacted={}\nindex_generation={}\n",
            self.stats.estimated_prompt_tokens,
            self.stats.stable_prefix_tokens,
            self.stats.repository_snippets,
            self.stats.repository_tokens,
            self.stats.memory_records,
            self.stats.memory_tokens,
            self.stats.conversation_messages,
            self.stats.conversation_tokens,
            self.stats.retrieval_budget_tokens,
            self.stats.context_window,
            self.stats.compacted,
            self.index_generation
        )
    }
}
