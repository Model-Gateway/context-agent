//! Task memory: the structured state of one task (spec §8.6, §9.1).

use crate::keywords::extract_keywords;
use ctx_core::{SourceRef, TaskCategory, TaskId, ValidationRecord};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStepStatus {
    Pending,
    InProgress,
    Done,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanStep {
    pub description: String,
    pub status: PlanStepStatus,
}

/// Everything a compacted checkpoint must retain (spec §8.6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskState {
    pub task_id: TaskId,
    pub objective: String,
    pub category: TaskCategory,
    /// Requirements and constraints stated by the user or discovered.
    #[serde(default)]
    pub requirements: Vec<String>,
    /// Important user decisions ("use the existing Spinner", "no new deps").
    #[serde(default)]
    pub decisions: Vec<String>,
    #[serde(default)]
    pub plan: Vec<PlanStep>,
    #[serde(default)]
    pub completed_actions: Vec<String>,
    #[serde(default)]
    pub modified_files: Vec<String>,
    /// Files read or edited during the task; drives retrieval boosts.
    #[serde(default)]
    pub working_set: Vec<String>,
    #[serde(default)]
    pub validation: Vec<ValidationRecord>,
    #[serde(default)]
    pub unresolved: Vec<String>,
    #[serde(default)]
    pub source_refs: Vec<SourceRef>,
    #[serde(default)]
    pub remaining_work: Vec<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    /// Number of checkpoints taken.
    #[serde(default)]
    pub checkpoints: u32,
    /// Recovery cycles consumed.
    #[serde(default)]
    pub recovery_cycles: u32,
    /// Conversation sequence id before which history was compacted.
    #[serde(default)]
    pub compacted_before: i64,
    /// Files were modified after the last fully successful validation.
    #[serde(default)]
    pub unvalidated_changes: bool,
    /// Whether the most recent validation batch passed in full.
    #[serde(default)]
    pub last_validation_batch_passed: Option<bool>,
}

impl TaskState {
    pub fn new(task_id: TaskId, objective: impl Into<String>, category: TaskCategory) -> Self {
        let objective = objective.into();
        let keywords = extract_keywords(&objective);
        Self {
            task_id,
            objective,
            category,
            requirements: Vec::new(),
            decisions: Vec::new(),
            plan: Vec::new(),
            completed_actions: Vec::new(),
            modified_files: Vec::new(),
            working_set: Vec::new(),
            validation: Vec::new(),
            unresolved: Vec::new(),
            source_refs: Vec::new(),
            remaining_work: Vec::new(),
            keywords,
            checkpoints: 0,
            recovery_cycles: 0,
            compacted_before: 0,
            unvalidated_changes: false,
            last_validation_batch_passed: None,
        }
    }

    fn push_unique(list: &mut Vec<String>, item: String) {
        if !list.contains(&item) {
            list.push(item);
        }
    }

    pub fn note_requirement(&mut self, text: impl Into<String>) {
        Self::push_unique(&mut self.requirements, text.into());
    }

    pub fn note_decision(&mut self, text: impl Into<String>) {
        Self::push_unique(&mut self.decisions, text.into());
    }

    pub fn note_action(&mut self, text: impl Into<String>) {
        let text = text.into();
        self.completed_actions.push(text);
        if self.completed_actions.len() > 60 {
            self.completed_actions.remove(0);
        }
    }

    pub fn note_file_modified(&mut self, path: impl Into<String>) {
        let path = path.into();
        Self::push_unique(&mut self.working_set, path.clone());
        Self::push_unique(&mut self.modified_files, path);
    }

    pub fn note_file_read(&mut self, path: impl Into<String>) {
        Self::push_unique(&mut self.working_set, path.into());
    }

    pub fn note_unresolved(&mut self, text: impl Into<String>) {
        Self::push_unique(&mut self.unresolved, text.into());
    }

    pub fn resolve(&mut self, text: &str) {
        self.unresolved.retain(|u| u != text);
    }

    pub fn note_validation(&mut self, record: ValidationRecord) {
        self.validation.push(record);
    }

    pub fn note_source(&mut self, source: SourceRef) {
        if !self.source_refs.iter().any(|s| s.path == source.path && s.start_byte == source.start_byte) {
            self.source_refs.push(source);
        }
        if self.source_refs.len() > 40 {
            self.source_refs.remove(0);
        }
    }

    /// Extend the objective with a refinement from a follow-up message.
    pub fn refine(&mut self, message: &str) {
        self.requirements.push(message.to_string());
        for k in extract_keywords(message) {
            Self::push_unique(&mut self.keywords, k);
        }
    }

    /// Outcome of the most recent validation batch (all commands of one
    /// run_validation call), falling back to the last record.
    pub fn last_validation_passed(&self) -> Option<bool> {
        self.last_validation_batch_passed.or_else(|| self.validation.last().map(|v| v.passed))
    }

    /// Render the checkpoint summary supplied to the model after compaction.
    pub fn checkpoint_summary(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("Objective: {}\n", self.objective));
        if !self.requirements.is_empty() {
            s.push_str("Requirements:\n");
            for r in &self.requirements {
                s.push_str(&format!("- {r}\n"));
            }
        }
        if !self.decisions.is_empty() {
            s.push_str("User decisions:\n");
            for d in &self.decisions {
                s.push_str(&format!("- {d}\n"));
            }
        }
        if !self.plan.is_empty() {
            s.push_str("Plan:\n");
            for step in &self.plan {
                let mark = match step.status {
                    PlanStepStatus::Done => "x",
                    PlanStepStatus::InProgress => ">",
                    PlanStepStatus::Pending => " ",
                    PlanStepStatus::Skipped => "-",
                };
                s.push_str(&format!("- [{mark}] {}\n", step.description));
            }
        }
        if !self.completed_actions.is_empty() {
            s.push_str("Completed actions (most recent last):\n");
            let start = self.completed_actions.len().saturating_sub(20);
            for a in &self.completed_actions[start..] {
                s.push_str(&format!("- {a}\n"));
            }
        }
        if !self.modified_files.is_empty() {
            s.push_str(&format!("Modified files: {}\n", self.modified_files.join(", ")));
        }
        match self.validation.last() {
            Some(v) => s.push_str(&format!("Latest validation: `{}` {} ({})\n", v.command, if v.passed { "PASSED" } else { "FAILED" }, v.summary)),
            None => s.push_str("Validation: not yet run\n"),
        }
        if !self.unresolved.is_empty() {
            s.push_str("Unresolved problems:\n");
            for u in &self.unresolved {
                s.push_str(&format!("- {u}\n"));
            }
        }
        if !self.source_refs.is_empty() {
            s.push_str("Relevant source references (re-read with read_file before editing):\n");
            for r in self.source_refs.iter().rev().take(12) {
                s.push_str(&format!("- {}\n", r.display_range()));
            }
        }
        if !self.remaining_work.is_empty() {
            s.push_str("Remaining work:\n");
            for w in &self.remaining_work {
                s.push_str(&format!("- {w}\n"));
            }
        }
        s
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }

    pub fn from_json(value: &serde_json::Value) -> Option<Self> {
        serde_json::from_value(value.clone()).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkpoint_summary_contains_required_fields() {
        let mut state = TaskState::new(TaskId::new(), "Change the primary button to blue", TaskCategory::LocalizedEdit);
        state.note_decision("keep the existing colour tokens");
        state.note_file_modified("src/Button.tsx");
        state.note_unresolved("snapshot test still failing");
        state.note_validation(ValidationRecord { command: "npm test".into(), passed: false, exit_code: Some(1), duration_ms: 10, summary: "1 failed".into() });
        state.remaining_work.push("update snapshot".into());
        let text = state.checkpoint_summary();
        for needle in ["Objective:", "User decisions:", "Modified files: src/Button.tsx", "FAILED", "Unresolved problems:", "Remaining work:"] {
            assert!(text.contains(needle), "missing {needle}: {text}");
        }
        let json = state.to_json();
        assert_eq!(TaskState::from_json(&json).unwrap(), state);
        assert!(state.keywords.contains(&"button".to_string()));
    }
}
