//! Schema definitions.

use crate::db::Migration;

pub const INDEX_MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    sql: r#"
CREATE TABLE IF NOT EXISTS index_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
INSERT OR IGNORE INTO index_meta (key, value) VALUES ('generation', '0');

CREATE TABLE IF NOT EXISTS repositories (
    id TEXT PRIMARY KEY,
    root TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS worktrees (
    id INTEGER PRIMARY KEY,
    repository_id TEXT NOT NULL,
    path TEXT NOT NULL,
    branch TEXT,
    head TEXT,
    updated_at TEXT NOT NULL,
    UNIQUE(repository_id, path)
);

CREATE TABLE IF NOT EXISTS files (
    path TEXT PRIMARY KEY,
    language TEXT,
    kind TEXT NOT NULL DEFAULT 'source',
    size INTEGER NOT NULL,
    hash TEXT NOT NULL,
    mtime INTEGER,
    generation INTEGER NOT NULL,
    indexed_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS files_language ON files(language);

CREATE TABLE IF NOT EXISTS symbols (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    parent TEXT,
    start_byte INTEGER NOT NULL,
    end_byte INTEGER NOT NULL,
    start_line INTEGER NOT NULL,
    end_line INTEGER NOT NULL,
    signature TEXT,
    generation INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS symbols_name ON symbols(name);
CREATE INDEX IF NOT EXISTS symbols_name_nocase ON symbols(name COLLATE NOCASE);
CREATE INDEX IF NOT EXISTS symbols_path ON symbols(path);

CREATE TABLE IF NOT EXISTS chunks (
    id TEXT PRIMARY KEY,
    path TEXT NOT NULL,
    language TEXT,
    symbol TEXT,
    symbol_kind TEXT,
    start_byte INTEGER NOT NULL,
    end_byte INTEGER NOT NULL,
    start_line INTEGER NOT NULL,
    end_line INTEGER NOT NULL,
    content_hash TEXT NOT NULL,
    file_hash TEXT NOT NULL,
    content TEXT NOT NULL,
    generation INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS chunks_path ON chunks(path);
CREATE INDEX IF NOT EXISTS chunks_symbol ON chunks(symbol);

CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
    content,
    tokens,
    path,
    symbol,
    chunk_id UNINDEXED,
    tokenize = 'unicode61 tokenchars ''_'''
);

CREATE TABLE IF NOT EXISTS relationships (
    id INTEGER PRIMARY KEY,
    from_path TEXT NOT NULL,
    from_symbol TEXT,
    kind TEXT NOT NULL,
    target TEXT NOT NULL,
    to_path TEXT,
    to_symbol TEXT,
    confidence REAL NOT NULL,
    source TEXT NOT NULL,
    generation INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS relationships_from ON relationships(from_path);
CREATE INDEX IF NOT EXISTS relationships_to ON relationships(to_path);
CREATE INDEX IF NOT EXISTS relationships_to_symbol ON relationships(to_symbol);

CREATE TABLE IF NOT EXISTS embeddings (
    chunk_id TEXT PRIMARY KEY,
    content_hash TEXT NOT NULL,
    model_id TEXT NOT NULL,
    dimension INTEGER NOT NULL,
    vector BLOB NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS embeddings_model ON embeddings(model_id);
"#,
}];

pub const SESSION_MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    sql: r#"
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    repository_id TEXT NOT NULL,
    mode TEXT NOT NULL,
    model TEXT NOT NULL,
    active_task_id TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    metadata TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    repository_id TEXT NOT NULL,
    objective TEXT NOT NULL,
    category TEXT NOT NULL,
    state TEXT NOT NULL,
    status TEXT,
    parent_task_id TEXT,
    summary TEXT,
    state_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    completed_at TEXT
);
CREATE INDEX IF NOT EXISTS tasks_session ON tasks(session_id);
CREATE INDEX IF NOT EXISTS tasks_updated ON tasks(updated_at);

CREATE TABLE IF NOT EXISTS events (
    id INTEGER PRIMARY KEY,
    session_id TEXT,
    task_id TEXT,
    kind TEXT NOT NULL,
    payload TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS events_task ON events(task_id, id);
CREATE INDEX IF NOT EXISTS events_session ON events(session_id, id);

CREATE TABLE IF NOT EXISTS conversation (
    id INTEGER PRIMARY KEY,
    session_id TEXT NOT NULL,
    task_id TEXT,
    message TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS conversation_task ON conversation(task_id, id);
CREATE INDEX IF NOT EXISTS conversation_session ON conversation(session_id, id);

CREATE TABLE IF NOT EXISTS memories (
    id TEXT PRIMARY KEY,
    repository_id TEXT NOT NULL,
    task_id TEXT,
    category TEXT NOT NULL,
    content TEXT NOT NULL,
    evidence TEXT NOT NULL DEFAULT '[]',
    verification TEXT NOT NULL,
    tags TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    invalidated INTEGER NOT NULL DEFAULT 0,
    invalidation_reason TEXT
);
CREATE INDEX IF NOT EXISTS memories_repo ON memories(repository_id, category);
CREATE INDEX IF NOT EXISTS memories_task ON memories(task_id);

CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
    content,
    tags,
    memory_id UNINDEXED,
    tokenize = 'unicode61 tokenchars ''_'''
);

CREATE TABLE IF NOT EXISTS checkpoints (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    session_id TEXT,
    reason TEXT NOT NULL,
    summary_json TEXT NOT NULL,
    event_seq INTEGER NOT NULL,
    conversation_seq INTEGER NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS checkpoints_task ON checkpoints(task_id, id);

CREATE TABLE IF NOT EXISTS model_requests (
    id INTEGER PRIMARY KEY,
    task_id TEXT,
    session_id TEXT,
    step INTEGER NOT NULL,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    prompt_tokens INTEGER,
    completion_tokens INTEGER,
    cached_tokens INTEGER,
    usage_source TEXT,
    latency_ms INTEGER NOT NULL,
    finish_reason TEXT,
    error TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS model_requests_task ON model_requests(task_id);

CREATE TABLE IF NOT EXISTS validation_runs (
    id INTEGER PRIMARY KEY,
    task_id TEXT NOT NULL,
    command TEXT NOT NULL,
    passed INTEGER NOT NULL,
    exit_code INTEGER,
    duration_ms INTEGER NOT NULL,
    summary TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS validation_runs_task ON validation_runs(task_id);

CREATE TABLE IF NOT EXISTS metrics (
    id INTEGER PRIMARY KEY,
    task_id TEXT,
    name TEXT NOT NULL,
    value REAL NOT NULL,
    unit TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS metrics_task ON metrics(task_id, name);
"#,
}, Migration {
    version: 2,
    // What each request cost (USD; NULL when no price was known) and the
    // gateway's id for it, whose billed amount replaces the estimate.
    sql: r#"
ALTER TABLE model_requests ADD COLUMN cost_usd REAL;
ALTER TABLE model_requests ADD COLUMN gateway_request_id TEXT;
CREATE INDEX IF NOT EXISTS model_requests_created ON model_requests(created_at);
CREATE INDEX IF NOT EXISTS model_requests_gateway ON model_requests(gateway_request_id);
"#,
}];
