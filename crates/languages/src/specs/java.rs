//! Java.

use crate::treesitter::{first_child_of_kind, ImportRule, LanguageSpec, NameSource, SymbolRule, TargetSource};
use crate::types::{LanguageId, Relationship, RelationshipKind, SymbolKind};
use tree_sitter::Node;

static SYMBOLS: &[SymbolRule] = &[
    SymbolRule::container("class_declaration", SymbolKind::Class, NameSource::Field("name")),
    SymbolRule::container("interface_declaration", SymbolKind::Interface, NameSource::Field("name")),
    SymbolRule::container("enum_declaration", SymbolKind::Enum, NameSource::Field("name")),
    SymbolRule::container("record_declaration", SymbolKind::Class, NameSource::Field("name")),
    SymbolRule::new("annotation_type_declaration", SymbolKind::Interface, NameSource::Field("name")),
    SymbolRule::new("method_declaration", SymbolKind::Method, NameSource::Field("name")),
    SymbolRule::new("constructor_declaration", SymbolKind::Method, NameSource::Field("name")),
    // `private final List<String> lines = ...;` → `declarator` → first `identifier` is the name.
    SymbolRule::new("field_declaration", SymbolKind::Field, NameSource::FieldDescendant("declarator", "identifier")),
];

static IMPORTS: &[ImportRule] = &[
    // `import_declaration` has no fields; see `custom_relationships`.
    ImportRule { node_kind: "import_declaration", relationship: RelationshipKind::Import, target: TargetSource::Custom, confidence: 0.9 },
    // `extends_interfaces` is an unfielded child of `interface_declaration`, so the
    // generic inheritance table cannot reach it; handled in `custom_relationships`.
    ImportRule { node_kind: "interface_declaration", relationship: RelationshipKind::Extends, target: TargetSource::Custom, confidence: 0.8 },
];

fn node_text(node: Node<'_>, source: &str) -> Option<String> {
    node.utf8_text(source.as_bytes()).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// `import a.b.C;` → `a.b.C`, `import a.b.*;` → `a.b.*`,
/// `import static a.b.C.m;` → `static a.b.C.m`.
fn import_relationship(node: Node<'_>, source: &str) -> Option<Relationship> {
    let mut is_static = false;
    let mut wildcard = false;
    let mut path: Option<String> = None;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "static" => is_static = true,
            "asterisk" => wildcard = true,
            "identifier" | "scoped_identifier" => path = node_text(child, source),
            _ => {}
        }
    }
    let mut target = path?;
    if wildcard {
        target.push_str(".*");
    }
    if is_static {
        target = format!("static {target}");
    }
    Some(Relationship { kind: RelationshipKind::Import, from_symbol: None, target, to_symbol: None, confidence: 0.9, start_byte: node.start_byte() })
}

/// `interface A extends B, C<T>` → Extends B, Extends C (from A).
fn interface_extends(node: Node<'_>, source: &str) -> Vec<Relationship> {
    let from = node.child_by_field_name("name").and_then(|n| node_text(n, source));
    let Some(list) = first_child_of_kind(node, "extends_interfaces").and_then(|n| first_child_of_kind(n, "type_list")) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut cursor = list.walk();
    for ty in list.children(&mut cursor) {
        if !ty.is_named() {
            continue;
        }
        // `generic_type` → its leading `type_identifier` / `scoped_type_identifier`.
        let base = if ty.kind() == "generic_type" { ty.named_child(0).unwrap_or(ty) } else { ty };
        let Some(text) = node_text(base, source) else { continue };
        let name = text.rsplit('.').next().unwrap_or(&text).to_string();
        out.push(Relationship { kind: RelationshipKind::Extends, from_symbol: from.clone(), target: name.clone(), to_symbol: Some(name), confidence: 0.8, start_byte: node.start_byte() });
    }
    out
}

fn custom_relationships(node: Node<'_>, source: &str) -> Vec<Relationship> {
    match node.kind() {
        "import_declaration" => import_relationship(node, source).into_iter().collect(),
        "interface_declaration" => interface_extends(node, source),
        _ => Vec::new(),
    }
}

pub fn spec() -> LanguageSpec {
    LanguageSpec {
        id: LanguageId::Java,
        language: || tree_sitter_java::LANGUAGE.into(),
        extensions: &["java"],
        filenames: &[],
        symbols: SYMBOLS,
        imports: IMPORTS,
        calls: &[("method_invocation", "name"), ("object_creation_expression", "type")],
        inheritance: &[
            ("class_declaration", "superclass", RelationshipKind::Extends),
            ("class_declaration", "interfaces", RelationshipKind::Implements),
            ("enum_declaration", "interfaces", RelationshipKind::Implements),
            ("record_declaration", "interfaces", RelationshipKind::Implements),
        ],
        function_scopes: &["method_declaration", "constructor_declaration", "lambda_expression"],
        custom_name: None,
        custom_imports: Some(custom_relationships),
        custom_symbol: None,
        // `is_test_path` lower-cases the path before matching, so markers are lower-case.
        test_markers: &["/src/test/", "test.java", "tests.java"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::specs::testing::*;
    use crate::LanguageAdapter;

    const SRC: &str = r#"
package com.example.billing;

import java.util.ArrayList;
import java.util.List;
import java.io.*;
import static java.util.Objects.requireNonNull;

/** An invoice. */
public class Invoice extends Document implements Printable, Serializable {
    private static final int MAX_LINES = 100;
    private final List<String> lines = new ArrayList<>();
    protected String customer;

    public Invoice(String customer) {
        this.customer = requireNonNull(customer);
    }

    @Override
    public void print(Printer printer) {
        int local = lines.size();
        String header = formatHeader(local);
        printer.write(header);
        lines.forEach(line -> {
            String padded = "  " + line;
            printer.write(padded);
        });
    }

    private String formatHeader(int count) {
        return customer + " (" + count + ")";
    }

    public static class Builder {
        private String customer;
        public Invoice build() { return new Invoice(customer); }
    }
}

interface Printable extends Closeable {
    void print(Printer printer);
}

enum Status implements Serializable { DRAFT, SENT, PAID }

record LineItem(String sku, int quantity) implements Comparable<LineItem> {
    public int total() { return quantity; }
}

@interface Audited {
    String value() default "";
}
"#;

    #[test]
    fn extracts_symbols_with_parents() {
        let a = analyze(spec(), SRC);
        assert!(a.errors.is_empty(), "{:?}", a.errors);
        assert!(has_symbol(&a, "Invoice", SymbolKind::Class));
        assert!(has_symbol(&a, "Invoice", SymbolKind::Method), "constructor is a method");
        assert!(has_symbol(&a, "Builder", SymbolKind::Class));
        assert!(has_symbol(&a, "Printable", SymbolKind::Interface));
        assert!(has_symbol(&a, "Status", SymbolKind::Enum));
        assert!(has_symbol(&a, "LineItem", SymbolKind::Class));
        assert!(has_symbol(&a, "Audited", SymbolKind::Interface));
        assert!(has_symbol(&a, "MAX_LINES", SymbolKind::Field));
        assert!(has_symbol(&a, "lines", SymbolKind::Field));
        assert!(has_symbol(&a, "print", SymbolKind::Method));
        assert_eq!(parent_of(&a, "print").as_deref(), Some("Invoice"));
        assert_eq!(parent_of(&a, "formatHeader").as_deref(), Some("Invoice"));
        assert_eq!(parent_of(&a, "lines").as_deref(), Some("Invoice"));
        assert_eq!(parent_of(&a, "Builder").as_deref(), Some("Invoice"));
        assert_eq!(parent_of(&a, "build").as_deref(), Some("Builder"));
        assert_eq!(parent_of(&a, "total").as_deref(), Some("LineItem"));
        assert_eq!(parent_of(&a, "Printable"), None);
        let ctor = a.symbols.iter().find(|s| s.name == "Invoice" && s.kind == SymbolKind::Method).unwrap();
        assert_eq!(ctor.parent.as_deref(), Some("Invoice"));
        let iface_print = a.symbols.iter().filter(|s| s.name == "print").find(|s| s.parent.as_deref() == Some("Printable"));
        assert!(iface_print.is_some(), "interface method gets the interface as parent");
        // Locals (including inside lambdas) never become symbols.
        for local in ["local", "header", "padded", "line", "printer", "count"] {
            assert!(!names(&a).contains(&local.to_string()), "{local} leaked as a symbol");
        }
        let cls = a.symbols.iter().find(|s| s.name == "Invoice" && s.kind == SymbolKind::Class).unwrap();
        assert!(cls.signature.as_deref().unwrap().starts_with("public class Invoice extends Document"));
        assert_eq!(cls.start_line, 10);
    }

    #[test]
    fn extracts_imports_inheritance_and_calls() {
        let a = analyze(spec(), SRC);
        let imps = imports(&a);
        assert_eq!(imps, vec!["java.util.ArrayList", "java.util.List", "java.io.*", "static java.util.Objects.requireNonNull"]);

        let rel = |kind: RelationshipKind, target: &str| a.relationships.iter().find(|r| r.kind == kind && r.target == target);
        let ext = rel(RelationshipKind::Extends, "Document").expect("class extends");
        assert_eq!(ext.from_symbol.as_deref(), Some("Invoice"));
        assert_eq!(rel(RelationshipKind::Implements, "Printable").unwrap().from_symbol.as_deref(), Some("Invoice"));
        assert_eq!(rel(RelationshipKind::Implements, "Serializable").unwrap().from_symbol.as_deref(), Some("Invoice"));
        let iface = rel(RelationshipKind::Extends, "Closeable").expect("interface extends");
        assert_eq!(iface.from_symbol.as_deref(), Some("Printable"));
        assert_eq!(iface.to_symbol.as_deref(), Some("Closeable"));
        let rec = rel(RelationshipKind::Implements, "Comparable").expect("record implements (generic stripped)");
        assert_eq!(rec.from_symbol.as_deref(), Some("LineItem"));
        assert!(a.relationships.iter().any(|r| r.kind == RelationshipKind::Implements && r.target == "Serializable" && r.from_symbol.as_deref() == Some("Status")));

        let calls = calls(&a);
        for expected in ["requireNonNull", "size", "formatHeader", "write", "forEach", "ArrayList", "Invoice"] {
            assert!(calls.contains(&expected.to_string()), "missing call {expected} in {calls:?}");
        }
        let call = a.relationships.iter().find(|r| r.kind == RelationshipKind::Call && r.target == "formatHeader").unwrap();
        assert_eq!(call.from_symbol.as_deref(), Some("print"));
        let ctor_call = a.relationships.iter().find(|r| r.kind == RelationshipKind::Call && r.target == "Invoice" && r.from_symbol.as_deref() == Some("build"));
        assert!(ctor_call.is_some(), "`new Invoice(...)` inside Builder.build is a call from build");
    }

    #[test]
    fn chunks_follow_symbols() {
        let a = analyze(spec(), SRC);
        let syms: Vec<Option<String>> = a.chunks.iter().map(|c| c.symbol.clone()).collect();
        assert_eq!(syms[0], None, "package + import block is a gap chunk");
        for top in ["Invoice", "Printable", "Status", "LineItem", "Audited"] {
            assert!(syms.contains(&Some(top.into())), "missing chunk for {top} in {syms:?}");
        }
        assert!(!syms.contains(&Some("Builder".into())), "nested class stays inside its outer class chunk");
        let total: usize = a.chunks.iter().map(|c| c.content.len()).sum();
        assert!(total >= SRC.trim().len() * 9 / 10, "chunks cover {total} of {} bytes", SRC.trim().len());
    }

    #[test]
    fn recognises_test_paths() {
        let adapter = crate::treesitter::TreeSitterAdapter::new(spec());
        assert!(adapter.is_test_path("app/src/test/java/com/example/InvoiceTest.java"));
        assert!(adapter.is_test_path("app/src/main/java/com/example/InvoiceTests.java"));
        assert!(adapter.is_test_path("app/src/main/java/com/example/InvoiceTest.java"));
        assert!(!adapter.is_test_path("app/src/main/java/com/example/Invoice.java"));
    }
}
