//! Benchmark execution.

use crate::report::{BenchmarkReport, TaskRunResult};
use crate::spec::BenchmarkTask;
use crate::variant::Variant;
use ctx_agent::{AgentRuntime, RepositoryContext};
use ctx_core::{CancellationToken, Config, Result, TaskStatus};
use ctx_providers::ModelProvider;
use ctx_telemetry::MetricsCollector;
use ctx_tools::AutoApprove;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub type ProviderFactory = Arc<dyn Fn() -> Arc<dyn ModelProvider> + Send + Sync>;

#[derive(Clone)]
pub struct BenchmarkOptions {
    pub variants: Vec<Variant>,
    /// Workspace root that task `repository` paths are relative to.
    pub workspace_root: PathBuf,
    pub base_config: Config,
    pub provider: ProviderFactory,
    pub task_filter: Option<Vec<String>>,
    /// Keep snapshot directories for inspection.
    pub keep_snapshots: bool,
    pub progress: Option<Arc<dyn Fn(String) + Send + Sync>>,
    /// Results from earlier reports. A (task, variant) pair with a valid
    /// previous result is not run again; skipped and environment-error runs
    /// are repeated.
    pub previous: Vec<TaskRunResult>,
}

/// Copy a fixture into a fresh directory, skipping build artefacts.
pub fn snapshot_repository(src: &Path, dest: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if matches!(name_str.as_ref(), "target" | "node_modules" | "__pycache__" | ".git" | "build" | "dist" | "vendor") {
            continue;
        }
        let path = entry.path();
        let to = dest.join(&name);
        if path.is_dir() {
            snapshot_repository(&path, &to)?;
        } else {
            std::fs::copy(&path, &to)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&to, std::fs::Permissions::from_mode(entry.metadata()?.permissions().mode()));
            }
        }
    }
    Ok(())
}

fn run_validation(dir: &Path, command: &str, timeout: Duration) -> bool {
    let shell = ctx_core::platform::default_shell();
    let invocation = shell.invocation(command);
    let mut cmd = std::process::Command::new(&shell.program);
    cmd.args(&invocation.args);
    #[cfg(windows)]
    if let Some(tail) = &invocation.raw_tail {
        use std::os::windows::process::CommandExt;
        cmd.raw_arg(tail);
    }
    ctx_core::platform::hide_console_window(&mut cmd);
    let child = cmd.current_dir(dir).env("NO_COLOR", "1").env("GIT_TERMINAL_PROMPT", "0").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
    let Ok(mut child) = child else { return false };
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) => {
                if started.elapsed() > timeout {
                    let _ = child.kill();
                    return false;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(_) => return false,
        }
    }
}

fn note(opts: &BenchmarkOptions, msg: String) {
    if let Some(p) = &opts.progress {
        p(msg);
    }
}

/// Run every task under every variant.
pub async fn run_benchmark(tasks: &[BenchmarkTask], opts: &BenchmarkOptions) -> Result<BenchmarkReport> {
    let mut results = Vec::new();
    let mut model = String::new();
    let mut provider_name = String::new();
    for task in tasks {
        if let Some(filter) = &opts.task_filter {
            if !filter.iter().any(|f| f == &task.id) {
                continue;
            }
        }
        let missing: Vec<String> = task.tools_available().into_iter().filter(|(_, ok)| !ok).map(|(t, _)| t).collect();
        for variant in &opts.variants {
            if let Some(prev) = opts.previous.iter().rev().find(|r| r.task_id == task.id && r.variant == *variant) {
                if !prev.needs_rerun() {
                    note(opts, format!("{} / {} (kept from previous report)", task.id, variant.name()));
                    results.push(prev.clone());
                    continue;
                }
            }
            if !missing.is_empty() {
                results.push(skipped(task, *variant, format!("missing tools: {}", missing.join(", "))));
                continue;
            }
            note(opts, format!("{} / {}", task.id, variant.name()));
            let src = opts.workspace_root.join(&task.repository);
            if !src.is_dir() {
                results.push(skipped(task, *variant, format!("repository not found: {}", src.display())));
                continue;
            }
            let snapshot = tempfile_dir(&format!("ctx-bench-{}-{}", task.id, variant.name()))?;
            snapshot_repository(&src, &snapshot)?;
            let validation_before: Vec<(String, bool)> = task.validation.iter().map(|c| (c.clone(), run_validation(&snapshot, c, Duration::from_secs(task.timeout_secs)))).collect();

            let config = variant.config(&opts.base_config);
            let provider = (opts.provider)();
            model = provider.model().to_string();
            provider_name = provider.name().to_string();
            let outcome = run_one(task, *variant, &snapshot, config, provider).await;
            let validation_after: Vec<(String, bool)> = task.validation.iter().map(|c| (c.clone(), run_validation(&snapshot, c, Duration::from_secs(task.timeout_secs)))).collect();
            let success = !validation_after.is_empty() && validation_after.iter().all(|(_, ok)| *ok) || (task.validation.is_empty() && outcome.as_ref().map(|o| o.status == "completed").unwrap_or(false));
            let regression = validation_before.iter().zip(&validation_after).any(|((_, before), (_, after))| *before && !*after);
            let mut result = match outcome {
                Ok(o) => o,
                Err(e) => RunSummary { error: Some(e.clone()), infra_error: Some(e), status: "error".into(), ..Default::default() },
            };
            result.task_id = task.id.clone();
            let expected: Vec<String> = task.prompts().into_iter().flat_map(|(_, f)| f).collect();
            let expected_modified = if expected.is_empty() { None } else { Some(expected.iter().filter(|f| result.modified_files.contains(f)).count() as f64 / expected.len() as f64) };
            results.push(TaskRunResult {
                task_id: task.id.clone(),
                variant: *variant,
                category: task.category.as_str().into(),
                languages: task.languages.clone(),
                skipped: None,
                success,
                regression,
                validation_before,
                validation_after,
                turns: task.prompts().len(),
                steps: result.steps,
                model_calls: result.model_calls,
                tool_calls: result.tool_calls,
                prompt_tokens: result.prompt_tokens,
                completion_tokens: result.completion_tokens,
                cached_tokens: result.cached_tokens,
                cost_usd: result.cost_usd,
                duration_ms: result.duration_ms,
                recovery_cycles: result.recovery_cycles,
                model_driven_reads: result.model_driven_reads,
                model_driven_searches: result.model_driven_searches,
                retrieval_recall: result.retrieval_recall,
                expected_modified,
                modified_files: result.modified_files,
                agent_status: result.status,
                error: result.error,
                infra_error: result.infra_error,
            });
            if !opts.keep_snapshots {
                let _ = std::fs::remove_dir_all(&snapshot);
            }
        }
    }
    let summaries = BenchmarkReport::summarize(&results);
    Ok(BenchmarkReport {
        generated_at: chrono::Utc::now().to_rfc3339(),
        model,
        provider: provider_name,
        policy_hash: ctx_context_policy_hash(),
        tasks: tasks.len(),
        summaries,
        results,
        config: serde_json::to_value(&opts.base_config).unwrap_or_default(),
    })
}

fn ctx_context_policy_hash() -> String {
    // Avoid a direct dependency on ctx-context: the agent re-exports nothing
    // for this, so compute from the runtime's known policy via the agent
    // crate's context dependency.
    ctx_agent::policy_hash()
}

fn skipped(task: &BenchmarkTask, variant: Variant, reason: String) -> TaskRunResult {
    TaskRunResult {
        task_id: task.id.clone(),
        variant,
        category: task.category.as_str().into(),
        languages: task.languages.clone(),
        skipped: Some(reason),
        success: false,
        regression: false,
        validation_before: vec![],
        validation_after: vec![],
        turns: task.prompts().len(),
        steps: 0,
        model_calls: 0,
        tool_calls: 0,
        prompt_tokens: 0,
        completion_tokens: 0,
        cached_tokens: 0,
        cost_usd: None,
        duration_ms: 0,
        recovery_cycles: 0,
        model_driven_reads: 0,
        model_driven_searches: 0,
        retrieval_recall: None,
        expected_modified: None,
        modified_files: vec![],
        agent_status: "skipped".into(),
        error: None,
        infra_error: None,
    }
}

fn tempfile_dir(prefix: &str) -> Result<PathBuf> {
    let base = std::env::temp_dir().join("context-agent-bench");
    std::fs::create_dir_all(&base)?;
    let unique = format!("{prefix}-{}-{}", std::process::id(), chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0));
    let dir = base.join(unique);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[derive(Default)]
struct RunSummary {
    task_id: String,
    status: String,
    steps: u32,
    model_calls: u32,
    tool_calls: u32,
    prompt_tokens: u64,
    completion_tokens: u64,
    cached_tokens: u64,
    cost_usd: Option<f64>,
    duration_ms: u64,
    recovery_cycles: u64,
    model_driven_reads: u64,
    model_driven_searches: u64,
    retrieval_recall: Option<f64>,
    modified_files: Vec<String>,
    error: Option<String>,
    /// Set when the run ended because of the environment (provider or
    /// runtime error), not because of the agent's work.
    infra_error: Option<String>,
}

async fn run_one(task: &BenchmarkTask, variant: Variant, snapshot: &Path, config: Config, provider: Arc<dyn ModelProvider>) -> std::result::Result<RunSummary, String> {
    let ctx = RepositoryContext::open_ephemeral(snapshot, config).map_err(|e| e.to_string())?;
    ctx.ensure_indexed(None).map_err(|e| e.to_string())?;
    let cancel = CancellationToken::new();
    let runtime = AgentRuntime::new(ctx.clone(), provider, Arc::new(AutoApprove)).with_options(variant.options()).with_cancel(cancel.clone());
    let session = runtime.sessions().start("benchmark", runtime.context().config.model.name.as_str()).map_err(|e| e.to_string())?;
    let mut summary = RunSummary { task_id: task.id.clone(), ..Default::default() };
    let deadline = Duration::from_secs(task.timeout_secs);
    let started = std::time::Instant::now();
    let mut recall_hits = 0usize;
    let mut recall_total = 0usize;
    for (prompt, expected) in task.prompts() {
        let session = ctx.stores.sessions.get_session(&session.id).map_err(|e| e.to_string())?.ok_or("session vanished")?;
        let remaining = deadline.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            summary.error = Some("timeout".into());
            summary.status = "timeout".into();
            break;
        }
        let cancel_clone = cancel.clone();
        let timer = tokio::spawn(async move {
            tokio::time::sleep(remaining).await;
            cancel_clone.cancel();
        });
        let outcome = runtime.run(&session, &prompt).await;
        timer.abort();
        match outcome {
            Ok(o) => {
                summary.status = o.status.as_str().into();
                // The runtime only reports Failed when a runtime/provider
                // error ended the task; the agent's own unsuccessful
                // outcomes are Blocked.
                if o.status == TaskStatus::Failed {
                    summary.infra_error = Some(o.summary.chars().take(300).collect());
                }
                summary.steps += o.steps;
                summary.model_calls += o.model_calls;
                summary.tool_calls += o.tool_calls;
                summary.prompt_tokens += o.usage.prompt_tokens;
                summary.completion_tokens += o.usage.completion_tokens;
                summary.cached_tokens += o.usage.cached_prompt_tokens.unwrap_or(0);
                summary.cost_usd = match (summary.cost_usd, o.cost_usd) { (Some(a), Some(b)) => Some(a + b), (None, b) => b, (a, None) => a };
                summary.duration_ms += o.duration_ms;
                for f in o.modified_files {
                    if !summary.modified_files.contains(&f) {
                        summary.modified_files.push(f);
                    }
                }
                if let Ok(m) = MetricsCollector::summarize(&ctx.stores.sessions, o.task_id.as_str(), None) {
                    summary.recovery_cycles += m.recovery_cycles;
                    summary.model_driven_reads += m.model_driven_file_reads;
                    summary.model_driven_searches += m.search_calls;
                }
                if !expected.is_empty() {
                    if let Ok(Some(state)) = ctx.memory.load_task_state(&o.task_id) {
                        let retrieved: Vec<&str> = state.source_refs.iter().map(|s| s.path.as_str()).collect();
                        recall_total += expected.len();
                        recall_hits += expected.iter().filter(|e| retrieved.contains(&e.as_str())).count();
                    }
                }
                if o.status == TaskStatus::Cancelled {
                    summary.error = Some("timeout".into());
                    break;
                }
                if summary.infra_error.is_some() {
                    break;
                }
            }
            Err(e) => {
                summary.error = Some(e.to_string());
                summary.infra_error = Some(e.to_string());
                summary.status = "error".into();
                break;
            }
        }
    }
    if recall_total > 0 {
        summary.retrieval_recall = Some(recall_hits as f64 / recall_total as f64);
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::BenchmarkTask;
    use ctx_core::{EmbeddingBackend, PermissionProfile, TaskCategory};
    use ctx_providers::{MockProvider, MockTurn};

    #[tokio::test]
    async fn mock_benchmark_round_trip() {
        let ws = tempfile::tempdir().unwrap();
        let repo = ws.path().join("fixture");
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(repo.join("src/lib.py"), "def add(a, b):\n    return a - b\n").unwrap();
        std::fs::write(repo.join("test_lib.py"), "import unittest\nfrom src.lib import add\nclass T(unittest.TestCase):\n    def test_add(self):\n        self.assertEqual(add(1, 2), 3)\nif __name__ == '__main__':\n    unittest.main()\n").unwrap();
        std::fs::write(repo.join("src/__init__.py"), "").unwrap();
        let task = BenchmarkTask {
            id: "fix-add".into(),
            repository: "fixture".into(),
            category: TaskCategory::LocalizedEdit,
            languages: vec!["python".into()],
            prompt: "fix add".into(),
            validation: vec![format!("{} -m unittest test_lib -v", ctx_core::platform::python_interpreter())],
            expected_files: vec!["src/lib.py".into()],
            requires_tools: vec![ctx_core::platform::python_interpreter().into()],
            timeout_secs: 120,
            turns: vec![],
            source: PathBuf::new(),
        };
        let mut config = Config::default();
        config.data_dir = ws.path().join("data");
        config.permissions.profile = PermissionProfile::WorkspaceAutonomous;
        config.embeddings.backend = EmbeddingBackend::Hashing;
        config.embeddings.dimension = 64;
        config.validation.commands = vec![format!("{} -m unittest test_lib -v", ctx_core::platform::python_interpreter())];
        let provider: ProviderFactory = Arc::new(|| {
            Arc::new(MockProvider::new(vec![
                MockTurn::ToolCalls(vec![("read_file".into(), serde_json::json!({"path": "src/lib.py"}))]),
                MockTurn::ToolCalls(vec![("apply_patch".into(), serde_json::json!({"path": "src/lib.py", "edits": [{"old_text": "return a - b", "new_text": "return a + b"}]}))]),
                MockTurn::Text("fixed".into()),
            ]))
        });
        let opts = BenchmarkOptions { variants: vec![Variant::A, Variant::E], workspace_root: ws.path().to_path_buf(), base_config: config.clone(), provider, task_filter: None, keep_snapshots: false, progress: None, previous: vec![] };
        let report = run_benchmark(&[task.clone()], &opts).await.unwrap();
        assert_eq!(report.results.len(), 2);
        for r in &report.results {
            assert!(r.skipped.is_none());
            assert!(r.success, "{r:#?}");
            assert_eq!(r.validation_before[0].1, false);
            assert_eq!(r.validation_after[0].1, true);
            assert!(!r.regression);
            assert_eq!(r.modified_files, vec!["src/lib.py"]);
        }
        let e = report.results.iter().find(|r| r.variant == Variant::E).unwrap();
        assert_eq!(e.retrieval_recall, Some(1.0), "proactive retrieval covered the expected file");
        assert!(report.summaries.iter().all(|s| s.success_rate == 1.0));
        assert!(report.render_markdown().contains("| E-adaptive | 1 | 100.0% (1) |"));
        // The fixture itself was not modified (snapshot isolation).
        assert!(std::fs::read_to_string(repo.join("src/lib.py")).unwrap().contains("a - b"));

        // An outage: every request fails. Runs are recorded as environment
        // errors and excluded from n instead of counting as failures.
        let outage: ProviderFactory = Arc::new(|| {
            Arc::new(MockProvider::new(vec![MockTurn::Error(ctx_core::ProviderError::Api { status: 503, code: None, message: "No provider route is available".into() }); 8]))
        });
        let opts_outage = BenchmarkOptions { variants: vec![Variant::A, Variant::E], workspace_root: ws.path().to_path_buf(), base_config: config.clone(), provider: outage, task_filter: None, keep_snapshots: false, progress: None, previous: vec![] };
        let failed = run_benchmark(&[task.clone()], &opts_outage).await.unwrap();
        assert!(failed.results.iter().all(|r| r.infra_error.is_some() && !r.success), "{:#?}", failed.results);
        assert!(failed.summaries.iter().all(|s| s.n == 0 && s.errors == 1));
        assert!(failed.render_markdown().contains("env error (excluded)"));

        // Resume: keep A's earlier pass, repeat only E's errored run.
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = calls.clone();
        let healthy: ProviderFactory = Arc::new(move || {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Arc::new(MockProvider::new(vec![
                MockTurn::ToolCalls(vec![("read_file".into(), serde_json::json!({"path": "src/lib.py"}))]),
                MockTurn::ToolCalls(vec![("apply_patch".into(), serde_json::json!({"path": "src/lib.py", "edits": [{"old_text": "return a - b", "new_text": "return a + b"}]}))]),
                MockTurn::Text("fixed".into()),
            ]))
        });
        let mut previous = vec![report.results.iter().find(|r| r.variant == Variant::A).unwrap().clone()];
        previous.push(failed.results.iter().find(|r| r.variant == Variant::E).unwrap().clone());
        let opts_resume = BenchmarkOptions { variants: vec![Variant::A, Variant::E], workspace_root: ws.path().to_path_buf(), base_config: config, provider: healthy, task_filter: None, keep_snapshots: false, progress: None, previous };
        let resumed = run_benchmark(&[task], &opts_resume).await.unwrap();
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1, "only the errored run is repeated");
        assert!(resumed.results.iter().all(|r| r.success && r.infra_error.is_none()));
    }

    #[test]
    fn wilson_in_summary() {
        let (lo, hi) = crate::stats::wilson_interval(1, 1, 1.96);
        assert!(lo > 0.2 && hi == 1.0);
    }
}
