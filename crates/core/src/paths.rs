//! Application data directory layout (spec §13.1).

use crate::ids::RepositoryId;
use std::path::PathBuf;

pub const DATA_DIR_ENV: &str = "CTX_DATA_DIR";

/// `~/.context-agent` unless overridden by `CTX_DATA_DIR`.
pub fn default_data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var(DATA_DIR_ENV) {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    if let Some(base) = directories::BaseDirs::new() {
        return base.home_dir().join(".context-agent");
    }
    PathBuf::from(".context-agent")
}

#[derive(Debug, Clone)]
pub struct DataLayout {
    pub root: PathBuf,
}

impl DataLayout {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn config_file(&self) -> PathBuf {
        self.root.join("config.toml")
    }

    pub fn repositories_dir(&self) -> PathBuf {
        self.root.join("repositories")
    }

    pub fn repository_dir(&self, id: &RepositoryId) -> PathBuf {
        self.repositories_dir().join(id.as_str())
    }

    pub fn index_db(&self, id: &RepositoryId) -> PathBuf {
        self.repository_dir(id).join("metadata.sqlite")
    }

    pub fn sessions_db(&self, id: &RepositoryId) -> PathBuf {
        self.repository_dir(id).join("sessions.sqlite")
    }

    pub fn vectors_file(&self, id: &RepositoryId) -> PathBuf {
        self.repository_dir(id).join("vectors.index")
    }

    pub fn providers_dir(&self) -> PathBuf {
        self.root.join("providers")
    }

    /// Linked git worktrees the desktop app creates for parallel sessions
    /// (`worktree/create`), grouped by the parent repository's id.
    pub fn worktrees_dir(&self) -> PathBuf {
        self.root.join("worktrees")
    }

    pub fn worktree_dir(&self, parent: &RepositoryId, slug: &str) -> PathBuf {
        self.worktrees_dir().join(parent.as_str()).join(slug)
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    pub fn ensure_repository_dir(&self, id: &RepositoryId) -> std::io::Result<PathBuf> {
        let dir = self.repository_dir(id);
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }
}
