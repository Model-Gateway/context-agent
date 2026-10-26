//! C.

use crate::treesitter::{ImportRule, LanguageSpec, NameSource, SymbolRule, TargetSource};
use crate::types::{LanguageId, RelationshipKind, SymbolKind};
use tree_sitter::Node;

static SYMBOLS: &[SymbolRule] = &[
    // `declarator` nests pointer_declarator / function_declarator around the name.
    SymbolRule::new("function_definition", SymbolKind::Function, NameSource::FieldDescendant("declarator", "identifier")),
    SymbolRule::new("type_definition", SymbolKind::TypeAlias, NameSource::FieldDescendant("declarator", "type_identifier")),
    SymbolRule::new("preproc_def", SymbolKind::Macro, NameSource::Field("name")),
    SymbolRule::new("preproc_function_def", SymbolKind::Macro, NameSource::Field("name")),
    // Globals only; declarations inside function bodies are locals.
    SymbolRule::top_level("declaration", SymbolKind::Variable, NameSource::Custom),
];

static IMPORTS: &[ImportRule] = &[ImportRule { node_kind: "preproc_include", relationship: RelationshipKind::Include, target: TargetSource::Field("path"), confidence: 0.9 }];

/// Named aggregates with a body are definitions; forward declarations
/// (`struct Foo;`) and anonymous aggregates are not symbols.
fn aggregate_symbol(node: Node<'_>, source: &str) -> Option<(String, SymbolKind)> {
    let kind = match node.kind() {
        "struct_specifier" | "union_specifier" => SymbolKind::Struct,
        "enum_specifier" => SymbolKind::Enum,
        _ => return None,
    };
    node.child_by_field_name("body")?;
    let name = node.child_by_field_name("name")?.utf8_text(source.as_bytes()).ok()?.trim().to_string();
    if name.is_empty() {
        return None;
    }
    Some((name, kind))
}

/// Name of the variable a `declaration` introduces (first declarator only).
fn declaration_name(node: Node<'_>, source: &str) -> Option<String> {
    if node.kind() != "declaration" {
        return None;
    }
    let declarator = node.child_by_field_name("declarator")?;
    let ident = declared_identifier(declarator, false)?;
    ident.utf8_text(source.as_bytes()).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Walks a declarator chain down to the declared identifier. A
/// `function_declarator` is a prototype (`int f(int);`), not a variable,
/// unless an initialiser proves otherwise (`int (*fp)(int) = cb;`).
fn declared_identifier(node: Node<'_>, allow_function: bool) -> Option<Node<'_>> {
    match node.kind() {
        "identifier" => Some(node),
        "init_declarator" => declared_identifier(node.child_by_field_name("declarator")?, true),
        "function_declarator" if allow_function => declared_identifier(node.child_by_field_name("declarator")?, true),
        "pointer_declarator" | "array_declarator" | "parenthesized_declarator" | "attributed_declarator" => {
            let inner = node.child_by_field_name("declarator").or_else(|| {
                let mut cursor = node.walk();
                let first = node.named_children(&mut cursor).next();
                first
            })?;
            declared_identifier(inner, allow_function)
        }
        _ => None,
    }
}

pub fn spec() -> LanguageSpec {
    LanguageSpec {
        id: LanguageId::C,
        language: || tree_sitter_c::LANGUAGE.into(),
        extensions: &["c", "h"],
        filenames: &[],
        symbols: SYMBOLS,
        imports: IMPORTS,
        calls: &[("call_expression", "function")],
        inheritance: &[],
        function_scopes: &["function_definition"],
        custom_name: Some(declaration_name),
        custom_imports: None,
        custom_symbol: Some(aggregate_symbol),
        test_markers: &["/test/", "/tests/", "_test.c", "test_"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::specs::testing::*;

    const SRC: &str = r#"
#include <stdio.h>
#include <stdlib.h>
#include "registry.h"

#define MAX_USERS 64
#define SQUARE(x) ((x) * (x))

struct Session;

typedef unsigned long user_id_t;
typedef int (*compare_fn)(const void *, const void *);

typedef struct Point {
    int x;
    int y;
} Point;

struct User {
    user_id_t id;
    char name[32];
    struct Address {
        char city[16];
    } address;
};

union Value {
    int i;
    float f;
};

enum Color { RED, GREEN, BLUE };

static int user_count = 0;
int debug_level;
char *registry_name;

static int compare_ids(const void *a, const void *b);

struct User *find_user(struct User *users, user_id_t id) {
    int index = 0;
    struct User *found = NULL;
    for (index = 0; index < user_count; index++) {
        if (users[index].id == id) {
            found = &users[index];
        }
    }
    return found;
}

static int compare_ids(const void *a, const void *b) {
    const struct User *ua = a;
    const struct User *ub = b;
    return (int)(ua->id - ub->id);
}

int main(void) {
    struct User users[MAX_USERS];
    qsort(users, user_count, sizeof(struct User), compare_ids);
    find_user(users, 7);
    printf("%d\n", SQUARE(user_count));
    return 0;
}
"#;

    #[test]
    fn extracts_symbols_with_parents() {
        let a = analyze(spec(), SRC);
        assert!(a.errors.is_empty(), "{:?}", a.errors);
        assert!(has_symbol(&a, "MAX_USERS", SymbolKind::Macro));
        assert!(has_symbol(&a, "SQUARE", SymbolKind::Macro));
        assert!(has_symbol(&a, "user_id_t", SymbolKind::TypeAlias));
        assert!(has_symbol(&a, "compare_fn", SymbolKind::TypeAlias));
        assert!(has_symbol(&a, "Point", SymbolKind::TypeAlias));
        assert!(has_symbol(&a, "Point", SymbolKind::Struct));
        assert!(has_symbol(&a, "User", SymbolKind::Struct));
        assert!(has_symbol(&a, "Address", SymbolKind::Struct));
        assert_eq!(parent_of(&a, "Address").as_deref(), Some("User"));
        assert!(has_symbol(&a, "Value", SymbolKind::Struct));
        assert!(has_symbol(&a, "Color", SymbolKind::Enum));
        assert!(has_symbol(&a, "user_count", SymbolKind::Variable));
        assert!(has_symbol(&a, "debug_level", SymbolKind::Variable));
        assert!(has_symbol(&a, "registry_name", SymbolKind::Variable));
        assert!(has_symbol(&a, "find_user", SymbolKind::Function));
        assert!(has_symbol(&a, "compare_ids", SymbolKind::Function));
        assert!(has_symbol(&a, "main", SymbolKind::Function));
        assert_eq!(parent_of(&a, "find_user"), None);
        // Forward declarations, prototypes, parameters and locals are not symbols.
        assert!(!names(&a).contains(&"Session".to_string()));
        assert!(!has_symbol(&a, "compare_ids", SymbolKind::Variable));
        assert!(!names(&a).contains(&"index".to_string()));
        assert!(!names(&a).contains(&"found".to_string()));
        assert!(!names(&a).contains(&"users".to_string()));
        assert!(!names(&a).contains(&"ua".to_string()));
        let main = a.symbols.iter().find(|s| s.name == "main").unwrap();
        assert!(main.signature.as_deref().unwrap().starts_with("int main(void)"));
        assert_eq!(main.start_line, 57);
    }

    #[test]
    fn extracts_includes_and_calls() {
        let a = analyze(spec(), SRC);
        let imps = imports(&a);
        assert!(imps.contains(&"stdio.h".to_string()));
        assert!(imps.contains(&"stdlib.h".to_string()));
        assert!(imps.contains(&"registry.h".to_string()));
        assert!(a.relationships.iter().filter(|r| r.kind == RelationshipKind::Include).count() == 3);
        let calls = calls(&a);
        assert!(calls.contains(&"qsort".to_string()));
        assert!(calls.contains(&"find_user".to_string()));
        assert!(calls.contains(&"printf".to_string()));
        assert!(calls.contains(&"SQUARE".to_string()));
        let call = a.relationships.iter().find(|r| r.target == "find_user").unwrap();
        assert_eq!(call.from_symbol.as_deref(), Some("main"));
    }

    #[test]
    fn chunks_follow_symbols() {
        let a = analyze(spec(), SRC);
        let syms: Vec<Option<String>> = a.chunks.iter().map(|c| c.symbol.clone()).collect();
        assert!(syms.contains(&Some("User".into())));
        assert!(syms.contains(&Some("find_user".into())));
        assert!(syms.contains(&Some("main".into())));
        assert_eq!(syms[0], None, "leading include block is a gap chunk");
        // Whitespace-only gaps between items are dropped; everything else is kept.
        let total: usize = a.chunks.iter().map(|c| c.content.len()).sum();
        assert!(total >= SRC.trim().len() * 9 / 10, "chunks cover {total} of {} bytes", SRC.trim().len());
    }
}
