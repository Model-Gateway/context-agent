//! Everything needed to work on one repository.

use crate::validation::{configured_validation_commands, validation_commands};
use ctx_core::{Config, PermissionProfile, RepositoryId, Result};
use ctx_embeddings::{build_embedder, EmbeddingService};
use ctx_indexing::{IndexReport, Indexer, IndexerOptions, RepositoryDiscovery, RepositoryProfile, ValidationCommand};
use ctx_languages::{ChunkBudget, LanguageRegistry};
use ctx_memory::MemoryStore;
use ctx_retrieval::RetrievalEngine;
use ctx_security::{PathPolicy, PolicyEngine, Redactor};
use ctx_storage::RepositoryStores;
use ctx_telemetry::EventLog;
use parking_lot::RwLock;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct RepositoryContext {
    pub root: PathBuf,
    pub id: RepositoryId,
    pub config: Config,
    pub stores: RepositoryStores,
    pub registry: Arc<LanguageRegistry>,
    pub indexer: Arc<Indexer>,
    pub embeddings: Option<Arc<EmbeddingService>>,
    pub retrieval: Arc<RetrievalEngine>,
    pub memory: MemoryStore,
    pub policy: Arc<PolicyEngine>,
    pub redactor: Arc<Redactor>,
    pub events: EventLog,
    /// The sessions working in this checkout: what each has read and
    /// written, and which are running (shared by every runtime opened on
    /// this context).
    pub activity: Arc<ctx_tools::ProjectActivity>,
    profile: RwLock<Option<RepositoryProfile>>,
}

impl RepositoryContext {
    /// Open the repository containing `start` (its git toplevel when inside
    /// a repository). Does not index; see [`Self::ensure_indexed`].
    pub fn open(start: &Path, config: Config) -> Result<Arc<Self>> {
        Self::open_exact(&RepositoryDiscovery::find_root(start), config)
    }

    /// Open `root` exactly as given, without climbing to an enclosing git
    /// repository. Used when the caller names the repository explicitly
    /// (`--repo`, benchmark fixtures, a sub-project of a monorepo).
    pub fn open_exact(root: &Path, config: Config) -> Result<Arc<Self>> {
        let root = ctx_core::platform::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        let id = RepositoryId::from_root(&root);
        let stores = RepositoryStores::open(&config, &id)?;
        Self::build(root, id, config, stores)
    }

    /// Open `root` exactly as given with in-memory stores (tests and
    /// evaluation runs, which always name the repository explicitly).
    pub fn open_ephemeral(root: &Path, config: Config) -> Result<Arc<Self>> {
        let root = ctx_core::platform::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        let id = RepositoryId::from_root(&root);
        let stores = RepositoryStores::in_memory()?;
        Self::build(root, id, config, stores)
    }

    fn build(root: PathBuf, id: RepositoryId, config: Config, stores: RepositoryStores) -> Result<Arc<Self>> {
        let registry = Arc::new(LanguageRegistry::with_defaults());
        let embeddings = match build_embedder(&config.embeddings)? {
            Some(embedder) => Some(Arc::new(EmbeddingService::load(stores.index.clone(), embedder)?)),
            None => None,
        };
        let options = IndexerOptions { chunk_budget: ChunkBudget { max_bytes: config.index.max_chunk_bytes, min_bytes: 200 }, embed: embeddings.is_some() };
        let indexer = Arc::new(Indexer::new(&root, &config.index, options, stores.index.clone(), registry.clone(), embeddings.clone())?);
        let memory = MemoryStore::new(stores.sessions.clone(), id.clone());
        let retrieval = Arc::new(RetrievalEngine::new(indexer.clone(), Some(memory.clone())));
        let mut roots = vec![indexer.root().to_path_buf()];
        roots.extend(config.permissions.extra_workspace_roots.iter().cloned());
        let paths = Arc::new(PathPolicy::new(roots, &config.index.credential_patterns)?);
        let policy = Arc::new(PolicyEngine::new(config.permissions.clone(), paths));
        let mut redactor = Redactor::new(config.telemetry.redact_secrets);
        if let Some(key) = config.model.api_key() {
            redactor.add_known_value(key);
        }
        let redactor = Arc::new(redactor.with_environment_secrets());
        let events = EventLog::new(stores.sessions.clone(), redactor.clone(), config.telemetry.store_full_tool_output);
        Ok(Arc::new(Self { root: indexer.root().to_path_buf(), id, config, stores, registry, indexer, embeddings, retrieval, memory, policy, redactor, events, activity: ctx_tools::ProjectActivity::new(), profile: RwLock::new(None) }))
    }

    pub fn with_permission_profile(self: &Arc<Self>, profile: PermissionProfile) -> Arc<PolicyEngine> {
        Arc::new((*self.policy).clone().with_profile(profile))
    }

    /// Index the repository if it has never been indexed; otherwise reconcile
    /// (hash-gated, cheap) so retrieval never runs against a stale index.
    pub fn ensure_indexed(&self, progress: Option<&dyn Fn(ctx_indexing::indexer::IndexProgress)>) -> Result<IndexReport> {
        let generation = self.stores.index.generation()?;
        let report = if generation == 0 { self.indexer.full_index(progress)? } else { self.indexer.reconcile()? };
        if report.files_indexed > 0 || report.files_removed > 0 || self.profile.read().is_none() {
            *self.profile.write() = None;
        }
        // Branch switches invalidate source-derived knowledge (spec §9.6).
        if let Some(git) = RepositoryDiscovery::git_info(&self.root) {
            let stored = self.stores.index.worktree(self.id.as_str(), &self.root.to_string_lossy())?;
            if let Some((prev_branch, _)) = stored {
                if prev_branch.is_some() && prev_branch != git.branch {
                    let n = self.memory.invalidate_for_branch_change(prev_branch.as_deref(), git.branch.as_deref())?;
                    tracing::info!(invalidated = n, "branch changed; source-derived memories invalidated");
                }
            }
            self.stores.index.upsert_repository(self.id.as_str(), &self.root.to_string_lossy())?;
            self.stores.index.upsert_worktree(self.id.as_str(), &self.root.to_string_lossy(), git.branch.as_deref(), git.head.as_deref())?;
        }
        Ok(report)
    }

    /// Repository profile (cached until the index changes).
    pub fn profile(&self) -> Result<RepositoryProfile> {
        if let Some(p) = self.profile.read().as_ref() {
            return Ok(p.clone());
        }
        let p = self.indexer.profile()?;
        *self.profile.write() = Some(p.clone());
        Ok(p)
    }

    /// The profile when it needs no walk of the repository: cached, or
    /// read from a published index. `None` while the first index runs.
    pub fn indexed_profile(&self) -> Result<Option<RepositoryProfile>> {
        if let Some(p) = self.profile.read().as_ref() {
            return Ok(Some(p.clone()));
        }
        let Some(p) = self.indexer.indexed_profile()? else { return Ok(None) };
        *self.profile.write() = Some(p.clone());
        Ok(Some(p))
    }

    /// Validation commands without walking the repository: configured
    /// ones always, detected ones once `indexed_profile` has a profile.
    pub fn indexed_validation_commands(&self) -> Result<Vec<ValidationCommand>> {
        if let Some(commands) = configured_validation_commands(&self.config.validation) {
            return Ok(commands);
        }
        Ok(self.indexed_profile()?.map(|p| p.validation).unwrap_or_default())
    }

    pub fn validation_commands(&self) -> Result<Vec<ValidationCommand>> {
        Ok(validation_commands(&self.config.validation, &self.profile()?))
    }

    /// Stable text describing the repository for the trusted instruction
    /// layer.
    pub fn repository_summary(&self) -> Result<String> {
        self.repository_summary_for(&self.policy)
    }

    /// [`Self::repository_summary`] stating `policy`'s permission profile
    /// (a runtime may run under a different profile than configured).
    pub fn repository_summary_for(&self, policy: &PolicyEngine) -> Result<String> {
        let profile = self.profile()?;
        let mut s = String::new();
        s.push_str(&format!("Repository: {} ({} files", self.root.display(), profile.file_count));
        if let Some(git) = &profile.git {
            s.push_str(&format!(", branch {}{}", git.branch.clone().unwrap_or_else(|| "detached".into()), if git.dirty_files.is_empty() { String::new() } else { format!(", {} uncommitted change(s) that must be preserved", git.dirty_files.len()) }));
        }
        s.push_str(").\n");
        let langs: Vec<String> = profile.languages.iter().filter(|(l, _)| *l != "other").map(|(l, n)| format!("{l} ({n})")).collect();
        if !langs.is_empty() {
            s.push_str(&format!("Languages: {}.\n", langs.join(", ")));
        }
        if !profile.build_files.is_empty() {
            s.push_str(&format!("Build files: {}.\n", profile.build_files.iter().take(8).map(|b| b.path.clone()).collect::<Vec<_>>().join(", ")));
        }
        if self.validation_commands()?.is_empty() {
            s.push_str("Validation: no project test/build command was detected; use execute_command with the project's own tooling if you want to check your work.\n");
        } else {
            s.push_str("Validation: run_validation runs the project's detected test, build, typecheck and lint commands.\n");
        }
        if !profile.docs.is_empty() {
            s.push_str(&format!("Documentation: {}.\n", profile.docs.iter().take(5).cloned().collect::<Vec<_>>().join(", ")));
        }
        s.push_str(&format!("Permission profile: {}. {}\n", policy.profile().as_str(), match policy.profile() {
            PermissionProfile::ApprovalRequired => "Edits and non-read-only commands prompt the user for approval; batch related edits sensibly.",
            PermissionProfile::WorkspaceAutonomous => "Routine edits and validation run without prompts; network and destructive commands prompt.",
            PermissionProfile::BypassPermissions => "Nothing prompts: edits and commands run immediately, so take extra care with destructive or irreversible actions. Denied commands, credential files and paths outside the workspace are still blocked.",
        }));
        if policy.sandbox().enforced {
            s.push_str(&format!("Command wrapper (developer mode): {}\n", policy.sandbox().description));
        }
        Ok(s)
    }
}
