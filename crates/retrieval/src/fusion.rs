//! Ranking and fusion (spec §7.7).

use crate::query::QueryAnalysis;
use crate::types::{SourceCandidate, StrategyKind};
use std::collections::HashMap;

pub struct RankContext<'a> {
    pub analysis: &'a QueryAnalysis,
    pub working_set: &'a [String],
    pub scope: &'a [String],
    /// Paths boosted by relevant memory evidence.
    pub memory_paths: &'a [String],
    pub current_generation: u64,
    pub candidate_generations: &'a HashMap<String, u64>,
}

/// Pluggable ranking so alternatives can be compared experimentally.
pub trait Ranker: Send + Sync {
    fn name(&self) -> &str;
    fn rank(&self, candidates: Vec<SourceCandidate>, ctx: &RankContext<'_>) -> Vec<SourceCandidate>;
}

/// Reciprocal Rank Fusion with per-strategy weights plus code-specific
/// bonuses.
#[derive(Debug, Clone)]
pub struct RrfRanker {
    pub k: f64,
    pub weights: HashMap<StrategyKind, f64>,
}

impl Default for RrfRanker {
    fn default() -> Self {
        let mut weights = HashMap::new();
        weights.insert(StrategyKind::Path, 1.2);
        weights.insert(StrategyKind::Symbol, 1.3);
        weights.insert(StrategyKind::Lexical, 1.0);
        weights.insert(StrategyKind::Semantic, 0.8);
        weights.insert(StrategyKind::Expansion, 0.6);
        weights.insert(StrategyKind::WorkingSet, 0.7);
        weights.insert(StrategyKind::Memory, 0.5);
        Self { k: 60.0, weights }
    }
}

/// Merge duplicate candidates (same path+range) accumulating their signals.
pub fn merge_duplicates(candidates: Vec<SourceCandidate>) -> Vec<SourceCandidate> {
    let mut merged: Vec<SourceCandidate> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for c in candidates {
        let key = c.key();
        match index.get(&key) {
            Some(&i) => {
                let existing = &mut merged[i];
                for s in c.signals {
                    if !existing.signals.iter().any(|e| e.strategy == s.strategy) {
                        existing.signals.push(s);
                    }
                }
                if existing.content.is_none() {
                    existing.content = c.content;
                }
                if existing.content_hash.is_empty() {
                    existing.content_hash = c.content_hash;
                    existing.file_hash = c.file_hash;
                    existing.chunk_id = c.chunk_id;
                }
                existing.reasons.extend(c.reasons);
            }
            None => {
                index.insert(key, merged.len());
                merged.push(c);
            }
        }
    }
    merged
}

fn path_matches_scope(path: &str, scope: &[String]) -> bool {
    scope.iter().any(|s| {
        let s = s.trim_matches('/');
        path == s || path.starts_with(&format!("{s}/")) || path.contains(&format!("/{s}/"))
    })
}

impl Ranker for RrfRanker {
    fn name(&self) -> &str {
        "rrf"
    }

    fn rank(&self, candidates: Vec<SourceCandidate>, ctx: &RankContext<'_>) -> Vec<SourceCandidate> {
        let mut merged = merge_duplicates(candidates);
        let lower_filenames: Vec<String> = ctx.analysis.filenames.iter().map(|f| f.to_lowercase()).collect();
        let lower_paths: Vec<String> = ctx.analysis.paths.iter().map(|p| p.to_lowercase()).collect();
        let lower_symbols: Vec<String> = ctx.analysis.symbols.iter().map(|s| s.to_lowercase()).collect();
        for c in merged.iter_mut() {
            let mut score = 0.0;
            for s in &c.signals {
                let w = self.weights.get(&s.strategy).copied().unwrap_or(1.0);
                score += w / (self.k + s.rank as f64);
            }
            // Agreement across strategies is strong evidence.
            let distinct = c.strategies().len();
            if distinct >= 2 {
                score *= 1.0 + 0.25 * (distinct as f64 - 1.0);
                c.reasons.push(format!("{distinct} strategies agree"));
            }
            let path_lower = c.path.to_lowercase();
            let name_lower = path_lower.rsplit('/').next().unwrap_or(&path_lower).to_string();
            if lower_paths.iter().any(|p| path_lower == *p || path_lower.ends_with(&format!("/{p}"))) {
                score += 0.08;
                c.reasons.push("exact path match".into());
            } else if lower_filenames.iter().any(|f| name_lower == *f) {
                score += 0.06;
                c.reasons.push("exact filename match".into());
            }
            if let Some(sym) = &c.symbol {
                let sym_lower = sym.to_lowercase();
                let bare = sym_lower.rsplit('.').next().unwrap_or(&sym_lower).to_string();
                if lower_symbols.iter().any(|s| *s == sym_lower || *s == bare || s.rsplit("::").next() == Some(bare.as_str())) {
                    score += 0.07;
                    c.reasons.push("exact identifier match".into());
                }
            }
            if ctx.working_set.iter().any(|w| w == &c.path) {
                score += 0.03;
                c.reasons.push("active working set".into());
            }
            if ctx.memory_paths.iter().any(|m| m == &c.path) {
                score += 0.02;
                c.reasons.push("referenced by relevant memory".into());
            }
            if !ctx.scope.is_empty() {
                if path_matches_scope(&c.path, ctx.scope) {
                    score += 0.05;
                    c.reasons.push("inside requested scope".into());
                } else {
                    score *= 0.5;
                    c.reasons.push("outside requested scope".into());
                }
            }
            let is_test = name_lower.contains(".test.") || name_lower.contains(".spec.") || name_lower.contains("_test.") || name_lower.starts_with("test_") || path_lower.contains("/tests/") || path_lower.contains("/__tests__/") || path_lower.contains("/spec/");
            if is_test && !ctx.analysis.mentions_tests {
                score *= 0.75;
                c.reasons.push("test file (not requested)".into());
            }
            let is_doc = name_lower.ends_with(".md") || name_lower.ends_with(".rst") || name_lower.ends_with(".txt");
            if is_doc && !ctx.analysis.mentions_docs {
                score *= 0.6;
                c.reasons.push("documentation (not requested)".into());
            }
            if let Some(gen) = ctx.candidate_generations.get(&c.path) {
                if *gen < ctx.current_generation.saturating_sub(1) {
                    // Indexed several generations ago: slightly less trusted
                    // until re-verified (verification happens later anyway).
                    score *= 0.97;
                }
            }
            if c.symbol_kind.as_deref() == Some("section") && c.symbol.is_none() {
                // Gap chunks (imports, module-level statements) are rarely
                // the edit target.
                score *= 0.85;
            }
            c.fused_score = score;
        }
        merged.sort_by(|a, b| b.fused_score.partial_cmp(&a.fused_score).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.path.cmp(&b.path)).then_with(|| a.start_byte.cmp(&b.start_byte)));
        merged
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::analyze_query;
    use crate::types::Signal;

    fn cand(path: &str, symbol: Option<&str>, signals: Vec<(StrategyKind, usize)>) -> SourceCandidate {
        SourceCandidate {
            chunk_id: None,
            path: path.into(),
            symbol: symbol.map(|s| s.into()),
            symbol_kind: Some("function".into()),
            language: None,
            start_byte: 0,
            end_byte: 10,
            start_line: 1,
            end_line: 2,
            content_hash: "h".into(),
            file_hash: "f".into(),
            content: None,
            signals: signals.into_iter().map(|(s, r)| Signal { strategy: s, rank: r, score: 1.0, detail: String::new() }).collect(),
            fused_score: 0.0,
            reasons: vec![],
        }
    }

    #[test]
    fn agreement_and_exact_matches_win() {
        let a = analyze_query("add a spinner to LoginButton.tsx");
        let ws: Vec<String> = vec![];
        let gens = HashMap::new();
        let ctx = RankContext { analysis: &a, working_set: &ws, scope: &ws, memory_paths: &ws, current_generation: 1, candidate_generations: &gens };
        let ranked = RrfRanker::default().rank(
            vec![
                cand("src/Footer.tsx", Some("Footer"), vec![(StrategyKind::Lexical, 1)]),
                cand("src/LoginButton.tsx", Some("LoginButton"), vec![(StrategyKind::Lexical, 2)]),
                cand("src/LoginButton.tsx", Some("LoginButton"), vec![(StrategyKind::Symbol, 1)]),
                cand("src/__tests__/LoginButton.test.tsx", Some("test"), vec![(StrategyKind::Lexical, 3), (StrategyKind::Path, 1)]),
            ],
            &ctx,
        );
        assert_eq!(ranked[0].path, "src/LoginButton.tsx");
        assert!(ranked[0].reasons.iter().any(|r| r.contains("exact filename")));
        assert!(ranked[0].reasons.iter().any(|r| r.contains("strategies agree")));
        assert_eq!(ranked.len(), 3, "duplicates merged");
        let test_rank = ranked.iter().position(|c| c.path.contains("__tests__")).unwrap();
        assert!(test_rank > 0);
    }
}
