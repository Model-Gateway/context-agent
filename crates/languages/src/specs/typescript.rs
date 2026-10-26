//! TypeScript and TSX. Both specs share the rule tables below; they differ
//! only in grammar, [`LanguageId`] and extensions.

use crate::treesitter::{first_child_of_kind, strip_quotes, ImportRule, LanguageSpec, NameSource, SymbolRule, TargetSource};
use crate::types::{LanguageId, Relationship, RelationshipKind, SymbolKind};
use tree_sitter::Node;

static SYMBOLS: &[SymbolRule] = &[
    SymbolRule::new("function_declaration", SymbolKind::Function, NameSource::Field("name")),
    SymbolRule::new("generator_function_declaration", SymbolKind::Function, NameSource::Field("name")),
    SymbolRule::new("function_signature", SymbolKind::Function, NameSource::Field("name")),
    SymbolRule::container("class_declaration", SymbolKind::Class, NameSource::Field("name")),
    SymbolRule::container("abstract_class_declaration", SymbolKind::Class, NameSource::Field("name")),
    SymbolRule::new("method_definition", SymbolKind::Method, NameSource::Field("name")),
    SymbolRule::new("method_signature", SymbolKind::Method, NameSource::Field("name")),
    SymbolRule::new("abstract_method_signature", SymbolKind::Method, NameSource::Field("name")),
    SymbolRule::container("interface_declaration", SymbolKind::Interface, NameSource::Field("name")),
    SymbolRule::new("type_alias_declaration", SymbolKind::TypeAlias, NameSource::Field("name")),
    SymbolRule::new("enum_declaration", SymbolKind::Enum, NameSource::Field("name")),
    // `namespace NS {}` and `declare module "x" {}`; the name may be a string.
    SymbolRule::container("internal_module", SymbolKind::Namespace, NameSource::Custom),
    SymbolRule::container("module", SymbolKind::Namespace, NameSource::Custom),
];

static IMPORTS: &[ImportRule] = &[
    ImportRule { node_kind: "import_statement", relationship: RelationshipKind::Import, target: TargetSource::Field("source"), confidence: 0.9 },
    ImportRule { node_kind: "export_statement", relationship: RelationshipKind::Export, target: TargetSource::Field("source"), confidence: 0.9 },
    // `require("x")` and dynamic `import("x")`; see `ts_imports`.
    ImportRule { node_kind: "call_expression", relationship: RelationshipKind::Import, target: TargetSource::Custom, confidence: 0.8 },
    // `implements A, B` and `interface X extends A, B` carry their bases as
    // repeated children rather than a single field; see `ts_imports`.
    ImportRule { node_kind: "implements_clause", relationship: RelationshipKind::Implements, target: TargetSource::Custom, confidence: 0.8 },
    ImportRule { node_kind: "extends_type_clause", relationship: RelationshipKind::Extends, target: TargetSource::Custom, confidence: 0.8 },
];

static CALLS: &[(&str, &str)] = &[("call_expression", "function"), ("new_expression", "constructor")];

static INHERITANCE: &[(&str, &str, RelationshipKind)] = &[("extends_clause", "value", RelationshipKind::Extends)];

static FUNCTION_SCOPES: &[&str] = &["function_declaration", "arrow_function", "function_expression", "method_definition", "generator_function_declaration"];

/// Ancestors that make a `variable_declarator` a local rather than a
/// module-level binding.
static LOCAL_SCOPES: &[&str] = &["function_declaration", "arrow_function", "function_expression", "method_definition", "generator_function_declaration", "class_static_block", "for_statement", "for_in_statement"];

static TEST_MARKERS: &[&str] = &[".test.", ".spec.", "/__tests__/"];

fn node_text(node: Node<'_>, source: &str) -> Option<String> {
    node.utf8_text(source.as_bytes()).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn field_text(node: Node<'_>, field: &str, source: &str) -> Option<String> {
    node.child_by_field_name(field).and_then(|n| node_text(n, source))
}

/// `Button`, `AppShell`: an upper-case initial followed by at least one
/// lower-case letter (so `MAX` stays a plain function).
fn is_pascal_case(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_uppercase()) && chars.any(|c| c.is_ascii_lowercase())
}

fn has_ancestor_in(node: Node<'_>, kinds: &[&str]) -> bool {
    let mut cur = node.parent();
    while let Some(n) = cur {
        if kinds.contains(&n.kind()) {
            return true;
        }
        cur = n.parent();
    }
    false
}

/// Name of the class or interface that owns a heritage clause.
fn heritage_owner(node: Node<'_>, source: &str) -> Option<String> {
    let mut cur = node.parent();
    while let Some(n) = cur {
        if matches!(n.kind(), "class_declaration" | "abstract_class_declaration" | "class" | "interface_declaration") {
            return field_text(n, "name", source);
        }
        cur = n.parent();
    }
    None
}

fn ts_name(node: Node<'_>, source: &str) -> Option<String> {
    match node.kind() {
        "internal_module" | "module" => field_text(node, "name", source).map(|t| strip_quotes(&t)).filter(|t| !t.is_empty()),
        _ => None,
    }
}

fn declarator_symbol(node: Node<'_>, source: &str) -> Option<(String, SymbolKind)> {
    let name_node = node.child_by_field_name("name")?;
    // Destructuring patterns (`const { a, b } = ...`) are not symbols.
    if name_node.kind() != "identifier" {
        return None;
    }
    // Bindings inside function bodies (including closures) are locals.
    if has_ancestor_in(node, LOCAL_SCOPES) {
        return None;
    }
    let name = node_text(name_node, source)?;
    match node.child_by_field_name("value").map(|v| v.kind()) {
        Some("arrow_function") if is_pascal_case(&name) => return Some((name, SymbolKind::Component)),
        Some("arrow_function") | Some("function_expression") => return Some((name, SymbolKind::Function)),
        _ => {}
    }
    let declaration = node.parent()?;
    let is_const = declaration.kind() == "lexical_declaration" && field_text(declaration, "kind", source).as_deref() == Some("const");
    Some((name, if is_const { SymbolKind::Constant } else { SymbolKind::Variable }))
}

fn ts_symbol(node: Node<'_>, source: &str) -> Option<(String, SymbolKind)> {
    match node.kind() {
        "variable_declarator" => declarator_symbol(node, source),
        // `handler = () => {...}` class fields behave like methods.
        "public_field_definition" => {
            if node.child_by_field_name("value")?.kind() != "arrow_function" {
                return None;
            }
            field_text(node, "name", source).map(|name| (name, SymbolKind::Method))
        }
        _ => None,
    }
}

fn module_call(node: Node<'_>, source: &str) -> Option<Relationship> {
    let callee = node.child_by_field_name("function")?;
    let confidence = match callee.kind() {
        "import" => 0.7,
        "identifier" if node_text(callee, source).as_deref() == Some("require") => 0.8,
        _ => return None,
    };
    // Only literal specifiers can be resolved.
    let specifier = first_child_of_kind(node.child_by_field_name("arguments")?, "string")?;
    let target = strip_quotes(&node_text(specifier, source)?);
    if target.is_empty() {
        return None;
    }
    Some(Relationship { kind: RelationshipKind::Import, from_symbol: None, target, to_symbol: None, confidence, start_byte: node.start_byte() })
}

fn heritage(node: Node<'_>, source: &str, kind: RelationshipKind) -> Vec<Relationship> {
    let from = heritage_owner(node, source);
    let mut cursor = node.walk();
    let bases: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
    bases
        .into_iter()
        .filter_map(|base| node_text(base, source))
        .map(|t| t.split('<').next().unwrap_or(&t).trim().to_string())
        .filter(|t| !t.is_empty())
        .map(|name| Relationship { kind, from_symbol: from.clone(), target: name.clone(), to_symbol: Some(name), confidence: 0.8, start_byte: node.start_byte() })
        .collect()
}

fn ts_imports(node: Node<'_>, source: &str) -> Vec<Relationship> {
    match node.kind() {
        "call_expression" => module_call(node, source).into_iter().collect(),
        "implements_clause" => heritage(node, source, RelationshipKind::Implements),
        "extends_type_clause" => heritage(node, source, RelationshipKind::Extends),
        _ => Vec::new(),
    }
}

pub fn spec() -> LanguageSpec {
    LanguageSpec {
        id: LanguageId::TypeScript,
        language: || tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        extensions: &["ts", "mts", "cts"],
        filenames: &[],
        symbols: SYMBOLS,
        imports: IMPORTS,
        calls: CALLS,
        inheritance: INHERITANCE,
        function_scopes: FUNCTION_SCOPES,
        custom_name: Some(ts_name),
        custom_imports: Some(ts_imports),
        custom_symbol: Some(ts_symbol),
        test_markers: TEST_MARKERS,
    }
}

pub fn tsx_spec() -> LanguageSpec {
    LanguageSpec { id: LanguageId::Tsx, language: || tree_sitter_typescript::LANGUAGE_TSX.into(), extensions: &["tsx"], ..spec() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::specs::testing::*;

    const SRC: &str = r#"
import React, { useState } from "react";
import type { Props } from "./types";
import * as utils from "../utils";
export { helper } from "./helper";
const fs = require("fs");
const lazy = import("./lazy");

export const MAX_RETRIES = 3;
let counter = 0;

export function greet(name: string): string {
  const local = name.trim();
  return `hi ${local}`;
}

export const double = (n: number): number => n * 2;
const triple = function (n: number) { return n * 3; };

export interface Clickable extends Props {
  onClick(): void;
}

export type Id = string | number;

export enum Color { Red, Green }

export abstract class Base {
  abstract label(): string;
}

export class Service extends Base implements Clickable {
  private count = 0;
  handler = () => { this.count++; };

  constructor(private name: string) { super(); }

  onClick(): void { utils.log(this.name); }

  label(): string { return greet(this.name); }

  static create(): Service { return new Service("svc"); }
}

export namespace Config {
  export const DEFAULT = "x";
  export function load() { return DEFAULT; }
}

declare module "legacy" {
  export function init(): void;
}

export default function main() {
  const svc = Service.create();
  svc.onClick();
  console.log(double(MAX_RETRIES));
}
"#;

    const TSX: &str = r#"
import React, { useState } from "react";
import type { ButtonProps } from "./types";

export const Button = ({ label, onPress }: ButtonProps) => {
  const [count, setCount] = useState(0);
  const handle = () => { setCount(count + 1); onPress(); };
  return <button onClick={handle}>{label}</button>;
};

export default function App() {
  const title = "Demo";
  return (
    <main>
      <h1>{title}</h1>
      <Button label="Go" onPress={() => console.log("pressed")} />
    </main>
  );
}

export class Panel extends React.Component<ButtonProps> {
  render() {
    return <section><Button label="x" onPress={() => {}} /></section>;
  }
}
"#;

    #[test]
    fn extracts_symbols_with_parents() {
        let a = analyze(spec(), SRC);
        assert!(a.errors.is_empty(), "{:?}", a.errors);
        assert!(has_symbol(&a, "greet", SymbolKind::Function));
        assert!(has_symbol(&a, "main", SymbolKind::Function), "export default function is named");
        assert!(has_symbol(&a, "double", SymbolKind::Function));
        assert!(has_symbol(&a, "triple", SymbolKind::Function));
        assert!(has_symbol(&a, "Base", SymbolKind::Class));
        assert!(has_symbol(&a, "Service", SymbolKind::Class));
        assert!(has_symbol(&a, "Clickable", SymbolKind::Interface));
        assert!(has_symbol(&a, "Id", SymbolKind::TypeAlias));
        assert!(has_symbol(&a, "Color", SymbolKind::Enum));
        assert!(has_symbol(&a, "Config", SymbolKind::Namespace));
        assert!(has_symbol(&a, "legacy", SymbolKind::Namespace), "module name is unquoted");
        assert!(has_symbol(&a, "MAX_RETRIES", SymbolKind::Constant));
        assert!(has_symbol(&a, "counter", SymbolKind::Variable));
        assert!(has_symbol(&a, "fs", SymbolKind::Constant));
        assert!(has_symbol(&a, "handler", SymbolKind::Method), "arrow-function field is a method");
        assert_eq!(parent_of(&a, "handler").as_deref(), Some("Service"));
        assert_eq!(parent_of(&a, "constructor").as_deref(), Some("Service"));
        assert_eq!(parent_of(&a, "create").as_deref(), Some("Service"));
        assert!(a.symbols.iter().any(|s| s.name == "onClick" && s.kind == SymbolKind::Method && s.parent.as_deref() == Some("Clickable")));
        assert!(a.symbols.iter().any(|s| s.name == "onClick" && s.kind == SymbolKind::Method && s.parent.as_deref() == Some("Service")));
        assert!(a.symbols.iter().any(|s| s.name == "label" && s.parent.as_deref() == Some("Base")));
        assert_eq!(parent_of(&a, "DEFAULT").as_deref(), Some("Config"));
        assert_eq!(parent_of(&a, "load").as_deref(), Some("Config"));
        assert_eq!(parent_of(&a, "init").as_deref(), Some("legacy"));
        assert_eq!(parent_of(&a, "greet"), None);
        let found = names(&a);
        assert!(!found.contains(&"local".to_string()), "locals are not symbols");
        assert!(!found.contains(&"svc".to_string()));
        assert!(!found.contains(&"count".to_string()), "plain fields are not symbols");
        let main = a.symbols.iter().find(|s| s.name == "main").unwrap();
        assert!(main.signature.as_deref().unwrap().starts_with("function main()"));
        assert_eq!(main.start_line, 54);
    }

    #[test]
    fn extracts_imports_inheritance_and_calls() {
        let a = analyze(spec(), SRC);
        let imps = imports(&a);
        for expected in ["react", "./types", "../utils", "fs", "./lazy"] {
            assert!(imps.contains(&expected.to_string()), "missing import {expected} in {imps:?}");
        }
        assert!(!imps.contains(&"./helper".to_string()), "re-exports are not imports");
        assert!(a.relationships.iter().any(|r| r.kind == RelationshipKind::Export && r.target == "./helper"));
        let ext = a.relationships.iter().find(|r| r.kind == RelationshipKind::Extends && r.target == "Base").unwrap();
        assert_eq!(ext.from_symbol.as_deref(), Some("Service"));
        let imp = a.relationships.iter().find(|r| r.kind == RelationshipKind::Implements && r.target == "Clickable").unwrap();
        assert_eq!(imp.from_symbol.as_deref(), Some("Service"));
        let iface = a.relationships.iter().find(|r| r.kind == RelationshipKind::Extends && r.target == "Props").unwrap();
        assert_eq!(iface.from_symbol.as_deref(), Some("Clickable"));
        let calls = calls(&a);
        for expected in ["create", "onClick", "log", "double", "greet", "Service"] {
            assert!(calls.contains(&expected.to_string()), "missing call {expected} in {calls:?}");
        }
        let call = a.relationships.iter().find(|r| r.kind == RelationshipKind::Call && r.target == "double").unwrap();
        assert_eq!(call.from_symbol.as_deref(), Some("main"));
        let ctor = a.relationships.iter().find(|r| r.kind == RelationshipKind::Call && r.target == "Service").unwrap();
        assert_eq!(ctor.from_symbol.as_deref(), Some("create"));
    }

    #[test]
    fn chunks_follow_symbols() {
        let a = analyze(spec(), SRC);
        let syms: Vec<Option<String>> = a.chunks.iter().map(|c| c.symbol.clone()).collect();
        assert!(syms.contains(&Some("Service".into())));
        assert!(syms.contains(&Some("greet".into())));
        assert!(syms.contains(&Some("main".into())));
        assert!(syms.contains(&Some("Config".into())));
        assert_eq!(syms[0], None, "leading import block is a gap chunk");
        let total: usize = a.chunks.iter().map(|c| c.content.len()).sum();
        assert!(total >= SRC.trim().len() * 9 / 10, "chunks cover {total} of {} bytes", SRC.trim().len());
    }

    #[test]
    fn tsx_components_and_jsx() {
        let a = analyze(tsx_spec(), TSX);
        assert!(a.errors.is_empty(), "{:?}", a.errors);
        assert!(has_symbol(&a, "Button", SymbolKind::Component));
        assert!(has_symbol(&a, "App", SymbolKind::Function));
        assert!(has_symbol(&a, "Panel", SymbolKind::Class));
        assert_eq!(parent_of(&a, "render").as_deref(), Some("Panel"));
        let found = names(&a);
        assert!(!found.contains(&"title".to_string()), "locals are not symbols");
        assert!(!found.contains(&"count".to_string()));
        assert!(!found.contains(&"handle".to_string()), "nested arrow functions are not symbols");
        assert!(imports(&a).contains(&"react".to_string()));
        let ext = a.relationships.iter().find(|r| r.kind == RelationshipKind::Extends).unwrap();
        assert_eq!(ext.from_symbol.as_deref(), Some("Panel"));
        assert_eq!(ext.target, "React.Component");
        let hook = a.relationships.iter().find(|r| r.kind == RelationshipKind::Call && r.target == "useState").unwrap();
        assert_eq!(hook.from_symbol.as_deref(), Some("Button"));
        let syms: Vec<Option<String>> = a.chunks.iter().map(|c| c.symbol.clone()).collect();
        assert_eq!(syms[0], None, "leading import block is a gap chunk");
        assert!(syms.contains(&Some("Button".into())));
        assert!(syms.contains(&Some("App".into())));
        assert!(syms.contains(&Some("Panel".into())));
    }
}
