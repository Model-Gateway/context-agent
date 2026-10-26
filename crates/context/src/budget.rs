//! Token budgeting (spec §7.11, §8.1).

use ctx_core::{ContextConfig, TaskCategory};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ContextBudget {
    pub model_window: usize,
    pub reserve_output: usize,
    pub reserve_tools: usize,
    pub stable_prefix: usize,
    pub instructions: usize,
    pub conversation: usize,
    /// Tokens available for repository + memory context after reservations.
    pub available_for_context: usize,
    /// Budget granted to retrieved source snippets.
    pub retrieval: usize,
    /// Budget granted to memory records.
    pub memory: usize,
}

impl ContextBudget {
    pub fn compute(config: &ContextConfig, category: TaskCategory, model_window: usize, max_output: usize, stable_prefix: usize, instructions: usize, conversation: usize) -> Self {
        let reserve_output = max_output.min(config.reserve_output_tokens.max(1024)).max(512);
        let reserve_tools = config.reserve_tool_tokens;
        let fixed = reserve_output + reserve_tools + stable_prefix + instructions + conversation;
        let available = model_window.saturating_sub(fixed);
        let mut range = config.budget_for(category);
        if range.min > range.max {
            range.min = range.max;
        }
        // Spend up to 60% of the headroom on retrieved source, clamped to the
        // configured range for the task category; never exceed what is free.
        let retrieval = ((available as f64 * 0.6) as usize).clamp(range.min.min(available), range.max).min(available);
        let memory = (retrieval / 5).min(1_500).min(available.saturating_sub(retrieval));
        Self { model_window, reserve_output, reserve_tools, stable_prefix, instructions, conversation, available_for_context: available, retrieval, memory }
    }

    /// Whether the compiled prompt crosses the compaction threshold.
    pub fn over_threshold(&self, config: &ContextConfig, estimated_prompt: usize) -> bool {
        estimated_prompt as f64 > self.model_window as f64 * config.compaction_threshold as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_respects_category_ranges_and_headroom() {
        let cfg = ContextConfig::default();
        let b = ContextBudget::compute(&cfg, TaskCategory::LocalizedEdit, 128_000, 16_000, 2_000, 500, 3_000);
        assert_eq!(b.retrieval, 6_000, "clamped to the localized-edit maximum");
        let b = ContextBudget::compute(&cfg, TaskCategory::ComplexFeature, 128_000, 16_000, 2_000, 500, 3_000);
        assert_eq!(b.retrieval, 32_000);
        // Tiny window: retrieval shrinks to what is free.
        let b = ContextBudget::compute(&cfg, TaskCategory::ComplexFeature, 24_000, 4_000, 2_000, 500, 1_000);
        assert!(b.retrieval <= b.available_for_context);
        assert!(b.retrieval < 12_000);
        // 0.75 * 24_000 = 18_000
        assert!(b.over_threshold(&cfg, 20_000));
        assert!(!b.over_threshold(&cfg, 10_000));
    }
}
