//! Ruby.
//!
//! Top-level `def`s are emitted as `Method` (Ruby has no free functions:
//! they are private methods on `Object`); methods inside a `class` or
//! `module` get it as their `parent` through the container rules.
//! `require` / `require_relative` / `load` / `autoload` are ordinary `call`
//! nodes in the grammar, so imports go through `custom_imports`.

use crate::treesitter::{first_child_of_kind, strip_quotes, ImportRule, LanguageSpec, NameSource, SymbolRule, TargetSource};
use crate::types::{LanguageId, Relationship, RelationshipKind, SymbolKind};
use tree_sitter::Node;

static SYMBOLS: &[SymbolRule] = &[
    SymbolRule::new("method", SymbolKind::Method, NameSource::Field("name")),
    SymbolRule::new("singleton_method", SymbolKind::Method, NameSource::Field("name")),
    SymbolRule::container("class", SymbolKind::Class, NameSource::Field("name")),
    SymbolRule::container("module", SymbolKind::Module, NameSource::Field("name")),
];

static IMPORTS: &[ImportRule] = &[ImportRule { node_kind: "call", relationship: RelationshipKind::Import, target: TargetSource::Custom, confidence: 0.9 }];

/// `require 'x'`, `require_relative 'x'`, `load 'x.rb'`, `autoload :X, 'x'`
/// → the first string argument, unquoted.
fn ruby_imports(node: Node<'_>, source: &str) -> Vec<Relationship> {
    if node.kind() != "call" {
        return Vec::new();
    }
    let Some(method) = node.child_by_field_name("method").and_then(|m| m.utf8_text(source.as_bytes()).ok()) else {
        return Vec::new();
    };
    let confidence = match method {
        "require" | "require_relative" => 0.9,
        "load" | "autoload" => 0.7,
        _ => return Vec::new(),
    };
    let Some(args) = node.child_by_field_name("arguments") else {
        return Vec::new();
    };
    let Some(target) = first_child_of_kind(args, "string").and_then(|s| s.utf8_text(source.as_bytes()).ok()).map(strip_quotes).filter(|t| !t.is_empty()) else {
        return Vec::new();
    };
    vec![Relationship { kind: RelationshipKind::Import, from_symbol: None, target, to_symbol: None, confidence, start_byte: node.start_byte() }]
}

pub fn spec() -> LanguageSpec {
    LanguageSpec {
        id: LanguageId::Ruby,
        language: || tree_sitter_ruby::LANGUAGE.into(),
        extensions: &["rb", "rake", "gemspec", "ru"],
        filenames: &["Gemfile", "Rakefile", "Guardfile"],
        symbols: SYMBOLS,
        imports: IMPORTS,
        calls: &[("call", "method")],
        // The `superclass` node's text is `< Base`; the generic splitter drops the `<`.
        inheritance: &[("class", "superclass", RelationshipKind::Extends)],
        function_scopes: &["method", "singleton_method", "lambda", "block", "do_block"],
        custom_name: None,
        custom_imports: Some(ruby_imports),
        custom_symbol: None,
        test_markers: &["/spec/", "_spec.rb", "/test/", "_test.rb"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::specs::testing::*;

    const SRC: &str = r#"
require 'json'
require_relative '../lib/helpers'
load 'config/settings.rb'
autoload :Widget, 'app/models/widget'

module Billing
  TAX_RATE = 0.2

  class Invoice < ApplicationRecord
    attr_reader :total

    def initialize(total)
      @total = total
      discount = 0
    end

    def self.generate(order)
      invoice = new(order.total)
      invoice.save!
      invoice
    end

    def formatted_total
      format("%.2f", total)
    end

    private

    def apply_tax(amount)
      amount * (1 + TAX_RATE)
    end
  end

  class Receipt < Billing::Invoice
    def print
      puts formatted_total
    end
  end
end

class Reporter
  def run(invoices)
    invoices.each do |inv|
      line = inv.formatted_total
      puts line
    end
    doubler = ->(x) { x * 2 }
    items.map { |i| i.name }
    Billing::Invoice.generate(order)
    JSON.parse(body)
  end
end

def standalone_helper
  Reporter.new.run([])
end
"#;

    #[test]
    fn extracts_symbols_with_parents() {
        let a = analyze(spec(), SRC);
        assert!(a.errors.is_empty(), "{:?}", a.errors);
        assert!(has_symbol(&a, "Billing", SymbolKind::Module));
        assert!(has_symbol(&a, "Invoice", SymbolKind::Class));
        assert!(has_symbol(&a, "Receipt", SymbolKind::Class));
        assert!(has_symbol(&a, "Reporter", SymbolKind::Class));
        assert!(has_symbol(&a, "initialize", SymbolKind::Method));
        assert!(has_symbol(&a, "generate", SymbolKind::Method), "singleton method `def self.generate`");
        assert!(has_symbol(&a, "standalone_helper", SymbolKind::Method));
        assert_eq!(parent_of(&a, "Invoice").as_deref(), Some("Billing"));
        assert_eq!(parent_of(&a, "Receipt").as_deref(), Some("Billing"));
        assert_eq!(parent_of(&a, "initialize").as_deref(), Some("Invoice"));
        assert_eq!(parent_of(&a, "generate").as_deref(), Some("Invoice"));
        assert_eq!(parent_of(&a, "formatted_total").as_deref(), Some("Invoice"));
        assert_eq!(parent_of(&a, "apply_tax").as_deref(), Some("Invoice"));
        assert_eq!(parent_of(&a, "print").as_deref(), Some("Receipt"));
        assert_eq!(parent_of(&a, "run").as_deref(), Some("Reporter"));
        assert_eq!(parent_of(&a, "standalone_helper"), None);
        let n = names(&a);
        assert!(!n.contains(&"discount".to_string()), "local variable must not be a symbol");
        assert!(!n.contains(&"invoice".to_string()));
        assert!(!n.contains(&"line".to_string()));
        assert!(!n.contains(&"doubler".to_string()));
        assert!(!n.contains(&"inv".to_string()), "block parameter must not be a symbol");
        let reporter = a.symbols.iter().find(|s| s.name == "Reporter").unwrap();
        assert_eq!(reporter.signature.as_deref(), Some("class Reporter"));
        assert_eq!(reporter.start_line, 42);
        let generate = a.symbols.iter().find(|s| s.name == "generate").unwrap();
        assert!(generate.signature.as_deref().unwrap().starts_with("def self.generate("));
    }

    #[test]
    fn extracts_imports_inheritance_and_calls() {
        let a = analyze(spec(), SRC);
        let imps = imports(&a);
        assert!(imps.contains(&"json".to_string()));
        assert!(imps.contains(&"../lib/helpers".to_string()));
        assert!(imps.contains(&"config/settings.rb".to_string()));
        assert!(imps.contains(&"app/models/widget".to_string()), "autoload takes the string, not the symbol: {imps:?}");
        assert_eq!(imps.len(), 4, "{imps:?}");
        let json = a.relationships.iter().find(|r| r.target == "json").unwrap();
        assert_eq!(json.kind, RelationshipKind::Import);
        assert!(json.confidence > 0.8);

        let extends: Vec<(Option<String>, String)> = a.relationships.iter().filter(|r| r.kind == RelationshipKind::Extends).map(|r| (r.from_symbol.clone(), r.target.clone())).collect();
        assert!(extends.contains(&(Some("Invoice".into()), "ApplicationRecord".into())), "{extends:?}");
        assert!(extends.contains(&(Some("Receipt".into()), "Billing::Invoice".into())), "{extends:?}");
        assert!(!extends.iter().any(|(_, t)| t.starts_with('<')), "leading `<` must be stripped: {extends:?}");

        let calls = calls(&a);
        assert!(calls.contains(&"new".to_string()));
        assert!(calls.contains(&"save".to_string()), "`save!` loses its bang: {calls:?}");
        assert!(calls.contains(&"format".to_string()));
        assert!(calls.contains(&"puts".to_string()));
        assert!(calls.contains(&"each".to_string()));
        assert!(calls.contains(&"generate".to_string()));
        assert!(calls.contains(&"parse".to_string()));
        assert!(calls.contains(&"run".to_string()));
        let parse = a.relationships.iter().find(|r| r.kind == RelationshipKind::Call && r.target == "parse").unwrap();
        assert_eq!(parse.from_symbol.as_deref(), Some("run"));
        let fmt = a.relationships.iter().find(|r| r.kind == RelationshipKind::Call && r.target == "format").unwrap();
        assert_eq!(fmt.from_symbol.as_deref(), Some("formatted_total"));
        let run = a.relationships.iter().find(|r| r.kind == RelationshipKind::Call && r.target == "run").unwrap();
        assert_eq!(run.from_symbol.as_deref(), Some("standalone_helper"));
    }

    #[test]
    fn chunks_follow_symbols() {
        let a = analyze(spec(), SRC);
        let syms: Vec<Option<String>> = a.chunks.iter().map(|c| c.symbol.clone()).collect();
        assert!(syms.contains(&Some("Billing".into())));
        assert!(syms.contains(&Some("Reporter".into())));
        assert!(syms.contains(&Some("standalone_helper".into())));
        assert_eq!(syms[0], None, "leading require block is a gap chunk");
        assert!(a.chunks[0].content.contains("require 'json'"));
        // Nested classes and methods stay inside their module chunk.
        assert!(!syms.contains(&Some("Invoice".into())));
        assert!(!syms.contains(&Some("run".into())));
        let total: usize = a.chunks.iter().map(|c| c.content.len()).sum();
        assert!(total >= SRC.trim().len() * 9 / 10, "chunks cover {total} of {} bytes", SRC.trim().len());
    }
}
