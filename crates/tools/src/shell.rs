//! Command execution: execute_command, run_validation, git_status, git_diff.

use crate::registry::{Tool, ToolContext, ToolEffect, ToolOutput};
use async_trait::async_trait;
use ctx_core::{CtxError, Result, ValidationRecord};
use ctx_indexing::ValidationKind;
use ctx_security::{Action, ActionKind};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;

/// Output kept per stream; anything past this is read and discarded so a
/// chatty background process never blocks on a full pipe.
const OUTPUT_CAP: usize = 4 * 1024 * 1024;
/// How long to keep collecting output after the shell itself exits. A
/// process the command backgrounded (`server &`) can hold the pipes open
/// indefinitely, so the tool returns once this passes instead of waiting
/// for it to exit.
const DRAIN_GRACE: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
pub struct CommandResult {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
    pub timed_out: bool,
    /// The shell exited but processes it started in the background were
    /// still holding its output open; they keep running.
    pub left_running: bool,
}

impl CommandResult {
    pub fn success(&self) -> bool {
        self.exit_code == Some(0) && !self.timed_out
    }

    pub fn combined(&self) -> String {
        match (self.stdout.trim().is_empty(), self.stderr.trim().is_empty()) {
            (true, true) => String::new(),
            (false, true) => self.stdout.clone(),
            (true, false) => format!("[stderr]\n{}", self.stderr),
            (false, false) => format!("{}\n[stderr]\n{}", self.stdout, self.stderr),
        }
    }

    /// Last informative line (test summaries, error lines).
    pub fn summary_line(&self) -> String {
        let text = self.combined();
        let lines: Vec<&str> = text.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
        let preferred = lines.iter().rev().find(|l| {
            let lower = l.to_lowercase();
            lower.contains("test result") || lower.contains("passed") || lower.contains("failed") || lower.starts_with("ok") || lower.starts_with("fail") || lower.contains("error")
        });
        preferred.or(lines.last()).map(|l| l.chars().take(200).collect()).unwrap_or_default()
    }
}

/// Run a shell command with a timeout, cancellation, and captured output.
///
/// The command runs in its own process group so a timeout or cancel kills
/// everything it started, and the call never waits on output pipes beyond
/// the timeout: background processes that outlive the shell are left
/// running and reported through `left_running`.
pub async fn run_shell(ctx: &ToolContext, command: &str, cwd: &Path, timeout: Duration) -> Result<CommandResult> {
    run_shell_in(ctx, &ctx.shell, command, cwd, timeout).await
}

/// [`run_shell`] in `shell` instead of the session's shell.
pub async fn run_shell_in(ctx: &ToolContext, shell: &str, command: &str, cwd: &Path, timeout: Duration) -> Result<CommandResult> {
    let wrapped = ctx.policy.sandbox().wrap_command(shell, command);
    let program = wrapped.program.clone();
    let started = Instant::now();
    let deadline = tokio::time::Instant::now() + timeout;
    let mut cmd = tokio::process::Command::new(&program);
    cmd.args(&wrapped.args)
        .current_dir(cwd)
        .env("NO_COLOR", "1")
        .env("FORCE_COLOR", "0")
        .env("CLICOLOR", "0")
        .env("TERM", "dumb")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("CTX_AGENT", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    cmd.process_group(0);
    #[cfg(windows)]
    {
        // No console window (the desktop app runs ctx without one) and its
        // own process group, so the user's Ctrl+C in the TUI's console does
        // not reach it, like the Unix process group above.
        cmd.creation_flags(ctx_core::platform::CREATE_NO_WINDOW | ctx_core::platform::CREATE_NEW_PROCESS_GROUP);
        if let Some(tail) = &wrapped.raw_tail {
            cmd.raw_arg(tail);
        }
    }
    #[cfg(not(windows))]
    if let Some(tail) = &wrapped.raw_tail {
        cmd.arg(tail.trim_matches('"'));
    }
    let mut child = cmd.spawn().map_err(|e| CtxError::tool(format!("failed to start `{program}`: {e}")))?;
    let group = ProcessTree::adopt(&child);
    let stdout = Capture::spawn(child.stdout.take().unwrap());
    let stderr = Capture::spawn(child.stderr.take().unwrap());
    let mut timed_out = false;
    let status = tokio::select! {
        s = child.wait() => Some(s.map_err(|e| CtxError::tool(e.to_string()))?),
        _ = tokio::time::sleep_until(deadline) => {
            timed_out = true;
            group.kill();
            let _ = child.kill().await;
            None
        }
        _ = ctx.cancel.cancelled() => {
            group.kill();
            let _ = child.kill().await;
            return Err(CtxError::Cancelled);
        }
    };
    // Collect what is left in the pipes, but only briefly: anything still
    // holding them is a background process that may never exit.
    let drain_until = tokio::time::Instant::now() + DRAIN_GRACE;
    let drained = tokio::select! {
        _ = async { stdout.done().await; stderr.done().await } => true,
        _ = tokio::time::sleep_until(drain_until) => false,
        _ = ctx.cancel.cancelled() => return Err(CtxError::Cancelled),
    };
    Ok(CommandResult {
        exit_code: status.and_then(|s| s.code()),
        stdout: stdout.text(),
        stderr: stderr.text(),
        duration_ms: started.elapsed().as_millis() as u64,
        timed_out,
        left_running: !drained && !timed_out,
    })
}

/// Reads one output pipe to EOF on its own task, keeping the first
/// `OUTPUT_CAP` bytes. The task outlives `run_shell` when a background
/// process still holds the pipe, so that process never sees a closed pipe.
struct Capture {
    buf: std::sync::Arc<parking_lot::Mutex<Vec<u8>>>,
    eof: tokio::sync::watch::Receiver<bool>,
}

impl Capture {
    fn spawn<R: tokio::io::AsyncRead + Unpin + Send + 'static>(mut pipe: R) -> Self {
        let buf = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
        let (tx, eof) = tokio::sync::watch::channel(false);
        let sink = buf.clone();
        tokio::spawn(async move {
            let mut chunk = [0u8; 8192];
            while let Ok(n) = pipe.read(&mut chunk).await {
                if n == 0 {
                    break;
                }
                let mut b = sink.lock();
                let room = OUTPUT_CAP.saturating_sub(b.len());
                b.extend_from_slice(&chunk[..n.min(room)]);
            }
            let _ = tx.send(true);
        });
        Self { buf, eof }
    }

    async fn done(&self) {
        let mut eof = self.eof.clone();
        let _ = eof.wait_for(|done| *done).await;
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.buf.lock()).to_string()
    }
}

/// Everything a command started, so a timeout or cancel can kill it all:
/// the process group it leads on Unix, a Job Object on Windows (which also
/// catches grandchildren whose parent already exited). Background processes
/// a command leaves running on purpose are not killed when this is dropped.
struct ProcessTree {
    #[cfg(unix)]
    pgid: Option<i32>,
    #[cfg(windows)]
    job: Option<windows_job::Job>,
}

impl ProcessTree {
    fn adopt(child: &tokio::process::Child) -> Self {
        #[cfg(unix)]
        {
            Self { pgid: child.id().and_then(|p| i32::try_from(p).ok()).filter(|p| *p > 0) }
        }
        #[cfg(windows)]
        {
            Self { job: child.raw_handle().and_then(windows_job::Job::assign) }
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = child;
            Self {}
        }
    }

    /// SIGKILL the process group / terminate every process in the job.
    fn kill(&self) {
        #[cfg(unix)]
        if let Some(pgid) = self.pgid {
            // SAFETY: killpg only sends a signal; a stale pgid fails with ESRCH.
            unsafe {
                libc::killpg(pgid, libc::SIGKILL);
            }
        }
        #[cfg(windows)]
        if let Some(job) = &self.job {
            job.terminate();
        }
    }
}

#[cfg(windows)]
mod windows_job {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{AssignProcessToJobObject, CreateJobObjectW, TerminateJobObject};

    /// An unnamed Job Object holding one command's process tree. It is
    /// created without `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, so closing the
    /// handle leaves background processes running.
    pub struct Job(HANDLE);

    // SAFETY: a job handle is a kernel object handle, usable from any thread.
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    impl Job {
        pub fn assign(process: std::os::windows::io::RawHandle) -> Option<Job> {
            // SAFETY: plain Win32 calls on a handle we own; failures return null / 0.
            unsafe {
                let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if job.is_null() {
                    return None;
                }
                if AssignProcessToJobObject(job, process as HANDLE) == 0 {
                    CloseHandle(job);
                    return None;
                }
                Some(Job(job))
            }
        }

        pub fn terminate(&self) {
            // SAFETY: the handle is valid until Drop.
            unsafe {
                TerminateJobObject(self.0, 1);
            }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // SAFETY: closing our own handle once.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

fn resolve_cwd(ctx: &ToolContext, cwd: Option<&str>) -> Result<PathBuf> {
    match cwd {
        Some(c) if !c.is_empty() && c != "." => {
            let abs = ctx.policy.resolve_path(c)?;
            if !abs.is_dir() {
                return Err(CtxError::invalid(format!("cwd {c} is not a directory")));
            }
            Ok(abs)
        }
        _ => Ok(ctx.root.clone()),
    }
}

fn tail_lines(text: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= max_lines {
        return text.to_string();
    }
    format!("… [{} earlier line(s) omitted]\n{}", lines.len() - max_lines, lines[lines.len() - max_lines..].join("\n"))
}

// ---------------------------------------------------------------------------

pub struct ExecuteCommand;

/// The description, with a note about the host shell on Windows. Computed
/// once per process and constant afterwards, so the tool list stays
/// byte-identical across requests (provider prompt caches keep hitting).
fn execute_command_description() -> &'static str {
    static DESCRIPTION: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    DESCRIPTION.get_or_init(|| {
        let shell = ctx_core::platform::default_shell();
        let background = match shell.kind {
            ctx_core::platform::ShellKind::PowerShell => "`Start-Process -NoNewWindow -RedirectStandardOutput $env:TEMP\\x.log <program> <args>`",
            ctx_core::platform::ShellKind::Cmd => "`start /b cmd /c \"<command> > %TEMP%\\x.log 2>&1\"`",
            ctx_core::platform::ShellKind::Posix => "`cmd > /tmp/x.log 2>&1 &`",
        };
        let powershell = if ctx_core::platform::powershell_alongside().is_some() {
            " For Windows administration (disks, services, the registry, CIM/WMI, event logs) pass `shell: \"powershell\"` and write plain PowerShell. Never wrap a script in `powershell -Command` inside bash: bash rewrites its `$` and quotes first."
        } else {
            ""
        };
        format!(
            "Run a shell command in the repository (builds, tests, linters, inspection). Output is captured; long-running commands are killed at the timeout. To start a server or watcher, background it with its output redirected ({background}); the call returns once the shell exits and the process keeps running. Destructive or network commands may require user approval.{}{powershell}",
            shell.prompt_note()
        )
    })
}

/// The parameter schema, with a `shell` choice when PowerShell runs
/// alongside the default shell. Built once, like the description.
fn execute_command_parameters() -> &'static Value {
    static PARAMETERS: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    PARAMETERS.get_or_init(|| {
        let mut parameters = json!({
            "type": "object",
            "properties": {
                "command": {"type": "string", "minLength": 1},
                "cwd": {"type": "string", "description": "Working directory relative to the repository root"},
                "timeout_secs": {"type": "integer", "minimum": 1, "maximum": 3600}
            },
            "required": ["command"],
            "additionalProperties": false
        });
        if ctx_core::platform::powershell_alongside().is_some() {
            let default = ctx_core::platform::default_shell().display_name();
            parameters["properties"]["shell"] = json!({
                "type": "string",
                "enum": [default, "powershell"],
                "description": format!("The shell that runs `command` (default {default})")
            });
        }
        parameters
    })
}

#[derive(Deserialize)]
struct ExecArgs {
    command: String,
    cwd: Option<String>,
    timeout_secs: Option<u64>,
    shell: Option<String>,
}

/// The shell program `shell: "powershell"` asks for: the session's shell
/// when it already is PowerShell, else PowerShell alongside it.
fn requested_shell(ctx: &ToolContext, shell: Option<&str>) -> Result<String> {
    if shell != Some("powershell") || ctx_core::platform::Shell::from_program(ctx.shell.as_str()).kind == ctx_core::platform::ShellKind::PowerShell {
        return Ok(ctx.shell.clone());
    }
    ctx_core::platform::powershell_alongside().map(|s| s.program).ok_or_else(|| CtxError::tool("PowerShell is not available on this host"))
}

#[async_trait]
impl Tool for ExecuteCommand {
    fn name(&self) -> &str {
        "execute_command"
    }

    fn description(&self) -> &str {
        execute_command_description()
    }

    fn parameters(&self) -> Value {
        execute_command_parameters().clone()
    }

    fn action(&self, args: &Value, ctx: &ToolContext) -> Result<Action> {
        let command = args["command"].as_str().unwrap_or_default().to_string();
        let cwd = args["cwd"].as_str().unwrap_or(".");
        let analysis = ctx.policy.classifier().classify(&command);
        let shell = match args["shell"].as_str() {
            Some("powershell") => "\nshell: PowerShell",
            _ => "",
        };
        let details = format!("$ {command}\ncwd: {cwd}{shell}\nclassification: {} ({})", analysis.class.as_str(), analysis.reasons.join("; "));
        Ok(Action { tool: "execute_command".into(), kind: ActionKind::Command { command: command.clone() }, summary: format!("run: {command}"), details })
    }

    async fn invoke(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let a: ExecArgs = serde_json::from_value(args)?;
        let cwd = resolve_cwd(ctx, a.cwd.as_deref())?;
        let shell = requested_shell(ctx, a.shell.as_deref())?;
        let timeout = Duration::from_secs(a.timeout_secs.unwrap_or(ctx.command_timeout.as_secs().max(1)));
        let result = run_shell_in(ctx, &shell, &a.command, &cwd, timeout).await?;
        let status = if result.timed_out { format!("TIMED OUT after {}s", timeout.as_secs()) } else { format!("exit {}", result.exit_code.map(|c| c.to_string()).unwrap_or_else(|| "signal".into())) };
        let mut out = format!("$ {}\n({status}, {} ms)\n{}", a.command, result.duration_ms, result.combined());
        if result.left_running {
            out.push_str("\n[background process(es) started by this command are still running and holding its output open; output after this point is not captured — redirect it to a file to read it later]");
        }
        let mut output = ToolOutput::ok(out, format!("{} → {status}", a.command.chars().take(60).collect::<String>()));
        output.is_error = result.timed_out || result.exit_code.is_none();
        Ok(output.with_effect(ToolEffect::CommandRun { command: a.command, exit_code: result.exit_code, duration_ms: result.duration_ms }))
    }
}

// ---------------------------------------------------------------------------

pub struct RunValidation;

#[derive(Deserialize)]
struct ValidationArgs {
    kind: Option<String>,
    command: Option<String>,
    cwd: Option<String>,
}

fn kind_matches(kind: &str, k: ValidationKind) -> bool {
    match kind {
        "test" => k == ValidationKind::Test,
        "build" => k == ValidationKind::Build,
        "typecheck" => k == ValidationKind::Typecheck,
        "lint" => k == ValidationKind::Lint,
        "all" => true,
        _ => false,
    }
}

fn select_commands(ctx: &ToolContext, kind: &str, override_cmd: Option<&str>, cwd: Option<&str>) -> Vec<(String, String)> {
    if let Some(c) = override_cmd {
        return vec![(c.to_string(), cwd.unwrap_or(".").to_string())];
    }
    // Every command of the requested kind runs; a batch passes only when
    // all of them pass.
    ctx.validation_commands.iter().filter(|v| kind_matches(kind, v.kind)).map(|v| (v.command.clone(), v.cwd.clone())).collect()
}

#[async_trait]
impl Tool for RunValidation {
    fn name(&self) -> &str {
        "run_validation"
    }

    fn description(&self) -> &str {
        "Run the project's own validation commands (detected from package.json, Cargo.toml, pyproject.toml, go.mod, Makefile, …). kind=test (default), build, typecheck, lint or all. Pass `command` to run a specific command as validation instead."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "kind": {"type": "string", "enum": ["test", "build", "typecheck", "lint", "all"]},
                "command": {"type": "string", "description": "Explicit validation command (overrides detection)"},
                "cwd": {"type": "string"}
            },
            "additionalProperties": false
        })
    }

    fn action(&self, args: &Value, ctx: &ToolContext) -> Result<Action> {
        let kind = args["kind"].as_str().unwrap_or("test");
        let commands = select_commands(ctx, kind, args["command"].as_str(), args["cwd"].as_str());
        if commands.is_empty() {
            return Err(CtxError::tool(format!("no {kind} validation command was detected for this repository; run the project's command with execute_command or pass `command`")));
        }
        let joined = commands.iter().map(|(c, d)| if d == "." { c.clone() } else { format!("(cd {d} && {c})") }).collect::<Vec<_>>().join(" && ");
        // A model-supplied override is an arbitrary command and is gated
        // exactly like execute_command; only commands drawn from
        // configuration or build-file detection count as validation.
        let is_detected = args["command"].as_str().map(|c| ctx.validation_commands.iter().any(|v| v.command == c)).unwrap_or(true);
        let action_kind = if is_detected { ActionKind::Validation { command: joined.clone() } } else { ActionKind::Command { command: joined.clone() } };
        Ok(Action { tool: "run_validation".into(), kind: action_kind, summary: format!("validate ({kind}): {joined}"), details: commands.iter().map(|(c, d)| format!("{d}: {c}")).collect::<Vec<_>>().join("\n") })
    }

    async fn invoke(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let a: ValidationArgs = serde_json::from_value(args)?;
        let kind = a.kind.as_deref().unwrap_or("test");
        let commands = select_commands(ctx, kind, a.command.as_deref(), a.cwd.as_deref());
        let mut out = String::new();
        let mut output = ToolOutput::ok(String::new(), String::new());
        let mut all_passed = true;
        for (command, cwd) in &commands {
            let dir = resolve_cwd(ctx, Some(cwd))?;
            let result = run_shell(ctx, command, &dir, ctx.command_timeout.max(Duration::from_secs(60))).await?;
            let passed = result.success();
            all_passed &= passed;
            let summary = if result.timed_out { format!("timed out after {}s", ctx.command_timeout.as_secs()) } else { result.summary_line() };
            out.push_str(&format!("### {command}{} → {} (exit {}, {} ms)\n{}\n\n", if cwd == "." { String::new() } else { format!(" [cwd {cwd}]") }, if passed { "PASSED" } else { "FAILED" }, result.exit_code.map(|c| c.to_string()).unwrap_or_else(|| "none".into()), result.duration_ms, tail_lines(&result.combined(), 150)));
            output.effects.push(ToolEffect::Validation { record: ValidationRecord { command: command.clone(), passed, exit_code: result.exit_code, duration_ms: result.duration_ms, summary } });
            output.effects.push(ToolEffect::CommandRun { command: command.clone(), exit_code: result.exit_code, duration_ms: result.duration_ms });
        }
        let n = commands.len();
        output.content = format!("Validation ({kind}): {} of {n} command(s) passed.\n\n{out}", output.effects.iter().filter(|e| matches!(e, ToolEffect::Validation { record } if record.passed)).count());
        output.full_len = output.content.len();
        output.summary = format!("validation {kind}: {}", if all_passed { "all passed".to_string() } else { format!("{} of {n} passed", n - output.effects.iter().filter(|e| matches!(e, ToolEffect::Validation { record } if !record.passed)).count()) });
        Ok(output)
    }
}

// ---------------------------------------------------------------------------

pub struct GitStatus;

#[async_trait]
impl Tool for GitStatus {
    fn name(&self) -> &str {
        "git_status"
    }

    fn description(&self) -> &str {
        "Show the git branch and working-tree status (staged, modified, untracked files)."
    }

    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {}, "additionalProperties": false})
    }

    fn action(&self, _args: &Value, _ctx: &ToolContext) -> Result<Action> {
        Ok(Action { tool: "git_status".into(), kind: ActionKind::GitInspect, summary: "git status".into(), details: String::new() })
    }

    async fn invoke(&self, _args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let result = run_shell(ctx, "git status --porcelain=v1 --branch", &ctx.root, Duration::from_secs(30)).await?;
        if !result.success() {
            return Ok(ToolOutput::error(format!("git status failed: {}", result.combined().trim())));
        }
        let text = result.stdout;
        let mut lines = text.lines();
        let branch = lines.next().unwrap_or("").trim_start_matches("## ").to_string();
        let entries: Vec<&str> = lines.collect();
        let mut out = format!("Branch: {branch}\n{} changed path(s)\n", entries.len());
        for e in entries.iter().take(200) {
            out.push_str(e);
            out.push('\n');
        }
        if entries.len() > 200 {
            out.push_str(&format!("… {} more\n", entries.len() - 200));
        }
        Ok(ToolOutput::ok(out, format!("git status: {} change(s) on {branch}", entries.len())))
    }
}

pub struct GitDiff;

#[derive(Deserialize)]
struct DiffArgs {
    path: Option<String>,
    #[serde(default)]
    staged: bool,
    context_lines: Option<u32>,
}

#[async_trait]
impl Tool for GitDiff {
    fn name(&self) -> &str {
        "git_diff"
    }

    fn description(&self) -> &str {
        "Show the working-tree diff (or the staged diff), optionally limited to one path. Use it to review your own changes before finishing."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "staged": {"type": "boolean"},
                "context_lines": {"type": "integer", "minimum": 0, "maximum": 20}
            },
            "additionalProperties": false
        })
    }

    fn action(&self, _args: &Value, _ctx: &ToolContext) -> Result<Action> {
        Ok(Action { tool: "git_diff".into(), kind: ActionKind::GitInspect, summary: "git diff".into(), details: String::new() })
    }

    async fn invoke(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let a: DiffArgs = serde_json::from_value(args)?;
        let mut cmd = String::from("git --no-pager diff");
        if a.staged {
            cmd.push_str(" --staged");
        }
        if let Some(n) = a.context_lines {
            cmd.push_str(&format!(" -U{n}"));
        }
        if let Some(p) = &a.path {
            let abs = ctx.policy.paths().resolve(p)?;
            cmd.push_str(&format!(" -- {}", ctx_core::platform::Shell::from_program(ctx.shell.as_str()).quote(&ctx.relative(&abs))));
        }
        let result = run_shell(ctx, &cmd, &ctx.root, Duration::from_secs(60)).await?;
        if !result.success() {
            return Ok(ToolOutput::error(format!("git diff failed: {}", result.combined().trim())));
        }
        let stat = run_shell(ctx, &format!("{cmd} --stat"), &ctx.root, Duration::from_secs(60)).await.map(|r| r.stdout).unwrap_or_default();
        let diff = result.stdout;
        let out = if diff.trim().is_empty() { "No differences.".to_string() } else { format!("{}\n{}", stat.trim_end(), diff) };
        let files = stat.lines().filter(|l| l.contains('|')).count();
        Ok(ToolOutput::ok(out, format!("git diff: {files} file(s)")))
    }
}
