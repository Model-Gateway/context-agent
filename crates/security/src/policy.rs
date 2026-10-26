//! Policy engine combining permission profiles with path and command policy.

use crate::commands::{CommandClass, CommandClassifier};
use crate::paths::PathPolicy;
use crate::sandbox::SandboxStatus;
use ctx_core::{McpToolPermission, PermissionProfile, PermissionsConfig, RiskLevel};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActionKind {
    ReadFile { path: String },
    ListFiles,
    Search,
    WriteFile { path: String, creates: bool },
    Patch { path: String },
    DeleteFile { path: String },
    Command { command: String },
    Validation { command: String },
    GitInspect,
    MemoryRead,
    MemoryWrite,
    /// Deploying sub-agents; each agent's own actions are evaluated on their own.
    Delegate { agents: usize },
    /// Asking the user questions in the desktop app; the answer is the user's.
    AskUser,
    /// Sending a query to the configured web search provider.
    WebSearch { query: String },
    /// Downloading a URL; `private` when its host is loopback, a private
    /// or link-local address, or a `.local`/`.localhost` name.
    WebFetch { url: String, private: bool },
    /// Driving the desktop app's embedded browser. `interactive` when the
    /// action can send data to the site (opening a URL, clicking, typing,
    /// running script); reading the page, scrolling or taking a screenshot
    /// is not. `private` as for [`ActionKind::WebFetch`], except that
    /// loopback (a dev server on this machine) counts as public: testing
    /// the user's own app is what the browser is for.
    Browser { url: String, private: bool, interactive: bool },
    /// Calling a tool of a Model Context Protocol server the user
    /// configured. `read_only` and `destructive` are the server's own
    /// annotations (`readOnlyHint`, `destructiveHint`) on the tool;
    /// `permission` is the user's override for it, which wins over both.
    Mcp {
        server: String,
        tool: String,
        read_only: bool,
        destructive: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        permission: Option<McpToolPermission>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Action {
    pub tool: String,
    pub kind: ActionKind,
    /// One-line summary for approval prompts.
    pub summary: String,
    /// Details (patch preview, full command).
    pub details: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum Decision {
    Allow,
    Deny { reason: String },
    RequireApproval { reason: String, risk: RiskLevel },
}

impl Decision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Decision::Allow)
    }
}

/// Evaluates actions against the configured permission profile.
#[derive(Debug, Clone)]
pub struct PolicyEngine {
    profile: PermissionProfile,
    config: PermissionsConfig,
    paths: Arc<PathPolicy>,
    sandbox: SandboxStatus,
    classifier: CommandClassifier,
}

impl PolicyEngine {
    pub fn new(config: PermissionsConfig, paths: Arc<PathPolicy>) -> Self {
        let sandbox = SandboxStatus::from_config(&config.sandbox);
        Self { profile: config.profile, config, paths, sandbox, classifier: CommandClassifier }
    }

    pub fn with_profile(mut self, profile: PermissionProfile) -> Self {
        self.profile = profile;
        self
    }

    pub fn profile(&self) -> PermissionProfile {
        self.profile
    }

    pub fn sandbox(&self) -> &SandboxStatus {
        &self.sandbox
    }

    pub fn paths(&self) -> &PathPolicy {
        &self.paths
    }

    pub fn classifier(&self) -> &CommandClassifier {
        &self.classifier
    }

    /// Resolve a tool's path argument. Bypass may reach outside the
    /// workspace roots; every other profile stays inside them.
    pub fn resolve_path(&self, user_path: &str) -> ctx_core::Result<PathBuf> {
        if self.profile == PermissionProfile::BypassPermissions {
            Ok(self.paths.resolve_unrestricted(user_path))
        } else {
            self.paths.resolve(user_path)
        }
    }

    /// Risk level of an action independent of the profile.
    pub fn risk_of(&self, action: &Action) -> RiskLevel {
        match &action.kind {
            ActionKind::ReadFile { .. } | ActionKind::ListFiles | ActionKind::Search | ActionKind::GitInspect | ActionKind::MemoryRead | ActionKind::Delegate { .. } | ActionKind::AskUser => {
                RiskLevel::ReadOnly
            }
            ActionKind::WriteFile { .. } | ActionKind::Patch { .. } | ActionKind::MemoryWrite => RiskLevel::Write,
            ActionKind::DeleteFile { .. } => RiskLevel::Destructive,
            ActionKind::WebSearch { .. } | ActionKind::WebFetch { .. } => RiskLevel::Network,
            ActionKind::Browser { interactive: false, .. } => RiskLevel::ReadOnly,
            ActionKind::Browser { interactive: true, .. } => RiskLevel::Network,
            ActionKind::Mcp { read_only: true, .. } => RiskLevel::ReadOnly,
            ActionKind::Mcp { destructive: true, .. } => RiskLevel::Destructive,
            ActionKind::Mcp { .. } => RiskLevel::Execute,
            ActionKind::Command { command } | ActionKind::Validation { command } => self.classifier.classify(command).class.risk(),
        }
    }

    pub fn evaluate(&self, action: &Action) -> Decision {
        match self.evaluate_rules(action) {
            // Bypass turns every prompt into an allow; denials still stand.
            Decision::RequireApproval { .. } if self.profile == PermissionProfile::BypassPermissions => Decision::Allow,
            decision => decision,
        }
    }

    fn evaluate_rules(&self, action: &Action) -> Decision {
        // Credential checks apply regardless of profile; bypass lifts only
        // the workspace-root check.
        if let Some(path) = match &action.kind {
            ActionKind::ReadFile { path } | ActionKind::WriteFile { path, .. } | ActionKind::Patch { path } | ActionKind::DeleteFile { path } => Some(path),
            _ => None,
        } {
            if let Err(e) = self.resolve_path(path) {
                return Decision::Deny { reason: e.to_string() };
            }
            if self.paths.is_credential_path(path) {
                return Decision::Deny { reason: format!("{path} matches a protected credential pattern") };
            }
        }

        match &action.kind {
            ActionKind::ReadFile { .. } | ActionKind::ListFiles | ActionKind::Search | ActionKind::GitInspect | ActionKind::MemoryRead | ActionKind::Delegate { .. } | ActionKind::AskUser => {
                Decision::Allow
            }
            ActionKind::MemoryWrite => Decision::Allow,
            ActionKind::WriteFile { .. } | ActionKind::Patch { .. } => match self.profile {
                PermissionProfile::ApprovalRequired => {
                    Decision::RequireApproval { reason: "edits require approval under this profile".into(), risk: RiskLevel::Write }
                }
                PermissionProfile::WorkspaceAutonomous | PermissionProfile::BypassPermissions => Decision::Allow,
            },
            // Bypass turns this into an allow in `evaluate`.
            ActionKind::DeleteFile { .. } => Decision::RequireApproval { reason: "file deletion is destructive".into(), risk: RiskLevel::Destructive },
            // The query leaves the machine, so it is shown first unless the
            // profile lets the agent work on its own.
            ActionKind::WebSearch { .. } => match self.profile {
                PermissionProfile::ApprovalRequired if !self.config.allow_network_commands => {
                    Decision::RequireApproval { reason: "web searches send the query to an external search provider".into(), risk: RiskLevel::Network }
                }
                _ => Decision::Allow,
            },
            // Local services (dev servers, admin consoles, cloud metadata)
            // are reachable only with the user's say-so, under every
            // profile but bypass.
            ActionKind::WebFetch { private: true, .. } => {
                Decision::RequireApproval { reason: "the URL points at this machine or a private network".into(), risk: RiskLevel::Network }
            }
            ActionKind::WebFetch { private: false, .. } => match self.profile {
                PermissionProfile::ApprovalRequired if !self.config.allow_network_commands => {
                    Decision::RequireApproval { reason: "fetching a web page contacts an external site".into(), risk: RiskLevel::Network }
                }
                _ => Decision::Allow,
            },
            // Looking at the page the browser already shows stays local.
            ActionKind::Browser { interactive: false, .. } => Decision::Allow,
            ActionKind::Browser { private: true, .. } => {
                Decision::RequireApproval { reason: "the page is on a private network".into(), risk: RiskLevel::Network }
            }
            ActionKind::Browser { private: false, .. } => match self.profile {
                PermissionProfile::ApprovalRequired if !self.config.allow_network_commands => {
                    Decision::RequireApproval { reason: "driving the browser sends input to an external site".into(), risk: RiskLevel::Network }
                }
                _ => Decision::Allow,
            },
            // The user configured the server, so a tool it marks read-only
            // runs like a read of the repository. Anything else acts on the
            // world outside the workspace and is shown first under the ask
            // profile; a tool the server marks destructive is shown under
            // every profile but bypass. The user's own setting for the tool
            // (or its server) comes first.
            ActionKind::Mcp { permission: Some(McpToolPermission::Allow), .. } => Decision::Allow,
            ActionKind::Mcp { server, permission: Some(McpToolPermission::Ask), destructive, .. } => Decision::RequireApproval {
                reason: format!("you set this {server} tool to always ask"),
                risk: if *destructive { RiskLevel::Destructive } else { RiskLevel::Execute },
            },
            ActionKind::Mcp { read_only: true, .. } => Decision::Allow,
            ActionKind::Mcp { server, destructive: true, .. } => {
                Decision::RequireApproval { reason: format!("the {server} server marks this tool destructive"), risk: RiskLevel::Destructive }
            }
            ActionKind::Mcp { server, .. } => match self.profile {
                PermissionProfile::ApprovalRequired => {
                    Decision::RequireApproval { reason: format!("the tool acts through the {server} MCP server"), risk: RiskLevel::Execute }
                }
                PermissionProfile::WorkspaceAutonomous | PermissionProfile::BypassPermissions => Decision::Allow,
            },
            ActionKind::Command { command } | ActionKind::Validation { command } => self.evaluate_command(command, matches!(action.kind, ActionKind::Validation { .. })),
        }
    }

    fn evaluate_command(&self, command: &str, is_validation: bool) -> Decision {
        if CommandClassifier::matches_prefix(command, &self.config.denied_commands) {
            return Decision::Deny { reason: "command matches a denied prefix".into() };
        }
        let analysis = self.classifier.classify(command);
        let risk = analysis.class.risk();
        let reasons = analysis.reasons.join("; ");

        if CommandClassifier::matches_prefix(command, &self.config.allowed_commands) && analysis.class != CommandClass::Destructive {
            return Decision::Allow;
        }

        match self.profile {
            PermissionProfile::ApprovalRequired => match analysis.class {
                CommandClass::ReadOnly => Decision::Allow,
                _ => Decision::RequireApproval { reason: if reasons.is_empty() { "commands require approval under this profile".into() } else { reasons }, risk },
            },
            PermissionProfile::WorkspaceAutonomous | PermissionProfile::BypassPermissions => match analysis.class {
                CommandClass::ReadOnly | CommandClass::BuildTest | CommandClass::Write => Decision::Allow,
                CommandClass::Network => {
                    if self.config.allow_network_commands {
                        Decision::Allow
                    } else {
                        Decision::RequireApproval { reason: format!("network access: {reasons}"), risk }
                    }
                }
                CommandClass::Unknown => {
                    // Detected project validation commands (e.g. `ruby -e …`,
                    // `vendor/bin/phpunit`) are allowed; the tool layer only
                    // labels an action as validation when the command came
                    // from configuration or build-file detection.
                    if is_validation {
                        Decision::Allow
                    } else {
                        Decision::RequireApproval { reason: format!("unrecognised command: {reasons}"), risk }
                    }
                }
                CommandClass::Destructive => Decision::RequireApproval { reason: format!("destructive: {reasons}"), risk },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine(profile: PermissionProfile) -> (PolicyEngine, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let cfg = PermissionsConfig { profile, allowed_commands: vec!["cargo test".into()], denied_commands: vec!["rm -rf /".into()], ..Default::default() };
        let paths = PathPolicy::new(vec![dir.path().to_path_buf()], &ctx_core::IndexConfig::default().credential_patterns).unwrap();
        (PolicyEngine::new(cfg, Arc::new(paths)), dir)
    }

    fn cmd(c: &str) -> Action {
        Action { tool: "execute_command".into(), kind: ActionKind::Command { command: c.into() }, summary: c.into(), details: c.into() }
    }

    fn write(p: &str) -> Action {
        Action { tool: "write_file".into(), kind: ActionKind::WriteFile { path: p.into(), creates: true }, summary: p.into(), details: String::new() }
    }

    #[test]
    fn approval_profile_asks_for_edits_and_commands() {
        let (e, _d) = engine(PermissionProfile::ApprovalRequired);
        assert!(matches!(e.evaluate(&write("src/a.rs")), Decision::RequireApproval { .. }));
        assert!(matches!(e.evaluate(&cmd("npm test")), Decision::RequireApproval { .. }));
        assert!(matches!(e.evaluate(&cmd("cargo test --all")), Decision::Allow));
        assert!(matches!(e.evaluate(&cmd("git status")), Decision::Allow));
    }

    #[test]
    fn workspace_profile_allows_validation_but_not_network() {
        let (e, _d) = engine(PermissionProfile::WorkspaceAutonomous);
        assert!(matches!(e.evaluate(&write("src/a.rs")), Decision::Allow));
        assert!(matches!(e.evaluate(&cmd("npm test")), Decision::Allow));
        assert!(matches!(e.evaluate(&cmd("curl http://x")), Decision::RequireApproval { risk: RiskLevel::Network, .. }));
        assert!(matches!(e.evaluate(&cmd("rm -rf build")), Decision::RequireApproval { risk: RiskLevel::Destructive, .. }));
        assert!(matches!(e.evaluate(&cmd("rm -rf /")), Decision::Deny { .. }));
    }

    #[test]
    fn bypass_never_prompts_but_keeps_hard_denials() {
        let (e, _d) = engine(PermissionProfile::BypassPermissions);
        assert!(matches!(e.evaluate(&write("src/a.rs")), Decision::Allow));
        assert!(matches!(e.evaluate(&cmd("curl http://x")), Decision::Allow));
        assert!(matches!(e.evaluate(&cmd("rm -rf build")), Decision::Allow));
        assert!(matches!(e.evaluate(&cmd("some-unknown-tool --flag")), Decision::Allow));
        let delete = Action { tool: "delete".into(), kind: ActionKind::DeleteFile { path: "src/a.rs".into() }, summary: String::new(), details: String::new() };
        assert!(matches!(e.evaluate(&delete), Decision::Allow));
        assert!(matches!(e.evaluate(&cmd("rm -rf /")), Decision::Deny { .. }), "denied prefixes still apply");
        assert!(matches!(e.evaluate(&write(".env")), Decision::Deny { .. }), "credential files stay protected");
        let outside = tempfile::tempdir().unwrap();
        let outside_file = outside.path().join("notes.txt").to_string_lossy().to_string();
        let read = Action { tool: "read_file".into(), kind: ActionKind::ReadFile { path: outside_file.clone() }, summary: String::new(), details: String::new() };
        assert!(matches!(e.evaluate(&read), Decision::Allow), "bypass may reach outside the workspace");
        assert!(matches!(e.evaluate(&write(&outside_file)), Decision::Allow));
        assert!(e.resolve_path(&outside_file).is_ok());
        let outside_env = outside.path().join(".env").to_string_lossy().to_string();
        assert!(matches!(e.evaluate(&write(&outside_env)), Decision::Deny { .. }), "credential files outside stay protected");
    }

    #[test]
    fn web_search_asks_only_under_the_approval_profile() {
        let search = Action { tool: "web_search".into(), kind: ActionKind::WebSearch { query: "tokio select".into() }, summary: String::new(), details: String::new() };
        let (e, _d) = engine(PermissionProfile::ApprovalRequired);
        assert!(matches!(e.evaluate(&search), Decision::RequireApproval { risk: RiskLevel::Network, .. }));
        for profile in [PermissionProfile::WorkspaceAutonomous, PermissionProfile::BypassPermissions] {
            let (e, _d) = engine(profile);
            assert!(matches!(e.evaluate(&search), Decision::Allow), "{profile:?}");
        }
    }

    #[test]
    fn web_fetch_of_private_hosts_always_asks_except_under_bypass() {
        let fetch = |private| Action { tool: "web_fetch".into(), kind: ActionKind::WebFetch { url: "http://x".into(), private }, summary: String::new(), details: String::new() };
        let (e, _d) = engine(PermissionProfile::ApprovalRequired);
        assert!(matches!(e.evaluate(&fetch(false)), Decision::RequireApproval { .. }));
        let (e, _d) = engine(PermissionProfile::WorkspaceAutonomous);
        assert!(matches!(e.evaluate(&fetch(false)), Decision::Allow));
        assert!(matches!(e.evaluate(&fetch(true)), Decision::RequireApproval { risk: RiskLevel::Network, .. }));
        let (e, _d) = engine(PermissionProfile::BypassPermissions);
        assert!(matches!(e.evaluate(&fetch(true)), Decision::Allow));
    }

    #[test]
    fn browser_reads_are_free_and_interaction_follows_the_fetch_rules() {
        let browse = |private, interactive| Action { tool: "browser".into(), kind: ActionKind::Browser { url: "http://x".into(), private, interactive }, summary: String::new(), details: String::new() };
        for profile in PermissionProfile::ALL {
            let (e, _d) = engine(profile);
            assert!(matches!(e.evaluate(&browse(true, false)), Decision::Allow), "{profile:?}");
            assert_eq!(e.risk_of(&browse(false, false)), RiskLevel::ReadOnly);
        }
        let (e, _d) = engine(PermissionProfile::ApprovalRequired);
        assert!(matches!(e.evaluate(&browse(false, true)), Decision::RequireApproval { risk: RiskLevel::Network, .. }));
        let (e, _d) = engine(PermissionProfile::WorkspaceAutonomous);
        assert!(matches!(e.evaluate(&browse(false, true)), Decision::Allow));
        assert!(matches!(e.evaluate(&browse(true, true)), Decision::RequireApproval { .. }));
        let (e, _d) = engine(PermissionProfile::BypassPermissions);
        assert!(matches!(e.evaluate(&browse(true, true)), Decision::Allow));
    }

    #[test]
    fn credential_paths_are_denied_everywhere_and_escapes_outside_bypass() {
        for profile in PermissionProfile::ALL {
            let (e, _d) = engine(profile);
            assert!(matches!(e.evaluate(&write(".env")), Decision::Deny { .. }), "{profile:?}");
            if profile != PermissionProfile::BypassPermissions {
                assert!(matches!(e.evaluate(&write("../../etc/passwd")), Decision::Deny { .. }), "{profile:?}");
            }
        }
    }

    #[test]
    fn the_developer_wrapper_never_changes_what_is_allowed() {
        let dir = tempfile::tempdir().unwrap();
        let sandbox = ctx_core::SandboxConfig { kind: ctx_core::SandboxKind::Wrapper, wrapper: vec!["env".into()] };
        let cfg = PermissionsConfig { profile: PermissionProfile::WorkspaceAutonomous, sandbox, ..Default::default() };
        let paths = PathPolicy::new(vec![dir.path().to_path_buf()], &ctx_core::IndexConfig::default().credential_patterns).unwrap();
        let e = PolicyEngine::new(cfg, Arc::new(paths));
        assert!(e.sandbox().enforced, "the wrapper is reported honestly");
        let wrapped = e.sandbox().wrap_command("sh", "ls");
        assert_eq!(wrapped.program, "env");
        assert_eq!(wrapped.args, vec!["sh".to_string(), "-c".to_string(), "ls".to_string()]);
        assert!(matches!(e.evaluate(&cmd("git push")), Decision::RequireApproval { risk: RiskLevel::Network, .. }), "wrapping is not a licence");
    }

    #[test]
    fn mcp_tools_follow_their_annotations() {
        let mcp = |read_only, destructive| Action { tool: "mcp__gh__x".into(), kind: ActionKind::Mcp { server: "gh".into(), tool: "x".into(), read_only, destructive, permission: None }, summary: String::new(), details: String::new() };
        let (ask, _a) = engine(PermissionProfile::ApprovalRequired);
        let (auto, _b) = engine(PermissionProfile::WorkspaceAutonomous);
        let (bypass, _c) = engine(PermissionProfile::BypassPermissions);
        assert!(ask.evaluate(&mcp(true, false)).is_allowed(), "read-only tools run under every profile");
        assert!(matches!(ask.evaluate(&mcp(false, false)), Decision::RequireApproval { risk: RiskLevel::Execute, .. }));
        assert!(auto.evaluate(&mcp(false, false)).is_allowed());
        assert!(matches!(auto.evaluate(&mcp(false, true)), Decision::RequireApproval { risk: RiskLevel::Destructive, .. }), "destructive tools ask under auto");
        assert!(bypass.evaluate(&mcp(false, true)).is_allowed());
        assert_eq!(ask.risk_of(&mcp(true, false)), RiskLevel::ReadOnly);
    }

    #[test]
    fn the_users_mcp_tool_setting_wins_over_annotations() {
        let mcp = |read_only, destructive, permission| Action { tool: "mcp__gh__x".into(), kind: ActionKind::Mcp { server: "gh".into(), tool: "x".into(), read_only, destructive, permission: Some(permission) }, summary: String::new(), details: String::new() };
        let (ask, _a) = engine(PermissionProfile::ApprovalRequired);
        let (auto, _b) = engine(PermissionProfile::WorkspaceAutonomous);
        let (bypass, _c) = engine(PermissionProfile::BypassPermissions);
        assert!(ask.evaluate(&mcp(false, true, McpToolPermission::Allow)).is_allowed(), "allow runs even a destructive tool");
        assert!(matches!(auto.evaluate(&mcp(true, false, McpToolPermission::Ask)), Decision::RequireApproval { risk: RiskLevel::Execute, .. }), "ask asks even for a read-only tool");
        assert!(matches!(auto.evaluate(&mcp(false, true, McpToolPermission::Ask)), Decision::RequireApproval { risk: RiskLevel::Destructive, .. }));
        assert!(bypass.evaluate(&mcp(false, false, McpToolPermission::Ask)).is_allowed(), "the bypass profile never prompts");
    }
}
