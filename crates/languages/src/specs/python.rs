//! Python.

use crate::treesitter::{last_identifier, ImportRule, LanguageSpec, NameSource, SymbolRule, TargetSource};
use crate::types::{LanguageId, Relationship, RelationshipKind, SymbolKind};
use tree_sitter::Node;

static SYMBOLS: &[SymbolRule] = &[
    // Methods are reported as functions; the enclosing `class_definition`
    // container records the class as their parent. `decorated_definition`
    // is transparent: the walk recurses into it and finds the definition.
    SymbolRule::new("function_definition", SymbolKind::Function, NameSource::Field("name")),
    SymbolRule::container("class_definition", SymbolKind::Class, NameSource::Field("name")),
];

static IMPORTS: &[ImportRule] = &[
    ImportRule { node_kind: "import_statement", relationship: RelationshipKind::Import, target: TargetSource::Custom, confidence: 0.9 },
    ImportRule { node_kind: "import_from_statement", relationship: RelationshipKind::Import, target: TargetSource::Custom, confidence: 0.9 },
    // `superclasses` is a parenthesised `argument_list`, which the generic
    // inheritance splitter cannot parse (it would yield `Mixin)`), so base
    // classes are emitted as `Extends` from `python_relationships` instead.
    ImportRule { node_kind: "class_definition", relationship: RelationshipKind::Extends, target: TargetSource::Custom, confidence: 0.8 },
];

fn text(node: Node<'_>, source: &str) -> Option<String> {
    node.utf8_text(source.as_bytes()).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn is_upper_case(name: &str) -> bool {
    name.chars().any(|c| c.is_ascii_uppercase()) && !name.chars().any(|c| c.is_ascii_lowercase())
}

/// Module-level `NAME = value` → `Constant` when `NAME` is UPPER_CASE, else
/// `Variable`. Assignments inside function, lambda or class bodies are not
/// symbols.
fn module_assignment(node: Node<'_>, source: &str) -> Option<(String, SymbolKind)> {
    if node.kind() != "assignment" {
        return None;
    }
    let left = node.child_by_field_name("left")?;
    if left.kind() != "identifier" || node.child_by_field_name("right").is_none() {
        return None;
    }
    let mut cur = node.parent();
    while let Some(p) = cur {
        if matches!(p.kind(), "function_definition" | "class_definition" | "lambda") {
            return None;
        }
        cur = p.parent();
    }
    let name = text(left, source)?;
    let kind = if is_upper_case(&name) { SymbolKind::Constant } else { SymbolKind::Variable };
    Some((name, kind))
}

fn import(target: String, node: Node<'_>) -> Relationship {
    Relationship { kind: RelationshipKind::Import, from_symbol: None, target, to_symbol: None, confidence: 0.9, start_byte: node.start_byte() }
}

/// Imports (`import a.b, c as d`, `from .pkg import x`) and base classes.
fn python_relationships(node: Node<'_>, source: &str) -> Vec<Relationship> {
    let mut out = Vec::new();
    let mut cursor = node.walk();
    match node.kind() {
        "import_statement" => {
            for name in node.children_by_field_name("name", &mut cursor) {
                let module = if name.kind() == "aliased_import" { name.child_by_field_name("name") } else { Some(name) };
                if let Some(t) = module.and_then(|n| text(n, source)) {
                    out.push(import(t, node));
                }
            }
        }
        "import_from_statement" => {
            // `module_name` is a `dotted_name` or a `relative_import`
            // (`.models`, `..pkg.sub`, or a bare `.`).
            if let Some(t) = node.child_by_field_name("module_name").and_then(|n| text(n, source)) {
                out.push(import(t, node));
            }
        }
        "class_definition" => {
            let class = node.child_by_field_name("name").and_then(|n| text(n, source));
            if let Some(bases) = node.child_by_field_name("superclasses") {
                for base in bases.named_children(&mut cursor) {
                    // `metaclass=ABCMeta` and friends are not base classes.
                    if base.kind() == "keyword_argument" {
                        continue;
                    }
                    let Some(t) = text(base, source) else { continue };
                    // `Generic[T]` → `Generic`, `models.Model` → `Model`.
                    let name = last_identifier(t.split('[').next().unwrap_or(&t));
                    if name.is_empty() || !name.chars().next().map(|c| c.is_alphabetic() || c == '_').unwrap_or(false) {
                        continue;
                    }
                    out.push(Relationship { kind: RelationshipKind::Extends, from_symbol: class.clone(), target: name.clone(), to_symbol: Some(name), confidence: 0.8, start_byte: node.start_byte() });
                }
            }
        }
        _ => {}
    }
    out
}

pub fn spec() -> LanguageSpec {
    LanguageSpec {
        id: LanguageId::Python,
        language: || tree_sitter_python::LANGUAGE.into(),
        extensions: &["py", "pyi", "pyw"],
        filenames: &[],
        symbols: SYMBOLS,
        imports: IMPORTS,
        calls: &[("call", "function")],
        inheritance: &[],
        function_scopes: &["function_definition", "lambda"],
        custom_name: None,
        custom_imports: Some(python_relationships),
        custom_symbol: Some(module_assignment),
        test_markers: &["/tests/", "/test/", "test_", "_test.py", "conftest"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::specs::testing::*;

    const SRC: &str = r#"
import os
import sys as system
import os.path, json
from collections import OrderedDict, defaultdict
from .models import User
from ..pkg.sub import thing
from . import sibling

MAX_RETRIES = 3
default_name: str = "svc"


def helper(x):
    local_var = 1
    LOCAL_CONST = 2
    return x + local_var


@decorator
def decorated(y):
    return y


class Base:
    def ping(self):
        return "pong"


@dataclass
class Service(Base, Mixin, metaclass=ABCMeta):
    counter = 0

    def __init__(self, name):
        self.name = name

    @staticmethod
    def build(cfg):
        inner = helper(cfg)
        return Service(inner)

    async def run(self):
        await self.step()


square = lambda v: v * v


def main():
    s = Service.build({})
    s.run()
    print(helper(2))
    obj.attr.method()
"#;

    #[test]
    fn extracts_symbols_with_parents() {
        let a = analyze(spec(), SRC);
        assert!(a.errors.is_empty(), "{:?}", a.errors);
        assert!(has_symbol(&a, "helper", SymbolKind::Function));
        assert_eq!(parent_of(&a, "helper"), None);
        assert!(has_symbol(&a, "decorated", SymbolKind::Function), "decorated_definition is transparent");
        assert!(has_symbol(&a, "Base", SymbolKind::Class));
        assert!(has_symbol(&a, "Service", SymbolKind::Class), "decorated class is found");
        assert_eq!(parent_of(&a, "ping").as_deref(), Some("Base"));
        assert_eq!(parent_of(&a, "__init__").as_deref(), Some("Service"));
        assert_eq!(parent_of(&a, "build").as_deref(), Some("Service"), "decorated method keeps its class as parent");
        assert_eq!(parent_of(&a, "run").as_deref(), Some("Service"));
        assert!(has_symbol(&a, "MAX_RETRIES", SymbolKind::Constant));
        assert!(has_symbol(&a, "default_name", SymbolKind::Variable));
        assert!(has_symbol(&a, "square", SymbolKind::Variable));
        let names = names(&a);
        assert!(!names.contains(&"local_var".to_string()));
        assert!(!names.contains(&"LOCAL_CONST".to_string()), "UPPER_CASE locals are still locals");
        assert!(!names.contains(&"inner".to_string()));
        assert!(!names.contains(&"s".to_string()));
        assert!(!names.contains(&"counter".to_string()), "class attributes are not module-level symbols");
        let main = a.symbols.iter().find(|s| s.name == "main").unwrap();
        assert!(main.signature.as_deref().unwrap().starts_with("def main():"));
        let main_line = SRC.lines().position(|l| l.starts_with("def main")).unwrap() + 1;
        assert_eq!(main.start_line, main_line);
    }

    #[test]
    fn extracts_imports_inheritance_and_calls() {
        let a = analyze(spec(), SRC);
        let imps = imports(&a);
        assert!(imps.contains(&"os".to_string()));
        assert!(imps.contains(&"sys".to_string()), "aliased import records the module, not the alias");
        assert!(!imps.contains(&"system".to_string()));
        assert!(imps.contains(&"os.path".to_string()));
        assert!(imps.contains(&"json".to_string()));
        assert!(imps.contains(&"collections".to_string()));
        assert!(imps.contains(&".models".to_string()));
        assert!(imps.contains(&"..pkg.sub".to_string()));
        assert!(imps.contains(&".".to_string()));
        let bases: Vec<&Relationship> = a.relationships.iter().filter(|r| r.kind == RelationshipKind::Extends).collect();
        assert_eq!(bases.len(), 2, "{bases:?}");
        assert!(bases.iter().all(|r| r.from_symbol.as_deref() == Some("Service")));
        assert!(bases.iter().any(|r| r.target == "Base" && r.to_symbol.as_deref() == Some("Base")));
        assert!(bases.iter().any(|r| r.target == "Mixin"));
        let calls = calls(&a);
        assert!(calls.contains(&"build".to_string()));
        assert!(calls.contains(&"run".to_string()));
        assert!(calls.contains(&"print".to_string()));
        assert!(calls.contains(&"helper".to_string()));
        assert!(calls.contains(&"step".to_string()));
        assert!(calls.contains(&"method".to_string()));
        assert!(calls.contains(&"Service".to_string()));
        let print = a.relationships.iter().find(|r| r.kind == RelationshipKind::Call && r.target == "print").unwrap();
        assert_eq!(print.from_symbol.as_deref(), Some("main"));
        let helper_from_build = a.relationships.iter().find(|r| r.kind == RelationshipKind::Call && r.target == "helper" && r.from_symbol.as_deref() == Some("build"));
        assert!(helper_from_build.is_some(), "call inside a method is attributed to the method");
    }

    #[test]
    fn chunks_follow_symbols() {
        let a = analyze(spec(), SRC);
        let syms: Vec<Option<String>> = a.chunks.iter().map(|c| c.symbol.clone()).collect();
        assert!(syms.contains(&Some("helper".into())));
        assert!(syms.contains(&Some("Base".into())));
        assert!(syms.contains(&Some("Service".into())));
        assert!(syms.contains(&Some("main".into())));
        assert!(syms.contains(&Some("MAX_RETRIES".into())));
        assert!(!syms.contains(&Some("build".into())), "methods stay inside their class chunk");
        assert_eq!(syms[0], None, "leading import block is a gap chunk");
        // Whitespace-only gaps between items are dropped; everything else is kept.
        let total: usize = a.chunks.iter().map(|c| c.content.len()).sum();
        assert!(total >= SRC.trim().len() * 9 / 10, "chunks cover {total} of {} bytes", SRC.trim().len());
    }
}
