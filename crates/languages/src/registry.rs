//! Language registry: maps paths to adapters.

use crate::plaintext::PlainTextAdapter;
use crate::specs;
use crate::treesitter::TreeSitterAdapter;
use crate::types::{LanguageId, SupportLevel};
use crate::LanguageAdapter;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

pub struct LanguageRegistry {
    adapters: Vec<Arc<dyn LanguageAdapter>>,
    by_extension: HashMap<String, usize>,
    by_filename: HashMap<String, usize>,
    by_id: HashMap<LanguageId, usize>,
    fallback: Arc<dyn LanguageAdapter>,
}

impl LanguageRegistry {
    /// Registry with every built-in language adapter plus text fallbacks.
    pub fn with_defaults() -> Self {
        let mut registry = Self::empty();
        for spec in specs::all_specs() {
            registry.register(Arc::new(TreeSitterAdapter::new(spec)));
        }
        registry.register(Arc::new(PlainTextAdapter::new(LanguageId::Markdown, &["md", "mdx", "markdown", "mdown", "mkd"], &[])));
        registry.register(Arc::new(PlainTextAdapter::new(LanguageId::Json, &["json", "jsonc", "json5"], &[".babelrc", ".eslintrc", ".prettierrc"])));
        registry.register(Arc::new(PlainTextAdapter::new(LanguageId::Yaml, &["yaml", "yml"], &[".clang-format", ".clangd"])));
        registry.register(Arc::new(PlainTextAdapter::new(LanguageId::Toml, &["toml"], &["Cargo.lock"])));
        registry.register(Arc::new(PlainTextAdapter::new(LanguageId::Shell, &["sh", "bash", "zsh", "fish"], &["Makefile", "Dockerfile", "Justfile", ".bashrc", ".zshrc", ".bash_profile", ".zprofile", ".profile", ".envrc"])));
        registry.register(Arc::new(PlainTextAdapter::new(LanguageId::Html, &["html", "htm", "xhtml"], &[])));
        registry.register(Arc::new(PlainTextAdapter::new(LanguageId::Css, &["css", "scss", "sass", "less", "pcss", "postcss"], &[])));
        // `.env` and `.env.local` resolve through the multi-part extension lookup.
        registry.register(Arc::new(PlainTextAdapter::new(LanguageId::Dotenv, &["env", "env.example", "env.sample", "env.template", "env.local", "env.development", "env.production", "env.staging", "env.test"], &[])));
        registry.register(Arc::new(PlainTextAdapter::generic()));
        registry
    }

    pub fn empty() -> Self {
        Self {
            adapters: Vec::new(),
            by_extension: HashMap::new(),
            by_filename: HashMap::new(),
            by_id: HashMap::new(),
            fallback: Arc::new(PlainTextAdapter::generic()),
        }
    }

    pub fn register(&mut self, adapter: Arc<dyn LanguageAdapter>) {
        let idx = self.adapters.len();
        for ext in adapter.extensions() {
            self.by_extension.entry(ext.to_ascii_lowercase()).or_insert(idx);
        }
        for name in adapter.filenames() {
            self.by_filename.entry(name.to_string()).or_insert(idx);
        }
        self.by_id.entry(adapter.identifier()).or_insert(idx);
        self.adapters.push(adapter);
    }

    /// Adapter claiming this path, if any (no fallback).
    pub fn detect(&self, path: &str) -> Option<Arc<dyn LanguageAdapter>> {
        let p = Path::new(path);
        let file_name = p.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
        if let Some(&idx) = self.by_filename.get(&file_name) {
            return Some(self.adapters[idx].clone());
        }
        // Multi-part extensions like `.d.ts` or `.env.example`.
        let lower = file_name.to_ascii_lowercase();
        let mut parts = lower.split('.').skip(1).collect::<Vec<_>>();
        while !parts.is_empty() {
            let ext = parts.join(".");
            if let Some(&idx) = self.by_extension.get(&ext) {
                return Some(self.adapters[idx].clone());
            }
            parts.remove(0);
        }
        None
    }

    /// Adapter for this path, falling back to the universal text adapter.
    pub fn adapter_for(&self, path: &str) -> Arc<dyn LanguageAdapter> {
        self.detect(path).unwrap_or_else(|| self.fallback.clone())
    }

    pub fn get(&self, id: LanguageId) -> Option<Arc<dyn LanguageAdapter>> {
        self.by_id.get(&id).map(|&i| self.adapters[i].clone())
    }

    pub fn language_of(&self, path: &str) -> Option<LanguageId> {
        self.detect(path).map(|a| a.identifier())
    }

    pub fn languages(&self) -> Vec<(LanguageId, SupportLevel)> {
        self.adapters.iter().map(|a| (a.identifier(), a.support_level())).collect()
    }

    pub fn parsed_languages(&self) -> Vec<LanguageId> {
        self.adapters.iter().filter(|a| a.support_level() >= SupportLevel::Parsed).map(|a| a.identifier()).collect()
    }
}

impl Default for LanguageRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_by_extension_and_filename() {
        let r = LanguageRegistry::with_defaults();
        assert_eq!(r.language_of("src/main.rs"), Some(LanguageId::Rust));
        assert_eq!(r.language_of("types.d.ts"), Some(LanguageId::TypeScript));
        assert_eq!(r.language_of("a.tsx"), Some(LanguageId::Tsx));
        assert_eq!(r.language_of("x.h"), Some(LanguageId::C));
        assert_eq!(r.language_of("x.hpp"), Some(LanguageId::Cpp));
        assert_eq!(r.language_of("Gemfile"), Some(LanguageId::Ruby));
        assert_eq!(r.language_of("app.py"), Some(LanguageId::Python));
        assert_eq!(r.language_of("main.go"), Some(LanguageId::Go));
        assert_eq!(r.language_of("App.java"), Some(LanguageId::Java));
        assert_eq!(r.language_of("index.php"), Some(LanguageId::Php));
        assert_eq!(r.language_of("a.mjs"), Some(LanguageId::JavaScript));
        assert_eq!(r.language_of("lib.rs"), Some(LanguageId::Rust));
        assert_eq!(r.language_of("README.md"), Some(LanguageId::Markdown));
        assert_eq!(r.language_of("Makefile"), Some(LanguageId::Shell));
        assert_eq!(r.language_of("public/index.html"), Some(LanguageId::Html));
        assert_eq!(r.language_of("styles/app.module.scss"), Some(LanguageId::Css));
        assert_eq!(r.language_of(".env"), Some(LanguageId::Dotenv));
        assert_eq!(r.language_of("config/.env.local"), Some(LanguageId::Dotenv));
        assert_eq!(r.language_of(".env.example"), Some(LanguageId::Dotenv));
        assert_eq!(r.language_of("docker.env"), Some(LanguageId::Dotenv));
        assert!(r.detect("binary.bin").is_none());
        assert_eq!(r.adapter_for("binary.bin").identifier(), LanguageId::PlainText);
    }
}
