//! `@app` mentions in a message: the user names an installed app (an MCP
//! server, by its `[mcp.servers.<name>]` key) to point the agent at its
//! tools, as `@` does in Claude Code and Codex. Both UIs complete them from
//! the installed apps; the runtime turns them into one trusted instruction
//! for the turn.

use super::{McpServerState, McpServerStatus};

/// Tools listed by name in the instruction, per app.
const LISTED_TOOLS: usize = 12;

/// The servers `text` mentions as `@name`, in order of first mention, each
/// once. A mention starts the text or follows whitespace or an opening
/// bracket or quote (so `me@github.com` is not one), and matches a name in
/// `names` case-insensitively.
pub fn mentioned_servers(text: &str, names: &[String]) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '@' && (i == 0 || chars[i - 1].is_whitespace() || "([{\"'`".contains(chars[i - 1])) {
            let start = i + 1;
            let mut end = start;
            while end < chars.len() && (chars[end].is_ascii_alphanumeric() || chars[end] == '_' || chars[end] == '-') {
                end += 1;
            }
            // A trailing `-` is punctuation ("@github- and"), not part of a name.
            let mut word: String = chars[start..end].iter().collect();
            while word.ends_with('-') {
                word.pop();
            }
            if let Some(name) = names.iter().find(|n| n.eq_ignore_ascii_case(&word)) {
                if !found.contains(name) {
                    found.push(name.clone());
                }
            }
            i = end.max(i + 1);
        } else {
            i += 1;
        }
    }
    found
}

/// The trusted instruction for a turn whose message mentions apps: which
/// tools to reach for, and which mentioned apps cannot be used right now.
/// `None` when nothing installed is mentioned.
pub fn mention_instruction(text: &str, servers: &[McpServerStatus]) -> Option<String> {
    let names: Vec<String> = servers.iter().map(|s| s.name.clone()).collect();
    let mentioned = mentioned_servers(text, &names);
    if mentioned.is_empty() {
        return None;
    }
    let mut ready = Vec::new();
    let mut unavailable = Vec::new();
    for name in &mentioned {
        let Some(status) = servers.iter().find(|s| &s.name == name) else { continue };
        let tools: Vec<&str> = status.tools.iter().filter(|t| t.enabled).map(|t| t.qualified_name.as_str()).collect();
        match status.state {
            McpServerState::Connected if !tools.is_empty() => {
                let shown = tools.iter().take(LISTED_TOOLS).copied().collect::<Vec<_>>().join(", ");
                let more = if tools.len() > LISTED_TOOLS { format!(" and {} more", tools.len() - LISTED_TOOLS) } else { String::new() };
                ready.push(format!("@{name} ({shown}{more})"));
            }
            McpServerState::Connected => unavailable.push(format!("@{name} (connected, but every tool is switched off)")),
            McpServerState::Connecting => unavailable.push(format!("@{name} (still connecting)")),
            McpServerState::Disabled => unavailable.push(format!("@{name} (paused; the user can turn it on in Apps)")),
            McpServerState::Failed => unavailable.push(format!("@{name} (failed: {})", status.error.as_deref().unwrap_or("unknown error"))),
        }
    }
    let mut out = String::new();
    if !ready.is_empty() {
        out.push_str(&format!("The user's message mentions {}: use those apps' tools for the parts of the request they fit, in preference to other ways of doing the same thing. ", ready.join("; ")));
    }
    if !unavailable.is_empty() {
        out.push_str(&format!("Mentioned but unavailable: {}. Tell the user instead of working around it silently. ", unavailable.join("; ")));
    }
    Some(out.trim_end().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::McpToolInfo;

    fn names() -> Vec<String> {
        vec!["github".into(), "computer-use".into(), "playwright".into()]
    }

    #[test]
    fn mentions_are_found_at_word_starts_only() {
        assert_eq!(mentioned_servers("@github list my open PRs", &names()), vec!["github"]);
        assert_eq!(mentioned_servers("use @Computer-Use, then (@github) and @github again", &names()), vec!["computer-use", "github"]);
        assert!(mentioned_servers("mail me@github.com", &names()).is_empty(), "an email address is not a mention");
        assert!(mentioned_servers("@githubby and @unknown", &names()).is_empty(), "whole names only");
        assert_eq!(mentioned_servers("ask @playwright-", &names()), vec!["playwright"]);
    }

    fn status(name: &str, state: McpServerState, tools: &[&str]) -> McpServerStatus {
        McpServerStatus {
            name: name.into(),
            enabled: true,
            app: Some(name.into()),
            state,
            error: None,
            transport: "stdio".into(),
            target: String::new(),
            server_name: None,
            server_version: None,
            protocol_version: None,
            instructions: None,
            tools: tools.iter().map(|t| McpToolInfo { name: (*t).into(), qualified_name: format!("mcp__{name}__{t}"), description: String::new(), read_only: false, destructive: false, enabled: true }).collect(),
        }
    }

    #[test]
    fn instructions_name_tools_and_unavailable_apps() {
        let servers = vec![status("github", McpServerState::Connected, &["search_issues", "get_issue"]), status("playwright", McpServerState::Disabled, &[])];
        let text = mention_instruction("@github and @playwright: check the flaky issue", &servers).unwrap();
        assert!(text.contains("@github (mcp__github__search_issues, mcp__github__get_issue)"), "{text}");
        assert!(text.contains("@playwright (paused"), "{text}");
        assert_eq!(mention_instruction("no mentions here", &servers), None);
    }
}
