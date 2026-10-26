//! Slash commands for the interactive session, approval modes, and the
//! suggestions shown while a command is being typed.

use ctx_core::{PermissionProfile, ReasoningEffort};

pub struct Command {
    pub name: &'static str,
    pub args: &'static str,
    pub about: &'static str,
}

pub const COMMANDS: &[Command] = &[
    Command { name: "/new", args: "", about: "clear the conversation and start a new session" },
    Command { name: "/resume", args: "[search]", about: "go back to an earlier conversation in this repository" },
    Command { name: "/model", args: "[name]", about: "switch model" },
    Command { name: "/approval", args: "[mode]", about: "change what needs your approval" },
    Command { name: "/effort", args: "[level]", about: "reasoning effort: auto, one of the model's effort values, or ultra (deploys agents)" },
    Command { name: "/agents", args: "", about: "sub-agents deployed in this session, and each one's full transcript" },
    Command { name: "/mcp", args: "", about: "Model Context Protocol servers: their state and the tools they add" },
    Command { name: "/signin", args: "", about: "sign in to ModelGateway and send model requests through it" },
    Command { name: "/signout", args: "", about: "sign out of ModelGateway and restore the previous endpoint" },
    Command { name: "/usage", args: "[period] [filters]", about: "ModelGateway balance, plan and spend by model and project (today, 24h, 7d, 30d, 90d, year, all; model:<name>, project:<name>)" },
    Command { name: "/billing", args: "[command]", about: "ModelGateway plans and credit: topup <amount>, plan <id> [balance], prefer <order>" },
    Command { name: "/help", args: "", about: "list commands and keys" },
];

/// Approval modes: the names people use for permission profiles, in the
/// order Shift-Tab cycles through them (bypass excepted: it is only ever
/// chosen by name).
pub const MODES: &[(&str, PermissionProfile, &str)] = &[
    ("ask", PermissionProfile::ApprovalRequired, "ask before edits and commands that change things"),
    ("auto", PermissionProfile::WorkspaceAutonomous, "edit and run builds and tests freely; ask before risky commands"),
    ("bypass", PermissionProfile::BypassPermissions, "never ask; only denied commands and paths outside the workspace are blocked"),
];

/// The `/effort` choices for a model: `auto` (no value sent, the model
/// decides), the effort values the model accepts as models.dev lists them
/// (`low`, `medium`, `high` when it does not know the model), then `ultra`.
pub fn effort_choices(levels: Option<&[String]>) -> Vec<Option<ReasoningEffort>> {
    let values: Vec<ReasoningEffort> = match levels {
        Some(levels) => levels.iter().filter_map(|v| ReasoningEffort::parse(v)).filter(|e| !e.deploys_agents()).collect(),
        None => ReasoningEffort::STANDARD.iter().map(|v| ReasoningEffort::level(v)).collect(),
    };
    std::iter::once(None).chain(values.into_iter().map(Some)).chain(std::iter::once(Some(ReasoningEffort::Ultra))).collect()
}

pub fn effort_name(effort: Option<&ReasoningEffort>) -> &str {
    effort.map(ReasoningEffort::as_str).unwrap_or("auto")
}

pub fn effort_about(effort: Option<&ReasoningEffort>) -> &'static str {
    match effort.map(ReasoningEffort::as_str) {
        None => "the model decides how hard to think",
        Some("ultra") => "the model's strongest effort, plus sub-agents that explore and implement in parallel",
        Some("none") => "no reasoning",
        Some("minimal") => "barely any reasoning; fastest",
        Some("low") => "quick answers; fewer reasoning tokens",
        Some("medium") => "balanced",
        Some("high") => "think hard before acting",
        Some("xhigh") => "think harder still",
        Some("max") => "the model's maximum",
        Some(_) => "",
    }
}

/// `Some(None)` for `auto`, `Some(Some(effort))` for a value, `None` for junk.
pub fn parse_effort(text: &str) -> Option<Option<ReasoningEffort>> {
    if text.trim().eq_ignore_ascii_case("auto") {
        return Some(None);
    }
    ReasoningEffort::parse(text).map(Some)
}

pub fn mode_name(profile: PermissionProfile) -> &'static str {
    MODES.iter().find(|(_, p, _)| *p == profile).map(|(name, _, _)| *name).unwrap_or("ask")
}

pub fn mode_about(profile: PermissionProfile) -> &'static str {
    MODES.iter().find(|(_, p, _)| *p == profile).map(|(_, _, about)| *about).unwrap_or("")
}

/// A mode by its short name or its configuration name (`workspace_autonomous`).
pub fn parse_mode(text: &str) -> Option<PermissionProfile> {
    let t = text.trim().to_ascii_lowercase();
    MODES.iter().find(|(name, _, _)| *name == t).map(|(_, p, _)| *p).or_else(|| PermissionProfile::parse(&t))
}

/// The mode after `profile` in Shift-Tab order. Bypass is never cycled
/// into, so a stray keypress cannot turn prompts off.
pub fn next_mode(profile: PermissionProfile) -> PermissionProfile {
    let cycle: Vec<PermissionProfile> = MODES.iter().map(|(_, p, _)| *p).filter(|p| *p != PermissionProfile::BypassPermissions).collect();
    match cycle.iter().position(|p| *p == profile) {
        Some(i) => cycle[(i + 1) % cycle.len()],
        None => cycle[0],
    }
}

pub fn help() -> String {
    let mut s = String::new();
    for c in COMMANDS {
        s.push_str(&format!("{:<19} {}\n", format!("{} {}", c.name, c.args).trim_end(), c.about));
    }
    s.push_str("\nApproval modes (/approval <mode>; shift-tab cycles all but bypass):\n");
    for (name, _, about) in MODES {
        s.push_str(&format!("  {name:<11}{about}\n"));
    }
    s.push_str("\nReasoning effort (/effort <level>; the model's own values when models.dev lists them):\n");
    for effort in effort_choices(None) {
        s.push_str(&format!("  {:<11}{}\n", effort_name(effort.as_ref()), effort_about(effort.as_ref())));
    }
    s.push_str("\n↑↓ earlier prompts · mouse wheel or pgup/pgdn scroll · home/end top/latest\nwhile a task runs: enter steers it (the model reads it at its next step) · tab or ⌘/ctrl-enter queues it for after\ntab completes · @ mentions an installed app (its tools are preferred) or a file (its text goes with the message) · ctrl-o thinking and agent detail · ↓ on an empty prompt or /agents opens the agents view
ctrl-t event log · esc quit\nselect text with option-drag (iTerm2, Terminal.app) or shift-drag");
    s
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub label: String,
    pub hint: String,
    pub about: String,
    /// The whole input line once this suggestion is accepted.
    pub replacement: String,
    /// The command this completes takes an argument (Tab adds a space).
    pub takes_args: bool,
}

/// Suggestions for a partly typed slash command: command names first, then
/// arguments for `/approval` (modes) and `/model` (models listed earlier).
/// `effort_levels` are the current model's effort values (models.dev).
pub fn suggestions(input: &str, models: &[String], mode: PermissionProfile, model: &str, effort: Option<&ReasoningEffort>, effort_levels: Option<&[String]>) -> Vec<Suggestion> {
    if !input.starts_with('/') || input.contains('\n') {
        return Vec::new();
    }
    let Some((command, partial)) = input.split_once(' ') else {
        let typed = input.to_ascii_lowercase();
        return COMMANDS
            .iter()
            .filter(|c| c.name.starts_with(&typed))
            .map(|c| Suggestion { label: c.name.into(), hint: c.args.into(), about: c.about.into(), replacement: c.name.into(), takes_args: !c.args.is_empty() })
            .collect();
    };
    let partial = partial.trim_start().to_ascii_lowercase();
    if command == "/usage" {
        return usage_suggestions(&partial);
    }
    if partial.contains(' ') {
        return Vec::new();
    }
    let current = |yes: bool| if yes { "current".to_string() } else { String::new() };
    match command {
        "/approval" => MODES
            .iter()
            .filter(|(name, _, _)| name.starts_with(&partial))
            .map(|(name, p, about)| Suggestion { label: (*name).into(), hint: current(*p == mode), about: (*about).into(), replacement: format!("/approval {name}"), takes_args: false })
            .collect(),
        "/model" => models
            .iter()
            .filter(|m| m.to_ascii_lowercase().contains(&partial))
            .take(50)
            .map(|m| Suggestion { label: m.clone(), hint: current(m == model), about: String::new(), replacement: format!("/model {m}"), takes_args: false })
            .collect(),
        "/effort" => effort_choices(effort_levels)
            .into_iter()
            .filter(|e| effort_name(e.as_ref()).starts_with(&partial))
            .map(|e| {
                let name = effort_name(e.as_ref());
                Suggestion { label: name.into(), hint: current(e.as_ref() == effort), about: effort_about(e.as_ref()).into(), replacement: format!("/effort {name}"), takes_args: false }
            })
            .collect(),
        "/billing" => crate::account::BILLING_COMMANDS
            .iter()
            .filter(|(name, _)| name.starts_with(&partial))
            .map(|(name, about)| Suggestion { label: (*name).into(), hint: String::new(), about: (*about).into(), replacement: format!("/billing {name}"), takes_args: *name != "confirm" })
            .collect(),
        _ => Vec::new(),
    }
}

/// An installed app `@` can mention (from `McpManager::status`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MentionCandidate {
    pub name: String,
    /// "connected · 9 tools", "paused", "failed", …
    pub state: String,
    pub about: String,
}

/// The `@` word being typed at the end of the input: where it starts and
/// what follows the `@` (`None` for slash commands and plain words).
pub fn mention_partial(input: &str) -> Option<(usize, &str)> {
    if input.starts_with('/') {
        return None;
    }
    let word_start = input.char_indices().filter(|(_, c)| c.is_whitespace()).last().map(|(i, c)| i + c.len_utf8()).unwrap_or(0);
    // `＠` is what CJK input methods type for `@`.
    let word = &input[word_start..];
    word.strip_prefix('@').or_else(|| word.strip_prefix('＠')).map(|partial| (word_start, partial))
}

/// Suggestions for an `@` mention being typed at the end of the input:
/// installed apps whose name starts with (then contains) the partial word,
/// then `files`, the repository paths already matched against it (see
/// `ctx_tools::mentions::search_files`). Accepting one replaces the
/// partial word with `@name ` or `@path `.
pub fn mention_suggestions(input: &str, apps: &[MentionCandidate], files: &[String]) -> Vec<Suggestion> {
    let Some((word_start, partial)) = mention_partial(input) else { return Vec::new() };
    let lower = partial.to_ascii_lowercase();
    // An app name typed in full needs no menu (Enter sends).
    if !lower.is_empty() && apps.iter().any(|a| a.name.eq_ignore_ascii_case(&lower)) {
        return Vec::new();
    }
    let name_like = partial.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    let mut out: Vec<Suggestion> = Vec::new();
    if name_like {
        let mut matches: Vec<&MentionCandidate> = apps.iter().filter(|a| a.name.to_ascii_lowercase().starts_with(&lower)).collect();
        matches.extend(apps.iter().filter(|a| !a.name.to_ascii_lowercase().starts_with(&lower) && a.name.to_ascii_lowercase().contains(&lower)));
        out.extend(matches.into_iter().map(|a| Suggestion { label: format!("@{}", a.name), hint: a.state.clone(), about: a.about.clone(), replacement: format!("{}@{} ", &input[..word_start], a.name), takes_args: false }));
    }
    for path in files {
        if out.len() >= 10 {
            break;
        }
        let inserted = ctx_tools::mentions::mention_text(path);
        out.push(Suggestion { label: format!("@{path}"), hint: "file".into(), about: String::new(), replacement: format!("{}{inserted}", &input[..word_start]), takes_args: false });
    }
    out
}

/// `/usage` takes a period and filters in any order: the word being typed
/// completes to a period (until one is given) or a filter key. After the
/// first word, suggestions wait for a letter, so Enter on a trailing space
/// runs the line as typed.
fn usage_suggestions(partial: &str) -> Vec<Suggestion> {
    let (done, word) = partial.rsplit_once(' ').unwrap_or(("", partial));
    if word.contains([':', '=']) || (!done.is_empty() && word.is_empty()) {
        return Vec::new();
    }
    let has_period = done.split_whitespace().any(|w| !w.contains([':', '=']));
    let prefix = if done.is_empty() { "/usage ".to_string() } else { format!("/usage {done} ") };
    let periods = crate::account::PERIODS.iter().filter(|_| !has_period).map(|(name, about)| (*name, *about));
    periods
        .chain(crate::account::FILTERS.iter().map(|(name, about)| (*name, *about)))
        .filter(|(name, _)| name.starts_with(word))
        .map(|(name, about)| Suggestion { label: name.into(), hint: String::new(), about: about.into(), replacement: format!("{prefix}{name}"), takes_args: false })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(s: &[Suggestion]) -> Vec<&str> {
        s.iter().map(|s| s.label.as_str()).collect()
    }

    #[test]
    fn command_names_complete_by_prefix() {
        let ask = PermissionProfile::ApprovalRequired;
        assert_eq!(labels(&suggestions("/", &[], ask, "m", None, None)), vec!["/new", "/resume", "/model", "/approval", "/effort", "/agents", "/mcp", "/signin", "/signout", "/usage", "/billing", "/help"]);
        assert_eq!(labels(&suggestions("/ap", &[], ask, "m", None, None)), vec!["/approval"]);
        assert!(suggestions("/ap", &[], ask, "m", None, None)[0].takes_args);
        assert!(suggestions("/zzz", &[], ask, "m", None, None).is_empty());
        assert!(suggestions("fix the bug", &[], ask, "m", None, None).is_empty());
    }

    #[test]
    fn arguments_complete_for_modes_and_known_models() {
        let auto = PermissionProfile::WorkspaceAutonomous;
        let modes = suggestions("/approval ", &[], auto, "m", None, None);
        assert_eq!(labels(&modes), vec!["ask", "auto", "bypass"]);
        assert_eq!(modes[1].hint, "current");
        assert_eq!(suggestions("/approval by", &[], auto, "m", None, None)[0].replacement, "/approval bypass");
        let models = vec!["gpt-4.1".to_string(), "gpt-4.1-mini".to_string(), "o3".to_string()];
        assert_eq!(labels(&suggestions("/model mini", &models, auto, "o3", None, None)), vec!["gpt-4.1-mini"]);
        assert_eq!(suggestions("/model ", &models, auto, "o3", None, None)[2].hint, "current");
        assert!(suggestions("/model a b", &models, auto, "o3", None, None).is_empty());
        let efforts = suggestions("/effort ", &[], auto, "m", Some(&ReasoningEffort::Ultra), None);
        assert_eq!(labels(&efforts), vec!["auto", "low", "medium", "high", "ultra"]);
        assert_eq!(efforts[4].hint, "current");
        let opus: Vec<String> = ["low", "medium", "high", "xhigh", "max"].map(String::from).to_vec();
        let xhigh = ReasoningEffort::level("xhigh");
        let efforts = suggestions("/effort ", &[], auto, "m", Some(&xhigh), Some(&opus));
        assert_eq!(labels(&efforts), vec!["auto", "low", "medium", "high", "xhigh", "max", "ultra"], "the model's own values");
        assert_eq!(efforts[4].hint, "current");
        assert_eq!(labels(&suggestions("/effort ", &[], auto, "m", None, Some(&[]))), vec!["auto", "ultra"], "a model without effort control");
        assert_eq!(suggestions("/effort u", &[], auto, "m", None, None)[0].replacement, "/effort ultra");
    }

    #[test]
    fn mentions_complete_installed_apps() {
        let apps = vec![
            MentionCandidate { name: "github".into(), state: "connected".into(), about: String::new() },
            MentionCandidate { name: "computer-use".into(), state: "connected".into(), about: String::new() },
            MentionCandidate { name: "playwright".into(), state: "paused".into(), about: String::new() },
        ];
        let none: Vec<String> = Vec::new();
        assert_eq!(labels(&mention_suggestions("@", &apps, &none)), vec!["@github", "@computer-use", "@playwright"]);
        let one = mention_suggestions("check issues on @gi", &apps, &none);
        assert_eq!(labels(&one), vec!["@github"]);
        assert_eq!(one[0].replacement, "check issues on @github ");
        assert_eq!(labels(&mention_suggestions("@use", &apps, &none)), vec!["@computer-use"], "contains, after prefixes");
        assert!(mention_suggestions("@github", &apps, &none).is_empty(), "typed in full");
        assert!(mention_suggestions("mail me@gi", &apps, &none).is_empty(), "an email address is not a mention");
        assert!(mention_suggestions("/model @g", &apps, &none).is_empty(), "slash commands keep their own menu");
        assert!(mention_suggestions("@github ", &apps, &none).is_empty(), "after the space the mention is done");
        let files = vec!["src/main.rs".to_string(), "docs/My Notes.md".to_string()];
        let mixed = mention_suggestions("explain @ma", &apps, &files);
        assert_eq!(labels(&mixed), vec!["@src/main.rs", "@docs/My Notes.md"], "no app matches `ma`; files follow");
        assert_eq!(labels(&mention_suggestions("@m", &apps, &files[..1])), vec!["@computer-use", "@src/main.rs"], "apps first, then files");
        assert_eq!(mixed[1].replacement, "explain @\"docs/My Notes.md\" ", "paths with spaces are quoted");
        assert_eq!(labels(&mention_suggestions("@src/ma", &apps, &files[..1])), vec!["@src/main.rs"], "a path is not an app name");
        assert_eq!(mention_partial("fix @src/a"), Some((4, "src/a")));
        assert_eq!(mention_suggestions("＠gi", &apps, &none)[0].replacement, "@github ", "full-width ＠ completes to @");
    }

    #[test]
    fn usage_completes_periods_then_filters() {
        let ask = PermissionProfile::ApprovalRequired;
        let usage = |input: &str| suggestions(input, &[], ask, "m", None, None);
        assert_eq!(labels(&usage("/usage ")), vec!["today", "24h", "7d", "30d", "90d", "year", "all", "model:", "project:"]);
        assert_eq!(usage("/usage 2")[0].replacement, "/usage 24h");
        assert_eq!(labels(&usage("/usage 7d m")), vec!["model:"]);
        assert_eq!(usage("/usage 7d p")[0].replacement, "/usage 7d project:");
        assert!(usage("/usage 7d a").is_empty(), "one period only");
        assert_eq!(labels(&usage("/usage model:gpt a")), vec!["all"]);
        assert!(usage("/usage 7d ").is_empty(), "enter runs the line as typed");
        assert!(usage("/usage model:gp").is_empty());
    }

    #[test]
    fn efforts_parse() {
        assert_eq!(parse_effort("auto"), Some(None));
        assert_eq!(parse_effort("Ultra"), Some(Some(ReasoningEffort::Ultra)));
        assert_eq!(parse_effort("max"), Some(Some(ReasoningEffort::level("max"))));
        assert_eq!(parse_effort("very hard"), None);
        assert_eq!(effort_name(Some(&ReasoningEffort::level("xhigh"))), "xhigh");
        assert_eq!(effort_name(None), "auto");
        assert_eq!(ReasoningEffort::Ultra.wire_value(), "high", "the endpoint never sees `ultra`");
    }

    #[test]
    fn modes_parse_and_cycle() {
        assert_eq!(parse_mode("Auto"), Some(PermissionProfile::WorkspaceAutonomous));
        assert_eq!(parse_mode("read_only"), None, "read-only is gone");
        assert_eq!(parse_mode("sandbox"), None, "sandbox is gone");
        assert_eq!(parse_mode("yolo"), None);
        assert_eq!(parse_mode("bypass"), Some(PermissionProfile::BypassPermissions));
        assert_eq!(parse_mode("bypass_permissions"), Some(PermissionProfile::BypassPermissions));
        let mut p = PermissionProfile::ApprovalRequired;
        let mut seen = Vec::new();
        for _ in 0..4 {
            p = next_mode(p);
            seen.push(mode_name(p));
        }
        assert_eq!(seen, vec!["auto", "ask", "auto", "ask"], "shift-tab never lands on bypass");
        assert_eq!(next_mode(PermissionProfile::BypassPermissions), PermissionProfile::ApprovalRequired);
    }
}
