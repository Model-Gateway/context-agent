//! Benchmark results.

use crate::stats::{mean, wilson_interval};
use crate::variant::Variant;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskRunResult {
    pub task_id: String,
    pub variant: Variant,
    pub category: String,
    pub languages: Vec<String>,
    pub skipped: Option<String>,
    pub success: bool,
    /// A validation command that passed before the agent ran fails after.
    pub regression: bool,
    pub validation_before: Vec<(String, bool)>,
    pub validation_after: Vec<(String, bool)>,
    pub turns: usize,
    pub steps: u32,
    pub model_calls: u32,
    pub tool_calls: u32,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cached_tokens: u64,
    pub cost_usd: Option<f64>,
    pub duration_ms: u64,
    pub recovery_cycles: u64,
    pub model_driven_reads: u64,
    pub model_driven_searches: u64,
    /// Fraction of expected files that appeared in proactive context.
    pub retrieval_recall: Option<f64>,
    /// Fraction of expected files the agent actually modified.
    pub expected_modified: Option<f64>,
    pub modified_files: Vec<String>,
    pub agent_status: String,
    pub error: Option<String>,
    /// The run was cut short by the environment (provider outage, rate
    /// limit, runtime error), not by the agent failing the task. Such runs
    /// are excluded from success statistics and can be repeated with
    /// `ctx eval run --resume`.
    #[serde(default)]
    pub infra_error: Option<String>,
}

impl TaskRunResult {
    /// Whether this result should be repeated when resuming: skipped runs
    /// (a tool may have been installed since) and runs that hit an
    /// environment error. Old reports have no `infra_error` field, so a
    /// `failed`/`error` agent status (only produced by runtime errors) also
    /// counts.
    pub fn needs_rerun(&self) -> bool {
        self.skipped.is_some() || self.infra_error.is_some() || matches!(self.agent_status.as_str(), "failed" | "error")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VariantSummary {
    pub variant: Variant,
    pub n: u64,
    pub skipped: u64,
    /// Runs excluded because of environment errors (see `infra_error`).
    #[serde(default)]
    pub errors: u64,
    pub successes: u64,
    pub success_rate: f64,
    pub success_ci95: (f64, f64),
    pub regressions: u64,
    pub regression_rate: f64,
    pub mean_steps: f64,
    pub mean_model_calls: f64,
    pub mean_prompt_tokens: f64,
    pub mean_completion_tokens: f64,
    pub mean_cached_tokens: f64,
    pub mean_duration_ms: f64,
    pub mean_model_driven_reads: f64,
    pub mean_model_driven_searches: f64,
    pub mean_retrieval_recall: Option<f64>,
    pub total_cost_usd: Option<f64>,
    pub cost_per_success_usd: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkReport {
    pub generated_at: String,
    pub model: String,
    pub provider: String,
    pub policy_hash: String,
    pub tasks: usize,
    pub summaries: Vec<VariantSummary>,
    pub results: Vec<TaskRunResult>,
    pub config: serde_json::Value,
}

impl BenchmarkReport {
    pub fn summarize(results: &[TaskRunResult]) -> Vec<VariantSummary> {
        let mut by_variant: BTreeMap<String, Vec<&TaskRunResult>> = BTreeMap::new();
        for r in results {
            by_variant.entry(r.variant.name().to_string()).or_default().push(r);
        }
        by_variant
            .into_values()
            .map(|rs| {
                let variant = rs[0].variant;
                let ran: Vec<&&TaskRunResult> = rs.iter().filter(|r| r.skipped.is_none() && r.infra_error.is_none()).collect();
                let errors = rs.iter().filter(|r| r.skipped.is_none() && r.infra_error.is_some()).count() as u64;
                let n = ran.len() as u64;
                let successes = ran.iter().filter(|r| r.success).count() as u64;
                let regressions = ran.iter().filter(|r| r.regression).count() as u64;
                let recall: Vec<f64> = ran.iter().filter_map(|r| r.retrieval_recall).collect();
                let costs: Vec<f64> = ran.iter().filter_map(|r| r.cost_usd).collect();
                let total_cost = if costs.is_empty() { None } else { Some(costs.iter().sum()) };
                VariantSummary {
                    variant,
                    n,
                    skipped: (rs.len() as u64) - n - errors,
                    errors,
                    successes,
                    success_rate: if n == 0 { 0.0 } else { successes as f64 / n as f64 },
                    success_ci95: wilson_interval(successes, n, 1.96),
                    regressions,
                    regression_rate: if n == 0 { 0.0 } else { regressions as f64 / n as f64 },
                    mean_steps: mean(ran.iter().map(|r| r.steps as f64)),
                    mean_model_calls: mean(ran.iter().map(|r| r.model_calls as f64)),
                    mean_prompt_tokens: mean(ran.iter().map(|r| r.prompt_tokens as f64)),
                    mean_completion_tokens: mean(ran.iter().map(|r| r.completion_tokens as f64)),
                    mean_cached_tokens: mean(ran.iter().map(|r| r.cached_tokens as f64)),
                    mean_duration_ms: mean(ran.iter().map(|r| r.duration_ms as f64)),
                    mean_model_driven_reads: mean(ran.iter().map(|r| r.model_driven_reads as f64)),
                    mean_model_driven_searches: mean(ran.iter().map(|r| r.model_driven_searches as f64)),
                    mean_retrieval_recall: if recall.is_empty() { None } else { Some(mean(recall.into_iter())) },
                    total_cost_usd: total_cost,
                    cost_per_success_usd: total_cost.filter(|_| successes > 0).map(|c| c / successes as f64),
                }
            })
            .collect()
    }

    pub fn render_markdown(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("# Benchmark report\n\nGenerated {} with model `{}` ({}), policy {}; {} task(s).\n\n", self.generated_at, self.model, self.provider, self.policy_hash, self.tasks));
        s.push_str("| Variant | n | success | 95% CI | regressions | env errors | steps | model calls | prompt tok | cached tok | reads | searches | recall | duration (s) |\n|---|---|---|---|---|---|---|---|---|---|---|---|---|---|\n");
        for v in &self.summaries {
            s.push_str(&format!(
                "| {} | {} | {:.1}% ({}) | {:.1}–{:.1}% | {} | {} | {:.1} | {:.1} | {:.0} | {:.0} | {:.1} | {:.1} | {} | {:.1} |\n",
                v.variant.name(),
                v.n,
                v.success_rate * 100.0,
                v.successes,
                v.success_ci95.0 * 100.0,
                v.success_ci95.1 * 100.0,
                v.regressions,
                v.errors,
                v.mean_steps,
                v.mean_model_calls,
                v.mean_prompt_tokens,
                v.mean_cached_tokens,
                v.mean_model_driven_reads,
                v.mean_model_driven_searches,
                v.mean_retrieval_recall.map(|r| format!("{:.0}%", r * 100.0)).unwrap_or_else(|| "-".into()),
                v.mean_duration_ms / 1000.0
            ));
        }
        s.push_str("\n## Per task\n\n| task | variant | result | status | steps | validation after | error |\n|---|---|---|---|---|---|---|\n");
        for r in &self.results {
            s.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} | {} |\n",
                r.task_id,
                r.variant.name(),
                match (&r.skipped, &r.infra_error) {
                    (Some(reason), _) => format!("skipped ({reason})"),
                    (None, Some(_)) => "env error (excluded)".into(),
                    (None, None) if r.success => "pass".into(),
                    (None, None) => "fail".into(),
                },
                r.agent_status,
                r.steps,
                r.validation_after.iter().map(|(c, ok)| format!("{} {}", if *ok { "ok" } else { "FAIL" }, c.chars().take(40).collect::<String>())).collect::<Vec<_>>().join("; "),
                r.infra_error.clone().or_else(|| r.error.clone()).unwrap_or_default().replace('|', "/").chars().take(100).collect::<String>()
            ));
        }
        s.push_str("\nSuccess = every validation command exits 0 after the run, judged independently of the agent's own report. Regression = a command that passed before the run fails afterwards. Env errors = runs cut short by the provider or runtime (outages, rate limits); they are excluded from n and can be repeated with `ctx eval run --resume <report.json>`. Confidence intervals are Wilson score intervals; prefer non-inferiority tests against the baseline before claiming improvements.\n");
        s
    }
}
