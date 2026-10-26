//! Per-language tree-sitter specifications. Each module exposes
//! `pub fn spec() -> LanguageSpec` and its own tests against sample source.

pub mod c;
pub mod cpp;
pub mod go;
pub mod java;
pub mod javascript;
pub mod php;
pub mod python;
pub mod ruby;
pub mod rust;
pub mod typescript;

use crate::treesitter::LanguageSpec;

/// Every built-in language spec, in registration priority order.
pub fn all_specs() -> Vec<LanguageSpec> {
    vec![
        rust::spec(),
        python::spec(),
        typescript::spec(),
        typescript::tsx_spec(),
        javascript::spec(),
        go::spec(),
        java::spec(),
        c::spec(),
        cpp::spec(),
        ruby::spec(),
        php::spec(),
    ]
}

#[cfg(test)]
pub(crate) mod testing {
    use crate::treesitter::{LanguageSpec, TreeSitterAdapter};
    use crate::types::*;
    use crate::LanguageAdapter;

    pub struct Analysis {
        pub symbols: Vec<Symbol>,
        pub relationships: Vec<Relationship>,
        pub chunks: Vec<CodeChunk>,
        pub errors: Vec<ParseError>,
    }

    pub fn analyze(spec: LanguageSpec, source: &str) -> Analysis {
        let adapter = TreeSitterAdapter::new(spec);
        let parsed = adapter.parse(source);
        let symbols = adapter.extract_symbols(source, &parsed.tree);
        let relationships = adapter.extract_relationships(source, &parsed.tree);
        let chunks = adapter.chunk(source, &parsed.tree, ChunkBudget::default());
        Analysis { symbols, relationships, chunks, errors: parsed.errors }
    }

    pub fn names(a: &Analysis) -> Vec<String> {
        a.symbols.iter().map(|s| s.name.clone()).collect()
    }

    pub fn has_symbol(a: &Analysis, name: &str, kind: SymbolKind) -> bool {
        a.symbols.iter().any(|s| s.name == name && s.kind == kind)
    }

    pub fn parent_of(a: &Analysis, name: &str) -> Option<String> {
        a.symbols.iter().find(|s| s.name == name).and_then(|s| s.parent.clone())
    }

    pub fn imports(a: &Analysis) -> Vec<String> {
        a.relationships.iter().filter(|r| matches!(r.kind, RelationshipKind::Import | RelationshipKind::Include)).map(|r| r.target.clone()).collect()
    }

    pub fn calls(a: &Analysis) -> Vec<String> {
        a.relationships.iter().filter(|r| r.kind == RelationshipKind::Call).map(|r| r.target.clone()).collect()
    }
}

#[cfg(test)]
mod registry_smoke {
    use super::all_specs;
    use crate::treesitter::TreeSitterAdapter;
    use crate::types::LanguageId;
    use crate::LanguageAdapter;

    /// A one-line, syntactically valid snippet for every registered language.
    fn snippet(id: LanguageId) -> &'static str {
        match id {
            LanguageId::Rust => "fn main() {}",
            LanguageId::Python => "x = 1",
            LanguageId::TypeScript => "const x: number = 1;",
            LanguageId::Tsx => "const x = <div />;",
            LanguageId::JavaScript => "const x = 1;",
            LanguageId::Go => "package main",
            LanguageId::Java => "class A {}",
            LanguageId::C => "int x;",
            LanguageId::Cpp => "int x;",
            LanguageId::Ruby => "x = 1",
            LanguageId::Php => "<?php $x = 1;",
            other => panic!("no smoke snippet for {other:?}; add one when registering a new spec"),
        }
    }

    #[test]
    fn registers_every_language_in_order() {
        let ids: Vec<LanguageId> = all_specs().iter().map(|s| s.id).collect();
        assert_eq!(
            ids,
            vec![
                LanguageId::Rust,
                LanguageId::Python,
                LanguageId::TypeScript,
                LanguageId::Tsx,
                LanguageId::JavaScript,
                LanguageId::Go,
                LanguageId::Java,
                LanguageId::C,
                LanguageId::Cpp,
                LanguageId::Ruby,
                LanguageId::Php,
            ]
        );
    }

    #[test]
    fn every_spec_parses_a_trivial_snippet() {
        for spec in all_specs() {
            let id = spec.id;
            let source = snippet(id);
            let adapter = TreeSitterAdapter::new(spec);
            assert_eq!(adapter.identifier(), id);
            let parsed = adapter.parse(source);
            assert!(parsed.tree.root().is_some(), "{id:?}: parse returned no tree for {source:?}");
            assert!(parsed.errors.is_empty(), "{id:?}: unexpected parse errors for {source:?}: {:?}", parsed.errors);
        }
    }
}
