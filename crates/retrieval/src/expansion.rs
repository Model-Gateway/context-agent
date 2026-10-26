//! Bounded relationship expansion (spec §7.8).

use crate::strategies::candidate_from_chunk;
use crate::types::{Signal, SourceCandidate, StrategyKind};
use ctx_core::Result;
use ctx_storage::{IndexStore, RelationshipRecord};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy)]
pub struct ExpansionOptions {
    /// How many top-ranked candidates seed expansion.
    pub max_seeds: usize,
    /// Maximum candidates added by expansion.
    pub max_added: usize,
    /// Also follow reverse edges (files that import / test the seed).
    pub include_dependents: bool,
    /// 1 = direct relationships only; 2 = one more hop.
    pub depth: usize,
}

impl Default for ExpansionOptions {
    fn default() -> Self {
        Self { max_seeds: 5, max_added: 8, include_dependents: true, depth: 1 }
    }
}

const FOLLOW_KINDS: &[&str] = &["import", "include", "test_of", "style_of", "extends", "implements", "module"];

fn last_symbol_of_target(target: &str) -> Option<String> {
    // `crate::auth::{login, Session}` → None (several); `../hooks/useAuth` → useAuth;
    // `com.example.calc.Parser` → Parser; `svc.pricing` → pricing.
    if target.contains('{') || target.contains(',') || target.ends_with('*') {
        return None;
    }
    let last = target.rsplit(|c| c == '/' || c == ':' || c == '.' || c == '\\').next()?.trim();
    if last.is_empty() || last.contains(' ') {
        None
    } else {
        Some(last.to_string())
    }
}

fn candidate_for_target(store: &IndexStore, rel: &RelationshipRecord, to_path: &str, rank: usize, detail: String) -> Result<Option<SourceCandidate>> {
    // Prefer the chunk defining the referenced symbol, then the file head.
    let symbol = rel.to_symbol.clone().or_else(|| last_symbol_of_target(&rel.target));
    if let Some(sym) = symbol {
        if let Some(ch) = store.chunk_for_symbol(to_path, &sym)? {
            return Ok(Some(candidate_from_chunk(&ch, Signal { strategy: StrategyKind::Expansion, rank, score: rel.confidence, detail })));
        }
    }
    let chunks = store.chunks_in_file(to_path)?;
    // The first symbol-bearing chunk is more useful than an import block.
    let pick = chunks.iter().find(|c| c.symbol.is_some()).or_else(|| chunks.first());
    Ok(pick.map(|ch| candidate_from_chunk(ch, Signal { strategy: StrategyKind::Expansion, rank, score: rel.confidence, detail })))
}

/// Expand from the top candidates along resolved relationships.
pub fn expand(store: &IndexStore, seeds: &[SourceCandidate], opts: ExpansionOptions) -> Result<Vec<SourceCandidate>> {
    let mut seed_paths: Vec<String> = Vec::new();
    for s in seeds {
        if !seed_paths.contains(&s.path) {
            seed_paths.push(s.path.clone());
        }
        if seed_paths.len() >= opts.max_seeds {
            break;
        }
    }
    let seed_set: HashSet<String> = seed_paths.iter().cloned().collect();
    let mut added: Vec<SourceCandidate> = Vec::new();
    let mut seen_paths: HashSet<String> = seed_set.clone();
    let mut frontier = seed_paths.clone();
    let mut rank = 0usize;
    for _hop in 0..opts.depth.max(1) {
        let mut next_frontier = Vec::new();
        for path in &frontier {
            let mut edges: Vec<(RelationshipRecord, String, String)> = Vec::new();
            for rel in store.relationships_from(path)? {
                if !FOLLOW_KINDS.contains(&rel.kind.as_str()) {
                    continue;
                }
                if let Some(to) = rel.to_path.clone() {
                    let detail = format!("{} `{}` from {}", rel.kind, rel.target, path);
                    edges.push((rel, to, detail));
                }
            }
            if opts.include_dependents {
                for rel in store.relationships_to(path)? {
                    if !FOLLOW_KINDS.contains(&rel.kind.as_str()) {
                        continue;
                    }
                    let detail = match rel.kind.as_str() {
                        "test_of" => format!("tests for {path}"),
                        "style_of" => format!("styles for {path}"),
                        _ => format!("{} {} ({})", rel.from_path, rel.kind, path),
                    };
                    let from = rel.from_path.clone();
                    edges.push((rel, from, detail));
                }
            }
            // Highest confidence first, tests and styles before generic imports.
            edges.sort_by(|a, b| {
                let pa = if a.0.kind == "test_of" || a.0.kind == "style_of" { 0 } else { 1 };
                let pb = if b.0.kind == "test_of" || b.0.kind == "style_of" { 0 } else { 1 };
                pa.cmp(&pb).then_with(|| b.0.confidence.partial_cmp(&a.0.confidence).unwrap_or(std::cmp::Ordering::Equal))
            });
            for (rel, to, detail) in edges {
                if seen_paths.contains(&to) {
                    continue;
                }
                rank += 1;
                if let Some(cand) = candidate_for_target(store, &rel, &to, rank, detail)? {
                    seen_paths.insert(to.clone());
                    next_frontier.push(to);
                    added.push(cand);
                }
                if added.len() >= opts.max_added {
                    return Ok(added);
                }
            }
        }
        frontier = next_frontier;
        if frontier.is_empty() {
            break;
        }
    }
    Ok(added)
}
