//! Deterministic query analysis (spec §7.3).

use ctx_core::TaskCategory;
use ctx_languages::split_identifier;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct QueryAnalysis {
    pub raw: String,
    /// Exact file names mentioned (e.g. `LoginButton.tsx`).
    pub filenames: Vec<String>,
    /// Path fragments with directories (e.g. `src/auth/login.ts`).
    pub paths: Vec<String>,
    /// Extensions explicitly mentioned (`.tsx`, `tsx files`).
    pub extensions: Vec<String>,
    /// Identifier-like tokens (camelCase, snake_case, PascalCase, dotted).
    pub symbols: Vec<String>,
    /// Quoted strings, likely user-visible text or literals.
    pub quoted: Vec<String>,
    /// Natural-language keywords (lowercased content words + identifier parts).
    pub keywords: Vec<String>,
    /// Directory scope the user restricted the request to (`in src/auth`).
    pub scope: Vec<String>,
    /// The request mentions tests explicitly.
    pub mentions_tests: bool,
    /// The request mentions documentation explicitly.
    pub mentions_docs: bool,
    /// Refers to earlier work ("the previous change", "earlier").
    pub refers_to_prior_work: bool,
    pub category: TaskCategory,
}

const STOPWORDS: &[&str] = &[
    "the", "a", "an", "and", "or", "but", "of", "to", "in", "on", "for", "with", "that", "this", "these", "those", "it", "its", "is",
    "are", "was", "were", "be", "been", "have", "has", "had", "do", "does", "did", "can", "could", "should", "would", "will",
    "please", "make", "add", "update", "change", "fix", "use", "using", "set", "get", "into", "from", "as", "at", "by", "so",
    "if", "then", "than", "also", "too", "just", "now", "very", "more", "less", "some", "all", "any", "not", "no", "yes", "you",
    "we", "i", "me", "my", "our", "your", "there", "here", "when", "where", "which", "what", "who", "how", "why", "want", "need",
    "like", "new", "one", "way", "code", "file", "files", "function", "method", "class", "should", "must", "only", "after", "before",
    "while", "during", "instead", "existing", "current", "currently", "still", "already", "sure", "ensure", "implement", "write",
];

fn re(pattern: &str, cell: &'static OnceLock<Regex>) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("valid regex"))
}

static PATH_RE: OnceLock<Regex> = OnceLock::new();
static FILE_RE: OnceLock<Regex> = OnceLock::new();
static QUOTE_RE: OnceLock<Regex> = OnceLock::new();
static IDENT_RE: OnceLock<Regex> = OnceLock::new();
static EXT_RE: OnceLock<Regex> = OnceLock::new();
static SCOPE_RE: OnceLock<Regex> = OnceLock::new();

const KNOWN_EXTS: &[&str] = &["ts", "tsx", "js", "jsx", "mjs", "cjs", "py", "rs", "go", "java", "c", "h", "cpp", "cc", "hpp", "rb", "php", "css", "scss", "md", "json", "yaml", "yml", "toml", "sql", "html", "vue", "svelte", "kt", "swift", "sh"];

pub fn analyze_query(text: &str) -> QueryAnalysis {
    let mut a = QueryAnalysis { raw: text.to_string(), ..Default::default() };
    let path_re = re(r#"(?:^|[\s`'"(])((?:[A-Za-z0-9_.\-]+/)+[A-Za-z0-9_.\-]+)"#, &PATH_RE);
    let file_re = re(r"\b([A-Za-z0-9_\-]+(?:\.[A-Za-z0-9_\-]+)*\.(?:[a-z]{1,5}))\b", &FILE_RE);
    let quote_re = re(r#""([^"]{2,120})"|'([^']{2,120})'|`([^`]{1,120})`"#, &QUOTE_RE);
    let ident_re = re(r"\b([A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*)\b", &IDENT_RE);
    let ext_re = re(r"(?i)\b([a-z]{1,5})\s+files?\b|\*?\.([a-z]{1,5})\b", &EXT_RE);
    let scope_re = re(r"(?i)\b(?:in|under|inside|within)\s+(?:the\s+)?`?((?:[A-Za-z0-9_.\-]+/)+[A-Za-z0-9_.\-]*)`?", &SCOPE_RE);

    let lower = text.to_lowercase();

    for cap in quote_re.captures_iter(text) {
        let q = cap.get(1).or_else(|| cap.get(2)).or_else(|| cap.get(3)).map(|m| m.as_str().trim().to_string());
        if let Some(q) = q {
            if !q.is_empty() && !a.quoted.contains(&q) {
                a.quoted.push(q);
            }
        }
    }
    for cap in path_re.captures_iter(text) {
        let p = cap[1].trim_end_matches(|c| c == '.' || c == ',' || c == ':' || c == ';').to_string();
        if p.contains('/') && !a.paths.contains(&p) {
            a.paths.push(p);
        }
    }
    for cap in scope_re.captures_iter(text) {
        let s = cap[1].trim_end_matches('/').to_string();
        if !s.is_empty() && !a.scope.contains(&s) {
            a.scope.push(s);
        }
    }
    for cap in file_re.captures_iter(text) {
        let f = cap[1].to_string();
        let ext = f.rsplit('.').next().unwrap_or("").to_lowercase();
        if KNOWN_EXTS.contains(&ext.as_str()) && !a.filenames.contains(&f) {
            a.filenames.push(f);
        }
    }
    for cap in ext_re.captures_iter(text) {
        let e = cap.get(1).or_else(|| cap.get(2)).map(|m| m.as_str().to_lowercase());
        if let Some(e) = e {
            if KNOWN_EXTS.contains(&e.as_str()) && !a.extensions.contains(&e) {
                a.extensions.push(e);
            }
        }
    }
    // Identifier-like tokens: camelCase, PascalCase with lowercase, snake_case,
    // or path-qualified (`a::b`). Plain lowercase words are keywords instead.
    // A capitalised word at the start of the text or of a sentence is
    // ordinary prose ("Make the footer sticky"), not a symbol, unless it
    // also carries an inner capital, underscore or digit.
    for cap in ident_re.captures_iter(text) {
        let m = cap.get(1).unwrap();
        let tok = m.as_str().to_string();
        let has_upper_inner = tok.chars().skip(1).any(|c| c.is_uppercase());
        let has_underscore = tok.contains('_') && tok.len() > 2;
        let has_digit = tok.chars().any(|c| c.is_ascii_digit());
        let qualified = tok.contains("::");
        let sentence_start = text[..m.start()].trim_end().is_empty() || text[..m.start()].trim_end().ends_with(|c: char| matches!(c, '.' | '!' | '?' | ':' | ';'));
        let pascal = tok.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) && tok.chars().any(|c| c.is_lowercase()) && tok.len() > 3 && !sentence_start;
        if (has_upper_inner || has_underscore || has_digit || qualified || pascal) && !a.symbols.contains(&tok) {
            a.symbols.push(tok);
        }
    }
    // Quoted identifiers count as symbols too.
    for q in &a.quoted {
        if !q.contains(' ') && q.chars().all(|c| c.is_alphanumeric() || c == '_' || c == ':' || c == '.') && !a.symbols.contains(q) {
            a.symbols.push(q.clone());
        }
    }

    // Keywords: content words plus identifier parts.
    let mut push_kw = |w: String| {
        if w.len() >= 2 && !STOPWORDS.contains(&w.as_str()) && !w.chars().all(|c| c.is_ascii_digit()) && !a.keywords.contains(&w) {
            a.keywords.push(w);
        }
    };
    for word in lower.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
        if word.is_empty() {
            continue;
        }
        push_kw(word.to_string());
        for part in split_identifier(word) {
            push_kw(part);
        }
    }
    for sym in a.symbols.clone() {
        for part in split_identifier(&sym) {
            push_kw(part);
        }
    }
    for f in a.filenames.clone() {
        if let Some(stem) = f.split('.').next() {
            for part in split_identifier(stem) {
                push_kw(part);
            }
        }
    }
    for q in a.quoted.clone() {
        for word in q.to_lowercase().split(|c: char| !(c.is_alphanumeric() || c == '_')) {
            push_kw(word.to_string());
        }
    }

    a.mentions_tests = lower.contains("test") || lower.contains("spec") || lower.contains("failing");
    a.mentions_docs = lower.contains("readme") || lower.contains("docs") || lower.contains("documentation") || lower.contains("changelog");
    a.refers_to_prior_work = ["earlier", "previous", "previously", "last time", "you did", "you changed", "you added", "before that", "the change you", "undo", "revert"].iter().any(|c| lower.contains(c));
    a.category = classify_category(&a, &lower);
    a
}

/// Number of distinct files the request names (a path and its own basename
/// count once).
pub fn distinct_file_mentions(a: &QueryAnalysis) -> usize {
    let mut names: Vec<String> = a.paths.iter().map(|p| p.rsplit('/').next().unwrap_or(p).to_lowercase()).collect();
    for f in &a.filenames {
        let lower = f.to_lowercase();
        if !names.contains(&lower) {
            names.push(lower);
        }
    }
    names.len()
}

/// Heuristic task-category classification used to size budgets (§7.11).
pub fn classify_category(a: &QueryAnalysis, lower: &str) -> TaskCategory {
    let words = lower.split_whitespace().count();
    let files = distinct_file_mentions(a);
    let investigation = ["why", "explain", "investigate", "find out", "figure out", "what causes", "debug", "diagnose", "where is", "where does", "how does", "root cause", "understand", "look into", "which file", "trace"]
        .iter()
        .any(|c| lower.contains(c));
    let broad = ["refactor", "across", "every", "all the", "all of", "throughout", "rename", "migrate", "consolidate", "extract", "split", "reorganize", "module", "architecture", "end-to-end", "end to end", "feature", "implement", "introduce", "support for", "wire up", "integrate"]
        .iter()
        .any(|c| lower.contains(c));
    let repair = ["fix", "failing", "broken", "bug", "error", "crash", "doesn't", "does not", "not working", "regression"].iter().any(|c| lower.contains(c));
    if investigation && !repair && !broad {
        return TaskCategory::Investigation;
    }
    let strong = ["refactor", "migrate", "feature", "implement", "architecture", "end-to-end", "end to end", "introduce", "support for", "integrate"].iter().any(|c| lower.contains(c));
    if broad && (strong || words > 18) {
        return TaskCategory::ComplexFeature;
    }
    if broad || files >= 2 || (repair && words > 12) {
        return TaskCategory::MultiFileChange;
    }
    if repair && a.filenames.is_empty() && a.symbols.is_empty() && a.quoted.is_empty() {
        return TaskCategory::Investigation;
    }
    TaskCategory::LocalizedEdit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_files_symbols_quotes_and_keywords() {
        let a = analyze_query("Add loading feedback to the LoginButton in src/components/LoginButton.tsx when the \"Sign in\" button is clicked; see useAuth and handle_login");
        assert_eq!(a.filenames, vec!["LoginButton.tsx"]);
        assert_eq!(a.paths, vec!["src/components/LoginButton.tsx"]);
        assert!(a.symbols.contains(&"LoginButton".to_string()));
        assert!(a.symbols.contains(&"useAuth".to_string()));
        assert!(a.symbols.contains(&"handle_login".to_string()));
        assert_eq!(a.quoted, vec!["Sign in"]);
        for k in ["loading", "feedback", "login", "button", "sign", "auth", "handle"] {
            assert!(a.keywords.contains(&k.to_string()), "missing keyword {k}: {:?}", a.keywords);
        }
        assert_eq!(a.category, TaskCategory::LocalizedEdit);
    }

    #[test]
    fn sentence_initial_words_are_not_symbols() {
        let a = analyze_query("Make the footer sticky at the bottom. Request a confirmation dialog");
        assert!(a.symbols.is_empty(), "{:?}", a.symbols);
        let a = analyze_query("Make LoginButton sticky");
        assert_eq!(a.symbols, vec!["LoginButton"]);
    }

    #[test]
    fn scope_and_category() {
        let a = analyze_query("Refactor the authentication module in src/auth to use async handlers across all services");
        assert_eq!(a.scope, vec!["src/auth"]);
        assert_eq!(a.category, TaskCategory::ComplexFeature);
        let a = analyze_query("Why do the pricing tests fail intermittently?");
        assert_eq!(a.category, TaskCategory::Investigation);
        assert!(a.mentions_tests);
        let a = analyze_query("Rename apply_discount to apply_percentage_discount in pricing.py and handlers.py");
        assert_eq!(a.category, TaskCategory::MultiFileChange);
        assert_eq!(a.filenames.len(), 2);
        let a = analyze_query("Make it darker");
        assert_eq!(a.category, TaskCategory::LocalizedEdit);
        assert!(a.keywords.contains(&"darker".to_string()));
        let a = analyze_query("Revert the change you made earlier to the footer");
        assert!(a.refers_to_prior_work);
    }
}
