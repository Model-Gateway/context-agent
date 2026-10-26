//! Generic tree-sitter adapter configured by a declarative [`LanguageSpec`].
//!
//! Per-language knowledge (node kinds, field names) lives in
//! [`crate::specs`]; this module contains only the generic walk, so adding a
//! language never touches the retrieval engine.

use crate::chunking::chunk_by_symbols;
use crate::types::*;
use crate::LanguageAdapter;
use tree_sitter::{Language, Node, Parser};

/// How to find a symbol's name inside its node.
#[derive(Debug, Clone, Copy)]
pub enum NameSource {
    /// `node.child_by_field_name(field)`.
    Field(&'static str),
    /// First direct child of the given kind.
    ChildKind(&'static str),
    /// Field, then the first descendant of the given kind within it (e.g. C
    /// declarators: `declarator` → `identifier`).
    FieldDescendant(&'static str, &'static str),
    /// Delegate to `LanguageSpec::custom_name`.
    Custom,
}

#[derive(Debug, Clone, Copy)]
pub struct SymbolRule {
    pub node_kind: &'static str,
    pub symbol_kind: SymbolKind,
    pub name: NameSource,
    /// Nested symbols record this symbol as their parent.
    pub container: bool,
    /// Only emit when not inside a function body (avoids local variables).
    pub top_level_only: bool,
}

impl SymbolRule {
    pub const fn new(node_kind: &'static str, symbol_kind: SymbolKind, name: NameSource) -> Self {
        Self { node_kind, symbol_kind, name, container: false, top_level_only: false }
    }

    pub const fn container(node_kind: &'static str, symbol_kind: SymbolKind, name: NameSource) -> Self {
        Self { node_kind, symbol_kind, name, container: true, top_level_only: false }
    }

    pub const fn top_level(node_kind: &'static str, symbol_kind: SymbolKind, name: NameSource) -> Self {
        Self { node_kind, symbol_kind, name, container: false, top_level_only: true }
    }
}

/// Where an import rule finds its module specifier.
#[derive(Debug, Clone, Copy)]
pub enum TargetSource {
    Field(&'static str),
    ChildKind(&'static str),
    /// First descendant (depth-first) of the given kind.
    DescendantKind(&'static str),
    /// Delegate to `LanguageSpec::custom_imports`.
    Custom,
}

#[derive(Debug, Clone, Copy)]
pub struct ImportRule {
    pub node_kind: &'static str,
    pub relationship: RelationshipKind,
    pub target: TargetSource,
    pub confidence: f64,
}

pub type CustomNameFn = fn(Node<'_>, &str) -> Option<String>;
pub type CustomImportsFn = fn(Node<'_>, &str) -> Vec<Relationship>;
pub type CustomSymbolFn = fn(Node<'_>, &str) -> Option<(String, SymbolKind)>;

/// Declarative description of a tree-sitter grammar for the generic adapter.
pub struct LanguageSpec {
    pub id: LanguageId,
    pub language: fn() -> Language,
    pub extensions: &'static [&'static str],
    pub filenames: &'static [&'static str],
    pub symbols: &'static [SymbolRule],
    pub imports: &'static [ImportRule],
    /// `(call node kind, callee field name)`.
    pub calls: &'static [(&'static str, &'static str)],
    /// `(node kind, field holding the base type(s), relationship kind)`.
    pub inheritance: &'static [(&'static str, &'static str, RelationshipKind)],
    /// Node kinds whose bodies are function scopes (local declarations
    /// inside them are skipped for `top_level_only` rules).
    pub function_scopes: &'static [&'static str],
    pub custom_name: Option<CustomNameFn>,
    pub custom_imports: Option<CustomImportsFn>,
    /// Called for every node before generic rules; may emit a symbol.
    pub custom_symbol: Option<CustomSymbolFn>,
    /// Extra extensions that mark a file as a test for this language.
    pub test_markers: &'static [&'static str],
}

pub struct TreeSitterAdapter {
    spec: LanguageSpec,
    language: Language,
}

impl TreeSitterAdapter {
    pub fn new(spec: LanguageSpec) -> Self {
        let language = (spec.language)();
        Self { spec, language }
    }

    pub fn spec(&self) -> &LanguageSpec {
        &self.spec
    }

    fn parser(&self) -> Option<Parser> {
        let mut parser = Parser::new();
        parser.set_language(&self.language).ok()?;
        Some(parser)
    }

    fn symbol_rule(&self, kind: &str) -> Option<&SymbolRule> {
        self.spec.symbols.iter().find(|r| r.node_kind == kind)
    }

    fn resolve_name(&self, node: Node<'_>, rule: &SymbolRule, source: &str) -> Option<String> {
        match rule.name {
            NameSource::Field(field) => node.child_by_field_name(field).and_then(|n| text(n, source)),
            NameSource::ChildKind(kind) => first_child_of_kind(node, kind).and_then(|n| text(n, source)),
            NameSource::FieldDescendant(field, kind) => node.child_by_field_name(field).and_then(|n| first_descendant_of_kind(n, kind)).and_then(|n| text(n, source)),
            NameSource::Custom => self.spec.custom_name.and_then(|f| f(node, source)),
        }
    }

    fn walk_symbols(&self, node: Node<'_>, source: &str, parent: Option<&str>, in_function: bool, out: &mut Vec<Symbol>) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if !child.is_named() {
                continue;
            }
            let mut emitted: Option<(String, SymbolKind, bool)> = None;
            if let Some(custom) = self.spec.custom_symbol {
                if let Some((name, kind)) = custom(child, source) {
                    emitted = Some((name, kind, kind.is_container()));
                }
            }
            if emitted.is_none() {
                if let Some(rule) = self.symbol_rule(child.kind()) {
                    if !(rule.top_level_only && in_function) {
                        if let Some(name) = self.resolve_name(child, rule, source) {
                            emitted = Some((name, rule.symbol_kind, rule.container));
                        }
                    }
                }
            }
            let child_is_fn = self.spec.function_scopes.contains(&child.kind());
            match emitted {
                Some((name, kind, container)) => {
                    out.push(make_symbol(child, source, &name, kind, parent));
                    let next_parent = if container { Some(name.as_str()) } else { parent };
                    let next_in_fn = in_function || child_is_fn || matches!(kind, SymbolKind::Function | SymbolKind::Method);
                    self.walk_symbols(child, source, next_parent, next_in_fn, out);
                }
                None => {
                    self.walk_symbols(child, source, parent, in_function || child_is_fn, out);
                }
            }
        }
    }

    fn enclosing_symbol<'a>(symbols: &'a [Symbol], byte: usize) -> Option<&'a Symbol> {
        symbols.iter().filter(|s| s.start_byte <= byte && byte < s.end_byte).min_by_key(|s| s.end_byte - s.start_byte)
    }

    fn walk_relationships(&self, node: Node<'_>, source: &str, symbols: &[Symbol], out: &mut Vec<Relationship>) {
        let kind = node.kind();
        // Imports.
        for rule in self.spec.imports.iter().filter(|r| r.node_kind == kind) {
            let targets: Vec<String> = match rule.target {
                TargetSource::Field(f) => node.child_by_field_name(f).and_then(|n| text(n, source)).into_iter().collect(),
                TargetSource::ChildKind(k) => first_child_of_kind(node, k).and_then(|n| text(n, source)).into_iter().collect(),
                TargetSource::DescendantKind(k) => first_descendant_of_kind(node, k).and_then(|n| text(n, source)).into_iter().collect(),
                TargetSource::Custom => {
                    if let Some(f) = self.spec.custom_imports {
                        out.extend(f(node, source));
                    }
                    Vec::new()
                }
            };
            for t in targets {
                let target = strip_quotes(&t);
                if !target.is_empty() {
                    out.push(Relationship { kind: rule.relationship, from_symbol: None, target, to_symbol: None, confidence: rule.confidence, start_byte: node.start_byte() });
                }
            }
        }
        // Inheritance.
        for (_, field, rel) in self.spec.inheritance.iter().filter(|(nk, _, _)| *nk == kind) {
            if let Some(base) = node.child_by_field_name(field) {
                let from = Self::enclosing_symbol(symbols, node.start_byte()).map(|s| s.name.clone()).or_else(|| {
                    // The node itself may be the class declaration.
                    symbols.iter().find(|s| s.start_byte == node.start_byte()).map(|s| s.name.clone())
                });
                for name in split_type_list(&text(base, source).unwrap_or_default()) {
                    out.push(Relationship { kind: *rel, from_symbol: from.clone(), target: name.clone(), to_symbol: Some(name), confidence: 0.8, start_byte: node.start_byte() });
                }
            }
        }
        // Calls / references.
        for (ck, field) in self.spec.calls.iter().filter(|(ck, _)| *ck == kind) {
            if let Some(callee) = node.child_by_field_name(field) {
                if let Some(t) = text(callee, source) {
                    let name = last_identifier(&t);
                    if !name.is_empty() && name.len() > 1 {
                        let from = Self::enclosing_symbol(symbols, node.start_byte()).map(|s| s.name.clone());
                        out.push(Relationship { kind: RelationshipKind::Call, from_symbol: from, target: name.clone(), to_symbol: Some(name), confidence: 0.4, start_byte: node.start_byte() });
                    }
                }
            }
            let _ = ck;
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_relationships(child, source, symbols, out);
        }
    }
}

fn text(node: Node<'_>, source: &str) -> Option<String> {
    node.utf8_text(source.as_bytes()).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

pub fn first_child_of_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    let found = node.children(&mut cursor).find(|c| c.kind() == kind);
    found
}

pub fn first_descendant_of_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    if node.kind() == kind {
        return Some(node);
    }
    let mut cursor = node.walk();
    let children: Vec<Node<'a>> = node.children(&mut cursor).collect();
    for child in children {
        if let Some(found) = first_descendant_of_kind(child, kind) {
            return Some(found);
        }
    }
    None
}

pub fn strip_quotes(s: &str) -> String {
    let t = s.trim();
    let t = t.trim_start_matches(|c| c == '"' || c == '\'' || c == '`' || c == '<');
    let t = t.trim_end_matches(|c| c == '"' || c == '\'' || c == '`' || c == '>' || c == ';');
    t.to_string()
}

/// `foo.bar` → `bar`, `Foo::bar` → `bar`, `obj->method` → `method`,
/// `a.b<T>` → `b`.
pub fn last_identifier(callee: &str) -> String {
    let base = callee.split(|c| c == '<' || c == '(' || c == '!').next().unwrap_or(callee);
    base.rsplit(|c| c == '.' || c == ':' || c == '>' || c == '-' || c == '/' || c == '\\').next().unwrap_or(base).trim().to_string()
}

fn split_type_list(text: &str) -> Vec<String> {
    text.split(|c| c == ',' || c == '&' || c == '+' || c == ' ')
        .map(|s| s.trim().trim_start_matches("implements").trim_start_matches("extends").trim())
        .map(|s| s.split('<').next().unwrap_or(s).trim().to_string())
        .filter(|s| !s.is_empty() && s.chars().next().map(|c| c.is_alphabetic() || c == '_').unwrap_or(false))
        .collect()
}

fn make_symbol(node: Node<'_>, source: &str, name: &str, kind: SymbolKind, parent: Option<&str>) -> Symbol {
    let start_byte = node.start_byte();
    let end_byte = node.end_byte().min(source.len());
    let first_line = source[start_byte..end_byte].lines().next().unwrap_or("").trim();
    let signature = if first_line.is_empty() { None } else { Some(first_line.chars().take(160).collect()) };
    Symbol {
        name: name.to_string(),
        kind,
        parent: parent.map(|p| p.to_string()),
        start_byte,
        end_byte,
        start_line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        signature,
    }
}

fn collect_errors(node: Node<'_>, source: &str, out: &mut Vec<ParseError>) {
    if out.len() >= 20 {
        return;
    }
    if node.is_error() || node.is_missing() {
        out.push(ParseError {
            start_byte: node.start_byte(),
            end_byte: node.end_byte(),
            start_line: node.start_position().row + 1,
            message: if node.is_missing() { format!("missing {}", node.kind()) } else { format!("syntax error near `{}`", source[node.start_byte()..node.end_byte().min(source.len())].chars().take(40).collect::<String>()) },
        });
        return;
    }
    if !node.has_error() {
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_errors(child, source, out);
    }
}

impl LanguageAdapter for TreeSitterAdapter {
    fn identifier(&self) -> LanguageId {
        self.spec.id
    }

    fn extensions(&self) -> &[&str] {
        self.spec.extensions
    }

    fn filenames(&self) -> &[&str] {
        self.spec.filenames
    }

    fn support_level(&self) -> SupportLevel {
        SupportLevel::Parsed
    }

    fn parse(&self, source: &str) -> ParseResult {
        let Some(mut parser) = self.parser() else {
            return ParseResult { tree: SyntaxTree::none(), errors: vec![ParseError { start_byte: 0, end_byte: 0, start_line: 1, message: "parser unavailable".into() }] };
        };
        match parser.parse(source, None) {
            Some(tree) => {
                let mut errors = Vec::new();
                collect_errors(tree.root_node(), source, &mut errors);
                ParseResult { tree: SyntaxTree { inner: Some(tree) }, errors }
            }
            None => ParseResult { tree: SyntaxTree::none(), errors: vec![ParseError { start_byte: 0, end_byte: 0, start_line: 1, message: "parse returned no tree".into() }] },
        }
    }

    fn extract_symbols(&self, source: &str, tree: &SyntaxTree) -> Vec<Symbol> {
        let Some(root) = tree.root() else { return Vec::new() };
        let mut out = Vec::new();
        self.walk_symbols(root, source, None, false, &mut out);
        out
    }

    fn extract_relationships(&self, source: &str, tree: &SyntaxTree) -> Vec<Relationship> {
        let Some(root) = tree.root() else { return Vec::new() };
        let symbols = self.extract_symbols(source, tree);
        let mut out = Vec::new();
        self.walk_relationships(root, source, &symbols, &mut out);
        // Deduplicate call references per (from, target).
        let mut seen = std::collections::HashSet::new();
        out.retain(|r| r.kind != RelationshipKind::Call || seen.insert((r.from_symbol.clone(), r.target.clone())));
        out
    }

    fn chunk(&self, source: &str, tree: &SyntaxTree, budget: ChunkBudget) -> Vec<CodeChunk> {
        let symbols = self.extract_symbols(source, tree);
        if symbols.is_empty() {
            return crate::chunking::chunk_by_lines(source, budget);
        }
        chunk_by_symbols(source, &symbols, budget)
    }

    fn is_test_path(&self, path: &str) -> bool {
        let lower = path.to_ascii_lowercase();
        if self.spec.test_markers.iter().any(|m| lower.contains(m)) {
            return true;
        }
        lower.contains("/test/")
            || lower.contains("/tests/")
            || lower.contains("/__tests__/")
            || lower.contains("/spec/")
            || lower.contains(".test.")
            || lower.contains(".spec.")
            || lower.contains("_test.")
            || lower.starts_with("test_")
            || lower.contains("/test_")
    }
}
