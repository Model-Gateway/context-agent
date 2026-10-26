//! Model Context Protocol support: the servers in `[mcp]` are connected
//! once per process, their tools join the agent's tool set as
//! `mcp__<server>__<tool>`, and their state is reported to the UIs.
//!
//! The client speaks JSON-RPC 2.0 over stdio (a child process) or
//! Streamable HTTP, negotiates `initialize`, lists tools (following
//! `tools/list_changed`) and calls them. Sampling, prompts and resources are
//! not used; `roots/list` answers with no roots.

pub mod catalog;
pub mod http;
pub mod mentions;
pub mod oauth;
pub mod protocol;
pub mod stdio;
pub mod tool;

pub use protocol::{McpClient, McpToolSpec, ServerNotice, Transport};
pub use catalog::{catalog, catalog_app, CatalogAccess, CatalogApp, CatalogInput, CatalogOAuth};
pub use mentions::{mention_instruction, mentioned_servers};
pub use tool::{qualified_name, split_qualified, McpTool};

use crate::registry::Tool;
use ctx_core::paths::DataLayout;
use ctx_core::{CredentialStore, CtxError, McpConfig, McpServerConfig, McpTransport, Result};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

/// Replaces every `${NAME}` with that environment variable (empty when it
/// is not set), so configuration files can refer to keys without holding them.
pub fn expand_env(text: &str) -> String {
    expand_value(text, None)
}

/// [`expand_env`], plus `${credential:NAME}`: the key stored under NAME in
/// the credential store (empty when there is none), which is how a signed-in
/// app's token reaches its server without being written to `config.toml`.
pub fn expand_value(text: &str, credentials: Option<&CredentialStore>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after.find('}');
        let inner = end.map(|e| &after[..e]);
        match (end, inner) {
            (Some(end), Some(inner)) if inner.starts_with("credential:") => {
                let name = inner["credential:".len()..].trim();
                let key = credentials.and_then(|c| c.get(name)).map(|c| c.api_key.clone()).unwrap_or_default();
                out.push_str(&key);
                rest = &after[end + 1..];
            }
            (Some(end), Some(inner)) if end > 0 && inner.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') => {
                out.push_str(&std::env::var(inner).unwrap_or_default());
                rest = &after[end + 1..];
            }
            _ => {
                out.push_str("${");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpServerState {
    /// Switched off in the configuration (or the whole `[mcp]` table is).
    Disabled,
    Connecting,
    Connected,
    Failed,
}

/// One tool of a connected server, for listings. `enabled` is false for a
/// tool the server offers but the entry's `tools` allowlist leaves out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpToolInfo {
    pub name: String,
    pub qualified_name: String,
    pub description: String,
    pub read_only: bool,
    pub destructive: bool,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// One configured server as the UIs show it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpServerStatus {
    pub name: String,
    pub enabled: bool,
    /// The catalog app this server was installed from, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    pub state: McpServerState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub transport: String,
    /// The command line or URL.
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(default)]
    pub tools: Vec<McpToolInfo>,
}

enum SlotState {
    Disabled,
    Connecting,
    /// `specs` is everything the server offers; `tools` the ones offered to
    /// the model (the entry's `tools` allowlist applied).
    Connected { client: Arc<McpClient>, tools: Vec<Arc<McpTool>>, specs: Vec<McpToolSpec> },
    Failed(String),
}

struct Slot {
    name: String,
    /// Replaced in place when only the tool permissions change.
    config: RwLock<McpServerConfig>,
    state: Mutex<SlotState>,
    /// Bumped by every (re)connect, so a notice from an earlier connection
    /// cannot touch a later one.
    serial: AtomicU64,
}

/// The process's MCP servers: connected in the background from a
/// [`McpConfig`], reconfigured when the user changes it, and queried for
/// tools at the start of every run.
pub struct McpManager {
    config: RwLock<McpConfig>,
    /// Working directory for stdio servers without a `cwd` of their own.
    default_cwd: PathBuf,
    /// Where `${credential:NAME}` values come from (`None`: they expand empty).
    credentials: Option<DataLayout>,
    slots: RwLock<BTreeMap<String, Arc<Slot>>>,
    /// Bumped whenever a server's state or tool list changes.
    generation: watch::Sender<u64>,
    active: bool,
}

impl McpManager {
    /// Starts connecting every active server. Must be called on a Tokio
    /// runtime: connections run as background tasks.
    pub fn new(config: &McpConfig, default_cwd: PathBuf, credentials: Option<DataLayout>) -> Arc<Self> {
        let (generation, _) = watch::channel(0);
        let manager = Arc::new(Self { config: RwLock::new(config.clone()), default_cwd, credentials, slots: RwLock::new(BTreeMap::new()), generation, active: true });
        manager.apply(config);
        manager
    }

    /// A manager that connects nothing (mock runs, tests).
    pub fn disabled() -> Arc<Self> {
        let (generation, _) = watch::channel(0);
        Arc::new(Self { config: RwLock::new(McpConfig::default()), default_cwd: PathBuf::new(), credentials: None, slots: RwLock::new(BTreeMap::new()), generation, active: false })
    }

    pub fn config(&self) -> McpConfig {
        self.config.read().clone()
    }

    /// Whether any server is configured and switched on.
    pub fn has_servers(&self) -> bool {
        self.active && self.config.read().active_servers().next().is_some()
    }

    /// Changes since the last generation: a receiver to await.
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.generation.subscribe()
    }

    fn bump(&self) {
        self.generation.send_modify(|g| *g += 1);
    }

    /// Replaces the configuration: servers that are gone or changed are
    /// closed, new and changed active ones connected, the rest kept.
    pub fn reconfigure(self: &Arc<Self>, config: &McpConfig) {
        if !self.active {
            return;
        }
        *self.config.write() = config.clone();
        self.apply(config);
    }

    fn apply(self: &Arc<Self>, config: &McpConfig) {
        if !self.active {
            return;
        }
        let mut to_close: Vec<Arc<Slot>> = Vec::new();
        let mut to_connect: Vec<Arc<Slot>> = Vec::new();
        {
            let mut slots = self.slots.write();
            let gone: Vec<String> = slots.keys().filter(|k| !config.servers.contains_key(*k)).cloned().collect();
            for name in gone {
                if let Some(slot) = slots.remove(&name) {
                    to_close.push(slot);
                }
            }
            for (name, server) in &config.servers {
                let active = config.enabled && server.enabled;
                // Only the tool permissions changed: rebuild the tools on the
                // live connection instead of reconnecting.
                if let Some(slot) = slots.get(name) {
                    let in_place = {
                        let current = slot.config.read();
                        *current != *server && current.differs_only_in_permissions(server)
                    };
                    if in_place && active == !matches!(*slot.state.lock(), SlotState::Disabled) {
                        *slot.config.write() = server.clone();
                        if let SlotState::Connected { client, tools, specs } = &mut *slot.state.lock() {
                            *tools = build_tools(name, server, specs.clone(), client, Duration::from_secs(config.call_timeout_secs));
                        }
                        continue;
                    }
                }
                let unchanged = slots.get(name).map(|s| *s.config.read() == *server).unwrap_or(false);
                if unchanged {
                    let slot = slots.get(name).cloned().expect("present");
                    let currently_disabled = matches!(*slot.state.lock(), SlotState::Disabled);
                    if active == !currently_disabled {
                        continue;
                    }
                }
                if let Some(old) = slots.remove(name) {
                    to_close.push(old);
                }
                let slot = Arc::new(Slot { name: name.clone(), config: RwLock::new(server.clone()), state: Mutex::new(if active { SlotState::Connecting } else { SlotState::Disabled }), serial: AtomicU64::new(0) });
                slots.insert(name.clone(), slot.clone());
                if active {
                    to_connect.push(slot);
                }
            }
        }
        for slot in to_close {
            let client = match std::mem::replace(&mut *slot.state.lock(), SlotState::Disabled) {
                SlotState::Connected { client, .. } => Some(client),
                _ => None,
            };
            slot.serial.fetch_add(1, Ordering::SeqCst);
            if let Some(client) = client {
                tokio::spawn(async move { client.close().await });
            }
        }
        for slot in to_connect {
            self.spawn_connect(slot);
        }
        self.bump();
    }

    fn spawn_connect(self: &Arc<Self>, slot: Arc<Slot>) {
        let manager = self.clone();
        tokio::spawn(async move { manager.connect(slot).await });
    }

    async fn connect(self: Arc<Self>, slot: Arc<Slot>) {
        let (startup, call_timeout) = {
            let c = self.config.read();
            (Duration::from_secs(c.startup_timeout_secs), Duration::from_secs(c.call_timeout_secs))
        };
        let serial = slot.serial.fetch_add(1, Ordering::SeqCst) + 1;
        let previous = std::mem::replace(&mut *slot.state.lock(), SlotState::Connecting);
        if let SlotState::Connected { client, .. } = previous {
            client.close().await;
        }
        self.bump();
        let notices: protocol::NoticeSink = {
            let manager = self.clone();
            let slot = slot.clone();
            Arc::new(move |notice| manager.on_notice(&slot, serial, notice))
        };
        let outcome = tokio::time::timeout(startup, async {
            // Read fresh for every connection: a sign-in may have just stored a token.
            let credentials = self.credentials.as_ref().and_then(|layout| CredentialStore::load(layout).ok());
            let config = slot.config.read().clone();
            let transport = open_transport(&config, &self.default_cwd, credentials.as_ref(), notices).await?;
            let client = McpClient::connect(transport).await?;
            let tools = client.list_tools().await?;
            Ok::<_, CtxError>((client, tools))
        })
        .await;
        if slot.serial.load(Ordering::SeqCst) != serial {
            // Reconfigured or reconnected meanwhile; that connection owns the slot.
            if let Ok(Ok((client, _))) = outcome {
                client.close().await;
            }
            return;
        }
        let state = match outcome {
            Ok(Ok((client, specs))) => {
                let client = Arc::new(client);
                let tools = build_tools(&slot.name, &slot.config.read(), specs.clone(), &client, call_timeout);
                tracing::info!(server = %slot.name, tools = tools.len(), "MCP server connected");
                SlotState::Connected { client, tools, specs }
            }
            Ok(Err(e)) => {
                tracing::warn!(server = %slot.name, error = %e, "MCP server failed");
                SlotState::Failed(e.to_string())
            }
            Err(_) => {
                tracing::warn!(server = %slot.name, "MCP server did not start in time");
                SlotState::Failed(format!("did not initialize within {}s (mcp.startup_timeout_secs)", startup.as_secs()))
            }
        };
        *slot.state.lock() = state;
        self.bump();
    }

    fn on_notice(self: &Arc<Self>, slot: &Arc<Slot>, serial: u64, notice: ServerNotice) {
        if slot.serial.load(Ordering::SeqCst) != serial {
            return;
        }
        match notice {
            ServerNotice::ToolsChanged => {
                let manager = self.clone();
                let slot = slot.clone();
                tokio::spawn(async move { manager.refresh_tools(slot, serial).await });
            }
            ServerNotice::Log { level, message } => {
                tracing::debug!(server = %slot.name, %level, "mcp: {message}");
            }
            ServerNotice::Closed(reason) => {
                let mut state = slot.state.lock();
                if matches!(*state, SlotState::Connected { .. } | SlotState::Connecting) {
                    *state = SlotState::Failed(reason);
                    drop(state);
                    self.bump();
                }
            }
        }
    }

    async fn refresh_tools(self: Arc<Self>, slot: Arc<Slot>, serial: u64) {
        let client = match &*slot.state.lock() {
            SlotState::Connected { client, .. } => client.clone(),
            _ => return,
        };
        let call_timeout = Duration::from_secs(self.config.read().call_timeout_secs);
        match client.list_tools().await {
            Ok(specs) => {
                if slot.serial.load(Ordering::SeqCst) != serial {
                    return;
                }
                let tools = build_tools(&slot.name, &slot.config.read(), specs.clone(), &client, call_timeout);
                let mut state = slot.state.lock();
                if let SlotState::Connected { tools: current, specs: current_specs, .. } = &mut *state {
                    *current = tools;
                    *current_specs = specs;
                }
                drop(state);
                self.bump();
            }
            Err(e) => tracing::warn!(server = %slot.name, error = %e, "could not refresh the MCP tool list"),
        }
    }

    fn any_connecting(&self) -> bool {
        self.slots.read().values().any(|s| matches!(*s.state.lock(), SlotState::Connecting))
    }

    /// Waits until no server is still connecting (bounded by the startup
    /// timeout, which every connection attempt honours itself).
    pub async fn ready(&self) {
        if !self.active {
            return;
        }
        let bound = Duration::from_secs(self.config.read().startup_timeout_secs) + Duration::from_secs(5);
        let mut rx = self.generation.subscribe();
        let _ = tokio::time::timeout(bound, async {
            loop {
                if !self.any_connecting() {
                    return;
                }
                if rx.changed().await.is_err() {
                    return;
                }
            }
        })
        .await;
    }

    /// Reconnects one server (`Some(name)`) or every active one, and waits
    /// for the result.
    pub async fn reconnect(self: &Arc<Self>, name: Option<&str>) -> Result<()> {
        if !self.active {
            return Ok(());
        }
        let slots: Vec<Arc<Slot>> = {
            let slots = self.slots.read();
            match name {
                Some(n) => vec![slots.get(n).cloned().ok_or_else(|| CtxError::Config(format!("no MCP server named `{n}` is configured")))?],
                None => slots.values().cloned().collect(),
            }
        };
        for slot in slots {
            if matches!(*slot.state.lock(), SlotState::Disabled) {
                continue;
            }
            self.spawn_connect(slot);
        }
        self.ready().await;
        Ok(())
    }

    /// Closes every connection.
    pub async fn shutdown(&self) {
        let slots: Vec<Arc<Slot>> = self.slots.read().values().cloned().collect();
        for slot in slots {
            slot.serial.fetch_add(1, Ordering::SeqCst);
            let client = match std::mem::replace(&mut *slot.state.lock(), SlotState::Disabled) {
                SlotState::Connected { client, .. } => Some(client),
                _ => None,
            };
            if let Some(client) = client {
                client.close().await;
            }
        }
    }

    /// The tools of every connected server. When two servers' names
    /// collide after sanitizing, the later one's tools are dropped with a
    /// warning rather than silently replacing the first.
    pub fn tools(&self) -> Vec<Arc<dyn Tool>> {
        self.collect(|_, tools| tools.iter().map(|t| t.clone() as Arc<dyn Tool>).collect())
    }

    /// The read-only tools only (for sub-agents that must not change anything).
    pub fn read_only_tools(&self) -> Vec<Arc<dyn Tool>> {
        self.collect(|_, tools| tools.iter().filter(|t| t.spec().read_only()).map(|t| t.clone() as Arc<dyn Tool>).collect())
    }

    fn collect(&self, pick: impl Fn(&str, &[Arc<McpTool>]) -> Vec<Arc<dyn Tool>>) -> Vec<Arc<dyn Tool>> {
        let mut out: Vec<Arc<dyn Tool>> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for slot in self.slots.read().values() {
            let state = slot.state.lock();
            if let SlotState::Connected { tools, .. } = &*state {
                for tool in pick(&slot.name, tools) {
                    if seen.insert(tool.name().to_string()) {
                        out.push(tool);
                    } else {
                        tracing::warn!(tool = tool.name(), server = %slot.name, "MCP tool name collides with another server's tool; skipped");
                    }
                }
            }
        }
        out
    }

    /// Every configured server's state, by name.
    pub fn status(&self) -> Vec<McpServerStatus> {
        self.slots
            .read()
            .values()
            .map(|slot| {
                let state = slot.state.lock();
                let config = slot.config.read();
                let mut status = McpServerStatus {
                    name: slot.name.clone(),
                    enabled: config.enabled,
                    app: config.app.clone(),
                    state: match &*state {
                        SlotState::Disabled => McpServerState::Disabled,
                        SlotState::Connecting => McpServerState::Connecting,
                        SlotState::Connected { .. } => McpServerState::Connected,
                        SlotState::Failed(_) => McpServerState::Failed,
                    },
                    error: None,
                    transport: config.transport.as_str().to_string(),
                    target: config.target(),
                    server_name: None,
                    server_version: None,
                    protocol_version: None,
                    instructions: None,
                    tools: Vec::new(),
                };
                match &*state {
                    SlotState::Connected { client, tools, specs } => {
                        status.server_name = Some(client.server.name.clone()).filter(|n| !n.is_empty());
                        status.server_version = Some(client.server.version.clone()).filter(|v| !v.is_empty());
                        status.protocol_version = Some(client.protocol_version.clone());
                        status.instructions = client.instructions.clone();
                        status.tools = specs
                            .iter()
                            .map(|spec| McpToolInfo {
                                name: spec.name.clone(),
                                qualified_name: qualified_name(&slot.name, &spec.name),
                                description: spec.description.clone().unwrap_or_default(),
                                read_only: spec.read_only(),
                                destructive: spec.destructive(),
                                enabled: tools.iter().any(|t| t.spec().name == spec.name),
                            })
                            .collect();
                    }
                    SlotState::Failed(e) => status.error = Some(e.clone()),
                    _ => {}
                }
                status
            })
            .collect()
    }
}

fn build_tools(server: &str, config: &McpServerConfig, specs: Vec<McpToolSpec>, client: &Arc<McpClient>, call_timeout: Duration) -> Vec<Arc<McpTool>> {
    let mut tools: Vec<Arc<McpTool>> = Vec::new();
    for spec in specs {
        if !config.tool_allowed(&spec.name) {
            continue;
        }
        let permission = config.tool_permission(&spec.name);
        let tool = McpTool::new(server, spec, client.clone(), call_timeout).with_permission(permission);
        if tools.iter().any(|t| t.name() == tool.name()) {
            tracing::warn!(server, tool = tool.name(), "two tools of one MCP server share a sanitized name; the second is skipped");
            continue;
        }
        tools.push(Arc::new(tool));
    }
    tools
}

async fn open_transport(config: &McpServerConfig, default_cwd: &std::path::Path, credentials: Option<&CredentialStore>, notices: protocol::NoticeSink) -> Result<Box<dyn Transport>> {
    // A credential the entry refers to but nobody signed in for: say so
    // instead of starting a server that fails on an empty token.
    for name in oauth::referenced_credentials(config) {
        if credentials.and_then(|c| c.get(&name)).map(|c| c.api_key.trim().is_empty()).unwrap_or(true) {
            return Err(CtxError::tool(format!("not signed in: no stored `{name}` credential (sign in again from the app's page in Apps, or run `ctx mcp login`)")));
        }
    }
    let expand = |text: &str| expand_value(text, credentials);
    match config.transport {
        McpTransport::Stdio => {
            let command = expand(config.command.as_deref().unwrap_or_default());
            let args: Vec<String> = config.args.iter().map(|a| expand(a)).collect();
            let env: BTreeMap<String, String> = config.env.iter().map(|(k, v)| (k.clone(), expand(v))).collect();
            let cwd = config.cwd.as_deref().map(|c| expand(c)).map(PathBuf::from).unwrap_or_else(|| default_cwd.to_path_buf());
            Ok(Box::new(stdio::StdioTransport::spawn(&command, &args, &env, &cwd, notices).await?))
        }
        McpTransport::Http => {
            let url = expand(config.url.as_deref().unwrap_or_default());
            let headers: Vec<(String, String)> = config.headers.iter().map(|(k, v)| (k.clone(), expand(v))).collect();
            Ok(Box::new(http::HttpTransport::new(&url, headers, notices)?))
        }
        McpTransport::Builtin => {
            // The running `ctx` binary serves it: `ctx mcp-serve <name>`.
            let exe = std::env::current_exe().map_err(|e| CtxError::tool(format!("cannot locate the ctx binary: {e}")))?;
            let name = config.builtin.clone().unwrap_or_default();
            let args = vec!["mcp-serve".to_string(), name];
            Ok(Box::new(stdio::StdioTransport::spawn(&exe.display().to_string(), &args, &BTreeMap::new(), default_cwd, notices).await?))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_placeholders_read_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let layout = DataLayout::new(dir.path().to_path_buf());
        let mut store = CredentialStore::load(&layout).unwrap();
        store.set("github", ctx_core::StoredCredential { api_key: "gho_secret".into(), ..Default::default() });
        assert_eq!(expand_value("token ${credential:github}", Some(&store)), "token gho_secret");
        assert_eq!(expand_value("${credential:missing}", Some(&store)), "");
        assert_eq!(expand_value("${credential:github}", None), "");
    }

    #[test]
    fn env_placeholders_are_expanded() {
        std::env::set_var("CTX_MCP_TEST_TOKEN", "secret");
        assert_eq!(expand_env("Bearer ${CTX_MCP_TEST_TOKEN}"), "Bearer secret");
        assert_eq!(expand_env("${CTX_MCP_TEST_UNSET_VAR}x"), "x");
        assert_eq!(expand_env("${not valid}"), "${not valid}");
        assert_eq!(expand_env("plain $HOME"), "plain $HOME");
        std::env::remove_var("CTX_MCP_TEST_TOKEN");
    }

    #[tokio::test]
    async fn disabled_servers_are_listed_but_never_started() {
        let mut config = McpConfig::default();
        config.servers.insert("off".into(), McpServerConfig { enabled: false, ..McpServerConfig::stdio("definitely-not-a-program", vec![]) });
        let manager = McpManager::new(&config, std::env::temp_dir(), None);
        manager.ready().await;
        let status = manager.status();
        assert_eq!(status.len(), 1);
        assert_eq!(status[0].state, McpServerState::Disabled);
        assert!(manager.tools().is_empty());
        assert!(!manager.has_servers());
    }

    #[tokio::test]
    async fn a_missing_program_fails_fast_and_reports_why() {
        let mut config = McpConfig::default();
        config.startup_timeout_secs = 5;
        config.servers.insert("ghost".into(), McpServerConfig::stdio("ctx-no-such-mcp-server-binary", vec![]));
        let manager = McpManager::new(&config, std::env::temp_dir(), None);
        manager.ready().await;
        let status = manager.status();
        assert_eq!(status[0].state, McpServerState::Failed);
        assert!(status[0].error.as_deref().unwrap_or("").contains("could not start"), "{:?}", status[0].error);
        assert!(manager.has_servers());
    }

    // The fake server is a `#!/bin/sh` script, which Windows cannot execute.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_scripted_stdio_server_lists_and_calls_tools() {
        // A shell script that speaks just enough MCP: initialize, tools/list
        // with one echo tool, tools/call echoing its arguments.
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("server.sh");
        std::fs::write(
            &script,
            r##"#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*) id=$(printf '%s' "$line" | sed -E 's/.*"id":([0-9]+).*/\1/'); printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"echo","version":"1.0"}}}\n' "$id" ;;
    *'"method":"tools/list"'*) id=$(printf '%s' "$line" | sed -E 's/.*"id":([0-9]+).*/\1/'); printf '{"jsonrpc":"2.0","id":%s,"result":{"tools":[{"name":"echo","description":"Echo","inputSchema":{"type":"object","properties":{"text":{"type":"string"}}},"annotations":{"readOnlyHint":true}}]}}\n' "$id" ;;
    *'"method":"tools/call"'*) id=$(printf '%s' "$line" | sed -E 's/.*"id":([0-9]+).*/\1/'); printf '{"jsonrpc":"2.0","id":%s,"result":{"content":[{"type":"text","text":"echoed"}]}}\n' "$id" ;;
    *) ;;
  esac
done
"##,
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let mut config = McpConfig::default();
        config.startup_timeout_secs = 10;
        config.servers.insert("echo-srv".into(), McpServerConfig::stdio(script.display().to_string(), vec![]));
        let manager = McpManager::new(&config, dir.path().to_path_buf(), None);
        manager.ready().await;
        let status = manager.status();
        assert_eq!(status[0].state, McpServerState::Connected, "{:?}", status[0].error);
        assert_eq!(status[0].server_name.as_deref(), Some("echo"));
        assert_eq!(status[0].tools.len(), 1);
        assert_eq!(status[0].tools[0].qualified_name, "mcp__echo-srv__echo");
        assert!(status[0].tools[0].read_only);
        let tools = manager.tools();
        assert_eq!(tools.len(), 1);
        assert_eq!(manager.read_only_tools().len(), 1);
        let client = match &*manager.slots.read()["echo-srv"].state.lock() {
            SlotState::Connected { client, .. } => client.clone(),
            _ => unreachable!(),
        };
        let result = client.call_tool("echo", serde_json::json!({"text": "hi"})).await.unwrap();
        assert_eq!(result.text, "echoed");
        manager.shutdown().await;
        assert!(manager.tools().is_empty());
    }
}
