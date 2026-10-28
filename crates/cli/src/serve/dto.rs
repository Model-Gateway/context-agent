//! Wire shapes: every result and notification payload is an explicit DTO mapped by hand from the library structs, plus the request parameter types.

use super::*;

// ---------------------------------------------------------------------------
// DTOs: results
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub(super) struct ConfigInfo {
    pub(super) protocol_version: u32,
    pub(super) ctx_version: &'static str,
    pub(super) data_dir: String,
    pub(super) config_file: String,
    pub(super) config_sources: Vec<String>,
    pub(super) mock: bool,
    pub(super) model: ModelInfo,
    pub(super) permissions: PermissionsInfo,
    pub(super) pricing: Option<PricingInfo>,
    pub(super) spend: ctx_core::SpendConfig,
    pub(super) mcp: ctx_core::McpConfig,
    /// The machine `ctx serve` runs on; the desktop app shows it for a
    /// backend reached over SSH and starts its folder picker in `home_dir`.
    pub(super) host: HostInfo,
}

/// `initialize.host`: where this server runs.
#[derive(Serialize, Clone)]
pub(super) struct HostInfo {
    pub(super) hostname: String,
    pub(super) user: String,
    pub(super) home_dir: String,
    /// `std::env::consts::OS`: "macos", "linux", "windows", …
    pub(super) os: &'static str,
}

/// `fs/list` params: a directory to list, `~`-expanded; absent or empty is
/// the home directory.
#[derive(Deserialize)]
pub(super) struct FsListParams {
    #[serde(default)]
    pub(super) path: Option<String>,
    /// Leave files out (a folder picker).
    #[serde(default)]
    pub(super) dirs_only: bool,
}

/// `fs/list` result.
#[derive(Serialize)]
pub(super) struct FsListing {
    /// The listed directory, canonical.
    pub(super) path: String,
    /// Its parent, absent at the root.
    pub(super) parent: Option<String>,
    pub(super) home: String,
    pub(super) entries: Vec<FsEntry>,
}

#[derive(Serialize)]
pub(super) struct FsEntry {
    pub(super) name: String,
    pub(super) path: String,
    pub(super) is_dir: bool,
    /// A directory holding a `.git` entry (a repository root).
    pub(super) is_git: bool,
    pub(super) is_hidden: bool,
}

#[derive(Deserialize)]
pub(super) struct McpInstallParams {
    pub(super) id: String,
    /// Values for the app's inputs, by key (environment variable or header).
    #[serde(default)]
    pub(super) inputs: BTreeMap<String, String>,
    /// Tool patterns switched off on the connect screen.
    #[serde(default)]
    pub(super) exclude_tools: Vec<String>,
}

#[derive(Deserialize)]
pub(super) struct McpUninstallParams {
    pub(super) id: String,
}

/// `mcp/oauth/start`, `mcp/oauth/account`, `mcp/oauth/signout`.
#[derive(Deserialize)]
pub(super) struct McpOAuthAppParams {
    /// The catalog app (`github`).
    pub(super) app: String,
}

#[derive(Deserialize)]
pub(super) struct McpOAuthPollParams {
    pub(super) app: String,
    pub(super) device_code: String,
}

#[derive(Deserialize, Default)]
pub(super) struct McpReconnectParams {
    /// One server by name; every active one when absent.
    #[serde(default)]
    pub(super) server: Option<String>,
}

#[derive(Serialize)]
pub(super) struct ModelInfo {
    pub(super) name: String,
    pub(super) base_url: String,
    pub(super) api_key_env: String,
    pub(super) api_key_set: bool,
    /// `model.credential`: the stored credential the key comes from, if any.
    pub(super) credential: Option<String>,
    pub(super) context_window: u32,
    pub(super) max_output_tokens: u32,
    pub(super) reasoning_effort: Option<ctx_core::ReasoningEffort>,
}

#[derive(Serialize)]
pub(super) struct PermissionsInfo {
    pub(super) profile: &'static str,
    pub(super) allowed_commands: Vec<String>,
    pub(super) denied_commands: Vec<String>,
    pub(super) allow_network_commands: bool,
}

/// `[pricing]`: the flat fallback prices (each optional) and per-model prices.
#[derive(Serialize)]
pub(super) struct PricingInfo {
    pub(super) input_per_million: Option<f64>,
    pub(super) cached_input_per_million: Option<f64>,
    pub(super) output_per_million: Option<f64>,
    pub(super) models: BTreeMap<String, ctx_core::ModelPrice>,
}

#[derive(Serialize)]
pub(super) struct LanguageCount {
    pub(super) language: String,
    pub(super) files: u64,
}

#[derive(Serialize)]
pub(super) struct GitSummary {
    pub(super) branch: Option<String>,
    pub(super) head: Option<String>,
    pub(super) dirty_files: usize,
    /// Set when the root is a linked worktree of another checkout
    /// (`worktree/create`): the app nests it under that project.
    pub(super) worktree: Option<WorktreeSummary>,
}

#[derive(Serialize, Clone)]
pub(super) struct WorktreeSummary {
    /// The main worktree's root (the parent project's `root`).
    pub(super) parent_root: String,
    /// Whether the worktree lives in the data dir (`worktree/create` made it).
    pub(super) managed: bool,
}

#[derive(Deserialize)]
pub(super) struct WorktreeCreateParams {
    pub(super) project_id: String,
    /// Folder and branch name (slugified); a random pair of words otherwise.
    #[serde(default)]
    pub(super) name: Option<String>,
    /// Commit-ish the new branch starts from (`HEAD` of the project otherwise).
    #[serde(default)]
    pub(super) base: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct WorktreeRemoveParams {
    pub(super) project_id: String,
    #[serde(default)]
    pub(super) delete_branch: bool,
    /// Remove even with uncommitted changes (`git worktree remove --force`).
    #[serde(default)]
    pub(super) force: bool,
}

#[derive(Deserialize)]
pub(super) struct WorktreeMergeParams {
    pub(super) project_id: String,
    /// Remove the worktree and delete its branch once the merge landed.
    #[serde(default)]
    pub(super) delete_after: bool,
}

#[derive(Serialize)]
pub(super) struct WorktreeEntry {
    pub(super) path: String,
    pub(super) branch: Option<String>,
    pub(super) head: Option<String>,
    pub(super) is_main: bool,
    pub(super) managed: bool,
}

#[derive(Serialize)]
pub(super) struct IndexInfo {
    pub(super) generation: u64,
    pub(super) files: u64,
    pub(super) symbols: u64,
    pub(super) chunks: u64,
    pub(super) relationships: u64,
    pub(super) embeddings: u64,
    pub(super) last_full_index_at: Option<String>,
    pub(super) languages: Vec<LanguageCount>,
}

#[derive(Serialize)]
pub(super) struct ValidationCommandInfo {
    pub(super) command: String,
    pub(super) kind: String,
    pub(super) cwd: String,
    pub(super) source: String,
}

#[derive(Serialize, Clone)]
pub(super) struct RunningTaskInfo {
    pub(super) session_id: String,
    pub(super) task_id: Option<String>,
    pub(super) started_at: String,
}

#[derive(Serialize)]
pub(super) struct ProjectInfo {
    pub(super) project_id: String,
    pub(super) root: String,
    pub(super) name: String,
    pub(super) git: Option<GitSummary>,
    /// True on the `project/open` that ran `git init` for this folder.
    pub(super) git_initialized: bool,
    pub(super) index: IndexInfo,
    pub(super) indexing: bool,
    pub(super) model: String,
    pub(super) permission_profile: &'static str,
    pub(super) sandbox: &'static str,
    pub(super) validation_commands: Vec<ValidationCommandInfo>,
    pub(super) languages: Vec<LanguageCount>,
    pub(super) build_files: Vec<String>,
    pub(super) file_count: usize,
    /// The oldest of `running_tasks` (clients written for one run per project).
    pub(super) running_task: Option<RunningTaskInfo>,
    /// Every session's running task, oldest first.
    pub(super) running_tasks: Vec<RunningTaskInfo>,
}

#[derive(Serialize)]
pub(super) struct SessionSummary {
    pub(super) id: String,
    pub(super) model: String,
    pub(super) mode: String,
    pub(super) title: String,
    pub(super) created_at: String,
    pub(super) updated_at: String,
    pub(super) active_task_id: Option<String>,
    pub(super) task_count: usize,
    pub(super) last_status: Option<String>,
}

#[derive(Serialize)]
pub(super) struct TaskSummary {
    pub(super) id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) session_id: Option<String>,
    pub(super) objective: String,
    pub(super) category: String,
    pub(super) state: String,
    pub(super) status: Option<String>,
    pub(super) summary: Option<String>,
    pub(super) created_at: String,
    pub(super) updated_at: String,
    pub(super) completed_at: Option<String>,
    pub(super) modified_files: Vec<String>,
    pub(super) unresolved: Vec<String>,
}

#[derive(Serialize)]
pub(super) struct ToolCallInfo {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) arguments: Value,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum HistoryMessage {
    User {
        id: i64,
        text: String,
        /// The image files sent with the message (they may since be gone).
        #[serde(skip_serializing_if = "Vec::is_empty")]
        images: Vec<String>,
        /// The text files sent with the message.
        #[serde(skip_serializing_if = "Vec::is_empty")]
        files: Vec<HistoryFile>,
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        runtime: bool,
        /// A working-tree snapshot exists for the run this message started
        /// (`session/rewind` can restore files to it).
        snapshot: bool,
        created_at: String,
    },
    Assistant {
        id: i64,
        text: Option<String>,
        tool_calls: Vec<ToolCallInfo>,
        created_at: String,
    },
    ToolResult {
        id: i64,
        call_id: String,
        tool_name: String,
        content: String,
        is_error: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        full_len: Option<u64>,
        created_at: String,
    },
}

#[derive(Serialize)]
pub(super) struct UsageInfo {
    pub(super) prompt_tokens: u64,
    pub(super) completion_tokens: u64,
    pub(super) cached_tokens: u64,
    pub(super) requests: u64,
    pub(super) cost_usd: Option<f64>,
}

#[derive(Serialize)]
pub(super) struct Turn {
    pub(super) task: TaskSummary,
    pub(super) messages: Vec<HistoryMessage>,
    pub(super) events: Vec<AgentEvent>,
    pub(super) outcome: Option<TaskOutcome>,
    pub(super) usage: UsageInfo,
}

#[derive(Serialize)]
pub(super) struct SessionHistory {
    pub(super) session: SessionSummary,
    pub(super) turns: Vec<Turn>,
}

#[derive(Serialize)]
pub(super) struct CapabilitySummary {
    pub(super) probed_at: Option<String>,
    pub(super) agent_capable: bool,
    pub(super) streaming: bool,
    pub(super) tool_calling: bool,
    pub(super) parallel_tool_calls: bool,
    pub(super) reasoning_controls: bool,
    pub(super) issues: Vec<String>,
}

impl CapabilitySummary {
    pub(super) fn from_profile(p: &CapabilityProfile) -> Self {
        Self {
            probed_at: p.probed_at.map(|t| t.to_rfc3339()),
            agent_capable: p.is_agent_capable(),
            streaming: !p.streaming.is_known_unsupported(),
            tool_calling: p.tool_calling.is_supported(),
            parallel_tool_calls: !p.parallel_tool_calls.is_known_unsupported(),
            reasoning_controls: p.reasoning_controls.is_supported(),
            issues: p.blocking_issues(),
        }
    }

    pub(super) fn mock() -> Self {
        Self { probed_at: None, agent_capable: true, streaming: true, tool_calling: true, parallel_tool_calls: true, reasoning_controls: false, issues: Vec::new() }
    }
}

#[derive(Serialize)]
pub(super) struct ModelsList {
    pub(super) models: Vec<String>,
    pub(super) current: String,
    pub(super) source: &'static str,
    pub(super) error: Option<String>,
    /// ModelGateway's savings and 24 h error rate per model (`model_stats`);
    /// empty for other endpoints.
    pub(super) stats: crate::model_stats::ModelStats,
}

#[derive(Serialize)]
pub(super) struct GitFileStatus {
    pub(super) path: String,
    pub(super) status: String,
    pub(super) staged: bool,
    /// Lines added / removed against HEAD (`git diff --numstat`; an untracked
    /// file counts its lines). `null` for binary files and when unknown.
    pub(super) insertions: Option<u64>,
    pub(super) deletions: Option<u64>,
}

#[derive(Serialize)]
pub(super) struct GitStatus {
    pub(super) branch: Option<String>,
    pub(super) head: Option<String>,
    /// Subject of the HEAD commit (for "Undo last commit").
    pub(super) head_subject: Option<String>,
    /// `origin/main` when the branch tracks one, with the commits ahead / behind it.
    pub(super) upstream: Option<String>,
    pub(super) ahead: u64,
    pub(super) behind: u64,
    pub(super) remotes: Vec<gitops::GitRemote>,
    pub(super) files: Vec<GitFileStatus>,
}

#[derive(Serialize)]
pub(super) struct FileContent {
    pub(super) path: String,
    pub(super) content: String,
    pub(super) truncated: bool,
    pub(super) language: Option<String>,
    pub(super) size: u64,
    /// `sha256:…` of the bytes returned (the whole file unless truncated).
    pub(super) hash: String,
}

#[derive(Serialize)]
pub(super) struct FileWriteResult {
    pub(super) path: String,
    pub(super) hash: String,
    pub(super) size: u64,
    pub(super) language: Option<String>,
    pub(super) created: bool,
}

#[derive(Serialize)]
pub(super) struct SymbolInfo {
    pub(super) name: String,
    pub(super) kind: String,
    pub(super) path: String,
    pub(super) parent: Option<String>,
    pub(super) line: usize,
    pub(super) end_line: usize,
    pub(super) signature: Option<String>,
}

#[derive(Serialize)]
pub(super) struct MemoryInfo {
    pub(super) id: String,
    pub(super) category: String,
    pub(super) verification: String,
    pub(super) content: String,
    pub(super) evidence_paths: Vec<String>,
    pub(super) created_at: String,
    pub(super) invalidated: bool,
}

#[derive(Serialize)]
pub(super) struct DoctorRow {
    pub(super) label: String,
    pub(super) value: String,
    pub(super) ok: bool,
}

#[derive(Serialize)]
pub(super) struct DoctorSection {
    pub(super) title: String,
    pub(super) rows: Vec<DoctorRow>,
}

#[derive(Serialize, Default)]
pub(super) struct DoctorReport {
    pub(super) problems: u32,
    pub(super) sections: Vec<DoctorSection>,
}

impl DoctorReport {
    pub(super) fn section(&mut self, title: &str) -> &mut DoctorSection {
        self.sections.push(DoctorSection { title: title.to_string(), rows: Vec::new() });
        self.sections.last_mut().expect("just pushed")
    }

    pub(super) fn row(&mut self, label: &str, value: impl Into<String>, ok: bool) {
        if !ok {
            self.problems += 1;
        }
        let row = DoctorRow { label: label.to_string(), value: value.into(), ok };
        match self.sections.last_mut() {
            Some(s) => s.rows.push(row),
            None => self.section("general").rows.push(row),
        }
    }
}

// ---------------------------------------------------------------------------
// DTOs: params
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub(super) struct ProjectParams {
    pub(super) project_id: String,
}

#[derive(Deserialize)]
pub(super) struct OpenParams {
    pub(super) path: String,
    #[serde(default)]
    pub(super) exact: bool,
    /// Run `git init` first when the folder is outside any repository.
    #[serde(default)]
    pub(super) init_git: bool,
}

#[derive(Deserialize)]
pub(super) struct RewindParams {
    pub(super) project_id: String,
    pub(super) session_id: String,
    /// A user message (`session/history` or `task/finished.message_id`).
    pub(super) message_id: i64,
    /// Put the working tree back to the snapshot taken before that message.
    #[serde(default)]
    pub(super) restore_files: bool,
}

fn default_limit_100() -> usize {
    100
}

fn default_limit_50() -> usize {
    50
}

#[derive(Deserialize)]
pub(super) struct SessionsListParams {
    pub(super) project_id: String,
    #[serde(default = "default_limit_100")]
    pub(super) limit: usize,
}

#[derive(Deserialize)]
pub(super) struct SessionCreateParams {
    pub(super) project_id: String,
    #[serde(default)]
    pub(super) title: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct SessionRenameParams {
    pub(super) project_id: String,
    pub(super) session_id: String,
    pub(super) title: String,
}

#[derive(Deserialize)]
pub(super) struct SessionParams {
    pub(super) project_id: String,
    pub(super) session_id: String,
}

#[derive(Deserialize, Default)]
pub(super) struct RunOptionsParams {
    #[serde(default)]
    pub(super) max_steps: Option<u32>,
    #[serde(default)]
    pub(super) proactive_retrieval: Option<bool>,
    #[serde(default)]
    pub(super) memory_context: Option<bool>,
    #[serde(default)]
    pub(super) stream: Option<bool>,
    #[serde(default)]
    pub(super) debug_prompt: Option<bool>,
}

impl RunOptionsParams {
    pub(super) fn into_options(self) -> RunOptions {
        let mut o = RunOptions::default();
        if let Some(v) = self.max_steps {
            o.max_steps = Some(v);
        }
        if let Some(v) = self.proactive_retrieval {
            o.proactive_retrieval = v;
        }
        if let Some(v) = self.memory_context {
            o.memory_context = v;
        }
        if let Some(v) = self.stream {
            o.stream = v;
        }
        if let Some(v) = self.debug_prompt {
            o.debug_prompt = v;
        }
        o
    }
}

#[derive(Deserialize)]
pub(super) struct TaskRunParams {
    pub(super) project_id: String,
    pub(super) session_id: String,
    pub(super) prompt: String,
    /// PNG, JPEG, GIF or WebP images sent with the prompt: absolute paths
    /// on this machine, or the bytes inline (a client on another machine).
    #[serde(default)]
    pub(super) images: Vec<ImageParam>,
    /// UTF-8 text files sent with the prompt, inlined into the message.
    #[serde(default)]
    pub(super) files: Vec<FileParam>,
    /// Omitted or `null` both mean `RunOptions::default()`.
    #[serde(default)]
    pub(super) options: Option<RunOptionsParams>,
}

/// One `task/run` image: `"/abs/path.png"`, or `{ "name", "data" }` with the
/// file's bytes as base64 when the client cannot share a filesystem with the
/// server (the desktop app over SSH).
#[derive(Deserialize, Clone)]
#[serde(untagged)]
pub(super) enum ImageParam {
    Path(String),
    Inline {
        /// Base64 (standard alphabet) of the image file's bytes.
        data: String,
        /// Shown in `session/history` in place of a path.
        #[serde(default)]
        name: Option<String>,
    },
}

#[derive(Deserialize, Clone)]
pub(super) struct FileParam {
    /// Absolute path of a UTF-8 text file on this machine; absent when the
    /// text comes inline in `content`.
    #[serde(default)]
    pub(super) path: Option<String>,
    /// What the model is told the file is called; defaults to the path's
    /// file name (a client sends `report.pdf` for text it extracted from it).
    /// Required with `content`.
    #[serde(default)]
    pub(super) name: Option<String>,
    /// The file's text, sent by a client on another machine instead of a path.
    #[serde(default)]
    pub(super) content: Option<String>,
}

#[derive(Serialize)]
pub(super) struct HistoryFile {
    pub(super) path: String,
    pub(super) name: String,
}

#[derive(Deserialize)]
pub(super) struct ApprovalParams {
    pub(super) project_id: String,
    /// The session whose run asked; omitted, the request id alone decides.
    #[serde(default)]
    pub(super) session_id: Option<String>,
    pub(super) request_id: String,
    pub(super) decision: String,
}

/// `question/respond`: the app's answer to one `question/request`.
#[derive(Deserialize)]
pub(super) struct QuestionRespondParams {
    pub(super) project_id: String,
    pub(super) session_id: String,
    pub(super) request_id: String,
    #[serde(default)]
    pub(super) answers: Option<Vec<QuestionAnswer>>,
    #[serde(default)]
    pub(super) message: Option<String>,
}

/// `browser/respond`: the app's answer to one `browser/request`.
#[derive(Deserialize)]
pub(super) struct BrowserRespondParams {
    pub(super) project_id: String,
    pub(super) session_id: String,
    pub(super) request_id: String,
    #[serde(default = "default_true")]
    pub(super) ok: bool,
    #[serde(default)]
    pub(super) content: String,
    #[serde(default)]
    pub(super) url: Option<String>,
    #[serde(default)]
    pub(super) title: Option<String>,
    #[serde(default)]
    pub(super) error: Option<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize)]
pub(super) struct TaskSteerParams {
    pub(super) project_id: String,
    pub(super) session_id: String,
    pub(super) prompt: String,
    /// As in `task/run`.
    #[serde(default)]
    pub(super) images: Vec<ImageParam>,
    #[serde(default)]
    pub(super) files: Vec<FileParam>,
}

#[derive(Deserialize)]
pub(super) struct TaskCancelParams {
    pub(super) project_id: String,
    /// The session whose run to cancel; omitted, every run of the project.
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct PermissionsParams {
    pub(super) project_id: String,
    pub(super) profile: String,
}

#[derive(Deserialize, Default)]
pub(super) struct ModelsListParams {
    #[serde(default)]
    pub(super) project_id: Option<String>,
    #[serde(default)]
    pub(super) refresh: bool,
}

#[derive(Deserialize)]
pub(super) struct ModelsSwitchParams {
    pub(super) project_id: String,
    pub(super) model: String,
}

#[derive(Deserialize)]
pub(super) struct GitDiffParams {
    pub(super) project_id: String,
    #[serde(default)]
    pub(super) path: Option<String>,
    #[serde(default)]
    pub(super) staged: bool,
}

#[derive(Deserialize)]
pub(super) struct FilesSearchParams {
    pub(super) project_id: String,
    #[serde(default)]
    pub(super) query: String,
    #[serde(default)]
    pub(super) limit: Option<usize>,
}

#[derive(Deserialize)]
pub(super) struct FileReadParams {
    pub(super) project_id: String,
    pub(super) path: String,
    #[serde(default)]
    pub(super) max_bytes: Option<usize>,
}

#[derive(Deserialize)]
pub(super) struct FileWriteParams {
    pub(super) project_id: String,
    pub(super) path: String,
    pub(super) content: String,
    #[serde(default)]
    pub(super) expected_hash: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct SymbolsFindParams {
    pub(super) project_id: String,
    pub(super) query: String,
    #[serde(default)]
    pub(super) mode: SymbolsFindMode,
    #[serde(default = "default_limit_50")]
    pub(super) limit: usize,
}

#[derive(Deserialize, Default, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub(super) enum SymbolsFindMode {
    #[default]
    Prefix,
    Exact,
    Contains,
}

#[derive(Deserialize)]
pub(super) struct TasksListParams {
    pub(super) project_id: String,
    #[serde(default = "default_limit_50")]
    pub(super) limit: usize,
}

#[derive(Deserialize)]
pub(super) struct MemoryListParams {
    pub(super) project_id: String,
    #[serde(default)]
    pub(super) category: Option<String>,
    #[serde(default = "default_limit_50")]
    pub(super) limit: usize,
}

#[derive(Deserialize, Default)]
pub(super) struct DoctorParams {
    #[serde(default)]
    pub(super) project_id: Option<String>,
    #[serde(default)]
    pub(super) probe: bool,
}

#[derive(Deserialize)]
pub(super) struct ConfigPatchParams {
    pub(super) values: serde_json::Map<String, Value>,
}
