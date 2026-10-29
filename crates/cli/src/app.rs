//! Shared setup for all commands.

use crate::Cli;
use ctx_agent::RepositoryContext;
use ctx_core::{Config, CtxError, LoadOptions, PermissionProfile, ReasoningEffort, RepositoryId, Result};
use ctx_indexing::RepositoryDiscovery;
use ctx_providers::{build_provider, load_profile, save_profile, ModelProvider};
use ctx_telemetry::{init_tracing, TracingTarget};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub struct App {
    pub config: Config,
    pub ctx: Arc<RepositoryContext>,
}

/// The repository to work on: `--repo` exactly as given, otherwise the git
/// root containing the current directory (or the directory itself).
pub fn repository_root(cli: &Cli) -> PathBuf {
    match &cli.repo {
        Some(repo) => ctx_core::platform::canonicalize(repo).unwrap_or_else(|_| repo.clone()),
        None => RepositoryDiscovery::find_root(&std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))),
    }
}

pub fn load_config(cli: &Cli) -> Result<Config> {
    let root = repository_root(cli);
    let mut config = Config::load(&LoadOptions { explicit_path: cli.config.clone(), repository_root: Some(root.clone()), data_dir: cli.data_dir.clone() })?;
    let explicit_model = cli.model.is_some() || std::env::var_os("CTX_MODEL_NAME").is_some();
    apply_remembered_model(&mut config, &root, explicit_model);
    let explicit_profile = cli.profile.is_some() || std::env::var_os("CTX_PERMISSION_PROFILE").is_some();
    apply_remembered_profile(&mut config, &root, explicit_profile);
    apply_remembered_effort(&mut config, &root, cli.effort.is_some());
    if let Some(model) = &cli.model {
        config.model.name = model.clone();
    }
    if let Some(profile) = &cli.profile {
        config.permissions.profile = PermissionProfile::parse(profile).ok_or_else(|| CtxError::Config(format!("unknown permission profile `{profile}`")))?;
    }
    if let Some(effort) = &cli.effort {
        config.model.reasoning_effort = Some(ReasoningEffort::parse(effort).ok_or_else(|| CtxError::Config(format!("unknown reasoning effort `{effort}` (a model's effort value such as low, high or max, or ultra)")))?);
    }
    config.validate()?;
    crate::fallback_index::start(&config);
    Ok(config)
}

/// Choices remembered for one workspace, kept beside its index under the
/// data directory (never in the repository itself).
#[derive(Debug, Default, Serialize, Deserialize)]
struct WorkspacePreferences {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model: Option<RememberedModel>,
    /// Permission profile (approval mode) as configured, e.g. `workspace_autonomous`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    approval: Option<String>,
    /// Reasoning effort picked with `/effort`: a level, or `auto` for none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    effort: Option<String>,
}

/// A model picked with `/model`, tied to the endpoint it came from.
#[derive(Debug, Serialize, Deserialize)]
struct RememberedModel {
    name: String,
    base_url: String,
}

fn preferences_path(config: &Config, root: &Path) -> PathBuf {
    config.layout().repository_dir(&RepositoryId::from_root(root)).join("preferences.json")
}

fn read_preferences(config: &Config, root: &Path) -> WorkspacePreferences {
    std::fs::read_to_string(preferences_path(config, root)).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

/// The model last picked in this workspace, if it was picked on the
/// endpoint that is configured now.
pub fn remembered_model(config: &Config, root: &Path) -> Option<String> {
    let model = read_preferences(config, root).model?;
    (model.base_url.trim_end_matches('/') == config.model.base_url.trim_end_matches('/')).then_some(model.name)
}

/// Use the workspace's remembered model unless one was named explicitly
/// (`--model` or `CTX_MODEL_NAME`).
fn apply_remembered_model(config: &mut Config, root: &Path, explicit: bool) {
    if explicit {
        return;
    }
    if let Some(name) = remembered_model(config, root) {
        config.model.name = name;
    }
}

/// Remember `config`'s model for the workspace at `root`.
pub fn remember_model(config: &Config, root: &Path) -> Result<()> {
    update_preferences(config, root, |prefs| prefs.model = Some(RememberedModel { name: config.model.name.clone(), base_url: config.model.base_url.clone() }))
}

/// The approval mode last picked in this workspace. Bypass is never
/// restored, even if the file says so: a new session must not start with
/// every prompt switched off.
pub fn remembered_profile(config: &Config, root: &Path) -> Option<PermissionProfile> {
    read_preferences(config, root).approval.as_deref().and_then(PermissionProfile::parse).filter(|p| *p != PermissionProfile::BypassPermissions)
}

/// Use the workspace's remembered approval mode unless one was named
/// explicitly (`--profile` or `CTX_PERMISSION_PROFILE`).
fn apply_remembered_profile(config: &mut Config, root: &Path, explicit: bool) {
    if explicit {
        return;
    }
    if let Some(profile) = remembered_profile(config, root) {
        config.permissions.profile = profile;
    }
}

/// Remember `profile` for the workspace at `root`. Returns `false` for
/// bypass, which is never remembered (the previous choice is kept).
pub fn remember_profile(config: &Config, root: &Path, profile: PermissionProfile) -> Result<bool> {
    if profile == PermissionProfile::BypassPermissions {
        return Ok(false);
    }
    update_preferences(config, root, |prefs| prefs.approval = Some(profile.as_str().to_string()))?;
    Ok(true)
}

/// The reasoning effort last picked in this workspace: `Some(None)` when
/// `/effort auto` was chosen, `None` when nothing was ever picked.
pub fn remembered_effort(config: &Config, root: &Path) -> Option<Option<ReasoningEffort>> {
    let stored = read_preferences(config, root).effort?;
    if stored == "auto" {
        return Some(None);
    }
    ReasoningEffort::parse(&stored).map(Some)
}

/// Use the workspace's remembered effort unless one was named explicitly
/// (`--effort`).
fn apply_remembered_effort(config: &mut Config, root: &Path, explicit: bool) {
    if explicit {
        return;
    }
    if let Some(effort) = remembered_effort(config, root) {
        config.model.reasoning_effort = effort;
    }
}

/// Remember `effort` (`None` = auto) for the workspace at `root`.
pub fn remember_effort(config: &Config, root: &Path, effort: Option<ReasoningEffort>) -> Result<()> {
    update_preferences(config, root, |prefs| prefs.effort = Some(effort.map(|e| e.as_str().to_string()).unwrap_or_else(|| "auto".into())))
}

fn update_preferences(config: &Config, root: &Path, change: impl FnOnce(&mut WorkspacePreferences)) -> Result<()> {
    let mut prefs = read_preferences(config, root);
    change(&mut prefs);
    let path = preferences_path(config, root);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(&prefs)?)?;
    Ok(())
}

pub fn init_logging(cli: &Cli, config: &Config, to_file: bool) {
    let level = match cli.verbose {
        0 => config.telemetry.log_level.clone(),
        1 => "info".into(),
        2 => "debug".into(),
        _ => "trace".into(),
    };
    let target = if to_file { TracingTarget::File(config.layout().logs_dir().join("ctx.log")) } else if cli.verbose > 0 { TracingTarget::Stderr } else { TracingTarget::File(config.layout().logs_dir().join("ctx.log")) };
    init_tracing(&level, target);
}

pub fn open(cli: &Cli, log_to_file: bool) -> Result<App> {
    let config = load_config(cli)?;
    init_logging(cli, &config, log_to_file);
    let root = repository_root(cli);
    let ctx = RepositoryContext::open_exact(&root, config.clone())?;
    Ok(App { config, ctx })
}

/// Build the configured provider, applying a cached capability profile or
/// probing the endpoint (spec §5.4).
pub async fn provider(config: &Config, probe_if_missing: bool, quiet: bool) -> Result<Arc<dyn ModelProvider>> {
    let provider = build_provider(config)?;
    match load_profile(config)? {
        Some(profile) => {
            provider.apply_profile(&profile);
            if !profile.is_agent_capable() && !quiet {
                eprintln!("warning: cached capability profile reports issues: {}", profile.blocking_issues().join("; "));
            }
        }
        None if probe_if_missing => {
            if !quiet {
                eprintln!("probing provider capabilities for {} at {} …", config.model.name, config.model.base_url);
            }
            let cancel = ctx_core::CancellationToken::new();
            let probe = tokio::time::timeout(Duration::from_secs(120), provider.probe(&cancel)).await;
            match probe {
                Ok(Ok(profile)) => {
                    let path = save_profile(config, &profile)?;
                    if !quiet {
                        eprintln!("capability profile saved to {}", path.display());
                    }
                    if !profile.is_agent_capable() {
                        return Err(CtxError::Provider(ctx_core::ProviderError::Unsupported(format!(
                            "the endpoint does not satisfy the required capabilities: {}. Run `ctx doctor --probe` for details.",
                            profile.blocking_issues().join("; ")
                        ))));
                    }
                }
                Ok(Err(e)) => return Err(e),
                Err(_) => return Err(CtxError::Timeout(Duration::from_secs(120))),
            }
        }
        None => {}
    }
    Ok(provider)
}

/// The process's MCP servers, connecting in the background; stdio servers
/// without a `cwd` of their own run in the repository.
pub fn mcp_manager(config: &Config, root: &Path) -> Arc<ctx_tools::McpManager> {
    ctx_tools::McpManager::new(&config.mcp, root.to_path_buf(), Some(config.layout()))
}

pub fn exit_code_for(status: ctx_core::TaskStatus) -> i32 {
    match status {
        ctx_core::TaskStatus::Completed => 0,
        ctx_core::TaskStatus::Failed => 1,
        ctx_core::TaskStatus::Blocked => 10,
        ctx_core::TaskStatus::Cancelled => 130,
    }
}

pub fn format_usage(usage: &ctx_core::TokenUsage, cost: Option<f64>) -> String {
    format!(
        "{} prompt + {} completion tokens{}{} ({})",
        usage.prompt_tokens,
        usage.completion_tokens,
        usage.cached_prompt_tokens.filter(|c| *c > 0).map(|c| format!(", {c} cached")).unwrap_or_default(),
        cost.map(|c| format!(", ${c:.4}")).unwrap_or_default(),
        match usage.source {
            ctx_core::UsageSource::Reported => "provider-reported",
            ctx_core::UsageSource::Estimated => "estimated",
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(data: &Path, base_url: &str, model: &str) -> Config {
        let mut c = Config::default();
        c.data_dir = data.to_path_buf();
        c.model.base_url = base_url.into();
        c.model.name = model.into();
        c
    }

    #[test]
    fn models_are_remembered_per_workspace_and_endpoint() {
        let data = tempfile::tempdir().unwrap();
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let picked = config(data.path(), "https://gw.example/v1", "kimi-for-coding");
        remember_model(&picked, a.path()).unwrap();

        let mut fresh = config(data.path(), "https://gw.example/v1/", "gpt-4.1");
        apply_remembered_model(&mut fresh, a.path(), false);
        assert_eq!(fresh.model.name, "kimi-for-coding", "same workspace, same endpoint");

        let mut other = config(data.path(), "https://gw.example/v1", "gpt-4.1");
        apply_remembered_model(&mut other, b.path(), false);
        assert_eq!(other.model.name, "gpt-4.1", "another workspace keeps the default");

        let mut moved = config(data.path(), "https://api.openai.com/v1", "gpt-4.1");
        apply_remembered_model(&mut moved, a.path(), false);
        assert_eq!(moved.model.name, "gpt-4.1", "a different endpoint ignores the remembered model");

        let mut flagged = config(data.path(), "https://gw.example/v1", "o3");
        apply_remembered_model(&mut flagged, a.path(), true);
        assert_eq!(flagged.model.name, "o3", "an explicit --model wins");

        remember_model(&config(data.path(), "https://gw.example/v1", "o3"), a.path()).unwrap();
        assert_eq!(remembered_model(&fresh, a.path()).as_deref(), Some("o3"), "the latest pick replaces the old one");
    }

    #[test]
    fn approval_modes_are_remembered_except_bypass() {
        let data = tempfile::tempdir().unwrap();
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let c = config(data.path(), "https://gw.example/v1", "kimi");
        remember_model(&c, a.path()).unwrap();
        assert!(remember_profile(&c, a.path(), PermissionProfile::WorkspaceAutonomous).unwrap());
        assert_eq!(remembered_model(&c, a.path()).as_deref(), Some("kimi"), "saving the mode keeps the model");

        let mut fresh = config(data.path(), "https://gw.example/v1", "kimi");
        apply_remembered_profile(&mut fresh, a.path(), false);
        assert_eq!(fresh.permissions.profile, PermissionProfile::WorkspaceAutonomous);

        let mut other = config(data.path(), "https://gw.example/v1", "kimi");
        apply_remembered_profile(&mut other, b.path(), false);
        assert_eq!(other.permissions.profile, PermissionProfile::ApprovalRequired, "other workspaces keep the default");

        let mut flagged = config(data.path(), "https://gw.example/v1", "kimi");
        flagged.permissions.profile = PermissionProfile::BypassPermissions;
        apply_remembered_profile(&mut flagged, a.path(), true);
        assert_eq!(flagged.permissions.profile, PermissionProfile::BypassPermissions, "an explicit --profile wins");

        assert!(!remember_profile(&c, a.path(), PermissionProfile::BypassPermissions).unwrap());
        assert_eq!(remembered_profile(&c, a.path()), Some(PermissionProfile::WorkspaceAutonomous), "bypass leaves the previous mode");

        std::fs::write(preferences_path(&c, a.path()), r#"{"approval": "bypass_permissions"}"#).unwrap();
        assert_eq!(remembered_profile(&c, a.path()), None, "a stored bypass is never restored");
        std::fs::write(preferences_path(&c, a.path()), r#"{"approval": "read_only"}"#).unwrap();
        assert_eq!(remembered_profile(&c, a.path()), None, "a mode from before read-only was removed is ignored");
    }

    #[test]
    fn reasoning_effort_is_remembered_per_workspace() {
        let data = tempfile::tempdir().unwrap();
        let a = tempfile::tempdir().unwrap();
        let c = config(data.path(), "https://gw.example/v1", "kimi");
        assert_eq!(remembered_effort(&c, a.path()), None, "nothing picked yet");
        remember_effort(&c, a.path(), Some(ReasoningEffort::Ultra)).unwrap();
        let mut fresh = config(data.path(), "https://gw.example/v1", "kimi");
        apply_remembered_effort(&mut fresh, a.path(), false);
        assert_eq!(fresh.model.reasoning_effort, Some(ReasoningEffort::Ultra));
        remember_effort(&c, a.path(), None).unwrap();
        let mut auto = config(data.path(), "https://gw.example/v1", "kimi");
        auto.model.reasoning_effort = Some(ReasoningEffort::level("high"));
        apply_remembered_effort(&mut auto, a.path(), false);
        assert_eq!(auto.model.reasoning_effort, None, "`auto` clears a configured level");
        let mut flagged = config(data.path(), "https://gw.example/v1", "kimi");
        flagged.model.reasoning_effort = Some(ReasoningEffort::level("low"));
        apply_remembered_effort(&mut flagged, a.path(), true);
        assert_eq!(flagged.model.reasoning_effort, Some(ReasoningEffort::level("low")), "an explicit --effort wins");
    }
}
