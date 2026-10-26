//! `ctx run`: headless execution with stable exit codes (spec §12.3).

use crate::app::{self, App};
use crate::Cli;
use async_trait::async_trait;
use ctx_agent::{AgentRuntime, RunOptions};
use ctx_core::{AgentEvent, ApprovalDecision, ApprovalRequest, CancellationToken, CtxError, Result, TaskOutcome};
use ctx_tools::{ApprovalHandler, AutoApprove, DenyAll};
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

pub struct HeadlessOptions {
    pub prompt: String,
    pub json: bool,
    pub approval: String,
    pub timeout: Option<u64>,
    pub max_steps: Option<u32>,
    pub stream: bool,
    pub probe: bool,
    pub session: Option<String>,
    pub new_session: bool,
    pub variant: Option<String>,
}

/// Approvals answered on the terminal.
struct StdinApprovals;

#[async_trait]
impl ApprovalHandler for StdinApprovals {
    async fn request(&self, request: ApprovalRequest) -> ApprovalDecision {
        let prompt = format!("\n[approval needed] {} ({}): {}\n{}\nApprove? [y]es / [n]o / [a]lways this session: ", request.tool, request.risk.as_str(), request.summary, request.details.lines().take(30).collect::<Vec<_>>().join("\n"));
        tokio::task::spawn_blocking(move || {
            eprint!("{prompt}");
            let _ = std::io::stderr().flush();
            let mut line = String::new();
            if std::io::stdin().read_line(&mut line).is_err() {
                return ApprovalDecision::Deny;
            }
            match line.trim().to_ascii_lowercase().as_str() {
                "y" | "yes" => ApprovalDecision::Approve,
                "a" | "always" => ApprovalDecision::ApproveForSession,
                _ => ApprovalDecision::Deny,
            }
        })
        .await
        .unwrap_or(ApprovalDecision::Deny)
    }
}

pub async fn run(cli: &Cli, opts: HeadlessOptions) -> Result<i32> {
    let App { config, ctx } = app::open(cli, true)?;
    crate::billing::install(&config);
    let provider = app::provider(&config, opts.probe, opts.json).await?;
    let approvals: Arc<dyn ApprovalHandler> = match opts.approval.as_str() {
        "auto" => Arc::new(AutoApprove),
        "deny" => Arc::new(DenyAll),
        "ask" => Arc::new(StdinApprovals),
        other => return Err(CtxError::InvalidArgument(format!("unknown approval policy `{other}` (expected ask, auto or deny)"))),
    };
    let mut run_options = match &opts.variant {
        Some(v) => ctx_evaluation::Variant::parse(v).ok_or_else(|| CtxError::InvalidArgument(format!("unknown variant `{v}`")))?.options(),
        None => RunOptions::default(),
    };
    run_options.max_steps = opts.max_steps.or(run_options.max_steps);
    run_options.stream = opts.stream;

    let (tx, mut rx) = tokio::sync::mpsc::channel::<AgentEvent>(1024);
    let json = opts.json;
    let printer = tokio::spawn(async move {
        let mut outcome: Option<TaskOutcome> = None;
        let mut streamed_text = false;
        let mut streamed = String::new();
        let stdout = std::io::stdout();
        while let Some(ev) = rx.recv().await {
            if json {
                if ev.is_persistent() {
                    eprintln!("{}", serde_json::to_string(&ev).unwrap_or_default());
                }
                if let AgentEvent::TaskCompleted { outcome: o, .. } = ev {
                    outcome = Some(o);
                }
                continue;
            }
            match ev {
                AgentEvent::ModelTextDelta { text, .. } => {
                    let mut out = stdout.lock();
                    let _ = out.write_all(text.as_bytes());
                    let _ = out.flush();
                    streamed.push_str(&text);
                    streamed_text = true;
                }
                AgentEvent::ModelResponse { tool_call_count, text, .. } => {
                    if streamed_text {
                        println!();
                        streamed_text = false;
                    }
                    if let Some(t) = text {
                        if !streamed.is_empty() && t.trim() != streamed.trim() {
                            println!("[final response superseded the streamed text]\n{t}");
                        }
                    }
                    streamed.clear();
                    if tool_call_count > 0 {
                        eprintln!("· {tool_call_count} tool call(s)");
                    }
                }
                AgentEvent::ToolCallStarted { name, arguments, .. } => {
                    let args = arguments.to_string();
                    eprintln!("→ {name} {}", args.chars().take(160).collect::<String>());
                }
                AgentEvent::ToolCallFinished { success, summary, duration_ms, .. } => {
                    eprintln!("  {} {summary} ({duration_ms} ms)", if success { crate::transcript::glyphs().ok } else { crate::transcript::glyphs().fail });
                }
                AgentEvent::StateChanged { to, .. } => eprintln!("[{}]", to.as_str()),
                AgentEvent::ValidationFinished { record, .. } => eprintln!("  validation `{}`: {}", record.command, if record.passed { "PASSED" } else { "FAILED" }),
                AgentEvent::FileChanged { path, kind, .. } => eprintln!("  file {kind:?}: {path}"),
                AgentEvent::Checkpoint { reason, .. } => eprintln!("  checkpoint: {reason}"),
                AgentEvent::Warning { message, .. } => eprintln!("  note: {message}"),
                AgentEvent::ApprovalRequested { .. } | AgentEvent::ApprovalResolved { .. } => {}
                AgentEvent::ContextCompiled { stats, .. } => eprintln!("  context: ~{} prompt tokens, {} snippets, {} memories{}", stats.estimated_prompt_tokens, stats.repository_snippets, stats.memory_records, if stats.compacted { " (compacted)" } else { "" }),
                AgentEvent::AgentStarted { agent_id, name, mode, objective, .. } => eprintln!("{} agent {name} [{agent_id}] ({}) started: {}", crate::transcript::glyphs().agents, mode.as_str(), objective.lines().next().unwrap_or("").chars().take(120).collect::<String>()),
                AgentEvent::AgentProgress { agent_id, inner, .. } => match *inner {
                    AgentEvent::ToolCallStarted { name, arguments, .. } => eprintln!("  [{agent_id}] → {name} {}", arguments.to_string().chars().take(120).collect::<String>()),
                    AgentEvent::ToolCallFinished { success, summary, duration_ms, .. } => eprintln!("  [{agent_id}]   {} {summary} ({duration_ms} ms)", if success { crate::transcript::glyphs().ok } else { crate::transcript::glyphs().fail }),
                    AgentEvent::FileChanged { path, kind, .. } => eprintln!("  [{agent_id}]   file {kind:?}: {path}"),
                    _ => {}
                },
                AgentEvent::AgentFinished { agent_id, report, .. } => eprintln!("{} agent {} [{agent_id}] {}: {} ({} steps, {} tool calls, {} ms)", crate::transcript::glyphs().agents, report.name, report.status.as_str(), report.summary.lines().next().unwrap_or("").chars().take(160).collect::<String>(), report.steps, report.tool_calls, report.duration_ms),
                AgentEvent::TaskCompleted { outcome: o, .. } => outcome = Some(o),
                _ => {}
            }
        }
        outcome
    });

    let cancel = CancellationToken::new();
    {
        let cancel = cancel.clone();
        tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                eprintln!("\ninterrupt received; cancelling …");
                cancel.cancel();
            }
        });
    }
    if let Some(secs) = opts.timeout {
        let cancel = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(secs)).await;
            eprintln!("timeout reached; cancelling …");
            cancel.cancel();
        });
    }

    let mcp = app::mcp_manager(&config, &ctx.root);
    let runtime = AgentRuntime::new(ctx.clone(), provider, approvals).with_event_sink(tx).with_cancel(cancel).with_options(run_options).with_mcp(mcp.clone());
    let session = match (&opts.session, opts.new_session) {
        (Some(id), _) => runtime.sessions().resume(id)?,
        (None, false) => match runtime.sessions().resume_latest()? {
            Some(s) => s,
            None => runtime.sessions().start("headless", &config.model.name)?,
        },
        (None, true) => runtime.sessions().start("headless", &config.model.name)?,
    };
    if !json {
        let effort = config.model.reasoning_effort.as_ref().map(|e| format!(" · {} effort{}", e.as_str(), if e.deploys_agents() { " (sub-agents enabled)" } else { "" })).unwrap_or_default();
        eprintln!("session {} · task starting in {}{effort}", &session.id[..8], ctx.root.display());
    }
    let outcome = runtime.run(&session, &opts.prompt).await?;
    drop(runtime);
    let _ = printer.await;

    if json {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({"session_id": session.id, "outcome": outcome}))?);
    } else {
        println!("\n=== {} ({} steps, {} model calls, {} tool calls, {:.1}s) ===", outcome.status.as_str().to_uppercase(), outcome.steps, outcome.model_calls, outcome.tool_calls, outcome.duration_ms as f64 / 1000.0);
        if !outcome.summary.trim().is_empty() {
            println!("{}", outcome.summary.trim());
        }
        if !outcome.modified_files.is_empty() {
            println!("\nModified files: {}", outcome.modified_files.join(", "));
        }
        for v in &outcome.validation {
            println!("Validation `{}`: {} — {}", v.command, if v.passed { "PASSED" } else { "FAILED" }, v.summary);
        }
        if !outcome.unresolved.is_empty() {
            println!("\nUnresolved:");
            for u in &outcome.unresolved {
                println!("  - {u}");
            }
        }
        println!("\nUsage: {}", app::format_usage(&outcome.usage, outcome.cost_usd));
        println!("Task {} in session {}", outcome.task_id, session.id);
    }
    mcp.shutdown().await;
    Ok(app::exit_code_for(outcome.status))
}
