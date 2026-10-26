//! Every initial-language fixture indexes structurally and retrieves.

use ctx_agent::RepositoryContext;
use ctx_integration_tests::{fixture, test_config};
use ctx_retrieval::RetrievalRequest;
use ctx_storage::SymbolMatchMode;

fn index(name: &str) -> (tempfile::TempDir, std::sync::Arc<RepositoryContext>) {
    let data = tempfile::tempdir().unwrap();
    let ctx = RepositoryContext::open_ephemeral(&fixture(name), test_config(data.path())).unwrap();
    let report = ctx.ensure_indexed(None).unwrap();
    assert!(report.errors.is_empty(), "{name}: {:?}", report.errors);
    (data, ctx)
}

fn has_symbol(ctx: &RepositoryContext, name: &str) -> bool {
    !ctx.stores.index.find_symbols(name, SymbolMatchMode::Exact, 5).unwrap().is_empty()
}

#[test]
fn all_fixtures_index_with_symbols_and_relationships() {
    let expectations: &[(&str, &str, &[&str])] = &[
        ("ts-web-app", "tsx", &["LoginButton", "Spinner", "Footer", "useAuth"]),
        ("py-service", "python", &["apply_discount"]),
        ("rust-cli", "rust", &["greet", "shout"]),
        ("go-service", "go", &["main"]),
        ("java-lib", "java", &["Calculator", "Parser"]),
        ("c-lib", "c", &[]),
        ("cpp-lib", "cpp", &["Matrix"]),
        ("ruby-gem", "ruby", &["Slug"]),
        ("php-app", "php", &["Cart", "Money"]),
        ("polyglot", "python", &["format_price", "fetchProducts"]),
    ];
    for (name, language, symbols) in expectations {
        let (_data, ctx) = index(name);
        let stats = ctx.stores.index.stats().unwrap();
        assert!(stats.files > 0 && stats.chunks > 0, "{name}: {stats:?}");
        assert!(stats.symbols > 0, "{name}: no symbols extracted ({language})");
        assert!(stats.languages.iter().any(|(l, _)| l == language), "{name}: language {language} not detected in {:?}", stats.languages);
        for s in *symbols {
            assert!(has_symbol(&ctx, s), "{name}: expected symbol `{s}`");
        }
        assert!(stats.relationships > 0, "{name}: no relationships");
        assert!(stats.embeddings == stats.chunks, "{name}: embeddings {} != chunks {}", stats.embeddings, stats.chunks);
        let profile = ctx.profile().unwrap();
        assert!(!profile.validation.is_empty(), "{name}: no validation command detected");
    }
}

#[test]
fn explicit_root_inside_a_git_repository_is_used_as_is() {
    let outer = tempfile::tempdir().unwrap();
    let ok = std::process::Command::new("git").args(["init", "-q"]).current_dir(outer.path()).status().map(|s| s.success()).unwrap_or(false);
    if !ok {
        eprintln!("git unavailable; skipping");
        return;
    }
    let sub = outer.path().join("packages/app");
    std::fs::create_dir_all(sub.join("src")).unwrap();
    std::fs::write(sub.join("src/main.py"), "def main():\n    return 1\n").unwrap();
    std::fs::write(outer.path().join("other.py"), "def other():\n    return 2\n").unwrap();
    let data = tempfile::tempdir().unwrap();
    let ctx = RepositoryContext::open_ephemeral(&sub, test_config(data.path())).unwrap();
    ctx.ensure_indexed(None).unwrap();
    let files: Vec<String> = ctx.stores.index.list_files().unwrap().into_iter().map(|f| f.path).collect();
    assert_eq!(files, vec!["src/main.py".to_string()], "only the named directory is indexed, with paths relative to it");
}

#[test]
fn polyglot_has_two_format_price_definitions() {
    let (_data, ctx) = index("polyglot");
    let defs = ctx.stores.index.find_symbols("format_price", SymbolMatchMode::Exact, 10).unwrap();
    let paths: Vec<&str> = defs.iter().map(|d| d.path.as_str()).collect();
    assert!(paths.iter().any(|p| p.ends_with(".py")) && paths.iter().any(|p| p.ends_with(".mjs")), "{paths:?}");
}

#[tokio::test]
async fn retrieval_targets_the_right_file_per_language() {
    let cases = [
        ("ts-web-app", "Add a loading spinner to the login button while authentication is in progress", "src/components/LoginButton.tsx"),
        ("py-service", "Fix the apply_discount percentage bug", "svc/pricing.py"),
        ("rust-cli", "Make greet handle empty names", "src/commands.rs"),
        ("java-lib", "Calculator should support exponent", "src/main/java/com/example/calc/Calculator.java"),
        ("ruby-gem", "Slug.generate should handle accented characters", "lib/textkit/slug.rb"),
        ("php-app", "Cart total should apply Discount", "src/Cart.php"),
    ];
    for (name, query, expected) in cases {
        let (_data, ctx) = index(name);
        let result = ctx.retrieval.retrieve(RetrievalRequest::new(query, 4000)).await.unwrap();
        let paths = result.paths();
        assert!(paths.iter().take(3).any(|p| p == expected), "{name}: {query:?} → {paths:?} (confidence {:?})", result.confidence);
    }
}

#[tokio::test]
async fn prompt_injection_in_source_stays_data() {
    let (_data, ctx) = index("ts-web-app");
    let result = ctx.retrieval.retrieve(RetrievalRequest::new("api client request helper", 4000)).await.unwrap();
    let payload = ctx_core::ContextPayload {
        sections: result
            .context
            .iter()
            .map(|s| ctx_core::ContextSection { kind: ctx_core::ContextSectionKind::RepositorySource, trust: ctx_core::TrustLevel::Untrusted, title: s.source.path.clone(), body: s.content.clone(), rationale: None })
            .collect(),
    };
    let rendered = payload.render();
    if rendered.contains("ignore all previous instructions") {
        assert!(rendered.starts_with(ctx_core::ContextPayload::PREAMBLE));
        assert!(rendered.contains("<<<BEGIN REPOSITORY SOURCE"));
    }
}
