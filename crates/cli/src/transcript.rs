//! The interactive session's conversation: what happened, and how it is laid
//! out into terminal rows (hanging-indent word wrap, light markdown, thinking
//! and tool rows, task outcome footers).

use ctx_core::{AgentEvent, AgentReport, TaskOutcome, TokenUsage};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use serde_json::Value;
use std::time::{Duration, Instant};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// 256-colour palette: mid tones that stay legible on dark and light
/// terminal backgrounds.
pub mod theme {
    use ratatui::style::Color;

    pub const ACCENT: Color = Color::Indexed(74);
    pub const AGENT: Color = Color::Indexed(108);
    pub const THINK: Color = Color::Indexed(140);
    pub const CODE: Color = Color::Indexed(173);
    pub const OK: Color = Color::Indexed(71);
    pub const WARN: Color = Color::Indexed(178);
    pub const ERR: Color = Color::Indexed(167);
    pub const DIM: Color = Color::Indexed(244);
    pub const FAINT: Color = Color::Indexed(239);
    pub const SELECTED_BG: Color = Color::Indexed(236);
}

use theme::*;

#[cfg(test)]
const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// The status glyphs the TUI and `ctx run` draw. Every glyph is one cell
/// wide in both sets, so layouts hold either way.
pub struct Glyphs {
    pub spinner: &'static [&'static str],
    pub ok: &'static str,
    pub fail: &'static str,
    pub blocked: &'static str,
    pub pending: &'static str,
    /// Sub-agents.
    pub agents: &'static str,
}

const UNICODE_GLYPHS: Glyphs = Glyphs { spinner: &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"], ok: "✓", fail: "✗", blocked: "◆", pending: "○", agents: "◈" };
/// For consoles whose fonts lack braille and dingbats (legacy conhost with
/// Consolas or Lucida Console): characters from the WGL4 set every Windows
/// console font has.
const FALLBACK_GLYPHS: Glyphs = Glyphs { spinner: &["|", "/", "-", "\\"], ok: "√", fail: "×", blocked: "!", pending: "○", agents: "♦" };

/// The glyph set for this terminal, chosen once: Unicode everywhere except
/// the legacy Windows console host, recognised by its console window
/// (Windows Terminal, VS Code, WezTerm and other ConPTY terminals have a
/// pseudoconsole window instead). Without a console window, a Windows terminal is
/// trusted when it is known to render Unicode (Windows Terminal, VS Code,
/// ConEmu, WezTerm, Alacritty and anything setting `TERM`).
/// `CTX_ASCII=1` forces the fallback set, `CTX_ASCII=0` the Unicode one.
pub fn glyphs() -> &'static Glyphs {
    static CHOSEN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let unicode = *CHOSEN.get_or_init(|| {
        if cfg!(test) {
            return true;
        }
        match std::env::var("CTX_ASCII").ok().as_deref().map(str::trim) {
            Some("1") | Some("true") | Some("yes") => return false,
            Some("0") | Some("false") | Some("no") => return true,
            _ => {}
        }
        if !cfg!(windows) {
            return true;
        }
        // The environment alone is not enough: a conhost started from
        // inside Windows Terminal (`conhost.exe cmd`) inherits WT_SESSION.
        match legacy_console_window() {
            Some(legacy) => !legacy,
            None => ["WT_SESSION", "TERM_PROGRAM", "ConEmuTask", "WEZTERM_EXECUTABLE", "ALACRITTY_LOG", "TERM"].iter().any(|v| std::env::var_os(v).is_some_and(|x| !x.is_empty())),
        }
    });
    if unicode {
        &UNICODE_GLYPHS
    } else {
        &FALLBACK_GLYPHS
    }
}

/// Whether this process's console is the legacy console host's own window:
/// `Some(true)` for a shown `ConsoleWindowClass` window, `Some(false)` for a
/// pseudoconsole (`PseudoConsoleWindow`, behind Windows Terminal, VS Code
/// and other ConPTY terminals) or a hidden console (ConEmu), `None` without
/// a console.
#[cfg(windows)]
fn legacy_console_window() -> Option<bool> {
    use windows_sys::Win32::System::Console::GetConsoleWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetClassNameW, IsWindowVisible};
    // SAFETY: queries on the console window, checked for null; the class
    // name buffer is ours and its length is passed.
    unsafe {
        let window = GetConsoleWindow();
        if window.is_null() {
            return None;
        }
        let mut class = [0u16; 64];
        let len = GetClassNameW(window, class.as_mut_ptr(), class.len() as i32).max(0) as usize;
        Some(String::from_utf16_lossy(&class[..len]) == "ConsoleWindowClass" && IsWindowVisible(window) != 0)
    }
}

#[cfg(not(windows))]
fn legacy_console_window() -> Option<bool> {
    None
}

/// Spinner frame for something that started at `since`.
pub fn spinner(since: Instant) -> &'static str {
    let frames = glyphs().spinner;
    frames[(since.elapsed().as_millis() / 80) as usize % frames.len()]
}

pub fn fmt_duration(d: Duration) -> String {
    let ms = d.as_millis() as u64;
    match ms {
        0..=999 => format!("{ms}ms"),
        1_000..=59_999 => format!("{:.1}s", ms as f64 / 1000.0),
        _ => format!("{}m {}s", ms / 60_000, (ms % 60_000) / 1000),
    }
}

/// Whole seconds, for live timers and thinking time.
pub fn fmt_secs(d: Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m {}s", secs / 60, secs % 60)
    }
}

fn plural(n: u32, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

pub fn fmt_tokens(n: u64) -> String {
    if n < 1000 {
        n.to_string()
    } else {
        format!("{:.1}k", n as f64 / 1000.0)
    }
}

pub struct ToolResult {
    pub ok: bool,
    pub summary: String,
    pub duration: Duration,
}

pub struct OutcomeView {
    pub status: String,
    pub stats: Vec<String>,
    /// Shown only when it adds something to the agent's last message.
    pub summary: Option<String>,
    pub modified: Vec<String>,
    pub unresolved: Vec<String>,
}

/// One tool call made by a sub-agent.
pub struct AgentTool {
    pub call_id: String,
    pub label: ToolLabel,
    pub started: Instant,
    pub result: Option<ToolResult>,
}

/// A sub-agent's final report, as shown in the transcript.
pub struct AgentReportView {
    pub status: String,
    pub summary: String,
    pub duration: Duration,
    pub steps: u32,
    pub tool_calls: u32,
    pub modified: Vec<String>,
}

/// A sub-agent deployed by the Ultra effort tier.
pub struct AgentView {
    pub agent_id: String,
    pub name: String,
    pub mode: String,
    pub objective: String,
    pub started: Instant,
    pub tools: Vec<AgentTool>,
    /// What the agent is saying in its current step.
    pub text: String,
    pub report: Option<AgentReportView>,
    /// Prompt and completion tokens so far (the report's once it is done).
    pub tokens: u64,
    /// The agent's own conversation, from its brief on, for the agents view.
    pub log: Transcript,
}

impl AgentView {
    pub fn is_running(&self) -> bool {
        self.report.is_none()
    }

    /// The status icon and its colour.
    pub fn icon(&self) -> (&'static str, ratatui::style::Color) {
        match self.report.as_ref().map(|r| r.status.as_str()) {
            None => (spinner(self.started), THINK),
            Some("completed") => (glyphs().ok, OK),
            Some("blocked") => (glyphs().blocked, WARN),
            Some("failed") => (glyphs().fail, ERR),
            _ => (glyphs().pending, DIM),
        }
    }

    /// `explore · 4 tools · 12.3k tokens · 8s` while running, then
    /// `explore · completed · 3 steps · 1 tool call · 2.4s · 12.3k tokens`.
    pub fn stats(&self) -> Vec<Span<'static>> {
        let tokens = if self.tokens > 0 { format!(" · {} tokens", fmt_tokens(self.tokens)) } else { String::new() };
        match &self.report {
            None => vec![styled(format!(" · {}", self.mode), fg(DIM)), styled(format!(" · {}{tokens} · {}", plural(self.tools.len() as u32, "tool", "tools"), fmt_secs(self.started.elapsed())), fg(DIM))],
            Some(r) => vec![
                styled(format!(" · {}", self.mode), fg(DIM)),
                styled(format!(" · {}", r.status), fg(self.icon().1)),
                styled(format!(" · {} · {} · {}{tokens}", plural(r.steps, "step", "steps"), plural(r.tool_calls, "tool call", "tool calls"), fmt_duration(r.duration)), fg(DIM)),
            ],
        }
    }

    /// One line on what the agent is doing (its latest text or tool), or
    /// the first line of its report once it is done.
    pub fn activity(&self) -> String {
        let first_line = |s: &str| s.trim().lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
        match &self.report {
            Some(r) => first_line(&r.summary),
            None => {
                let text = self.text.trim().lines().last().unwrap_or("").trim().to_string();
                match self.tools.last() {
                    Some(t) if t.result.is_none() || text.is_empty() => t.label.plain(),
                    _ if !text.is_empty() => text,
                    _ => first_line(&self.objective),
                }
            }
        }
    }
}

pub enum Entry {
    Notice(String),
    Warning(String),
    Error(String),
    User(String),
    Agent(String),
    Thinking { text: String, started: Instant, ended: Option<Duration> },
    Tool { call_id: String, label: ToolLabel, started: Instant, result: Option<ToolResult> },
    Validation { command: String, passed: bool, summary: String },
    /// Sub-agents deployed together (one `spawn_agents` call).
    Agents(Vec<AgentView>),
    Outcome(OutcomeView),
}

#[derive(Default)]
pub struct Transcript {
    pub entries: Vec<Entry>,
    /// The agent entry the current model response streams into.
    streaming: Option<usize>,
    /// The thinking row of the model request in flight.
    thinking: Option<usize>,
}

impl Transcript {
    pub fn push(&mut self, entry: Entry) {
        self.entries.push(entry);
    }

    pub fn model_request_started(&mut self) {
        self.end_thinking();
        self.streaming = None;
        self.entries.push(Entry::Thinking { text: String::new(), started: Instant::now(), ended: None });
        self.thinking = Some(self.entries.len() - 1);
    }

    pub fn reasoning_delta(&mut self, delta: &str) {
        if let Some(Entry::Thinking { text, .. }) = self.thinking.and_then(|i| self.entries.get_mut(i)) {
            text.push_str(delta);
        }
    }

    pub fn text_delta(&mut self, delta: &str) {
        self.end_thinking();
        match self.streaming.and_then(|i| self.entries.get_mut(i)) {
            Some(Entry::Agent(text)) => text.push_str(delta),
            _ => {
                self.entries.push(Entry::Agent(delta.to_string()));
                self.streaming = Some(self.entries.len() - 1);
            }
        }
    }

    /// The final text is authoritative: a retried request may have
    /// superseded whatever was streamed.
    pub fn model_response(&mut self, final_text: Option<String>) {
        self.end_thinking();
        if let Some(t) = final_text.filter(|t| !t.trim().is_empty()) {
            match self.streaming.and_then(|i| self.entries.get_mut(i)) {
                Some(Entry::Agent(text)) => *text = t,
                _ => self.entries.push(Entry::Agent(t)),
            }
        }
        self.streaming = None;
    }

    pub fn tool_started(&mut self, call_id: String, name: String, arguments: &Value) {
        self.end_thinking();
        self.streaming = None;
        let label = ToolLabel::new(&name, arguments);
        self.entries.push(Entry::Tool { call_id, label, started: Instant::now(), result: None });
    }

    pub fn tool_finished(&mut self, id: &str, ok: bool, summary: String, duration_ms: u64) {
        for entry in self.entries.iter_mut().rev() {
            if let Entry::Tool { call_id, result, .. } = entry {
                if call_id == id {
                    *result = Some(ToolResult { ok, summary, duration: Duration::from_millis(duration_ms) });
                    return;
                }
            }
        }
    }

    /// A sub-agent was deployed: it joins the open agent group (agents
    /// deployed together while one is still running), or starts one.
    pub fn agent_started(&mut self, agent_id: String, name: String, mode: String, objective: String) {
        self.end_thinking();
        self.streaming = None;
        let mut log = Transcript::default();
        log.push(Entry::User(objective.clone()));
        let view = AgentView { agent_id, name, mode, objective, started: Instant::now(), tools: Vec::new(), text: String::new(), report: None, tokens: 0, log };
        if let Some(Entry::Agents(agents)) = self.entries.last_mut() {
            if agents.iter().any(AgentView::is_running) {
                agents.push(view);
                return;
            }
        }
        self.entries.push(Entry::Agents(vec![view]));
    }

    /// Every sub-agent in the conversation, in the order they were deployed.
    pub fn agents(&self) -> impl Iterator<Item = &AgentView> {
        self.entries.iter().flat_map(|e| match e {
            Entry::Agents(agents) => agents.as_slice(),
            _ => &[],
        })
    }

    fn agent_mut(&mut self, agent_id: &str) -> Option<&mut AgentView> {
        self.entries.iter_mut().rev().find_map(|e| match e {
            Entry::Agents(agents) => agents.iter_mut().find(|a| a.agent_id == agent_id),
            _ => None,
        })
    }

    /// One of a sub-agent's own events: its tool rows and current text,
    /// and its own conversation.
    pub fn agent_event(&mut self, agent_id: &str, event: &AgentEvent) {
        let Some(agent) = self.agent_mut(agent_id) else { return };
        agent.log.apply_agent_event(event);
        match event {
            AgentEvent::Usage { cumulative, .. } => agent.tokens = tokens(cumulative),
            AgentEvent::ModelRequestStarted { .. } => agent.text.clear(),
            AgentEvent::ModelTextDelta { text, .. } => agent.text.push_str(text),
            AgentEvent::ModelResponse { text: Some(text), .. } if !text.trim().is_empty() => agent.text = text.clone(),
            AgentEvent::ToolCallStarted { call_id, name, arguments, .. } => {
                agent.tools.push(AgentTool { call_id: call_id.clone(), label: ToolLabel::new(name, arguments), started: Instant::now(), result: None });
            }
            AgentEvent::ToolCallFinished { call_id, success, summary, duration_ms, .. } => {
                if let Some(t) = agent.tools.iter_mut().rev().find(|t| &t.call_id == call_id && t.result.is_none()) {
                    t.result = Some(ToolResult { ok: *success, summary: summary.clone(), duration: Duration::from_millis(*duration_ms) });
                }
            }
            _ => {}
        }
    }

    pub fn agent_finished(&mut self, agent_id: &str, report: &AgentReport) {
        let Some(agent) = self.agent_mut(agent_id) else { return };
        for t in &mut agent.tools {
            if t.result.is_none() {
                t.result = Some(ToolResult { ok: false, summary: "interrupted".into(), duration: t.started.elapsed() });
            }
        }
        agent.report = Some(AgentReportView { status: report.status.as_str().to_string(), summary: report.summary.clone(), duration: Duration::from_millis(report.duration_ms), steps: report.steps, tool_calls: report.tool_calls, modified: report.modified_files.clone() });
        agent.tokens = agent.tokens.max(tokens(&report.usage));
        agent.log.interrupt_running();
        // An agent that never got to run (or was cancelled) has no outcome of its own.
        if !agent.log.entries.iter().any(|e| matches!(e, Entry::Outcome(_))) && !report.summary.trim().is_empty() {
            agent.log.push(Entry::Error(format!("{}: {}", report.status.as_str(), report.summary.trim())));
        }
    }

    /// A sub-agent's own event laid out as a conversation of its own.
    fn apply_agent_event(&mut self, event: &AgentEvent) {
        match event {
            AgentEvent::ModelRequestStarted { .. } => self.model_request_started(),
            AgentEvent::ModelReasoningDelta { text, .. } => self.reasoning_delta(text),
            AgentEvent::ModelTextDelta { text, .. } => self.text_delta(text),
            AgentEvent::ModelResponse { text, .. } => self.model_response(text.clone()),
            AgentEvent::ToolCallStarted { call_id, name, arguments, .. } => self.tool_started(call_id.clone(), name.clone(), arguments),
            AgentEvent::ToolCallFinished { call_id, success, summary, duration_ms, .. } => self.tool_finished(call_id, *success, summary.clone(), *duration_ms),
            AgentEvent::ValidationFinished { record, .. } => self.push(Entry::Validation { command: record.command.clone(), passed: record.passed, summary: record.summary.clone() }),
            AgentEvent::TaskCompleted { outcome, .. } => self.task_finished(outcome),
            _ => {}
        }
    }

    /// Ends the thinking row and marks tools and agents still running as
    /// interrupted (the task they belong to is over).
    pub fn interrupt_running(&mut self) {
        self.end_thinking();
        self.streaming = None;
        for entry in &mut self.entries {
            if let Entry::Tool { started, result: result @ None, .. } = entry {
                *result = Some(ToolResult { ok: false, summary: "interrupted".into(), duration: started.elapsed() });
            }
            if let Entry::Agents(agents) = entry {
                for a in agents.iter_mut().filter(|a| a.is_running()) {
                    a.report = Some(AgentReportView { status: "interrupted".into(), summary: String::new(), duration: a.started.elapsed(), steps: 0, tool_calls: a.tools.len() as u32, modified: Vec::new() });
                    a.log.interrupt_running();
                }
            }
        }
    }

    pub fn task_finished(&mut self, outcome: &TaskOutcome) {
        self.interrupt_running();
        let summary = outcome.summary.trim();
        let repeats_agent = self.last_agent_text().is_some_and(|t| normalize(t).contains(&normalize(summary)));
        let mut stats = vec![plural(outcome.steps, "step", "steps"), plural(outcome.tool_calls, "tool call", "tool calls"), fmt_duration(Duration::from_millis(outcome.duration_ms))];
        let tokens = outcome.usage.prompt_tokens + outcome.usage.completion_tokens;
        if tokens > 0 {
            stats.push(format!("{} tokens", fmt_tokens(tokens)));
        }
        if let Some(cost) = outcome.cost_usd {
            stats.push(format!("${cost:.4}"));
        }
        self.entries.push(Entry::Outcome(OutcomeView {
            status: outcome.status.as_str().to_string(),
            stats,
            summary: (!summary.is_empty() && !repeats_agent).then(|| summary.to_string()),
            modified: outcome.modified_files.clone(),
            unresolved: outcome.unresolved.clone(),
        }));
    }

    fn end_thinking(&mut self) {
        if let Some(Entry::Thinking { started, ended, .. }) = self.thinking.take().and_then(|i| self.entries.get_mut(i)) {
            *ended = Some(started.elapsed());
        }
    }

    fn last_agent_text(&self) -> Option<&str> {
        self.entries.iter().rev().find_map(|e| match e {
            Entry::Agent(t) => Some(t.as_str()),
            _ => None,
        })
    }

    /// Lay the whole conversation out for a viewport `width` columns wide.
    pub fn render(&self, width: usize, show_thinking: bool) -> Vec<Line<'static>> {
        let width = width.max(20);
        let mut out: Vec<Line<'static>> = Vec::new();
        let mut prev_tool = false;
        for entry in &self.entries {
            let lines = render_entry(entry, width, show_thinking);
            if lines.is_empty() {
                continue;
            }
            let is_tool = matches!(entry, Entry::Tool { .. });
            // Consecutive tool rows read as one block.
            if !out.is_empty() && !(prev_tool && is_tool) {
                out.push(Line::raw(""));
            }
            out.extend(lines);
            prev_tool = is_tool;
        }
        out
    }
}

fn tokens(usage: &TokenUsage) -> u64 {
    usage.prompt_tokens + usage.completion_tokens
}

fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// How a tool call's main object is shortened to fit one row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectKind {
    /// Keep the end (the file name): `…/scripts/recover.mjs`.
    Path,
    /// Keep the start: `npm run check -- --wat…`.
    Text,
    /// Like `Text`, styled as code.
    Command,
}

/// A tool call as a short English phrase: `Read src/main.rs (lines 10–40)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolLabel {
    pub verb: String,
    pub object: String,
    pub kind: ObjectKind,
    /// Qualifiers dropped first when the row is too narrow.
    pub extra: String,
}

impl ToolLabel {
    pub fn new(tool: &str, args: &Value) -> Self {
        let str_arg = |key: &str| args.get(key).and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
        let num_arg = |key: &str| args.get(key).and_then(|v| v.as_u64());
        let within = |key: &str| str_arg(key).map(|p| format!(" in {p}")).unwrap_or_default();
        let quoted = |s: String| format!("\"{s}\"");
        let label = |verb: &str, object: Option<String>, kind: ObjectKind, extra: String| ToolLabel { verb: verb.into(), object: object.unwrap_or_default(), kind, extra };
        match tool {
            "read_file" => {
                let lines = match (num_arg("start_line"), num_arg("end_line")) {
                    (Some(a), Some(b)) => format!(" (lines {a}–{b})"),
                    (Some(a), None) => format!(" (from line {a})"),
                    (None, Some(b)) => format!(" (to line {b})"),
                    (None, None) => String::new(),
                };
                label("Read", str_arg("path"), ObjectKind::Path, lines)
            }
            "write_file" => label("Write", str_arg("path"), ObjectKind::Path, String::new()),
            "apply_patch" => {
                let edits = args.get("edits").and_then(|e| e.as_array()).map(|e| e.len()).filter(|n| *n > 1).map(|n| format!(" ({n} edits)")).unwrap_or_default();
                label("Edit", str_arg("path"), ObjectKind::Path, edits)
            }
            "list_files" => match (str_arg("pattern"), str_arg("directory")) {
                (Some(p), _) => label("List", Some(p), ObjectKind::Text, within("directory")),
                (None, Some(d)) => label("List files in", Some(d), ObjectKind::Path, String::new()),
                (None, None) => label("List files", None, ObjectKind::Text, String::new()),
            },
            "search_code" => label("Search for", str_arg("query").map(quoted), ObjectKind::Text, within("path_prefix")),
            "find_symbol" => {
                let what = str_arg("kind").map(|k| format!("{} ", k.replace('_', " "))).unwrap_or_default();
                label(&format!("Find {what}definition of").replace("  ", " "), str_arg("name"), ObjectKind::Text, String::new())
            }
            "find_references" => label("Find references to", str_arg("name"), ObjectKind::Text, within("path_prefix")),
            "execute_command" => label("Run", str_arg("command"), ObjectKind::Command, within("cwd")),
            "run_validation" => match str_arg("command") {
                Some(c) => label("Validate with", Some(c), ObjectKind::Command, String::new()),
                None => {
                    let verb = match str_arg("kind").as_deref() {
                        Some("test") => "Run tests",
                        Some("build") => "Build the project",
                        Some("typecheck") => "Type-check the project",
                        Some("lint") => "Lint the project",
                        _ => "Validate the project",
                    };
                    label(verb, None, ObjectKind::Text, String::new())
                }
            },
            "git_status" => label("Check git status", None, ObjectKind::Text, String::new()),
            "git_diff" => {
                let verb = if args.get("staged").and_then(|v| v.as_bool()).unwrap_or(false) { "Show staged changes" } else { "Show changes" };
                match str_arg("path") {
                    Some(p) => label(&format!("{verb} in"), Some(p), ObjectKind::Path, String::new()),
                    None => label(verb, None, ObjectKind::Text, String::new()),
                }
            }
            "retrieve_memory" => match (str_arg("query"), str_arg("task_id"), str_arg("category")) {
                (Some(q), _, _) => label("Recall notes about", Some(quoted(q)), ObjectKind::Text, String::new()),
                (None, Some(t), _) => label("Recall task", Some(t), ObjectKind::Text, String::new()),
                (None, None, Some(c)) => label("Recall", Some(c.replace('_', " ")), ObjectKind::Text, String::new()),
                (None, None, None) => label("Recall recent work", None, ObjectKind::Text, String::new()),
            },
            "inspect_context" => label("Inspect context", str_arg("what").map(|w| format!("({w})")), ObjectKind::Text, String::new()),
            "web_search" => label("Search the web for", str_arg("query").map(quoted), ObjectKind::Text, String::new()),
            "web_fetch" => label("Fetch", str_arg("url"), ObjectKind::Text, args.get("offset").and_then(|o| o.as_u64()).filter(|o| *o > 0).map(|o| format!(" (from character {o})")).unwrap_or_default()),
            "spawn_agents" => {
                let agents = args.get("agents").and_then(|a| a.as_array()).cloned().unwrap_or_default();
                let names: Vec<String> = agents.iter().filter_map(|a| a.get("name").and_then(|n| n.as_str()).map(str::to_string)).collect();
                let background = if args.get("background").and_then(|b| b.as_bool()).unwrap_or(false) { " in the background" } else { "" };
                label(&format!("Deploy {}", plural(agents.len() as u32, "agent", "agents")), (!names.is_empty()).then(|| names.join(", ")), ObjectKind::Text, background.into())
            }
            "send_input" => {
                let interrupt = if args.get("interrupt").and_then(|b| b.as_bool()).unwrap_or(false) { " (interrupting)" } else { "" };
                label("Message agent", str_arg("id"), ObjectKind::Text, interrupt.into())
            }
            "wait_agents" => {
                let ids: Vec<String> = args.get("ids").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|i| i.as_str().map(str::to_string)).collect()).unwrap_or_default();
                label("Wait for", Some(if ids.is_empty() { "agents".into() } else { ids.join(", ") }), ObjectKind::Text, String::new())
            }
            "read_agent" => match str_arg("id") {
                Some(id) => label("Check on agent", Some(id), ObjectKind::Text, String::new()),
                None => label("List agents", None, ObjectKind::Text, String::new()),
            },
            "close_agent" => label("Stop agent", str_arg("id"), ObjectKind::Text, String::new()),
            other => {
                let mut verb = other.replace('_', " ");
                if let Some(c) = verb.get(0..1) {
                    verb = c.to_uppercase() + &verb[1..];
                }
                let object = match args {
                    Value::Object(m) if m.is_empty() => None,
                    Value::Null => None,
                    v => Some(v.to_string()),
                };
                label(&verb, object, ObjectKind::Text, String::new())
            }
        }
    }

    /// The whole label as plain text: `Read src/auth.rs (lines 1–40)`.
    pub fn plain(&self) -> String {
        let object = if self.object.is_empty() { String::new() } else { format!(" {}", self.object) };
        format!("{}{object}{}", self.verb, self.extra)
    }

    /// One row at most `width` columns wide: the verb in bold, the object
    /// shortened to fit, then whatever qualifiers still fit.
    fn spans(&self, width: usize, suffix: &str) -> Vec<Span<'static>> {
        let verb_w = self.verb.width();
        let mut spans = vec![styled(self.verb.clone(), Style::default().add_modifier(Modifier::BOLD))];
        let mut room = width.saturating_sub(verb_w + suffix.width());
        let mut extra = self.extra.as_str();
        if !self.object.is_empty() {
            room = room.saturating_sub(1);
            if self.object.width() + extra.width() > room {
                extra = "";
            }
            let budget = room.saturating_sub(extra.width());
            let object = match self.kind {
                ObjectKind::Path => truncate_start(&self.object, budget),
                ObjectKind::Text | ObjectKind::Command => truncate_end(&self.object, budget),
            };
            let style = if self.kind == ObjectKind::Command { fg(CODE) } else { Style::default() };
            spans.push(Span::raw(" "));
            spans.push(styled(object, style));
        } else if extra.width() > room {
            extra = "";
        }
        if !extra.is_empty() {
            spans.push(styled(extra.to_string(), fg(DIM)));
        }
        spans
    }
}

/// Shorten from the front, preferring to cut at a `/`, so the end stays.
fn truncate_start(s: &str, max: usize) -> String {
    if s.width() <= max {
        return s.to_string();
    }
    if max <= 1 {
        return "…".chars().take(max).collect();
    }
    let mut tail = String::new();
    let mut w = 0;
    for ch in s.chars().rev() {
        let cw = ch.width().unwrap_or(0);
        if w + cw > max - 1 {
            break;
        }
        tail.insert(0, ch);
        w += cw;
    }
    match tail.find('/') {
        Some(i) if i > 0 && tail.len() - i > 8 => format!("…{}", &tail[i..]),
        _ => format!("…{tail}"),
    }
}

/// Shorten from the end with a trailing ellipsis.
pub fn truncate_end(s: &str, max: usize) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.width() <= max {
        return s;
    }
    if max <= 1 {
        return "…".chars().take(max).collect();
    }
    let mut head = String::new();
    let mut w = 0;
    for ch in s.chars() {
        let cw = ch.width().unwrap_or(0);
        if w + cw > max - 1 {
            break;
        }
        head.push(ch);
        w += cw;
    }
    head.push('…');
    head
}

fn styled(text: impl Into<String>, style: Style) -> Span<'static> {
    Span::styled(text.into(), style)
}

fn fg(color: ratatui::style::Color) -> Style {
    Style::default().fg(color)
}

fn indent(n: usize) -> Vec<Span<'static>> {
    vec![Span::raw(" ".repeat(n))]
}

fn render_entry(entry: &Entry, width: usize, show_thinking: bool) -> Vec<Line<'static>> {
    match entry {
        Entry::Notice(text) => text.lines().flat_map(|l| wrap(vec![styled(l, fg(DIM))], width, indent(2), indent(2))).collect(),
        Entry::Warning(text) => wrap(vec![styled(text.clone(), fg(WARN))], width, vec![styled("! ", fg(WARN).add_modifier(Modifier::BOLD))], indent(2)),
        Entry::Error(text) => wrap(vec![styled(text.clone(), fg(ERR))], width, vec![styled(format!("{} ", glyphs().fail), fg(ERR))], indent(2)),
        Entry::User(text) => {
            let bold = Style::default().add_modifier(Modifier::BOLD);
            let mut out = Vec::new();
            for (i, l) in text.lines().enumerate() {
                let first = if i == 0 { vec![styled("› ", fg(ACCENT).add_modifier(Modifier::BOLD))] } else { indent(2) };
                out.extend(wrap(vec![styled(l, bold)], width, first, indent(2)));
            }
            out
        }
        Entry::Agent(text) => markdown(text, width, vec![styled("● ", fg(AGENT))], indent(2)),
        Entry::Thinking { text, started, ended } => render_thinking(text, *started, *ended, width, show_thinking),
        Entry::Tool { label, started, result, .. } => {
            let (icon, icon_style) = match result {
                None => (spinner(*started), fg(THINK)),
                Some(r) if r.ok => (glyphs().ok, fg(OK)),
                Some(_) => (glyphs().fail, fg(ERR)),
            };
            let elapsed = if result.is_none() && started.elapsed() >= Duration::from_secs(1) { format!(" · {}", fmt_secs(started.elapsed())) } else { String::new() };
            let mut row = vec![styled(format!("{icon} "), icon_style)];
            row.extend(label.spans(width.saturating_sub(2), &elapsed));
            if !elapsed.is_empty() {
                row.push(styled(elapsed, fg(DIM)));
            }
            let mut out = vec![Line::from(row)];
            if let Some(r) = result {
                let took = fmt_duration(r.duration);
                let first_line = r.summary.lines().next().unwrap_or("").trim();
                let mut row = vec![styled("  └ ", fg(FAINT))];
                if !first_line.is_empty() {
                    let style = if r.ok { fg(DIM) } else { fg(ERR) };
                    row.push(styled(truncate_end(first_line, width.saturating_sub(4 + 3 + took.width())), style));
                    row.push(styled(" · ", fg(FAINT)));
                }
                row.push(styled(took, fg(DIM)));
                out.push(Line::from(row));
            }
            out
        }
        Entry::Agents(agents) => render_agents(agents, width, show_thinking),
        Entry::Validation { command, passed, summary } => {
            let (icon, color, verdict) = if *passed { (glyphs().ok, OK, "passed") } else { (glyphs().fail, ERR, "failed") };
            let mut out = wrap(
                vec![styled("validation ", Style::default().add_modifier(Modifier::BOLD)), styled(command.clone(), fg(CODE)), styled(format!(" {verdict}"), fg(color))],
                width,
                vec![styled(format!("{icon} "), fg(color))],
                indent(2),
            );
            if !summary.trim().is_empty() {
                let mut rows = wrap(vec![styled(summary.trim().to_string(), fg(DIM))], width, vec![styled("  └ ", fg(FAINT))], indent(4));
                rows.truncate(3);
                out.extend(rows);
            }
            out
        }
        Entry::Outcome(o) => render_outcome(o, width),
    }
}

fn render_thinking(text: &str, started: Instant, ended: Option<Duration>, width: usize, show_thinking: bool) -> Vec<Line<'static>> {
    let body_style = fg(DIM).add_modifier(Modifier::ITALIC);
    let gutter = || vec![styled("  ▏ ", fg(FAINT))];
    let body = |text: &str| -> Vec<Line<'static>> { text.trim().lines().filter(|l| !l.trim().is_empty()).flat_map(|l| wrap(vec![styled(l, body_style)], width, gutter(), gutter())).collect() };
    match ended {
        None => {
            let mut out = vec![Line::from(vec![
                styled(format!("{} ", spinner(started)), fg(THINK)),
                styled("Thinking…", fg(THINK)),
                styled(format!(" {}", fmt_secs(started.elapsed())), fg(DIM)),
            ])];
            let rows = body(text);
            let keep = if show_thinking { rows.len() } else { 3 };
            out.extend(rows.into_iter().rev().take(keep).collect::<Vec<_>>().into_iter().rev());
            out
        }
        // Without reasoning text the wait was plain latency; nothing to show.
        Some(_) if text.trim().is_empty() => Vec::new(),
        Some(d) => {
            let mut out = vec![Line::from(vec![
                styled("✻ ", fg(THINK)),
                styled(format!("Thought for {}", fmt_secs(d.max(Duration::from_secs(1)))), fg(DIM)),
                styled(if show_thinking { "" } else { "  ctrl-o to expand" }, fg(DIM)),
            ])];
            if show_thinking {
                out.extend(body(text));
            }
            out
        }
    }
}

/// Sub-agents: a header with the running count, then one block per agent
/// (status, name, mode, tool count, time) with its latest tool rows while
/// it runs and its report once it is done. Ctrl-O shows every tool row and
/// the whole report.
fn render_agents(agents: &[AgentView], width: usize, show_thinking: bool) -> Vec<Line<'static>> {
    let running = agents.iter().filter(|a| a.is_running()).count();
    let earliest = agents.iter().map(|a| a.started).min().unwrap_or_else(Instant::now);
    let elapsed = if running > 0 { earliest.elapsed() } else { agents.iter().map(|a| a.started + a.report.as_ref().map(|r| r.duration).unwrap_or_default()).max().map(|end| end.saturating_duration_since(earliest)).unwrap_or_default() };
    let mut head = vec![styled(plural(agents.len() as u32, "agent", "agents"), Style::default().add_modifier(Modifier::BOLD))];
    head.push(styled(
        if running > 0 { format!("  {running} running · {}", fmt_secs(elapsed)) } else { format!("  done · {}", fmt_secs(elapsed)) },
        fg(DIM),
    ));
    let mut out = wrap(head, width, vec![styled(format!("{} ", glyphs().agents), fg(THINK))], indent(2));
    for a in agents {
        let (icon, color) = a.icon();
        let mut row = vec![styled(a.name.clone(), Style::default().add_modifier(Modifier::BOLD))];
        row.extend(a.stats());
        out.extend(wrap(row, width, vec![Span::raw("  "), styled(format!("{icon} "), fg(color))], indent(4)));
        if show_thinking {
            let brief = a.objective.split_whitespace().collect::<Vec<_>>().join(" ");
            if !brief.is_empty() {
                out.extend(wrap(vec![styled("brief: ", fg(FAINT)), styled(brief, fg(DIM))], width, indent(6), indent(13)));
            }
        }
        if a.is_running() {
            let keep = if show_thinking { a.tools.len() } else { 3 };
            for t in a.tools.iter().rev().take(keep).collect::<Vec<_>>().into_iter().rev() {
                let (ti, ts) = match &t.result {
                    None => (spinner(t.started), fg(THINK)),
                    Some(r) if r.ok => (glyphs().ok, fg(OK)),
                    Some(_) => (glyphs().fail, fg(ERR)),
                };
                let mut row = vec![Span::raw("      "), styled(format!("{ti} "), ts)];
                row.extend(t.label.spans(width.saturating_sub(8), ""));
                out.push(Line::from(row));
            }
            let latest = a.text.trim().lines().last().unwrap_or("").trim();
            if !latest.is_empty() {
                out.push(Line::from(vec![Span::raw("      "), styled("▏ ", fg(FAINT)), styled(truncate_end(latest, width.saturating_sub(8)), fg(DIM).add_modifier(Modifier::ITALIC))]));
            }
        } else if let Some(r) = &a.report {
            if show_thinking {
                for t in &a.tools {
                    let (ti, ts) = match &t.result {
                        Some(res) if res.ok => (glyphs().ok, fg(OK)),
                        _ => (glyphs().fail, fg(ERR)),
                    };
                    let mut row = vec![Span::raw("      "), styled(format!("{ti} "), ts)];
                    row.extend(t.label.spans(width.saturating_sub(8), ""));
                    out.push(Line::from(row));
                }
            }
            let summary = r.summary.trim();
            if !summary.is_empty() {
                let mut rows = markdown(summary, width, indent(6), indent(6));
                if !show_thinking {
                    rows.truncate(2);
                }
                out.extend(rows);
            }
            if !r.modified.is_empty() {
                out.extend(wrap(vec![styled("modified  ", fg(DIM)), styled(r.modified.join(", "), fg(CODE))], width, indent(6), indent(16)));
            }
        }
    }
    let hint = if !show_thinking && agents.iter().any(|a| !a.is_running() || a.tools.len() > 3) { "ctrl-o expands · /agents opens each agent's full transcript" } else { "/agents opens each agent's full transcript" };
    out.push(Line::from(vec![Span::raw("  "), styled(hint, fg(FAINT))]));
    out
}

fn render_outcome(o: &OutcomeView, width: usize) -> Vec<Line<'static>> {
    let (icon, color) = match o.status.as_str() {
        "completed" => (glyphs().ok, OK),
        "blocked" => (glyphs().blocked, WARN),
        "failed" => (glyphs().fail, ERR),
        _ => (glyphs().pending, DIM),
    };
    let mut head = vec![styled(o.status.clone(), fg(color).add_modifier(Modifier::BOLD))];
    if !o.stats.is_empty() {
        head.push(styled(format!("  {}", o.stats.join(" · ")), fg(DIM)));
    }
    let mut out = wrap(head, width, vec![styled(format!("{icon} "), fg(color))], indent(2));
    if let Some(summary) = &o.summary {
        out.extend(markdown(summary, width, indent(2), indent(2)));
    }
    if !o.modified.is_empty() {
        out.extend(wrap(vec![styled("modified  ", fg(DIM)), styled(o.modified.join(", "), fg(CODE))], width, indent(2), indent(12)));
    }
    if !o.unresolved.is_empty() {
        out.extend(wrap(vec![styled("unresolved  ", fg(WARN)), styled(o.unresolved.join("; "), Style::default())], width, indent(2), indent(14)));
    }
    out.push(Line::styled("─".repeat(width), fg(FAINT)));
    out
}

/// Render light markdown: headings, bullets, numbered lists, quotes, rules,
/// fenced code, and inline `code` / **bold**. The first row starts with
/// `first`; every other row is indented by `rest` (plus any list indent).
pub fn markdown(text: &str, width: usize, first: Vec<Span<'static>>, rest: Vec<Span<'static>>) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut in_code = false;
    let mut prefix_used = false;
    let mut next_prefix = |out_len: usize| -> Vec<Span<'static>> {
        if !prefix_used && out_len == 0 {
            prefix_used = true;
            first.clone()
        } else {
            rest.clone()
        }
    };
    let mut last_blank = true;
    for raw in text.trim_end().lines() {
        let line = raw.replace('\t', "    ");
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            let mut p = next_prefix(out.len());
            p.push(styled("▏ ", fg(FAINT)));
            let mut cont = rest.clone();
            cont.push(styled("▏ ", fg(FAINT)));
            out.extend(wrap(vec![styled(line.clone(), fg(CODE))], width, p, cont));
            last_blank = false;
            continue;
        }
        if trimmed.is_empty() {
            if !last_blank && !out.is_empty() {
                out.push(Line::raw(""));
            }
            last_blank = true;
            continue;
        }
        last_blank = false;
        let lead = line.len() - trimmed.len();
        let mut p = next_prefix(out.len());
        let mut cont = rest.clone();
        if let Some(h) = heading(trimmed) {
            out.extend(wrap(inline(h, Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)), width, p, cont));
        } else if is_rule(trimmed) {
            let used: usize = p.iter().map(|s| s.width()).sum();
            p.push(styled("─".repeat(width.saturating_sub(used).min(40)), fg(FAINT)));
            out.push(Line::from(p));
        } else if let Some(quote) = trimmed.strip_prefix('>') {
            p.push(styled("▎ ", fg(FAINT)));
            cont.push(styled("▎ ", fg(FAINT)));
            out.extend(wrap(inline(quote.trim_start(), fg(DIM).add_modifier(Modifier::ITALIC)), width, p, cont));
        } else if let Some((marker, body)) = list_item(trimmed) {
            p.push(Span::raw(" ".repeat(lead)));
            p.push(styled(marker.clone(), fg(DIM)));
            cont.push(Span::raw(" ".repeat(lead + marker.width())));
            out.extend(wrap(inline(body, Style::default()), width, p, cont));
        } else {
            p.push(Span::raw(" ".repeat(lead)));
            cont.push(Span::raw(" ".repeat(lead)));
            out.extend(wrap(inline(trimmed, Style::default()), width, p, cont));
        }
    }
    while out.last().is_some_and(|l| l.width() == 0) {
        out.pop();
    }
    out
}

fn heading(line: &str) -> Option<&str> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    (1..=6).contains(&hashes).then(|| &line[hashes..]).filter(|rest| rest.starts_with(' ')).map(str::trim)
}

fn is_rule(line: &str) -> bool {
    let t = line.trim();
    t.len() >= 3 && (t.chars().all(|c| c == '-') || t.chars().all(|c| c == '*') || t.chars().all(|c| c == '_'))
}

/// `- item`, `* item`, `+ item` or `12. item`: the rendered marker and body.
fn list_item(line: &str) -> Option<(String, &str)> {
    for bullet in ["- ", "* ", "+ "] {
        if let Some(body) = line.strip_prefix(bullet) {
            return Some(("• ".to_string(), body));
        }
    }
    let digits = line.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 && digits < 4 {
        if let Some(body) = line[digits..].strip_prefix(". ") {
            return Some((format!("{}. ", &line[..digits]), body));
        }
    }
    None
}

/// Inline `code` and **bold** spans.
pub fn inline(text: &str, base: Style) -> Vec<Span<'static>> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut bold = false;
    let mut rest = text;
    let style = |bold: bool| if bold { base.add_modifier(Modifier::BOLD) } else { base };
    while let Some(c) = rest.chars().next() {
        if c == '`' {
            if let Some(end) = rest[1..].find('`') {
                if !buf.is_empty() {
                    out.push(styled(std::mem::take(&mut buf), style(bold)));
                }
                out.push(styled(&rest[1..1 + end], fg(CODE)));
                rest = &rest[end + 2..];
                continue;
            }
        }
        if rest.starts_with("**") && (bold || rest[2..].contains("**")) {
            if !buf.is_empty() {
                out.push(styled(std::mem::take(&mut buf), style(bold)));
            }
            bold = !bold;
            rest = &rest[2..];
            continue;
        }
        buf.push(c);
        rest = &rest[c.len_utf8()..];
    }
    if !buf.is_empty() {
        out.push(styled(buf, style(bold)));
    }
    out
}

/// Word-wrap styled spans to `width` columns. The first row starts with
/// `first` and every continuation row with `rest`, so wrapped text keeps
/// its hanging indent. Words wider than a row are broken by character.
pub fn wrap(spans: Vec<Span<'static>>, width: usize, first: Vec<Span<'static>>, rest: Vec<Span<'static>>) -> Vec<Line<'static>> {
    let mut rows = Rows::new(width, first, rest);
    for span in spans {
        let mut word = String::new();
        for ch in span.content.chars() {
            if word.starts_with(' ') != (ch == ' ') && !word.is_empty() {
                rows.token(std::mem::take(&mut word), span.style);
            }
            word.push(ch);
        }
        if !word.is_empty() {
            rows.token(word, span.style);
        }
    }
    rows.finish()
}

/// Row builder behind [`wrap`].
struct Rows {
    width: usize,
    rest: Vec<Span<'static>>,
    done: Vec<Line<'static>>,
    row: Vec<Span<'static>>,
    /// Spans of `row` that are prefix, not content.
    prefix_len: usize,
    col: usize,
    content_start: usize,
    wrapped: bool,
}

impl Rows {
    fn new(width: usize, first: Vec<Span<'static>>, rest: Vec<Span<'static>>) -> Self {
        let col = first.iter().map(|s| s.width()).sum();
        Self { width, rest, done: Vec::new(), prefix_len: first.len(), row: first, col, content_start: col, wrapped: false }
    }

    fn newline(&mut self) {
        while self.row.len() > self.prefix_len && self.row.last().is_some_and(|s| s.content.trim().is_empty()) {
            self.row.pop();
        }
        self.done.push(Line::from(std::mem::replace(&mut self.row, self.rest.clone())));
        self.prefix_len = self.rest.len();
        self.col = self.rest.iter().map(|s| s.width()).sum();
        self.content_start = self.col;
        self.wrapped = true;
    }

    fn token(&mut self, text: String, style: Style) {
        let w = text.width();
        if text.starts_with(' ') {
            // Whitespace never starts a wrapped row.
            if self.wrapped && self.col == self.content_start {
                return;
            }
            if self.col + w > self.width {
                self.newline();
            } else {
                self.row.push(Span::styled(text, style));
                self.col += w;
            }
            return;
        }
        if self.col + w > self.width && self.col > self.content_start {
            self.newline();
        }
        if self.col + w <= self.width {
            self.row.push(Span::styled(text, style));
            self.col += w;
            return;
        }
        // Too wide for any row: break it by character.
        let mut piece = String::new();
        for ch in text.chars() {
            let cw = ch.width().unwrap_or(0);
            if self.col + cw > self.width && self.col > self.content_start {
                if !piece.is_empty() {
                    self.row.push(Span::styled(std::mem::take(&mut piece), style));
                }
                self.newline();
            }
            piece.push(ch);
            self.col += cw;
        }
        if !piece.is_empty() {
            self.row.push(Span::styled(piece, style));
        }
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        self.done.push(Line::from(self.row));
        self.done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(lines: &[Line]) -> Vec<String> {
        lines.iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>()).collect()
    }

    #[test]
    fn wrapped_rows_keep_the_hanging_indent_and_fit_the_width() {
        let rows = wrap(vec![Span::raw("Current working tree (I'll preserve these): three untracked paths under new-api/scripts")], 30, vec![Span::raw("● ")], indent(2));
        let t = text(&rows);
        assert!(t.len() > 2, "{t:?}");
        assert!(t[0].starts_with("● Current"));
        for row in &t[1..] {
            assert!(row.starts_with("  ") && !row.starts_with("   "), "{row:?}");
        }
        assert!(rows.iter().all(|r| r.width() <= 30), "{t:?}");
    }

    #[test]
    fn long_words_break_by_character() {
        let rows = wrap(vec![Span::raw("new-api/scripts/recover-astra-channels.test.mjs")], 20, indent(2), indent(2));
        assert!(rows.iter().all(|r| r.width() <= 20));
        assert_eq!(text(&rows).concat().replace(' ', ""), "new-api/scripts/recover-astra-channels.test.mjs");
    }

    #[test]
    fn markdown_lists_code_and_inline_styles() {
        let md = "What would you like?\n\n- Implement or fix something in a fairly long bullet that has to wrap\n- Run `go test ./...`\n\n```\nfn main() {}\n```";
        let t = text(&markdown(md, 40, vec![Span::raw("● ")], indent(2)));
        assert_eq!(t[0], "● What would you like?");
        assert_eq!(t[1], "");
        assert!(t[2].starts_with("  • Implement"));
        assert!(t[3].starts_with("    "), "bullet continuation aligns with the text: {t:?}");
        assert!(t.iter().any(|r| r == "  • Run go test ./..."));
        assert!(t.iter().any(|r| r == "  ▏ fn main() {}"));
        let spans = inline("a **b** `c`", Style::default());
        assert!(spans.iter().any(|s| s.content == "b" && s.style.add_modifier.contains(Modifier::BOLD)));
        assert!(spans.iter().any(|s| s.content == "c" && s.style.fg == Some(CODE)));
        assert_eq!(inline("2 ** 3", Style::default()).len(), 1, "an unmatched ** stays literal");
    }

    #[test]
    fn outcome_does_not_repeat_the_agent_message() {
        let mut t = Transcript::default();
        t.model_request_started();
        t.text_delta("Hi! I'm ready.\n\nWhat would you like me to do?");
        t.model_response(None);
        let outcome = TaskOutcome {
            task_id: ctx_core::TaskId::from_string("t"),
            status: ctx_core::TaskStatus::Completed,
            summary: "Hi! I'm ready. What would you like me to do?".into(),
            modified_files: vec![],
            validation: vec![],
            unresolved: vec![],
            steps: 1,
            model_calls: 1,
            tool_calls: 0,
            usage: ctx_core::TokenUsage::estimated(1200, 300),
            cost_usd: None,
            duration_ms: 2400,
        };
        t.task_finished(&outcome);
        let rows = text(&t.render(80, false));
        assert_eq!(rows.iter().filter(|r| r.contains("What would you like")).count(), 1, "{rows:?}");
        assert!(rows.iter().any(|r| r.starts_with("✓ completed  1 step · 0 tool calls · 2.4s · 1.5k tokens")), "{rows:?}");
        // Thinking without reasoning text leaves no trace once done.
        assert!(!rows.iter().any(|r| r.contains("Thought")));
    }

    #[test]
    fn tools_render_detail_spinner_and_result() {
        let mut t = Transcript::default();
        t.model_request_started();
        t.reasoning_delta("I should read the file first.");
        t.tool_started("c1".into(), "read_file".into(), &serde_json::json!({"path": "src/main.rs"}));
        t.tool_started("c2".into(), "execute_command".into(), &serde_json::json!({"command": "cargo test"}));
        let running = text(&t.render(80, false));
        assert!(running.iter().any(|r| r.ends_with("Read src/main.rs") && SPINNER.iter().any(|s| r.starts_with(s))), "{running:?}");
        t.tool_finished("c1", true, "120 lines".into(), 12);
        t.tool_finished("c2", false, "exit status 101\nmore".into(), 3400);
        let rows = text(&t.render(80, false));
        assert!(rows.iter().any(|r| r.starts_with("✻ Thought for")));
        assert!(rows.contains(&"✓ Read src/main.rs".to_string()), "{rows:?}");
        assert!(rows.contains(&"  └ 120 lines · 12ms".to_string()), "{rows:?}");
        assert!(rows.contains(&"✗ Run cargo test".to_string()), "{rows:?}");
        assert!(rows.contains(&"  └ exit status 101 · 3.4s".to_string()), "{rows:?}");
        let i = rows.iter().position(|r| r.starts_with("✓ Read")).unwrap();
        assert!(rows[i + 2].starts_with("✗ Run"), "tool rows are not separated by blank lines: {rows:?}");
        let expanded = text(&t.render(80, true));
        assert!(expanded.iter().any(|r| r.contains("I should read the file first.")));
    }

    #[test]
    fn agents_render_running_rows_then_reports() {
        let mut t = Transcript::default();
        t.agent_started("agent-1".into(), "auth-explorer".into(), "explore".into(), "Find the login flow".into());
        t.agent_started("agent-2".into(), "api-implementer".into(), "implement".into(), "Add the endpoint".into());
        t.agent_event("agent-1", &AgentEvent::ToolCallStarted { task_id: ctx_core::TaskId::from_string("a"), call_id: "c1".into(), name: "read_file".into(), arguments: serde_json::json!({"path": "src/auth.rs"}) });
        t.agent_event("agent-1", &AgentEvent::ModelTextDelta { task_id: ctx_core::TaskId::from_string("a"), text: "Looking at the login handler".into() });
        let running = text(&t.render(80, false));
        assert!(running.iter().any(|r| r.starts_with("◈ 2 agents  2 running")), "{running:?}");
        assert!(running.iter().any(|r| r.contains("auth-explorer · explore · 1 tool")), "{running:?}");
        assert!(running.iter().any(|r| r.contains("Read src/auth.rs")), "{running:?}");
        assert!(running.iter().any(|r| r.contains("Looking at the login handler")), "{running:?}");
        let report = AgentReport { agent_id: "agent-1".into(), name: "auth-explorer".into(), mode: ctx_core::AgentMode::Explore, status: ctx_core::TaskStatus::Completed, summary: "The login flow lives in `src/auth.rs`.".into(), modified_files: vec![], unresolved: vec![], steps: 3, tool_calls: 1, usage: ctx_core::TokenUsage::estimated(100, 20), cost_usd: None, duration_ms: 2400 };
        t.agent_finished("agent-1", &report);
        let mut second = report.clone();
        second.agent_id = "agent-2".into();
        second.name = "api-implementer".into();
        second.status = ctx_core::TaskStatus::Blocked;
        second.modified_files = vec!["src/api.rs".into()];
        t.agent_finished("agent-2", &second);
        let done = text(&t.render(80, false));
        assert!(done.iter().any(|r| r.starts_with("◈ 2 agents  done")), "{done:?}");
        assert!(done.iter().any(|r| r.starts_with("  ✓ auth-explorer · explore · completed · 3 steps · 1 tool call · 2.4s")), "{done:?}");
        assert!(done.iter().any(|r| r.starts_with("  ◆ api-implementer · implement · blocked")), "{done:?}");
        assert!(done.iter().any(|r| r.contains("The login flow lives in src/auth.rs.")), "{done:?}");
        assert!(done.iter().any(|r| r.contains("modified  src/api.rs")), "{done:?}");
        assert!(!done.iter().any(|r| r.contains("Read src/auth.rs")), "finished agents fold their tool rows away: {done:?}");
        let expanded = text(&t.render(80, true));
        assert!(expanded.iter().any(|r| r.contains("Read src/auth.rs")), "ctrl-o shows them");
        assert!(expanded.iter().any(|r| r.contains("brief: Find the login flow")), "and the briefs: {expanded:?}");
        // A later deployment starts a new group once the first is done.
        t.agent_started("agent-3".into(), "verifier".into(), "explore".into(), "Check".into());
        assert_eq!(t.entries.iter().filter(|e| matches!(e, Entry::Agents(_))).count(), 2);
    }

    fn label(tool: &str, args: Value, width: usize) -> String {
        ToolLabel::new(tool, &args).spans(width, "").iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn tool_calls_read_as_plain_english() {
        assert_eq!(label("read_file", serde_json::json!({"path": "src/main.rs", "start_line": 10, "end_line": 40}), 80), "Read src/main.rs (lines 10–40)");
        assert_eq!(label("apply_patch", serde_json::json!({"path": "src/a.rs", "edits": [{}, {}]}), 80), "Edit src/a.rs (2 edits)");
        assert_eq!(label("write_file", serde_json::json!({"path": "docs/x.md"}), 80), "Write docs/x.md");
        assert_eq!(label("search_code", serde_json::json!({"query": "retryDelay", "path_prefix": "new-api/"}), 80), "Search for \"retryDelay\" in new-api/");
        assert_eq!(label("find_symbol", serde_json::json!({"name": "AuthService", "kind": "class"}), 80), "Find class definition of AuthService");
        assert_eq!(label("find_symbol", serde_json::json!({"name": "login"}), 80), "Find definition of login");
        assert_eq!(label("find_references", serde_json::json!({"name": "login"}), 80), "Find references to login");
        assert_eq!(label("list_files", serde_json::json!({"pattern": "*.go", "directory": "cmd"}), 80), "List *.go in cmd");
        assert_eq!(label("execute_command", serde_json::json!({"command": "go test ./..."}), 80), "Run go test ./...");
        assert_eq!(label("run_validation", serde_json::json!({"kind": "test"}), 80), "Run tests");
        assert_eq!(label("git_status", serde_json::json!({}), 80), "Check git status");
        assert_eq!(label("git_diff", serde_json::json!({"staged": true}), 80), "Show staged changes");
        assert_eq!(label("retrieve_memory", serde_json::json!({"query": "auth flow"}), 80), "Recall notes about \"auth flow\"");
        assert_eq!(label("inspect_context", serde_json::json!({"what": "stats"}), 80), "Inspect context (stats)");
        assert_eq!(label("web_search", serde_json::json!({"query": "tokio select"}), 80), "Search the web for \"tokio select\"");
        assert_eq!(label("web_fetch", serde_json::json!({"url": "https://docs.rs/tokio", "offset": 23400}), 80), "Fetch https://docs.rs/tokio (from character 23400)");
        assert_eq!(label("some_new_tool", serde_json::json!({}), 80), "Some new tool");
        assert_eq!(label("spawn_agents", serde_json::json!({"agents": [{"name": "docs-explorer"}, {"name": "tree-checker"}]}), 80), "Deploy 2 agents docs-explorer, tree-checker");
    }

    #[test]
    fn long_objects_are_shortened_to_one_row() {
        let path = "new-api/scripts/recovery/channels/recover-astra-channels.test.mjs";
        let read = label("read_file", serde_json::json!({"path": path, "start_line": 1, "end_line": 400}), 40);
        assert!(read.width() <= 40, "{read}");
        assert!(read.starts_with("Read …/") && read.ends_with("recover-astra-channels.test.mjs"), "keeps the file name, drops the qualifier: {read}");
        let run = label("execute_command", serde_json::json!({"command": "node --test new-api/scripts/recover-astra-channels.test.mjs --reporter spec"}), 40);
        assert!(run.width() <= 40 && run.starts_with("Run node --test") && run.ends_with('…'), "{run}");
        let mut t = Transcript::default();
        t.tool_started("c".into(), "read_file".into(), &serde_json::json!({"path": path}));
        t.tool_finished("c", true, "a very long summary that would otherwise wrap onto a second row of the transcript".into(), 5);
        let rows = text(&t.render(40, false));
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert!(rows.iter().all(|r| r.width() <= 40), "{rows:?}");
    }
}
