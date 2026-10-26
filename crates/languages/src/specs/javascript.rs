//! JavaScript (ES modules, CommonJS and JSX; `.js`, `.mjs`, `.cjs`, `.jsx`).

use crate::treesitter::{last_identifier, strip_quotes, ImportRule, LanguageSpec, NameSource, SymbolRule, TargetSource};
use crate::types::{LanguageId, Relationship, RelationshipKind, SymbolKind};
use tree_sitter::Node;

static SYMBOLS: &[SymbolRule] = &[
    SymbolRule::new("function_declaration", SymbolKind::Function, NameSource::Field("name")),
    SymbolRule::new("generator_function_declaration", SymbolKind::Function, NameSource::Field("name")),
    SymbolRule::container("class_declaration", SymbolKind::Class, NameSource::Field("name")),
    SymbolRule::new("method_definition", SymbolKind::Method, NameSource::Field("name")),
    // Arrow-valued fields are promoted to `Method` by `js_symbol` first.
    SymbolRule::new("field_definition", SymbolKind::Field, NameSource::Field("property")),
];

static IMPORTS: &[ImportRule] = &[
    ImportRule { node_kind: "import_statement", relationship: RelationshipKind::Import, target: TargetSource::Field("source"), confidence: 0.9 },
    // Only `export ... from '<source>'` has a `source` field; plain exports yield nothing.
    ImportRule { node_kind: "export_statement", relationship: RelationshipKind::Export, target: TargetSource::Field("source"), confidence: 0.9 },
    // `require('x')` and dynamic `import('x')`.
    ImportRule { node_kind: "call_expression", relationship: RelationshipKind::Import, target: TargetSource::Custom, confidence: 0.9 },
    // `class_heritage` is an unnamed child of the class node (no field), so the
    // generic inheritance table cannot express it.
    ImportRule { node_kind: "class_heritage", relationship: RelationshipKind::Extends, target: TargetSource::Custom, confidence: 0.8 },
];

static FUNCTION_SCOPES: &[&str] = &["function_declaration", "arrow_function", "function_expression", "generator_function", "generator_function_declaration", "method_definition", "class_static_block"];

fn node_text(node: Node<'_>, source: &str) -> Option<String> {
    node.utf8_text(source.as_bytes()).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn is_pascal_case(name: &str) -> bool {
    matches!(name.chars().next(), Some(c) if c.is_ascii_uppercase()) && name.chars().any(|c| c.is_ascii_lowercase())
}

fn inside_function(node: Node<'_>) -> bool {
    let mut cur = node.parent();
    while let Some(n) = cur {
        if FUNCTION_SCOPES.contains(&n.kind()) {
            return true;
        }
        cur = n.parent();
    }
    false
}

/// The sole `variable_declarator` of a declaration statement, if it has exactly one.
fn single_declarator(stmt: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = stmt.walk();
    let mut decls = stmt.named_children(&mut cursor).filter(|c| c.kind() == "variable_declarator");
    let first = decls.next()?;
    if decls.next().is_some() {
        None
    } else {
        Some(first)
    }
}

fn declarator_symbol(decl: Node<'_>, source: &str) -> Option<(String, SymbolKind)> {
    let name_node = decl.child_by_field_name("name")?;
    if name_node.kind() != "identifier" {
        return None;
    }
    let name = node_text(name_node, source)?;
    match decl.child_by_field_name("value").map(|v| v.kind()) {
        Some("arrow_function") => {
            let kind = if is_pascal_case(&name) { SymbolKind::Component } else { SymbolKind::Function };
            Some((name, kind))
        }
        Some("function_expression") | Some("generator_function") => Some((name, SymbolKind::Function)),
        Some("class") => Some((name, SymbolKind::Class)),
        _ => {
            // Plain bindings are symbols only at module level.
            if inside_function(decl) {
                return None;
            }
            let stmt = decl.parent()?;
            let kind = match stmt.kind() {
                "lexical_declaration" if stmt.child_by_field_name("kind").map(|k| k.kind()) == Some("const") => SymbolKind::Constant,
                "lexical_declaration" | "variable_declaration" => SymbolKind::Variable,
                _ => return None,
            };
            Some((name, kind))
        }
    }
}

fn js_symbol(node: Node<'_>, source: &str) -> Option<(String, SymbolKind)> {
    match node.kind() {
        // A single-declarator statement is emitted on the statement itself so
        // the symbol spans `const name = ...;` rather than just `name = ...`.
        "lexical_declaration" | "variable_declaration" => declarator_symbol(single_declarator(node)?, source),
        // Multi-declarator statements (`const a = 1, b = 2;`) fall back to one
        // symbol per declarator. `for (const x of xs)` declarators are skipped.
        "variable_declarator" => {
            let stmt = node.parent()?;
            if !matches!(stmt.kind(), "lexical_declaration" | "variable_declaration") || single_declarator(stmt).is_some() {
                return None;
            }
            declarator_symbol(node, source)
        }
        "field_definition" => {
            let value = node.child_by_field_name("value")?;
            if !matches!(value.kind(), "arrow_function" | "function_expression" | "generator_function") {
                return None;
            }
            let name = node_text(node.child_by_field_name("property")?, source)?;
            Some((name, SymbolKind::Method))
        }
        _ => None,
    }
}

fn js_imports(node: Node<'_>, source: &str) -> Vec<Relationship> {
    match node.kind() {
        "call_expression" => {
            let Some(callee) = node.child_by_field_name("function") else { return Vec::new() };
            let confidence = match callee.kind() {
                "import" => 0.8,
                "identifier" if node_text(callee, source).as_deref() == Some("require") => 0.9,
                _ => return Vec::new(),
            };
            let Some(arg) = node.child_by_field_name("arguments").and_then(|a| a.named_child(0)) else { return Vec::new() };
            if arg.kind() != "string" {
                return Vec::new();
            }
            let target = strip_quotes(&node_text(arg, source).unwrap_or_default());
            if target.is_empty() {
                return Vec::new();
            }
            vec![Relationship { kind: RelationshipKind::Import, from_symbol: None, target, to_symbol: None, confidence, start_byte: node.start_byte() }]
        }
        "class_heritage" => {
            let Some(base) = node.named_child(0) else { return Vec::new() };
            let name = last_identifier(&node_text(base, source).unwrap_or_default());
            if name.is_empty() {
                return Vec::new();
            }
            let from = node.parent().and_then(|c| c.child_by_field_name("name")).and_then(|n| node_text(n, source));
            vec![Relationship { kind: RelationshipKind::Extends, from_symbol: from, target: name.clone(), to_symbol: Some(name), confidence: 0.8, start_byte: node.start_byte() }]
        }
        _ => Vec::new(),
    }
}

pub fn spec() -> LanguageSpec {
    LanguageSpec {
        id: LanguageId::JavaScript,
        language: || tree_sitter_javascript::LANGUAGE.into(),
        extensions: &["js", "mjs", "cjs", "jsx"],
        filenames: &[],
        symbols: SYMBOLS,
        imports: IMPORTS,
        calls: &[("call_expression", "function"), ("new_expression", "constructor")],
        inheritance: &[],
        function_scopes: FUNCTION_SCOPES,
        custom_name: None,
        custom_imports: Some(js_imports),
        custom_symbol: Some(js_symbol),
        test_markers: &[".test.", ".spec.", "/__tests__/"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::specs::testing::*;

    const SRC: &str = r#"
import React, { useState } from 'react';
import * as utils from './utils';
import './styles.css';
const fs = require('fs');
export { helper } from './helper';
export * from './all';

export const MAX_RETRIES = 3;
let counter = 0;
var legacy = true;

export default function fetchData(url) {
    const local = 1;
    const inner = () => local + 1;
    return fetch(url);
}

function* idGen() {
    yield 1;
}

const add = (a, b) => a + b;
const multiply = function (a, b) { return a * b; };
export const LoginButton = ({ label }) => <button onClick={add}>{label}</button>;

export class Widget extends Component {
    static count = 0;
    handleClick = (e) => { this.setState({}); };
    constructor(props) { super(props); }
    render() { return this.helper(); }
    get label() { return 'x'; }
    static create() { return new Widget(); }
}

class Plain {
    method() {}
}

async function load() {
    const mod = await import('./lazy.js');
    const w = new Widget({});
    w.render();
    add(1, 2);
    utils.format(mod);
}

module.exports = { add, load };
"#;

    #[test]
    fn extracts_symbols_with_parents() {
        let a = analyze(spec(), SRC);
        assert!(a.errors.is_empty(), "{:?}", a.errors);
        assert!(has_symbol(&a, "fetchData", SymbolKind::Function));
        assert!(has_symbol(&a, "idGen", SymbolKind::Function));
        assert!(has_symbol(&a, "add", SymbolKind::Function));
        assert!(has_symbol(&a, "multiply", SymbolKind::Function));
        assert!(has_symbol(&a, "load", SymbolKind::Function));
        assert!(has_symbol(&a, "LoginButton", SymbolKind::Component));
        assert!(has_symbol(&a, "Widget", SymbolKind::Class));
        assert!(has_symbol(&a, "Plain", SymbolKind::Class));
        assert!(has_symbol(&a, "MAX_RETRIES", SymbolKind::Constant));
        assert!(has_symbol(&a, "fs", SymbolKind::Constant));
        assert!(has_symbol(&a, "counter", SymbolKind::Variable));
        assert!(has_symbol(&a, "legacy", SymbolKind::Variable));
        assert!(has_symbol(&a, "handleClick", SymbolKind::Method));
        assert!(has_symbol(&a, "count", SymbolKind::Field));
        assert_eq!(parent_of(&a, "handleClick").as_deref(), Some("Widget"));
        assert_eq!(parent_of(&a, "count").as_deref(), Some("Widget"));
        assert_eq!(parent_of(&a, "constructor").as_deref(), Some("Widget"));
        assert_eq!(parent_of(&a, "render").as_deref(), Some("Widget"));
        assert_eq!(parent_of(&a, "label").as_deref(), Some("Widget"));
        assert_eq!(parent_of(&a, "create").as_deref(), Some("Widget"));
        assert_eq!(parent_of(&a, "method").as_deref(), Some("Plain"));
        assert_eq!(parent_of(&a, "fetchData"), None);
        // Nested function-valued bindings are symbols (like nested declarations);
        // plain locals are not.
        assert!(has_symbol(&a, "inner", SymbolKind::Function));
        assert!(!names(&a).contains(&"local".to_string()));
        assert!(!names(&a).contains(&"mod".to_string()));
        assert!(!names(&a).contains(&"w".to_string()));
        let fetch_data = a.symbols.iter().find(|s| s.name == "fetchData").unwrap();
        assert!(fetch_data.signature.as_deref().unwrap().starts_with("function fetchData(url)"));
        assert_eq!(fetch_data.start_line, 13);
        let add = a.symbols.iter().find(|s| s.name == "add").unwrap();
        assert_eq!(add.signature.as_deref(), Some("const add = (a, b) => a + b;"));
    }

    #[test]
    fn extracts_imports_inheritance_and_calls() {
        let a = analyze(spec(), SRC);
        let imps = imports(&a);
        assert!(imps.contains(&"react".to_string()));
        assert!(imps.contains(&"./utils".to_string()));
        assert!(imps.contains(&"./styles.css".to_string()));
        assert!(imps.contains(&"fs".to_string()), "require() import");
        assert!(imps.contains(&"./lazy.js".to_string()), "dynamic import()");
        let exports: Vec<&str> = a.relationships.iter().filter(|r| r.kind == RelationshipKind::Export).map(|r| r.target.as_str()).collect();
        assert_eq!(exports, vec!["./helper", "./all"]);
        let ext = a.relationships.iter().find(|r| r.kind == RelationshipKind::Extends).unwrap();
        assert_eq!(ext.target, "Component");
        assert_eq!(ext.from_symbol.as_deref(), Some("Widget"));
        assert_eq!(ext.to_symbol.as_deref(), Some("Component"));
        let calls = calls(&a);
        assert!(calls.contains(&"fetch".to_string()));
        assert!(calls.contains(&"render".to_string()));
        assert!(calls.contains(&"add".to_string()));
        assert!(calls.contains(&"helper".to_string()));
        assert!(calls.contains(&"format".to_string()));
        assert!(calls.contains(&"Widget".to_string()), "new expression");
        let call = a.relationships.iter().find(|r| r.kind == RelationshipKind::Call && r.target == "fetch").unwrap();
        assert_eq!(call.from_symbol.as_deref(), Some("fetchData"));
        let call = a.relationships.iter().find(|r| r.kind == RelationshipKind::Call && r.target == "helper").unwrap();
        assert_eq!(call.from_symbol.as_deref(), Some("render"));
        let froms: Vec<Option<&str>> = a.relationships.iter().filter(|r| r.kind == RelationshipKind::Call && r.target == "Widget").map(|r| r.from_symbol.as_deref()).collect();
        assert_eq!(froms, vec![Some("create"), Some("load")]);
    }

    #[test]
    fn chunks_follow_symbols() {
        let a = analyze(spec(), SRC);
        let syms: Vec<Option<String>> = a.chunks.iter().map(|c| c.symbol.clone()).collect();
        assert!(syms.contains(&Some("fetchData".into())));
        assert!(syms.contains(&Some("Widget".into())));
        assert!(syms.contains(&Some("LoginButton".into())));
        assert!(syms.contains(&Some("load".into())));
        assert_eq!(syms[0], None, "leading import block is a gap chunk");
        assert!(a.chunks[0].content.trim_start().starts_with("import React"));
        // Whitespace-only gaps between items are dropped; everything else is kept.
        let total: usize = a.chunks.iter().map(|c| c.content.len()).sum();
        assert!(total >= SRC.trim().len() * 9 / 10, "chunks cover {total} of {} bytes", SRC.trim().len());
    }
}
