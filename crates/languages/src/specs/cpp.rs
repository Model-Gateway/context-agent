//! C++.
//!
//! Builds on the C shape (functions, struct/enum/union bodies, typedefs,
//! preprocessor macros and includes) and adds classes, namespaces, templates
//! (transparent: the inner declaration is walked as a child), `using` type
//! aliases, qualified out-of-line definitions (`Widget::draw`) and methods
//! declared inside class bodies.

use crate::treesitter::{ImportRule, LanguageSpec, NameSource, SymbolRule, TargetSource};
use crate::types::{LanguageId, Relationship, RelationshipKind, SymbolKind};
use tree_sitter::Node;

static SYMBOLS: &[SymbolRule] = &[
    SymbolRule::new("function_definition", SymbolKind::Function, NameSource::Custom),
    // The type specifiers only count as definitions when they carry a body;
    // `custom_name` returns `None` for forward declarations and type uses.
    SymbolRule::container("class_specifier", SymbolKind::Class, NameSource::Custom),
    SymbolRule::container("struct_specifier", SymbolKind::Struct, NameSource::Custom),
    SymbolRule::container("union_specifier", SymbolKind::Struct, NameSource::Custom),
    SymbolRule::container("enum_specifier", SymbolKind::Enum, NameSource::Custom),
    SymbolRule::container("namespace_definition", SymbolKind::Namespace, NameSource::Field("name")),
    SymbolRule::new("alias_declaration", SymbolKind::TypeAlias, NameSource::Field("name")),
    SymbolRule::new("type_definition", SymbolKind::TypeAlias, NameSource::FieldDescendant("declarator", "type_identifier")),
    SymbolRule::new("preproc_def", SymbolKind::Macro, NameSource::Field("name")),
    SymbolRule::new("preproc_function_def", SymbolKind::Macro, NameSource::Field("name")),
];

static IMPORTS: &[ImportRule] = &[
    ImportRule { node_kind: "preproc_include", relationship: RelationshipKind::Include, target: TargetSource::Field("path"), confidence: 0.9 },
    // `base_class_clause` is a plain child of `class_specifier` /
    // `struct_specifier`, not a field, so the generic inheritance table cannot
    // reach it; `custom_imports` emits one `Extends` per base instead.
    ImportRule { node_kind: "base_class_clause", relationship: RelationshipKind::Extends, target: TargetSource::Custom, confidence: 0.8 },
];

fn text(node: Node<'_>, source: &str) -> Option<String> {
    node.utf8_text(source.as_bytes()).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn first_named_child(node: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = node.walk();
    let found = node.named_children(&mut cursor).next();
    found
}

/// Text of a name-like node: `Widget::draw`, `~Widget`, `operator==`,
/// `operator int`; template arguments are dropped (`Box<T>::get` → `Box::get`).
fn name_text(node: Node<'_>, source: &str) -> Option<String> {
    match node.kind() {
        "qualified_identifier" => {
            let name = name_text(node.child_by_field_name("name")?, source)?;
            match node.child_by_field_name("scope") {
                Some(scope) => Some(format!("{}::{name}", name_text(scope, source).or_else(|| text(scope, source))?)),
                None => Some(name),
            }
        }
        "template_function" | "template_method" | "template_type" => name_text(node.child_by_field_name("name")?, source),
        "operator_cast" => text(node.child_by_field_name("type")?, source).map(|t| format!("operator {t}")),
        "identifier" | "field_identifier" | "type_identifier" | "namespace_identifier" | "destructor_name" | "operator_name" => text(node, source),
        _ => None,
    }
}

/// Follows a declaration's `declarator` chain down to the declared name,
/// looking through pointer / reference / array / init declarators. The flag
/// is `true` when a `function_declarator` was crossed, i.e. the node declares
/// a function rather than an object (a parenthesised declarator resets it, so
/// `int (*cb)(int)` is a function-pointer variable, not a function).
fn declarator_name(node: Node<'_>, source: &str) -> Option<(String, bool)> {
    let mut cur = node.child_by_field_name("declarator")?;
    let mut is_function = false;
    loop {
        match cur.kind() {
            "function_declarator" => {
                is_function = true;
                cur = cur.child_by_field_name("declarator")?;
            }
            "parenthesized_declarator" => {
                is_function = false;
                cur = first_named_child(cur)?;
            }
            "pointer_declarator" | "init_declarator" | "array_declarator" => cur = cur.child_by_field_name("declarator")?,
            "reference_declarator" | "attributed_declarator" => cur = first_named_child(cur)?,
            "operator_cast" => return name_text(cur, source).map(|n| (n, true)),
            _ => return name_text(cur, source).map(|n| (n, is_function)),
        }
    }
}

/// Kind of the nearest ancestor that is not a `template_declaration`.
fn enclosing_kind<'a>(node: Node<'a>) -> &'a str {
    let mut cur = node.parent();
    while let Some(p) = cur {
        if p.kind() != "template_declaration" {
            return p.kind();
        }
        cur = p.parent();
    }
    ""
}

fn in_function_scope(node: Node<'_>) -> bool {
    let mut cur = node.parent();
    while let Some(p) = cur {
        if matches!(p.kind(), "function_definition" | "lambda_expression") {
            return true;
        }
        cur = p.parent();
    }
    false
}

fn is_const(node: Node<'_>, source: &str) -> bool {
    let mut cursor = node.walk();
    let found = node.named_children(&mut cursor).any(|c| c.kind() == "type_qualifier" && matches!(text(c, source).as_deref(), Some("const" | "constexpr" | "constinit")));
    found
}

fn custom_name(node: Node<'_>, source: &str) -> Option<String> {
    match node.kind() {
        "function_definition" => declarator_name(node, source).map(|(name, _)| name),
        "class_specifier" | "struct_specifier" | "union_specifier" | "enum_specifier" => {
            node.child_by_field_name("body")?;
            name_text(node.child_by_field_name("name")?, source)
        }
        _ => None,
    }
}

fn custom_symbol(node: Node<'_>, source: &str) -> Option<(String, SymbolKind)> {
    let enclosing = enclosing_kind(node);
    let in_class = enclosing == "field_declaration_list";
    match node.kind() {
        // Inline method definition inside a class body.
        "function_definition" if in_class => declarator_name(node, source).map(|(name, _)| (name, SymbolKind::Method)),
        // Method declarations; constructors and destructors have no type and
        // therefore parse as plain `declaration`. Data members are skipped.
        "field_declaration" | "declaration" if in_class => match declarator_name(node, source) {
            Some((name, true)) => Some((name, SymbolKind::Method)),
            _ => None,
        },
        // Namespace / file scope: prototypes and globals. Locals are skipped
        // here rather than via `top_level_only` because the kind depends on
        // the declarator.
        "declaration" if enclosing != "friend_declaration" && !in_function_scope(node) => {
            let (name, is_function) = declarator_name(node, source)?;
            let kind = if is_function {
                SymbolKind::Function
            } else if is_const(node, source) {
                SymbolKind::Constant
            } else {
                SymbolKind::Variable
            };
            Some((name, kind))
        }
        _ => None,
    }
}

fn base_classes(node: Node<'_>, source: &str) -> Vec<Relationship> {
    if node.kind() != "base_class_clause" {
        return Vec::new();
    }
    let from = node.parent().and_then(|p| p.child_by_field_name("name")).and_then(|n| name_text(n, source));
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|c| matches!(c.kind(), "type_identifier" | "qualified_identifier" | "template_type"))
        .filter_map(|c| name_text(c, source))
        .map(|target| Relationship { kind: RelationshipKind::Extends, from_symbol: from.clone(), target: target.clone(), to_symbol: Some(target), confidence: 0.8, start_byte: node.start_byte() })
        .collect()
}

pub fn spec() -> LanguageSpec {
    LanguageSpec {
        id: LanguageId::Cpp,
        language: || tree_sitter_cpp::LANGUAGE.into(),
        extensions: &["cpp", "cc", "cxx", "hpp", "hh", "hxx", "h++", "ipp", "inl"],
        filenames: &[],
        symbols: SYMBOLS,
        imports: IMPORTS,
        calls: &[("call_expression", "function")],
        inheritance: &[],
        function_scopes: &["function_definition", "lambda_expression"],
        custom_name: Some(custom_name),
        custom_imports: Some(base_classes),
        custom_symbol: Some(custom_symbol),
        test_markers: &["_test.cpp", "_test.cc", "_test.cxx", "_tests.cpp", "_tests.cc", "/gtest/", "/googletest/", "/catch2/", "/doctest/"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::specs::testing::*;

    const SRC: &str = r#"
#include <vector>
#include <string>
#include "widget.h"

#define MAX_WIDGETS 10
#define SQUARE(x) ((x) * (x))

namespace app {

class Widget : public Base, private Mixin {
public:
    Widget();
    ~Widget();
    void draw() const;
    int size() const { return n_; }
    static Widget make();
    bool operator==(const Widget& o) const;
private:
    int n_;
};

Widget::Widget() : n_(0) {}

void Widget::draw() const {
    int local = n_;
    render(local);
}

struct Point : Shape { int x; int y; };

template <typename T>
T identity(T v) { return v; }

template <typename T>
class Box {
public:
    T get() const { return v_; }
private:
    T v_;
};

enum class Color { Red, Green };
union Value { int i; float f; };

using Id = int;
typedef int Count;

const int kLimit = 3;
int counter = 0;
int* make_ptr(int a);

}  // namespace app

int main() {
    app::Widget w;
    w.draw();
    auto v = app::identity(3);
    auto fn = [](int x) { int y = x; return y; };
    return SQUARE(fn(v));
}
"#;

    #[test]
    fn extracts_symbols_with_parents() {
        let a = analyze(spec(), SRC);
        assert!(a.errors.is_empty(), "{:?}", a.errors);
        assert!(has_symbol(&a, "app", SymbolKind::Namespace));
        assert!(has_symbol(&a, "Widget", SymbolKind::Class));
        assert_eq!(parent_of(&a, "Widget").as_deref(), Some("app"));
        // Methods declared or defined inside the class body.
        let method_parent = |name: &str| a.symbols.iter().find(|s| s.name == name && s.kind == SymbolKind::Method).and_then(|s| s.parent.clone());
        assert_eq!(method_parent("Widget").as_deref(), Some("Widget"), "constructor");
        assert_eq!(method_parent("~Widget").as_deref(), Some("Widget"), "destructor");
        assert_eq!(method_parent("draw").as_deref(), Some("Widget"));
        assert_eq!(method_parent("size").as_deref(), Some("Widget"));
        assert_eq!(method_parent("make").as_deref(), Some("Widget"));
        assert_eq!(method_parent("operator==").as_deref(), Some("Widget"));
        assert_eq!(method_parent("get").as_deref(), Some("Box"));
        // Out-of-line definitions keep their qualified name.
        assert!(has_symbol(&a, "Widget::Widget", SymbolKind::Function));
        assert!(has_symbol(&a, "Widget::draw", SymbolKind::Function));
        assert_eq!(parent_of(&a, "Widget::draw").as_deref(), Some("app"));
        assert!(has_symbol(&a, "Point", SymbolKind::Struct));
        assert!(has_symbol(&a, "identity", SymbolKind::Function));
        assert!(has_symbol(&a, "Box", SymbolKind::Class));
        assert!(has_symbol(&a, "Color", SymbolKind::Enum));
        assert!(has_symbol(&a, "Value", SymbolKind::Struct));
        assert!(has_symbol(&a, "Id", SymbolKind::TypeAlias));
        assert!(has_symbol(&a, "Count", SymbolKind::TypeAlias));
        assert!(has_symbol(&a, "kLimit", SymbolKind::Constant));
        assert!(has_symbol(&a, "counter", SymbolKind::Variable));
        assert!(has_symbol(&a, "make_ptr", SymbolKind::Function));
        assert!(has_symbol(&a, "MAX_WIDGETS", SymbolKind::Macro));
        assert!(has_symbol(&a, "SQUARE", SymbolKind::Macro));
        let n = names(&a);
        for local in ["local", "w", "v", "fn", "x", "y", "n_", "v_"] {
            assert!(!n.contains(&local.to_string()), "{local} must not be a symbol");
        }
        let main = a.symbols.iter().find(|s| s.name == "main").unwrap();
        assert_eq!(main.kind, SymbolKind::Function);
        assert_eq!(main.parent, None);
        assert!(main.signature.as_deref().unwrap().starts_with("int main()"));
        assert_eq!(main.start_line, 55);
    }

    #[test]
    fn extracts_includes_inheritance_and_calls() {
        let a = analyze(spec(), SRC);
        let imps = imports(&a);
        assert!(imps.contains(&"vector".to_string()));
        assert!(imps.contains(&"string".to_string()));
        assert!(imps.contains(&"widget.h".to_string()));
        let ext: Vec<(Option<String>, String)> = a.relationships.iter().filter(|r| r.kind == RelationshipKind::Extends).map(|r| (r.from_symbol.clone(), r.target.clone())).collect();
        assert!(ext.contains(&(Some("Widget".into()), "Base".into())), "{ext:?}");
        assert!(ext.contains(&(Some("Widget".into()), "Mixin".into())), "{ext:?}");
        assert!(ext.contains(&(Some("Point".into()), "Shape".into())), "{ext:?}");
        let calls = calls(&a);
        assert!(calls.contains(&"render".to_string()));
        assert!(calls.contains(&"draw".to_string()));
        assert!(calls.contains(&"identity".to_string()));
        assert!(calls.contains(&"SQUARE".to_string()));
        let render = a.relationships.iter().find(|r| r.target == "render").unwrap();
        assert_eq!(render.from_symbol.as_deref(), Some("Widget::draw"));
        let draw = a.relationships.iter().find(|r| r.kind == RelationshipKind::Call && r.target == "draw").unwrap();
        assert_eq!(draw.from_symbol.as_deref(), Some("main"));
    }

    #[test]
    fn chunks_follow_symbols() {
        let a = analyze(spec(), SRC);
        let syms: Vec<Option<String>> = a.chunks.iter().map(|c| c.symbol.clone()).collect();
        assert!(syms.contains(&Some("app".into())));
        assert!(syms.contains(&Some("main".into())));
        assert!(syms.contains(&Some("SQUARE".into())));
        assert_eq!(syms[0], None, "leading include block is a gap chunk");
        // Whitespace-only gaps between items are dropped; everything else is kept.
        let total: usize = a.chunks.iter().map(|c| c.content.len()).sum();
        assert!(total >= SRC.trim().len() * 9 / 10, "chunks cover {total} of {} bytes", SRC.trim().len());
    }
}
