//! Canonical identifiers.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::path::Path;

macro_rules! id_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Generate a fresh random identifier.
            pub fn new() -> Self {
                Self(uuid::Uuid::new_v4().to_string())
            }

            /// Wrap an existing identifier string (e.g. loaded from storage).
            pub fn from_string(s: impl Into<String>) -> Self {
                Self(s.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// A short, human-friendly prefix for display.
            pub fn short(&self) -> &str {
                &self.0[..self.0.len().min(8)]
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), self.0)
            }
        }

        impl From<String> for $name {
            fn from(s: String) -> Self {
                Self(s)
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.to_string())
            }
        }
    };
}

id_type!(
    /// Identifies a task: one user objective that may span many model turns.
    TaskId
);
id_type!(
    /// Identifies an interactive or headless session containing tasks.
    SessionId
);
id_type!(
    /// Identifies a memory record.
    MemoryId
);
id_type!(
    /// Identifies a context checkpoint.
    CheckpointId
);
id_type!(
    /// Identifies a source chunk in the index.
    ChunkId
);

/// Canonical repository identity, derived from the canonical root path so
/// that every CLI invocation against the same checkout agrees on it.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RepositoryId(String);

impl RepositoryId {
    pub fn from_root(root: &Path) -> Self {
        let canonical = crate::platform::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        let mut hasher = Sha256::new();
        hasher.update(canonical.to_string_lossy().as_bytes());
        let digest = hasher.finalize();
        Self(hex::encode(&digest[..12]))
    }

    pub fn from_string(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RepositoryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for RepositoryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RepositoryId({})", self.0)
    }
}

/// Wall-clock timestamp used across storage and events.
pub type Timestamp = chrono::DateTime<chrono::Utc>;

pub fn now() -> Timestamp {
    chrono::Utc::now()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_id_is_stable_for_same_path() {
        let dir = tempfile::tempdir().unwrap();
        let a = RepositoryId::from_root(dir.path());
        let b = RepositoryId::from_root(dir.path());
        assert_eq!(a, b);
        assert_eq!(a.as_str().len(), 24);
    }

    #[test]
    fn task_ids_are_unique() {
        assert_ne!(TaskId::new(), TaskId::new());
    }
}
