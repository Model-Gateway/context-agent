//! The context compiler (spec §8.2–§8.4).

use crate::budget::ContextBudget;
use crate::compaction::TaskCheckpoint;
use crate::explain::{ContextExplanation, ExcludedExplanation, MemoryExplanation, SourceExplanation};
use crate::policy::{policy_hash, SYSTEM_POLICY};
use crate::retention::prune_conversation;
use ctx_core::{estimate_tokens, AgentMessage, CacheHint, ContextConfig, ContextPayload, ContextSection, ContextSectionKind, ContextStats, ModelRequest, ToolDefinition, TrustLevel};
use ctx_memory::TaskState;
use ctx_retrieval::RetrievalResult;

pub struct CompileInput<'a> {
    pub state: &'a TaskState,
    /// Active conversation for this task, ending with the latest user
    /// message (or the tool results the model is waiting on).
    pub conversation: &'a [AgentMessage],
    pub retrieval: Option<&'a RetrievalResult>,
    pub tools: Vec<ToolDefinition>,
    /// Stable per-repository summary (languages, validation commands,
    /// conventions). Part of the trusted instructions.
    pub repository_summary: Option<&'a str>,
    /// Dynamic trusted instructions for this task (permission profile notes,
    /// stage guidance).
    pub task_instructions: Option<&'a str>,
    pub checkpoint: Option<&'a TaskCheckpoint>,
    pub model_window: usize,
    pub max_output_tokens: usize,
    pub config: &'a ContextConfig,
    pub prompt_cache_key: Option<String>,
    pub index_generation: u64,
    /// Extra diagnostics sections (freshness warnings, injected tool notes).
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CompiledContext {
    pub request: ModelRequest,
    pub stats: ContextStats,
    pub explanation: ContextExplanation,
    /// The prompt crossed the compaction threshold; the caller should
    /// checkpoint and rebuild before the next request.
    pub needs_compaction: bool,
}

pub struct ContextCompiler;

impl ContextCompiler {
    pub fn compile(input: CompileInput<'_>) -> CompiledContext {
        let config = input.config;
        let mut explanation = ContextExplanation { policy_hash: policy_hash(), index_generation: input.index_generation, ..Default::default() };

        // ----- stable prefix -----------------------------------------------
        let policy = AgentMessage::policy(SYSTEM_POLICY);
        let tools_tokens: usize = input.tools.iter().map(|t| estimate_tokens(&t.name) + estimate_tokens(&t.description) + estimate_tokens(&t.parameters.to_string())).sum();
        let stable_prefix_tokens = estimate_tokens(SYSTEM_POLICY) + tools_tokens;

        // ----- dynamic trusted instructions ---------------------------------
        let mut instructions = String::new();
        if let Some(summary) = input.repository_summary {
            instructions.push_str(summary.trim());
            instructions.push_str("\n\n");
        }
        if let Some(extra) = input.task_instructions {
            instructions.push_str(extra.trim());
            instructions.push('\n');
        }
        let instructions_msg = if instructions.trim().is_empty() { None } else { Some(AgentMessage::task_instructions(instructions.trim())) };
        let instructions_tokens = instructions_msg.as_ref().map(|m| estimate_tokens(&m.text_for_estimate())).unwrap_or(0);

        // ----- conversation with retention ------------------------------------
        let (conversation, prune) = prune_conversation(input.conversation, config);
        let conversation_tokens: usize = conversation.iter().map(|m| estimate_tokens(&m.text_for_estimate())).sum();
        explanation.pruned_tool_results = prune.elided_tool_results;
        explanation.truncated_tool_results = prune.truncated_tool_results;

        // ----- budget -------------------------------------------------------------
        let budget = ContextBudget::compute(config, input.state.category, input.model_window, input.max_output_tokens, stable_prefix_tokens, instructions_tokens, conversation_tokens);
        explanation.budget = budget;
        explanation.task_category = Some(input.state.category);

        // ----- context payload -------------------------------------------------
        let mut sections: Vec<ContextSection> = Vec::new();
        let mut repository_tokens = 0usize;
        let mut memory_tokens = 0usize;
        let mut snippets = 0usize;
        let mut memory_records = 0usize;

        if let Some(cp) = input.checkpoint {
            sections.push(ContextSection {
                kind: ContextSectionKind::TaskCheckpoint,
                trust: TrustLevel::Trusted,
                title: format!("checkpoint {} ({} earlier messages compacted)", cp.id.short(), cp.dropped_messages),
                body: cp.summary.clone(),
                rationale: Some(cp.reason.clone()),
            });
            explanation.compacted = true;
            explanation.checkpoint_id = Some(cp.id.to_string());
        }

        if let Some(r) = input.retrieval {
            explanation.retrieval_confidence = Some(r.confidence.as_str().to_string());
            explanation.confidence_rationale = Some(r.diagnostics.confidence_rationale.clone());
            explanation.query_summary = Some(format!(
                "files={:?} symbols={:?} quoted={:?} keywords={} scope={:?}",
                r.analysis.filenames,
                r.analysis.symbols.iter().take(6).collect::<Vec<_>>(),
                r.analysis.quoted,
                r.analysis.keywords.len(),
                r.analysis.scope
            ));
            explanation.strategy_timings = r.diagnostics.strategies.iter().map(|s| (s.strategy.clone(), s.hits, s.duration_ms)).collect();
            explanation.stale_refreshed = r.diagnostics.stale_refreshed;
            explanation.dropped_missing = r.diagnostics.dropped_missing;

            let mut used = 0usize;
            let mut remaining_candidates: Vec<String> = Vec::new();
            for snippet in &r.context {
                let tokens = estimate_tokens(&snippet.content);
                if used + tokens > budget.retrieval && snippets > 0 {
                    explanation.excluded.push(ExcludedExplanation { path: snippet.source.path.clone(), start_line: snippet.source.start_line, end_line: snippet.source.end_line, symbol: snippet.symbol.clone(), score: snippet.score, reason: "compiler budget exhausted".into() });
                    remaining_candidates.push(snippet.source.display_range());
                    continue;
                }
                used += tokens;
                snippets += 1;
                let title = format!(
                    "{} (lines {}-{}{}) [{}]",
                    snippet.source.path,
                    snippet.source.start_line,
                    snippet.source.end_line,
                    snippet.symbol.as_ref().map(|s| format!(", {s}")).unwrap_or_default(),
                    &snippet.source.file_hash.trim_start_matches("sha256:")[..8]
                );
                sections.push(ContextSection { kind: ContextSectionKind::RepositorySource, trust: TrustLevel::Untrusted, title, body: snippet.content.clone(), rationale: Some(snippet.rationale.clone()) });
                explanation.selected_sources.push(SourceExplanation {
                    path: snippet.source.path.clone(),
                    start_line: snippet.source.start_line,
                    end_line: snippet.source.end_line,
                    symbol: snippet.symbol.clone(),
                    score: snippet.score,
                    strategies: snippet.rationale.split(", ").filter(|p| p.contains('#')).map(|p| p.split('#').next().unwrap_or(p).to_string()).collect(),
                    reasons: snippet.rationale.split(", ").filter(|p| !p.contains('#')).map(|p| p.to_string()).collect(),
                    tokens,
                    refreshed: snippet.rationale.contains("refreshed"),
                });
            }
            repository_tokens = used;
            for e in &r.diagnostics.excluded {
                explanation.excluded.push(ExcludedExplanation { path: e.path.clone(), start_line: e.start_line, end_line: e.end_line, symbol: e.symbol.clone(), score: e.fused_score, reason: e.reason.clone() });
            }
            // Candidates that were ranked but not included, as leads.
            let shown: std::collections::HashSet<String> = r.context.iter().map(|s| s.source.display_range()).collect();
            for c in r.candidates.iter().filter(|c| !shown.contains(&c.source_ref().display_range())).take(10) {
                remaining_candidates.push(format!("{}:{}-{}{}", c.path, c.start_line, c.end_line, c.symbol.as_ref().map(|s| format!(" ({s})")).unwrap_or_default()));
            }
            let mut summary = String::new();
            summary.push_str(r.confidence.guidance());
            summary.push('\n');
            if !remaining_candidates.is_empty() {
                summary.push_str("Other plausible locations (not included; read_file to inspect): ");
                summary.push_str(&remaining_candidates.join("; "));
                summary.push('\n');
            }
            if r.diagnostics.stale_refreshed > 0 {
                summary.push_str(&format!("{} snippet(s) were refreshed because the files changed since indexing.\n", r.diagnostics.stale_refreshed));
            }
            sections.push(ContextSection { kind: ContextSectionKind::RetrievalSummary, trust: TrustLevel::Trusted, title: format!("confidence {}", r.confidence.as_str()), body: summary, rationale: None });

            // Memories under their own budget.
            let mut mem_used = 0usize;
            for (m, score) in &r.memories {
                let body = format!("[{} | {}] {}", m.category.as_str(), m.verification.as_str(), m.content);
                let tokens = estimate_tokens(&body);
                let included = mem_used + tokens <= budget.memory && m.is_current();
                explanation.memories.push(MemoryExplanation { id: m.id.to_string(), category: m.category.as_str().into(), verification: m.verification.as_str().into(), score: *score, tokens, preview: m.content.chars().take(100).collect(), included });
                if !included {
                    continue;
                }
                mem_used += tokens;
                memory_records += 1;
                let evidence = m.evidence_paths().collect::<Vec<_>>().join(", ");
                sections.push(ContextSection { kind: ContextSectionKind::Memory, trust: TrustLevel::Untrusted, title: format!("{} ({}){}", m.category.as_str(), m.verification.as_str(), if evidence.is_empty() { String::new() } else { format!(" evidence: {evidence}") }), body: m.content.clone(), rationale: Some(format!("score {score:.3}")) });
            }
            memory_tokens = mem_used;
        }
        for d in &input.diagnostics {
            sections.push(ContextSection { kind: ContextSectionKind::Diagnostics, trust: TrustLevel::Trusted, title: "runtime".into(), body: d.clone(), rationale: None });
        }

        // ----- assemble -------------------------------------------------------
        let mut messages: Vec<AgentMessage> = vec![policy];
        let stable_prefix_len = 1;
        if let Some(m) = instructions_msg {
            messages.push(m);
        }
        let payload = ContextPayload { sections };
        let payload_tokens = if payload.is_empty() { 0 } else { estimate_tokens(&payload.render()) };
        if !payload.is_empty() {
            messages.push(AgentMessage::Context(payload));
        }
        messages.extend(conversation.iter().cloned());

        let estimated_prompt_tokens = stable_prefix_tokens + instructions_tokens + payload_tokens + conversation_tokens;
        let stats = ContextStats {
            estimated_prompt_tokens,
            stable_prefix_tokens,
            repository_snippets: snippets,
            repository_tokens,
            memory_records,
            memory_tokens,
            conversation_messages: conversation.len(),
            conversation_tokens,
            retrieval_budget_tokens: budget.retrieval,
            context_window: input.model_window,
            compacted: input.checkpoint.is_some(),
        };
        let needs_compaction = budget.over_threshold(config, estimated_prompt_tokens) || conversation_tokens > input.model_window / 2;
        if needs_compaction {
            explanation.warnings.push(format!("prompt estimate {estimated_prompt_tokens} exceeds the compaction threshold; a checkpoint will be taken at the next safe boundary"));
        }
        explanation.stats = stats.clone();
        explanation.stable_prefix_messages = stable_prefix_len;
        explanation.prompt_cache_key = input.prompt_cache_key.clone();

        let mut request = ModelRequest::new(messages, input.tools);
        request.max_output_tokens = Some(input.max_output_tokens as u32);
        request.cache = CacheHint { stable_prefix_len, prompt_cache_key: input.prompt_cache_key };
        request.tags.insert("policy_hash".into(), policy_hash());
        request.tags.insert("task_id".into(), input.state.task_id.to_string());
        CompiledContext { request, stats, explanation, needs_compaction }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ctx_core::{SourceRef, TaskCategory, TaskId, VerifiedSourceSnippet};
    use ctx_retrieval::{analyze_query, RetrievalConfidence, RetrievalDiagnostics};

    fn retrieval() -> RetrievalResult {
        let content = "export function LoginButton() {}";
        RetrievalResult {
            candidates: vec![],
            context: vec![VerifiedSourceSnippet {
                source: SourceRef::whole_file("src/LoginButton.tsx", content),
                content: content.into(),
                symbol: Some("LoginButton".into()),
                language: Some("tsx".into()),
                rationale: "symbol#1, lexical#2, exact filename match".into(),
                score: 0.05,
            }],
            memories: vec![],
            confidence: RetrievalConfidence::High,
            index_generation: 3,
            diagnostics: RetrievalDiagnostics { confidence_rationale: "exact".into(), ..Default::default() },
            analysis: analyze_query("LoginButton.tsx spinner"),
        }
    }

    #[test]
    fn layers_are_ordered_and_trust_preserved() {
        let state = TaskState::new(TaskId::new(), "Add a spinner to LoginButton.tsx", TaskCategory::LocalizedEdit);
        let conversation = vec![AgentMessage::user("Add a spinner to LoginButton.tsx")];
        let r = retrieval();
        let compiled = ContextCompiler::compile(CompileInput {
            state: &state,
            conversation: &conversation,
            retrieval: Some(&r),
            tools: vec![ToolDefinition { name: "read_file".into(), description: "read".into(), parameters: serde_json::json!({"type": "object"}) }],
            repository_summary: Some("Languages: typescript. Validation: npm test"),
            task_instructions: Some("Permission profile: approval_required."),
            checkpoint: None,
            model_window: 128_000,
            max_output_tokens: 16_000,
            config: &ContextConfig::default(),
            prompt_cache_key: Some("repo-1".into()),
            index_generation: 3,
            diagnostics: vec![],
        });
        let m = &compiled.request.messages;
        assert!(matches!(m[0], AgentMessage::Policy(_)));
        assert!(matches!(m[1], AgentMessage::TaskInstructions(_)));
        assert!(matches!(m[2], AgentMessage::Context(_)));
        assert!(matches!(m[3], AgentMessage::User(_)));
        if let AgentMessage::Context(p) = &m[2] {
            assert!(p.sections.iter().any(|s| s.kind == ContextSectionKind::RepositorySource && s.trust == TrustLevel::Untrusted));
            assert!(p.sections.iter().any(|s| s.kind == ContextSectionKind::RetrievalSummary && s.body.contains("HIGH")));
        }
        assert_eq!(compiled.request.cache.stable_prefix_len, 1);
        assert_eq!(compiled.request.cache.prompt_cache_key.as_deref(), Some("repo-1"));
        assert_eq!(compiled.stats.repository_snippets, 1);
        assert!(!compiled.needs_compaction);
        assert_eq!(compiled.explanation.selected_sources[0].strategies, vec!["symbol", "lexical"]);
        let text = compiled.explanation.render_explain();
        assert!(text.contains("Selected sources"));
        assert!(text.contains("src/LoginButton.tsx:1-1"));
    }

    #[test]
    fn compaction_flag_and_checkpoint_section() {
        let state = TaskState::new(TaskId::new(), "big task", TaskCategory::ComplexFeature);
        let mut conversation = vec![AgentMessage::user("big task")];
        for i in 0..40 {
            conversation.push(AgentMessage::assistant_text("x".repeat(3_000)));
            conversation.push(AgentMessage::user(format!("more {i}")));
        }
        let cfg = ContextConfig { keep_recent_turns: 100, ..Default::default() };
        let (cp, kept) = crate::compaction::compact(&state, &conversation, 2, 50, "threshold");
        let compiled = ContextCompiler::compile(CompileInput {
            state: &state,
            conversation: &conversation,
            retrieval: None,
            tools: vec![],
            repository_summary: None,
            task_instructions: None,
            checkpoint: None,
            model_window: 32_000,
            max_output_tokens: 4_000,
            config: &cfg,
            prompt_cache_key: None,
            index_generation: 0,
            diagnostics: vec![],
        });
        assert!(compiled.needs_compaction);
        let after = ContextCompiler::compile(CompileInput {
            state: &state,
            conversation: &kept,
            retrieval: None,
            tools: vec![],
            repository_summary: None,
            task_instructions: None,
            checkpoint: Some(&cp),
            model_window: 32_000,
            max_output_tokens: 4_000,
            config: &cfg,
            prompt_cache_key: None,
            index_generation: 0,
            diagnostics: vec![],
        });
        assert!(!after.needs_compaction);
        assert!(after.stats.compacted);
        if let AgentMessage::Context(p) = &after.request.messages[1] {
            assert_eq!(p.sections[0].kind, ContextSectionKind::TaskCheckpoint);
        } else {
            panic!("expected checkpoint context");
        }
    }
}
