//! Canonical source references (spec §6.5, §7.12).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Hash the given bytes with SHA-256 and return a `sha256:<hex>` string.
pub fn content_hash(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

/// Reference to an exact byte range of a repository file at a known version.
/// Byte offsets and hashes are canonical; line numbers are presentation
/// metadata.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceRef {
    /// Repository-relative path using `/` separators.
    pub path: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: usize,
    pub end_line: usize,
    /// Hash of the referenced range.
    pub content_hash: String,
    /// Hash of the whole file when the reference was created.
    pub file_hash: String,
}

impl SourceRef {
    pub fn whole_file(path: impl Into<String>, content: &str) -> Self {
        let hash = content_hash(content.as_bytes());
        Self {
            path: path.into(),
            start_byte: 0,
            end_byte: content.len(),
            start_line: 1,
            end_line: content.lines().count().max(1),
            content_hash: hash.clone(),
            file_hash: hash,
        }
    }

    pub fn display_range(&self) -> String {
        format!("{}:{}-{}", self.path, self.start_line, self.end_line)
    }

    pub fn len_bytes(&self) -> usize {
        self.end_byte.saturating_sub(self.start_byte)
    }
}

/// A snippet whose content has been verified against the current file on
/// disk before being supplied to the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerifiedSourceSnippet {
    pub source: SourceRef,
    pub content: String,
    pub symbol: Option<String>,
    pub language: Option<String>,
    /// Why the retrieval engine chose this snippet.
    pub rationale: String,
    pub score: f64,
}

/// Normalize a path to a repository-relative, forward-slash string.
pub fn relative_display(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let s = rel.to_string_lossy().replace('\\', "/");
    s.trim_start_matches("./").to_string()
}

pub fn join_relative(root: &Path, rel: &str) -> PathBuf {
    root.join(rel)
}

/// Compute 1-based line numbers for a byte range in `text`.
pub fn line_range(text: &str, start_byte: usize, end_byte: usize) -> (usize, usize) {
    let start_byte = start_byte.min(text.len());
    let end_byte = end_byte.min(text.len()).max(start_byte);
    let start_line = text[..start_byte].matches('\n').count() + 1;
    let inner = &text[start_byte..end_byte];
    let mut end_line = start_line + inner.matches('\n').count();
    if inner.ends_with('\n') && end_line > start_line {
        end_line -= 1;
    }
    (start_line, end_line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashing_is_prefixed() {
        assert!(content_hash(b"abc").starts_with("sha256:"));
    }

    #[test]
    fn line_ranges() {
        let text = "a\nb\nc\n";
        assert_eq!(line_range(text, 0, 1), (1, 1));
        assert_eq!(line_range(text, 2, 6), (2, 3));
        assert_eq!(line_range(text, 0, 6), (1, 3));
    }

    #[test]
    fn relative_display_normalizes() {
        let root = Path::new("/repo");
        assert_eq!(relative_display(root, Path::new("/repo/src/a.rs")), "src/a.rs");
    }
}
