//! Persistent event log (spec §14.3).

use ctx_core::{AgentEvent, Result, SessionId, TaskId};
use ctx_security::Redactor;
use ctx_storage::{EventRecord, SessionStore};
use std::sync::Arc;

/// Appends runtime events to the session store with redaction and optional
/// truncation of large tool outputs.
#[derive(Clone)]
pub struct EventLog {
    store: Arc<SessionStore>,
    redactor: Arc<Redactor>,
    max_payload_chars: usize,
}

impl EventLog {
    pub fn new(store: Arc<SessionStore>, redactor: Arc<Redactor>, store_full_output: bool) -> Self {
        Self { store, redactor, max_payload_chars: if store_full_output { usize::MAX } else { 16_000 } }
    }

    pub fn store(&self) -> &Arc<SessionStore> {
        &self.store
    }

    pub fn redactor(&self) -> &Redactor {
        &self.redactor
    }

    /// Record an [`AgentEvent`]. Returns the event sequence id, or `None` for
    /// transient events that are not persisted.
    pub fn record(&self, session_id: &SessionId, event: &AgentEvent) -> Result<Option<i64>> {
        if !event.is_persistent() {
            return Ok(None);
        }
        let mut payload = serde_json::to_value(event)?;
        self.redactor.redact_json(&mut payload);
        truncate_strings(&mut payload, self.max_payload_chars);
        let id = self.store.append_event(Some(session_id.as_str()), Some(event.task_id().as_str()), event.name(), &payload)?;
        Ok(Some(id))
    }

    /// Record an arbitrary structured event (e.g. a model request snapshot).
    pub fn record_raw(&self, session_id: Option<&SessionId>, task_id: Option<&TaskId>, kind: &str, mut payload: serde_json::Value) -> Result<i64> {
        self.redactor.redact_json(&mut payload);
        truncate_strings(&mut payload, self.max_payload_chars);
        self.store.append_event(session_id.map(|s| s.as_str()), task_id.map(|t| t.as_str()), kind, &payload)
    }

    pub fn events_for_task(&self, task_id: &TaskId) -> Result<Vec<EventRecord>> {
        self.store.events_for_task(task_id.as_str(), 0, usize::MAX / 2)
    }

    /// Reconstruct typed events for a task (events with unknown kinds are
    /// skipped).
    pub fn replay(&self, task_id: &TaskId) -> Result<Vec<AgentEvent>> {
        Ok(self
            .events_for_task(task_id)?
            .into_iter()
            .filter_map(|r| serde_json::from_value::<AgentEvent>(r.payload).ok())
            .collect())
    }

    pub fn prune(&self, retention_days: u32) -> Result<u64> {
        self.store.prune_events(retention_days)
    }
}

fn truncate_strings(value: &mut serde_json::Value, max: usize) {
    if max == usize::MAX {
        return;
    }
    match value {
        serde_json::Value::String(s) if s.len() > max => {
            let mut cut = max;
            while !s.is_char_boundary(cut) {
                cut -= 1;
            }
            let omitted = s.len() - cut;
            s.truncate(cut);
            s.push_str(&format!("\n…[{omitted} bytes truncated in event log]"));
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(|v| truncate_strings(v, max)),
        serde_json::Value::Object(map) => map.values_mut().for_each(|v| truncate_strings(v, max)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ctx_core::ExecutionState;

    #[test]
    fn records_and_replays_with_redaction() {
        let store = Arc::new(SessionStore::in_memory().unwrap());
        let log = EventLog::new(store, Arc::new(Redactor::default()), true);
        let session = SessionId::new();
        let task = TaskId::new();
        log.record(&session, &AgentEvent::StateChanged { task_id: task.clone(), from: ExecutionState::Idle, to: ExecutionState::Preparing }).unwrap();
        log.record(
            &session,
            &AgentEvent::Warning { task_id: task.clone(), message: "leaked sk-abcdefghijklmnopqrstuvwxyz0123".into() },
        )
        .unwrap();
        assert!(log.record(&session, &AgentEvent::ModelTextDelta { task_id: task.clone(), text: "x".into() }).unwrap().is_none());
        let events = log.replay(&task).unwrap();
        assert_eq!(events.len(), 2);
        match &events[1] {
            AgentEvent::Warning { message, .. } => assert!(message.contains("[REDACTED]")),
            _ => panic!(),
        }
    }
}
