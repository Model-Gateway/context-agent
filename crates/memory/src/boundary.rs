//! Task boundary detection (spec §9.4).
//!
//! Deterministic heuristics decide whether a new message continues,
//! refines, reverses, or starts a task, or refers to an earlier one. When
//! uncertain the decision preserves continuity.

use crate::keywords::extract_keywords;
use crate::taskstate::TaskState;
use ctx_core::TaskId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskBoundary {
    /// Continue the active (incomplete) task.
    Continue,
    /// Adjust the active task's outcome ("make it darker").
    Refine,
    /// Start a distinct task.
    NewTask,
    /// Refers to an earlier completed task.
    ReferEarlier { task_id: TaskId },
    /// Explicitly undo previous work.
    Reverse { task_id: Option<TaskId> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoundaryDecision {
    pub boundary: TaskBoundary,
    /// 0.0–1.0
    pub confidence: f64,
    pub rationale: String,
}

/// Minimal view of an earlier task for reference matching.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskSummary {
    pub task_id: TaskId,
    pub objective: String,
    pub keywords: Vec<String>,
    pub completed: bool,
    pub modified_files: Vec<String>,
}

/// Reversal is only recognised when the message *starts* with the cue
/// (first three words) or uses an explicit phrase, so "add undo/redo
/// support" is a feature request, not a reversal.
const REVERSE_LEADING: &[&str] = &["undo", "revert", "rollback", "discard"];
const REVERSE_PHRASES: &[&str] = &["roll back", "put it back", "put that back", "go back to how", "restore the original", "never mind", "nevermind", "scrap that", "revert the", "undo the", "undo that", "revert that"];
const EARLIER_CUES: &[&str] = &["earlier", "previous", "previously", "before that", "the one you", "last time", "you did", "you changed", "you added", "you made", "that change you", "the earlier", "the other day", "yesterday", "first task", "original task", "back in"];
const ANAPHORA: &[&str] = &["it", "that", "this", "those", "these", "them", "there", "same", "one"];
const CONTINUATION_CUES: &[&str] = &["also", "too", "as well", "instead", "actually", "and then", "additionally", "plus", "on top of that", "while you're at it", "while you are at it", "same for", "do the same", "darker", "lighter", "bigger", "smaller", "larger", "shorter", "longer", "faster", "slower", "more", "less", "again", "another", "other", "rest", "remaining", "still", "try again", "retry", "keep going", "continue", "go on", "next", "finish", "done?", "what about"];
/// Problem reports only continue a task that is still in progress.
const PROBLEM_CUES: &[&str] = &["doesn't work", "does not work", "didn't work", "not working", "failing", "broke", "broken", "crashes", "error"];

fn contains_any<'a>(text: &str, cues: &[&'a str]) -> Option<&'a str> {
    cues.iter().copied().find(|c| contains_word(text, c))
}

/// Substring match on word boundaries (`undo` does not match `undoable`).
fn contains_word(text: &str, cue: &str) -> bool {
    let mut start = 0;
    while let Some(pos) = text[start..].find(cue) {
        let abs = start + pos;
        let before_ok = abs == 0 || !text[..abs].chars().last().map(|c| c.is_alphanumeric()).unwrap_or(false);
        let after_ok = abs + cue.len() >= text.len() || !text[abs + cue.len()..].chars().next().map(|c| c.is_alphanumeric()).unwrap_or(false);
        if before_ok && after_ok {
            return true;
        }
        start = abs + cue.len();
    }
    false
}

fn reversal_cue<'a>(lower: &str, words: &[&str]) -> Option<&'a str> {
    let leading = words.iter().take(3).map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()));
    for w in leading {
        if let Some(c) = REVERSE_LEADING.iter().find(|c| **c == w) {
            return Some(c);
        }
    }
    REVERSE_PHRASES.iter().copied().find(|c| contains_word(lower, c))
}

fn word_set(text: &str) -> Vec<String> {
    extract_keywords(text)
}

fn overlap(a: &[String], b: &[String]) -> usize {
    a.iter().filter(|w| b.contains(w)).count()
}

/// Decide how `message` relates to the active task and recent history.
pub fn classify_boundary(message: &str, active: Option<&TaskState>, active_completed: bool, recent: &[TaskSummary]) -> BoundaryDecision {
    let lower = message.trim().to_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();
    // Keywords come from the original text so camelCase identifiers split.
    let keywords = word_set(message.trim());
    let continuation_cue = contains_any(&lower, CONTINUATION_CUES).or_else(|| if active_completed { None } else { contains_any(&lower, PROBLEM_CUES) });
    let short = words.len() <= 7;
    let has_anaphora = words.iter().any(|w| ANAPHORA.contains(&w.trim_matches(|c: char| !c.is_alphanumeric())));

    // 1. Explicit reversal.
    if let Some(cue) = reversal_cue(&lower, &words) {
        let target = best_match(&keywords, recent).map(|t| t.task_id.clone()).or_else(|| active.map(|a| a.task_id.clone()));
        return BoundaryDecision { boundary: TaskBoundary::Reverse { task_id: target }, confidence: 0.85, rationale: format!("reversal cue `{cue}`") };
    }

    // 2. Explicit reference to an earlier task (not the active one).
    if let Some(cue) = contains_any(&lower, EARLIER_CUES) {
        let candidates: Vec<&TaskSummary> = recent.iter().filter(|t| active.map(|a| a.task_id != t.task_id).unwrap_or(true)).collect();
        if let Some(best) = best_match_in(&keywords, &candidates) {
            return BoundaryDecision { boundary: TaskBoundary::ReferEarlier { task_id: best.task_id.clone() }, confidence: 0.75, rationale: format!("earlier-task cue `{cue}` with keyword overlap") };
        }
        if let Some(best) = candidates.first().filter(|_| has_anaphora || short) {
            return BoundaryDecision { boundary: TaskBoundary::ReferEarlier { task_id: best.task_id.clone() }, confidence: 0.5, rationale: format!("earlier-task cue `{cue}`; most recent other task assumed") };
        }
    }

    let Some(active) = active else {
        // No active task: a keyword match against a recent task combined with
        // anaphoric or comparative phrasing refers back to it; otherwise the
        // message is new work.
        if let Some(best) = best_match(&keywords, recent) {
            let ov = overlap(&keywords, &best.keywords);
            if ov >= 2 || (ov >= 1 && (has_anaphora || continuation_cue.is_some())) {
                return BoundaryDecision { boundary: TaskBoundary::ReferEarlier { task_id: best.task_id.clone() }, confidence: 0.55, rationale: "keyword overlap with a recent task and follow-up phrasing".into() };
            }
        }
        return BoundaryDecision { boundary: TaskBoundary::NewTask, confidence: 0.9, rationale: "no active task".into() };
    };

    let active_overlap = overlap(&keywords, &active.keywords) + overlap(&keywords, &active.modified_files.iter().flat_map(|f| extract_keywords(f)).collect::<Vec<_>>());

    // 3. Strong continuity signals.
    if active_overlap >= 2 || (active_overlap >= 1 && (has_anaphora || short || continuation_cue.is_some())) {
        let boundary = if active_completed { TaskBoundary::Refine } else { TaskBoundary::Continue };
        return BoundaryDecision { boundary, confidence: 0.85, rationale: format!("{active_overlap} keyword(s) overlap the active task") };
    }
    if has_anaphora && short && keywords.len() <= 3 {
        let boundary = if active_completed { TaskBoundary::Refine } else { TaskBoundary::Continue };
        return BoundaryDecision { boundary, confidence: 0.7, rationale: "short anaphoric follow-up (e.g. `make it darker`)".into() };
    }
    if let Some(cue) = continuation_cue {
        if !active_completed || short {
            let boundary = if active_completed { TaskBoundary::Refine } else { TaskBoundary::Continue };
            return BoundaryDecision { boundary, confidence: 0.6, rationale: format!("continuation cue `{cue}`") };
        }
    }

    // 4. Distinct subject with no overlap.
    if !active_completed && keywords.len() <= 2 && !has_anaphora {
        // Very short, non-anaphoric, no overlap: ambiguous → keep continuity.
        return BoundaryDecision { boundary: TaskBoundary::Continue, confidence: 0.4, rationale: "ambiguous short message; preserving continuity".into() };
    }
    if let Some(best) = best_match(&keywords, recent) {
        if best.task_id != active.task_id && overlap(&keywords, &best.keywords) >= 2 {
            return BoundaryDecision { boundary: TaskBoundary::ReferEarlier { task_id: best.task_id.clone() }, confidence: 0.6, rationale: "keywords match an earlier task better than the active one".into() };
        }
    }
    BoundaryDecision { boundary: TaskBoundary::NewTask, confidence: if keywords.len() >= 3 { 0.8 } else { 0.55 }, rationale: "new subject with no overlap with the active task".into() }
}

fn best_match<'a>(keywords: &[String], recent: &'a [TaskSummary]) -> Option<&'a TaskSummary> {
    recent.iter().filter(|t| overlap(keywords, &t.keywords) >= 1).max_by_key(|t| overlap(keywords, &t.keywords))
}

fn best_match_in<'a>(keywords: &[String], recent: &[&'a TaskSummary]) -> Option<&'a TaskSummary> {
    recent.iter().copied().filter(|t| overlap(keywords, &t.keywords) >= 1).max_by_key(|t| overlap(keywords, &t.keywords))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ctx_core::TaskCategory;

    fn active(objective: &str) -> TaskState {
        let mut s = TaskState::new(TaskId::from("t-active"), objective, TaskCategory::LocalizedEdit);
        s.note_file_modified("src/components/Button.tsx");
        s
    }

    fn summary(id: &str, objective: &str, files: &[&str]) -> TaskSummary {
        TaskSummary { task_id: TaskId::from(id), objective: objective.into(), keywords: extract_keywords(objective), completed: true, modified_files: files.iter().map(|f| f.to_string()).collect() }
    }

    #[test]
    fn spec_examples() {
        let a = active("Change the primary button to blue");
        let d = classify_boundary("Make it darker.", Some(&a), true, &[]);
        assert_eq!(d.boundary, TaskBoundary::Refine, "{d:?}");
        let d = classify_boundary("Update the footer copyright.", Some(&a), true, &[]);
        assert_eq!(d.boundary, TaskBoundary::NewTask, "{d:?}");
    }

    #[test]
    fn continues_incomplete_task_on_overlap() {
        let a = active("Add a loading spinner to the login button");
        let d = classify_boundary("The spinner should only show while isLoading is true", Some(&a), false, &[]);
        assert_eq!(d.boundary, TaskBoundary::Continue);
        let d = classify_boundary("tests are failing", Some(&a), false, &[]);
        assert_eq!(d.boundary, TaskBoundary::Continue);
    }

    #[test]
    fn reversal_and_earlier_reference() {
        let a = active("Update the footer copyright");
        let earlier = summary("t-1", "Change the primary button to blue", &["src/components/Button.tsx"]);
        let d = classify_boundary("Actually, revert the button colour change", Some(&a), true, &[earlier.clone()]);
        assert!(matches!(d.boundary, TaskBoundary::Reverse { task_id: Some(ref t) } if t.as_str() == "t-1"), "{d:?}");
        let d = classify_boundary("Earlier you changed the button colour; make that one green", Some(&a), true, &[earlier.clone()]);
        assert!(matches!(d.boundary, TaskBoundary::ReferEarlier { ref task_id } if task_id.as_str() == "t-1"), "{d:?}");
        let d = classify_boundary("Make the button darker", None, false, &[earlier]);
        assert!(matches!(d.boundary, TaskBoundary::ReferEarlier { .. }), "{d:?}");
    }

    #[test]
    fn feature_requests_mentioning_undo_are_not_reversals() {
        let a = active("Implement the rich text editor");
        let d = classify_boundary("Add undo/redo support to the editor", Some(&a), false, &[]);
        assert!(!matches!(d.boundary, TaskBoundary::Reverse { .. }), "{d:?}");
        let d = classify_boundary("Explain the error handling in main.rs", Some(&a), true, &[]);
        assert_eq!(d.boundary, TaskBoundary::NewTask, "{d:?}");
        let d = classify_boundary("Revert that editor change", Some(&a), true, &[]);
        assert!(matches!(d.boundary, TaskBoundary::Reverse { .. }), "{d:?}");
    }

    #[test]
    fn new_task_without_active() {
        let d = classify_boundary("Refactor the authentication module to use async handlers", None, false, &[]);
        assert_eq!(d.boundary, TaskBoundary::NewTask);
    }
}
