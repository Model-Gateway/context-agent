//! Full and incremental indexing (spec §6.7).

use crate::discovery::{RepositoryDiscovery, RepositoryProfile};
use crate::freshness::{verify_source, Freshness};
use crate::resolve::{companions_in, Resolver, StemIndex};
use crate::walker::{looks_binary, WalkedFile, Walker};
use ctx_core::{content_hash, CtxError, IndexConfig, Result, SourceRef};
use ctx_embeddings::EmbeddingService;
use ctx_languages::{identifier_tokens, ChunkBudget, LanguageRegistry, RelationshipKind};
use ctx_storage::{ChunkRecord, FileRecord, IndexStore, RelationshipRecord, SymbolRecord};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct IndexerOptions {
    pub chunk_budget: ChunkBudget,
    pub embed: bool,
}

impl Default for IndexerOptions {
    fn default() -> Self {
        Self { chunk_budget: ChunkBudget::default(), embed: true }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexError {
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct IndexReport {
    pub generation: u64,
    pub files_scanned: usize,
    pub files_indexed: usize,
    pub files_unchanged: usize,
    pub files_removed: usize,
    pub symbols: usize,
    pub chunks: usize,
    pub relationships: usize,
    pub resolved_relationships: u64,
    pub embedded: usize,
    pub reused_embeddings: usize,
    pub duration_ms: u64,
    pub errors: Vec<IndexError>,
}

/// Progress callback payload for long-running full indexes.
#[derive(Debug, Clone, Copy)]
pub struct IndexProgress {
    pub done: usize,
    pub total: usize,
}

pub struct Indexer {
    root: PathBuf,
    config: IndexConfig,
    options: IndexerOptions,
    store: Arc<IndexStore>,
    registry: Arc<LanguageRegistry>,
    embeddings: Option<Arc<EmbeddingService>>,
    walker: Walker,
    /// Serializes index mutations from watcher threads and CLI commands.
    lock: Mutex<()>,
    /// `Some` while a full index runs: paths [`Self::update_paths`] was
    /// asked for meanwhile, applied before that index publishes, so an
    /// edit or watcher event never waits for a long first index.
    deferred: Mutex<Option<Vec<PathBuf>>>,
}

/// Pending embedding work: (chunk id, content hash, text).
type PendingEmbedding = (String, String, String);

impl Indexer {
    pub fn new(
        root: &Path,
        config: &IndexConfig,
        options: IndexerOptions,
        store: Arc<IndexStore>,
        registry: Arc<LanguageRegistry>,
        embeddings: Option<Arc<EmbeddingService>>,
    ) -> Result<Self> {
        let root = ctx_core::platform::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        let walker = Walker::new(&root, config)?;
        store.set_meta("repository_root", &root.to_string_lossy())?;
        Ok(Self { root, config: config.clone(), options, store, registry, embeddings, walker, lock: Mutex::new(()), deferred: Mutex::new(None) })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn store(&self) -> &Arc<IndexStore> {
        &self.store
    }

    pub fn registry(&self) -> &Arc<LanguageRegistry> {
        &self.registry
    }

    pub fn embeddings(&self) -> Option<&Arc<EmbeddingService>> {
        self.embeddings.as_ref()
    }

    pub fn walker(&self) -> &Walker {
        &self.walker
    }

    pub fn config(&self) -> &IndexConfig {
        &self.config
    }

    /// The repository profile (no index writes): from the published
    /// index's files when there is one, otherwise from a walk.
    pub fn profile(&self) -> Result<RepositoryProfile> {
        if let Some(profile) = self.indexed_profile()? {
            return Ok(profile);
        }
        let files = self.walker.walk(&self.registry);
        RepositoryDiscovery::profile(&self.root, &files, &self.registry)
    }

    /// The profile from the published index's files; `None` before the
    /// first index completes (or while a rebuild runs), which is when only
    /// a walk could tell.
    pub fn indexed_profile(&self) -> Result<Option<RepositoryProfile>> {
        if self.store.generation()? == 0 {
            return Ok(None);
        }
        let files = self.store.list_files()?;
        RepositoryDiscovery::profile_of(&self.root, files.iter().map(|f| (f.path.as_str(), f.size)), &self.registry).map(Some)
    }

    /// Repository-relative path for an absolute path, tolerant of symlinked
    /// prefixes (e.g. `/var` vs `/private/var` on macOS).
    pub fn relative(&self, abs: &Path) -> Option<String> {
        let canonical = canonicalize_lenient(abs);
        canonical.strip_prefix(&self.root).ok().map(|p| p.to_string_lossy().replace('\\', "/"))
    }

    /// Index every file, remove records for files that disappeared, resolve
    /// relationships, refresh embeddings, and publish a new generation.
    pub fn full_index(&self, progress: Option<&dyn Fn(IndexProgress)>) -> Result<IndexReport> {
        let _guard = self.lock.lock();
        *self.deferred.lock() = Some(Vec::new());
        let result = self.full_index_locked(progress);
        // Stop deferring and apply what was deferred while still holding
        // the lock, so no update lands in between.
        let deferred = self.deferred.lock().take().unwrap_or_default();
        let mut report = result?;
        if !deferred.is_empty() {
            let late = self.update_paths_locked(&deferred)?;
            report.generation = late.generation;
            report.files_indexed += late.files_indexed;
            report.files_removed += late.files_removed;
            report.errors.extend(late.errors);
        }
        Ok(report)
    }

    /// Whether a full index is running (a first index or a rebuild).
    pub fn full_index_running(&self) -> bool {
        self.deferred.lock().is_some()
    }

    fn full_index_locked(&self, progress: Option<&dyn Fn(IndexProgress)>) -> Result<IndexReport> {
        let started = Instant::now();
        let mut report = IndexReport::default();
        let generation = self.store.generation()? + 1;
        let files = self.walker.walk(&self.registry);
        report.files_scanned = files.len();
        let live: HashSet<String> = files.iter().map(|f| f.relative.clone()).collect();

        // Remove records for files no longer present.
        for existing in self.store.list_files()? {
            if !live.contains(&existing.path) {
                self.store.delete_file(&existing.path)?;
                report.files_removed += 1;
            }
        }

        let mut pending: Vec<PendingEmbedding> = Vec::new();
        let mut changed_paths: Vec<String> = Vec::new();
        let total = files.len();
        for (i, file) in files.iter().enumerate() {
            match self.index_file(file, generation, &mut report, &mut pending) {
                Ok(true) => changed_paths.push(file.relative.clone()),
                Ok(false) => {}
                Err(e) => report.errors.push(IndexError { path: file.relative.clone(), message: e.to_string() }),
            }
            if let Some(cb) = progress {
                if i % 50 == 0 || i + 1 == total {
                    cb(IndexProgress { done: i + 1, total });
                }
            }
        }

        let all_paths: HashSet<String> = live.clone();
        self.refresh_companions(&changed_paths, &all_paths, generation, &mut report)?;
        report.resolved_relationships = self.resolve_relationships(&all_paths)?;
        self.refresh_embeddings(&pending, Some(&all_paths), &mut report)?;

        report.generation = self.store.bump_generation()?;
        self.store.set_meta("last_full_index_at", &chrono::Utc::now().to_rfc3339())?;
        if let Some(git) = RepositoryDiscovery::git_info(&self.root) {
            let repo_id = ctx_core::RepositoryId::from_root(&self.root);
            self.store.upsert_repository(repo_id.as_str(), &self.root.to_string_lossy())?;
            self.store.upsert_worktree(repo_id.as_str(), &self.root.to_string_lossy(), git.branch.as_deref(), git.head.as_deref())?;
        }
        report.duration_ms = started.elapsed().as_millis() as u64;
        tracing::info!(generation = report.generation, indexed = report.files_indexed, unchanged = report.files_unchanged, removed = report.files_removed, ms = report.duration_ms, "full index complete");
        Ok(report)
    }

    /// Incrementally update a set of absolute paths (created, modified or
    /// deleted). Unknown or excluded paths are ignored. While a full index
    /// runs the paths are queued for it instead, and the empty report
    /// carries the current generation.
    pub fn update_paths(&self, paths: &[PathBuf]) -> Result<IndexReport> {
        if let Some(queue) = self.deferred.lock().as_mut() {
            queue.extend(paths.iter().cloned());
            return Ok(IndexReport { generation: self.store.generation()?, ..Default::default() });
        }
        let _guard = self.lock.lock();
        self.update_paths_locked(paths)
    }

    fn update_paths_locked(&self, paths: &[PathBuf]) -> Result<IndexReport> {
        let started = Instant::now();
        let mut report = IndexReport::default();
        let generation = self.store.generation()? + 1;
        let mut pending: Vec<PendingEmbedding> = Vec::new();
        let mut changed_paths: Vec<String> = Vec::new();
        let mut touched = false;
        for raw in paths {
            let abs = canonicalize_lenient(raw);
            let abs = &abs;
            let Some(rel) = self.relative(abs) else { continue };
            if rel.is_empty() {
                continue;
            }
            if abs.is_file() {
                if !self.walker.should_index(abs) {
                    // Became oversized, binary or excluded: drop stale records.
                    if self.store.get_file(&rel)?.is_some() {
                        self.store.delete_file(&rel)?;
                        report.files_removed += 1;
                        touched = true;
                    }
                    continue;
                }
                report.files_scanned += 1;
                let Ok(meta) = std::fs::metadata(abs) else { continue };
                let (kind, language) = Walker::classify(&rel, &self.registry);
                let file = WalkedFile {
                    absolute: abs.clone(),
                    relative: rel.clone(),
                    size: meta.len(),
                    mtime: meta.modified().ok().and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64),
                    kind,
                    language,
                };
                match self.index_file(&file, generation, &mut report, &mut pending) {
                    Ok(true) => {
                        touched = true;
                        changed_paths.push(rel.clone());
                    }
                    Ok(false) => {}
                    Err(e) => report.errors.push(IndexError { path: rel.clone(), message: e.to_string() }),
                }
            } else if abs.is_dir() {
                // A directory event: index its current contents.
                let files = self.walker.walk(&self.registry);
                for f in files.iter().filter(|f| f.absolute.starts_with(abs)) {
                    match self.index_file(f, generation, &mut report, &mut pending) {
                        Ok(true) => {
                            touched = true;
                            changed_paths.push(f.relative.clone());
                        }
                        Ok(false) => {}
                        Err(e) => report.errors.push(IndexError { path: f.relative.clone(), message: e.to_string() }),
                    }
                }
            } else if self.store.get_file(&rel)?.is_some() {
                self.store.delete_file(&rel)?;
                report.files_removed += 1;
                touched = true;
            } else {
                // A deleted directory: remove every record beneath it.
                let prefix = format!("{rel}/");
                for existing in self.store.list_files()? {
                    if existing.path.starts_with(&prefix) {
                        self.store.delete_file(&existing.path)?;
                        report.files_removed += 1;
                        touched = true;
                    }
                }
            }
        }
        if touched {
            let all_paths: HashSet<String> = self.store.list_files()?.into_iter().map(|f| f.path).collect();
            self.refresh_companions(&changed_paths, &all_paths, generation, &mut report)?;
            report.resolved_relationships = self.resolve_relationships(&all_paths)?;
            // Re-indexed files replace their chunk ids; prune dead vectors.
            self.refresh_embeddings(&pending, Some(&all_paths), &mut report)?;
            report.generation = self.store.bump_generation()?;
        } else {
            report.generation = self.store.generation()?;
        }
        report.duration_ms = started.elapsed().as_millis() as u64;
        Ok(report)
    }

    /// Re-walk and reconcile with the store; recovers from missed watcher
    /// events and runs whenever a repository is opened. Files whose size and
    /// mtime match their record are trusted without being read; only the
    /// rest go through hash-gated indexing, and when nothing changed no
    /// derived data is rebuilt and the generation stays put.
    pub fn reconcile(&self) -> Result<IndexReport> {
        if self.embeddings_incomplete()? {
            return self.full_index(None);
        }
        let started = Instant::now();
        let files = self.walker.walk(&self.registry);
        let known: HashMap<String, FileRecord> = self.store.list_files()?.into_iter().map(|f| (f.path.clone(), f)).collect();
        let live: HashSet<&str> = files.iter().map(|f| f.relative.as_str()).collect();
        let mut suspects: Vec<PathBuf> = files.iter().filter(|f| !known.get(&f.relative).is_some_and(|r| unchanged_on_disk(r, f))).map(|f| f.absolute.clone()).collect();
        suspects.extend(known.keys().filter(|p| !live.contains(p.as_str())).map(|p| self.root.join(p)));
        let mut report = if suspects.is_empty() { IndexReport { generation: self.store.generation()?, ..Default::default() } } else { self.update_paths(&suspects)? };
        report.files_scanned = files.len();
        report.files_unchanged = files.len().saturating_sub(report.files_indexed);
        report.duration_ms = started.elapsed().as_millis() as u64;
        tracing::info!(generation = report.generation, checked = suspects.len(), indexed = report.files_indexed, removed = report.files_removed, ms = report.duration_ms, "reconcile complete");
        Ok(report)
    }

    /// Chunks without a vector (first run with embeddings enabled, or the
    /// embedding model changed); only a full index backfills them.
    fn embeddings_incomplete(&self) -> Result<bool> {
        match &self.embeddings {
            Some(svc) if self.options.embed => Ok((svc.len() as u64) < self.store.stats()?.chunks),
            _ => Ok(false),
        }
    }

    /// Verify a repository-relative path against the index; re-index it when
    /// the working tree differs. Returns the freshness observed *before*
    /// re-indexing so callers know the retrieved snippet was stale.
    pub fn ensure_fresh(&self, source: &SourceRef) -> Result<Freshness> {
        let freshness = verify_source(&self.root, source);
        match &freshness {
            Freshness::Fresh => {}
            Freshness::Changed { .. } | Freshness::Missing => {
                let abs = self.root.join(&source.path);
                self.update_paths(&[abs])?;
            }
        }
        Ok(freshness)
    }

    /// Hash-gated per-file indexing. Returns `Ok(true)` when records changed.
    fn index_file(&self, file: &WalkedFile, generation: u64, report: &mut IndexReport, pending: &mut Vec<PendingEmbedding>) -> Result<bool> {
        let bytes = std::fs::read(&file.absolute)?;
        // The walk does not sniff files of a known language; a binary one
        // is caught here, from the bytes just read.
        if looks_binary(&bytes) {
            if self.store.get_file(&file.relative)?.is_some() {
                self.store.delete_file(&file.relative)?;
                report.files_removed += 1;
            }
            return Ok(false);
        }
        let hash = content_hash(&bytes);
        if let Some(existing) = self.store.get_file(&file.relative)? {
            if existing.hash == hash {
                report.files_unchanged += 1;
                // Record fresh stat data so the next reconcile can skip
                // reading this file (e.g. after a checkout touched it, or
                // when it was indexed in the same second it was saved).
                if !unchanged_on_disk(&existing, file) {
                    self.store.upsert_file(&FileRecord { size: file.size, mtime: file.mtime, indexed_at: ctx_core::now(), ..existing.clone() })?;
                }
                // Still make sure embeddings exist for this file's chunks
                // (e.g. after the embedding model changed).
                if self.options.embed {
                    if let Some(svc) = &self.embeddings {
                        for ch in self.store.chunks_in_file(&file.relative)? {
                            if svc.is_empty() || !self.has_embedding(&ch.id) {
                                let text = embedding_text(&ch);
                                pending.push((ch.id.clone(), content_hash(text.as_bytes()), text));
                            }
                        }
                    }
                }
                return Ok(false);
            }
        }
        let text = String::from_utf8_lossy(&bytes).to_string();
        let adapter = self.registry.adapter_for(&file.relative);
        let analysis = adapter.analyze(&text, self.options.chunk_budget);
        let language = file.language.clone();

        let symbols: Vec<SymbolRecord> = analysis
            .symbols
            .iter()
            .map(|s| SymbolRecord {
                id: None,
                path: file.relative.clone(),
                name: s.name.clone(),
                kind: s.kind.as_str().to_string(),
                parent: s.parent.clone(),
                start_byte: s.start_byte,
                end_byte: s.end_byte,
                start_line: s.start_line,
                end_line: s.end_line,
                signature: s.signature.clone(),
                generation,
            })
            .collect();

        let path_tokens = identifier_tokens(&file.relative.replace(['/', '.', '-'], " "));
        let chunks: Vec<ChunkRecord> = analysis
            .chunks
            .iter()
            .map(|c| {
                let chunk_hash = content_hash(c.content.as_bytes());
                let id_seed = format!("{}|{}|{}|{}", file.relative, c.start_byte, c.part, chunk_hash);
                let id = format!("chunk_{}", &content_hash(id_seed.as_bytes())[7..27]);
                let mut tokens = identifier_tokens(&c.content);
                if let Some(sym) = &c.symbol {
                    tokens.push(' ');
                    tokens.push_str(&identifier_tokens(sym));
                }
                tokens.push(' ');
                tokens.push_str(&path_tokens);
                ChunkRecord {
                    id,
                    path: file.relative.clone(),
                    language: language.clone(),
                    symbol: c.symbol.clone(),
                    symbol_kind: c.symbol_kind.map(|k| k.as_str().to_string()),
                    start_byte: c.start_byte,
                    end_byte: c.end_byte,
                    start_line: c.start_line,
                    end_line: c.end_line,
                    content_hash: chunk_hash,
                    file_hash: hash.clone(),
                    content: c.content.clone(),
                    tokens,
                    generation,
                }
            })
            .collect();

        let relationships: Vec<RelationshipRecord> = analysis
            .relationships
            .iter()
            .map(|r| RelationshipRecord {
                id: None,
                from_path: file.relative.clone(),
                from_symbol: r.from_symbol.clone(),
                kind: r.kind.as_str().to_string(),
                target: r.target.clone(),
                to_path: None,
                to_symbol: r.to_symbol.clone(),
                confidence: r.confidence,
                source: "parser".into(),
                generation,
            })
            .collect();

        let record = FileRecord {
            path: file.relative.clone(),
            language: language.clone(),
            kind: file.kind.as_str().to_string(),
            size: file.size,
            hash,
            mtime: file.mtime,
            generation,
            indexed_at: ctx_core::now(),
        };
        self.store.replace_file_records(&record, &symbols, &chunks, &relationships)?;
        report.files_indexed += 1;
        report.symbols += symbols.len();
        report.chunks += chunks.len();
        report.relationships += relationships.len();
        if !analysis.parse_errors.is_empty() {
            tracing::debug!(path = %file.relative, errors = analysis.parse_errors.len(), "parse errors (indexed with partial structure)");
        }
        if self.options.embed && self.embeddings.is_some() {
            for ch in &chunks {
                let text = embedding_text(ch);
                pending.push((ch.id.clone(), content_hash(text.as_bytes()), text));
            }
        }
        Ok(true)
    }

    fn has_embedding(&self, chunk_id: &str) -> bool {
        self.embeddings.as_ref().map(|svc| svc.search_cache_contains(chunk_id)).unwrap_or(false)
    }

    /// Heuristic companion relationships (tests, styles) for changed files
    /// and for the existing files that may now pair with them (a test
    /// created before its source, a stylesheet before its component).
    fn refresh_companions(&self, changed: &[String], all_paths: &HashSet<String>, generation: u64, report: &mut IndexReport) -> Result<()> {
        let mut records = Vec::new();
        let stem_of = |p: &str| -> String {
            let name = p.rsplit('/').next().unwrap_or(p);
            let stem = name.split('.').next().unwrap_or(name).to_ascii_lowercase();
            stem.trim_start_matches("test_").trim_end_matches("_test").trim_end_matches("_spec").trim_end_matches("tests").trim_end_matches("test").trim_end_matches("spec").to_string()
        };
        // Every lookup goes through a by-stem map: on a full index every
        // path is "changed", and scanning all paths per changed path made
        // this step quadratic (or worse) in the repository's size.
        let mut by_stem: HashMap<String, Vec<&String>> = HashMap::new();
        for p in all_paths {
            by_stem.entry(stem_of(p)).or_default().push(p);
        }
        let index = StemIndex::new(all_paths);
        let mut seen: HashSet<&str> = changed.iter().map(String::as_str).collect();
        let mut targets: Vec<String> = changed.to_vec();
        for path in changed {
            let stem = stem_of(path);
            if stem.is_empty() {
                continue;
            }
            for other in by_stem.get(&stem).into_iter().flatten() {
                if seen.insert(other.as_str()) {
                    targets.push((*other).clone());
                }
            }
        }
        for path in &targets {
            self.store.delete_relationships_by_source(path, "heuristic")?;
            for (target, kind) in companions_in(path, &index) {
                records.push(RelationshipRecord {
                    id: None,
                    from_path: path.clone(),
                    from_symbol: None,
                    kind: kind.to_string(),
                    target: target.clone(),
                    to_path: Some(target),
                    to_symbol: None,
                    confidence: 0.6,
                    source: "heuristic".into(),
                    generation,
                });
            }
        }
        if !records.is_empty() {
            report.relationships += records.len();
            self.store.insert_relationships(&records)?;
        }
        Ok(())
    }

    fn resolve_relationships(&self, all_paths: &HashSet<String>) -> Result<u64> {
        let languages: HashMap<String, Option<String>> = self.store.list_files()?.into_iter().map(|f| (f.path, f.language)).collect();
        let go_module = all_paths.iter().find(|p| p.as_str() == "go.mod" || p.ends_with("/go.mod")).and_then(|p| std::fs::read_to_string(self.root.join(p)).ok()).and_then(|text| text.lines().find_map(|l| l.strip_prefix("module ").map(|m| m.trim().to_string())));
        let resolver = Resolver::new(all_paths, go_module);
        self.store.resolve_relationships(&|from, target| {
            let lang = languages.get(from).and_then(|l| l.as_deref());
            resolver.resolve(from, target, lang)
        })
    }

    fn refresh_embeddings(&self, pending: &[PendingEmbedding], live_ids_from: Option<&HashSet<String>>, report: &mut IndexReport) -> Result<()> {
        let Some(svc) = &self.embeddings else { return Ok(()) };
        if !self.options.embed {
            return Ok(());
        }
        if !pending.is_empty() {
            let stats = svc.refresh(pending)?;
            report.embedded += stats.embedded;
            report.reused_embeddings += stats.reused;
        }
        if live_ids_from.is_some() {
            let live: HashSet<String> = self.store.all_chunk_ids_and_hashes()?.into_iter().map(|(id, _)| id).collect();
            svc.retain(&live)?;
        }
        Ok(())
    }

    /// Relationship kinds considered when expanding context for a file.
    pub fn expansion_kinds() -> &'static [RelationshipKind] {
        &[RelationshipKind::Import, RelationshipKind::Include, RelationshipKind::TestOf, RelationshipKind::StyleOf, RelationshipKind::Extends, RelationshipKind::Implements]
    }
}

/// Whether a walked file can be trusted to match its record without reading
/// it. mtimes have one-second resolution, so a file modified in the same
/// second it was indexed is re-read (the "racy" case).
fn unchanged_on_disk(record: &FileRecord, file: &WalkedFile) -> bool {
    match (record.mtime, file.mtime) {
        (Some(recorded), Some(current)) => record.size == file.size && recorded == current && recorded < record.indexed_at.timestamp(),
        _ => false,
    }
}

/// Canonicalize a path; for paths that do not exist, canonicalize the
/// nearest existing ancestor and re-append the missing tail.
pub fn canonicalize_lenient(path: &Path) -> PathBuf {
    if let Ok(c) = ctx_core::platform::canonicalize(path) {
        return c;
    }
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    let mut cur = path.to_path_buf();
    while !cur.exists() {
        match cur.file_name() {
            Some(name) => {
                tail.push(name.to_os_string());
                if !cur.pop() {
                    return path.to_path_buf();
                }
            }
            None => return path.to_path_buf(),
        }
    }
    let mut out = ctx_core::platform::canonicalize(&cur).unwrap_or(cur);
    for part in tail.iter().rev() {
        out.push(part);
    }
    out
}

/// Text embedded for a chunk: path and symbol give the model locality cues.
fn embedding_text(chunk: &ChunkRecord) -> String {
    let mut s = String::with_capacity(chunk.content.len() + 64);
    s.push_str(&chunk.path);
    if let Some(sym) = &chunk.symbol {
        s.push(' ');
        s.push_str(sym);
    }
    s.push('\n');
    // Cap very large chunks: the hashing embedder is linear but a learned
    // model would truncate anyway.
    let mut end = chunk.content.len().min(8_000);
    while !chunk.content.is_char_boundary(end) {
        end -= 1;
    }
    s.push_str(&chunk.content[..end]);
    s
}

impl From<CtxError> for IndexError {
    fn from(e: CtxError) -> Self {
        IndexError { path: String::new(), message: e.to_string() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ctx_embeddings::HashingEmbedder;
    use ctx_storage::SymbolMatchMode;

    fn fixture() -> (tempfile::TempDir, Indexer) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src/components")).unwrap();
        std::fs::create_dir_all(root.join("src/hooks")).unwrap();
        std::fs::create_dir_all(root.join("svc")).unwrap();
        std::fs::create_dir_all(root.join("tests")).unwrap();
        std::fs::write(root.join("src/components/LoginButton.tsx"), "import { useAuth } from '../hooks/useAuth';\nimport './LoginButton.css';\nexport function LoginButton() { const { login } = useAuth(); return login; }\n").unwrap();
        std::fs::write(root.join("src/hooks/useAuth.ts"), "export function useAuth() { return { login: () => 1, isLoading: false }; }\n").unwrap();
        std::fs::write(root.join("src/components/LoginButton.css"), ".login { color: blue; }\n").unwrap();
        std::fs::write(root.join("svc/pricing.py"), "def apply_discount(price, pct):\n    return price - pct\n").unwrap();
        std::fs::write(root.join("tests/test_pricing.py"), "from svc.pricing import apply_discount\n\ndef test_it():\n    assert apply_discount(100, 10) == 90\n").unwrap();
        std::fs::write(root.join("README.md"), "# Demo\n\n## Pricing\nDiscounts are percentages.\n").unwrap();
        let store = Arc::new(IndexStore::in_memory().unwrap());
        let embeddings = Arc::new(EmbeddingService::load(store.clone(), Arc::new(HashingEmbedder::new(64))).unwrap());
        let indexer = Indexer::new(root, &IndexConfig::default(), IndexerOptions::default(), store, Arc::new(LanguageRegistry::with_defaults()), Some(embeddings)).unwrap();
        (dir, indexer)
    }

    #[test]
    fn full_index_populates_everything() {
        let (_dir, indexer) = fixture();
        let report = indexer.full_index(None).unwrap();
        assert_eq!(report.errors, Vec::<IndexError>::new());
        assert_eq!(report.files_scanned, 6);
        assert_eq!(report.files_indexed, 6);
        assert!(report.symbols >= 1, "rust-only registry still extracts python/ts? symbols={}", report.symbols);
        assert!(report.chunks >= 6);
        assert_eq!(report.generation, 1);
        let store = indexer.store();
        let stats = store.stats().unwrap();
        assert_eq!(stats.files, 6);
        assert!(stats.embeddings >= 6);
        // Lexical search finds the python function by identifier parts.
        let hits = store.search_lexical("\"apply\" AND \"discount\"", 10).unwrap();
        assert!(hits.iter().any(|h| h.chunk.path == "svc/pricing.py"));
        // Companion heuristics: test -> source, style -> component.
        let rels = store.relationships_from("tests/test_pricing.py").unwrap();
        assert!(rels.iter().any(|r| r.kind == "test_of" && r.to_path.as_deref() == Some("svc/pricing.py")), "{rels:?}");
        let rels = store.relationships_from("src/components/LoginButton.css").unwrap();
        assert!(rels.iter().any(|r| r.kind == "style_of" && r.to_path.as_deref() == Some("src/components/LoginButton.tsx")));
        // Markdown sections become symbols.
        assert!(!store.find_symbols("Pricing", SymbolMatchMode::Exact, 5).unwrap().is_empty());
        // Second run is a no-op apart from the generation bump.
        let again = indexer.full_index(None).unwrap();
        assert_eq!(again.files_indexed, 0);
        assert_eq!(again.files_unchanged, 6);
        assert_eq!(again.embedded, 0);
    }

    #[test]
    fn incremental_update_and_delete() {
        let (dir, indexer) = fixture();
        indexer.full_index(None).unwrap();
        let path = dir.path().join("svc/pricing.py");
        std::fs::write(&path, "def apply_percentage_discount(price, pct):\n    return price * (1 - pct / 100)\n").unwrap();
        let report = indexer.update_paths(&[path.clone()]).unwrap();
        assert_eq!(report.files_indexed, 1);
        assert_eq!(report.generation, 2);
        let store = indexer.store();
        assert!(store.search_lexical("\"percentage\"", 5).unwrap().iter().any(|h| h.chunk.path == "svc/pricing.py"));
        // Unchanged file → no work.
        let report = indexer.update_paths(&[path.clone()]).unwrap();
        assert_eq!(report.files_indexed, 0);
        assert_eq!(report.files_unchanged, 1);
        // Delete → records removed, embeddings pruned.
        std::fs::remove_file(&path).unwrap();
        let before = indexer.embeddings().unwrap().len();
        let report = indexer.update_paths(&[path]).unwrap();
        assert_eq!(report.files_removed, 1);
        assert!(store.get_file("svc/pricing.py").unwrap().is_none());
        assert!(store.search_lexical("\"percentage\"", 5).unwrap().is_empty());
        assert!(indexer.embeddings().unwrap().len() < before);
    }

    #[test]
    fn companions_link_sources_created_after_their_tests() {
        let (dir, indexer) = fixture();
        indexer.full_index(None).unwrap();
        std::fs::write(dir.path().join("tests/test_shipping.py"), "from svc.shipping import fee\n").unwrap();
        indexer.update_paths(&[dir.path().join("tests/test_shipping.py")]).unwrap();
        std::fs::write(dir.path().join("svc/shipping.py"), "def fee(): return 1\n").unwrap();
        indexer.update_paths(&[dir.path().join("svc/shipping.py")]).unwrap();
        let rels = indexer.store().relationships_from("tests/test_shipping.py").unwrap();
        assert!(rels.iter().any(|r| r.kind == "test_of" && r.to_path.as_deref() == Some("svc/shipping.py")), "{rels:?}");
        // A file that grows past the size limit is dropped from the index.
        let big = "x".repeat(2_000_000);
        std::fs::write(dir.path().join("svc/shipping.py"), big).unwrap();
        let report = indexer.update_paths(&[dir.path().join("svc/shipping.py")]).unwrap();
        assert_eq!(report.files_removed, 1);
        assert!(indexer.store().get_file("svc/shipping.py").unwrap().is_none());
    }

    #[test]
    fn ensure_fresh_reindexes_changed_files() {
        let (dir, indexer) = fixture();
        indexer.full_index(None).unwrap();
        let chunk = indexer.store().chunks_in_file("src/hooks/useAuth.ts").unwrap().remove(0);
        let source = SourceRef { path: chunk.path.clone(), start_byte: chunk.start_byte, end_byte: chunk.end_byte, start_line: chunk.start_line, end_line: chunk.end_line, content_hash: chunk.content_hash.clone(), file_hash: chunk.file_hash.clone() };
        assert!(indexer.ensure_fresh(&source).unwrap().is_fresh());
        std::fs::write(dir.path().join("src/hooks/useAuth.ts"), "// changed\nexport function useAuth() { return {}; }\n").unwrap();
        assert!(!indexer.ensure_fresh(&source).unwrap().is_fresh());
        let updated = indexer.store().get_file("src/hooks/useAuth.ts").unwrap().unwrap();
        assert_ne!(updated.hash, chunk.file_hash);
    }

    fn set_mtime(path: &Path, secs_from_now: i64) {
        let now = std::time::SystemTime::now();
        let t = if secs_from_now < 0 { now - std::time::Duration::from_secs(secs_from_now.unsigned_abs()) } else { now + std::time::Duration::from_secs(secs_from_now as u64) };
        std::fs::File::options().write(true).open(path).unwrap().set_modified(t).unwrap();
    }

    /// The fixture with every file last modified an hour ago, fully indexed.
    fn settled_fixture() -> (tempfile::TempDir, Indexer) {
        let (dir, indexer) = fixture();
        for f in indexer.walker().walk(indexer.registry()) {
            set_mtime(&f.absolute, -3600);
        }
        assert_eq!(indexer.full_index(None).unwrap().generation, 1);
        (dir, indexer)
    }

    #[test]
    fn reconcile_without_changes_reads_nothing_and_keeps_the_generation() {
        let (_dir, indexer) = settled_fixture();
        let report = indexer.reconcile().unwrap();
        assert_eq!((report.files_scanned, report.files_indexed, report.files_removed, report.files_unchanged), (6, 0, 0, 6));
        assert_eq!(report.generation, 1);
        assert_eq!(indexer.store().generation().unwrap(), 1);
    }

    #[test]
    fn reconcile_picks_up_edits_additions_and_deletions() {
        let (dir, indexer) = settled_fixture();
        let root = dir.path();
        std::fs::write(root.join("svc/pricing.py"), "def apply_surcharge(price, pct):\n    return price + pct\n").unwrap();
        std::fs::write(root.join("svc/tax.py"), "def compute_vat(amount):\n    return amount * 0.2\n").unwrap();
        std::fs::remove_file(root.join("README.md")).unwrap();
        let report = indexer.reconcile().unwrap();
        assert_eq!((report.files_indexed, report.files_removed), (2, 1));
        assert_eq!(report.generation, 2);
        assert!(indexer.store().get_file("README.md").unwrap().is_none());
        assert!(indexer.store().search_lexical("\"surcharge\"", 10).unwrap().iter().any(|h| h.chunk.path == "svc/pricing.py"));
        assert!(indexer.store().search_lexical("\"vat\"", 10).unwrap().iter().any(|h| h.chunk.path == "svc/tax.py"));
        // Settled again: nothing to do, and once the clock has moved past
        // the edits their records are trusted without reading.
        let again = indexer.reconcile().unwrap();
        assert_eq!((again.files_indexed, again.generation), (0, 2));
        std::thread::sleep(std::time::Duration::from_millis(1100));
        indexer.reconcile().unwrap();
        for f in indexer.walker().walk(indexer.registry()) {
            assert!(unchanged_on_disk(&indexer.store().get_file(&f.relative).unwrap().unwrap(), &f), "{} would be re-read", f.relative);
        }
    }

    #[test]
    fn reconcile_rereads_files_whose_mtime_is_not_older_than_their_record() {
        let (dir, indexer) = settled_fixture();
        let path = dir.path().join("svc/pricing.py");
        // Same size, and an mtime at or after the index time: stat data
        // cannot prove the file unchanged, so it must be hashed.
        std::fs::write(&path, "def apply_discount(price, pct):\n    return price + pct\n").unwrap();
        set_mtime(&path, 60);
        indexer.full_index(None).unwrap();
        std::fs::write(&path, "def apply_discount(price, pct):\n    return price * pct\n").unwrap();
        set_mtime(&path, 60);
        let report = indexer.reconcile().unwrap();
        assert_eq!(report.files_indexed, 1);
        assert!(indexer.store().search_lexical("\"apply\" AND \"discount\"", 10).unwrap().iter().any(|h| h.chunk.content.contains("price * pct")));
    }

    #[test]
    fn touched_but_identical_files_are_hashed_once_then_trusted() {
        let (dir, indexer) = settled_fixture();
        let path = dir.path().join("README.md");
        set_mtime(&path, -60);
        let report = indexer.reconcile().unwrap();
        assert_eq!((report.files_indexed, report.generation), (0, 1));
        let record = indexer.store().get_file("README.md").unwrap().unwrap();
        let walked = indexer.walker().walk(indexer.registry()).into_iter().find(|f| f.relative == "README.md").unwrap();
        assert_eq!(record.mtime, walked.mtime);
        assert!(unchanged_on_disk(&record, &walked));
    }
}
