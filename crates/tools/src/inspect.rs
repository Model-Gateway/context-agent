//! Memory and context inspection tools: retrieve_memory, inspect_context.

use crate::registry::{Tool, ToolContext, ToolOutput};
use async_trait::async_trait;
use ctx_core::{content_hash, Result, TaskId};
use ctx_memory::MemoryCategory;
use ctx_security::{Action, ActionKind};
use serde::Deserialize;
use serde_json::{json, Value};

pub struct RetrieveMemory;

#[derive(Deserialize)]
struct MemoryArgs {
    query: Option<String>,
    category: Option<String>,
    task_id: Option<String>,
    #[serde(default)]
    recent_tasks: bool,
    limit: Option<usize>,
}

#[async_trait]
impl Tool for RetrieveMemory {
    fn name(&self) -> &str {
        "retrieve_memory"
    }

    fn description(&self) -> &str {
        "Recall knowledge from earlier work: search project knowledge and past task summaries by query, list recent tasks, or fetch a specific earlier task's state. Memory records show their verification level; treat hypotheses with care."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "category": {"type": "string", "enum": ["task", "project_knowledge", "episodic"]},
                "task_id": {"type": "string", "description": "Fetch the stored state of this earlier task (id or prefix)"},
                "recent_tasks": {"type": "boolean", "description": "List recent tasks in this repository"},
                "limit": {"type": "integer", "minimum": 1, "maximum": 50}
            },
            "additionalProperties": false
        })
    }

    fn action(&self, _args: &Value, _ctx: &ToolContext) -> Result<Action> {
        Ok(Action { tool: "retrieve_memory".into(), kind: ActionKind::MemoryRead, summary: "retrieve memory".into(), details: String::new() })
    }

    async fn invoke(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let a: MemoryArgs = serde_json::from_value(args)?;
        let Some(memory) = &ctx.memory else {
            return Ok(ToolOutput::error("memory is not available in this session"));
        };
        let limit = a.limit.unwrap_or(10);
        let mut out = String::new();
        if let Some(id) = &a.task_id {
            let store = memory.session_store();
            match store.find_task(id)? {
                Some(task) => {
                    out.push_str(&format!("Task {} [{}] {}\nObjective: {}\n", task.id, task.status.clone().unwrap_or_else(|| task.state.clone()), task.updated_at.format("%Y-%m-%d %H:%M"), task.objective));
                    if let Some(state) = memory.load_task_state(&TaskId::from_string(task.id.clone()))? {
                        out.push_str(&state.checkpoint_summary());
                    }
                    if let Some(summary) = task.summary {
                        out.push_str(&format!("Final report: {summary}\n"));
                    }
                }
                None => out.push_str(&format!("No task matches `{id}`.\n")),
            }
        }
        if a.recent_tasks {
            let tasks = memory.session_store().list_tasks(memory.repository_id().as_str(), limit)?;
            out.push_str(&format!("Recent tasks ({}):\n", tasks.len()));
            for t in tasks.iter().filter(|t| t.id != ctx.task_id.as_str()) {
                let files = t.state_json.get("modified_files").and_then(|f| f.as_array()).map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default();
                out.push_str(&format!("- {} [{}] {} — {}{}\n", &t.id[..8.min(t.id.len())], t.status.clone().unwrap_or_else(|| t.state.clone()), t.updated_at.format("%Y-%m-%d"), t.objective.chars().take(100).collect::<String>(), if files.is_empty() { String::new() } else { format!(" (files: {files})") }));
            }
        }
        if let Some(query) = &a.query {
            let hits = memory.search(query, limit, false)?;
            out.push_str(&format!("Memory search for \"{query}\": {} record(s)\n", hits.len()));
            for (m, score) in hits {
                let evidence = m.evidence_paths().collect::<Vec<_>>().join(", ");
                out.push_str(&format!("- [{} | {} | {:.2}] {}{}\n", m.category.as_str(), m.verification.as_str(), score, m.content, if evidence.is_empty() { String::new() } else { format!(" (evidence: {evidence})") }));
            }
        } else if let Some(cat) = &a.category {
            let category = MemoryCategory::parse(cat).unwrap_or(MemoryCategory::ProjectKnowledge);
            let records = memory.list(Some(category), None, false, limit)?;
            out.push_str(&format!("{} {} record(s):\n", records.len(), cat));
            for m in records {
                out.push_str(&format!("- [{}] {}\n", m.verification.as_str(), m.content));
            }
        }
        if out.is_empty() {
            out.push_str("Specify query, category, task_id or recent_tasks.");
        }
        Ok(ToolOutput::ok(out, "retrieved memory"))
    }
}

pub struct InspectContext;

#[derive(Deserialize)]
struct InspectArgs {
    what: Option<String>,
}

#[async_trait]
impl Tool for InspectContext {
    fn name(&self) -> &str {
        "inspect_context"
    }

    fn description(&self) -> &str {
        "Inspect how your context was assembled: which sources were selected and why, what was excluded, budget use (what=explain|sources|stats), or whether files in your working set changed on disk (what=freshness)."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"what": {"type": "string", "enum": ["explain", "sources", "stats", "freshness"]}},
            "additionalProperties": false
        })
    }

    fn action(&self, _args: &Value, _ctx: &ToolContext) -> Result<Action> {
        Ok(Action { tool: "inspect_context".into(), kind: ActionKind::MemoryRead, summary: "inspect context".into(), details: String::new() })
    }

    async fn invoke(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let a: InspectArgs = serde_json::from_value(args)?;
        let what = a.what.as_deref().unwrap_or("explain");
        if what == "freshness" {
            let mut out = String::from("Working-set freshness:\n");
            let files = ctx.working_set.read().clone();
            for rel in files {
                let abs = ctx.root.join(&rel);
                let indexed = ctx.indexer.as_ref().and_then(|i| i.store().get_file(&rel).ok().flatten());
                let status = match (std::fs::read(&abs), indexed) {
                    (Err(_), _) => "MISSING on disk".to_string(),
                    (Ok(bytes), Some(rec)) => {
                        if content_hash(&bytes) == rec.hash { "unchanged since indexing".into() } else { "CHANGED since indexing (re-read before editing)".into() }
                    }
                    (Ok(_), None) => "not in the index".into(),
                };
                out.push_str(&format!("- {rel}: {status}\n"));
            }
            return Ok(ToolOutput::ok(out, "freshness"));
        }
        let guard = ctx.context_explanation.read();
        let Some(explanation) = guard.as_ref() else {
            return Ok(ToolOutput::ok("No context explanation is available yet.", "no explanation"));
        };
        let text = match what {
            "sources" => explanation.render_sources(),
            "stats" => explanation.render_stats(),
            _ => explanation.render_explain(),
        };
        Ok(ToolOutput::ok(text, format!("context {what}")))
    }
}
