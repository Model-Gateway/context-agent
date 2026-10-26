//! `ctx` — Context CLI (spec §12).

mod account;
mod app;
mod billing;
mod commands;
mod computer;
mod gateway;
mod headless;
mod model_stats;
mod models_dev;
mod resume;
mod serve;
mod slash;
mod transcript;
mod tui;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "ctx", version, about = "Context: a local-first, model-agnostic coding agent with an intelligent context engine.")]
pub struct Cli {
    /// Explicit configuration file (overrides the layered defaults).
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,
    /// Repository directory (defaults to the current directory's git root).
    #[arg(long, global = true)]
    pub repo: Option<PathBuf>,
    /// Model name override (model.name).
    #[arg(long, global = true)]
    pub model: Option<String>,
    /// Permission profile override: approval_required | workspace_autonomous | bypass_permissions.
    #[arg(long, global = true)]
    pub profile: Option<String>,
    /// Reasoning effort override: one of the model's effort values (low, high, max, …) or ultra (its strongest, plus sub-agents).
    #[arg(long, global = true)]
    pub effort: Option<String>,
    /// Application data directory (defaults to ~/.context-agent or $CTX_DATA_DIR).
    #[arg(long, global = true)]
    pub data_dir: Option<PathBuf>,
    /// Increase log verbosity (-v info, -vv debug, -vvv trace).
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,
    /// Continue this repository's most recent conversation.
    #[arg(short = 'c', long = "continue", conflicts_with = "resume")]
    pub continue_session: bool,
    /// Resume a conversation by id (or its first characters); without an id, pick one from a list.
    #[arg(short = 'r', long, num_args = 0..=1, default_missing_value = "", value_name = "ID")]
    pub resume: Option<String>,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Write a configuration template and check the environment.
    Init {
        /// Also write a repository-level `.context-agent.toml`.
        #[arg(long = "repo-config")]
        repo_config: bool,
        #[arg(long)]
        force: bool,
    },
    /// Run one task headlessly.
    Run {
        /// The task to perform.
        #[arg(required = true)]
        prompt: Vec<String>,
        /// Emit machine-readable JSON (events on stderr, outcome on stdout).
        #[arg(long)]
        json: bool,
        /// Approval policy: ask (stdin), auto, deny.
        #[arg(long, default_value = "ask")]
        approval: String,
        /// Abort after this many seconds.
        #[arg(long)]
        timeout: Option<u64>,
        #[arg(long)]
        max_steps: Option<u32>,
        /// Disable streaming output.
        #[arg(long)]
        no_stream: bool,
        /// Skip provider capability probing when no profile is cached.
        #[arg(long)]
        no_probe: bool,
        /// Continue a specific session (default: the latest session).
        #[arg(long)]
        session: Option<String>,
        /// Start a fresh session.
        #[arg(long)]
        new_session: bool,
        /// Evaluation variant to emulate (A–E).
        #[arg(long)]
        variant: Option<String>,
    },
    /// Repository index operations.
    Index {
        #[command(subcommand)]
        cmd: IndexCmd,
    },
    /// Inspect how context is assembled.
    Context {
        #[command(subcommand)]
        cmd: ContextCmd,
    },
    /// Persistent memory.
    Memory {
        #[command(subcommand)]
        cmd: MemoryCmd,
    },
    /// Tasks and their history.
    Task {
        #[command(subcommand)]
        cmd: TaskCmd,
    },
    /// Show the working-tree diff.
    Diff {
        path: Option<String>,
    },
    /// Benchmarks and retrieval evaluation.
    Eval {
        #[command(subcommand)]
        cmd: EvalCmd,
    },
    /// Check configuration, provider, toolchains, language servers and index health.
    Doctor {
        /// Run live provider capability checks and save the profile.
        #[arg(long)]
        probe: bool,
    },
    /// Model Context Protocol servers: list, inspect, add, remove.
    Mcp {
        #[command(subcommand)]
        cmd: McpCmd,
    },
    /// Run one of ctx's built-in MCP servers over stdio (used by the `builtin` transport).
    #[command(name = "mcp-serve", hide = true)]
    McpServe {
        /// computer-use
        name: String,
    },
    /// Serve the desktop app: JSON-RPC 2.0 over stdin/stdout (docs/desktop-protocol.md).
    Serve {
        /// Replace the model provider with an offline scripted mock.
        #[arg(long)]
        mock: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum McpCmd {
    /// Configured servers, connected, with their state and tool counts.
    List,
    /// The tools a server offers (every connected server without a name).
    Tools {
        server: Option<String>,
    },
    /// Add a server to ~/.context-agent/config.toml (replacing one of the same name).
    Add {
        /// Letters, digits, `_` and `-`; prefixes the server's tool names.
        name: String,
        /// Streamable HTTP endpoint instead of a command.
        #[arg(long, conflicts_with = "command")]
        url: Option<String>,
        /// `KEY=VALUE` added to a stdio server's environment, or an HTTP header `Name: value` / `Name=value` (`${VAR}` reads an environment variable when the server starts).
        #[arg(long = "env", short = 'e', value_name = "KEY=VALUE")]
        env: Vec<String>,
        /// HTTP header sent with every request (`Authorization: Bearer ${TOKEN}`).
        #[arg(long = "header", short = 'H', value_name = "NAME: VALUE")]
        headers: Vec<String>,
        /// Working directory of a stdio server (default: the repository).
        #[arg(long)]
        cwd: Option<String>,
        /// Offer only these tools (repeatable).
        #[arg(long = "tool", value_name = "NAME")]
        tools: Vec<String>,
        /// Add the server switched off.
        #[arg(long)]
        disabled: bool,
        /// The command and its arguments (after `--`).
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
    /// Remove a server from the user configuration.
    Remove {
        name: String,
    },
    /// Switch a server on.
    Enable {
        name: String,
    },
    /// Switch a server off without removing it.
    Disable {
        name: String,
    },
    /// The app catalog: curated servers that `install` sets up.
    Catalog,
    /// Install an app from the catalog (`--env KEY=VALUE` fills its inputs; `${VAR}` reads the environment at start).
    Install {
        id: String,
        #[arg(long = "env", short = 'e', value_name = "KEY=VALUE")]
        env: Vec<String>,
    },
    /// Remove an installed app (a default app stays removed until installed again).
    Uninstall {
        id: String,
    },
    /// Sign in to an app's service in the browser (GitHub device flow) and install it with that sign-in.
    Login {
        id: String,
    },
    /// Forget an app's stored sign-in.
    Logout {
        id: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum IndexCmd {
    Status,
    Rebuild,
    /// Watch the repository and index changes until interrupted.
    Watch,
}

#[derive(Subcommand, Debug)]
pub enum ContextCmd {
    /// Why each source was selected (latest task, or a dry run for --query).
    Explain {
        #[arg(long)]
        query: Option<String>,
        #[arg(long)]
        task: Option<String>,
    },
    /// The message layers of the latest compiled request.
    Show {
        #[arg(long)]
        task: Option<String>,
    },
    /// Selected sources only.
    Sources {
        #[arg(long)]
        query: Option<String>,
        #[arg(long)]
        task: Option<String>,
    },
    /// Budget and size statistics.
    Stats {
        #[arg(long)]
        query: Option<String>,
        #[arg(long)]
        task: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum MemoryCmd {
    List {
        #[arg(long)]
        category: Option<String>,
        /// Include invalidated records.
        #[arg(long)]
        all: bool,
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    Search {
        query: String,
    },
    /// Invalidate a record.
    Forget {
        id: String,
        #[arg(long, default_value = "invalidated by user")]
        reason: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum TaskCmd {
    List {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    Show {
        id: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum EvalCmd {
    /// Run benchmark tasks across variants.
    Run {
        /// Task directory or a single task file.
        #[arg(default_value = "benchmarks/tasks")]
        tasks: PathBuf,
        /// Variants to run (comma separated, default all): A,B,C,D,E
        #[arg(long)]
        variants: Option<String>,
        /// Only these task ids (comma separated).
        #[arg(long)]
        filter: Option<String>,
        /// Use a mock provider that answers immediately (harness smoke test).
        #[arg(long)]
        mock: bool,
        /// Write the JSON report here (default benchmarks/results/<timestamp>.json).
        #[arg(long)]
        output: Option<PathBuf>,
        /// Keep snapshot directories for inspection.
        #[arg(long)]
        keep: bool,
        /// Earlier report(s) to resume from: valid results are kept; skipped
        /// runs and runs that hit provider/runtime errors are repeated.
        #[arg(long)]
        resume: Vec<PathBuf>,
        /// Override model.max_retries for this run (rides out longer
        /// gateway outages and rate limits).
        #[arg(long)]
        max_retries: Option<u32>,
    },
    /// Measure retrieval recall over labeled queries.
    Retrieval {
        #[arg(default_value = "benchmarks/retrieval/queries.jsonl")]
        queries: PathBuf,
        #[arg(long)]
        output: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() {
    ctx_core::platform::keep_std_handles_private();
    let cli = Cli::parse();
    let code = match run(cli).await {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            e.exit_code()
        }
    };
    std::process::exit(code);
}

async fn run(cli: Cli) -> ctx_core::Result<i32> {
    match cli.command {
        None => tui::run(&cli).await,
        Some(Command::Init { repo_config, force }) => commands::init(&cli, repo_config, force),
        Some(Command::Run { ref prompt, json, ref approval, timeout, max_steps, no_stream, no_probe, ref session, new_session, ref variant }) => {
            let opts = headless::HeadlessOptions { prompt: prompt.join(" "), json, approval: approval.clone(), timeout, max_steps, stream: !no_stream, probe: !no_probe, session: session.clone(), new_session, variant: variant.clone() };
            headless::run(&cli, opts).await
        }
        Some(Command::Index { ref cmd }) => commands::index(&cli, cmd).await,
        Some(Command::Context { ref cmd }) => commands::context(&cli, cmd).await,
        Some(Command::Memory { ref cmd }) => commands::memory(&cli, cmd),
        Some(Command::Task { ref cmd }) => commands::task(&cli, cmd),
        Some(Command::Diff { ref path }) => commands::diff(&cli, path.as_deref()),
        Some(Command::Eval { ref cmd }) => commands::eval(&cli, cmd).await,
        Some(Command::Doctor { probe }) => commands::doctor(&cli, probe).await,
        Some(Command::Mcp { ref cmd }) => commands::mcp(&cli, cmd).await,
        Some(Command::McpServe { ref name }) => computer::serve(name).await,
        Some(Command::Serve { mock }) => serve::run(&cli, mock).await,
    }
}
