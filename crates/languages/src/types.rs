//! Language-independent structural types.

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LanguageId {
    Rust,
    Python,
    TypeScript,
    Tsx,
    JavaScript,
    Go,
    Java,
    C,
    Cpp,
    Ruby,
    Php,
    Markdown,
    Json,
    Yaml,
    Toml,
    Shell,
    Html,
    Css,
    Dotenv,
    PlainText,
    Other,
}

impl LanguageId {
    pub fn as_str(&self) -> &'static str {
        match self {
            LanguageId::Rust => "rust",
            LanguageId::Python => "python",
            LanguageId::TypeScript => "typescript",
            LanguageId::Tsx => "tsx",
            LanguageId::JavaScript => "javascript",
            LanguageId::Go => "go",
            LanguageId::Java => "java",
            LanguageId::C => "c",
            LanguageId::Cpp => "cpp",
            LanguageId::Ruby => "ruby",
            LanguageId::Php => "php",
            LanguageId::Markdown => "markdown",
            LanguageId::Json => "json",
            LanguageId::Yaml => "yaml",
            LanguageId::Toml => "toml",
            LanguageId::Shell => "shell",
            LanguageId::Html => "html",
            LanguageId::Css => "css",
            LanguageId::Dotenv => "dotenv",
            LanguageId::PlainText => "text",
            LanguageId::Other => "other",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "rust" => LanguageId::Rust,
            "python" => LanguageId::Python,
            "typescript" => LanguageId::TypeScript,
            "tsx" => LanguageId::Tsx,
            "javascript" => LanguageId::JavaScript,
            "go" => LanguageId::Go,
            "java" => LanguageId::Java,
            "c" => LanguageId::C,
            "cpp" => LanguageId::Cpp,
            "ruby" => LanguageId::Ruby,
            "php" => LanguageId::Php,
            "markdown" => LanguageId::Markdown,
            "json" => LanguageId::Json,
            "yaml" => LanguageId::Yaml,
            "toml" => LanguageId::Toml,
            "shell" => LanguageId::Shell,
            "html" => LanguageId::Html,
            "css" => LanguageId::Css,
            "dotenv" => LanguageId::Dotenv,
            "text" => LanguageId::PlainText,
            "other" => LanguageId::Other,
            _ => return None,
        })
    }

    pub fn is_code(&self) -> bool {
        !matches!(self, LanguageId::Markdown | LanguageId::Json | LanguageId::Yaml | LanguageId::Toml | LanguageId::Dotenv | LanguageId::PlainText | LanguageId::Other)
    }
}

impl fmt::Display for LanguageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportLevel {
    Universal,
    Parsed,
    Semantic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Struct,
    Enum,
    Interface,
    Trait,
    Impl,
    Module,
    Namespace,
    TypeAlias,
    Constant,
    Variable,
    Field,
    Component,
    Macro,
    Section,
    Other,
}

impl SymbolKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SymbolKind::Function => "function",
            SymbolKind::Method => "method",
            SymbolKind::Class => "class",
            SymbolKind::Struct => "struct",
            SymbolKind::Enum => "enum",
            SymbolKind::Interface => "interface",
            SymbolKind::Trait => "trait",
            SymbolKind::Impl => "impl",
            SymbolKind::Module => "module",
            SymbolKind::Namespace => "namespace",
            SymbolKind::TypeAlias => "type_alias",
            SymbolKind::Constant => "constant",
            SymbolKind::Variable => "variable",
            SymbolKind::Field => "field",
            SymbolKind::Component => "component",
            SymbolKind::Macro => "macro",
            SymbolKind::Section => "section",
            SymbolKind::Other => "other",
        }
    }

    /// Whether symbols of this kind can contain other symbols.
    pub fn is_container(&self) -> bool {
        matches!(self, SymbolKind::Class | SymbolKind::Struct | SymbolKind::Enum | SymbolKind::Interface | SymbolKind::Trait | SymbolKind::Impl | SymbolKind::Module | SymbolKind::Namespace)
    }
}

/// A symbol definition with its byte range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    /// Enclosing symbol name, if nested (e.g. a method's class).
    pub parent: Option<String>,
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: usize,
    pub end_line: usize,
    /// First line of the definition, trimmed (used as a signature preview).
    pub signature: Option<String>,
}

impl Symbol {
    pub fn qualified_name(&self) -> String {
        match &self.parent {
            Some(p) => format!("{p}.{}", self.name),
            None => self.name.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipKind {
    Import,
    Export,
    Include,
    Call,
    Reference,
    Extends,
    Implements,
    TestOf,
    StyleOf,
    ConfigOf,
    Module,
}

impl RelationshipKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            RelationshipKind::Import => "import",
            RelationshipKind::Export => "export",
            RelationshipKind::Include => "include",
            RelationshipKind::Call => "call",
            RelationshipKind::Reference => "reference",
            RelationshipKind::Extends => "extends",
            RelationshipKind::Implements => "implements",
            RelationshipKind::TestOf => "test_of",
            RelationshipKind::StyleOf => "style_of",
            RelationshipKind::ConfigOf => "config_of",
            RelationshipKind::Module => "module",
        }
    }
}

/// A relationship from the analysed file to something else. `target` is
/// the raw text (module specifier, symbol name); path resolution happens in
/// the indexer, which records confidence (spec §6.6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Relationship {
    pub kind: RelationshipKind,
    /// Symbol in this file that owns the relationship, if any.
    pub from_symbol: Option<String>,
    pub target: String,
    /// Symbol being referenced, when known.
    pub to_symbol: Option<String>,
    /// 0.0–1.0; parser-derived imports are high, heuristic calls lower.
    pub confidence: f64,
    pub start_byte: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkBudget {
    /// Maximum bytes per chunk before subdivision.
    pub max_bytes: usize,
    /// Minimum bytes for a standalone gap chunk; smaller gaps merge.
    pub min_bytes: usize,
}

impl Default for ChunkBudget {
    fn default() -> Self {
        Self { max_bytes: 6_000, min_bytes: 200 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeChunk {
    pub symbol: Option<String>,
    pub symbol_kind: Option<SymbolKind>,
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: usize,
    pub end_line: usize,
    pub content: String,
    /// Index within a subdivided symbol (0 for whole-symbol chunks).
    pub part: usize,
}

/// Owned syntax tree. Plain-text adapters produce `None`.
pub struct SyntaxTree {
    pub inner: Option<tree_sitter::Tree>,
}

impl SyntaxTree {
    pub fn none() -> Self {
        Self { inner: None }
    }

    pub fn root(&self) -> Option<tree_sitter::Node<'_>> {
        self.inner.as_ref().map(|t| t.root_node())
    }
}

impl std::fmt::Debug for SyntaxTree {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.inner {
            Some(t) => write!(f, "SyntaxTree({})", t.root_node().kind()),
            None => write!(f, "SyntaxTree(None)"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseError {
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: usize,
    pub message: String,
}

#[derive(Debug)]
pub struct ParseResult {
    pub tree: SyntaxTree,
    pub errors: Vec<ParseError>,
}

#[derive(Debug)]
pub struct FileAnalysis {
    pub language: LanguageId,
    pub support: SupportLevel,
    pub symbols: Vec<Symbol>,
    pub relationships: Vec<Relationship>,
    pub chunks: Vec<CodeChunk>,
    pub parse_errors: Vec<ParseError>,
}
