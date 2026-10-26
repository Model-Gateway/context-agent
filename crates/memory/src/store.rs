//! Typed memory operations over the session store (spec §9.5, §9.6).

use crate::keywords::extract_keywords;
use crate::taskstate::TaskState;
use crate::types::*;
use ctx_core::{MemoryId, RepositoryId, Result, TaskId, TaskOutcome};
use ctx_storage::{MemoryFilter, SessionStore};
use std::sync::Arc;

#[derive(Clone)]
pub struct MemoryStore {
    store: Arc<SessionStore>,
    repository_id: RepositoryId,
}

impl MemoryStore {
    pub fn new(store: Arc<SessionStore>, repository_id: RepositoryId) -> Self {
        Self { store, repository_id }
    }

    pub fn repository_id(&self) -> &RepositoryId {
        &self.repository_id
    }

    pub fn session_store(&self) -> &Arc<SessionStore> {
        &self.store
    }

    /// Insert a record. An existing current record with identical category
    /// and content is updated instead (its verification is upgraded if the
    /// new evidence is stronger; never downgraded).
    pub fn remember(&self, mut record: MemoryRecord) -> Result<MemoryId> {
        record.repository_id = self.repository_id.clone();
        if record.tags.is_empty() {
            record.tags = extract_keywords(&record.content).into_iter().take(16).collect();
        }
        let existing = self
            .store
            .list_memories(&MemoryFilter { repository_id: Some(self.repository_id.to_string()), category: Some(record.category.as_str().into()), limit: 0, ..Default::default() })?
            .into_iter()
            .map(MemoryRecord::from_row)
            .find(|m| m.content.trim() == record.content.trim());
        if let Some(mut found) = existing {
            if record.verification > found.verification {
                found.verification = record.verification;
            }
            for e in record.evidence {
                if !found.evidence.contains(&e) {
                    found.evidence.push(e);
                }
            }
            if found.task_id.is_none() {
                found.task_id = record.task_id;
            }
            found.updated_at = ctx_core::now();
            self.store.update_memory(&found.to_row())?;
            return Ok(found.id);
        }
        self.store.insert_memory(&record.to_row())?;
        Ok(record.id)
    }

    pub fn update(&self, record: &MemoryRecord) -> Result<()> {
        self.store.update_memory(&record.to_row())
    }

    pub fn get(&self, id: &MemoryId) -> Result<Option<MemoryRecord>> {
        Ok(self.store.get_memory(id.as_str())?.map(MemoryRecord::from_row))
    }

    pub fn list(&self, category: Option<MemoryCategory>, task_id: Option<&TaskId>, include_invalidated: bool, limit: usize) -> Result<Vec<MemoryRecord>> {
        let rows = self.store.list_memories(&MemoryFilter {
            repository_id: Some(self.repository_id.to_string()),
            task_id: task_id.map(|t| t.to_string()),
            category: category.map(|c| c.as_str().to_string()),
            include_invalidated,
            limit,
        })?;
        Ok(rows.into_iter().map(MemoryRecord::from_row).collect())
    }

    /// Relevance search: lexical match weighted by verification strength.
    /// Recency is only a tie-breaker (spec §9.5). Invalidated and
    /// hypothesis-level records are excluded unless `include_hypotheses`.
    pub fn search(&self, query: &str, limit: usize, include_hypotheses: bool) -> Result<Vec<(MemoryRecord, f64)>> {
        let terms = extract_keywords(query);
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let mut hits: Vec<(MemoryRecord, f64)> = self
            .store
            .search_memories(&terms, self.repository_id.as_str(), limit * 3)?
            .into_iter()
            .map(|(row, score)| {
                let rec = MemoryRecord::from_row(row);
                let weight = rec.verification.weight();
                // Exact keyword coverage bonus.
                let rec_terms = extract_keywords(&rec.content);
                let coverage = terms.iter().filter(|t| rec_terms.contains(t)).count() as f64 / terms.len() as f64;
                let s = score * weight * (0.5 + coverage);
                (rec, s)
            })
            .filter(|(rec, _)| include_hypotheses || rec.verification.is_trusted())
            .collect();
        hits.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then_with(|| b.0.updated_at.cmp(&a.0.updated_at)));
        hits.truncate(limit);
        Ok(hits)
    }

    pub fn invalidate(&self, id: &MemoryId, reason: &str) -> Result<()> {
        self.store.invalidate_memory(id.as_str(), reason)
    }

    /// Invalidate source-derived knowledge that references a changed file
    /// (spec §9.6). Records whose evidence hash still matches `current_hash`
    /// are kept.
    pub fn invalidate_for_path(&self, path: &str, current_hash: Option<&str>, reason: &str) -> Result<usize> {
        let mut count = 0;
        for row in self.store.memories_referencing_path(self.repository_id.as_str(), path)? {
            let rec = MemoryRecord::from_row(row);
            if rec.category == MemoryCategory::Episodic {
                continue; // history stays history
            }
            let still_valid = rec.evidence.iter().filter(|e| e.path.as_deref() == Some(path)).all(|e| match (&e.hash, current_hash) {
                (Some(h), Some(c)) => h == c,
                _ => false,
            });
            if !still_valid {
                self.store.invalidate_memory(rec.id.as_str(), reason)?;
                count += 1;
            }
        }
        Ok(count)
    }

    /// Invalidate all source-derived project knowledge after a branch or
    /// worktree switch; it must be re-verified against the new tree.
    pub fn invalidate_for_branch_change(&self, from: Option<&str>, to: Option<&str>) -> Result<usize> {
        let reason = format!("branch changed from {} to {}", from.unwrap_or("?"), to.unwrap_or("?"));
        let mut count = 0;
        for rec in self.list(Some(MemoryCategory::ProjectKnowledge), None, false, 0)? {
            if rec.evidence.iter().any(|e| e.kind == EvidenceKind::Source) {
                self.store.invalidate_memory(rec.id.as_str(), &reason)?;
                count += 1;
            }
        }
        Ok(count)
    }

    /// Promote a record's verification only when evidence of the required
    /// strength is attached (spec §9.3: no automatic promotion).
    pub fn promote(&self, id: &MemoryId, to: VerificationStatus, evidence: EvidenceReference) -> Result<bool> {
        let Some(mut rec) = self.get(id)? else { return Ok(false) };
        let allowed = match to {
            VerificationStatus::TestValidated => evidence.kind == EvidenceKind::Test,
            VerificationStatus::ToolConfirmed => matches!(evidence.kind, EvidenceKind::ToolResult | EvidenceKind::Test),
            VerificationStatus::SourceDerived => matches!(evidence.kind, EvidenceKind::Source | EvidenceKind::ToolResult | EvidenceKind::Test),
            VerificationStatus::UserProvided => evidence.kind == EvidenceKind::User,
            VerificationStatus::Hypothesis => true,
        };
        if !allowed || to <= rec.verification {
            return Ok(false);
        }
        rec.verification = to;
        rec.evidence.push(evidence);
        rec.updated_at = ctx_core::now();
        self.update(&rec)?;
        Ok(true)
    }

    /// Record an episodic memory summarizing a finished task (spec §9.1).
    pub fn record_episode(&self, state: &TaskState, outcome: &TaskOutcome) -> Result<MemoryId> {
        let validation = match outcome.validation.last() {
            Some(v) if v.passed => format!("validation passed (`{}`)", v.command),
            Some(v) => format!("validation FAILED (`{}`: {})", v.command, v.summary),
            None => "no validation run".to_string(),
        };
        let content = format!(
            "Task {} [{}]: {}. Modified files: {}. Outcome: {}. {}",
            state.task_id.short(),
            outcome.status.as_str(),
            state.objective,
            if state.modified_files.is_empty() { "none".to_string() } else { state.modified_files.join(", ") },
            outcome.summary.chars().take(400).collect::<String>(),
            validation
        );
        let verification = if outcome.validation.iter().any(|v| v.passed) { VerificationStatus::TestValidated } else { VerificationStatus::ToolConfirmed };
        let mut record = MemoryRecord::new(self.repository_id.clone(), MemoryCategory::Episodic, content, verification).with_task(state.task_id.clone());
        for f in &state.modified_files {
            record.evidence.push(EvidenceReference::source(f.clone(), None));
        }
        record.tags = state.keywords.clone();
        record.tags.extend(state.modified_files.iter().flat_map(|f| extract_keywords(f)));
        record.tags.dedup();
        self.store.insert_memory(&record.to_row())?;
        Ok(record.id)
    }

    /// Persist the structured task state into the task row.
    pub fn save_task_state(&self, state: &TaskState) -> Result<()> {
        if let Some(mut task) = self.store.get_task(state.task_id.as_str())? {
            task.state_json = state.to_json();
            self.store.update_task(&task)?;
        }
        Ok(())
    }

    pub fn load_task_state(&self, task_id: &TaskId) -> Result<Option<TaskState>> {
        Ok(self.store.get_task(task_id.as_str())?.and_then(|t| TaskState::from_json(&t.state_json)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> MemoryStore {
        MemoryStore::new(Arc::new(SessionStore::in_memory().unwrap()), RepositoryId::from_string("repo"))
    }

    #[test]
    fn remember_dedupes_and_upgrades() {
        let m = store();
        let a = MemoryRecord::new(m.repository_id().clone(), MemoryCategory::ProjectKnowledge, "Buttons live in src/components/Button.tsx", VerificationStatus::Hypothesis);
        let id = m.remember(a.clone()).unwrap();
        let b = MemoryRecord::new(m.repository_id().clone(), MemoryCategory::ProjectKnowledge, "Buttons live in src/components/Button.tsx", VerificationStatus::SourceDerived)
            .with_evidence(EvidenceReference::source("src/components/Button.tsx", Some("sha256:a".into())));
        let id2 = m.remember(b).unwrap();
        assert_eq!(id, id2);
        let rec = m.get(&id).unwrap().unwrap();
        assert_eq!(rec.verification, VerificationStatus::SourceDerived);
        assert_eq!(rec.evidence.len(), 1);
        assert_eq!(m.list(Some(MemoryCategory::ProjectKnowledge), None, false, 0).unwrap().len(), 1);
    }

    #[test]
    fn search_excludes_hypotheses_and_weights_verification() {
        let m = store();
        m.remember(MemoryRecord::new(m.repository_id().clone(), MemoryCategory::ProjectKnowledge, "The login button shows a spinner from Spinner.tsx", VerificationStatus::Hypothesis)).unwrap();
        m.remember(MemoryRecord::new(m.repository_id().clone(), MemoryCategory::ProjectKnowledge, "Login flow uses useAuth hook; button is LoginButton.tsx", VerificationStatus::TestValidated)).unwrap();
        let hits = m.search("login button", 5, false).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].0.content.contains("LoginButton.tsx"));
        assert_eq!(m.search("login button", 5, true).unwrap().len(), 2);
    }

    #[test]
    fn invalidation_by_path_respects_hash() {
        let m = store();
        let fresh = MemoryRecord::new(m.repository_id().clone(), MemoryCategory::ProjectKnowledge, "fact A about pricing", VerificationStatus::SourceDerived)
            .with_evidence(EvidenceReference::source("svc/pricing.py", Some("sha256:new".into())));
        let stale = MemoryRecord::new(m.repository_id().clone(), MemoryCategory::ProjectKnowledge, "fact B about pricing", VerificationStatus::SourceDerived)
            .with_evidence(EvidenceReference::source("svc/pricing.py", Some("sha256:old".into())));
        m.remember(fresh).unwrap();
        m.remember(stale).unwrap();
        let n = m.invalidate_for_path("svc/pricing.py", Some("sha256:new"), "file changed").unwrap();
        assert_eq!(n, 1);
        let current = m.list(Some(MemoryCategory::ProjectKnowledge), None, false, 0).unwrap();
        assert_eq!(current.len(), 1);
        assert!(current[0].content.contains("fact A"));
        let all = m.list(Some(MemoryCategory::ProjectKnowledge), None, true, 0).unwrap();
        assert_eq!(all.len(), 2, "invalidated records remain as history");
    }

    #[test]
    fn promotion_requires_matching_evidence() {
        let m = store();
        let id = m.remember(MemoryRecord::new(m.repository_id().clone(), MemoryCategory::ProjectKnowledge, "tests use node --test", VerificationStatus::Hypothesis)).unwrap();
        assert!(!m.promote(&id, VerificationStatus::TestValidated, EvidenceReference::model()).unwrap());
        assert!(m.promote(&id, VerificationStatus::ToolConfirmed, EvidenceReference::tool("package.json scripts.test", None)).unwrap());
        assert_eq!(m.get(&id).unwrap().unwrap().verification, VerificationStatus::ToolConfirmed);
    }
}
