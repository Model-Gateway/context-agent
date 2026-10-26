//! Repository indexing (spec §6.4–§6.8).
//!
//! * [`discovery`] finds the repository root, worktree identity, languages,
//!   build files, and candidate validation commands.
//! * [`walker`] enumerates indexable files honouring `.gitignore`, custom
//!   exclusions, size limits, binary detection, and credential patterns.
//! * [`Indexer`] performs full and incremental indexing: hashing, parsing,
//!   symbol/chunk/relationship extraction, lexical and vector index updates,
//!   relationship resolution, and generation publishing.
//! * [`freshness`] verifies retrieved source against the working tree.
//! * [`watcher`] reacts to filesystem events and reconciles missed ones.

pub mod discovery;
pub mod freshness;
pub mod indexer;
pub mod resolve;
pub mod walker;
pub mod watcher;

pub use discovery::{BuildSystem, GitInfo, RepositoryDiscovery, RepositoryProfile, ValidationCommand, ValidationKind};
pub use freshness::{verify_source, Freshness};
pub use indexer::{IndexReport, Indexer, IndexerOptions};
pub use walker::{FileKind, WalkedFile, Walker};
pub use watcher::IndexWatcher;
