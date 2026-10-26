//! File enumeration.

use ctx_core::{IndexConfig, Result};
use ctx_languages::LanguageRegistry;
use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::{ParallelVisitor, ParallelVisitorBuilder, WalkBuilder, WalkState};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    Source,
    Test,
    Doc,
    Config,
    Build,
}

impl FileKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            FileKind::Source => "source",
            FileKind::Test => "test",
            FileKind::Doc => "doc",
            FileKind::Config => "config",
            FileKind::Build => "build",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WalkedFile {
    pub absolute: PathBuf,
    pub relative: String,
    pub size: u64,
    pub mtime: Option<i64>,
    pub kind: FileKind,
    pub language: Option<String>,
}

pub struct Walker {
    root: PathBuf,
    excludes: GlobSet,
    credentials: GlobSet,
    max_file_bytes: u64,
    index_hidden: bool,
    /// Root-level ignore rules so single-path decisions agree with the walk.
    gitignore: Option<ignore::gitignore::Gitignore>,
}

const BINARY_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "bmp", "ico", "webp", "svgz", "pdf", "zip", "gz", "tgz", "bz2", "xz", "7z", "rar", "jar", "war",
    "class", "o", "a", "so", "dylib", "dll", "exe", "bin", "wasm", "pyc", "pyo", "ttf", "otf", "woff", "woff2", "eot", "mp3",
    "mp4", "mov", "avi", "mkv", "wav", "flac", "ogg", "sqlite", "db", "ds_store", "lock", "psd", "ai", "heic",
];

pub fn build_globset(patterns: &[String]) -> Result<GlobSet> {
    let mut b = GlobSetBuilder::new();
    for p in patterns {
        b.add(Glob::new(p).map_err(|e| ctx_core::CtxError::Config(format!("invalid glob {p}: {e}")))?);
    }
    b.build().map_err(|e| ctx_core::CtxError::Config(e.to_string()))
}

impl Walker {
    pub fn new(root: &Path, config: &IndexConfig) -> Result<Self> {
        let mut builder = ignore::gitignore::GitignoreBuilder::new(root);
        let mut any = false;
        for file in [root.join(".gitignore"), root.join(".ignore"), root.join(".git/info/exclude")] {
            if file.is_file() {
                builder.add(file);
                any = true;
            }
        }
        let gitignore = if any { builder.build().ok() } else { None };
        Ok(Self {
            root: root.to_path_buf(),
            excludes: build_globset(&config.exclude)?,
            credentials: build_globset(&config.credential_patterns)?,
            max_file_bytes: config.max_file_bytes,
            index_hidden: config.index_hidden,
            gitignore,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn is_excluded(&self, rel: &str) -> bool {
        self.excludes.is_match(rel) || self.excludes.is_match(format!("x/{rel}"))
    }

    pub fn is_credential(&self, rel: &str) -> bool {
        let name = rel.rsplit('/').next().unwrap_or(rel);
        self.credentials.is_match(rel) || self.credentials.is_match(format!("x/{rel}")) || self.credentials.is_match(name)
    }

    /// Decide whether a single path (e.g. from a watcher event) is indexable.
    pub fn should_index(&self, abs: &Path) -> bool {
        let Ok(rel) = abs.strip_prefix(&self.root) else { return false };
        let rel = rel.to_string_lossy().replace('\\', "/");
        if rel.is_empty() || self.is_excluded(&rel) || self.is_credential(&rel) {
            return false;
        }
        if !self.index_hidden && rel.split('/').any(|seg| seg.starts_with('.') && seg != ".") {
            return false;
        }
        if rel.split('/').any(|seg| seg == ".git") {
            return false;
        }
        if let Some(gi) = &self.gitignore {
            if gi.matched_path_or_any_parents(abs, false).is_ignore() {
                return false;
            }
        }
        let Ok(meta) = std::fs::metadata(abs) else { return false };
        if !meta.is_file() || meta.len() > self.max_file_bytes {
            return false;
        }
        !is_binary(abs)
    }

    pub fn classify(rel: &str, registry: &LanguageRegistry) -> (FileKind, Option<String>) {
        let adapter = registry.detect(rel);
        let language = adapter.as_ref().map(|a| a.identifier().as_str().to_string());
        let name = rel.rsplit('/').next().unwrap_or(rel);
        let lower = rel.to_ascii_lowercase();
        let kind = if matches!(name, "package.json" | "Cargo.toml" | "pyproject.toml" | "setup.py" | "go.mod" | "pom.xml" | "build.gradle" | "build.gradle.kts" | "Makefile" | "CMakeLists.txt" | "Gemfile" | "Rakefile" | "composer.json" | "tsconfig.json") {
            FileKind::Build
        } else if adapter.as_ref().map(|a| a.is_test_path(rel)).unwrap_or(false) {
            FileKind::Test
        } else if lower.ends_with(".md") || lower.ends_with(".mdx") || lower.ends_with(".rst") || lower.ends_with(".txt") || lower.starts_with("docs/") || lower.contains("/docs/") {
            FileKind::Doc
        } else if lower.ends_with(".json") || lower.ends_with(".yaml") || lower.ends_with(".yml") || lower.ends_with(".toml") || lower.ends_with(".ini") || lower.ends_with(".cfg") || lower.ends_with(".env.example") {
            FileKind::Config
        } else {
            FileKind::Source
        };
        (kind, language)
    }

    /// Whether every path beneath directory `rel` is excluded (or is
    /// credential material), so the walk need not descend into it. Probed
    /// one and two levels down with names no pattern targets, so a glob
    /// such as `docs/*.md` (some files only) never prunes `docs/`.
    fn prunes_dir(&self, rel: &str) -> bool {
        let probes = [format!("{rel}/\u{1}"), format!("{rel}/\u{1}/\u{1}")];
        probes.iter().all(|p| self.is_excluded(p)) || probes.iter().all(|p| self.credentials.is_match(p) || self.credentials.is_match(format!("x/{p}")))
    }

    /// One walked entry: `None` when it is not an indexable file.
    fn visit(&self, entry: &ignore::DirEntry, registry: &LanguageRegistry) -> Option<WalkedFile> {
        let abs = entry.path();
        let rel = abs.strip_prefix(&self.root).ok()?.to_string_lossy().replace('\\', "/");
        if self.is_excluded(&rel) || self.is_credential(&rel) {
            return None;
        }
        let meta = entry.metadata().ok()?;
        if meta.len() > self.max_file_bytes {
            return None;
        }
        let (kind, language) = Self::classify(&rel, registry);
        // Opening every file to sniff it dominates the walk of a large
        // tree; a file of a known language is taken to be text here and
        // `Indexer::index_file` drops the rare binary one from the bytes
        // it reads anyway.
        let binary = if language.is_some() { has_binary_extension(abs) } else { is_binary(abs) };
        if binary {
            return None;
        }
        let mtime = meta.modified().ok().and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64);
        Some(WalkedFile { absolute: abs.to_path_buf(), relative: rel, size: meta.len(), mtime, kind, language })
    }

    /// Enumerate indexable files, in parallel; excluded directories are
    /// pruned rather than walked.
    pub fn walk(&self, registry: &LanguageRegistry) -> Vec<WalkedFile> {
        let out = Mutex::new(Vec::new());
        WalkBuilder::new(&self.root)
            .hidden(!self.index_hidden)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .require_git(false)
            .follow_links(false)
            .filter_entry(|entry| entry.file_name() != ".git")
            .build_parallel()
            .visit(&mut Collector { walker: self, registry, out: &out });
        let mut out = out.into_inner();
        out.sort_by(|a, b| a.relative.cmp(&b.relative));
        out
    }
}

/// Builds one [`Visit`] per walker thread.
struct Collector<'a> {
    walker: &'a Walker,
    registry: &'a LanguageRegistry,
    out: &'a Mutex<Vec<WalkedFile>>,
}

impl<'s> ParallelVisitorBuilder<'s> for Collector<'s> {
    fn build(&mut self) -> Box<dyn ParallelVisitor + 's> {
        Box::new(Visit { walker: self.walker, registry: self.registry, out: self.out, found: Vec::new() })
    }
}

/// A walker thread's visitor: collects locally, hands its files over once
/// when the thread finishes.
struct Visit<'a> {
    walker: &'a Walker,
    registry: &'a LanguageRegistry,
    out: &'a Mutex<Vec<WalkedFile>>,
    found: Vec<WalkedFile>,
}

impl ParallelVisitor for Visit<'_> {
    fn visit(&mut self, entry: std::result::Result<ignore::DirEntry, ignore::Error>) -> WalkState {
        let Ok(entry) = entry else { return WalkState::Continue };
        let Some(ft) = entry.file_type() else { return WalkState::Continue };
        if ft.is_dir() {
            let rel = entry.path().strip_prefix(&self.walker.root).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_default();
            if !rel.is_empty() && self.walker.prunes_dir(&rel) {
                return WalkState::Skip;
            }
        } else if ft.is_file() {
            self.found.extend(self.walker.visit(&entry, self.registry));
        }
        WalkState::Continue
    }
}

impl Drop for Visit<'_> {
    fn drop(&mut self) {
        self.out.lock().append(&mut self.found);
    }
}

fn has_binary_extension(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|ext| BINARY_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

/// Whether already-read content looks binary (a NUL in its first 8 KiB).
pub fn looks_binary(bytes: &[u8]) -> bool {
    bytes[..bytes.len().min(8192)].contains(&0)
}

/// Binary detection: known extensions, then a NUL-byte sniff of the first
/// 8 KiB.
pub fn is_binary(path: &Path) -> bool {
    if has_binary_extension(path) {
        return true;
    }
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(path) else { return true };
    let mut buf = [0u8; 8192];
    let Ok(n) = f.read(&mut buf) else { return true };
    looks_binary(&buf[..n])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walk_respects_gitignore_excludes_and_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("node_modules/x")).unwrap();
        std::fs::create_dir_all(root.join("build")).unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
        std::fs::write(root.join("src/lib_test.rs"), "#[test] fn t() {}").unwrap();
        std::fs::write(root.join("node_modules/x/index.js"), "x").unwrap();
        std::fs::write(root.join("build/out.txt"), "x").unwrap();
        std::fs::write(root.join(".env"), "SECRET=1").unwrap();
        std::fs::write(root.join("server.pem"), "-----BEGIN").unwrap();
        std::fs::write(root.join("image.png"), [0u8, 1, 2]).unwrap();
        std::fs::write(root.join("blob.dat"), [b'a', 0, b'b']).unwrap();
        std::fs::write(root.join("ignored.log"), "x").unwrap();
        std::fs::write(root.join(".gitignore"), "*.log\n").unwrap();
        std::fs::write(root.join("README.md"), "# hi").unwrap();
        let cfg = IndexConfig::default();
        let walker = Walker::new(root, &cfg).unwrap();
        let files = walker.walk(&LanguageRegistry::with_defaults());
        let rels: Vec<&str> = files.iter().map(|f| f.relative.as_str()).collect();
        assert!(rels.contains(&"src/main.rs"));
        assert!(rels.contains(&"README.md"));
        assert!(!rels.iter().any(|r| r.starts_with("node_modules")));
        assert!(!rels.iter().any(|r| r.starts_with("build/")));
        assert!(!rels.contains(&".env"));
        assert!(!rels.contains(&"server.pem"));
        assert!(!rels.contains(&"image.png"));
        assert!(!rels.contains(&"blob.dat"));
        assert!(!rels.contains(&"ignored.log"), "gitignore honoured without a .git dir");
        let main = files.iter().find(|f| f.relative == "src/main.rs").unwrap();
        assert_eq!(main.kind, FileKind::Source);
        assert_eq!(main.language.as_deref(), Some("rust"));
        assert_eq!(files.iter().find(|f| f.relative == "README.md").unwrap().kind, FileKind::Doc);
        assert!(walker.should_index(&root.join("src/main.rs")));
        assert!(!walker.should_index(&root.join(".env")));
        assert!(!walker.should_index(&root.join("ignored.log")), "single-path checks honour .gitignore");
    }

    #[test]
    fn walk_prunes_only_directories_excluded_all_the_way_down() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("docs/deep")).unwrap();
        std::fs::create_dir_all(root.join("pkg/node_modules/a/b")).unwrap();
        std::fs::write(root.join("docs/top.md"), "# top").unwrap();
        std::fs::write(root.join("docs/deep/nested.md"), "# nested").unwrap();
        std::fs::write(root.join("docs/deep/keep.txt"), "kept").unwrap();
        std::fs::write(root.join("pkg/node_modules/a/b/index.js"), "x").unwrap();
        std::fs::write(root.join("pkg/main.rs"), "fn main() {}").unwrap();
        // A known language with a NUL is kept by the walk (no sniff) and
        // dropped by the indexer; an unknown one is sniffed here.
        std::fs::write(root.join("pkg/odd.dat"), [b'a', 0]).unwrap();
        let cfg = IndexConfig { exclude: vec!["docs/*.md".into(), "**/node_modules/**".into()], ..IndexConfig::default() };
        let walker = Walker::new(root, &cfg).unwrap();
        assert!(walker.prunes_dir("pkg/node_modules"));
        assert!(!walker.prunes_dir("docs"), "docs/*.md excludes some files only");
        let rels: Vec<String> = walker.walk(&LanguageRegistry::with_defaults()).into_iter().map(|f| f.relative).collect();
        assert_eq!(rels, vec!["docs/deep/keep.txt".to_string(), "pkg/main.rs".to_string()]);
    }
}
