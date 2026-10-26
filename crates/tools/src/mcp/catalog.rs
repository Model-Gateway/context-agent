//! The app catalog: curated MCP servers the UIs offer as "apps". Installing
//! one writes a `[mcp.servers.<id>]` entry from its template plus the
//! values the user supplies for its inputs (tokens, keys).

use ctx_core::McpServerConfig;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Something an app needs from the user before it can run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogInput {
    /// The environment variable (stdio) or header name (http) it fills.
    pub key: String,
    pub label: String,
    pub help: String,
    pub secret: bool,
    pub required: bool,
    /// Where the value goes: `env` or `header`.
    pub kind: String,
}

/// One line of "What Context can access" on the connect screen: a group of
/// the app's tools the user can switch off before connecting (their
/// patterns go to the entry's `exclude_tools`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogAccess {
    pub title: String,
    pub description: String,
    /// Tool-name patterns (`*` wildcards) this group covers.
    pub tools: Vec<String>,
    /// Always on: the app is useless without it.
    pub required: bool,
}

fn access(title: &str, description: &str, tools: &[&str]) -> CatalogAccess {
    CatalogAccess { title: title.into(), description: description.into(), tools: tools.iter().map(|t| t.to_string()).collect(), required: false }
}

fn required(title: &str, description: &str) -> CatalogAccess {
    CatalogAccess { title: title.into(), description: description.into(), tools: vec!["*".into()], required: true }
}

/// Signing in instead of pasting a token (see `oauth.rs`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogOAuth {
    /// `github`.
    pub provider: String,
    /// The credential-store entry the token goes to.
    pub credential: String,
    /// The input the sign-in fills (with `${credential:<credential>}`).
    pub input: String,
    pub scopes: Vec<String>,
    /// Whether this build can sign in (a client ID is configured).
    pub available: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogApp {
    /// The `[mcp.servers.<id>]` key an install writes.
    pub id: String,
    pub name: String,
    /// One line for a card.
    pub tagline: String,
    /// A prompt that shows what the app is for ("Review open issues…").
    pub example_prompt: String,
    pub description: String,
    /// SF Symbol name for the app's icon (the fallback when `logo` is absent).
    pub icon: String,
    /// Name of the brand logo bundled with the desktop app (its asset
    /// catalog holds `logo-<name>`); `None` keeps the symbol.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
    /// The logo is a single-colour mark to tint for light and dark mode.
    #[serde(default)]
    pub logo_monochrome: bool,
    pub category: String,
    pub vendor: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    /// Served by the `ctx` binary itself.
    pub builtin: bool,
    /// Installed on every installation unless removed.
    pub default_installed: bool,
    /// What must be on the machine (`npx`, `uvx`, …); empty for built-ins.
    pub requirements: Vec<String>,
    pub inputs: Vec<CatalogInput>,
    /// What the app's tools can do, grouped for the connect screen.
    pub access: Vec<CatalogAccess>,
    /// Sign-in in place of the token input, when the service supports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth: Option<CatalogOAuth>,
    /// The entry an install writes (inputs are added to `env` / `headers`).
    pub template: McpServerConfig,
}

impl CatalogApp {
    /// The server entry for this app with the user's `inputs` filled in.
    /// A `${VAR}` value is kept as is (the variable is read at start-up).
    pub fn server(&self, inputs: &BTreeMap<String, String>) -> McpServerConfig {
        let mut server = self.template.clone();
        server.app = Some(self.id.clone());
        server.exclude_tools.clear();
        for input in &self.inputs {
            let Some(value) = inputs.get(&input.key).map(|v| v.trim()).filter(|v| !v.is_empty()) else { continue };
            match input.kind.as_str() {
                "header" => {
                    server.headers.insert(input.key.clone(), value.to_string());
                }
                _ => {
                    server.env.insert(input.key.clone(), value.to_string());
                }
            }
        }
        server
    }

    /// Required inputs that `inputs` leaves empty.
    pub fn missing_inputs<'a>(&'a self, inputs: &BTreeMap<String, String>) -> Vec<&'a CatalogInput> {
        self.inputs.iter().filter(|i| i.required && inputs.get(&i.key).map(|v| v.trim().is_empty()).unwrap_or(true)).collect()
    }
}

fn env_input(key: &str, label: &str, help: &str) -> CatalogInput {
    CatalogInput { key: key.into(), label: label.into(), help: help.into(), secret: true, required: true, kind: "env".into() }
}

fn npx(package: &str) -> McpServerConfig {
    McpServerConfig::stdio("npx", vec!["-y".into(), package.into()])
}

/// Every app this platform can run, built-ins first, then alphabetical.
/// Computer Use is macOS only.
pub fn catalog() -> Vec<CatalogApp> {
    all_apps().into_iter().filter(|a| cfg!(target_os = "macos") || a.id != "computer-use").collect()
}

fn all_apps() -> Vec<CatalogApp> {
    let node = vec!["Node.js (npx)".to_string()];
    vec![
        CatalogApp {
            id: "computer-use".into(),
            name: "Computer Use".into(),
            tagline: "See the screen and drive the mouse and keyboard".into(),
            example_prompt: "Open Safari, sign in to the staging site and tell me what looks broken".into(),
            description: "Lets the agent look at your Mac's screen and act on it: take screenshots, click, drag, scroll, type and press keys, open apps and URLs. Built into ctx; nothing to install. macOS asks once for Screen Recording and Accessibility permission for Context (or the terminal running ctx). Every action except screenshots asks for approval under the Ask profile.".into(),
            icon: "macwindow.and.cursorarrow".into(),
            logo: None,
            logo_monochrome: false,
            category: "Automation".into(),
            vendor: "Context".into(),
            homepage: None,
            builtin: true,
            default_installed: true,
            requirements: Vec::new(),
            inputs: Vec::new(),
            access: vec![
                access("See the screen", "Take screenshots and read the pointer position", &["screenshot", "cursor_position", "wait"]),
                access("Control the mouse", "Move, click, drag and scroll anywhere on screen", &["click", "move_mouse", "drag", "scroll"]),
                access("Type and press keys", "Type text and use keyboard shortcuts in the focused app", &["type", "key"]),
                access("Open apps and links", "Launch applications and open URLs or files", &["open"]),
            ],
            oauth: None,
            template: McpServerConfig::builtin("computer-use"),
        },
        CatalogApp {
            id: "context7".into(),
            name: "Context7".into(),
            tagline: "Up-to-date library documentation".into(),
            example_prompt: "Check the current Next.js docs before you change the router".into(),
            description: "Fetches current, version-specific documentation and code examples for libraries and frameworks, so the agent reads the real API instead of guessing from training data.".into(),
            icon: "books.vertical".into(),
            logo: Some("upstash".into()),
            logo_monochrome: false,
            category: "Documentation".into(),
            vendor: "Upstash".into(),
            homepage: Some("https://github.com/upstash/context7".into()),
            builtin: false,
            default_installed: false,
            requirements: node.clone(),
            inputs: vec![CatalogInput { key: "CONTEXT7_API_KEY".into(), label: "API key".into(), help: "Optional; raises the rate limit. From context7.com.".into(), secret: true, required: false, kind: "env".into() }],
            access: vec![required("Read library documentation", "Look up libraries and fetch their current docs and examples")],
            oauth: None,
            template: npx("@upstash/context7-mcp"),
        },
        CatalogApp {
            id: "fetch".into(),
            name: "Fetch".into(),
            tagline: "Read web pages as Markdown".into(),
            example_prompt: "Read this changelog page and summarize what affects us".into(),
            description: "Downloads a URL and converts the page to Markdown the agent can read, with paging for long pages. Complements the built-in web_fetch with a different extractor.".into(),
            icon: "arrow.down.doc".into(),
            logo: Some("mcp".into()),
            logo_monochrome: true,
            category: "Web".into(),
            vendor: "Model Context Protocol".into(),
            homepage: Some("https://github.com/modelcontextprotocol/servers/tree/main/src/fetch".into()),
            builtin: false,
            default_installed: false,
            requirements: vec!["uv (uvx)".into()],
            inputs: Vec::new(),
            access: vec![required("Fetch web pages", "Download a URL and read it as Markdown")],
            oauth: None,
            template: McpServerConfig::stdio("uvx", vec!["mcp-server-fetch".into()]),
        },
        CatalogApp {
            id: "github".into(),
            name: "GitHub".into(),
            tagline: "Issues, pull requests and repositories".into(),
            example_prompt: "Review open issues and pull requests".into(),
            description: "Search and read issues and pull requests, open and comment on them, inspect files and commits across your repositories with a personal access token.".into(),
            icon: "chevron.left.forwardslash.chevron.right".into(),
            logo: Some("github".into()),
            logo_monochrome: true,
            category: "Development".into(),
            vendor: "GitHub".into(),
            homepage: Some("https://github.com/github/github-mcp-server".into()),
            builtin: false,
            default_installed: false,
            requirements: node.clone(),
            inputs: vec![env_input("GITHUB_PERSONAL_ACCESS_TOKEN", "Personal access token", "A fine-grained or classic token from github.com/settings/tokens with the repository scopes you want the agent to have.")],
            access: vec![
                access("Read repositories and code", "Browse files, commits and search code across your repositories", &["get_file_contents", "search_repositories", "search_code", "list_commits", "search_users"]),
                access("Read issues and pull requests", "View issues, pull requests, reviews and their history", &["get_issue", "list_issues", "search_issues", "get_pull_request*", "list_pull_requests"]),
                access("Create and update issues", "Open issues, edit them and comment", &["create_issue", "update_issue", "add_issue_comment"]),
                access("Open and review pull requests", "Create pull requests, leave reviews and update branches", &["create_pull_request", "create_pull_request_review", "update_pull_request_branch"]),
                access("Write to repositories", "Push files, create branches and repositories, merge pull requests", &["create_or_update_file", "push_files", "create_branch", "create_repository", "fork_repository", "merge_pull_request"]),
            ],
            oauth: Some(CatalogOAuth {
                provider: "github".into(),
                credential: "github".into(),
                input: "GITHUB_PERSONAL_ACCESS_TOKEN".into(),
                scopes: vec!["repo".into(), "read:org".into(), "read:user".into()],
                available: crate::mcp::oauth::github_client_id().is_some(),
            }),
            template: npx("@modelcontextprotocol/server-github"),
        },
        CatalogApp {
            id: "memory".into(),
            name: "Knowledge Graph".into(),
            tagline: "Persistent memory across sessions".into(),
            example_prompt: "Remember how we deploy, so you can do it next time".into(),
            description: "A local knowledge graph of entities, relations and observations the agent can write to and recall from in later sessions, on top of Context's own memory.".into(),
            icon: "brain".into(),
            logo: Some("mcp".into()),
            logo_monochrome: true,
            category: "Memory".into(),
            vendor: "Model Context Protocol".into(),
            homepage: Some("https://github.com/modelcontextprotocol/servers/tree/main/src/memory".into()),
            builtin: false,
            default_installed: false,
            requirements: node.clone(),
            inputs: Vec::new(),
            access: vec![
                access("Read memories", "Search and read the knowledge graph", &["read_graph", "search_nodes", "open_nodes"]),
                access("Save memories", "Add entities, relations and observations", &["create_*", "add_*"]),
                access("Delete memories", "Remove entities, relations and observations", &["delete_*"]),
            ],
            oauth: None,
            template: npx("@modelcontextprotocol/server-memory"),
        },
        CatalogApp {
            id: "playwright".into(),
            name: "Playwright".into(),
            tagline: "Automate a real browser".into(),
            example_prompt: "Click through the signup flow and report what breaks".into(),
            description: "Drives Chromium through Playwright: navigate, click, fill forms, take screenshots and read the accessibility tree. Useful for end-to-end checks of a web app, beyond the app's own Browser tab.".into(),
            icon: "theatermasks".into(),
            logo: Some("playwright".into()),
            logo_monochrome: false,
            category: "Web".into(),
            vendor: "Microsoft".into(),
            homepage: Some("https://github.com/microsoft/playwright-mcp".into()),
            builtin: false,
            default_installed: false,
            requirements: node.clone(),
            inputs: Vec::new(),
            access: vec![
                access("Browse and read pages", "Open pages, take snapshots and screenshots, read console and network logs", &["browser_navigate*", "browser_snapshot", "browser_take_screenshot", "browser_console_messages", "browser_network_requests", "browser_tabs", "browser_wait_for", "browser_resize", "browser_close", "browser_install"]),
                access("Interact with pages", "Click, type, fill forms and press keys", &["browser_click", "browser_type", "browser_hover", "browser_drag", "browser_select_option", "browser_press_key", "browser_fill_form", "browser_handle_dialog"]),
                access("Run scripts and upload files", "Evaluate JavaScript in the page and upload local files", &["browser_evaluate", "browser_file_upload", "browser_run_code"]),
            ],
            oauth: None,
            template: npx("@playwright/mcp@latest"),
        },
        CatalogApp {
            id: "sequential-thinking".into(),
            name: "Sequential Thinking".into(),
            tagline: "Structured step-by-step reasoning".into(),
            example_prompt: "Plan the migration step by step before touching code".into(),
            description: "A scratchpad tool for breaking a hard problem into revisable steps; helps models without built-in reasoning plan before they act.".into(),
            icon: "list.number".into(),
            logo: Some("mcp".into()),
            logo_monochrome: true,
            category: "Reasoning".into(),
            vendor: "Model Context Protocol".into(),
            homepage: Some("https://github.com/modelcontextprotocol/servers/tree/main/src/sequentialthinking".into()),
            builtin: false,
            default_installed: false,
            requirements: node,
            inputs: Vec::new(),
            access: vec![required("Think step by step", "Work through a problem as revisable steps")],
            oauth: None,
            template: npx("@modelcontextprotocol/server-sequential-thinking"),
        },
    ]
}

/// The app with this id.
pub fn catalog_app(id: &str) -> Option<CatalogApp> {
    catalog().into_iter().find(|a| a.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_ids_are_valid_server_names_and_templates_validate() {
        let apps = catalog();
        if cfg!(target_os = "macos") {
            assert!(apps.iter().any(|a| a.id == "computer-use" && a.builtin && a.default_installed));
            assert_eq!(apps.iter().filter(|a| a.default_installed).count(), 1, "only Computer Use is installed by default");
        } else {
            assert!(!apps.iter().any(|a| a.id == "computer-use"), "Computer Use is macOS only");
            assert_eq!(apps.iter().filter(|a| a.default_installed).count(), 0);
        }
        let apps = all_apps();
        for app in &apps {
            assert!(McpServerConfig::valid_name(&app.id), "{}", app.id);
            assert!(!app.access.is_empty(), "{} lists what it can access", app.id);
            app.server(&BTreeMap::new()).validate(&app.id).unwrap();
            assert_eq!(app.builtin, app.template.transport == ctx_core::McpTransport::Builtin);
        }
    }

    #[test]
    fn inputs_land_in_env_or_headers() {
        let github = catalog_app("github").unwrap();
        assert_eq!(github.missing_inputs(&BTreeMap::new()).len(), 1);
        let mut inputs = BTreeMap::new();
        inputs.insert("GITHUB_PERSONAL_ACCESS_TOKEN".to_string(), "${GITHUB_TOKEN}".to_string());
        let server = github.server(&inputs);
        assert_eq!(server.env["GITHUB_PERSONAL_ACCESS_TOKEN"], "${GITHUB_TOKEN}");
        assert_eq!(server.app.as_deref(), Some("github"));
        assert!(github.missing_inputs(&inputs).is_empty());
    }
}
