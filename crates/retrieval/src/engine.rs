//! Orchestration of the retrieval pipeline (spec §7.2, §7.9–§7.12).

use crate::expansion::{expand, ExpansionOptions};
use crate::fusion::{RankContext, Ranker, RrfRanker};
use crate::query::{analyze_query, QueryAnalysis};
use crate::strategies;
use crate::types::*;
use ctx_core::{estimate_tokens, Result, TaskCategory, VerifiedSourceSnippet};
use ctx_indexing::{Freshness, Indexer};
use ctx_memory::{MemoryRecord, MemoryStore};
use ctx_storage::IndexStore;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct RetrievalRequest {
    pub query: String,
    /// Override the heuristic category.
    pub category: Option<TaskCategory>,
    /// Token budget for selected snippets.
    pub budget_tokens: usize,
    pub working_set: Vec<String>,
    /// Explicit scope restriction (paths).
    pub scope: Vec<String>,
    pub stage: RetrievalStage,
    /// Upper bound on fused candidates kept for verification.
    pub max_candidates: usize,
}

impl RetrievalRequest {
    pub fn new(query: impl Into<String>, budget_tokens: usize) -> Self {
        Self { query: query.into(), category: None, budget_tokens, working_set: Vec::new(), scope: Vec::new(), stage: RetrievalStage::Initial, max_candidates: 40 }
    }
}

pub struct RetrievalEngine {
    indexer: Arc<Indexer>,
    memory: Option<MemoryStore>,
    ranker: Arc<dyn Ranker>,
}

struct StrategyOutput {
    kind: StrategyKind,
    candidates: Vec<SourceCandidate>,
    duration_ms: u64,
    note: String,
}

impl RetrievalEngine {
    pub fn new(indexer: Arc<Indexer>, memory: Option<MemoryStore>) -> Self {
        Self { indexer, memory, ranker: Arc::new(RrfRanker::default()) }
    }

    pub fn with_ranker(mut self, ranker: Arc<dyn Ranker>) -> Self {
        self.ranker = ranker;
        self
    }

    pub fn indexer(&self) -> &Arc<Indexer> {
        &self.indexer
    }

    pub fn store(&self) -> &Arc<IndexStore> {
        self.indexer.store()
    }

    pub fn memory(&self) -> Option<&MemoryStore> {
        self.memory.as_ref()
    }

    /// Async entry point: runs the blocking pipeline on a worker thread.
    pub async fn retrieve(self: &Arc<Self>, request: RetrievalRequest) -> Result<RetrievalResult> {
        let engine = self.clone();
        tokio::task::spawn_blocking(move || engine.retrieve_blocking(&request)).await.map_err(|e| ctx_core::CtxError::other(format!("retrieval task failed: {e}")))?
    }

    /// The full pipeline, synchronous. Strategies run on scoped threads.
    pub fn retrieve_blocking(&self, request: &RetrievalRequest) -> Result<RetrievalResult> {
        let started = Instant::now();
        let store = self.store().clone();
        let generation = store.generation()?;
        let mut analysis = analyze_query(&request.query);
        if let Some(cat) = request.category {
            analysis.category = cat;
        }
        if !request.scope.is_empty() {
            for s in &request.scope {
                if !analysis.scope.contains(s) {
                    analysis.scope.push(s.clone());
                }
            }
        }
        let stage = request.stage;
        let prefix_matching = matches!(stage, RetrievalStage::Expanded | RetrievalStage::Recovery);
        let per_strategy = match stage {
            RetrievalStage::Initial | RetrievalStage::Focused => 25,
            RetrievalStage::Expanded | RetrievalStage::Recovery => 40,
        };

        // ----- strategies in parallel -----------------------------------
        let embeddings = self.indexer.embeddings().cloned();
        let working_set = request.working_set.clone();
        let query_text = request.query.clone();
        let analysis_ref = &analysis;
        let outputs: Vec<StrategyOutput> = std::thread::scope(|scope| {
            let mut handles = Vec::new();
            {
                let store = store.clone();
                handles.push(scope.spawn(move || timed(StrategyKind::Lexical, || strategies::lexical(&store, analysis_ref, per_strategy, prefix_matching))));
            }
            {
                let store = store.clone();
                handles.push(scope.spawn(move || timed(StrategyKind::Path, || strategies::paths(&store, analysis_ref, 12))));
            }
            {
                let store = store.clone();
                handles.push(scope.spawn(move || timed(StrategyKind::Symbol, || strategies::symbols(&store, analysis_ref, per_strategy))));
            }
            if let Some(emb) = embeddings.clone() {
                let store = store.clone();
                let q = query_text.clone();
                handles.push(scope.spawn(move || timed(StrategyKind::Semantic, || strategies::semantic(&store, &emb, &q, per_strategy))));
            }
            if !working_set.is_empty() {
                let store = store.clone();
                let ws = working_set.clone();
                handles.push(scope.spawn(move || timed(StrategyKind::WorkingSet, || strategies::working_set(&store, &ws, 6))));
            }
            handles.into_iter().filter_map(|h| h.join().ok()).collect()
        });

        // Memory search joins at compilation time (spec §7.2).
        let mem_started = Instant::now();
        let memories: Vec<(MemoryRecord, f64)> = match &self.memory {
            Some(m) => m.search(&request.query, 6, false).unwrap_or_default(),
            None => Vec::new(),
        };
        let memory_ms = mem_started.elapsed().as_millis() as u64;
        let memory_paths: Vec<String> = memories.iter().flat_map(|(m, _)| m.evidence_paths().map(|p| p.to_string())).collect();

        let mut diagnostics = RetrievalDiagnostics { stage, category: analysis.category, budget_tokens: request.budget_tokens, ..Default::default() };
        let mut all: Vec<SourceCandidate> = Vec::new();
        for out in outputs {
            diagnostics.strategies.push(StrategyDiagnostics { strategy: out.kind.as_str().into(), hits: out.candidates.len(), duration_ms: out.duration_ms, note: out.note });
            all.extend(out.candidates);
        }
        diagnostics.strategies.push(StrategyDiagnostics { strategy: "memory".into(), hits: memories.len(), duration_ms: memory_ms, note: if self.memory.is_some() { String::new() } else { "no memory store".into() } });
        diagnostics.candidates_before_fusion = all.len();

        // ----- fusion -----------------------------------------------------
        let generations: HashMap<String, u64> = store.list_files()?.into_iter().map(|f| (f.path, f.generation)).collect();
        let ctx = RankContext { analysis: &analysis, working_set: &request.working_set, scope: &analysis.scope.clone(), memory_paths: &memory_paths, current_generation: generation, candidate_generations: &generations };
        let mut ranked = self.ranker.rank(all, &ctx);

        // ----- expansion --------------------------------------------------
        let expansion_opts = match stage {
            RetrievalStage::Initial => ExpansionOptions { max_seeds: 4, max_added: 6, include_dependents: true, depth: 1 },
            RetrievalStage::Focused => ExpansionOptions { max_seeds: 3, max_added: 4, include_dependents: false, depth: 1 },
            RetrievalStage::Expanded => ExpansionOptions { max_seeds: 8, max_added: 14, include_dependents: true, depth: 2 },
            RetrievalStage::Recovery => ExpansionOptions { max_seeds: 10, max_added: 18, include_dependents: true, depth: 2 },
        };
        if !ranked.is_empty() {
            let seeds: Vec<SourceCandidate> = ranked.iter().take(expansion_opts.max_seeds).cloned().collect();
            let expanded = expand(&store, &seeds, expansion_opts)?;
            diagnostics.expanded = expanded.len();
            if !expanded.is_empty() {
                let mut combined = ranked;
                combined.extend(expanded);
                ranked = self.ranker.rank(combined, &ctx);
            }
        }
        ranked.truncate(request.max_candidates);
        diagnostics.candidates_after_fusion = ranked.len();

        // ----- source validation -----------------------------------------
        let mut freshness_cache: HashMap<String, Freshness> = HashMap::new();
        let mut verified: Vec<SourceCandidate> = Vec::new();
        for mut cand in ranked.iter().cloned() {
            if cand.content_hash.is_empty() {
                // Symbol-only candidate without a chunk: materialize from file.
                if let Some(ch) = store.chunks_in_file(&cand.path)?.into_iter().find(|c| c.start_byte <= cand.start_byte && c.end_byte >= cand.end_byte) {
                    cand.chunk_id = Some(ch.id.clone());
                    cand.content_hash = ch.content_hash.clone();
                    cand.file_hash = ch.file_hash.clone();
                    cand.content = Some(ch.content.clone());
                    cand.start_byte = ch.start_byte;
                    cand.end_byte = ch.end_byte;
                    cand.start_line = ch.start_line;
                    cand.end_line = ch.end_line;
                } else {
                    continue;
                }
            }
            let fresh = match freshness_cache.get(&cand.path) {
                Some(f) => f.clone(),
                None => {
                    let f = self.indexer.ensure_fresh(&cand.source_ref())?;
                    freshness_cache.insert(cand.path.clone(), f.clone());
                    f
                }
            };
            match fresh {
                Freshness::Fresh => {
                    if cand.content.is_none() {
                        if let Some(id) = &cand.chunk_id {
                            cand.content = store.get_chunk(id)?.map(|c| c.content);
                        }
                    }
                    verified.push(cand);
                }
                Freshness::Missing => {
                    diagnostics.dropped_missing += 1;
                    diagnostics.excluded.push(ExcludedCandidate { path: cand.path.clone(), start_line: cand.start_line, end_line: cand.end_line, symbol: cand.symbol.clone(), fused_score: cand.fused_score, reason: "file no longer exists".into() });
                }
                Freshness::Changed { current_hash, .. } => {
                    // The file was re-indexed by ensure_fresh; relocate the chunk.
                    // Only chunks of the current content qualify: while a full
                    // index runs the re-index is queued and the old ones remain.
                    diagnostics.stale_refreshed += 1;
                    let chunks: Vec<_> = store.chunks_in_file(&cand.path)?.into_iter().filter(|c| c.file_hash == current_hash).collect();
                    // Prefer the same symbol's chunk closest to the original
                    // position (a split symbol has several parts), then any
                    // chunk overlapping the original range.
                    let relocated = cand
                        .symbol
                        .as_ref()
                        .and_then(|sym| chunks.iter().filter(|c| c.symbol.as_deref() == Some(sym.as_str())).min_by_key(|c| c.start_byte.abs_diff(cand.start_byte)))
                        .or_else(|| chunks.iter().find(|c| c.start_byte <= cand.start_byte && c.end_byte > cand.start_byte))
                        .cloned();
                    match relocated {
                        Some(ch) => {
                            cand.reasons.push("refreshed from changed file".into());
                            cand.symbol = ch.symbol.clone();
                            cand.symbol_kind = ch.symbol_kind.clone();
                            cand.chunk_id = Some(ch.id.clone());
                            cand.start_byte = ch.start_byte;
                            cand.end_byte = ch.end_byte;
                            cand.start_line = ch.start_line;
                            cand.end_line = ch.end_line;
                            cand.content_hash = ch.content_hash.clone();
                            cand.file_hash = ch.file_hash.clone();
                            cand.content = Some(ch.content.clone());
                            verified.push(cand);
                        }
                        None => diagnostics.excluded.push(ExcludedCandidate { path: cand.path.clone(), start_line: cand.start_line, end_line: cand.end_line, symbol: cand.symbol.clone(), fused_score: cand.fused_score, reason: "range no longer present after file change".into() }),
                    }
                }
            }
        }

        // ----- selection under budget ------------------------------------
        let mut context: Vec<VerifiedSourceSnippet> = Vec::new();
        let mut used = 0usize;
        let mut selected_ranges: Vec<(String, usize, usize)> = Vec::new();
        for cand in &verified {
            let Some(content) = &cand.content else { continue };
            if selected_ranges.iter().any(|(p, s, e)| p == &cand.path && *s <= cand.start_byte && *e >= cand.end_byte) {
                diagnostics.excluded.push(ExcludedCandidate { path: cand.path.clone(), start_line: cand.start_line, end_line: cand.end_line, symbol: cand.symbol.clone(), fused_score: cand.fused_score, reason: "covered by an already selected range".into() });
                continue;
            }
            let mut text = content.clone();
            let mut tokens = estimate_tokens(&text);
            if used + tokens > request.budget_tokens {
                if context.is_empty() {
                    // Always supply the top candidate, truncated to the budget
                    // (never below a useful minimum).
                    let allowance = request.budget_tokens.max(120);
                    if tokens > allowance {
                        let max_chars = allowance.saturating_sub(30) * 4;
                        let mut cut = max_chars.min(text.len());
                        while cut > 0 && !text.is_char_boundary(cut) {
                            cut -= 1;
                        }
                        text.truncate(cut);
                        text.push_str("\n… [truncated to fit the context budget; use read_file for the rest]");
                        tokens = estimate_tokens(&text);
                    }
                } else {
                    diagnostics.excluded.push(ExcludedCandidate { path: cand.path.clone(), start_line: cand.start_line, end_line: cand.end_line, symbol: cand.symbol.clone(), fused_score: cand.fused_score, reason: "context budget exhausted".into() });
                    continue;
                }
            }
            used += tokens;
            selected_ranges.push((cand.path.clone(), cand.start_byte, cand.end_byte));
            context.push(VerifiedSourceSnippet {
                source: cand.source_ref(),
                content: text,
                symbol: cand.symbol.clone(),
                language: cand.language.clone(),
                rationale: cand.rationale(),
                score: cand.fused_score,
            });
        }
        diagnostics.used_tokens = used;

        // ----- confidence --------------------------------------------------
        let (confidence, rationale) = assess_confidence(&analysis, &verified);
        diagnostics.confidence_rationale = rationale;
        diagnostics.total_ms = started.elapsed().as_millis() as u64;

        let candidates: Vec<SourceCandidate> = verified.into_iter().map(|mut c| {
            c.content = None;
            c
        }).collect();
        Ok(RetrievalResult { candidates, context, memories, confidence, index_generation: generation, diagnostics, analysis })
    }
}

fn timed(kind: StrategyKind, f: impl FnOnce() -> Result<Vec<SourceCandidate>>) -> StrategyOutput {
    let started = Instant::now();
    match f() {
        Ok(candidates) => StrategyOutput { kind, candidates, duration_ms: started.elapsed().as_millis() as u64, note: String::new() },
        Err(e) => StrategyOutput { kind, candidates: Vec::new(), duration_ms: started.elapsed().as_millis() as u64, note: format!("error: {e}") },
    }
}

/// Calibration hooks: the thresholds here are the knobs the evaluation
/// harness tunes against labeled retrieval queries (spec §7.9).
pub fn assess_confidence(analysis: &QueryAnalysis, ranked: &[SourceCandidate]) -> (RetrievalConfidence, String) {
    let Some(top) = ranked.first() else {
        return (RetrievalConfidence::Low, "no candidates".into());
    };
    let explicit_target = !analysis.filenames.is_empty() || !analysis.paths.is_empty() || !analysis.symbols.is_empty();
    let exact = top.reasons.iter().any(|r| r.starts_with("exact"));
    let agreement = top.strategies().len();
    let second = ranked.get(1).map(|c| c.fused_score).unwrap_or(0.0);
    let margin = if top.fused_score > 0.0 { (top.fused_score - second) / top.fused_score } else { 0.0 };
    if explicit_target && !exact && ranked.iter().take(5).all(|c| !c.reasons.iter().any(|r| r.starts_with("exact"))) {
        return (RetrievalConfidence::Low, "the request names a file or symbol that was not found in the index".into());
    }
    if exact && (agreement >= 2 || margin > 0.3) {
        return (RetrievalConfidence::High, format!("exact match with {agreement} agreeing strategies and {:.0}% margin", margin * 100.0));
    }
    if agreement >= 2 || margin > 0.4 || (exact && explicit_target) {
        return (RetrievalConfidence::Medium, format!("{agreement} agreeing strategies, {:.0}% margin", margin * 100.0));
    }
    (RetrievalConfidence::Low, format!("single-strategy evidence with {:.0}% margin", margin * 100.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ctx_core::IndexConfig;
    use ctx_embeddings::{EmbeddingService, HashingEmbedder};
    use ctx_indexing::IndexerOptions;
    use ctx_languages::LanguageRegistry;
    use ctx_memory::{EvidenceReference, MemoryCategory, VerificationStatus};
    use ctx_storage::SessionStore;

    fn repo() -> (tempfile::TempDir, Arc<RetrievalEngine>) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src/components")).unwrap();
        std::fs::create_dir_all(root.join("src/hooks")).unwrap();
        std::fs::create_dir_all(root.join("src/__tests__")).unwrap();
        std::fs::write(root.join("src/components/LoginButton.tsx"), "import { useAuth } from '../hooks/useAuth';\nimport { Spinner } from './Spinner';\n\nexport function LoginButton() {\n  const { login, isLoading } = useAuth();\n  return <button onClick={login}>Sign in</button>;\n}\n").unwrap();
        std::fs::write(root.join("src/components/Spinner.tsx"), "export function Spinner() { return <span className=\"spinner\" />; }\n").unwrap();
        std::fs::write(root.join("src/components/Footer.tsx"), "export function Footer() { return <footer>© 2024 Example</footer>; }\n").unwrap();
        std::fs::write(root.join("src/hooks/useAuth.ts"), "export function useAuth() { return { login() {}, isLoading: false }; }\n").unwrap();
        std::fs::write(root.join("src/__tests__/LoginButton.test.tsx"), "import { LoginButton } from '../components/LoginButton';\ntest('renders', () => { LoginButton(); });\n").unwrap();
        std::fs::write(root.join("README.md"), "# App\n\n## Login\nThe login button lives in src/components/LoginButton.tsx.\n").unwrap();
        let store = Arc::new(IndexStore::in_memory().unwrap());
        let embeddings = Arc::new(EmbeddingService::load(store.clone(), Arc::new(HashingEmbedder::new(128))).unwrap());
        let indexer = Arc::new(Indexer::new(root, &IndexConfig::default(), IndexerOptions::default(), store, Arc::new(LanguageRegistry::with_defaults()), Some(embeddings)).unwrap());
        indexer.full_index(None).unwrap();
        let memory = MemoryStore::new(Arc::new(SessionStore::in_memory().unwrap()), ctx_core::RepositoryId::from_root(root));
        memory
            .remember(
                ctx_memory::MemoryRecord::new(memory.repository_id().clone(), MemoryCategory::ProjectKnowledge, "Login UI: LoginButton.tsx uses the useAuth hook", VerificationStatus::SourceDerived)
                    .with_evidence(EvidenceReference::source("src/components/LoginButton.tsx", None)),
            )
            .unwrap();
        (dir, Arc::new(RetrievalEngine::new(indexer, Some(memory))))
    }

    #[test]
    fn finds_login_button_with_dependencies() {
        let (_dir, engine) = repo();
        let result = engine.retrieve_blocking(&RetrievalRequest::new("Add loading feedback to the login button while authentication is in progress", 4000)).unwrap();
        let paths = result.paths();
        assert_eq!(paths.first().map(|s| s.as_str()), Some("src/components/LoginButton.tsx"), "{:?}\n{:#?}", paths, result.diagnostics);
        assert!(paths.iter().any(|p| p.ends_with("useAuth.ts")), "dependency expansion: {paths:?}");
        assert!(result.memories.len() == 1);
        assert!(matches!(result.confidence, RetrievalConfidence::High | RetrievalConfidence::Medium), "{:?}", result.diagnostics.confidence_rationale);
        assert!(result.diagnostics.strategies.iter().any(|s| s.strategy == "lexical" && s.hits > 0));
        assert!(result.context.iter().all(|s| !s.content.is_empty()));
    }

    #[test]
    fn explicit_filename_is_high_confidence_and_budget_is_respected() {
        let (_dir, engine) = repo();
        let result = engine.retrieve_blocking(&RetrievalRequest::new("Update the copyright year in Footer.tsx", 10)).unwrap();
        assert_eq!(result.context[0].source.path, "src/components/Footer.tsx", "{:#?}", result.diagnostics);
        assert_eq!(result.confidence, RetrievalConfidence::High, "{}", result.diagnostics.confidence_rationale);
        assert_eq!(result.context.len(), 1, "a tiny budget still yields the top candidate and nothing else");
        assert!(result.candidates.len() > 1);
        assert!(result.diagnostics.excluded.iter().any(|e| e.reason.contains("budget")), "{:#?}", result.diagnostics.excluded);
    }

    #[test]
    fn stale_candidates_are_refreshed() {
        let (dir, engine) = repo();
        std::fs::write(dir.path().join("src/components/Footer.tsx"), "// moved\n\nexport function Footer() { return <footer>© 2026 Example</footer>; }\n").unwrap();
        let result = engine.retrieve_blocking(&RetrievalRequest::new("Footer.tsx copyright", 2000)).unwrap();
        assert!(result.diagnostics.stale_refreshed >= 1);
        let footer = result.context.iter().find(|s| s.source.path.ends_with("Footer.tsx")).unwrap();
        assert!(footer.content.contains("2026"));
    }

    #[test]
    fn missing_target_lowers_confidence() {
        let (_dir, engine) = repo();
        let result = engine.retrieve_blocking(&RetrievalRequest::new("Fix the bug in PaymentGateway.ts", 2000)).unwrap();
        assert_eq!(result.confidence, RetrievalConfidence::Low);
    }

    #[tokio::test]
    async fn async_entry_point() {
        let (_dir, engine) = repo();
        let result = engine.retrieve(RetrievalRequest::new("spinner", 1000)).await.unwrap();
        assert!(result.paths().iter().any(|p| p.ends_with("Spinner.tsx")));
    }
}
