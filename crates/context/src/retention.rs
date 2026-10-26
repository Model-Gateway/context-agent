//! Context retention policy (spec §8.5).

use ctx_core::{estimate_tokens, AgentMessage, ContextConfig, ToolResultMessage};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetentionClass {
    Required,
    LikelyRelevant,
    Retrievable,
    Disposable,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PruneStats {
    pub messages: usize,
    pub elided_tool_results: usize,
    pub truncated_tool_results: usize,
    pub tokens_before: usize,
    pub tokens_after: usize,
}

/// Index of the first message of each user turn.
fn turn_starts(messages: &[AgentMessage]) -> Vec<usize> {
    let mut starts: Vec<usize> = messages.iter().enumerate().filter(|(_, m)| matches!(m, AgentMessage::User(_))).map(|(i, _)| i).collect();
    if starts.first() != Some(&0) && !messages.is_empty() {
        starts.insert(0, 0);
    }
    starts
}

/// Truncate a tool result to `max_chars`, keeping head and tail.
pub fn truncate_tool_output(content: &str, max_chars: usize) -> String {
    if content.len() <= max_chars {
        return content.to_string();
    }
    let head_len = max_chars * 2 / 3;
    let tail_len = max_chars - head_len;
    let mut head_end = head_len;
    while head_end > 0 && !content.is_char_boundary(head_end) {
        head_end -= 1;
    }
    let mut tail_start = content.len() - tail_len;
    while tail_start < content.len() && !content.is_char_boundary(tail_start) {
        tail_start += 1;
    }
    format!("{}\n… [{} characters omitted from the middle; full output is in the task event log] …\n{}", &content[..head_end], content.len() - head_end - (content.len() - tail_start), &content[tail_start..])
}

fn elide_tool_result(r: &ToolResultMessage) -> ToolResultMessage {
    let first_line = r.content.lines().next().unwrap_or("").chars().take(120).collect::<String>();
    ToolResultMessage {
        call_id: r.call_id.clone(),
        tool_name: r.tool_name.clone(),
        content: format!("[earlier {} result elided ({} chars). First line: {}. Re-run the tool or use read_file if this is needed again.]", r.tool_name, r.content.len(), first_line),
        is_error: r.is_error,
    }
}

/// Classify every message and shrink what can be shrunk without breaking
/// tool-call/result pairing. The latest `keep_recent_turns` turns keep their
/// full content (subject to per-result truncation); older tool results are
/// elided to a stub; user and assistant text is kept.
pub fn prune_conversation(messages: &[AgentMessage], config: &ContextConfig) -> (Vec<AgentMessage>, PruneStats) {
    let mut stats = PruneStats { messages: messages.len(), ..Default::default() };
    stats.tokens_before = messages.iter().map(|m| estimate_tokens(&m.text_for_estimate())).sum();
    let starts = turn_starts(messages);
    let keep = config.keep_recent_turns.max(1);
    let recent_from = if starts.len() > keep { starts[starts.len() - keep] } else { 0 };
    let mut out = Vec::with_capacity(messages.len());
    for (i, m) in messages.iter().enumerate() {
        let class = classify(i, m, recent_from, messages.len());
        let kept = match (class, m) {
            (RetentionClass::Retrievable | RetentionClass::Disposable, AgentMessage::ToolResult(r)) if r.content.len() > 300 => {
                stats.elided_tool_results += 1;
                AgentMessage::ToolResult(elide_tool_result(r))
            }
            (_, AgentMessage::ToolResult(r)) if r.content.len() > config.max_tool_output_chars => {
                stats.truncated_tool_results += 1;
                AgentMessage::ToolResult(ToolResultMessage { call_id: r.call_id.clone(), tool_name: r.tool_name.clone(), content: truncate_tool_output(&r.content, config.max_tool_output_chars), is_error: r.is_error })
            }
            (RetentionClass::Retrievable, AgentMessage::Assistant(a)) if a.text.as_ref().map(|t| t.len() > 4_000).unwrap_or(false) => {
                let mut a = a.clone();
                let t = a.text.take().unwrap();
                let mut cut = 4_000;
                while !t.is_char_boundary(cut) {
                    cut -= 1;
                }
                a.text = Some(format!("{}\n… [earlier assistant message truncated]", &t[..cut]));
                AgentMessage::Assistant(a)
            }
            _ => m.clone(),
        };
        out.push(kept);
    }
    stats.tokens_after = out.iter().map(|m| estimate_tokens(&m.text_for_estimate())).sum();
    (out, stats)
}

/// Retention class of message `i`.
pub fn classify(i: usize, m: &AgentMessage, recent_from: usize, len: usize) -> RetentionClass {
    if i + 1 == len {
        return RetentionClass::Required; // latest user request
    }
    match m {
        // Tool-call messages are required for protocol pairing; their
        // results may shrink but never disappear.
        AgentMessage::Assistant(a) if !a.tool_calls.is_empty() => RetentionClass::Required,
        AgentMessage::User(_) => RetentionClass::Required,
        AgentMessage::Policy(_) | AgentMessage::TaskInstructions(_) | AgentMessage::Context(_) => RetentionClass::Required,
        _ if i >= recent_from => RetentionClass::LikelyRelevant,
        AgentMessage::ToolResult(r) if r.tool_name == "search_code" || r.tool_name == "list_files" => RetentionClass::Disposable,
        _ => RetentionClass::Retrievable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ctx_core::{AssistantMessage, ToolCall};

    fn exchange(n: usize, big: bool) -> Vec<AgentMessage> {
        let body = if big { "x".repeat(5_000) } else { "short".into() };
        vec![
            AgentMessage::user(format!("request {n}")),
            AgentMessage::Assistant(AssistantMessage { text: None, tool_calls: vec![ToolCall { id: format!("c{n}"), name: "read_file".into(), arguments: serde_json::json!({"path": "a"}) }], provider_metadata: None }),
            AgentMessage::tool_result(format!("c{n}"), "read_file", body, false),
            AgentMessage::assistant_text(format!("done {n}")),
        ]
    }

    #[test]
    fn old_tool_results_are_elided_recent_kept() {
        let mut messages = Vec::new();
        for n in 0..5 {
            messages.extend(exchange(n, true));
        }
        messages.push(AgentMessage::user("latest"));
        let cfg = ContextConfig { keep_recent_turns: 2, max_tool_output_chars: 10_000, ..Default::default() };
        let (pruned, stats) = prune_conversation(&messages, &cfg);
        assert_eq!(pruned.len(), messages.len(), "no message is dropped");
        assert_eq!(stats.elided_tool_results, 4, "turns 0-3 elided; turn 4 and latest kept");
        match &pruned[2] {
            AgentMessage::ToolResult(r) => assert!(r.content.starts_with("[earlier read_file result elided")),
            _ => panic!(),
        }
        match &pruned[18] {
            AgentMessage::ToolResult(r) => assert_eq!(r.content.len(), 5_000),
            _ => panic!(),
        }
        assert!(stats.tokens_after < stats.tokens_before / 2);
    }

    #[test]
    fn huge_recent_outputs_are_truncated_not_elided() {
        let mut messages = exchange(0, false);
        messages[2] = AgentMessage::tool_result("c0", "execute_command", "y".repeat(50_000), false);
        messages.push(AgentMessage::user("next"));
        let cfg = ContextConfig { max_tool_output_chars: 1_000, ..Default::default() };
        let (pruned, stats) = prune_conversation(&messages, &cfg);
        assert_eq!(stats.truncated_tool_results, 1);
        match &pruned[2] {
            AgentMessage::ToolResult(r) => {
                assert!(r.content.len() < 1_300);
                assert!(r.content.contains("omitted from the middle"));
            }
            _ => panic!(),
        }
    }
}
