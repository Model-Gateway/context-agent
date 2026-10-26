//! Task metrics (spec §14.1).

use ctx_core::{Result, TaskId, Timestamp};
use ctx_storage::{MetricRecord, SessionStore};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

/// Aggregated metrics for one task, derived from persisted records so that
/// they survive restarts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TaskMetrics {
    pub task_id: String,
    pub success: Option<bool>,
    pub model_invocations: u64,
    pub tool_invocations: u64,
    pub files_retrieved_proactively: u64,
    pub model_driven_file_reads: u64,
    pub search_calls: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cached_tokens: u64,
    pub usage_reported_requests: u64,
    pub usage_estimated_requests: u64,
    pub model_latency_ms_total: u64,
    pub retrieval_latency_ms: BTreeMap<String, f64>,
    pub validation_attempts: u64,
    pub validation_passed: u64,
    pub recovery_cycles: u64,
    pub duration_ms: u64,
    pub cost_usd: Option<f64>,
    pub custom: BTreeMap<String, f64>,
}

/// In-memory counters flushed to storage per task.
#[derive(Clone)]
pub struct MetricsCollector {
    store: Arc<SessionStore>,
    task_id: TaskId,
    started: Instant,
    counters: Arc<Mutex<BTreeMap<String, f64>>>,
}

impl MetricsCollector {
    pub fn new(store: Arc<SessionStore>, task_id: TaskId) -> Self {
        Self { store, task_id, started: Instant::now(), counters: Arc::new(Mutex::new(BTreeMap::new())) }
    }

    pub fn increment(&self, name: &str) {
        self.add(name, 1.0);
    }

    pub fn add(&self, name: &str, value: f64) {
        let mut c = self.counters.lock();
        *c.entry(name.to_string()).or_insert(0.0) += value;
    }

    pub fn set(&self, name: &str, value: f64) {
        self.counters.lock().insert(name.to_string(), value);
    }

    pub fn get(&self, name: &str) -> f64 {
        self.counters.lock().get(name).copied().unwrap_or(0.0)
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    /// Record a timing sample under `name` (milliseconds, averaged on read).
    pub fn record_latency_ms(&self, name: &str, ms: f64) {
        self.add(&format!("{name}.total_ms"), ms);
        self.add(&format!("{name}.count"), 1.0);
    }

    /// Persist all counters as metric rows.
    pub fn flush(&self) -> Result<()> {
        let snapshot: Vec<(String, f64)> = self.counters.lock().iter().map(|(k, v)| (k.clone(), *v)).collect();
        let now: Timestamp = ctx_core::now();
        for (name, value) in snapshot {
            let unit = if name.ends_with("_ms") { "ms" } else if name.contains("tokens") { "tokens" } else { "count" };
            self.store.insert_metric(&MetricRecord { task_id: Some(self.task_id.to_string()), name, value, unit: unit.into(), created_at: now })?;
        }
        self.store.insert_metric(&MetricRecord {
            task_id: Some(self.task_id.to_string()),
            name: "task.duration_ms".into(),
            value: self.elapsed_ms() as f64,
            unit: "ms".into(),
            created_at: now,
        })?;
        Ok(())
    }

    /// Build the aggregated view for a task from storage.
    pub fn summarize(store: &SessionStore, task_id: &str, cost_usd: Option<f64>) -> Result<TaskMetrics> {
        let usage = store.usage_totals(Some(task_id), None)?;
        let validations = store.validation_runs_for_task(task_id)?;
        let metrics = store.metrics_for_task(task_id)?;
        // Counters are flushed once per run; a continued task has several
        // runs, so aggregate by summing every row of a name.
        let mut sums: BTreeMap<String, f64> = BTreeMap::new();
        for m in &metrics {
            *sums.entry(m.name.clone()).or_insert(0.0) += m.value;
        }
        let mut custom = BTreeMap::new();
        let mut retrieval_latency_ms = BTreeMap::new();
        let mut duration_ms = 0;
        for (name, value) in &sums {
            if name == "task.duration_ms" {
                duration_ms = *value as u64;
            } else if let Some(strategy) = name.strip_prefix("retrieval.").and_then(|s| s.strip_suffix(".total_ms")) {
                let count = sums.get(&format!("retrieval.{strategy}.count")).copied().unwrap_or(1.0);
                retrieval_latency_ms.insert(strategy.to_string(), value / count.max(1.0));
            } else if !name.ends_with(".count") {
                custom.insert(name.clone(), *value);
            }
        }
        let task = store.get_task(task_id)?;
        Ok(TaskMetrics {
            task_id: task_id.to_string(),
            success: task.as_ref().and_then(|t| t.status.as_deref().map(|s| s == "completed")),
            model_invocations: usage.requests,
            tool_invocations: store.count_events(task_id, Some("tool_call_finished"))?,
            files_retrieved_proactively: custom.get("retrieval.proactive_snippets").copied().unwrap_or(0.0) as u64,
            model_driven_file_reads: custom.get("tool.read_file").copied().unwrap_or(0.0) as u64,
            search_calls: custom.get("tool.search_code").copied().unwrap_or(0.0) as u64,
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            cached_tokens: usage.cached_tokens,
            usage_reported_requests: usage.reported_requests,
            usage_estimated_requests: usage.estimated_requests,
            model_latency_ms_total: usage.total_latency_ms,
            retrieval_latency_ms,
            validation_attempts: validations.len() as u64,
            validation_passed: validations.iter().filter(|v| v.passed).count() as u64,
            recovery_cycles: custom.get("agent.recovery_cycles").copied().unwrap_or(0.0) as u64,
            duration_ms,
            cost_usd,
            custom,
        })
    }
}
