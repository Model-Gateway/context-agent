//! Index store: files, symbols, chunks, lexical search, relationships,
//! embeddings.

use crate::db::{now_text, parse_ts, Database};
use crate::records::*;
use crate::schema::INDEX_MIGRATIONS;
use ctx_core::Result;
use rusqlite::{params, Connection, OptionalExtension, Row};
use std::path::Path;

pub struct IndexStore {
    db: Database,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub struct IndexStats {
    pub generation: u64,
    pub files: u64,
    pub symbols: u64,
    pub chunks: u64,
    pub relationships: u64,
    pub embeddings: u64,
    pub last_full_index_at: Option<String>,
    pub embedding_model: Option<String>,
    pub repository_root: Option<String>,
    pub languages: Vec<(String, u64)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LexicalHit {
    pub chunk: ChunkRecord,
    /// Positive BM25 relevance (higher is better).
    pub score: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolMatchMode {
    Exact,
    CaseInsensitive,
    Prefix,
    Contains,
}

fn file_from_row(r: &Row<'_>) -> rusqlite::Result<FileRecord> {
    Ok(FileRecord {
        path: r.get("path")?,
        language: r.get("language")?,
        kind: r.get("kind")?,
        size: r.get::<_, i64>("size")? as u64,
        hash: r.get("hash")?,
        mtime: r.get("mtime")?,
        generation: r.get::<_, i64>("generation")? as u64,
        indexed_at: parse_ts(&r.get::<_, String>("indexed_at")?),
    })
}

fn symbol_from_row(r: &Row<'_>) -> rusqlite::Result<SymbolRecord> {
    Ok(SymbolRecord {
        id: r.get("id")?,
        path: r.get("path")?,
        name: r.get("name")?,
        kind: r.get("kind")?,
        parent: r.get("parent")?,
        start_byte: r.get::<_, i64>("start_byte")? as usize,
        end_byte: r.get::<_, i64>("end_byte")? as usize,
        start_line: r.get::<_, i64>("start_line")? as usize,
        end_line: r.get::<_, i64>("end_line")? as usize,
        signature: r.get("signature")?,
        generation: r.get::<_, i64>("generation")? as u64,
    })
}

fn chunk_from_row(r: &Row<'_>) -> rusqlite::Result<ChunkRecord> {
    Ok(ChunkRecord {
        id: r.get("id")?,
        path: r.get("path")?,
        language: r.get("language")?,
        symbol: r.get("symbol")?,
        symbol_kind: r.get("symbol_kind")?,
        start_byte: r.get::<_, i64>("start_byte")? as usize,
        end_byte: r.get::<_, i64>("end_byte")? as usize,
        start_line: r.get::<_, i64>("start_line")? as usize,
        end_line: r.get::<_, i64>("end_line")? as usize,
        content_hash: r.get("content_hash")?,
        file_hash: r.get("file_hash")?,
        content: r.get("content")?,
        tokens: String::new(),
        generation: r.get::<_, i64>("generation")? as u64,
    })
}

fn relationship_from_row(r: &Row<'_>) -> rusqlite::Result<RelationshipRecord> {
    Ok(RelationshipRecord {
        id: r.get("id")?,
        from_path: r.get("from_path")?,
        from_symbol: r.get("from_symbol")?,
        kind: r.get("kind")?,
        target: r.get("target")?,
        to_path: r.get("to_path")?,
        to_symbol: r.get("to_symbol")?,
        confidence: r.get("confidence")?,
        source: r.get("source")?,
        generation: r.get::<_, i64>("generation")? as u64,
    })
}

const CHUNK_COLS: &str = "id, path, language, symbol, symbol_kind, start_byte, end_byte, start_line, end_line, content_hash, file_hash, content, generation";

fn vector_to_blob(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

fn blob_to_vector(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

/// Escape a term for use inside an FTS5 MATCH expression.
pub fn fts_quote(term: &str) -> String {
    format!("\"{}\"", term.replace('"', "\"\""))
}

impl IndexStore {
    pub fn open(path: &Path) -> Result<Self> {
        Ok(Self { db: Database::open(path, INDEX_MIGRATIONS)? })
    }

    pub fn in_memory() -> Result<Self> {
        Ok(Self { db: Database::in_memory(INDEX_MIGRATIONS)? })
    }

    pub fn database(&self) -> &Database {
        &self.db
    }

    /// Write a consistent snapshot of this store to `path` (`VACUUM INTO`),
    /// which must not exist yet. A linked worktree's project starts from a
    /// copy of its parent's index, so its first pass only hashes files
    /// instead of parsing and embedding every one of them again.
    pub fn copy_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let target = path.to_string_lossy().to_string();
        self.db.with(|c| {
            c.execute("VACUUM INTO ?1", params![target])?;
            Ok(())
        })
    }

    // ----- meta -----------------------------------------------------------

    pub fn get_meta(&self, key: &str) -> Result<Option<String>> {
        self.db.with(|c| {
            c.query_row("SELECT value FROM index_meta WHERE key = ?1", params![key], |r| r.get(0)).optional()
        })
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.db.with(|c| {
            c.execute(
                "INSERT INTO index_meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?;
            Ok(())
        })
    }

    pub fn generation(&self) -> Result<u64> {
        Ok(self.get_meta("generation")?.and_then(|v| v.parse().ok()).unwrap_or(0))
    }

    /// Publish a new index generation (spec §6.7 step 8).
    pub fn bump_generation(&self) -> Result<u64> {
        let next = self.generation()? + 1;
        self.set_meta("generation", &next.to_string())?;
        Ok(next)
    }

    pub fn upsert_repository(&self, id: &str, root: &str) -> Result<()> {
        self.db.with(|c| {
            let now = now_text();
            c.execute(
                "INSERT INTO repositories (id, root, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)
                 ON CONFLICT(id) DO UPDATE SET root = excluded.root, updated_at = excluded.updated_at",
                params![id, root, now],
            )?;
            Ok(())
        })
    }

    pub fn upsert_worktree(&self, repository_id: &str, path: &str, branch: Option<&str>, head: Option<&str>) -> Result<()> {
        self.db.with(|c| {
            c.execute(
                "INSERT INTO worktrees (repository_id, path, branch, head, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(repository_id, path) DO UPDATE SET branch = excluded.branch, head = excluded.head, updated_at = excluded.updated_at",
                params![repository_id, path, branch, head, now_text()],
            )?;
            Ok(())
        })
    }

    pub fn worktree(&self, repository_id: &str, path: &str) -> Result<Option<(Option<String>, Option<String>)>> {
        self.db.with(|c| {
            c.query_row(
                "SELECT branch, head FROM worktrees WHERE repository_id = ?1 AND path = ?2",
                params![repository_id, path],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
        })
    }

    // ----- files ----------------------------------------------------------

    pub fn upsert_file(&self, file: &FileRecord) -> Result<()> {
        self.db.with(|c| {
            c.execute(
                "INSERT INTO files (path, language, kind, size, hash, mtime, generation, indexed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(path) DO UPDATE SET language = excluded.language, kind = excluded.kind, size = excluded.size,
                   hash = excluded.hash, mtime = excluded.mtime, generation = excluded.generation, indexed_at = excluded.indexed_at",
                params![
                    file.path,
                    file.language,
                    file.kind,
                    file.size as i64,
                    file.hash,
                    file.mtime,
                    file.generation as i64,
                    file.indexed_at.to_rfc3339()
                ],
            )?;
            Ok(())
        })
    }

    pub fn get_file(&self, path: &str) -> Result<Option<FileRecord>> {
        self.db.with(|c| {
            c.query_row("SELECT * FROM files WHERE path = ?1", params![path], file_from_row).optional()
        })
    }

    pub fn list_files(&self) -> Result<Vec<FileRecord>> {
        self.db.with(|c| {
            let mut stmt = c.prepare("SELECT * FROM files ORDER BY path")?;
            let rows = stmt.query_map([], file_from_row)?;
            rows.collect()
        })
    }

    /// Case-insensitive substring search over paths; matches on path
    /// segments score higher in the retrieval layer.
    pub fn search_paths(&self, needle: &str, limit: usize) -> Result<Vec<FileRecord>> {
        let pattern = format!("%{}%", needle.replace('%', "\\%").replace('_', "\\_"));
        self.db.with(|c| {
            let mut stmt =
                c.prepare("SELECT * FROM files WHERE path LIKE ?1 ESCAPE '\\' COLLATE NOCASE ORDER BY length(path) LIMIT ?2")?;
            let rows = stmt.query_map(params![pattern, limit as i64], file_from_row)?;
            rows.collect()
        })
    }

    /// Remove a file and all derived records.
    pub fn delete_file(&self, path: &str) -> Result<()> {
        self.db.with_tx(|tx| {
            tx.execute("DELETE FROM embeddings WHERE chunk_id IN (SELECT id FROM chunks WHERE path = ?1)", params![path])?;
            delete_fts_rows(tx, path)?;
            tx.execute("DELETE FROM chunks WHERE path = ?1", params![path])?;
            tx.execute("DELETE FROM symbols WHERE path = ?1", params![path])?;
            tx.execute("DELETE FROM relationships WHERE from_path = ?1", params![path])?;
            tx.execute("UPDATE relationships SET to_path = NULL WHERE to_path = ?1", params![path])?;
            tx.execute("DELETE FROM files WHERE path = ?1", params![path])?;
            Ok(())
        })
    }

    /// Append relationships (used for heuristic companions added after the
    /// per-file records are written).
    pub fn insert_relationships(&self, relationships: &[RelationshipRecord]) -> Result<()> {
        self.db.with_tx(|tx| {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO relationships (from_path, from_symbol, kind, target, to_path, to_symbol, confidence, source, generation)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )?;
            for r in relationships {
                stmt.execute(params![
                    r.from_path,
                    r.from_symbol,
                    r.kind,
                    r.target,
                    r.to_path,
                    r.to_symbol,
                    r.confidence,
                    r.source,
                    r.generation as i64
                ])?;
            }
            Ok(())
        })
    }

    /// Remove heuristic relationships originating from a path.
    pub fn delete_relationships_by_source(&self, from_path: &str, source: &str) -> Result<()> {
        self.db.with(|c| {
            c.execute("DELETE FROM relationships WHERE from_path = ?1 AND source = ?2", params![from_path, source])?;
            Ok(())
        })
    }

    /// Atomically replace all derived records for one file.
    pub fn replace_file_records(
        &self,
        file: &FileRecord,
        symbols: &[SymbolRecord],
        chunks: &[ChunkRecord],
        relationships: &[RelationshipRecord],
    ) -> Result<()> {
        self.db.with_tx(|tx| {
            let path = &file.path;
            tx.execute(
                "INSERT INTO files (path, language, kind, size, hash, mtime, generation, indexed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(path) DO UPDATE SET language = excluded.language, kind = excluded.kind, size = excluded.size,
                   hash = excluded.hash, mtime = excluded.mtime, generation = excluded.generation, indexed_at = excluded.indexed_at",
                params![
                    file.path,
                    file.language,
                    file.kind,
                    file.size as i64,
                    file.hash,
                    file.mtime,
                    file.generation as i64,
                    file.indexed_at.to_rfc3339()
                ],
            )?;
            tx.execute("DELETE FROM symbols WHERE path = ?1", params![path])?;
            {
                let mut stmt = tx.prepare_cached(
                    "INSERT INTO symbols (path, name, kind, parent, start_byte, end_byte, start_line, end_line, signature, generation)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                )?;
                for s in symbols {
                    stmt.execute(params![
                        path,
                        s.name,
                        s.kind,
                        s.parent,
                        s.start_byte as i64,
                        s.end_byte as i64,
                        s.start_line as i64,
                        s.end_line as i64,
                        s.signature,
                        s.generation as i64
                    ])?;
                }
            }
            // Chunks: keep embeddings whose content hash is unchanged.
            let old_ids: Vec<String> = {
                let mut stmt = tx.prepare_cached("SELECT id FROM chunks WHERE path = ?1")?;
                let rows = stmt.query_map(params![path], |r| r.get(0))?;
                rows.collect::<rusqlite::Result<_>>()?
            };
            delete_fts_rows(tx, path)?;
            tx.execute("DELETE FROM chunks WHERE path = ?1", params![path])?;
            {
                let mut ins = tx.prepare_cached(&format!(
                    "INSERT INTO chunks ({CHUNK_COLS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)"
                ))?;
                let mut fts = tx.prepare_cached(
                    "INSERT INTO chunks_fts (content, tokens, path, symbol, chunk_id) VALUES (?1, ?2, ?3, ?4, ?5)",
                )?;
                for ch in chunks {
                    ins.execute(params![
                        ch.id,
                        path,
                        ch.language,
                        ch.symbol,
                        ch.symbol_kind,
                        ch.start_byte as i64,
                        ch.end_byte as i64,
                        ch.start_line as i64,
                        ch.end_line as i64,
                        ch.content_hash,
                        ch.file_hash,
                        ch.content,
                        ch.generation as i64
                    ])?;
                    fts.execute(params![ch.content, ch.tokens, path, ch.symbol, ch.id])?;
                }
            }
            // Only this file's previous chunks can have lost their row; a
            // `NOT IN (SELECT id FROM chunks)` over every embedding would
            // make each file cost as much as the whole index.
            {
                let new_ids: std::collections::HashSet<&str> = chunks.iter().map(|c| c.id.as_str()).collect();
                let mut stmt = tx.prepare_cached("DELETE FROM embeddings WHERE chunk_id = ?1")?;
                for id in old_ids.iter().filter(|id| !new_ids.contains(id.as_str())) {
                    stmt.execute(params![id])?;
                }
            }
            tx.execute("DELETE FROM relationships WHERE from_path = ?1", params![path])?;
            {
                let mut stmt = tx.prepare_cached(
                    "INSERT INTO relationships (from_path, from_symbol, kind, target, to_path, to_symbol, confidence, source, generation)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                )?;
                for r in relationships {
                    stmt.execute(params![
                        path,
                        r.from_symbol,
                        r.kind,
                        r.target,
                        r.to_path,
                        r.to_symbol,
                        r.confidence,
                        r.source,
                        r.generation as i64
                    ])?;
                }
            }
            Ok(())
        })
    }

    // ----- symbols --------------------------------------------------------

    pub fn find_symbols(&self, name: &str, mode: SymbolMatchMode, limit: usize) -> Result<Vec<SymbolRecord>> {
        self.db.with(|c| {
            let (sql, arg) = match mode {
                SymbolMatchMode::Exact => ("SELECT * FROM symbols WHERE name = ?1 ORDER BY path LIMIT ?2", name.to_string()),
                SymbolMatchMode::CaseInsensitive => (
                    "SELECT * FROM symbols WHERE name = ?1 COLLATE NOCASE ORDER BY path LIMIT ?2",
                    name.to_string(),
                ),
                SymbolMatchMode::Prefix => (
                    "SELECT * FROM symbols WHERE name LIKE ?1 ESCAPE '\\' ORDER BY length(name), path LIMIT ?2",
                    format!("{}%", name.replace('%', "\\%").replace('_', "\\_")),
                ),
                SymbolMatchMode::Contains => (
                    "SELECT * FROM symbols WHERE name LIKE ?1 ESCAPE '\\' COLLATE NOCASE ORDER BY length(name), path LIMIT ?2",
                    format!("%{}%", name.replace('%', "\\%").replace('_', "\\_")),
                ),
            };
            let mut stmt = c.prepare(sql)?;
            let rows = stmt.query_map(params![arg, limit as i64], symbol_from_row)?;
            rows.collect()
        })
    }

    pub fn symbols_in_file(&self, path: &str) -> Result<Vec<SymbolRecord>> {
        self.db.with(|c| {
            let mut stmt = c.prepare("SELECT * FROM symbols WHERE path = ?1 ORDER BY start_byte")?;
            let rows = stmt.query_map(params![path], symbol_from_row)?;
            rows.collect()
        })
    }

    /// Innermost symbol containing `byte` in `path`.
    pub fn symbol_at(&self, path: &str, byte: usize) -> Result<Option<SymbolRecord>> {
        self.db.with(|c| {
            c.query_row(
                "SELECT * FROM symbols WHERE path = ?1 AND start_byte <= ?2 AND end_byte >= ?2 ORDER BY (end_byte - start_byte) ASC LIMIT 1",
                params![path, byte as i64],
                symbol_from_row,
            )
            .optional()
        })
    }

    // ----- chunks ---------------------------------------------------------

    pub fn get_chunk(&self, id: &str) -> Result<Option<ChunkRecord>> {
        self.db.with(|c| {
            c.query_row(&format!("SELECT {CHUNK_COLS} FROM chunks WHERE id = ?1"), params![id], chunk_from_row)
                .optional()
        })
    }

    pub fn chunks_in_file(&self, path: &str) -> Result<Vec<ChunkRecord>> {
        self.db.with(|c| {
            let mut stmt = c.prepare(&format!("SELECT {CHUNK_COLS} FROM chunks WHERE path = ?1 ORDER BY start_byte"))?;
            let rows = stmt.query_map(params![path], chunk_from_row)?;
            rows.collect()
        })
    }

    /// Chunk containing a symbol definition in a file, if any.
    pub fn chunk_for_symbol(&self, path: &str, symbol: &str) -> Result<Option<ChunkRecord>> {
        self.db.with(|c| {
            c.query_row(
                &format!("SELECT {CHUNK_COLS} FROM chunks WHERE path = ?1 AND symbol = ?2 ORDER BY start_byte LIMIT 1"),
                params![path, symbol],
                chunk_from_row,
            )
            .optional()
        })
    }

    pub fn all_chunk_ids_and_hashes(&self) -> Result<Vec<(String, String)>> {
        self.db.with(|c| {
            let mut stmt = c.prepare("SELECT id, content_hash FROM chunks")?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect()
        })
    }

    /// BM25 lexical search. `match_expr` is a complete FTS5 MATCH expression.
    pub fn search_lexical(&self, match_expr: &str, limit: usize) -> Result<Vec<LexicalHit>> {
        self.db.with(|c| {
            let sql = format!(
                "SELECT c.{}, bm25(chunks_fts, 1.0, 1.5, 3.0, 4.0) AS score
                 FROM chunks_fts JOIN chunks c ON c.id = chunks_fts.chunk_id
                 WHERE chunks_fts MATCH ?1 ORDER BY score LIMIT ?2",
                CHUNK_COLS.replace(", ", ", c.")
            );
            let mut stmt = c.prepare(&sql)?;
            let collected: rusqlite::Result<Vec<LexicalHit>> = stmt
                .query_map(params![match_expr, limit as i64], |r| {
                    let chunk = chunk_from_row(r)?;
                    let score: f64 = r.get("score")?;
                    Ok(LexicalHit { chunk, score: -score })
                })
                .and_then(|rows| rows.collect());
            match collected {
                Ok(hits) => Ok(hits),
                // A malformed MATCH expression is a retrieval miss, not a
                // storage failure.
                Err(rusqlite::Error::SqliteFailure(_, Some(msg))) if msg.contains("fts5") => Ok(Vec::new()),
                Err(e) => Err(e),
            }
        })
    }

    // ----- relationships --------------------------------------------------

    pub fn relationships_from(&self, path: &str) -> Result<Vec<RelationshipRecord>> {
        self.db.with(|c| {
            let mut stmt = c.prepare("SELECT * FROM relationships WHERE from_path = ?1 ORDER BY id")?;
            let rows = stmt.query_map(params![path], relationship_from_row)?;
            rows.collect()
        })
    }

    pub fn relationships_to(&self, path: &str) -> Result<Vec<RelationshipRecord>> {
        self.db.with(|c| {
            let mut stmt = c.prepare("SELECT * FROM relationships WHERE to_path = ?1 ORDER BY id")?;
            let rows = stmt.query_map(params![path], relationship_from_row)?;
            rows.collect()
        })
    }

    pub fn relationships_to_symbol(&self, symbol: &str, limit: usize) -> Result<Vec<RelationshipRecord>> {
        self.db.with(|c| {
            let mut stmt = c.prepare("SELECT * FROM relationships WHERE to_symbol = ?1 OR target = ?1 ORDER BY confidence DESC LIMIT ?2")?;
            let rows = stmt.query_map(params![symbol, limit as i64], relationship_from_row)?;
            rows.collect()
        })
    }

    /// Resolve pending relationship targets to paths after all files are
    /// indexed. `resolver` maps (from_path, target) to a repository path.
    pub fn resolve_relationships(&self, resolver: &dyn Fn(&str, &str) -> Option<String>) -> Result<u64> {
        self.db.with_tx(|tx| {
            let pending: Vec<(i64, String, String)> = {
                let mut stmt = tx.prepare("SELECT id, from_path, target FROM relationships WHERE to_path IS NULL")?;
                let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
                rows.collect::<rusqlite::Result<_>>()?
            };
            let mut resolved = 0u64;
            let mut update = tx.prepare("UPDATE relationships SET to_path = ?1 WHERE id = ?2")?;
            for (id, from, target) in pending {
                if let Some(to) = resolver(&from, &target) {
                    update.execute(params![to, id])?;
                    resolved += 1;
                }
            }
            Ok(resolved)
        })
    }

    // ----- embeddings -----------------------------------------------------

    pub fn embedding_hash(&self, chunk_id: &str, model_id: &str) -> Result<Option<String>> {
        self.db.with(|c| {
            c.query_row(
                "SELECT content_hash FROM embeddings WHERE chunk_id = ?1 AND model_id = ?2",
                params![chunk_id, model_id],
                |r| r.get(0),
            )
            .optional()
        })
    }

    pub fn upsert_embeddings(&self, records: &[EmbeddingRecord]) -> Result<()> {
        self.db.with_tx(|tx| {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO embeddings (chunk_id, content_hash, model_id, dimension, vector, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(chunk_id) DO UPDATE SET content_hash = excluded.content_hash, model_id = excluded.model_id,
                   dimension = excluded.dimension, vector = excluded.vector, updated_at = excluded.updated_at",
            )?;
            for r in records {
                stmt.execute(params![
                    r.chunk_id,
                    r.content_hash,
                    r.model_id,
                    r.vector.len() as i64,
                    vector_to_blob(&r.vector),
                    now_text()
                ])?;
            }
            Ok(())
        })
    }

    pub fn load_embeddings(&self, model_id: &str) -> Result<Vec<EmbeddingRecord>> {
        self.db.with(|c| {
            let mut stmt = c.prepare("SELECT chunk_id, content_hash, model_id, vector FROM embeddings WHERE model_id = ?1")?;
            let rows = stmt.query_map(params![model_id], |r| {
                let blob: Vec<u8> = r.get(3)?;
                Ok(EmbeddingRecord {
                    chunk_id: r.get(0)?,
                    content_hash: r.get(1)?,
                    model_id: r.get(2)?,
                    vector: blob_to_vector(&blob),
                })
            })?;
            rows.collect()
        })
    }

    pub fn delete_embeddings_except_model(&self, model_id: &str) -> Result<u64> {
        self.db.with(|c| {
            let n = c.execute("DELETE FROM embeddings WHERE model_id != ?1", params![model_id])?;
            Ok(n as u64)
        })
    }

    pub fn delete_embeddings(&self, chunk_ids: &[String]) -> Result<u64> {
        self.db.with_tx(|tx| {
            let mut stmt = tx.prepare_cached("DELETE FROM embeddings WHERE chunk_id = ?1")?;
            let mut n = 0u64;
            for id in chunk_ids {
                n += stmt.execute(params![id])? as u64;
            }
            Ok(n)
        })
    }

    pub fn prune_orphan_embeddings(&self) -> Result<u64> {
        self.db.with(|c| {
            let n = c.execute("DELETE FROM embeddings WHERE chunk_id NOT IN (SELECT id FROM chunks)", [])?;
            Ok(n as u64)
        })
    }

    // ----- maintenance ----------------------------------------------------

    pub fn stats(&self) -> Result<IndexStats> {
        let generation = self.generation()?;
        let last_full_index_at = self.get_meta("last_full_index_at")?;
        let embedding_model = self.get_meta("embedding_model")?;
        let repository_root = self.get_meta("repository_root")?;
        self.db.with(|c| {
            let count = |sql: &str| -> rusqlite::Result<u64> { c.query_row(sql, [], |r| r.get::<_, i64>(0)).map(|n| n as u64) };
            let mut stmt = c.prepare(
                "SELECT COALESCE(language, 'unknown'), COUNT(*) FROM files GROUP BY language ORDER BY COUNT(*) DESC",
            )?;
            let languages = stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(IndexStats {
                generation,
                files: count("SELECT COUNT(*) FROM files")?,
                symbols: count("SELECT COUNT(*) FROM symbols")?,
                chunks: count("SELECT COUNT(*) FROM chunks")?,
                relationships: count("SELECT COUNT(*) FROM relationships")?,
                embeddings: count("SELECT COUNT(*) FROM embeddings")?,
                last_full_index_at,
                embedding_model,
                repository_root,
                languages,
            })
        })
    }

    /// Drop every index record (rebuild).
    pub fn clear(&self) -> Result<()> {
        self.db.with_tx(|tx| {
            tx.execute_batch(
                "DELETE FROM embeddings; DELETE FROM chunks_fts; DELETE FROM chunks; DELETE FROM symbols;
                 DELETE FROM relationships; DELETE FROM files;",
            )?;
            Ok(())
        })?;
        self.set_meta("generation", "0")
    }

    pub fn compact(&self) -> Result<()> {
        self.db.compact()
    }

    /// Run a read-only query closure; used by diagnostics.
    pub fn with_connection<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> Result<T> {
        self.db.with(f)
    }
}

/// Delete one file's full-text rows. `path` is an FTS column, so
/// `path = ?` alone scans every row (reading its content): with one call
/// per indexed file that made a first index quadratic. A phrase match on
/// the path's tokens finds the few candidates through the full-text index;
/// the equality then keeps only this exact path (`a/b.rs`, not `x/a/b.rs`).
fn delete_fts_rows(tx: &rusqlite::Transaction<'_>, path: &str) -> rusqlite::Result<()> {
    if !path.chars().any(char::is_alphanumeric) {
        // No tokens to match on.
        tx.execute("DELETE FROM chunks_fts WHERE path = ?1", params![path])?;
        return Ok(());
    }
    let phrase = format!("path : \"{}\"", path.replace('"', "\"\""));
    tx.execute("DELETE FROM chunks_fts WHERE rowid IN (SELECT rowid FROM chunks_fts WHERE chunks_fts MATCH ?1) AND path = ?2", params![phrase, path])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ctx_core::now;

    fn file(path: &str) -> FileRecord {
        FileRecord {
            path: path.into(),
            language: Some("rust".into()),
            kind: "source".into(),
            size: 10,
            hash: "sha256:abc".into(),
            mtime: None,
            generation: 1,
            indexed_at: now(),
        }
    }

    fn chunk(id: &str, path: &str, symbol: &str, content: &str, tokens: &str) -> ChunkRecord {
        ChunkRecord {
            id: id.into(),
            path: path.into(),
            language: Some("rust".into()),
            symbol: Some(symbol.into()),
            symbol_kind: Some("function".into()),
            start_byte: 0,
            end_byte: content.len(),
            start_line: 1,
            end_line: 1,
            content_hash: format!("sha256:{id}"),
            file_hash: "sha256:abc".into(),
            content: content.into(),
            tokens: tokens.into(),
            generation: 1,
        }
    }

    #[test]
    fn fts5_lexical_search_works() {
        let store = IndexStore::in_memory().unwrap();
        store
            .replace_file_records(
                &file("src/login.rs"),
                &[],
                &[
                    chunk("c1", "src/login.rs", "handle_login", "fn handle_login() { spinner(); }", "handle login"),
                    chunk("c2", "src/login.rs", "render_footer", "fn render_footer() {}", "render footer"),
                ],
                &[],
            )
            .unwrap();
        // Both chunks live in src/login.rs (path is indexed), but the chunk
        // whose symbol, tokens and content also match must rank first.
        let hits = store.search_lexical(&fts_quote("login"), 10).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].chunk.symbol.as_deref(), Some("handle_login"));
        assert!(hits[0].score > hits[1].score);
        let hits = store.search_lexical(&fts_quote("spinner"), 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(store.search_lexical("(\"a\" AND ()", 10).unwrap().is_empty(), "syntax errors are misses");
    }

    #[test]
    fn replacing_file_records_cleans_derived_rows() {
        let store = IndexStore::in_memory().unwrap();
        let f = file("a.rs");
        let sym = SymbolRecord {
            id: None,
            path: "a.rs".into(),
            name: "alpha".into(),
            kind: "function".into(),
            parent: None,
            start_byte: 0,
            end_byte: 5,
            start_line: 1,
            end_line: 1,
            signature: None,
            generation: 1,
        };
        store.replace_file_records(&f, &[sym.clone()], &[chunk("c1", "a.rs", "alpha", "fn alpha", "alpha")], &[]).unwrap();
        store
            .upsert_embeddings(&[EmbeddingRecord {
                chunk_id: "c1".into(),
                content_hash: "sha256:c1".into(),
                model_id: "m".into(),
                vector: vec![0.1, 0.2],
            }])
            .unwrap();
        assert_eq!(store.find_symbols("alpha", SymbolMatchMode::Exact, 5).unwrap().len(), 1);
        assert_eq!(store.load_embeddings("m").unwrap().len(), 1);
        // Re-index with different chunk id: old embedding is pruned.
        store.replace_file_records(&f, &[], &[chunk("c9", "a.rs", "beta", "fn beta", "beta")], &[]).unwrap();
        assert!(store.find_symbols("alpha", SymbolMatchMode::Exact, 5).unwrap().is_empty());
        assert!(store.load_embeddings("m").unwrap().is_empty());
        store.delete_file("a.rs").unwrap();
        assert!(store.get_file("a.rs").unwrap().is_none());
        assert!(store.search_lexical(&fts_quote("beta"), 5).unwrap().is_empty());
    }

    #[test]
    fn replacing_one_file_keeps_full_text_rows_of_paths_sharing_its_tokens() {
        let store = IndexStore::in_memory().unwrap();
        for path in ["a/b.rs", "x/a/b.rs", "a/b.rs.bak"] {
            store.replace_file_records(&file(path), &[], &[chunk(&format!("old {path}"), path, "f", "fn gamma", "gamma")], &[]).unwrap();
        }
        store.replace_file_records(&file("a/b.rs"), &[], &[chunk("new", "a/b.rs", "f", "fn delta", "delta")], &[]).unwrap();
        let gamma: Vec<String> = store.search_lexical(&fts_quote("gamma"), 10).unwrap().into_iter().map(|h| h.chunk.path).collect();
        assert_eq!(gamma.len(), 2, "only a/b.rs lost its old row: {gamma:?}");
        assert!(!gamma.contains(&"a/b.rs".to_string()));
        assert_eq!(store.search_lexical(&fts_quote("delta"), 10).unwrap().len(), 1);
        store.delete_file("x/a/b.rs").unwrap();
        assert_eq!(store.search_lexical(&fts_quote("gamma"), 10).unwrap().len(), 1);
        // A path without tokens falls back to the scan.
        store.replace_file_records(&file("+"), &[], &[chunk("p1", "+", "f", "fn eps", "eps")], &[]).unwrap();
        store.delete_file("+").unwrap();
        assert!(store.search_lexical(&fts_quote("eps"), 10).unwrap().is_empty());
    }

    #[test]
    fn generation_increments() {
        let store = IndexStore::in_memory().unwrap();
        assert_eq!(store.generation().unwrap(), 0);
        assert_eq!(store.bump_generation().unwrap(), 1);
        assert_eq!(store.stats().unwrap().generation, 1);
    }

    #[test]
    fn vector_blob_roundtrip() {
        let v = vec![1.5f32, -2.0, 0.25];
        assert_eq!(blob_to_vector(&vector_to_blob(&v)), v);
    }
}
