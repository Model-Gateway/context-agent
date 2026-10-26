//! Search tools: search_code (hybrid with text fallback), find_symbol,
//! find_references.

use crate::files::walk_files;
use crate::registry::{Tool, ToolContext, ToolEffect, ToolOutput};
use async_trait::async_trait;
use ctx_core::{CtxError, Result};
use ctx_retrieval::{RetrievalRequest, RetrievalStage};
use ctx_security::{Action, ActionKind};
use ctx_storage::SymbolMatchMode;
use grep_regex::RegexMatcherBuilder;
use grep_searcher::sinks::UTF8;
use grep_searcher::SearcherBuilder;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct GrepHit {
    pub path: String,
    pub line: u64,
    pub text: String,
}

/// Regex/text search over indexable files using ripgrep's libraries.
pub fn grep_files(ctx: &ToolContext, pattern: &str, is_regex: bool, case_sensitive: bool, path_prefix: Option<&str>, limit: usize) -> Result<(Vec<GrepHit>, usize)> {
    let expr = if is_regex { pattern.to_string() } else { regex::escape(pattern) };
    let matcher = RegexMatcherBuilder::new().case_insensitive(!case_sensitive).build(&expr).map_err(|e| CtxError::invalid(format!("invalid pattern: {e}")))?;
    let mut searcher = SearcherBuilder::new().line_number(true).build();
    let mut hits = Vec::new();
    let mut searched = 0usize;
    for file in walk_files(ctx) {
        if let Some(p) = path_prefix.and_then(crate::files::normalize_prefix) {
            if !crate::files::under_prefix(&file.relative, &p) {
                continue;
            }
        }
        if ctx.cancel.is_cancelled() {
            return Err(CtxError::Cancelled);
        }
        searched += 1;
        let rel = file.relative.clone();
        let result = searcher.search_path(
            &matcher,
            &file.absolute,
            UTF8(|lnum, line| {
                hits.push(GrepHit { path: rel.clone(), line: lnum, text: line.trim_end().chars().take(240).collect() });
                Ok(hits.len() < limit)
            }),
        );
        if let Err(e) = result {
            tracing::debug!(path = %file.relative, error = %e, "grep skipped file");
        }
        if hits.len() >= limit {
            break;
        }
    }
    Ok((hits, searched))
}

fn format_hits(hits: &[GrepHit]) -> String {
    hits.iter().map(|h| format!("{}:{}: {}", h.path, h.line, h.text.trim())).collect::<Vec<_>>().join("\n")
}

// ---------------------------------------------------------------------------

pub struct SearchCode;

#[derive(Deserialize)]
struct SearchArgs {
    query: String,
    mode: Option<String>,
    path_prefix: Option<String>,
    limit: Option<usize>,
    #[serde(default)]
    case_sensitive: bool,
}

#[async_trait]
impl Tool for SearchCode {
    fn name(&self) -> &str {
        "search_code"
    }

    fn description(&self) -> &str {
        "Search the repository. mode=hybrid (default) ranks code locations by path, identifiers, lexical and semantic similarity to a natural-language query; mode=text finds literal occurrences; mode=regex finds regular-expression matches. Returns path:line locations with previews."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "minLength": 1},
                "mode": {"type": "string", "enum": ["hybrid", "text", "regex"]},
                "path_prefix": {"type": "string", "description": "Only search under this directory"},
                "limit": {"type": "integer", "minimum": 1, "maximum": 200, "description": "Maximum results (default 20)"},
                "case_sensitive": {"type": "boolean"}
            },
            "required": ["query"],
            "additionalProperties": false
        })
    }

    fn action(&self, args: &Value, _ctx: &ToolContext) -> Result<Action> {
        Ok(Action { tool: "search_code".into(), kind: ActionKind::Search, summary: format!("search {}", args["query"].as_str().unwrap_or_default()), details: String::new() })
    }

    async fn invoke(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let a: SearchArgs = serde_json::from_value(args)?;
        let limit = a.limit.unwrap_or(20);
        let mode = a.mode.as_deref().unwrap_or("hybrid");
        let indexed = ctx.indexer.as_ref().map(|i| i.store().stats().map(|s| s.files > 0).unwrap_or(false)).unwrap_or(false);
        if mode == "hybrid" && indexed {
            if let Some(engine) = &ctx.retrieval {
                let mut req = RetrievalRequest::new(a.query.clone(), 3_000);
                req.stage = RetrievalStage::Focused;
                req.max_candidates = limit.max(5);
                if let Some(p) = a.path_prefix.as_deref().and_then(crate::files::normalize_prefix) {
                    req.scope.push(p);
                }
                req.working_set = ctx.working_set.read().clone();
                let result = engine.retrieve(req).await?;
                let mut out = format!("Hybrid search for \"{}\" — confidence {} ({}).\n", a.query, result.confidence.as_str(), result.diagnostics.confidence_rationale);
                if result.candidates.is_empty() {
                    out.push_str("No indexed matches. Try mode=text for literal strings, or list_files to explore.\n");
                } else {
                    for (i, c) in result.candidates.iter().take(limit).enumerate() {
                        out.push_str(&format!("{:>2}. {}:{}-{}{} [{}]{}\n", i + 1, c.path, c.start_line, c.end_line, c.symbol.as_ref().map(|s| format!(" ({s})")).unwrap_or_default(), c.strategies().iter().map(|s| s.as_str()).collect::<Vec<_>>().join("+"), if c.reasons.is_empty() { String::new() } else { format!(" — {}", c.reasons.join("; ")) }));
                        if i < 5 {
                            if let Some(snippet) = result.context.iter().find(|s| s.source.path == c.path && s.source.start_byte == c.start_byte) {
                                for line in snippet.content.lines().take(3) {
                                    out.push_str(&format!("      {}\n", line.chars().take(160).collect::<String>()));
                                }
                            }
                        }
                    }
                }
                let hits = result.candidates.len();
                return Ok(ToolOutput::ok(out, format!("hybrid search: {hits} candidate(s)")).with_effect(ToolEffect::Search { query: a.query, hits }));
            }
        }
        let is_regex = mode == "regex";
        let (hits, searched) = grep_files(ctx, &a.query, is_regex, a.case_sensitive, a.path_prefix.as_deref(), limit)?;
        let mut out = format!("{} search for \"{}\" in {searched} file(s): {} match(es){}\n", if is_regex { "Regex" } else { "Text" }, a.query, hits.len(), if hits.len() >= limit { " (limit reached)" } else { "" });
        if mode == "hybrid" && !indexed {
            out.push_str("(index unavailable; fell back to text search)\n");
        }
        out.push_str(&format_hits(&hits));
        let n = hits.len();
        Ok(ToolOutput::ok(out, format!("{mode} search: {n} match(es)")).with_effect(ToolEffect::Search { query: a.query, hits: n }))
    }
}

// ---------------------------------------------------------------------------

pub struct FindSymbol;

#[derive(Deserialize)]
struct SymbolArgs {
    name: String,
    kind: Option<String>,
    limit: Option<usize>,
}

#[async_trait]
impl Tool for FindSymbol {
    fn name(&self) -> &str {
        "find_symbol"
    }

    fn description(&self) -> &str {
        "Locate definitions of a function, class, type, constant or other symbol by name (exact, then case-insensitive, then prefix). Falls back to a definition-pattern text search when the index has no match."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "minLength": 1},
                "kind": {"type": "string", "description": "Optional filter: function, method, class, struct, enum, interface, trait, module, type_alias, constant, variable, component"},
                "limit": {"type": "integer", "minimum": 1, "maximum": 100}
            },
            "required": ["name"],
            "additionalProperties": false
        })
    }

    fn action(&self, args: &Value, _ctx: &ToolContext) -> Result<Action> {
        Ok(Action { tool: "find_symbol".into(), kind: ActionKind::Search, summary: format!("find symbol {}", args["name"].as_str().unwrap_or_default()), details: String::new() })
    }

    async fn invoke(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let a: SymbolArgs = serde_json::from_value(args)?;
        let limit = a.limit.unwrap_or(20);
        let bare = a.name.rsplit("::").next().unwrap_or(&a.name).rsplit('.').next().unwrap_or(&a.name).to_string();
        if let Some(indexer) = &ctx.indexer {
            let store = indexer.store();
            let mut found = Vec::new();
            let mut mode_used = "exact";
            for (mode, label) in [(SymbolMatchMode::Exact, "exact"), (SymbolMatchMode::CaseInsensitive, "case-insensitive"), (SymbolMatchMode::Prefix, "prefix"), (SymbolMatchMode::Contains, "substring")] {
                found = store.find_symbols(&bare, mode, limit * 2)?;
                if let Some(kind) = &a.kind {
                    found.retain(|s| s.kind.eq_ignore_ascii_case(kind));
                }
                if !found.is_empty() {
                    mode_used = label;
                    break;
                }
            }
            if !found.is_empty() {
                found.truncate(limit);
                let mut out = format!("{} definition(s) for `{}` ({mode_used} match):\n", found.len(), a.name);
                for s in &found {
                    out.push_str(&format!("{}:{}-{} [{}] {}{}{}\n", s.path, s.start_line, s.end_line, s.kind, s.parent.as_ref().map(|p| format!("{p}.")).unwrap_or_default(), s.name, s.signature.as_ref().map(|sig| format!(" — {sig}")).unwrap_or_default()));
                }
                let n = found.len();
                return Ok(ToolOutput::ok(out, format!("find_symbol {}: {n} definition(s)", a.name)).with_effect(ToolEffect::Search { query: a.name, hits: n }));
            }
        }
        // Text fallback: common definition keywords followed by the name.
        let pattern = format!(r"(?:\b(?:fn|def|func|function|class|struct|interface|enum|trait|type|module|const|let|var|val|final|static|public|private|protected|void|impl|macro_rules!|@interface)\b[^\n;]{{0,80}}\b{}\b)|(?:\b{}\s*[:=]\s*(?:function|\(|async|class|\{{))", regex::escape(&bare), regex::escape(&bare));
        let (hits, searched) = grep_files(ctx, &pattern, true, true, None, limit)?;
        if hits.is_empty() {
            return Ok(ToolOutput::ok(format!("No definition of `{}` found in the index or by text search across {searched} file(s). Try search_code with mode=text for usages.", a.name), format!("find_symbol {}: none", a.name)).with_effect(ToolEffect::Search { query: a.name, hits: 0 }));
        }
        let mut out = format!("{} likely definition(s) of `{}` (text fallback; not index-verified):\n", hits.len(), a.name);
        out.push_str(&format_hits(&hits));
        let n = hits.len();
        Ok(ToolOutput::ok(out, format!("find_symbol {}: {n} text match(es)", a.name)).with_effect(ToolEffect::Search { query: a.name, hits: n }))
    }
}

// ---------------------------------------------------------------------------

pub struct FindReferences;

#[derive(Deserialize)]
struct RefArgs {
    name: String,
    limit: Option<usize>,
    path_prefix: Option<String>,
}

#[async_trait]
impl Tool for FindReferences {
    fn name(&self) -> &str {
        "find_references"
    }

    fn description(&self) -> &str {
        "Find where a symbol is used: indexed call/import relationships plus whole-word text occurrences across the repository."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "minLength": 1},
                "limit": {"type": "integer", "minimum": 1, "maximum": 300, "description": "Maximum occurrences (default 50)"},
                "path_prefix": {"type": "string"}
            },
            "required": ["name"],
            "additionalProperties": false
        })
    }

    fn action(&self, args: &Value, _ctx: &ToolContext) -> Result<Action> {
        Ok(Action { tool: "find_references".into(), kind: ActionKind::Search, summary: format!("find references to {}", args["name"].as_str().unwrap_or_default()), details: String::new() })
    }

    async fn invoke(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let a: RefArgs = serde_json::from_value(args)?;
        let limit = a.limit.unwrap_or(50);
        let bare = a.name.rsplit("::").next().unwrap_or(&a.name).rsplit('.').next().unwrap_or(&a.name).to_string();
        let mut out = String::new();
        if let Some(indexer) = &ctx.indexer {
            let rels = indexer.store().relationships_to_symbol(&bare, limit)?;
            if !rels.is_empty() {
                out.push_str(&format!("Indexed relationships to `{bare}` ({}):\n", rels.len()));
                for r in &rels {
                    out.push_str(&format!("{}{} {} `{}` (confidence {:.1})\n", r.from_path, r.from_symbol.as_ref().map(|s| format!(" in {s}")).unwrap_or_default(), r.kind, r.target, r.confidence));
                }
            }
        }
        let pattern = format!(r"\b{}\b", regex::escape(&bare));
        let (hits, searched) = grep_files(ctx, &pattern, true, true, a.path_prefix.as_deref(), limit)?;
        out.push_str(&format!("Text occurrences of `{bare}` in {searched} file(s): {}{}\n", hits.len(), if hits.len() >= limit { " (limit reached)" } else { "" }));
        out.push_str(&format_hits(&hits));
        let n = hits.len();
        Ok(ToolOutput::ok(out, format!("find_references {bare}: {n} occurrence(s)")).with_effect(ToolEffect::Search { query: a.name, hits: n }))
    }
}
