//! Individual retrieval strategies. Each returns ranked candidates with a
//! single [`Signal`]; fusion combines them.

use crate::query::QueryAnalysis;
use crate::types::{Signal, SourceCandidate, StrategyKind};
use ctx_core::Result;
use ctx_embeddings::EmbeddingService;
use ctx_storage::{fts_quote, ChunkRecord, IndexStore, SymbolMatchMode};
use std::collections::HashMap;

pub fn candidate_from_chunk(chunk: &ChunkRecord, signal: Signal) -> SourceCandidate {
    SourceCandidate {
        chunk_id: Some(chunk.id.clone()),
        path: chunk.path.clone(),
        symbol: chunk.symbol.clone(),
        symbol_kind: chunk.symbol_kind.clone(),
        language: chunk.language.clone(),
        start_byte: chunk.start_byte,
        end_byte: chunk.end_byte,
        start_line: chunk.start_line,
        end_line: chunk.end_line,
        content_hash: chunk.content_hash.clone(),
        file_hash: chunk.file_hash.clone(),
        content: Some(chunk.content.clone()),
        signals: vec![signal],
        fused_score: 0.0,
        reasons: Vec::new(),
    }
}

/// Build an FTS5 MATCH expression: quoted phrases are required, other
/// terms are optional (OR) with prefix matching for longer identifiers.
pub fn build_fts_expression(a: &QueryAnalysis, prefix_matching: bool) -> Option<String> {
    let mut required: Vec<String> = Vec::new();
    for q in &a.quoted {
        let cleaned: String = q.chars().map(|c| if c.is_alphanumeric() || c == '_' || c == ' ' { c } else { ' ' }).collect();
        let cleaned = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
        if !cleaned.is_empty() {
            required.push(fts_quote(&cleaned));
        }
    }
    let mut optional: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for term in a.keywords.iter().chain(a.symbols.iter()) {
        let t = term.to_lowercase();
        let t = t.trim_matches(|c: char| !(c.is_alphanumeric() || c == '_')).to_string();
        if t.len() < 2 || !seen.insert(t.clone()) {
            continue;
        }
        if prefix_matching && t.len() >= 5 {
            optional.push(format!("{}*", fts_quote(&t)));
        } else {
            optional.push(fts_quote(&t));
        }
    }
    for f in &a.filenames {
        if let Some(stem) = f.split('.').next() {
            let s = stem.to_lowercase();
            if seen.insert(s.clone()) {
                optional.push(fts_quote(&s));
            }
        }
    }
    if required.is_empty() && optional.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    if !required.is_empty() {
        parts.push(required.join(" AND "));
    }
    if !optional.is_empty() {
        parts.push(format!("({})", optional.join(" OR ")));
    }
    Some(parts.join(" AND "))
}

pub fn lexical(store: &IndexStore, a: &QueryAnalysis, limit: usize, prefix_matching: bool) -> Result<Vec<SourceCandidate>> {
    let Some(expr) = build_fts_expression(a, prefix_matching) else { return Ok(Vec::new()) };
    let mut hits = store.search_lexical(&expr, limit)?;
    if hits.is_empty() && !a.quoted.is_empty() {
        // Required phrase not found verbatim: relax to optional terms.
        let relaxed = QueryAnalysis { quoted: vec![], ..a.clone() };
        if let Some(expr) = build_fts_expression(&relaxed, prefix_matching) {
            hits = store.search_lexical(&expr, limit)?;
        }
    }
    Ok(hits
        .into_iter()
        .enumerate()
        .map(|(i, h)| candidate_from_chunk(&h.chunk, Signal { strategy: StrategyKind::Lexical, rank: i + 1, score: h.score, detail: format!("bm25={:.2}", h.score) }))
        .collect())
}

pub fn paths(store: &IndexStore, a: &QueryAnalysis, limit: usize) -> Result<Vec<SourceCandidate>> {
    let mut scored: Vec<(String, f64, String)> = Vec::new();
    let mut consider = |needle: &str, weight: f64, store: &IndexStore| -> Result<()> {
        if needle.len() < 3 {
            return Ok(());
        }
        for f in store.search_paths(needle, 20)? {
            let name = f.path.rsplit('/').next().unwrap_or(&f.path).to_lowercase();
            let n = needle.to_lowercase();
            let quality = if f.path.to_lowercase() == n || name == n {
                1.0
            } else if name.split('.').next() == Some(n.as_str()) || f.path.to_lowercase().ends_with(&format!("/{n}")) {
                0.9
            } else if f.path.to_lowercase().split('/').any(|seg| seg == n || seg.split('.').next() == Some(n.as_str())) {
                0.7
            } else {
                0.4
            };
            let score = quality * weight;
            match scored.iter_mut().find(|(p, _, _)| *p == f.path) {
                Some(entry) => {
                    if score > entry.1 {
                        entry.1 = score;
                        entry.2 = format!("path matches `{needle}`");
                    }
                }
                None => scored.push((f.path.clone(), score, format!("path matches `{needle}`"))),
            }
        }
        Ok(())
    };
    for p in &a.paths {
        consider(p, 1.0, store)?;
    }
    for f in &a.filenames {
        consider(f, 1.0, store)?;
        if let Some(stem) = f.split('.').next() {
            consider(stem, 0.9, store)?;
        }
    }
    for s in &a.symbols {
        consider(s, 0.8, store)?;
    }
    for k in &a.keywords {
        if k.len() >= 4 {
            consider(k, 0.5, store)?;
        }
    }
    scored.sort_by(|x, y| y.1.partial_cmp(&x.1).unwrap_or(std::cmp::Ordering::Equal).then_with(|| x.0.len().cmp(&y.0.len())));
    scored.truncate(limit);
    let mut out = Vec::new();
    for (rank, (path, score, detail)) in scored.into_iter().enumerate() {
        // Represent the file by its leading chunk(s); fusion merges with
        // more specific chunk-level hits from other strategies.
        let chunks = store.chunks_in_file(&path)?;
        for (j, ch) in chunks.iter().take(2).enumerate() {
            out.push(candidate_from_chunk(ch, Signal { strategy: StrategyKind::Path, rank: rank + 1 + j, score, detail: detail.clone() }));
        }
    }
    Ok(out)
}

pub fn symbols(store: &IndexStore, a: &QueryAnalysis, limit: usize) -> Result<Vec<SourceCandidate>> {
    let mut names: Vec<String> = a.symbols.clone();
    // Also try plain keywords that look like identifiers in the index.
    for k in &a.keywords {
        if k.len() >= 4 && !names.iter().any(|n| n.eq_ignore_ascii_case(k)) {
            names.push(k.clone());
        }
    }
    let mut out: Vec<SourceCandidate> = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut rank = 0usize;
    for (i, name) in names.iter().enumerate() {
        let is_explicit = i < a.symbols.len();
        let bare = name.rsplit("::").next().unwrap_or(name).rsplit('.').next().unwrap_or(name);
        let modes: &[(SymbolMatchMode, f64)] = if is_explicit {
            &[(SymbolMatchMode::Exact, 1.0), (SymbolMatchMode::CaseInsensitive, 0.9), (SymbolMatchMode::Prefix, 0.6)]
        } else {
            &[(SymbolMatchMode::CaseInsensitive, 0.7)]
        };
        for (mode, quality) in modes {
            let found = store.find_symbols(bare, *mode, 10)?;
            if found.is_empty() {
                continue;
            }
            for sym in found {
                let key = format!("{}:{}", sym.path, sym.start_byte);
                if seen.contains_key(&key) {
                    continue;
                }
                rank += 1;
                seen.insert(key, rank);
                let chunk = store.chunk_for_symbol(&sym.path, &sym.name)?.or_else(|| {
                    // Fall back to any chunk overlapping the symbol range.
                    store.chunks_in_file(&sym.path).ok().and_then(|cs| cs.into_iter().find(|c| c.start_byte <= sym.start_byte && c.end_byte >= sym.end_byte.min(c.end_byte)))
                });
                let detail = format!("{} `{}` ({:?})", sym.kind, sym.name, mode);
                match chunk {
                    Some(ch) => out.push(candidate_from_chunk(&ch, Signal { strategy: StrategyKind::Symbol, rank, score: *quality, detail })),
                    None => out.push(SourceCandidate {
                        chunk_id: None,
                        path: sym.path.clone(),
                        symbol: Some(sym.name.clone()),
                        symbol_kind: Some(sym.kind.clone()),
                        language: None,
                        start_byte: sym.start_byte,
                        end_byte: sym.end_byte,
                        start_line: sym.start_line,
                        end_line: sym.end_line,
                        content_hash: String::new(),
                        file_hash: String::new(),
                        content: None,
                        signals: vec![Signal { strategy: StrategyKind::Symbol, rank, score: *quality, detail }],
                        fused_score: 0.0,
                        reasons: vec![],
                    }),
                }
                if out.len() >= limit {
                    return Ok(out);
                }
            }
            // Stop at the first mode that produced results for this name.
            break;
        }
    }
    Ok(out)
}

pub fn semantic(store: &IndexStore, embeddings: &EmbeddingService, query: &str, limit: usize) -> Result<Vec<SourceCandidate>> {
    let mut out = Vec::new();
    for (i, m) in embeddings.search(query, limit).map_err(|e| e)?.into_iter().enumerate() {
        if m.score < 0.05 {
            continue;
        }
        if let Some(ch) = store.get_chunk(&m.id)? {
            out.push(candidate_from_chunk(&ch, Signal { strategy: StrategyKind::Semantic, rank: i + 1, score: m.score as f64, detail: format!("cosine={:.2}", m.score) }));
        }
    }
    Ok(out)
}

/// Working-set files from the active task become candidates with a modest
/// signal so continuity is preserved across turns.
pub fn working_set(store: &IndexStore, paths: &[String], limit: usize) -> Result<Vec<SourceCandidate>> {
    let mut out = Vec::new();
    for (i, p) in paths.iter().rev().take(limit).enumerate() {
        if let Some(ch) = store.chunks_in_file(p)?.into_iter().next() {
            out.push(candidate_from_chunk(&ch, Signal { strategy: StrategyKind::WorkingSet, rank: i + 1, score: 0.5, detail: "in the active task's working set".into() }));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::analyze_query;

    #[test]
    fn fts_expression_requires_quotes_and_prefixes_long_terms() {
        let a = analyze_query("show the \"Sign in\" label on LoginButton");
        let expr = build_fts_expression(&a, true).unwrap();
        assert!(expr.starts_with("\"Sign in\" AND ("), "{expr}");
        assert!(expr.contains("\"loginbutton\"*") || expr.contains("\"login\""), "{expr}");
        let a = analyze_query("the");
        assert!(build_fts_expression(&a, true).is_none());
    }
}
