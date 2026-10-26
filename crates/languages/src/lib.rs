//! Multi-language repository intelligence (spec §6).
//!
//! Three support levels:
//! * **Universal**: any text file is chunked by lines ([`PlainTextAdapter`]).
//! * **Parsed**: tree-sitter grammars give symbols, imports, and
//!   symbol-aware chunks ([`treesitter::TreeSitterAdapter`] configured by a
//!   [`LanguageSpec`]).
//! * **Semantic**: language servers, provided by the separate `ctx-lsp`
//!   crate so that parser and LSP functionality are independently available.
//!
//! The retrieval engine never contains language-specific logic; it works
//! through the [`LanguageAdapter`] trait and the [`LanguageRegistry`].

pub mod chunking;
pub mod plaintext;
pub mod registry;
pub mod specs;
pub mod tokens;
pub mod treesitter;
pub mod types;

pub use plaintext::PlainTextAdapter;
pub use registry::LanguageRegistry;
pub use tokens::{identifier_tokens, split_identifier};
pub use treesitter::{ImportRule, LanguageSpec, NameSource, SymbolRule, TargetSource, TreeSitterAdapter};
pub use types::*;

/// A language adapter (spec §6.3). Implementations must be `Send + Sync` so
/// the indexer can parse files on worker threads.
pub trait LanguageAdapter: Send + Sync {
    /// The language this adapter handles.
    fn identifier(&self) -> LanguageId;

    /// File extensions (without dot) and exact filenames this adapter claims.
    fn extensions(&self) -> &[&str];
    fn filenames(&self) -> &[&str] {
        &[]
    }

    /// Support level offered by this adapter.
    fn support_level(&self) -> SupportLevel;

    fn parse(&self, source: &str) -> ParseResult;

    fn extract_symbols(&self, source: &str, tree: &SyntaxTree) -> Vec<Symbol>;

    fn extract_relationships(&self, source: &str, tree: &SyntaxTree) -> Vec<Relationship>;

    fn chunk(&self, source: &str, tree: &SyntaxTree, budget: ChunkBudget) -> Vec<CodeChunk>;

    /// Heuristic: does this path look like a test file for this language?
    fn is_test_path(&self, path: &str) -> bool {
        let lower = path.to_ascii_lowercase();
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

    /// Convenience: parse and extract everything in one call.
    fn analyze(&self, source: &str, budget: ChunkBudget) -> FileAnalysis {
        let parsed = self.parse(source);
        let symbols = self.extract_symbols(source, &parsed.tree);
        let relationships = self.extract_relationships(source, &parsed.tree);
        let chunks = self.chunk(source, &parsed.tree, budget);
        FileAnalysis { language: self.identifier(), support: self.support_level(), symbols, relationships, chunks, parse_errors: parsed.errors }
    }
}
