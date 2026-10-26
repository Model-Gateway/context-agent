//! Workspace path validation.

use ctx_core::{CtxError, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use std::path::{Component, Path, PathBuf};

/// Enforces that file operations stay inside allowed roots and never touch
/// credential files.
#[derive(Debug, Clone)]
pub struct PathPolicy {
    roots: Vec<PathBuf>,
    credential_globs: GlobSet,
    credential_patterns: Vec<String>,
}

impl PathPolicy {
    pub fn new(roots: Vec<PathBuf>, credential_patterns: &[String]) -> Result<Self> {
        let mut builder = GlobSetBuilder::new();
        for pat in credential_patterns {
            let glob = Glob::new(pat).map_err(|e| CtxError::Config(format!("invalid credential pattern {pat}: {e}")))?;
            builder.add(glob);
        }
        let credential_globs = builder.build().map_err(|e| CtxError::Config(e.to_string()))?;
        let roots = roots
            .into_iter()
            .map(|r| ctx_core::platform::canonicalize(&r).unwrap_or(r))
            .collect();
        Ok(Self { roots, credential_globs, credential_patterns: credential_patterns.to_vec() })
    }

    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    pub fn primary_root(&self) -> &Path {
        self.roots.first().map(|p| p.as_path()).unwrap_or_else(|| Path::new("."))
    }

    pub fn credential_patterns(&self) -> &[String] {
        &self.credential_patterns
    }

    /// Lexically normalize `..` and `.` components without touching the
    /// filesystem.
    pub fn normalize(path: &Path) -> PathBuf {
        let mut out = PathBuf::new();
        for comp in path.components() {
            match comp {
                Component::ParentDir => {
                    if !out.pop() {
                        out.push("..");
                    }
                }
                Component::CurDir => {}
                other => out.push(other.as_os_str()),
            }
        }
        out
    }

    /// Resolve a user-supplied path (absolute or relative to the primary
    /// root) to an absolute path guaranteed to live inside an allowed root.
    /// For paths that do not exist yet, the nearest existing ancestor is
    /// canonicalized so symlink escapes are still caught.
    pub fn resolve(&self, user_path: &str) -> Result<PathBuf> {
        let canonical = self.resolve_unrestricted(user_path);
        if !self.roots.iter().any(|root| canonical.starts_with(root)) {
            return Err(CtxError::PermissionDenied(format!(
                "path {} resolves outside the allowed workspace roots",
                user_path
            )));
        }
        Ok(canonical)
    }

    /// Like [`resolve`](Self::resolve) without the workspace-root check, for
    /// the bypass profile, which may reach anywhere on disk.
    pub fn resolve_unrestricted(&self, user_path: &str) -> PathBuf {
        let raw = Path::new(user_path);
        let joined = if raw.is_absolute() { raw.to_path_buf() } else { self.primary_root().join(raw) };
        let normalized = Self::normalize(&joined);

        // Canonicalize the deepest existing ancestor.
        let mut existing = normalized.clone();
        let mut tail: Vec<std::ffi::OsString> = Vec::new();
        while !existing.exists() {
            match existing.file_name() {
                Some(name) => {
                    tail.push(name.to_os_string());
                    if !existing.pop() {
                        break;
                    }
                }
                None => break,
            }
        }
        let mut canonical = ctx_core::platform::canonicalize(&existing).unwrap_or(existing);
        for part in tail.iter().rev() {
            canonical.push(part);
        }
        canonical
    }

    /// Repository-relative display path for an absolute path inside a root.
    pub fn relative(&self, absolute: &Path) -> String {
        for root in &self.roots {
            if let Ok(rel) = absolute.strip_prefix(root) {
                return rel.to_string_lossy().replace('\\', "/");
            }
        }
        absolute.to_string_lossy().replace('\\', "/")
    }

    /// Whether a repository-relative path matches a credential pattern.
    /// Matching is case-insensitive: on case-insensitive filesystems
    /// `.ENV` opens `.env`.
    pub fn is_credential_path(&self, relative: &str) -> bool {
        let rel = relative.trim_start_matches("./").replace('\\', "/");
        let lower = rel.to_lowercase();
        [rel.as_str(), lower.as_str()].iter().any(|r| {
            self.credential_globs.is_match(r)
                || self.credential_globs.is_match(format!("x/{r}"))
                || Path::new(r).file_name().map(|f| self.credential_globs.is_match(f)).unwrap_or(false)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(root: &Path) -> PathPolicy {
        let cfg = ctx_core::IndexConfig::default();
        PathPolicy::new(vec![root.to_path_buf()], &cfg.credential_patterns).unwrap()
    }

    #[test]
    fn rejects_parent_escape() {
        let dir = tempfile::tempdir().unwrap();
        let p = policy(dir.path());
        assert!(p.resolve("../outside.txt").is_err());
        assert!(p.resolve("src/../../etc/passwd").is_err());
        assert!(p.resolve("/etc/passwd").is_err());
    }

    #[test]
    fn resolves_new_files_inside_root() {
        let dir = tempfile::tempdir().unwrap();
        let p = policy(dir.path());
        let resolved = p.resolve("src/new/file.rs").unwrap();
        assert!(resolved.ends_with("src/new/file.rs"));
        assert_eq!(p.relative(&resolved), "src/new/file.rs");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_escape() {
        let outside = tempfile::tempdir().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(outside.path(), &link).unwrap();
        let p = policy(dir.path());
        assert!(p.resolve("link/secret.txt").is_err());
    }

    #[test]
    fn credential_patterns() {
        let dir = tempfile::tempdir().unwrap();
        let p = policy(dir.path());
        assert!(p.is_credential_path(".env"));
        assert!(p.is_credential_path("config/.env.production"));
        assert!(p.is_credential_path("certs/server.pem"));
        assert!(p.is_credential_path("~/.ssh/id_rsa"));
        assert!(p.is_credential_path(".ENV"));
        assert!(p.is_credential_path("Config/.Env.Production"));
        assert!(!p.is_credential_path("src/environment.ts"));
        assert!(!p.is_credential_path("README.md"));
    }
}
