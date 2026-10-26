//! SQLite persistence (spec §13).
//!
//! Two logical databases per repository:
//! * the **index store** (`metadata.sqlite`): files, symbols, chunks,
//!   lexical index, relationships, embeddings;
//! * the **session store** (`sessions.sqlite`): sessions, tasks, the event
//!   log, conversation messages, memories, checkpoints, model requests,
//!   validation runs, and metrics.
//!
//! Every repository-bound record carries the repository id and index records
//! carry a generation number so stale matches can be detected.

pub mod db;
pub mod index;
pub mod records;
pub mod schema;
pub mod sessions;
pub mod spend;

pub use db::Database;
pub use index::{fts_quote, IndexStats, IndexStore, LexicalHit, SymbolMatchMode};
pub use records::*;
pub use sessions::{MemoryFilter, SessionStore};
pub use spend::SpendLedger;

use ctx_core::{Config, RepositoryId, Result};
use std::sync::Arc;

/// Convenience bundle of both stores for one repository.
#[derive(Clone)]
pub struct RepositoryStores {
    pub index: Arc<IndexStore>,
    pub sessions: Arc<SessionStore>,
    /// Shared by every repository (`<data_dir>/spend.sqlite`).
    pub spend: Arc<SpendLedger>,
}

impl RepositoryStores {
    /// Open (creating if needed) both stores under the configured data dir.
    pub fn open(config: &Config, repository_id: &RepositoryId) -> Result<Self> {
        let layout = config.layout();
        layout.ensure_repository_dir(repository_id)?;
        let index = IndexStore::open(&layout.index_db(repository_id))?;
        let sessions = SessionStore::open(&layout.sessions_db(repository_id))?;
        let spend = SpendLedger::open(&layout)?;
        Ok(Self { index: Arc::new(index), sessions: Arc::new(sessions), spend: Arc::new(spend) })
    }

    /// In-memory stores for tests and ephemeral runs.
    pub fn in_memory() -> Result<Self> {
        Ok(Self {
            index: Arc::new(IndexStore::in_memory()?),
            sessions: Arc::new(SessionStore::in_memory()?),
            spend: Arc::new(SpendLedger::in_memory()?),
        })
    }
}
