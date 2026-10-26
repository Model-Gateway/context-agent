//! Context compaction and continuation (spec §8.6, §8.7).

use ctx_core::{AgentMessage, CheckpointId, TaskId, Timestamp};
use ctx_memory::TaskState;
use serde::{Deserialize, Serialize};

/// A compacted task checkpoint. The full event log remains available
/// externally; the checkpoint carries only what continuation requires.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskCheckpoint {
    pub id: CheckpointId,
    pub task_id: TaskId,
    pub created_at: Timestamp,
    pub reason: String,
    /// Rendered summary supplied to the model.
    pub summary: String,
    /// Structured snapshot for programmatic continuation.
    pub state: TaskState,
    pub kept_messages: usize,
    pub dropped_messages: usize,
}

impl TaskCheckpoint {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }

    pub fn from_json(value: &serde_json::Value) -> Option<Self> {
        serde_json::from_value(value.clone()).ok()
    }
}

/// Index at which to cut so that the kept tail starts at a safe boundary:
/// a user message, or an assistant message that issued the tool calls whose
/// results follow (never a dangling tool result).
fn safe_cut(messages: &[AgentMessage], desired: usize) -> usize {
    let mut cut = desired.min(messages.len());
    while cut > 0 {
        match &messages[cut] {
            AgentMessage::User(_) => break,
            AgentMessage::Assistant(a) if !a.tool_calls.is_empty() => break,
            AgentMessage::Assistant(_) => break,
            AgentMessage::ToolResult(_) => cut -= 1,
            _ => cut -= 1,
        }
    }
    cut
}

/// Build a checkpoint from the task state and keep only the most recent
/// `keep_recent_turns` user turns (at least one complete exchange) of the
/// conversation. Returns the checkpoint and the new active conversation.
pub fn compact(state: &TaskState, conversation: &[AgentMessage], keep_recent_turns: usize, max_kept_messages: usize, reason: &str) -> (TaskCheckpoint, Vec<AgentMessage>) {
    let user_indices: Vec<usize> = conversation.iter().enumerate().filter(|(_, m)| matches!(m, AgentMessage::User(_))).map(|(i, _)| i).collect();
    let keep = keep_recent_turns.max(1);
    let mut cut = if user_indices.len() > keep { user_indices[user_indices.len() - keep] } else { 0 };
    // A single enormous turn: keep only its tail, at a safe boundary.
    if conversation.len() - cut > max_kept_messages {
        cut = safe_cut(conversation, conversation.len() - max_kept_messages);
    }
    // Always keep the latest user message even if the tail is tiny.
    let kept: Vec<AgentMessage> = conversation[cut..].to_vec();
    let mut snapshot = state.clone();
    snapshot.checkpoints += 1;
    let checkpoint = TaskCheckpoint {
        id: CheckpointId::new(),
        task_id: state.task_id.clone(),
        created_at: ctx_core::now(),
        reason: reason.to_string(),
        summary: state.checkpoint_summary(),
        state: snapshot,
        kept_messages: kept.len(),
        dropped_messages: cut,
    };
    (checkpoint, kept)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ctx_core::{AssistantMessage, TaskCategory, ToolCall};

    #[test]
    fn keeps_recent_turns_and_never_splits_exchanges() {
        let state = TaskState::new(TaskId::new(), "objective", TaskCategory::LocalizedEdit);
        let mut conv = Vec::new();
        for n in 0..4 {
            conv.push(AgentMessage::user(format!("u{n}")));
            conv.push(AgentMessage::Assistant(AssistantMessage { text: None, tool_calls: vec![ToolCall { id: format!("c{n}"), name: "read_file".into(), arguments: serde_json::json!({}) }], provider_metadata: None }));
            conv.push(AgentMessage::tool_result(format!("c{n}"), "read_file", "body", false));
            conv.push(AgentMessage::assistant_text(format!("a{n}")));
        }
        let (cp, kept) = compact(&state, &conv, 2, 100, "test");
        assert_eq!(cp.dropped_messages, 8);
        assert!(matches!(kept[0], AgentMessage::User(_)));
        assert_eq!(kept.len(), 8);
        assert!(cp.summary.contains("Objective: objective"));
        assert_eq!(cp.state.checkpoints, 1);

        // One huge turn: the cut must not land on a dangling tool result.
        let mut huge = vec![AgentMessage::user("u")];
        for n in 0..30 {
            huge.push(AgentMessage::Assistant(AssistantMessage { text: None, tool_calls: vec![ToolCall { id: format!("h{n}"), name: "t".into(), arguments: serde_json::json!({}) }], provider_metadata: None }));
            huge.push(AgentMessage::tool_result(format!("h{n}"), "t", "r", false));
        }
        let (_cp, kept) = compact(&state, &huge, 2, 11, "test");
        assert!(kept.len() <= 12);
        assert!(matches!(&kept[0], AgentMessage::Assistant(a) if !a.tool_calls.is_empty()), "{:?}", kept[0]);
    }
}
