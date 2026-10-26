//! Session store: sessions, tasks, event log, conversation, memories,
//! checkpoints, model requests, validation runs, metrics.

use crate::db::{from_json, now_text, parse_ts, to_json, Database};
use crate::index::fts_quote;
use crate::records::*;
use crate::schema::SESSION_MIGRATIONS;
use ctx_core::Result;
use rusqlite::{params, OptionalExtension, Row};
use std::path::Path;

pub struct SessionStore {
    db: Database,
}

#[derive(Debug, Clone, Default)]
pub struct MemoryFilter {
    pub repository_id: Option<String>,
    pub task_id: Option<String>,
    pub category: Option<String>,
    pub include_invalidated: bool,
    pub limit: usize,
}

fn session_from_row(r: &Row<'_>) -> rusqlite::Result<SessionRecord> {
    Ok(SessionRecord {
        id: r.get("id")?,
        repository_id: r.get("repository_id")?,
        mode: r.get("mode")?,
        model: r.get("model")?,
        active_task_id: r.get("active_task_id")?,
        created_at: parse_ts(&r.get::<_, String>("created_at")?),
        updated_at: parse_ts(&r.get::<_, String>("updated_at")?),
        metadata: serde_json::from_str(&r.get::<_, String>("metadata")?).unwrap_or(serde_json::Value::Null),
    })
}

fn task_from_row(r: &Row<'_>) -> rusqlite::Result<TaskRecord> {
    Ok(TaskRecord {
        id: r.get("id")?,
        session_id: r.get("session_id")?,
        repository_id: r.get("repository_id")?,
        objective: r.get("objective")?,
        category: r.get("category")?,
        state: r.get("state")?,
        status: r.get("status")?,
        parent_task_id: r.get("parent_task_id")?,
        summary: r.get("summary")?,
        state_json: serde_json::from_str(&r.get::<_, String>("state_json")?).unwrap_or(serde_json::Value::Null),
        created_at: parse_ts(&r.get::<_, String>("created_at")?),
        updated_at: parse_ts(&r.get::<_, String>("updated_at")?),
        completed_at: r.get::<_, Option<String>>("completed_at")?.map(|s| parse_ts(&s)),
    })
}

fn event_from_row(r: &Row<'_>) -> rusqlite::Result<EventRecord> {
    Ok(EventRecord {
        id: r.get("id")?,
        session_id: r.get("session_id")?,
        task_id: r.get("task_id")?,
        kind: r.get("kind")?,
        payload: serde_json::from_str(&r.get::<_, String>("payload")?).unwrap_or(serde_json::Value::Null),
        created_at: parse_ts(&r.get::<_, String>("created_at")?),
    })
}

fn conversation_from_row(r: &Row<'_>) -> rusqlite::Result<ConversationRecord> {
    Ok(ConversationRecord {
        id: r.get("id")?,
        session_id: r.get("session_id")?,
        task_id: r.get("task_id")?,
        message: serde_json::from_str(&r.get::<_, String>("message")?).unwrap_or(serde_json::Value::Null),
        created_at: parse_ts(&r.get::<_, String>("created_at")?),
    })
}

fn memory_from_row(r: &Row<'_>) -> rusqlite::Result<MemoryRow> {
    Ok(MemoryRow {
        id: r.get("id")?,
        repository_id: r.get("repository_id")?,
        task_id: r.get("task_id")?,
        category: r.get("category")?,
        content: r.get("content")?,
        evidence: serde_json::from_str(&r.get::<_, String>("evidence")?).unwrap_or(serde_json::Value::Array(vec![])),
        verification: r.get("verification")?,
        tags: r.get("tags")?,
        created_at: parse_ts(&r.get::<_, String>("created_at")?),
        updated_at: parse_ts(&r.get::<_, String>("updated_at")?),
        invalidated: r.get::<_, i64>("invalidated")? != 0,
        invalidation_reason: r.get("invalidation_reason")?,
    })
}

fn checkpoint_from_row(r: &Row<'_>) -> rusqlite::Result<CheckpointRecord> {
    Ok(CheckpointRecord {
        id: r.get("id")?,
        task_id: r.get("task_id")?,
        session_id: r.get("session_id")?,
        reason: r.get("reason")?,
        summary: serde_json::from_str(&r.get::<_, String>("summary_json")?).unwrap_or(serde_json::Value::Null),
        event_seq: r.get("event_seq")?,
        conversation_seq: r.get("conversation_seq")?,
        created_at: parse_ts(&r.get::<_, String>("created_at")?),
    })
}

fn model_request_from_row(r: &Row<'_>) -> rusqlite::Result<ModelRequestRecord> {
    Ok(ModelRequestRecord {
        id: r.get("id")?,
        task_id: r.get("task_id")?,
        session_id: r.get("session_id")?,
        step: r.get::<_, i64>("step")? as u32,
        provider: r.get("provider")?,
        model: r.get("model")?,
        prompt_tokens: r.get::<_, Option<i64>>("prompt_tokens")?.map(|n| n as u64),
        completion_tokens: r.get::<_, Option<i64>>("completion_tokens")?.map(|n| n as u64),
        cached_tokens: r.get::<_, Option<i64>>("cached_tokens")?.map(|n| n as u64),
        usage_source: r.get("usage_source")?,
        latency_ms: r.get::<_, i64>("latency_ms")? as u64,
        finish_reason: r.get("finish_reason")?,
        error: r.get("error")?,
        created_at: parse_ts(&r.get::<_, String>("created_at")?),
        cost_usd: r.get("cost_usd")?,
        gateway_request_id: r.get("gateway_request_id")?,
    })
}

fn validation_from_row(r: &Row<'_>) -> rusqlite::Result<ValidationRunRecord> {
    Ok(ValidationRunRecord {
        id: r.get("id")?,
        task_id: r.get("task_id")?,
        command: r.get("command")?,
        passed: r.get::<_, i64>("passed")? != 0,
        exit_code: r.get("exit_code")?,
        duration_ms: r.get::<_, i64>("duration_ms")? as u64,
        summary: r.get("summary")?,
        created_at: parse_ts(&r.get::<_, String>("created_at")?),
    })
}

impl SessionStore {
    pub fn open(path: &Path) -> Result<Self> {
        Ok(Self { db: Database::open(path, SESSION_MIGRATIONS)? })
    }

    pub fn in_memory() -> Result<Self> {
        Ok(Self { db: Database::in_memory(SESSION_MIGRATIONS)? })
    }

    // ----- sessions -------------------------------------------------------

    pub fn create_session(&self, session: &SessionRecord) -> Result<()> {
        self.db.with(|c| {
            c.execute(
                "INSERT INTO sessions (id, repository_id, mode, model, active_task_id, created_at, updated_at, metadata)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    session.id,
                    session.repository_id,
                    session.mode,
                    session.model,
                    session.active_task_id,
                    session.created_at.to_rfc3339(),
                    session.updated_at.to_rfc3339(),
                    session.metadata.to_string()
                ],
            )?;
            Ok(())
        })
    }

    pub fn get_session(&self, id: &str) -> Result<Option<SessionRecord>> {
        self.db.with(|c| c.query_row("SELECT * FROM sessions WHERE id = ?1", params![id], session_from_row).optional())
    }

    pub fn set_active_task(&self, session_id: &str, task_id: Option<&str>) -> Result<()> {
        self.db.with(|c| {
            c.execute(
                "UPDATE sessions SET active_task_id = ?1, updated_at = ?2 WHERE id = ?3",
                params![task_id, now_text(), session_id],
            )?;
            Ok(())
        })
    }

    pub fn set_session_metadata(&self, session_id: &str, metadata: &serde_json::Value) -> Result<()> {
        self.db.with(|c| {
            c.execute("UPDATE sessions SET metadata = ?1, updated_at = ?2 WHERE id = ?3", params![metadata.to_string(), now_text(), session_id])?;
            Ok(())
        })
    }

    /// Mode of the sessions sub-agents run in; they belong to a parent task
    /// and never show up as conversations of their own.
    pub const AGENT_MODE: &'static str = "agent";

    /// The user's sessions, newest first. Sub-agent sessions are left out;
    /// see [`Self::child_sessions`].
    pub fn list_sessions(&self, repository_id: &str, limit: usize) -> Result<Vec<SessionRecord>> {
        self.db.with(|c| {
            let mut stmt =
                c.prepare("SELECT * FROM sessions WHERE repository_id = ?1 AND mode != ?3 ORDER BY updated_at DESC LIMIT ?2")?;
            let rows = stmt.query_map(params![repository_id, limit as i64, Self::AGENT_MODE], session_from_row)?;
            rows.collect()
        })
    }

    /// Sub-agent sessions deployed from `session_id` (`metadata.parent_session`).
    pub fn child_sessions(&self, session_id: &str) -> Result<Vec<SessionRecord>> {
        self.db.with(|c| {
            let mut stmt = c.prepare("SELECT * FROM sessions WHERE mode = ?1 AND json_extract(metadata, '$.parent_session') = ?2 ORDER BY created_at ASC")?;
            let rows = stmt.query_map(params![Self::AGENT_MODE, session_id], session_from_row)?;
            rows.collect()
        })
    }

    pub fn latest_session(&self, repository_id: &str) -> Result<Option<SessionRecord>> {
        Ok(self.list_sessions(repository_id, 1)?.into_iter().next())
    }

    // ----- tasks ----------------------------------------------------------

    pub fn create_task(&self, task: &TaskRecord) -> Result<()> {
        self.db.with(|c| {
            c.execute(
                "INSERT INTO tasks (id, session_id, repository_id, objective, category, state, status, parent_task_id, summary, state_json, created_at, updated_at, completed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    task.id,
                    task.session_id,
                    task.repository_id,
                    task.objective,
                    task.category,
                    task.state,
                    task.status,
                    task.parent_task_id,
                    task.summary,
                    task.state_json.to_string(),
                    task.created_at.to_rfc3339(),
                    task.updated_at.to_rfc3339(),
                    task.completed_at.map(|t| t.to_rfc3339())
                ],
            )?;
            Ok(())
        })
    }

    pub fn update_task(&self, task: &TaskRecord) -> Result<()> {
        self.db.with(|c| {
            c.execute(
                "UPDATE tasks SET objective = ?2, category = ?3, state = ?4, status = ?5, parent_task_id = ?6, summary = ?7,
                   state_json = ?8, updated_at = ?9, completed_at = ?10 WHERE id = ?1",
                params![
                    task.id,
                    task.objective,
                    task.category,
                    task.state,
                    task.status,
                    task.parent_task_id,
                    task.summary,
                    task.state_json.to_string(),
                    now_text(),
                    task.completed_at.map(|t| t.to_rfc3339())
                ],
            )?;
            Ok(())
        })
    }

    pub fn get_task(&self, id: &str) -> Result<Option<TaskRecord>> {
        self.db.with(|c| c.query_row("SELECT * FROM tasks WHERE id = ?1", params![id], task_from_row).optional())
    }

    /// Find a task by full id or unique prefix.
    pub fn find_task(&self, id_or_prefix: &str) -> Result<Option<TaskRecord>> {
        if let Some(t) = self.get_task(id_or_prefix)? {
            return Ok(Some(t));
        }
        let pattern = format!("{}%", id_or_prefix.replace('%', "").replace('_', ""));
        self.db.with(|c| {
            let mut stmt = c.prepare("SELECT * FROM tasks WHERE id LIKE ?1 ORDER BY updated_at DESC LIMIT 2")?;
            let rows: Vec<TaskRecord> = stmt.query_map(params![pattern], task_from_row)?.collect::<rusqlite::Result<_>>()?;
            Ok(if rows.len() == 1 { rows.into_iter().next() } else { None })
        })
    }

    pub fn list_tasks(&self, repository_id: &str, limit: usize) -> Result<Vec<TaskRecord>> {
        self.db.with(|c| {
            let mut stmt = c.prepare("SELECT * FROM tasks WHERE repository_id = ?1 ORDER BY updated_at DESC LIMIT ?2")?;
            let rows = stmt.query_map(params![repository_id, limit as i64], task_from_row)?;
            rows.collect()
        })
    }

    pub fn tasks_for_session(&self, session_id: &str) -> Result<Vec<TaskRecord>> {
        self.db.with(|c| {
            let mut stmt = c.prepare("SELECT * FROM tasks WHERE session_id = ?1 ORDER BY created_at ASC")?;
            let rows = stmt.query_map(params![session_id], task_from_row)?;
            rows.collect()
        })
    }

    // ----- events ---------------------------------------------------------

    pub fn append_event(&self, session_id: Option<&str>, task_id: Option<&str>, kind: &str, payload: &serde_json::Value) -> Result<i64> {
        self.db.with(|c| {
            c.execute(
                "INSERT INTO events (session_id, task_id, kind, payload, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![session_id, task_id, kind, payload.to_string(), now_text()],
            )?;
            Ok(c.last_insert_rowid())
        })
    }

    pub fn events_for_task(&self, task_id: &str, after_id: i64, limit: usize) -> Result<Vec<EventRecord>> {
        self.db.with(|c| {
            let mut stmt = c.prepare("SELECT * FROM events WHERE task_id = ?1 AND id > ?2 ORDER BY id ASC LIMIT ?3")?;
            let rows = stmt.query_map(params![task_id, after_id, limit as i64], event_from_row)?;
            rows.collect()
        })
    }

    pub fn events_for_session(&self, session_id: &str, limit: usize) -> Result<Vec<EventRecord>> {
        self.db.with(|c| {
            let mut stmt = c.prepare("SELECT * FROM events WHERE session_id = ?1 ORDER BY id ASC LIMIT ?2")?;
            let rows = stmt.query_map(params![session_id, limit as i64], event_from_row)?;
            rows.collect()
        })
    }

    pub fn count_events(&self, task_id: &str, kind: Option<&str>) -> Result<u64> {
        self.db.with(|c| {
            let n: i64 = match kind {
                Some(k) => c.query_row(
                    "SELECT COUNT(*) FROM events WHERE task_id = ?1 AND kind = ?2",
                    params![task_id, k],
                    |r| r.get(0),
                )?,
                None => c.query_row("SELECT COUNT(*) FROM events WHERE task_id = ?1", params![task_id], |r| r.get(0))?,
            };
            Ok(n as u64)
        })
    }

    pub fn last_event_id(&self, task_id: &str) -> Result<i64> {
        self.db.with(|c| {
            c.query_row("SELECT COALESCE(MAX(id), 0) FROM events WHERE task_id = ?1", params![task_id], |r| r.get(0))
        })
    }

    /// Delete events older than `days` (retention, spec §14.3).
    pub fn prune_events(&self, days: u32) -> Result<u64> {
        let cutoff = (chrono::Utc::now() - chrono::Duration::days(days as i64)).to_rfc3339();
        self.db.with(|c| {
            let n = c.execute("DELETE FROM events WHERE created_at < ?1", params![cutoff])?;
            Ok(n as u64)
        })
    }

    // ----- conversation ---------------------------------------------------

    pub fn append_message(&self, session_id: &str, task_id: Option<&str>, message: &serde_json::Value) -> Result<i64> {
        self.db.with(|c| {
            c.execute(
                "INSERT INTO conversation (session_id, task_id, message, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![session_id, task_id, message.to_string(), now_text()],
            )?;
            Ok(c.last_insert_rowid())
        })
    }

    pub fn messages_for_task(&self, task_id: &str, after_id: i64) -> Result<Vec<ConversationRecord>> {
        self.db.with(|c| {
            let mut stmt = c.prepare("SELECT * FROM conversation WHERE task_id = ?1 AND id > ?2 ORDER BY id ASC")?;
            let rows = stmt.query_map(params![task_id, after_id], conversation_from_row)?;
            rows.collect()
        })
    }

    pub fn messages_for_session(&self, session_id: &str, limit: usize) -> Result<Vec<ConversationRecord>> {
        self.db.with(|c| {
            let mut stmt = c.prepare(
                "SELECT * FROM (SELECT * FROM conversation WHERE session_id = ?1 ORDER BY id DESC LIMIT ?2) ORDER BY id ASC",
            )?;
            let rows = stmt.query_map(params![session_id, limit as i64], conversation_from_row)?;
            rows.collect()
        })
    }

    /// How many messages the user sent in a session (the resume picker's count).
    pub fn user_message_count(&self, session_id: &str) -> Result<u64> {
        self.db.with(|c| {
            c.query_row("SELECT COUNT(*) FROM conversation WHERE session_id = ?1 AND json_extract(message, '$.kind') = 'user'", params![session_id], |r| r.get::<_, i64>(0).map(|n| n as u64))
        })
    }

    /// Text the user typed, newest first, across every session of this
    /// repository (prompt history for the interactive session).
    pub fn recent_user_messages(&self, limit: usize) -> Result<Vec<String>> {
        self.db.with(|c| {
            let mut stmt = c.prepare(
                "SELECT json_extract(message, '$.text') FROM conversation
                 WHERE json_extract(message, '$.kind') = 'user' AND json_extract(message, '$.text') IS NOT NULL
                 ORDER BY id DESC LIMIT ?1",
            )?;
            let rows = stmt.query_map(params![limit as i64], |r| r.get::<_, String>(0))?;
            rows.collect()
        })
    }

    pub fn last_message_id(&self, task_id: &str) -> Result<i64> {
        self.db.with(|c| {
            c.query_row("SELECT COALESCE(MAX(id), 0) FROM conversation WHERE task_id = ?1", params![task_id], |r| r.get(0))
        })
    }

    // ----- memories -------------------------------------------------------

    pub fn insert_memory(&self, m: &MemoryRow) -> Result<()> {
        self.db.with_tx(|tx| {
            tx.execute(
                "INSERT INTO memories (id, repository_id, task_id, category, content, evidence, verification, tags, created_at, updated_at, invalidated, invalidation_reason)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    m.id,
                    m.repository_id,
                    m.task_id,
                    m.category,
                    m.content,
                    m.evidence.to_string(),
                    m.verification,
                    m.tags,
                    m.created_at.to_rfc3339(),
                    m.updated_at.to_rfc3339(),
                    m.invalidated as i64,
                    m.invalidation_reason
                ],
            )?;
            tx.execute(
                "INSERT INTO memories_fts (content, tags, memory_id) VALUES (?1, ?2, ?3)",
                params![m.content, m.tags, m.id],
            )?;
            Ok(())
        })
    }

    pub fn update_memory(&self, m: &MemoryRow) -> Result<()> {
        self.db.with_tx(|tx| {
            tx.execute(
                "UPDATE memories SET task_id = ?2, category = ?3, content = ?4, evidence = ?5, verification = ?6, tags = ?7,
                   updated_at = ?8, invalidated = ?9, invalidation_reason = ?10 WHERE id = ?1",
                params![
                    m.id,
                    m.task_id,
                    m.category,
                    m.content,
                    m.evidence.to_string(),
                    m.verification,
                    m.tags,
                    now_text(),
                    m.invalidated as i64,
                    m.invalidation_reason
                ],
            )?;
            tx.execute("DELETE FROM memories_fts WHERE memory_id = ?1", params![m.id])?;
            tx.execute(
                "INSERT INTO memories_fts (content, tags, memory_id) VALUES (?1, ?2, ?3)",
                params![m.content, m.tags, m.id],
            )?;
            Ok(())
        })
    }

    pub fn get_memory(&self, id: &str) -> Result<Option<MemoryRow>> {
        self.db.with(|c| c.query_row("SELECT * FROM memories WHERE id = ?1", params![id], memory_from_row).optional())
    }

    pub fn invalidate_memory(&self, id: &str, reason: &str) -> Result<()> {
        self.db.with(|c| {
            c.execute(
                "UPDATE memories SET invalidated = 1, invalidation_reason = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, reason, now_text()],
            )?;
            Ok(())
        })
    }

    pub fn list_memories(&self, filter: &MemoryFilter) -> Result<Vec<MemoryRow>> {
        let mut sql = String::from("SELECT * FROM memories WHERE 1 = 1");
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(repo) = &filter.repository_id {
            args.push(Box::new(repo.clone()));
            sql.push_str(&format!(" AND repository_id = ?{}", args.len()));
        }
        if let Some(task) = &filter.task_id {
            args.push(Box::new(task.clone()));
            sql.push_str(&format!(" AND task_id = ?{}", args.len()));
        }
        if let Some(cat) = &filter.category {
            args.push(Box::new(cat.clone()));
            sql.push_str(&format!(" AND category = ?{}", args.len()));
        }
        if !filter.include_invalidated {
            sql.push_str(" AND invalidated = 0");
        }
        sql.push_str(" ORDER BY updated_at DESC");
        if filter.limit > 0 {
            args.push(Box::new(filter.limit as i64));
            sql.push_str(&format!(" LIMIT ?{}", args.len()));
        }
        self.db.with(|c| {
            let mut stmt = c.prepare(&sql)?;
            let refs: Vec<&dyn rusqlite::ToSql> = args.iter().map(|a| a.as_ref()).collect();
            let rows = stmt.query_map(refs.as_slice(), memory_from_row)?;
            rows.collect()
        })
    }

    /// Lexical search over memories; returns (row, bm25 score).
    pub fn search_memories(&self, terms: &[String], repository_id: &str, limit: usize) -> Result<Vec<(MemoryRow, f64)>> {
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let expr = terms.iter().map(|t| fts_quote(t)).collect::<Vec<_>>().join(" OR ");
        self.db.with(|c| {
            let mut stmt = c.prepare(
                "SELECT m.*, bm25(memories_fts, 1.0, 2.0) AS score FROM memories_fts
                 JOIN memories m ON m.id = memories_fts.memory_id
                 WHERE memories_fts MATCH ?1 AND m.repository_id = ?2 AND m.invalidated = 0
                 ORDER BY score LIMIT ?3",
            )?;
            let rows = stmt.query_map(params![expr, repository_id, limit as i64], |r| {
                let row = memory_from_row(r)?;
                let score: f64 = r.get("score")?;
                Ok((row, -score))
            })?;
            rows.collect()
        })
    }

    /// Memories whose evidence references a path (for invalidation).
    pub fn memories_referencing_path(&self, repository_id: &str, path: &str) -> Result<Vec<MemoryRow>> {
        let escaped = path.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
        let needle = format!("%\"path\":\"{escaped}\"%");
        self.db.with(|c| {
            let mut stmt = c.prepare(
                "SELECT * FROM memories WHERE repository_id = ?1 AND invalidated = 0 AND evidence LIKE ?2 ESCAPE '\\'",
            )?;
            let rows = stmt.query_map(params![repository_id, needle], memory_from_row)?;
            rows.collect()
        })
    }

    // ----- checkpoints ----------------------------------------------------

    pub fn insert_checkpoint(&self, cp: &CheckpointRecord) -> Result<()> {
        self.db.with(|c| {
            c.execute(
                "INSERT INTO checkpoints (id, task_id, session_id, reason, summary_json, event_seq, conversation_seq, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    cp.id,
                    cp.task_id,
                    cp.session_id,
                    cp.reason,
                    cp.summary.to_string(),
                    cp.event_seq,
                    cp.conversation_seq,
                    cp.created_at.to_rfc3339()
                ],
            )?;
            Ok(())
        })
    }

    pub fn latest_checkpoint(&self, task_id: &str) -> Result<Option<CheckpointRecord>> {
        self.db.with(|c| {
            c.query_row(
                "SELECT * FROM checkpoints WHERE task_id = ?1 ORDER BY created_at DESC, rowid DESC LIMIT 1",
                params![task_id],
                checkpoint_from_row,
            )
            .optional()
        })
    }

    pub fn checkpoints_for_task(&self, task_id: &str) -> Result<Vec<CheckpointRecord>> {
        self.db.with(|c| {
            let mut stmt = c.prepare("SELECT * FROM checkpoints WHERE task_id = ?1 ORDER BY rowid ASC")?;
            let rows = stmt.query_map(params![task_id], checkpoint_from_row)?;
            rows.collect()
        })
    }

    // ----- model requests -------------------------------------------------

    pub fn insert_model_request(&self, r: &ModelRequestRecord) -> Result<i64> {
        self.db.with(|c| {
            c.execute(
                "INSERT INTO model_requests (task_id, session_id, step, provider, model, prompt_tokens, completion_tokens, cached_tokens, usage_source, latency_ms, finish_reason, error, created_at, cost_usd, gateway_request_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                params![
                    r.task_id,
                    r.session_id,
                    r.step as i64,
                    r.provider,
                    r.model,
                    r.prompt_tokens.map(|n| n as i64),
                    r.completion_tokens.map(|n| n as i64),
                    r.cached_tokens.map(|n| n as i64),
                    r.usage_source,
                    r.latency_ms as i64,
                    r.finish_reason,
                    r.error,
                    r.created_at.to_rfc3339(),
                    r.cost_usd,
                    r.gateway_request_id
                ],
            )?;
            Ok(c.last_insert_rowid())
        })
    }

    pub fn model_requests_for_task(&self, task_id: &str) -> Result<Vec<ModelRequestRecord>> {
        self.db.with(|c| {
            let mut stmt = c.prepare("SELECT * FROM model_requests WHERE task_id = ?1 ORDER BY id ASC")?;
            let rows = stmt.query_map(params![task_id], model_request_from_row)?;
            rows.collect()
        })
    }

    pub fn usage_totals(&self, task_id: Option<&str>, session_id: Option<&str>) -> Result<UsageTotals> {
        self.db.with(|c| {
            let (filter, arg) = match (task_id, session_id) {
                (Some(t), _) => ("task_id = ?1", t.to_string()),
                (None, Some(s)) => ("session_id = ?1", s.to_string()),
                (None, None) => ("?1 = ?1", "1".to_string()),
            };
            let sql = format!(
                "SELECT COUNT(*), COALESCE(SUM(prompt_tokens),0), COALESCE(SUM(completion_tokens),0), COALESCE(SUM(cached_tokens),0),
                        COALESCE(SUM(CASE WHEN usage_source = 'reported' THEN 1 ELSE 0 END),0),
                        COALESCE(SUM(CASE WHEN usage_source = 'estimated' THEN 1 ELSE 0 END),0),
                        COALESCE(SUM(latency_ms),0), COALESCE(SUM(CASE WHEN error IS NOT NULL THEN 1 ELSE 0 END),0),
                        COALESCE(SUM(cost_usd),0), COUNT(cost_usd)
                 FROM model_requests WHERE {filter}"
            );
            c.query_row(&sql, params![arg], |r| {
                Ok(UsageTotals {
                    requests: r.get::<_, i64>(0)? as u64,
                    prompt_tokens: r.get::<_, i64>(1)? as u64,
                    completion_tokens: r.get::<_, i64>(2)? as u64,
                    cached_tokens: r.get::<_, i64>(3)? as u64,
                    reported_requests: r.get::<_, i64>(4)? as u64,
                    estimated_requests: r.get::<_, i64>(5)? as u64,
                    total_latency_ms: r.get::<_, i64>(6)? as u64,
                    errors: r.get::<_, i64>(7)? as u64,
                    cost_usd: r.get::<_, f64>(8)?,
                    priced_requests: r.get::<_, i64>(9)? as u64,
                })
            })
        })
    }

    /// Replaces the estimate of the request ModelGateway knows as
    /// `gateway_request_id` with what it billed; the number of rows changed.
    pub fn set_billed_cost(&self, gateway_request_id: &str, cost_usd: f64) -> Result<usize> {
        self.db.with(|c| c.execute("UPDATE model_requests SET cost_usd = ?2 WHERE gateway_request_id = ?1", params![gateway_request_id, cost_usd]))
    }

    // ----- validation -----------------------------------------------------

    pub fn insert_validation_run(&self, v: &ValidationRunRecord) -> Result<i64> {
        self.db.with(|c| {
            c.execute(
                "INSERT INTO validation_runs (task_id, command, passed, exit_code, duration_ms, summary, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    v.task_id,
                    v.command,
                    v.passed as i64,
                    v.exit_code,
                    v.duration_ms as i64,
                    v.summary,
                    v.created_at.to_rfc3339()
                ],
            )?;
            Ok(c.last_insert_rowid())
        })
    }

    pub fn validation_runs_for_task(&self, task_id: &str) -> Result<Vec<ValidationRunRecord>> {
        self.db.with(|c| {
            let mut stmt = c.prepare("SELECT * FROM validation_runs WHERE task_id = ?1 ORDER BY id ASC")?;
            let rows = stmt.query_map(params![task_id], validation_from_row)?;
            rows.collect()
        })
    }

    // ----- metrics --------------------------------------------------------

    pub fn insert_metric(&self, m: &MetricRecord) -> Result<()> {
        self.db.with(|c| {
            c.execute(
                "INSERT INTO metrics (task_id, name, value, unit, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![m.task_id, m.name, m.value, m.unit, m.created_at.to_rfc3339()],
            )?;
            Ok(())
        })
    }

    pub fn metrics_for_task(&self, task_id: &str) -> Result<Vec<MetricRecord>> {
        self.db.with(|c| {
            let mut stmt = c.prepare("SELECT task_id, name, value, unit, created_at FROM metrics WHERE task_id = ?1 ORDER BY id")?;
            let rows = stmt.query_map(params![task_id], |r| {
                Ok(MetricRecord {
                    task_id: r.get(0)?,
                    name: r.get(1)?,
                    value: r.get(2)?,
                    unit: r.get(3)?,
                    created_at: parse_ts(&r.get::<_, String>(4)?),
                })
            })?;
            rows.collect()
        })
    }

    pub fn database(&self) -> &Database {
        &self.db
    }
}

// Keep the json helpers referenced so they are available to downstream users.
#[allow(dead_code)]
fn _json_helpers_used() {
    let _ = to_json::<i32>;
    let _ = from_json::<i32>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use ctx_core::now;

    fn session() -> SessionRecord {
        SessionRecord {
            id: "s1".into(),
            repository_id: "r1".into(),
            mode: "interactive".into(),
            model: "m".into(),
            active_task_id: None,
            created_at: now(),
            updated_at: now(),
            metadata: serde_json::json!({}),
        }
    }

    fn task(id: &str) -> TaskRecord {
        TaskRecord {
            id: id.into(),
            session_id: "s1".into(),
            repository_id: "r1".into(),
            objective: "Change the button".into(),
            category: "localized_edit".into(),
            state: "IDLE".into(),
            status: None,
            parent_task_id: None,
            summary: None,
            state_json: serde_json::json!({}),
            created_at: now(),
            updated_at: now(),
            completed_at: None,
        }
    }

    #[test]
    fn sessions_tasks_and_events_round_trip() {
        let store = SessionStore::in_memory().unwrap();
        store.create_session(&session()).unwrap();
        store.create_task(&task("t1")).unwrap();
        store.set_active_task("s1", Some("t1")).unwrap();
        assert_eq!(store.get_session("s1").unwrap().unwrap().active_task_id.as_deref(), Some("t1"));
        let id = store.append_event(Some("s1"), Some("t1"), "state_changed", &serde_json::json!({"to": "PREPARING"})).unwrap();
        assert!(id > 0);
        let events = store.events_for_task("t1", 0, 10).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, "state_changed");
        assert!(store.find_task("t").unwrap().is_some());
    }

    #[test]
    fn memory_search_and_invalidation() {
        let store = SessionStore::in_memory().unwrap();
        let m = MemoryRow {
            id: "m1".into(),
            repository_id: "r1".into(),
            task_id: Some("t1".into()),
            category: "project_knowledge".into(),
            content: "The primary button lives in src/components/Button.tsx".into(),
            evidence: serde_json::json!([{"kind":"source","path":"src/components/Button.tsx"}]),
            verification: "source_derived".into(),
            tags: "button component".into(),
            created_at: now(),
            updated_at: now(),
            invalidated: false,
            invalidation_reason: None,
        };
        store.insert_memory(&m).unwrap();
        let hits = store.search_memories(&["button".into()], "r1", 5).unwrap();
        assert_eq!(hits.len(), 1);
        let refs = store.memories_referencing_path("r1", "src/components/Button.tsx").unwrap();
        assert_eq!(refs.len(), 1);
        store.invalidate_memory("m1", "file changed").unwrap();
        assert!(store.search_memories(&["button".into()], "r1", 5).unwrap().is_empty());
        let all = store.list_memories(&MemoryFilter { repository_id: Some("r1".into()), include_invalidated: true, ..Default::default() }).unwrap();
        assert!(all[0].invalidated);
    }

    #[test]
    fn usage_totals_distinguish_sources() {
        let store = SessionStore::in_memory().unwrap();
        for (src, p) in [("reported", 100), ("estimated", 50)] {
            store
                .insert_model_request(&ModelRequestRecord {
                    id: None,
                    task_id: Some("t1".into()),
                    session_id: Some("s1".into()),
                    step: 1,
                    provider: "openai_compatible".into(),
                    model: "m".into(),
                    prompt_tokens: Some(p),
                    completion_tokens: Some(10),
                    cached_tokens: Some(20),
                    usage_source: Some(src.into()),
                    latency_ms: 5,
                    finish_reason: Some("stop".into()),
                    error: None,
                    created_at: now(),
                    cost_usd: (src == "reported").then_some(0.25),
                    gateway_request_id: Some(format!("gw-{src}")),
                })
                .unwrap();
        }
        let totals = store.usage_totals(Some("t1"), None).unwrap();
        assert_eq!(totals.requests, 2);
        assert_eq!(totals.prompt_tokens, 150);
        assert_eq!(totals.reported_requests, 1);
        assert_eq!(totals.estimated_requests, 1);
        assert_eq!(totals.cost(), Some(0.25));
        assert_eq!(store.set_billed_cost("gw-estimated", 0.5).unwrap(), 1);
        assert_eq!(store.usage_totals(Some("t1"), None).unwrap().cost(), Some(0.75));
    }

    #[test]
    fn recent_user_messages_lists_typed_text_newest_first() {
        let store = SessionStore::in_memory().unwrap();
        store.append_message("s1", None, &serde_json::json!({"kind": "user", "text": "first"})).unwrap();
        store.append_message("s1", None, &serde_json::json!({"kind": "assistant", "text": "reply"})).unwrap();
        store.append_message("s2", None, &serde_json::json!({"kind": "user", "text": "second"})).unwrap();
        assert_eq!(store.recent_user_messages(10).unwrap(), vec!["second".to_string(), "first".to_string()]);
        assert_eq!(store.recent_user_messages(1).unwrap(), vec!["second".to_string()]);
    }
}
