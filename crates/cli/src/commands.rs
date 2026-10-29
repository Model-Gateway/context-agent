//! Non-interactive subcommands.

use crate::app::{self, App};
use crate::{Cli, ContextCmd, EvalCmd, IndexCmd, MemoryCmd, TaskCmd};
use ctx_context::{CompileInput, ContextCompiler, ContextExplanation};
use ctx_core::{AgentMessage, Config, CtxError, Result, TaskId};
use ctx_evaluation::{load_tasks, run_benchmark, BenchmarkOptions, Variant};
use ctx_lsp::ServerCatalog;
use ctx_memory::{MemoryCategory, TaskState};
use ctx_providers::{load_profile, save_profile, MockProvider, MockTurn, ModelProvider};
use ctx_retrieval::{analyze_query, RetrievalRequest};
use ctx_telemetry::MetricsCollector;
use std::path::PathBuf;
use std::sync::Arc;

pub fn init(cli: &Cli, repo: bool, force: bool) -> Result<i32> {
    let config = app::load_config(cli)?;
    let layout = config.layout();
    let path = layout.config_file();
    if path.exists() && !force {
        println!("configuration already exists at {} (use --force to overwrite)", path.display());
    } else {
        Config::write_template(&path)?;
        println!("wrote configuration template to {}", path.display());
    }
    if repo {
        let root = app::repository_root(cli);
        let repo_cfg = root.join(Config::REPO_CONFIG_FILE);
        if repo_cfg.exists() && !force {
            println!("{} already exists", repo_cfg.display());
        } else {
            std::fs::write(&repo_cfg, "# Repository-level overrides for Context.\n# [permissions]\n# profile = \"workspace_autonomous\"\n# allowed_commands = [\"npm test\", \"cargo test\"]\n# [validation]\n# commands = [\"npm test\"]\n")?;
            println!("wrote {}", repo_cfg.display());
        }
    }
    println!("\nNext steps:\n  1. Set `model.base_url`, `model.name` and `model.api_key_env` in the config file.\n  2. Export the API key named by `api_key_env`.\n  3. Run `ctx doctor --probe` to verify the endpoint, then `ctx` for the interactive session or `ctx run \"...\"`.");
    Ok(0)
}

pub async fn index(cli: &Cli, cmd: &IndexCmd) -> Result<i32> {
    let App { ctx, config } = app::open(cli, true)?;
    match cmd {
        IndexCmd::Status => {
            let stats = ctx.stores.index.stats()?;
            println!("repository      {}", ctx.root.display());
            println!("repository id   {}", ctx.id);
            println!("generation      {}", stats.generation);
            println!("files           {}", stats.files);
            println!("symbols         {}", stats.symbols);
            println!("chunks          {}", stats.chunks);
            println!("relationships   {}", stats.relationships);
            println!("embeddings      {} ({})", stats.embeddings, stats.embedding_model.clone().unwrap_or_else(|| "none".into()));
            println!("last full index {}", stats.last_full_index_at.clone().unwrap_or_else(|| "never".into()));
            println!("watch enabled   {}", config.index.watch);
            if !stats.languages.is_empty() {
                println!("languages       {}", stats.languages.iter().map(|(l, n)| format!("{l}={n}")).collect::<Vec<_>>().join(", "));
            }
            if stats.generation == 0 {
                println!("\nthe repository has not been indexed yet; run `ctx index rebuild` or any task to index it");
            }
        }
        IndexCmd::Rebuild => {
            ctx.stores.index.clear()?;
            if let Some(e) = &ctx.embeddings {
                e.clear()?;
            }
            let started = std::time::Instant::now();
            let report = ctx.indexer.full_index(Some(&|p| {
                if p.done % 200 == 0 || p.done == p.total {
                    eprintln!("  indexed {}/{}", p.done, p.total);
                }
            }))?;
            println!("rebuilt index generation {} in {:.1}s: {} files, {} symbols, {} chunks, {} relationships ({} resolved), {} embeddings ({} reused)", report.generation, started.elapsed().as_secs_f64(), report.files_indexed, report.symbols, report.chunks, report.relationships, report.resolved_relationships, report.embedded, report.reused_embeddings);
            for e in report.errors.iter().take(10) {
                eprintln!("  warning: {}: {}", e.path, e.message);
            }
        }
        IndexCmd::Watch => {
            ctx.ensure_indexed(None)?;
            println!("watching {} (Ctrl-C to stop)", ctx.root.display());
            let _watcher = ctx_indexing::IndexWatcher::start(ctx.indexer.clone(), std::time::Duration::from_secs(300), |report| {
                println!("[{}] generation {}: {} indexed, {} removed ({} ms)", chrono::Local::now().format("%H:%M:%S"), report.generation, report.files_indexed, report.files_removed, report.duration_ms);
            })?;
            let _ = tokio::signal::ctrl_c().await;
        }
    }
    Ok(0)
}

fn latest_task_id(ctx: &ctx_agent::RepositoryContext, explicit: Option<&str>) -> Result<Option<String>> {
    match explicit {
        Some(id) => Ok(ctx.stores.sessions.find_task(id)?.map(|t| t.id)),
        None => Ok(ctx.stores.sessions.list_tasks(ctx.id.as_str(), 1)?.into_iter().next().map(|t| t.id)),
    }
}

fn latest_event<T: serde::de::DeserializeOwned>(ctx: &ctx_agent::RepositoryContext, task_id: &str, kind: &str) -> Result<Option<T>> {
    let events = ctx.stores.sessions.events_for_task(task_id, 0, usize::MAX / 2)?;
    Ok(events.into_iter().rev().find(|e| e.kind == kind).and_then(|e| serde_json::from_value(e.payload).ok()))
}

async fn dry_run_explanation(ctx: &ctx_agent::RepositoryContext, config: &Config, query: &str) -> Result<ContextExplanation> {
    ctx.ensure_indexed(None)?;
    let analysis = analyze_query(query);
    let mut req = RetrievalRequest::new(query, config.context.budget_for(analysis.category).max);
    req.category = Some(analysis.category);
    let retrieval = ctx.retrieval.retrieve(req).await?;
    let state = TaskState::new(TaskId::new(), query, analysis.category);
    let conversation = vec![AgentMessage::user(query)];
    let summary = ctx.repository_summary()?;
    let compiled = ContextCompiler::compile(CompileInput {
        state: &state,
        conversation: &conversation,
        retrieval: Some(&retrieval),
        tools: ctx_tools::ToolRegistry::standard().for_config(&config).definitions(),
        repository_summary: Some(&summary),
        task_instructions: None,
        checkpoint: None,
        model_window: config.model.context_window as usize,
        max_output_tokens: config.model.max_output_tokens as usize,
        config: &config.context,
        prompt_cache_key: Some(format!("ctx-{}", ctx.id.as_str())),
        index_generation: ctx.stores.index.generation()?,
        diagnostics: vec![],
    });
    Ok(compiled.explanation)
}

pub async fn context(cli: &Cli, cmd: &ContextCmd) -> Result<i32> {
    let App { ctx, config } = app::open(cli, true)?;
    let (query, task, what) = match cmd {
        ContextCmd::Explain { query, task } => (query, task, "explain"),
        ContextCmd::Sources { query, task } => (query, task, "sources"),
        ContextCmd::Stats { query, task } => (query, task, "stats"),
        ContextCmd::Show { task } => (&None, task, "show"),
    };
    if what == "show" {
        let Some(task_id) = latest_task_id(&ctx, task.as_deref())? else {
            println!("no tasks recorded yet");
            return Ok(0);
        };
        match latest_event::<serde_json::Value>(&ctx, &task_id, "context_request")? {
            Some(req) => {
                println!("latest compiled request for task {} (step {}, {} tools, stable prefix {} message(s){})", &task_id[..8], req["step"], req["tools"], req["stable_prefix_len"], req["prompt_cache_key"].as_str().map(|k| format!(", cache key {k}")).unwrap_or_default());
                if let Some(msgs) = req["messages"].as_array() {
                    for (i, m) in msgs.iter().enumerate() {
                        println!("{:>3}. {:<18} ~{:>6} tokens  {}", i + 1, m["kind"].as_str().unwrap_or(""), m["estimated_tokens"], m["preview"].as_str().unwrap_or("").replace('\n', " ").chars().take(100).collect::<String>());
                    }
                }
                println!("(previews only; full tool output is in the task event log: `ctx task show {}`)", &task_id[..8]);
            }
            None => println!("no compiled request recorded for task {task_id}"),
        }
        return Ok(0);
    }
    let explanation = match (query, task) {
        (Some(q), _) => dry_run_explanation(&ctx, &config, q).await?,
        (None, task) => {
            let Some(task_id) = latest_task_id(&ctx, task.as_deref())? else {
                println!("no tasks recorded yet; pass --query to dry-run retrieval");
                return Ok(0);
            };
            match latest_event::<ContextExplanation>(&ctx, &task_id, "context_explanation")? {
                Some(e) => e,
                None => {
                    println!("no context explanation recorded for task {task_id}");
                    return Ok(0);
                }
            }
        }
    };
    let text = match what {
        "sources" => explanation.render_sources(),
        "stats" => explanation.render_stats(),
        _ => explanation.render_explain(),
    };
    print!("{}", ctx.redactor.redact(&text));
    Ok(0)
}

pub fn memory(cli: &Cli, cmd: &MemoryCmd) -> Result<i32> {
    let App { ctx, config: _ } = app::open(cli, true)?;
    match cmd {
        MemoryCmd::List { category, all, limit } => {
            let cat = category.as_deref().map(|c| MemoryCategory::parse(c).ok_or_else(|| CtxError::InvalidArgument(format!("unknown category {c}")))).transpose()?;
            let records = ctx.memory.list(cat, None, *all, *limit)?;
            println!("{} memory record(s)", records.len());
            for m in records {
                println!("{} [{} | {}]{} {}{}", m.id.short(), m.category.as_str(), m.verification.as_str(), if m.invalidated { " (invalidated)" } else { "" }, ctx.redactor.redact(&m.content), {
                    let e: Vec<&str> = m.evidence_paths().collect();
                    if e.is_empty() { String::new() } else { format!(" ← {}", e.join(", ")) }
                });
            }
        }
        MemoryCmd::Search { query } => {
            for (m, score) in ctx.memory.search(query, 20, true)? {
                println!("{:.3} {} [{} | {}] {}", score, m.id.short(), m.category.as_str(), m.verification.as_str(), ctx.redactor.redact(&m.content));
            }
        }
        MemoryCmd::Forget { id, reason } => {
            let all = ctx.memory.list(None, None, true, 0)?;
            let matches: Vec<_> = all.iter().filter(|m| m.id.as_str().starts_with(id.as_str())).collect();
            match matches.as_slice() {
                [one] => {
                    ctx.memory.invalidate(&one.id, reason)?;
                    println!("invalidated {}", one.id);
                }
                [] => println!("no memory matches {id}"),
                _ => println!("{} memories match {id}; be more specific", matches.len()),
            }
        }
    }
    Ok(0)
}

pub fn task(cli: &Cli, cmd: &TaskCmd) -> Result<i32> {
    let App { ctx, .. } = app::open(cli, true)?;
    match cmd {
        TaskCmd::List { limit } => {
            let tasks = ctx.stores.sessions.list_tasks(ctx.id.as_str(), *limit)?;
            println!("{:<9} {:<10} {:<13} {:<17} objective", "task", "status", "state", "updated");
            for t in tasks {
                println!("{:<9} {:<10} {:<13} {:<17} {}", &t.id[..8], t.status.clone().unwrap_or_else(|| "-".into()), t.state, t.updated_at.format("%Y-%m-%d %H:%M"), t.objective.chars().take(80).collect::<String>());
            }
        }
        TaskCmd::Show { id } => {
            let Some(t) = ctx.stores.sessions.find_task(id)? else {
                println!("no task matches {id}");
                return Ok(1);
            };
            println!("task      {}\nsession   {}\nstatus    {}\nstate     {}\ncategory  {}\ncreated   {}\nupdated   {}", t.id, t.session_id, t.status.clone().unwrap_or_else(|| "-".into()), t.state, t.category, t.created_at.format("%Y-%m-%d %H:%M:%S"), t.updated_at.format("%Y-%m-%d %H:%M:%S"));
            if let Some(p) = &t.parent_task_id {
                println!("parent    {p}");
            }
            if let Some(state) = TaskState::from_json(&t.state_json) {
                println!("\n{}", ctx.redactor.redact(&state.checkpoint_summary()));
            }
            if let Some(summary) = &t.summary {
                println!("final report:\n{}\n", ctx.redactor.redact(summary));
            }
            let metrics = MetricsCollector::summarize(&ctx.stores.sessions, &t.id, None)?;
            let totals = ctx.stores.sessions.usage_totals(Some(&t.id), None)?;
            println!("model calls {} · tool calls {} · prompt tokens {} · completion {} · cached {} · usage source {} · latency {} ms · recovery cycles {} · duration {} ms", metrics.model_invocations, metrics.tool_invocations, totals.prompt_tokens, totals.completion_tokens, totals.cached_tokens, if totals.estimated_requests > 0 && totals.reported_requests == 0 { "estimated" } else if totals.estimated_requests > 0 { "mixed" } else { "reported" }, totals.total_latency_ms, metrics.recovery_cycles, metrics.duration_ms);
            if let Some(cost) = totals.cost() {
                println!("cost ${cost:.4}");
            }
            let checkpoints = ctx.stores.sessions.checkpoints_for_task(&t.id)?;
            let events = ctx.stores.sessions.count_events(&t.id, None)?;
            println!("checkpoints {} · events {}", checkpoints.len(), events);
            let runs = ctx.stores.sessions.validation_runs_for_task(&t.id)?;
            for r in runs {
                println!("validation {} `{}` {} ({} ms)", if r.passed { "PASS" } else { "FAIL" }, r.command, r.summary, r.duration_ms);
            }
        }
    }
    Ok(0)
}

pub fn diff(cli: &Cli, path: Option<&str>) -> Result<i32> {
    let root = app::repository_root(cli);
    let mut cmd = std::process::Command::new("git");
    cmd.arg("--no-pager").arg("diff").current_dir(&root);
    if let Some(p) = path {
        cmd.arg("--").arg(p);
    }
    let status = cmd.status()?;
    Ok(status.code().unwrap_or(1))
}

pub async fn doctor(cli: &Cli, probe: bool) -> Result<i32> {
    let config = app::load_config(cli)?;
    app::init_logging(cli, &config, true);
    let mut problems = 0;
    println!("== configuration");
    println!("data dir        {}", config.data_dir.display());
    println!("config sources  {}", if config.sources.is_empty() { "built-in defaults only (run `ctx init`)".to_string() } else { config.sources.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ") });
    match std::fs::create_dir_all(&config.data_dir) {
        Ok(_) => println!("data dir        writable"),
        Err(e) => {
            println!("data dir        NOT writable: {e}");
            problems += 1;
        }
    }
    let wrapper = ctx_security::SandboxStatus::from_config(&config.permissions.sandbox);
    println!("permissions     {}{}", config.permissions.profile.as_str(), if wrapper.enforced { format!(" · command wrapper (developer mode): {}", wrapper.description) } else { String::new() });
    println!("effort          {}", config.model.reasoning_effort.as_ref().map(|e| format!("{}{}", e.as_str(), if e.deploys_agents() { " (deploys sub-agents)" } else { "" })).unwrap_or_else(|| "auto".into()));
    if !config.ignored_repository_keys.is_empty() {
        println!("note            repository .context-agent.toml tried to set [{}]; only trusted configuration may set these", config.ignored_repository_keys.join("], ["));
    }

    println!("\n== repository");
    let root = app::repository_root(cli);
    println!("root            {}", root.display());
    match ctx_indexing::RepositoryDiscovery::git_info(&root) {
        Some(g) => println!("git             branch {} · head {} · {} uncommitted", g.branch.unwrap_or_else(|| "detached".into()), g.head.map(|h| h[..8.min(h.len())].to_string()).unwrap_or_default(), g.dirty_files.len()),
        None => println!("git             not a git repository (indexing still works; .gitignore rules are honoured if present)"),
    }
    let ctx = ctx_agent::RepositoryContext::open_exact(&root, config.clone())?;
    let stats = ctx.stores.index.stats()?;
    println!("index           generation {} · {} files · {} symbols · {} chunks · {} embeddings", stats.generation, stats.files, stats.symbols, stats.chunks, stats.embeddings);
    let profile = ctx.profile()?;
    println!("languages       {}", profile.languages.iter().map(|(l, n)| format!("{l}={n}")).collect::<Vec<_>>().join(", "));
    println!("\n== validation commands");
    let commands = ctx.validation_commands()?;
    if commands.is_empty() {
        println!("none detected (configure [validation].commands or add a build file)");
    }
    for c in &commands {
        let program = c.command.split_whitespace().next().unwrap_or("");
        let present = program.starts_with("./") || ctx_evaluation::spec::which(program);
        println!("{:<10} {:<40} cwd {:<10} {} ({})", format!("{:?}", c.kind).to_lowercase(), c.command.chars().take(40).collect::<String>(), c.cwd, if present { "ok" } else { "MISSING TOOL" }, c.source);
        if !present {
            problems += 1;
        }
    }
    println!("\n== language servers (optional)");
    for s in ServerCatalog::all_statuses() {
        if profile.languages.contains_key(&s.language) || cli.verbose > 0 {
            println!("{:<11} {:<28} {}", s.language, s.executable, if s.installed { "installed".to_string() } else { format!("not installed ({})", s.install_hint) });
        }
    }
    if !config.mcp.servers.is_empty() {
        println!("\n== mcp servers");
        if !config.mcp.enabled {
            println!("mcp             disabled (mcp.enabled = false)");
        }
        let mcp = app::mcp_manager(&config, &root);
        mcp.ready().await;
        for status in mcp.status() {
            let (line, ok) = crate::serve::mcp_doctor_line(&status);
            println!("{:<15} {line}", status.name);
            if !ok {
                problems += 1;
            }
        }
        mcp.shutdown().await;
    }

    println!("\n== model provider");
    println!("endpoint        {} · model {} · key {} ({})", config.model.base_url, config.model.name, ctx_core::api_key_source(&config.model), if ctx_core::resolve_api_key(&config, &config.model).is_some() { "set" } else { "NOT SET" });
    println!("resilience      {} retries (backoff {}–{} ms) · fallbacks: {}", config.model.max_retries, config.model.retry_base_delay_ms, config.model.retry_max_delay_ms, config.model.fallbacks_summary());
    for fb in config.model.chain().iter().skip(1) {
        // A fallback sharing the primary's key is covered by the line above.
        if (fb.api_key_env != config.model.api_key_env || fb.credential != config.model.credential) && ctx_core::resolve_api_key(&config, fb).is_none() && !fb.base_url.contains("localhost") && !fb.base_url.contains("127.0.0.1") {
            println!("warning         fallback {} at {}: {} NOT SET", fb.name, fb.base_url, ctx_core::api_key_source(fb));
        }
    }
    if ctx_core::resolve_api_key(&config, &config.model).is_none() && !config.model.base_url.contains("localhost") && !config.model.base_url.contains("127.0.0.1") {
        problems += 1;
    }
    let cached = load_profile(&config)?;
    let profile = if probe {
        let provider = ctx_providers::build_provider(&config)?;
        println!("probing …");
        let p = provider.probe(&ctx_core::CancellationToken::new()).await?;
        let path = save_profile(&config, &p)?;
        println!("profile saved   {}", path.display());
        Some(p)
    } else {
        cached
    };
    match profile {
        Some(p) => {
            println!("probed at       {}", p.probed_at.map(|t| t.to_rfc3339()).unwrap_or_else(|| "never".into()));
            for (name, status) in [
                ("connectivity", &p.connectivity),
                ("model listed", &p.model_listed),
                ("chat completion", &p.chat_completion),
                ("tool calling", &p.tool_calling),
                ("tool continuation", &p.tool_result_continuation),
                ("parallel tools", &p.parallel_tool_calls),
                ("streaming", &p.streaming),
                ("stream usage", &p.stream_usage),
                ("usage reporting", &p.usage_reporting),
                ("cached tokens", &p.cached_token_reporting),
                ("reasoning controls", &p.reasoning_controls),
                ("developer role", &p.developer_role),
            ] {
                println!("{:<19} {:<5} {}", name, status.symbol(), status.detail());
            }
            println!("max tokens field   {}", p.max_tokens_field);
            for n in &p.notes {
                println!("note: {n}");
            }
            if !p.is_agent_capable() {
                println!("PROBLEM: endpoint is not agent-capable: {}", p.blocking_issues().join("; "));
                problems += 1;
            }
        }
        None => println!("no capability profile cached; run `ctx doctor --probe`"),
    }
    let verdict = if problems == 0 { "no problems found".to_string() } else { format!("{problems} problem(s) found") };
    println!("\n{verdict}");
    Ok(if problems == 0 { 0 } else { 1 })
}

pub async fn eval(cli: &Cli, cmd: &EvalCmd) -> Result<i32> {
    let config = app::load_config(cli)?;
    app::init_logging(cli, &config, true);
    let workspace_root = std::env::current_dir()?;
    match cmd {
        EvalCmd::Run { tasks, variants, filter, mock, output, keep, resume, max_retries } => {
            let tasks = load_tasks(tasks)?;
            let mut config = config.clone();
            if let Some(n) = max_retries {
                config.model.max_retries = *n;
            }
            let mut previous = Vec::new();
            for path in resume {
                let text = std::fs::read_to_string(path)?;
                let report: ctx_evaluation::BenchmarkReport = serde_json::from_str(&text).map_err(|e| CtxError::InvalidArgument(format!("{}: not a benchmark report: {e}", path.display())))?;
                previous.extend(report.results);
            }
            if !previous.is_empty() {
                let rerun = previous.iter().filter(|r| r.needs_rerun()).count();
                eprintln!("resuming from {} earlier result(s); {rerun} will be repeated", previous.len());
            }
            let variants: Vec<Variant> = match variants {
                Some(v) => v.split(',').map(|s| Variant::parse(s).ok_or_else(|| CtxError::InvalidArgument(format!("unknown variant {s}")))).collect::<Result<_>>()?,
                None => Variant::all().to_vec(),
            };
            let config_for_provider = config.clone();
            let provider: ctx_evaluation::ProviderFactory = if *mock {
                Arc::new(|| Arc::new(MockProvider::new(vec![MockTurn::Text("Mock provider: no changes made.".into())]).with_fallback("done")) as Arc<dyn ModelProvider>)
            } else {
                let cached = load_profile(&config)?;
                Arc::new(move || {
                    let p = ctx_providers::build_provider(&config_for_provider).expect("provider");
                    if let Some(profile) = &cached {
                        p.apply_profile(profile);
                    }
                    p
                })
            };
            let opts = BenchmarkOptions {
                variants,
                workspace_root: workspace_root.clone(),
                base_config: config.clone(),
                provider,
                task_filter: filter.as_ref().map(|f| f.split(',').map(|s| s.trim().to_string()).collect()),
                keep_snapshots: *keep,
                progress: Some(Arc::new(|m| eprintln!("running {m}"))),
                previous,
            };
            let report = run_benchmark(&tasks, &opts).await?;
            let out = output.clone().unwrap_or_else(|| PathBuf::from("benchmarks/results").join(format!("{}.json", chrono::Utc::now().format("%Y%m%dT%H%M%SZ"))));
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&out, serde_json::to_string_pretty(&report)?)?;
            std::fs::write(out.with_extension("md"), report.render_markdown())?;
            println!("{}", report.render_markdown());
            println!("report written to {} and {}", out.display(), out.with_extension("md").display());
            Ok(0)
        }
        EvalCmd::Retrieval { queries, output } => {
            let queries = ctx_evaluation::spec::load_queries(queries)?;
            let report = ctx_evaluation::evaluate_retrieval(&queries, &workspace_root, &config).await?;
            println!("{}", report.render());
            if let Some(out) = output {
                std::fs::write(out, serde_json::to_string_pretty(&report)?)?;
            }
            Ok(0)
        }
    }
}


// ---------------------------------------------------------------------------
// ctx mcp
// ---------------------------------------------------------------------------

pub async fn mcp(cli: &Cli, cmd: &crate::McpCmd) -> Result<i32> {
    use crate::McpCmd;
    use ctx_core::{McpServerConfig, McpTransport};
    use ctx_tools::McpServerState;
    let config = app::load_config(cli)?;
    app::init_logging(cli, &config, true);
    let root = app::repository_root(cli);
    match cmd {
        McpCmd::List => {
            if config.mcp.servers.is_empty() {
                println!("no MCP servers configured; add one with `ctx mcp add <name> -- <command> [args…]` or `ctx mcp add <name> --url <url>`");
                return Ok(0);
            }
            if !config.mcp.enabled {
                println!("note: mcp.enabled = false; no server is connected and the model gets no MCP tools\n");
            }
            let manager = app::mcp_manager(&config, &root);
            manager.ready().await;
            let mut problems = 0;
            for status in manager.status() {
                let (line, ok) = crate::serve::mcp_doctor_line(&status);
                println!("{:<20} {line}", status.name);
                if !ok {
                    problems += 1;
                }
            }
            manager.shutdown().await;
            Ok(if problems == 0 { 0 } else { 1 })
        }
        McpCmd::Tools { server } => {
            if let Some(name) = server {
                if !config.mcp.servers.contains_key(name) {
                    return Err(CtxError::Config(format!("no MCP server named `{name}` is configured")));
                }
            }
            let manager = app::mcp_manager(&config, &root);
            manager.ready().await;
            let mut shown = 0;
            for status in manager.status().into_iter().filter(|s| server.as_deref().map(|n| n == s.name).unwrap_or(true)) {
                match status.state {
                    McpServerState::Connected => {
                        println!("== {} ({} tool(s))", status.name, status.tools.len());
                        if let Some(instructions) = &status.instructions {
                            println!("{}", indent(instructions.trim(), "   "));
                        }
                        for tool in &status.tools {
                            let marks = match (tool.read_only, tool.destructive) {
                                (true, _) => " [read-only]",
                                (false, true) => " [destructive]",
                                _ => "",
                            };
                            println!("{}{marks}", tool.qualified_name);
                            if !tool.description.trim().is_empty() {
                                println!("{}", indent(tool.description.trim(), "   "));
                            }
                            shown += 1;
                        }
                        println!();
                    }
                    _ => {
                        let (line, _) = crate::serve::mcp_doctor_line(&status);
                        println!("== {}: {line}\n", status.name);
                    }
                }
            }
            if shown == 0 {
                println!("no tools available");
            }
            manager.shutdown().await;
            Ok(0)
        }
        McpCmd::Add { name, url, env, headers, cwd, tools, disabled, command } => {
            if !McpServerConfig::valid_name(name) {
                return Err(CtxError::Config(format!("`{name}` is not a valid server name: use letters, digits, `_` and `-` (at most 32 characters)")));
            }
            let mut server = match (url, command.is_empty()) {
                (Some(url), _) => McpServerConfig::http(url.trim()),
                (None, false) => McpServerConfig::stdio(command[0].clone(), command[1..].to_vec()),
                (None, true) => return Err(CtxError::Config("give a command after `--` (stdio) or `--url <url>` (http)".into())),
            };
            for pair in env {
                let (k, v) = pair.split_once('=').ok_or_else(|| CtxError::Config(format!("`{pair}` is not KEY=VALUE")))?;
                server.env.insert(k.trim().to_string(), v.to_string());
            }
            for header in headers {
                let (k, v) = header.split_once(':').or_else(|| header.split_once('=')).ok_or_else(|| CtxError::Config(format!("`{header}` is not `Name: value`")))?;
                server.headers.insert(k.trim().to_string(), v.trim().to_string());
            }
            if server.transport == McpTransport::Stdio && !server.headers.is_empty() {
                return Err(CtxError::Config("--header applies to http servers only".into()));
            }
            if server.transport == McpTransport::Http && !server.env.is_empty() {
                return Err(CtxError::Config("--env applies to stdio servers only; send credentials with --header".into()));
            }
            server.cwd = cwd.clone();
            server.tools = tools.clone();
            server.enabled = !disabled;
            server.validate(name)?;
            let path = update_user_config(&config, |table| set_server(table, name, Some(toml::Value::try_from(&server).expect("a server config serializes"))))?;
            println!("added MCP server `{name}` ({} {}) to {}", server.transport.as_str(), server.target(), path.display());
            if !disabled {
                println!("tools will appear as mcp__{name}__<tool>; `ctx mcp tools {name}` lists them");
            }
            Ok(0)
        }
        McpCmd::Remove { name } => {
            if !config.mcp.servers.contains_key(name) {
                return Err(CtxError::Config(format!("no MCP server named `{name}` is configured")));
            }
            let path = update_user_config(&config, |table| set_server(table, name, None))?;
            println!("removed MCP server `{name}` from {}", path.display());
            Ok(0)
        }
        McpCmd::Catalog => {
            for app in ctx_tools::mcp::catalog() {
                let installed = config.mcp.servers.contains_key(&app.id);
                let mark = if installed { "installed" } else if app.default_installed { "removed" } else { "" };
                println!("{:<20} {:<22} {:<10} {}", app.id, app.name, mark, app.tagline);
                if !app.inputs.is_empty() {
                    println!("{:<20} needs: {}", "", app.inputs.iter().map(|i| format!("{}{}", i.key, if i.required { "" } else { " (optional)" })).collect::<Vec<_>>().join(", "));
                }
                if !app.requirements.is_empty() {
                    println!("{:<20} requires: {}", "", app.requirements.join(", "));
                }
            }
            println!("\n`ctx mcp install <id> [--env KEY=VALUE]…` installs one; `ctx mcp uninstall <id>` removes it.");
            Ok(0)
        }
        McpCmd::Install { id, env } => {
            let app = ctx_tools::mcp::catalog_app(id).ok_or_else(|| CtxError::Config(format!("no app `{id}` in the catalog; `ctx mcp catalog` lists them")))?;
            let mut inputs = std::collections::BTreeMap::new();
            for pair in env {
                let (k, v) = pair.split_once('=').ok_or_else(|| CtxError::Config(format!("`{pair}` is not KEY=VALUE")))?;
                inputs.insert(k.trim().to_string(), v.to_string());
            }
            let missing = app.missing_inputs(&inputs);
            if !missing.is_empty() && app.oauth.as_ref().map(|o| o.available).unwrap_or(false) {
                return Err(CtxError::Config(format!("{} needs a sign-in: run `ctx mcp login {}` (or pass {})", app.name, app.id, missing.iter().map(|i| format!("--env {}=…", i.key)).collect::<Vec<_>>().join(" "))));
            }
            if !missing.is_empty() {
                return Err(CtxError::Config(format!("{} needs {}; pass {}", app.name, missing.iter().map(|i| i.label.as_str()).collect::<Vec<_>>().join(", "), missing.iter().map(|i| format!("--env {}=…", i.key)).collect::<Vec<_>>().join(" "))));
            }
            let server = app.server(&inputs);
            let path = update_user_config(&config, |table| set_server(table, &app.id, Some(toml::Value::try_from(&server).expect("a server config serializes"))))?;
            println!("installed {} as MCP server `{}` ({})", app.name, app.id, path.display());
            println!("tools will appear as mcp__{}__<tool>; `ctx mcp tools {}` lists them", app.id, app.id);
            Ok(0)
        }
        McpCmd::Uninstall { id } => {
            let Some(server) = config.mcp.servers.get(id) else {
                return Err(CtxError::Config(format!("`{id}` is not installed")));
            };
            let credentials = ctx_tools::mcp::oauth::referenced_credentials(server);
            let path = update_user_config(&config, |table| set_server(table, id, None))?;
            for name in credentials {
                ctx_tools::mcp::oauth::sign_out(&config.layout(), &name)?;
            }
            println!("uninstalled `{id}` ({})", path.display());
            Ok(0)
        }
        McpCmd::Login { id } => {
            use ctx_tools::mcp::oauth::{self, DevicePoll};
            let app = ctx_tools::mcp::catalog_app(id).ok_or_else(|| CtxError::Config(format!("no app `{id}` in the catalog; `ctx mcp catalog` lists them")))?;
            let auth = app.oauth.clone().ok_or_else(|| CtxError::Config(format!("{} has no sign-in; install it with `ctx mcp install {id} --env …`", app.name)))?;
            let code = oauth::github_device_start(&auth.scopes).await?;
            println!("Sign in to GitHub: open {} and enter the code

    {}
", code.verification_uri, code.user_code);
            crate::tui::open_in_browser(&code.verification_uri);
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(code.expires_in.max(60));
            let mut interval = code.interval.max(5);
            let account = loop {
                if std::time::Instant::now() > deadline {
                    return Err(CtxError::tool("the code expired before it was approved; run the command again"));
                }
                tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
                match oauth::github_device_poll(&config.layout(), &auth.credential, &code.device_code).await? {
                    DevicePoll::Pending { interval: next } => interval = next.max(5),
                    DevicePoll::Approved { account } => break account,
                    DevicePoll::Denied => return Err(CtxError::tool("the sign-in was declined on GitHub")),
                    DevicePoll::Expired => return Err(CtxError::tool("the code expired before it was approved; run the command again")),
                }
            };
            let mut inputs = std::collections::BTreeMap::new();
            inputs.insert(auth.input.clone(), oauth::credential_reference(&auth.credential));
            let server = app.server(&inputs);
            let path = update_user_config(&config, |table| set_server(table, &app.id, Some(toml::Value::try_from(&server).expect("a server config serializes"))))?;
            println!("signed in to GitHub as {account}; {} is installed ({})", app.name, path.display());
            Ok(0)
        }
        McpCmd::Logout { id } => {
            let app = ctx_tools::mcp::catalog_app(id).ok_or_else(|| CtxError::Config(format!("no app `{id}` in the catalog")))?;
            let auth = app.oauth.ok_or_else(|| CtxError::Config(format!("{} has no sign-in", app.name)))?;
            let removed = ctx_tools::mcp::oauth::sign_out(&config.layout(), &auth.credential)?;
            println!("{}", if removed { format!("signed out of {}", app.name) } else { format!("not signed in to {}", app.name) });
            Ok(0)
        }
        McpCmd::Enable { name } | McpCmd::Disable { name } => {
            let enable = matches!(cmd, McpCmd::Enable { .. });
            let Some(server) = config.mcp.servers.get(name) else {
                return Err(CtxError::Config(format!("no MCP server named `{name}` is configured")));
            };
            let mut server = server.clone();
            server.enabled = enable;
            let path = update_user_config(&config, |table| set_server(table, name, Some(toml::Value::try_from(&server).expect("a server config serializes"))))?;
            println!("MCP server `{name}` {} ({})", if enable { "enabled" } else { "disabled" }, path.display());
            Ok(0)
        }
    }
}

fn indent(text: &str, prefix: &str) -> String {
    text.lines().map(|l| format!("{prefix}{l}")).collect::<Vec<_>>().join("\n")
}

/// Sets (or, with `None`, removes) `mcp.servers.<name>` in a configuration table.
fn set_server(table: &mut toml::Table, name: &str, value: Option<toml::Value>) {
    let value_is_none = value.is_none();
    let mcp = table.entry("mcp").or_insert_with(|| toml::Value::Table(toml::Table::new()));
    if !mcp.is_table() {
        *mcp = toml::Value::Table(toml::Table::new());
    }
    let mcp = mcp.as_table_mut().expect("table");
    let servers = mcp.entry("servers").or_insert_with(|| toml::Value::Table(toml::Table::new()));
    if !servers.is_table() {
        *servers = toml::Value::Table(toml::Table::new());
    }
    let servers = servers.as_table_mut().expect("table");
    match value {
        Some(v) => {
            servers.insert(name.to_string(), v);
        }
        None => {
            servers.remove(name);
            if servers.is_empty() {
                mcp.remove("servers");
            }
        }
    }
    // A default app removed stays removed; installed again, it is no longer
    // remembered as removed.
    if ctx_core::McpConfig::is_default_app(name) {
        let mut uninstalled: Vec<toml::Value> = mcp.get("uninstalled").and_then(|u| u.as_array()).cloned().unwrap_or_default();
        uninstalled.retain(|u| u.as_str() != Some(name));
        if value_is_none {
            uninstalled.push(toml::Value::String(name.to_string()));
        }
        if uninstalled.is_empty() {
            mcp.remove("uninstalled");
        } else {
            mcp.insert("uninstalled".into(), toml::Value::Array(uninstalled));
        }
    }
    if mcp.is_empty() {
        table.remove("mcp");
    }
}

/// Edits `~/.context-agent/config.toml` in place: parse, change, validate by
/// loading, write privately and atomically. Returns the file's path.
fn update_user_config(config: &Config, change: impl FnOnce(&mut toml::Table)) -> Result<PathBuf> {
    let path = config.layout().config_file();
    let mut table: toml::Table = match std::fs::read_to_string(&path) {
        Ok(text) => text.parse().map_err(|e: toml::de::Error| CtxError::Config(format!("{}: {e}", path.display())))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => toml::Table::new(),
        Err(e) => return Err(e.into()),
    };
    change(&mut table);
    let candidate: Config = toml::Value::Table(table.clone()).try_into().map_err(|e: toml::de::Error| CtxError::Config(format!("invalid configuration: {e}")))?;
    candidate.validate()?;
    let text = toml::to_string_pretty(&table).map_err(|e| CtxError::Config(format!("cannot serialize configuration: {e}")))?;
    crate::serve::write_private_atomic(&path, &text)?;
    Ok(path)
}

#[cfg(test)]
mod mcp_tests {
    use super::*;

    #[test]
    fn servers_are_added_and_removed_in_the_user_table() {
        let mut table = toml::Table::new();
        let server = ctx_core::McpServerConfig::stdio("npx", vec!["-y".into(), "server".into()]);
        set_server(&mut table, "gh", Some(toml::Value::try_from(&server).unwrap()));
        let config: Config = toml::Value::Table(table.clone()).try_into().unwrap();
        assert_eq!(config.mcp.servers["gh"].command.as_deref(), Some("npx"));
        assert_eq!(config.mcp.servers["gh"].args, vec!["-y".to_string(), "server".to_string()]);
        set_server(&mut table, "gh", None);
        assert!(!table.contains_key("mcp"), "an empty [mcp] table is dropped");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn removing_a_default_app_is_remembered() {
        let mut table = toml::Table::new();
        set_server(&mut table, "computer-use", None);
        let config: Config = toml::Value::Table(table.clone()).try_into().unwrap();
        assert_eq!(config.mcp.uninstalled, vec!["computer-use".to_string()]);
        let mut defaults = config.mcp.clone();
        defaults.apply_defaults();
        assert!(!defaults.servers.contains_key("computer-use"), "a removed default is not added back");
        set_server(&mut table, "computer-use", Some(toml::Value::try_from(ctx_core::McpServerConfig::builtin("computer-use")).unwrap()));
        let config: Config = toml::Value::Table(table).try_into().unwrap();
        assert!(config.mcp.uninstalled.is_empty());
        assert_eq!(config.mcp.servers["computer-use"].builtin.as_deref(), Some("computer-use"));
    }
}
