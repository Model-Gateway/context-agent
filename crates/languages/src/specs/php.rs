//! PHP.

use crate::treesitter::{first_child_of_kind, first_descendant_of_kind, last_identifier, strip_quotes, ImportRule, LanguageSpec, NameSource, SymbolRule, TargetSource};
use crate::types::{LanguageId, Relationship, RelationshipKind, SymbolKind};
use tree_sitter::Node;

static SYMBOLS: &[SymbolRule] = &[
    SymbolRule::new("function_definition", SymbolKind::Function, NameSource::Field("name")),
    SymbolRule::container("class_declaration", SymbolKind::Class, NameSource::Field("name")),
    SymbolRule::new("method_declaration", SymbolKind::Method, NameSource::Field("name")),
    SymbolRule::container("interface_declaration", SymbolKind::Interface, NameSource::Field("name")),
    SymbolRule::container("trait_declaration", SymbolKind::Trait, NameSource::Field("name")),
    SymbolRule::container("enum_declaration", SymbolKind::Enum, NameSource::Field("name")),
    // A file-level `namespace Foo;` has no body, so the rest of the file is a
    // sibling of this node rather than a child: nothing nests under it. Only
    // the braced form `namespace Foo { ... }` makes the walker record it as
    // the parent of its declarations.
    SymbolRule::container("namespace_definition", SymbolKind::Namespace, NameSource::Field("name")),
    SymbolRule::new("const_declaration", SymbolKind::Constant, NameSource::Custom),
    SymbolRule::new("property_declaration", SymbolKind::Field, NameSource::Custom),
];

/// Every rule here delegates to [`custom_imports`], which decides the
/// relationship kind and confidence itself (the grammar exposes `extends` /
/// `implements` clauses and `new` targets as plain children, not fields).
static IMPORTS: &[ImportRule] = &[
    ImportRule { node_kind: "namespace_use_declaration", relationship: RelationshipKind::Import, target: TargetSource::Custom, confidence: 0.9 },
    ImportRule { node_kind: "require_expression", relationship: RelationshipKind::Include, target: TargetSource::Custom, confidence: 0.8 },
    ImportRule { node_kind: "require_once_expression", relationship: RelationshipKind::Include, target: TargetSource::Custom, confidence: 0.8 },
    ImportRule { node_kind: "include_expression", relationship: RelationshipKind::Include, target: TargetSource::Custom, confidence: 0.8 },
    ImportRule { node_kind: "include_once_expression", relationship: RelationshipKind::Include, target: TargetSource::Custom, confidence: 0.8 },
    ImportRule { node_kind: "class_declaration", relationship: RelationshipKind::Extends, target: TargetSource::Custom, confidence: 0.8 },
    ImportRule { node_kind: "interface_declaration", relationship: RelationshipKind::Extends, target: TargetSource::Custom, confidence: 0.8 },
    ImportRule { node_kind: "enum_declaration", relationship: RelationshipKind::Implements, target: TargetSource::Custom, confidence: 0.8 },
    ImportRule { node_kind: "object_creation_expression", relationship: RelationshipKind::Call, target: TargetSource::Custom, confidence: 0.4 },
];

fn node_text(node: Node<'_>, source: &str) -> Option<String> {
    node.utf8_text(source.as_bytes()).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn named_children<'a>(node: Node<'a>) -> Vec<Node<'a>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

/// `const A = 1;` → `A`; `private string $label;` → `label`.
fn element_name(element: Node<'_>, source: &str) -> Option<String> {
    match element.kind() {
        "const_element" => first_child_of_kind(element, "name").and_then(|n| node_text(n, source)),
        "property_element" => element.child_by_field_name("name").and_then(|v| first_descendant_of_kind(v, "name")).and_then(|n| node_text(n, source)),
        _ => None,
    }
}

/// A declaration is named after its first declarator so the symbol spans the
/// whole statement (`public const X = 1;`), which keeps chunks and
/// signatures intact.
fn declaration_name(node: Node<'_>, source: &str) -> Option<String> {
    let element_kind = match node.kind() {
        "const_declaration" => "const_element",
        "property_declaration" => "property_element",
        _ => return None,
    };
    first_child_of_kind(node, element_kind).and_then(|e| element_name(e, source))
}

/// Extra declarators in one statement (`const A = 1, B = 2;`, `public $x, $y;`):
/// the first is emitted by the declaration rule, the rest here.
fn extra_declarator(node: Node<'_>, source: &str) -> Option<(String, SymbolKind)> {
    let kind = match node.kind() {
        "const_element" => SymbolKind::Constant,
        "property_element" => SymbolKind::Field,
        _ => return None,
    };
    let first = first_child_of_kind(node.parent()?, node.kind())?;
    if first.id() == node.id() {
        return None;
    }
    element_name(node, source).map(|name| (name, kind))
}

fn is_name_node(node: Node<'_>) -> bool {
    matches!(node.kind(), "name" | "qualified_name" | "relative_name")
}

/// `App\Models\User` with any leading `\` removed.
fn qualified(node: Node<'_>, source: &str) -> Option<String> {
    node_text(node, source).map(|t| t.trim_start_matches('\\').to_string()).filter(|t| !t.is_empty())
}

/// Nearest enclosing definition, so a `new Foo()` inside a method is
/// attributed to that method (mirrors the adapter's own `enclosing_symbol`).
fn enclosing_definition(node: Node<'_>, source: &str) -> Option<String> {
    let mut current = node.parent();
    while let Some(n) = current {
        match n.kind() {
            "function_definition" | "method_declaration" | "class_declaration" | "interface_declaration" | "trait_declaration" | "enum_declaration" | "namespace_definition" => {
                return n.child_by_field_name("name").and_then(|name| node_text(name, source));
            }
            "const_declaration" | "property_declaration" => return declaration_name(n, source),
            _ => current = n.parent(),
        }
    }
    None
}

fn import(kind: RelationshipKind, target: String, confidence: f64, node: Node<'_>) -> Relationship {
    Relationship { kind, from_symbol: None, target, to_symbol: None, confidence, start_byte: node.start_byte() }
}

/// `use A\B;`, `use A\B as C, D\E;`, `use A\{B, C as D};`, `use function A\f;`.
fn use_imports(node: Node<'_>, source: &str) -> Vec<Relationship> {
    let mut out = Vec::new();
    let clause_target = |clause: Node<'_>| named_children(clause).into_iter().find(|c| is_name_node(*c)).and_then(|c| qualified(c, source));
    for clause in named_children(node).into_iter().filter(|c| c.kind() == "namespace_use_clause") {
        out.extend(clause_target(clause).map(|t| import(RelationshipKind::Import, t, 0.9, node)));
    }
    if let Some(group) = node.child_by_field_name("body") {
        let prefix = first_child_of_kind(node, "namespace_name").and_then(|n| qualified(n, source)).map(|p| format!("{p}\\")).unwrap_or_default();
        for clause in named_children(group).into_iter().filter(|c| c.kind() == "namespace_use_clause") {
            out.extend(clause_target(clause).map(|t| import(RelationshipKind::Import, format!("{prefix}{t}"), 0.9, node)));
        }
    }
    out
}

/// `require_once __DIR__ . '/vendor/autoload.php';` → `/vendor/autoload.php`.
fn include_target(node: Node<'_>, source: &str) -> Vec<Relationship> {
    let literal = first_descendant_of_kind(node, "string").or_else(|| first_descendant_of_kind(node, "encapsed_string"));
    literal.and_then(|n| node_text(n, source)).map(|t| strip_quotes(&t)).filter(|t| !t.is_empty()).map(|t| import(RelationshipKind::Include, t, 0.8, node)).into_iter().collect()
}

/// `class A extends B implements C, D` → Extends B, Implements C, Implements D.
fn inheritance(node: Node<'_>, source: &str) -> Vec<Relationship> {
    let from = node.child_by_field_name("name").and_then(|n| node_text(n, source));
    let mut out = Vec::new();
    for clause in named_children(node) {
        let kind = match clause.kind() {
            "base_clause" => RelationshipKind::Extends,
            "class_interface_clause" => RelationshipKind::Implements,
            _ => continue,
        };
        for base in named_children(clause).into_iter().filter(|c| is_name_node(*c)) {
            if let Some(name) = node_text(base, source).map(|t| last_identifier(&t)).filter(|n| !n.is_empty()) {
                out.push(Relationship { kind, from_symbol: from.clone(), target: name.clone(), to_symbol: Some(name), confidence: 0.8, start_byte: node.start_byte() });
            }
        }
    }
    out
}

/// `new Foo(...)` → call to `Foo` (the grammar has no field for the class).
fn constructor_call(node: Node<'_>, source: &str) -> Vec<Relationship> {
    let Some(class) = named_children(node).into_iter().find(|c| is_name_node(*c)) else { return Vec::new() };
    let Some(name) = node_text(class, source).map(|t| last_identifier(&t)) else { return Vec::new() };
    if name.len() < 2 || matches!(name.to_ascii_lowercase().as_str(), "self" | "static" | "parent") {
        return Vec::new();
    }
    vec![Relationship { kind: RelationshipKind::Call, from_symbol: enclosing_definition(node, source), target: name.clone(), to_symbol: Some(name), confidence: 0.4, start_byte: node.start_byte() }]
}

fn custom_imports(node: Node<'_>, source: &str) -> Vec<Relationship> {
    match node.kind() {
        "namespace_use_declaration" => use_imports(node, source),
        "require_expression" | "require_once_expression" | "include_expression" | "include_once_expression" => include_target(node, source),
        "class_declaration" | "interface_declaration" | "enum_declaration" => inheritance(node, source),
        "object_creation_expression" => constructor_call(node, source),
        _ => Vec::new(),
    }
}

pub fn spec() -> LanguageSpec {
    LanguageSpec {
        id: LanguageId::Php,
        language: || tree_sitter_php::LANGUAGE_PHP.into(),
        extensions: &["php", "phtml", "php5", "php7", "phps", "inc"],
        filenames: &[],
        symbols: SYMBOLS,
        imports: IMPORTS,
        calls: &[("function_call_expression", "function"), ("member_call_expression", "name"), ("nullsafe_member_call_expression", "name"), ("scoped_call_expression", "name")],
        inheritance: &[],
        function_scopes: &["function_definition", "method_declaration", "anonymous_function", "arrow_function"],
        custom_name: Some(declaration_name),
        custom_imports: Some(custom_imports),
        custom_symbol: Some(extra_declarator),
        // Paths are lower-cased before matching, so PHPUnit's `FooTest.php`
        // convention is matched as `test.php`.
        test_markers: &["/tests/", "test.php"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::specs::testing::*;

    const SRC: &str = r#"<?php

namespace App\Services;

use App\Models\User;
use App\Contracts\{Renderable, Themeable as Skin};
use Psr\Log\LoggerInterface as Logger;

require_once __DIR__ . '/../helpers.php';

const MAX_ATTEMPTS = 10;

interface Clickable extends Renderable
{
    public function click(): void;
}

trait Loggable
{
    public function log(string $msg): void
    {
        error_log($msg);
    }
}

enum State: string implements Skin
{
    case Idle = 'idle';
    case Loading = 'loading';
}

abstract class Widget
{
    abstract public function render(): string;
}

class LoginButton extends Widget implements Clickable, Skin
{
    use Loggable;

    public const DEFAULT_LABEL = 'Log in', ALT_LABEL = 'Sign in';

    private string $label;
    protected static int $count = 0;

    public function __construct(string $label = self::DEFAULT_LABEL)
    {
        $local = strtoupper($label);
        $this->label = $local;
        self::$count++;
    }

    public function render(): string
    {
        return sprintf('<button>%s</button>', $this->label);
    }

    public function click(): void
    {
        $user = User::find(1);
        $this->log($user?->name());
    }
}

function make_button(string $label): LoginButton
{
    $button = new LoginButton($label);
    $double = fn($x) => $x * 2;
    $button->render();
    return $button;
}
"#;

    #[test]
    fn extracts_symbols_with_parents() {
        let a = analyze(spec(), SRC);
        assert!(a.errors.is_empty(), "{:?}", a.errors);
        assert!(has_symbol(&a, "App\\Services", SymbolKind::Namespace));
        assert!(has_symbol(&a, "MAX_ATTEMPTS", SymbolKind::Constant));
        assert!(has_symbol(&a, "Clickable", SymbolKind::Interface));
        assert!(has_symbol(&a, "Loggable", SymbolKind::Trait));
        assert!(has_symbol(&a, "State", SymbolKind::Enum));
        assert!(has_symbol(&a, "Widget", SymbolKind::Class));
        assert!(has_symbol(&a, "LoginButton", SymbolKind::Class));
        assert!(has_symbol(&a, "__construct", SymbolKind::Method));
        assert!(has_symbol(&a, "make_button", SymbolKind::Function));
        assert_eq!(parent_of(&a, "__construct").as_deref(), Some("LoginButton"));
        assert_eq!(parent_of(&a, "click").as_deref(), Some("Clickable"));
        assert_eq!(parent_of(&a, "log").as_deref(), Some("Loggable"));
        let renders: Vec<Option<String>> = a.symbols.iter().filter(|s| s.name == "render").map(|s| s.parent.clone()).collect();
        assert_eq!(renders, vec![Some("Widget".into()), Some("LoginButton".into())]);
        // Class constants and properties, including a second declarator.
        assert!(has_symbol(&a, "DEFAULT_LABEL", SymbolKind::Constant));
        assert!(has_symbol(&a, "ALT_LABEL", SymbolKind::Constant));
        assert_eq!(parent_of(&a, "ALT_LABEL").as_deref(), Some("LoginButton"));
        assert!(has_symbol(&a, "label", SymbolKind::Field));
        assert!(has_symbol(&a, "count", SymbolKind::Field));
        assert_eq!(parent_of(&a, "label").as_deref(), Some("LoginButton"));
        // A body-less `namespace X;` never becomes a parent.
        assert!(a.symbols.iter().all(|s| s.parent.as_deref() != Some("App\\Services")));
        // Locals, parameters and enum cases are not symbols.
        for local in ["local", "$local", "user", "button", "double", "msg", "Idle"] {
            assert!(!names(&a).contains(&local.to_string()), "{local} should not be a symbol");
        }
        let make = a.symbols.iter().find(|s| s.name == "make_button").unwrap();
        assert!(make.signature.as_deref().unwrap().starts_with("function make_button("));
        assert_eq!(make.start_line, 65);
        let max = a.symbols.iter().find(|s| s.name == "MAX_ATTEMPTS").unwrap();
        assert_eq!(max.signature.as_deref(), Some("const MAX_ATTEMPTS = 10;"));
    }

    #[test]
    fn extracts_imports_inheritance_and_calls() {
        let a = analyze(spec(), SRC);
        let imps = imports(&a);
        assert!(imps.contains(&"App\\Models\\User".to_string()));
        assert!(imps.contains(&"App\\Contracts\\Renderable".to_string()));
        assert!(imps.contains(&"App\\Contracts\\Themeable".to_string()), "{imps:?}");
        assert!(imps.contains(&"Psr\\Log\\LoggerInterface".to_string()));
        let include = a.relationships.iter().find(|r| r.kind == RelationshipKind::Include).unwrap();
        assert_eq!(include.target, "/../helpers.php");

        let rels = |kind: RelationshipKind| -> Vec<(Option<String>, String)> { a.relationships.iter().filter(|r| r.kind == kind).map(|r| (r.from_symbol.clone(), r.target.clone())).collect() };
        let extends = rels(RelationshipKind::Extends);
        assert!(extends.contains(&(Some("LoginButton".into()), "Widget".into())));
        assert!(extends.contains(&(Some("Clickable".into()), "Renderable".into())));
        let implements = rels(RelationshipKind::Implements);
        assert!(implements.contains(&(Some("LoginButton".into()), "Clickable".into())));
        assert!(implements.contains(&(Some("LoginButton".into()), "Skin".into())));
        assert!(implements.contains(&(Some("State".into()), "Skin".into())));

        let calls = calls(&a);
        for target in ["error_log", "strtoupper", "sprintf", "find", "log", "name", "LoginButton", "render"] {
            assert!(calls.contains(&target.to_string()), "missing call to {target}: {calls:?}");
        }
        let new_call = a.relationships.iter().find(|r| r.kind == RelationshipKind::Call && r.target == "LoginButton").unwrap();
        assert_eq!(new_call.from_symbol.as_deref(), Some("make_button"));
        let find = a.relationships.iter().find(|r| r.target == "find").unwrap();
        assert_eq!(find.from_symbol.as_deref(), Some("click"));
        let error_log = a.relationships.iter().find(|r| r.target == "error_log").unwrap();
        assert_eq!(error_log.from_symbol.as_deref(), Some("log"));
    }

    #[test]
    fn chunks_follow_symbols() {
        let a = analyze(spec(), SRC);
        let syms: Vec<Option<String>> = a.chunks.iter().map(|c| c.symbol.clone()).collect();
        assert!(syms.contains(&Some("App\\Services".into())));
        assert!(syms.contains(&Some("Clickable".into())));
        assert!(syms.contains(&Some("LoginButton".into())));
        assert!(syms.contains(&Some("make_button".into())));
        assert_eq!(syms[0], None, "php open tag is a gap chunk");
        let uses = a.chunks.iter().find(|c| c.content.contains("use App\\Models\\User;")).unwrap();
        assert_eq!(uses.symbol, None, "leading use block is a gap chunk");
        assert!(uses.content.contains("require_once"));
        // Methods stay inside their class chunk.
        assert!(!syms.contains(&Some("__construct".into())));
        let total: usize = a.chunks.iter().map(|c| c.content.len()).sum();
        assert!(total >= SRC.trim().len() * 9 / 10, "chunks cover {total} of {} bytes", SRC.trim().len());
    }

    #[test]
    fn braced_namespace_nests_and_test_paths_match() {
        let src = "<?php\nnamespace App\\Legacy {\n    const VERSION = '1.0';\n    class Helper {\n        public function run(): void {}\n    }\n}\n";
        let a = analyze(spec(), src);
        assert!(a.errors.is_empty(), "{:?}", a.errors);
        assert_eq!(parent_of(&a, "VERSION").as_deref(), Some("App\\Legacy"));
        assert_eq!(parent_of(&a, "Helper").as_deref(), Some("App\\Legacy"));
        assert_eq!(parent_of(&a, "run").as_deref(), Some("Helper"));

        use crate::LanguageAdapter;
        let adapter = crate::treesitter::TreeSitterAdapter::new(spec());
        assert!(adapter.is_test_path("tests/Unit/LoginButtonTest.php"));
        assert!(adapter.is_test_path("src/Services/LoginButtonTest.php"));
        assert!(!adapter.is_test_path("src/Services/LoginButton.php"));
        assert_eq!(adapter.extensions(), &["php", "phtml", "php5", "php7", "phps", "inc"]);
    }
}
