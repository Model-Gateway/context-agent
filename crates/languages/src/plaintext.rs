//! Universal fallback adapter: line-based chunking, no symbols. Markdown
//! files get heading-based sections so documentation retrieval has
//! structure; stylesheets their class / id selectors, custom properties,
//! variables, mixins and keyframes, and HTML its `id`s, so the editor can
//! complete them across files.

use crate::chunking::{chunk_by_lines, chunk_by_symbols};
use crate::types::*;
use crate::LanguageAdapter;
use ctx_core::line_range;

pub struct PlainTextAdapter {
    id: LanguageId,
    extensions: Vec<&'static str>,
    filenames: Vec<&'static str>,
}

impl PlainTextAdapter {
    pub fn new(id: LanguageId, extensions: &[&'static str], filenames: &[&'static str]) -> Self {
        Self { id, extensions: extensions.to_vec(), filenames: filenames.to_vec() }
    }

    pub fn generic() -> Self {
        Self::new(LanguageId::PlainText, &["txt", "text", "rst", "adoc", "org", "cfg", "ini", "conf", "properties", "sql", "graphql", "proto"], &[])
    }

    /// One symbol per name, spanning the line it is first defined on.
    fn push_once(symbols: &mut Vec<Symbol>, source: &str, line_start: usize, line: &str, name: &str, kind: SymbolKind) {
        if name.is_empty() || symbols.iter().any(|s| s.name == name) {
            return;
        }
        let end = line_start + line.trim_end_matches(['\n', '\r']).len();
        let (start_line, end_line) = line_range(source, line_start, end);
        let signature = line.trim().chars().take(120).collect::<String>();
        symbols.push(Symbol { name: name.to_string(), kind, parent: None, start_byte: line_start, end_byte: end, start_line, end_line, signature: Some(signature) });
    }

    fn css_name(rest: &str) -> &str {
        let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_')).unwrap_or(rest.len());
        &rest[..end]
    }

    /// `.class` / `#id` in selectors (the text before a `{`, or a line ending
    /// in `,`), `--custom: …`, SCSS `$var:`, `@mixin` / `@function`, `@keyframes`.
    fn css_symbols(source: &str) -> Vec<Symbol> {
        let mut symbols = Vec::new();
        let mut in_comment = false;
        let mut offset = 0;
        for line in source.split_inclusive('\n') {
            let line_start = offset;
            offset += line.len();
            // Blank out comments, keeping byte positions.
            let mut text = String::with_capacity(line.len());
            let mut rest = line;
            while !rest.is_empty() {
                if in_comment {
                    match rest.find("*/") {
                        Some(i) => { text.push_str(&" ".repeat(i + 2)); rest = &rest[i + 2..]; in_comment = false; }
                        None => { text.push_str(&" ".repeat(rest.len())); rest = ""; }
                    }
                } else if let Some(i) = rest.find("/*") {
                    text.push_str(&rest[..i]);
                    rest = &rest[i..];
                    in_comment = true;
                } else {
                    text.push_str(rest);
                    rest = "";
                }
            }
            let trimmed = text.trim();
            if trimmed.is_empty() || trimmed.starts_with("//") {
                continue;
            }
            if let Some(at) = trimmed.strip_prefix('@') {
                let (word, name) = at.split_once(char::is_whitespace).map(|(w, r)| (w, Self::css_name(r.trim_start()))).unwrap_or((at, ""));
                match word {
                    "mixin" | "function" => Self::push_once(&mut symbols, source, line_start, line, name, SymbolKind::Function),
                    "keyframes" | "-webkit-keyframes" => Self::push_once(&mut symbols, source, line_start, line, name, SymbolKind::Other),
                    _ => {}
                }
                continue;
            }
            if let Some(var) = trimmed.strip_prefix('$') {
                let name = Self::css_name(var);
                if var[name.len()..].trim_start().starts_with(':') {
                    Self::push_once(&mut symbols, source, line_start, line, &format!("${name}"), SymbolKind::Variable);
                }
                continue;
            }
            let mut search = trimmed;
            while let Some(i) = search.find("--") {
                let name = Self::css_name(&search[i + 2..]);
                let after = &search[i + 2 + name.len()..];
                let starts_declaration = i == 0 || search[..i].ends_with([' ', ';', '{', '\t']);
                if !name.is_empty() && starts_declaration && after.trim_start().starts_with(':') {
                    Self::push_once(&mut symbols, source, line_start, line, &format!("--{name}"), SymbolKind::Variable);
                }
                search = &search[i + 2 + name.len()..];
            }
            let selector = match trimmed.find('{') {
                Some(i) => &trimmed[..i],
                None if trimmed.ends_with(',') => trimmed,
                None => continue,
            };
            let bytes = selector.as_bytes();
            for (i, &b) in bytes.iter().enumerate() {
                if b != b'.' && b != b'#' {
                    continue;
                }
                // `1.5em`, `a.b` inside an attribute value, `#{$interpolation}`.
                if i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'\\') {
                    continue;
                }
                let name = Self::css_name(&selector[i + 1..]);
                if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) || selector[..i].matches(['"', '\'']).count() % 2 == 1 {
                    continue;
                }
                let kind = if b == b'.' { SymbolKind::Class } else { SymbolKind::Constant };
                Self::push_once(&mut symbols, source, line_start, line, name, kind);
            }
        }
        symbols
    }

    /// `id="…"` attributes (`<section id="pricing">`).
    fn html_ids(source: &str) -> Vec<Symbol> {
        let mut symbols = Vec::new();
        let mut offset = 0;
        for line in source.split_inclusive('\n') {
            let lower = line.to_ascii_lowercase();
            let mut from = 0;
            while let Some(i) = lower[from..].find("id=") {
                let at = from + i;
                from = at + 3;
                if at > 0 && !lower.as_bytes()[at - 1].is_ascii_whitespace() {
                    continue;
                }
                let Some(quote) = line[from..].chars().next().filter(|c| *c == '"' || *c == '\'') else { continue };
                let value = &line[from + 1..];
                if let Some(end) = value.find(quote) {
                    let name = value[..end].trim();
                    if !name.contains(['{', '<', '$', ' ']) {
                        Self::push_once(&mut symbols, source, offset, line, name, SymbolKind::Constant);
                    }
                }
            }
            offset += line.len();
        }
        symbols
    }

    fn markdown_sections(source: &str) -> Vec<Symbol> {
        let mut symbols = Vec::new();
        let mut headings: Vec<(usize, usize, String)> = Vec::new(); // (start_byte, level, title)
        let mut in_fence = false;
        let mut offset = 0;
        for line in source.split_inclusive('\n') {
            let trimmed = line.trim_start();
            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                in_fence = !in_fence;
            } else if !in_fence && trimmed.starts_with('#') {
                let level = trimmed.chars().take_while(|c| *c == '#').count();
                if level <= 6 && trimmed.chars().nth(level).map(|c| c == ' ').unwrap_or(false) {
                    let title = trimmed[level..].trim().trim_end_matches('#').trim().to_string();
                    headings.push((offset, level, title));
                }
            }
            offset += line.len();
        }
        for (i, (start, level, title)) in headings.iter().enumerate() {
            let end = headings[i + 1..].iter().find(|(_, l, _)| l <= level).map(|(s, _, _)| *s).unwrap_or(source.len());
            let (start_line, end_line) = line_range(source, *start, end);
            symbols.push(Symbol { name: title.clone(), kind: SymbolKind::Section, parent: None, start_byte: *start, end_byte: end, start_line, end_line, signature: Some(format!("{} {}", "#".repeat(*level), title)) });
        }
        symbols
    }
}

impl LanguageAdapter for PlainTextAdapter {
    fn identifier(&self) -> LanguageId {
        self.id
    }

    fn extensions(&self) -> &[&str] {
        &self.extensions
    }

    fn filenames(&self) -> &[&str] {
        &self.filenames
    }

    fn support_level(&self) -> SupportLevel {
        SupportLevel::Universal
    }

    fn parse(&self, _source: &str) -> ParseResult {
        ParseResult { tree: SyntaxTree::none(), errors: Vec::new() }
    }

    fn extract_symbols(&self, source: &str, _tree: &SyntaxTree) -> Vec<Symbol> {
        match self.id {
            LanguageId::Markdown => Self::markdown_sections(source),
            LanguageId::Css => Self::css_symbols(source),
            LanguageId::Html => Self::html_ids(source),
            _ => Vec::new(),
        }
    }

    fn extract_relationships(&self, _source: &str, _tree: &SyntaxTree) -> Vec<Relationship> {
        Vec::new()
    }

    fn chunk(&self, source: &str, tree: &SyntaxTree, budget: ChunkBudget) -> Vec<CodeChunk> {
        if self.id == LanguageId::Markdown {
            let sections = self.extract_symbols(source, tree);
            // Nested headings overlap their parents; keep only the finest
            // granularity that fits the budget by treating all as top-level
            // but removing those that contain other headings.
            let leaves: Vec<Symbol> = sections
                .iter()
                .filter(|s| !sections.iter().any(|o| o.start_byte > s.start_byte && o.end_byte <= s.end_byte))
                .cloned()
                .collect();
            if !leaves.is_empty() {
                return chunk_by_symbols(source, &leaves, budget);
            }
        }
        chunk_by_lines(source, budget)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_headings_become_sections() {
        let adapter = PlainTextAdapter::new(LanguageId::Markdown, &["md"], &[]);
        let src = "# Title\nintro\n\n## Install\nsteps\n\n```\n# not a heading\n```\n\n## Usage\nrun it\n";
        let tree = adapter.parse(src).tree;
        let syms = adapter.extract_symbols(src, &tree);
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["Title", "Install", "Usage"]);
        let chunks = adapter.chunk(src, &tree, ChunkBudget::default());
        assert!(chunks.iter().any(|c| c.symbol.as_deref() == Some("Install") && c.content.contains("steps")));
    }

    #[test]
    fn stylesheets_define_selectors_and_variables() {
        let adapter = PlainTextAdapter::new(LanguageId::Css, &["css"], &[]);
        let src = ":root { --brand-color: #0af; }\n/* .commented { } */\n.btn-primary:hover, #main .card,\n.nav > a[href=\"x.y\"] { width: 1.5em; color: var(--brand-color); }\n$gap: 4px;\n@mixin stack($n) {}\n@keyframes fade-in { from { opacity: 0 } }\n";
        let tree = adapter.parse(src).tree;
        let names: Vec<String> = adapter.extract_symbols(src, &tree).into_iter().map(|s| s.name).collect();
        assert_eq!(names, vec!["--brand-color", "btn-primary", "main", "card", "nav", "$gap", "stack", "fade-in"]);
    }

    #[test]
    fn html_ids_become_symbols() {
        let adapter = PlainTextAdapter::new(LanguageId::Html, &["html"], &[]);
        let src = "<section id=\"pricing\" class=\"x\">\n<div data-id=\"no\"></div><a id='top'></a>\n";
        let tree = adapter.parse(src).tree;
        let syms = adapter.extract_symbols(src, &tree);
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["pricing", "top"]);
        assert_eq!(syms[1].start_line, 2);
    }
}
