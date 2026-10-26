//! `@file` mentions: the repository files a message names as `@path`
//! (or `@"path with spaces"`), as in Claude Code and Codex. The UIs complete
//! them from [`FileIndex`] with [`search_files`]; the runtime reads each
//! mentioned file through the read policy and hands its current text to the
//! model with the message.

use ctx_indexing::Indexer;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long a walk of the repository serves completions before the next.
const FRESH_FOR: Duration = Duration::from_secs(10);

/// The repository's files (ignore rules applied), walked at most every ten
/// seconds, for completing `@` as the user types.
pub struct FileIndex {
    indexer: Arc<Indexer>,
    cached: Mutex<Option<(Instant, Arc<Vec<String>>)>>,
}

impl FileIndex {
    pub fn new(indexer: Arc<Indexer>) -> Arc<Self> {
        Arc::new(Self { indexer, cached: Mutex::new(None) })
    }

    /// Repository-relative paths, sorted.
    pub fn paths(&self) -> Arc<Vec<String>> {
        if let Some((at, paths)) = &*self.cached.lock() {
            if at.elapsed() < FRESH_FOR {
                return paths.clone();
            }
        }
        let mut paths: Vec<String> = self.indexer.walker().walk(self.indexer.registry()).into_iter().map(|f| f.relative).collect();
        paths.sort();
        let paths = Arc::new(paths);
        *self.cached.lock() = Some((Instant::now(), paths.clone()));
        paths
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<String> {
        search_files(&self.paths(), query, limit)
    }
}

/// The best `limit` paths for `query`: an exact file name, then file names
/// starting with it, then paths containing it (in the file name first),
/// then paths containing its characters in order. An empty query lists
/// the shallowest paths.
pub fn search_files(paths: &[String], query: &str, limit: usize) -> Vec<String> {
    let q = query.trim().trim_start_matches("./").to_lowercase();
    let mut scored: Vec<(i64, &String)> = Vec::new();
    for path in paths {
        let score = if q.is_empty() { Some(-(path.matches('/').count() as i64 * 100) - path.len() as i64) } else { score(&path.to_lowercase(), &q) };
        if let Some(score) = score {
            scored.push((score, path));
        }
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.len().cmp(&b.1.len())).then(a.1.cmp(b.1)));
    scored.into_iter().take(limit).map(|(_, p)| p.clone()).collect()
}

fn score(path: &str, q: &str) -> Option<i64> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let length_penalty = path.len() as i64;
    if name == q {
        return Some(10_000 - length_penalty);
    }
    if name.starts_with(q) {
        return Some(8_000 - length_penalty);
    }
    if let Some(i) = name.find(q) {
        return Some(6_000 - i as i64 * 10 - length_penalty);
    }
    if path.starts_with(q) {
        return Some(5_000 - length_penalty);
    }
    if let Some(i) = path.find(q) {
        return Some(4_000 - i as i64 - length_penalty);
    }
    // Characters in order, within the file name first (`mentmenu` →
    // MentionMenu.swift), else across the path.
    if let Some(s) = subsequence(name, q) {
        return Some(2_000 + s - length_penalty);
    }
    subsequence(path, q).map(|s| 1_000 + s - length_penalty)
}

/// Whether `q`'s characters appear in `text` in order: runs score, gaps cost.
fn subsequence(text: &str, q: &str) -> Option<i64> {
    let mut score = 0i64;
    let mut chars = text.char_indices();
    let mut last: Option<usize> = None;
    for qc in q.chars() {
        let (i, _) = chars.by_ref().find(|(_, c)| *c == qc)?;
        if let Some(prev) = last {
            if i == prev + 1 {
                score += 15;
            } else {
                score -= (i - prev) as i64;
            }
        }
        last = Some(i);
    }
    Some(score)
}

/// The text an accepted completion inserts: `@path ` or, for a path with
/// whitespace, `@"path" `.
pub fn mention_text(path: &str) -> String {
    if path.chars().any(char::is_whitespace) {
        format!("@\"{path}\" ")
    } else {
        format!("@{path} ")
    }
}

/// Every `@token` in `text`, in order, each once: `@"…"` taken whole, a
/// bare token up to whitespace. A mention starts the text or follows
/// whitespace or an opening bracket or quote (so `me@host.com` is not one).
/// For a bare token the variants without trailing punctuation are listed
/// too (`@src/a.rs,` → `src/a.rs,`, `src/a.rs`): the caller keeps the
/// first that names a file.
pub fn mention_tokens(text: &str) -> Vec<Vec<String>> {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<Vec<String>> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let starts = chars[i] == '@' && (i == 0 || chars[i - 1].is_whitespace() || "([{\"'`".contains(chars[i - 1]));
        if !starts {
            i += 1;
            continue;
        }
        let mut variants = Vec::new();
        if chars.get(i + 1) == Some(&'"') {
            let start = i + 2;
            let end = (start..chars.len()).find(|&j| chars[j] == '"');
            if let Some(end) = end {
                variants.push(chars[start..end].iter().collect::<String>());
                i = end + 1;
            } else {
                i += 1;
                continue;
            }
        } else {
            let start = i + 1;
            let mut end = start;
            while end < chars.len() && !chars[end].is_whitespace() {
                end += 1;
            }
            let mut token: String = chars[start..end].iter().collect();
            variants.push(token.clone());
            while token.ends_with(|c: char| ".,;:!?)]}'\"`".contains(c)) {
                token.pop();
                variants.push(token.clone());
            }
            i = end;
        }
        variants.retain(|v| !v.trim().is_empty());
        if !variants.is_empty() && !out.contains(&variants) {
            out.push(variants);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> Vec<String> {
        ["README.md", "src/main.rs", "src/app/main_window.rs", "crates/cli/src/tui.rs", "docs/cli.md", "tests/main_test.rs"].iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn search_prefers_names_then_paths_then_subsequences() {
        assert_eq!(search_files(&paths(), "main.rs", 3)[0], "src/main.rs", "the exact file name first");
        assert_eq!(search_files(&paths(), "main", 3), vec!["src/main.rs", "tests/main_test.rs", "src/app/main_window.rs"]);
        assert_eq!(search_files(&paths(), "cli", 2), vec!["docs/cli.md", "crates/cli/src/tui.rs"]);
        assert_eq!(search_files(&paths(), "ctui", 1), vec!["crates/cli/src/tui.rs"], "characters in order");
        let mut more = paths();
        more.extend(["apps/Components/StatusPill.swift".to_string(), "apps/Views/Composer/MentionMenu.swift".to_string()]);
        assert_eq!(search_files(&more, "mentmenu", 1), vec!["apps/Views/Composer/MentionMenu.swift"], "in the file name beats across the path");
        assert_eq!(search_files(&paths(), "", 2), vec!["README.md", "docs/cli.md"], "shallowest, then shortest, then alphabetical");
        assert!(search_files(&paths(), "zzz", 5).is_empty());
    }

    #[test]
    fn tokens_quote_and_trim_punctuation() {
        assert_eq!(mention_tokens("look at @src/a.rs, then"), vec![vec!["src/a.rs,".to_string(), "src/a.rs".to_string()]]);
        assert_eq!(mention_tokens("open @\"docs/My Notes.md\" please"), vec![vec!["docs/My Notes.md".to_string()]]);
        assert!(mention_tokens("mail me@host.com").is_empty());
        assert_eq!(mention_tokens("(@README.md)").len(), 1);
        assert_eq!(mention_text("a b/c.md"), "@\"a b/c.md\" ");
        assert_eq!(mention_text("src/a.rs"), "@src/a.rs ");
    }
}
