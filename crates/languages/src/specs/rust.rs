//! Rust (reference implementation for the spec pattern).

use crate::treesitter::{ImportRule, LanguageSpec, NameSource, SymbolRule, TargetSource};
use crate::types::{LanguageId, RelationshipKind, SymbolKind};
use tree_sitter::Node;

static SYMBOLS: &[SymbolRule] = &[
    SymbolRule::new("function_item", SymbolKind::Function, NameSource::Field("name")),
    SymbolRule::new("function_signature_item", SymbolKind::Function, NameSource::Field("name")),
    SymbolRule::container("struct_item", SymbolKind::Struct, NameSource::Field("name")),
    SymbolRule::container("enum_item", SymbolKind::Enum, NameSource::Field("name")),
    SymbolRule::container("union_item", SymbolKind::Struct, NameSource::Field("name")),
    SymbolRule::container("trait_item", SymbolKind::Trait, NameSource::Field("name")),
    SymbolRule::container("impl_item", SymbolKind::Impl, NameSource::Custom),
    SymbolRule::container("mod_item", SymbolKind::Module, NameSource::Field("name")),
    SymbolRule::new("type_item", SymbolKind::TypeAlias, NameSource::Field("name")),
    SymbolRule::top_level("const_item", SymbolKind::Constant, NameSource::Field("name")),
    SymbolRule::top_level("static_item", SymbolKind::Constant, NameSource::Field("name")),
    SymbolRule::new("macro_definition", SymbolKind::Macro, NameSource::Field("name")),
];

static IMPORTS: &[ImportRule] = &[ImportRule { node_kind: "use_declaration", relationship: RelationshipKind::Import, target: TargetSource::Field("argument"), confidence: 0.9 }];

fn impl_name(node: Node<'_>, source: &str) -> Option<String> {
    if node.kind() != "impl_item" {
        return None;
    }
    let ty = node.child_by_field_name("type")?.utf8_text(source.as_bytes()).ok()?.to_string();
    let ty = ty.split('<').next().unwrap_or(&ty).trim().to_string();
    match node.child_by_field_name("trait") {
        Some(t) => {
            let tr = t.utf8_text(source.as_bytes()).ok()?.to_string();
            let tr = tr.split('<').next().unwrap_or(&tr).trim().to_string();
            Some(format!("{tr} for {ty}"))
        }
        None => Some(ty),
    }
}

pub fn spec() -> LanguageSpec {
    LanguageSpec {
        id: LanguageId::Rust,
        language: || tree_sitter_rust::LANGUAGE.into(),
        extensions: &["rs"],
        filenames: &[],
        symbols: SYMBOLS,
        imports: IMPORTS,
        calls: &[("call_expression", "function"), ("macro_invocation", "macro")],
        inheritance: &[],
        function_scopes: &["function_item", "closure_expression"],
        custom_name: Some(impl_name),
        custom_imports: None,
        custom_symbol: None,
        test_markers: &["/tests/", "/benches/"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::specs::testing::*;

    const SRC: &str = r#"
use std::collections::HashMap;
use crate::auth::{login, Session};

pub const MAX: usize = 10;

pub struct LoginButton {
    pub label: String,
}

impl LoginButton {
    pub fn new(label: &str) -> Self {
        let local = 1;
        Self { label: label.to_string() }
    }

    fn render(&self) -> String {
        format!("{}", self.label)
    }
}

impl Default for LoginButton {
    fn default() -> Self { Self::new("Log in") }
}

pub trait Clickable {
    fn click(&self);
}

pub enum State { Idle, Loading }

mod inner {
    pub fn helper() {}
}

fn main() {
    let b = LoginButton::new("x");
    b.render();
    helper_call();
    println!("hi");
}
"#;

    #[test]
    fn extracts_symbols_with_parents() {
        let a = analyze(spec(), SRC);
        assert!(a.errors.is_empty(), "{:?}", a.errors);
        assert!(has_symbol(&a, "LoginButton", SymbolKind::Struct));
        assert!(has_symbol(&a, "LoginButton", SymbolKind::Impl));
        assert!(has_symbol(&a, "Default for LoginButton", SymbolKind::Impl));
        assert!(has_symbol(&a, "new", SymbolKind::Function));
        assert_eq!(parent_of(&a, "new").as_deref(), Some("LoginButton"));
        assert_eq!(parent_of(&a, "render").as_deref(), Some("LoginButton"));
        assert_eq!(parent_of(&a, "click").as_deref(), Some("Clickable"));
        assert_eq!(parent_of(&a, "helper").as_deref(), Some("inner"));
        assert!(has_symbol(&a, "MAX", SymbolKind::Constant));
        assert!(has_symbol(&a, "State", SymbolKind::Enum));
        assert!(has_symbol(&a, "Clickable", SymbolKind::Trait));
        assert!(!names(&a).contains(&"local".to_string()));
        let main = a.symbols.iter().find(|s| s.name == "main").unwrap();
        assert!(main.signature.as_deref().unwrap().starts_with("fn main()"));
        assert_eq!(main.start_line, 36);
    }

    #[test]
    fn extracts_imports_and_calls() {
        let a = analyze(spec(), SRC);
        let imps = imports(&a);
        assert!(imps.contains(&"std::collections::HashMap".to_string()));
        assert!(imps.contains(&"crate::auth::{login, Session}".to_string()));
        let calls = calls(&a);
        assert!(calls.contains(&"new".to_string()));
        assert!(calls.contains(&"render".to_string()));
        assert!(calls.contains(&"helper_call".to_string()));
        assert!(calls.contains(&"println".to_string()));
        let call = a.relationships.iter().find(|r| r.target == "helper_call").unwrap();
        assert_eq!(call.from_symbol.as_deref(), Some("main"));
    }

    #[test]
    fn chunks_follow_symbols() {
        let a = analyze(spec(), SRC);
        let syms: Vec<Option<String>> = a.chunks.iter().map(|c| c.symbol.clone()).collect();
        assert!(syms.contains(&Some("LoginButton".into())));
        assert!(syms.contains(&Some("main".into())));
        assert_eq!(syms[0], None, "leading use block is a gap chunk");
        // Whitespace-only gaps between items are dropped; everything else is kept.
        let total: usize = a.chunks.iter().map(|c| c.content.len()).sum();
        assert!(total >= SRC.trim().len() * 9 / 10, "chunks cover {total} of {} bytes", SRC.trim().len());
    }
}
