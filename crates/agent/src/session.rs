//! Sessions, task resolution and conversation persistence (spec §9.4).

use crate::repository::RepositoryContext;
use ctx_context::{compact, TaskCheckpoint};
use ctx_core::{estimate_tokens, AgentEvent, AgentMessage, CtxError, ExecutionState, Result, SessionId, TaskId};
use ctx_memory::{classify_boundary, extract_keywords, BoundaryDecision, TaskBoundary, TaskState, TaskSummary};
use ctx_retrieval::analyze_query;
use ctx_storage::{CheckpointRecord, SessionRecord, SessionStore, TaskRecord};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct TaskResolution {
    pub task: TaskRecord,
    pub state: TaskState,
    pub boundary: BoundaryDecision,
    /// The message continues or refines an existing task.
    pub continued: bool,
}

pub struct SessionManager {
    ctx: Arc<RepositoryContext>,
}

impl SessionManager {
    pub fn new(ctx: Arc<RepositoryContext>) -> Self {
        Self { ctx }
    }

    pub fn start(&self, mode: &str, model: &str) -> Result<SessionRecord> {
        self.start_with_metadata(mode, model, serde_json::json!({}))
    }

    pub fn start_with_metadata(&self, mode: &str, model: &str, metadata: serde_json::Value) -> Result<SessionRecord> {
        let now = ctx_core::now();
        let record = SessionRecord { id: SessionId::new().to_string(), repository_id: self.ctx.id.to_string(), mode: mode.into(), model: model.into(), active_task_id: None, created_at: now, updated_at: now, metadata };
        self.ctx.stores.sessions.create_session(&record)?;
        Ok(record)
    }

    pub fn resume(&self, id: &str) -> Result<SessionRecord> {
        self.ctx.stores.sessions.get_session(id)?.ok_or_else(|| CtxError::NotFound(format!("session {id}")))
    }

    pub fn resume_latest(&self) -> Result<Option<SessionRecord>> {
        self.ctx.stores.sessions.latest_session(self.ctx.id.as_str())
    }

    fn recent_summaries(&self, exclude: Option<&str>) -> Result<Vec<TaskSummary>> {
        Ok(self
            .ctx
            .stores
            .sessions
            .list_tasks(self.ctx.id.as_str(), 12)?
            .into_iter()
            .filter(|t| Some(t.id.as_str()) != exclude)
            .map(|t| {
                let state = TaskState::from_json(&t.state_json);
                TaskSummary {
                    task_id: TaskId::from_string(t.id.clone()),
                    keywords: state.as_ref().map(|s| s.keywords.clone()).unwrap_or_else(|| extract_keywords(&t.objective)),
                    modified_files: state.as_ref().map(|s| s.modified_files.clone()).unwrap_or_default(),
                    objective: t.objective,
                    completed: t.status.is_some(),
                }
            })
            .collect())
    }

    /// Decide which task a message belongs to and prepare its state.
    pub fn resolve_task(&self, session: &SessionRecord, message: &str) -> Result<TaskResolution> {
        let store = &self.ctx.stores.sessions;
        let active = match &session.active_task_id {
            Some(id) => store.get_task(id)?,
            None => None,
        };
        let active_state = active.as_ref().and_then(|t| TaskState::from_json(&t.state_json));
        let active_completed = active.as_ref().map(|t| t.status.is_some()).unwrap_or(true);
        let recent = self.recent_summaries(active.as_ref().map(|t| t.id.as_str()))?;
        let boundary = if session.mode == SessionStore::AGENT_MODE && active_state.is_some() {
            // A sub-agent's session holds one task: the lead agent's
            // follow-up messages always continue it.
            BoundaryDecision { boundary: if active_completed { TaskBoundary::Refine } else { TaskBoundary::Continue }, confidence: 1.0, rationale: "follow-up from the lead agent".into() }
        } else {
            classify_boundary(message, active_state.as_ref(), active_completed, &recent)
        };
        tracing::info!(boundary = ?boundary.boundary, confidence = boundary.confidence, rationale = %boundary.rationale, "task boundary");

        let now = ctx_core::now();
        let new_task = |objective: String, state: TaskState, parent: Option<String>| -> Result<(TaskRecord, TaskState)> {
            let record = TaskRecord {
                id: state.task_id.to_string(),
                session_id: session.id.clone(),
                repository_id: self.ctx.id.to_string(),
                objective,
                category: state.category.as_str().to_string(),
                state: ExecutionState::Idle.as_str().to_string(),
                status: None,
                parent_task_id: parent,
                summary: None,
                state_json: state.to_json(),
                created_at: now,
                updated_at: now,
                completed_at: None,
            };
            store.create_task(&record)?;
            Ok((record, state))
        };

        let (task, state, continued) = match (&boundary.boundary, active, active_state) {
            (TaskBoundary::Continue, Some(task), Some(mut state)) => {
                state.note_requirement(message);
                (task, state, true)
            }
            (TaskBoundary::Refine, Some(task), Some(mut state)) => {
                state.refine(message);
                (task, state, true)
            }
            (TaskBoundary::ReferEarlier { task_id }, _, _) => {
                let earlier = store.get_task(task_id.as_str())?;
                let category = analyze_query(message).category;
                let mut state = TaskState::new(TaskId::new(), message, category);
                if let Some(e) = &earlier {
                    state.note_requirement(format!("Refers to earlier task {} (\"{}\"); its result may need to be revisited.", &e.id[..8.min(e.id.len())], e.objective));
                    if let Some(es) = TaskState::from_json(&e.state_json) {
                        for f in &es.modified_files {
                            state.note_file_read(f.clone());
                        }
                        for k in es.keywords {
                            if !state.keywords.contains(&k) {
                                state.keywords.push(k);
                            }
                        }
                    }
                }
                let (record, state) = new_task(message.to_string(), state, earlier.map(|e| e.id))?;
                (record, state, false)
            }
            (TaskBoundary::Reverse { task_id }, _, _) => {
                let target = task_id.as_ref().and_then(|id| store.get_task(id.as_str()).ok().flatten());
                let mut state = TaskState::new(TaskId::new(), message, ctx_core::TaskCategory::LocalizedEdit);
                if let Some(t) = &target {
                    state.note_requirement(format!("Reverse the earlier task {} (\"{}\").", &t.id[..8.min(t.id.len())], t.objective));
                    if let Some(ts) = TaskState::from_json(&t.state_json) {
                        if !ts.modified_files.is_empty() {
                            state.note_requirement(format!("Files changed by that task: {}. Use git_diff/read_file to see their current state before reverting.", ts.modified_files.join(", ")));
                        }
                        for f in ts.modified_files {
                            state.note_file_read(f);
                        }
                    }
                }
                let (record, state) = new_task(message.to_string(), state, target.map(|t| t.id))?;
                (record, state, false)
            }
            _ => {
                let category = analyze_query(message).category;
                let state = TaskState::new(TaskId::new(), message, category);
                let (record, state) = new_task(message.to_string(), state, None)?;
                (record, state, false)
            }
        };
        store.set_active_task(&session.id, Some(&task.id))?;
        Ok(TaskResolution { task, state, boundary, continued })
    }

    /// Active conversation for a task (messages after the last compaction),
    /// with their storage ids.
    pub fn conversation(&self, task_id: &TaskId, compacted_before: i64) -> Result<Vec<(i64, AgentMessage)>> {
        Ok(self
            .ctx
            .stores
            .sessions
            .messages_for_task(task_id.as_str(), compacted_before)?
            .into_iter()
            .filter_map(|r| serde_json::from_value::<AgentMessage>(r.message).ok().map(|m| (r.id, m)))
            .collect())
    }

    pub fn append(&self, session: &SessionRecord, task_id: &TaskId, message: &AgentMessage) -> Result<i64> {
        self.ctx.stores.sessions.append_message(&session.id, Some(task_id.as_str()), &serde_json::to_value(message)?)
    }

    pub fn last_message_id(&self, task_id: &TaskId) -> Result<i64> {
        self.ctx.stores.sessions.last_message_id(task_id.as_str())
    }

    /// Compact the session's active task now, the way a run does at the
    /// threshold: a checkpoint replaces all but the recent turns, which the
    /// next run continues from. `None` when there is no task or nothing old
    /// enough to drop. The caller makes sure no run of the session is going.
    pub fn compact(&self, session: &SessionRecord) -> Result<Option<CompactOutcome>> {
        let store = &self.ctx.stores.sessions;
        let Some(task) = session.active_task_id.as_deref().map(|id| store.get_task(id)).transpose()?.flatten() else { return Ok(None) };
        let Some(mut state) = TaskState::from_json(&task.state_json) else { return Ok(None) };
        let task_id = TaskId::from_string(task.id.clone());
        let conversation = self.conversation(&task_id, state.compacted_before)?;
        let previous = store.latest_checkpoint(&task.id)?.and_then(|c| TaskCheckpoint::from_json(&c.summary));
        let messages: Vec<AgentMessage> = conversation.iter().map(|(_, m)| m.clone()).collect();
        let tokens = |messages: &[AgentMessage]| messages.iter().map(|m| estimate_tokens(&m.text_for_estimate())).sum::<usize>();
        let previous_checkpoint_tokens = previous.as_ref().map_or(0, |c| estimate_tokens(&c.summary));
        let tokens_before = tokens(&messages) + previous_checkpoint_tokens;
        let max_kept = (messages.len() / 2).max(4);
        let (checkpoint, kept) = compact(&state, &messages, self.ctx.config.context.keep_recent_turns, max_kept, "compacted on request");
        let dropped = conversation.len() - kept.len();
        if dropped == 0 {
            return Ok(None);
        }
        let boundary_id = conversation[dropped - 1].0;
        store.insert_checkpoint(&CheckpointRecord {
            id: checkpoint.id.to_string(),
            task_id: task.id.clone(),
            session_id: Some(session.id.clone()),
            reason: checkpoint.reason.clone(),
            summary: checkpoint.to_json(),
            event_seq: store.last_event_id(&task.id).unwrap_or(0),
            conversation_seq: boundary_id,
            created_at: ctx_core::now(),
        })?;
        state.compacted_before = boundary_id;
        state.checkpoints += 1;
        self.ctx.memory.save_task_state(&state)?;
        let event = AgentEvent::Checkpoint { task_id: task_id.clone(), checkpoint_id: checkpoint.id.clone(), reason: checkpoint.reason.clone() };
        if let Err(e) = self.ctx.events.record(&SessionId::from_string(session.id.clone()), &event) {
            tracing::warn!(error = %e, "failed to persist event");
        }
        Ok(Some(CompactOutcome {
            task_id: task.id,
            checkpoint_id: checkpoint.id.to_string(),
            dropped_messages: dropped,
            kept_messages: kept.len(),
            conversation_tokens: tokens(&kept),
            checkpoint_tokens: estimate_tokens(&checkpoint.summary),
            previous_checkpoint_tokens,
            tokens_before,
            tokens_after: tokens(&kept) + estimate_tokens(&checkpoint.summary),
        }))
    }
}

/// What `SessionManager::compact` did. The token counts are estimates: the
/// kept conversation, the new checkpoint summary and the one it replaces
/// (so a client can shift its last prompt estimate), and both together
/// before and after.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CompactOutcome {
    pub task_id: String,
    pub checkpoint_id: String,
    pub dropped_messages: usize,
    pub kept_messages: usize,
    pub conversation_tokens: usize,
    pub checkpoint_tokens: usize,
    pub previous_checkpoint_tokens: usize,
    pub tokens_before: usize,
    pub tokens_after: usize,
}
