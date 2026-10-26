//! Staleness checks (spec §6.8): an index match is a candidate, not
//! permission to act on outdated content.

use ctx_core::{content_hash, SourceRef};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Freshness {
    /// File hash matches the reference.
    Fresh,
    /// File changed since the reference was created. `range_intact` is
    /// true when the exact byte range still hashes to the same content.
    Changed { current_hash: String, range_intact: bool },
    Missing,
}

impl Freshness {
    pub fn is_fresh(&self) -> bool {
        matches!(self, Freshness::Fresh)
    }
}

/// Compare a source reference against the working tree.
pub fn verify_source(root: &Path, source: &SourceRef) -> Freshness {
    let path = root.join(&source.path);
    let Ok(bytes) = std::fs::read(&path) else { return Freshness::Missing };
    let current_hash = content_hash(&bytes);
    if current_hash == source.file_hash {
        return Freshness::Fresh;
    }
    let range_intact = source.end_byte <= bytes.len()
        && source.start_byte <= source.end_byte
        && content_hash(&bytes[source.start_byte..source.end_byte]) == source.content_hash;
    Freshness::Changed { current_hash, range_intact }
}

/// Read the current content of a reference's byte range when the file
/// still contains it unchanged; `None` when the range moved or the file is
/// gone.
pub fn current_range(root: &Path, source: &SourceRef) -> Option<String> {
    let bytes = std::fs::read(root.join(&source.path)).ok()?;
    if source.end_byte > bytes.len() {
        return None;
    }
    let slice = &bytes[source.start_byte..source.end_byte];
    if content_hash(slice) != source.content_hash {
        return None;
    }
    Some(String::from_utf8_lossy(slice).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_changes() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.rs");
        std::fs::write(&file, "fn a() {}\nfn b() {}\n").unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        let source = SourceRef {
            path: "a.rs".into(),
            start_byte: 0,
            end_byte: 9,
            start_line: 1,
            end_line: 1,
            content_hash: content_hash(b"fn a() {}"),
            file_hash: content_hash(text.as_bytes()),
        };
        assert_eq!(verify_source(dir.path(), &source), Freshness::Fresh);
        std::fs::write(&file, "fn a() {}\nfn c() {}\n").unwrap();
        match verify_source(dir.path(), &source) {
            Freshness::Changed { range_intact, .. } => assert!(range_intact),
            other => panic!("{other:?}"),
        }
        assert_eq!(current_range(dir.path(), &source).as_deref(), Some("fn a() {}"));
        std::fs::write(&file, "// moved\nfn a() {}\n").unwrap();
        match verify_source(dir.path(), &source) {
            Freshness::Changed { range_intact, .. } => assert!(!range_intact),
            other => panic!("{other:?}"),
        }
        std::fs::remove_file(&file).unwrap();
        assert_eq!(verify_source(dir.path(), &source), Freshness::Missing);
    }
}
