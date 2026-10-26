# Architecture

```
CLI / TUI (ctx-cli)
      │
Session manager ── task boundary detection (ctx-memory)
      │
Agent runtime (ctx-agent)
      │                     ┌──────────── Context compiler (ctx-context)
      ├── retrieval ────────┤  stable prefix · instructions · untrusted payload · retained history
      │   (ctx-retrieval)   └──────────── budget · retention · compaction · explain
      ├── model adapter (ctx-providers) ── OpenAI-compatible HTTP/SSE
      └── tool executor (ctx-tools) ── policy (ctx-security) ── approvals
                │
        workspace files · shell · git · validation
                │
        indexer (ctx-indexing) ── languages (tree-sitter) · embeddings · storage (SQLite)
```

## Data flow for one model call

1. `SessionManager::resolve_task` classifies the message (`Continue`, `Refine`, `NewTask`, `ReferEarlier`, `Reverse`) using deterministic cues and keyword overlap with the active and recent tasks, preserving continuity when uncertain.
2. `RepositoryContext::ensure_indexed` performs a hash-gated reconcile so retrieval never runs against stale records; a branch switch invalidates source-derived memories.
3. `RetrievalEngine::retrieve` runs strategies on scoped threads, fuses with RRF plus exact-match, agreement, working-set, scope and freshness signals, expands one or two hops along resolved relationships, re-verifies each candidate's file hash (re-indexing changed files and relocating chunks), and selects snippets under the category budget. Diagnostics explain every inclusion and exclusion.
4. `ContextCompiler::compile` lays out the layers. Trusted instructions are only the policy, the repository summary and runtime notes; repository source, memories and checkpoints are rendered inside a payload whose preamble and delimiters mark them as data. The compiler reports estimated tokens per layer and flags when compaction is needed.
5. The provider adapter maps the internal messages to the wire format (policy/instructions → `system`, payload → `user`, tool calls and results preserved by id), streams deltas, and normalizes usage, finish reasons, malformed tool arguments and errors.
6. `ToolRegistry::execute` validates arguments against the schema, maps the call to a security action, evaluates the permission profile, asks for approval when required, runs the tool, redacts and truncates the output, and returns the effects the runtime uses to update task state.
7. Between steps the runtime persists conversation, events, model-request usage, validation runs and the task state; it compacts into a checkpoint at a safe boundary when the prompt crosses the threshold.

## Storage layout

`~/.context-agent/repositories/<repository-id>/metadata.sqlite` holds files, symbols, chunks (+FTS5), relationships and embeddings; `sessions.sqlite` holds sessions, tasks, conversation, events, memories (+FTS5), checkpoints, model requests, validation runs and metrics. Capability profiles live under `~/.context-agent/providers/`.

## Extension points

- New language: add a `LanguageSpec` under `crates/languages/src/specs/` and register it in `all_specs()`.
- New provider transport: implement `ModelProvider` and extend `build_provider`.
- New ranking algorithm: implement `Ranker` and pass it to `RetrievalEngine::with_ranker`.
- New embedder: implement `Embedder` and extend `build_embedder`.
- New tool: implement `Tool` (schema, action mapping, effects) and add it to `ToolRegistry::standard`.
