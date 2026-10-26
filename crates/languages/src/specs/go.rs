//! Go.
//!
//! Methods are emitted by `custom_symbol` as `Receiver.method` (receiver
//! type with any leading `*` and type arguments dropped), so the qualified
//! name carries the type even though a top-level `method_declaration` has no
//! enclosing container to act as its parent. Interface method elements get
//! the interface as their parent through the generic container walk.

use crate::treesitter::{first_child_of_kind, first_descendant_of_kind, ImportRule, LanguageSpec, NameSource, SymbolRule, TargetSource};
use crate::types::{LanguageId, RelationshipKind, SymbolKind};
use tree_sitter::Node;

static SYMBOLS: &[SymbolRule] = &[
    SymbolRule::new("function_declaration", SymbolKind::Function, NameSource::Field("name")),
    SymbolRule::new("method_elem", SymbolKind::Method, NameSource::Field("name")),
    SymbolRule::new("type_alias", SymbolKind::TypeAlias, NameSource::Field("name")),
    SymbolRule::top_level("const_spec", SymbolKind::Constant, NameSource::Field("name")),
    SymbolRule::top_level("var_spec", SymbolKind::Variable, NameSource::Field("name")),
];

static IMPORTS: &[ImportRule] = &[ImportRule { node_kind: "import_spec", relationship: RelationshipKind::Import, target: TargetSource::Field("path"), confidence: 0.9 }];

fn node_text(node: Node<'_>, source: &str) -> Option<String> {
    node.utf8_text(source.as_bytes()).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// `(s *Server)` → `Server`, `(st *Stack[T])` → `Stack`.
fn receiver_type(method: Node<'_>, source: &str) -> Option<String> {
    let receiver = method.child_by_field_name("receiver")?;
    let decl = first_child_of_kind(receiver, "parameter_declaration")?;
    let ty = decl.child_by_field_name("type")?;
    node_text(first_descendant_of_kind(ty, "type_identifier")?, source)
}

fn go_symbol(node: Node<'_>, source: &str) -> Option<(String, SymbolKind)> {
    match node.kind() {
        "method_declaration" => {
            let name = node_text(node.child_by_field_name("name")?, source)?;
            let name = match receiver_type(node, source) {
                Some(recv) => format!("{recv}.{name}"),
                None => name,
            };
            Some((name, SymbolKind::Method))
        }
        "type_spec" => {
            let name = node_text(node.child_by_field_name("name")?, source)?;
            let kind = match node.child_by_field_name("type").map(|t| t.kind()) {
                Some("struct_type") => SymbolKind::Struct,
                Some("interface_type") => SymbolKind::Interface,
                _ => SymbolKind::TypeAlias,
            };
            Some((name, kind))
        }
        _ => None,
    }
}

pub fn spec() -> LanguageSpec {
    LanguageSpec {
        id: LanguageId::Go,
        language: || tree_sitter_go::LANGUAGE.into(),
        extensions: &["go"],
        filenames: &[],
        symbols: SYMBOLS,
        imports: IMPORTS,
        calls: &[("call_expression", "function")],
        inheritance: &[],
        function_scopes: &["function_declaration", "method_declaration", "func_literal"],
        custom_name: None,
        custom_imports: None,
        custom_symbol: Some(go_symbol),
        test_markers: &["_test.go"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::specs::testing::*;

    const SRC: &str = r#"
package server

import (
	"fmt"
	"net/http"
	str "strings"
)

const MaxConns = 10

var defaultName = "server"

var onStart = func() {
	var started = true
	_ = started
}

type Handler interface {
	ServeHTTP(w http.ResponseWriter, r *http.Request)
}

type Server struct {
	name  string
	conns int
}

type Alias = Server

type Port int

func NewServer(name string) *Server {
	local := 1
	var inner = 2
	_ = local + inner
	return &Server{name: name}
}

func (s *Server) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	fmt.Fprintf(w, "hello from %s", s.name)
	s.log(r.URL.Path)
}

func (s Server) log(path string) {
	fmt.Println(str.ToUpper(path))
}

func (p Port) String() string {
	return fmt.Sprintf(":%d", int(p))
}

func main() {
	srv := NewServer(defaultName)
	http.Handle("/", srv)
	onStart()
	fmt.Println(MaxConns, Port(8080).String())
}
"#;

    #[test]
    fn extracts_symbols_with_parents() {
        let a = analyze(spec(), SRC);
        assert!(a.errors.is_empty(), "{:?}", a.errors);
        assert!(has_symbol(&a, "Server", SymbolKind::Struct));
        assert!(has_symbol(&a, "Handler", SymbolKind::Interface));
        assert!(has_symbol(&a, "Alias", SymbolKind::TypeAlias));
        assert!(has_symbol(&a, "Port", SymbolKind::TypeAlias));
        assert!(has_symbol(&a, "NewServer", SymbolKind::Function));
        assert!(has_symbol(&a, "main", SymbolKind::Function));
        assert!(has_symbol(&a, "Server.ServeHTTP", SymbolKind::Method));
        assert!(has_symbol(&a, "Server.log", SymbolKind::Method));
        assert!(has_symbol(&a, "Port.String", SymbolKind::Method));
        assert_eq!(parent_of(&a, "ServeHTTP").as_deref(), Some("Handler"), "interface method element");
        assert!(has_symbol(&a, "MaxConns", SymbolKind::Constant));
        assert!(has_symbol(&a, "defaultName", SymbolKind::Variable));
        assert!(has_symbol(&a, "onStart", SymbolKind::Variable));
        for local in ["local", "inner", "started", "srv"] {
            assert!(!names(&a).contains(&local.to_string()), "{local} must not be a symbol");
        }
        let serve = a.symbols.iter().find(|s| s.name == "Server.ServeHTTP").unwrap();
        assert_eq!(serve.parent, None);
        assert!(serve.signature.as_deref().unwrap().starts_with("func (s *Server) ServeHTTP("));
        let main = a.symbols.iter().find(|s| s.name == "main").unwrap();
        assert!(main.signature.as_deref().unwrap().starts_with("func main()"));
        assert_eq!(main.start_line, 52);
    }

    #[test]
    fn extracts_imports_and_calls() {
        let a = analyze(spec(), SRC);
        let imps = imports(&a);
        assert_eq!(imps, vec!["fmt".to_string(), "net/http".to_string(), "strings".to_string()]);
        let calls = calls(&a);
        for callee in ["NewServer", "Fprintf", "ToUpper", "Handle", "onStart", "Println"] {
            assert!(calls.contains(&callee.to_string()), "missing call to {callee} in {calls:?}");
        }
        let call = a.relationships.iter().find(|r| r.target == "NewServer").unwrap();
        assert_eq!(call.from_symbol.as_deref(), Some("main"));
        let call = a.relationships.iter().find(|r| r.target == "Fprintf").unwrap();
        assert_eq!(call.from_symbol.as_deref(), Some("Server.ServeHTTP"));
    }

    #[test]
    fn chunks_follow_symbols() {
        let a = analyze(spec(), SRC);
        let syms: Vec<Option<String>> = a.chunks.iter().map(|c| c.symbol.clone()).collect();
        assert!(syms.contains(&Some("Server".into())));
        assert!(syms.contains(&Some("Server.ServeHTTP".into())));
        assert!(syms.contains(&Some("main".into())));
        assert_eq!(syms[0], None, "package clause and import block are a gap chunk");
        assert!(a.chunks[0].content.contains("\"net/http\""));
        // Whitespace-only gaps between items are dropped; everything else is kept.
        let total: usize = a.chunks.iter().map(|c| c.content.len()).sum();
        assert!(total >= SRC.trim().len() * 9 / 10, "chunks cover {total} of {} bytes", SRC.trim().len());
    }
}
