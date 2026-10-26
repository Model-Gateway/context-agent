//! Shared helpers for integration tests.

use ctx_core::{Config, EmbeddingBackend, PermissionProfile};
use std::path::{Path, PathBuf};

/// Workspace root (the directory containing `Cargo.toml` and `tests/`).
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("workspace root")
}

pub fn fixture(name: &str) -> PathBuf {
    workspace_root().join("tests/repositories").join(name)
}

/// Test configuration: autonomous, hashing embeddings, isolated data dir.
pub fn test_config(data_dir: &Path) -> Config {
    let mut cfg = Config::default();
    cfg.data_dir = data_dir.to_path_buf();
    cfg.permissions.profile = PermissionProfile::WorkspaceAutonomous;
    cfg.embeddings.backend = EmbeddingBackend::Hashing;
    cfg.embeddings.dimension = 64;
    cfg.agent.command_timeout_secs = 120;
    cfg
}

/// Copy a fixture into a temporary directory so tests never mutate the
/// checked-in repositories.
pub fn snapshot(name: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    ctx_evaluation::runner::snapshot_repository(&fixture(name), dir.path()).unwrap();
    dir
}
