//! Import-target resolution heuristics (spec §6.6). Every resolver returns
//! `None` rather than guessing when the target cannot be located.

use std::collections::HashSet;

pub struct Resolver<'a> {
    pub paths: &'a HashSet<String>,
    /// Lowercase basename → paths with that basename.
    basenames: std::collections::HashMap<String, Vec<String>>,
    pub go_module: Option<String>,
}

fn dirname(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[..i],
        None => "",
    }
}

fn join(dir: &str, rel: &str) -> String {
    let mut parts: Vec<&str> = if dir.is_empty() { Vec::new() } else { dir.split('/').collect() };
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

const JS_EXTS: &[&str] = &["ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts", "d.ts", "json", "css", "scss", "vue", "svelte"];

impl<'a> Resolver<'a> {
    pub fn new(paths: &'a HashSet<String>, go_module: Option<String>) -> Self {
        let mut basenames: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
        for p in paths {
            let base = p.rsplit('/').next().unwrap_or(p).to_ascii_lowercase();
            basenames.entry(base).or_default().push(p.clone());
        }
        Self { paths, basenames, go_module }
    }

    fn exists(&self, p: &str) -> Option<String> {
        if self.paths.contains(p) {
            Some(p.to_string())
        } else {
            None
        }
    }

    fn try_exts(&self, base: &str, exts: &[&str]) -> Option<String> {
        if let Some(p) = self.exists(base) {
            return Some(p);
        }
        for ext in exts {
            if let Some(p) = self.exists(&format!("{base}.{ext}")) {
                return Some(p);
            }
        }
        for ext in exts {
            if let Some(p) = self.exists(&format!("{base}/index.{ext}")) {
                return Some(p);
            }
        }
        None
    }

    fn unique_basename(&self, name: &str) -> Option<String> {
        let v = self.basenames.get(&name.to_ascii_lowercase())?;
        if v.len() == 1 {
            Some(v[0].clone())
        } else {
            None
        }
    }

    /// Resolve `target` imported from `from_path` for the given language.
    pub fn resolve(&self, from_path: &str, target: &str, language: Option<&str>) -> Option<String> {
        let dir = dirname(from_path);
        match language {
            Some("typescript") | Some("tsx") | Some("javascript") => self.resolve_js(dir, target),
            Some("python") => self.resolve_python(dir, target),
            Some("rust") => self.resolve_rust(from_path, target),
            Some("go") => self.resolve_go(target),
            Some("java") => self.resolve_java(target),
            Some("c") | Some("cpp") => self.resolve_c(dir, target),
            Some("ruby") => self.resolve_ruby(dir, target),
            Some("php") => self.resolve_php(dir, target),
            _ => self.resolve_js(dir, target),
        }
    }

    fn resolve_js(&self, dir: &str, target: &str) -> Option<String> {
        let t = target.trim();
        if t.starts_with('.') {
            return self.try_exts(&join(dir, t), JS_EXTS);
        }
        if let Some(rest) = t.strip_prefix("@/").or_else(|| t.strip_prefix("~/")) {
            for prefix in ["src", "app", "lib", ""] {
                let base = if prefix.is_empty() { rest.to_string() } else { format!("{prefix}/{rest}") };
                if let Some(p) = self.try_exts(&base, JS_EXTS) {
                    return Some(p);
                }
            }
        }
        // Bare specifier: external package unless it maps onto a repository
        // path exactly (monorepo path aliases).
        if !t.starts_with('@') && t.contains('/') {
            return self.try_exts(t, JS_EXTS);
        }
        None
    }

    fn resolve_python(&self, dir: &str, target: &str) -> Option<String> {
        let t = target.trim();
        let leading_dots = t.chars().take_while(|c| *c == '.').count();
        let module = &t[leading_dots..];
        let module_path = module.replace('.', "/");
        let mut bases: Vec<String> = Vec::new();
        if leading_dots > 0 {
            let mut base = dir.to_string();
            for _ in 1..leading_dots {
                base = dirname(&base).to_string();
            }
            bases.push(join(&base, &module_path));
        } else {
            bases.push(module_path.clone());
            // Search upward from the importing file for a package root.
            let mut d = dir.to_string();
            loop {
                bases.push(join(&d, &module_path));
                if d.is_empty() {
                    break;
                }
                d = dirname(&d).to_string();
            }
            for prefix in ["src", "lib", "app"] {
                bases.push(format!("{prefix}/{module_path}"));
            }
        }
        for base in bases {
            if base.is_empty() {
                continue;
            }
            if let Some(p) = self.exists(&format!("{base}.py")) {
                return Some(p);
            }
            if let Some(p) = self.exists(&format!("{base}/__init__.py")) {
                return Some(p);
            }
            // `from pkg.module import name` where name is a symbol: try parent.
            if let Some(parent) = base.rfind('/').map(|i| &base[..i]) {
                if let Some(p) = self.exists(&format!("{parent}.py")) {
                    return Some(p);
                }
            }
        }
        None
    }

    fn resolve_rust(&self, from_path: &str, target: &str) -> Option<String> {
        let t = target.trim();
        let first = t.split("::").next()?.trim_matches(|c| c == '{' || c == '}' || c == ' ');
        let segments: Vec<&str> = t.split("::").map(|s| s.trim()).filter(|s| !s.is_empty() && !s.starts_with('{') && *s != "*").collect();
        // Find the crate source root for the importing file.
        let src_root = {
            let mut d = dirname(from_path).to_string();
            loop {
                if d.ends_with("src") || d == "src" {
                    break d;
                }
                if d.is_empty() {
                    break "src".to_string();
                }
                d = dirname(&d).to_string();
            }
        };
        let module_dir = {
            // Module directory for `self::`/`super::` resolution.
            let d = dirname(from_path);
            let name = from_path.rsplit('/').next().unwrap_or("");
            if name == "mod.rs" || name == "lib.rs" || name == "main.rs" {
                d.to_string()
            } else {
                format!("{d}/{}", name.trim_end_matches(".rs"))
            }
        };
        let (base, rest): (String, &[&str]) = match first {
            "crate" => (src_root.clone(), &segments[1..]),
            "self" => (module_dir.clone(), &segments[1..]),
            "super" => (dirname(&module_dir).to_string(), &segments[1..]),
            _ => return None, // external crate or std
        };
        // Walk the longest resolvable prefix.
        let mut best: Option<String> = None;
        let mut cur = base;
        for seg in rest {
            let candidates = [format!("{cur}/{seg}.rs"), format!("{cur}/{seg}/mod.rs")];
            if let Some(found) = candidates.iter().find_map(|c| self.exists(c)) {
                best = Some(found);
                cur = format!("{cur}/{seg}");
            } else {
                break;
            }
        }
        best.or_else(|| {
            if rest.is_empty() {
                ["lib.rs", "main.rs", "mod.rs"].iter().find_map(|f| self.exists(&format!("{cur}/{f}")))
            } else {
                None
            }
        })
    }

    fn resolve_go(&self, target: &str) -> Option<String> {
        let t = target.trim();
        let module = self.go_module.as_deref()?;
        let rel = t.strip_prefix(module)?.trim_start_matches('/');
        let dir = if rel.is_empty() { String::new() } else { rel.to_string() };
        let mut candidates: Vec<&String> = self.paths.iter().filter(|p| dirname(p) == dir && p.ends_with(".go") && !p.ends_with("_test.go")).collect();
        candidates.sort();
        candidates.first().map(|s| s.to_string())
    }

    fn resolve_java(&self, target: &str) -> Option<String> {
        let t = target.trim().trim_start_matches("static ").trim_end_matches(".*");
        let rel = t.replace('.', "/");
        let suffix = format!("{rel}.java");
        let mut matches: Vec<&String> = self.paths.iter().filter(|p| p.ends_with(&suffix)).collect();
        if matches.is_empty() {
            // Wildcard package import or inner class: try the parent.
            if let Some(parent) = rel.rfind('/').map(|i| &rel[..i]) {
                let suffix = format!("{parent}.java");
                matches = self.paths.iter().filter(|p| p.ends_with(&suffix)).collect();
            }
        }
        matches.sort();
        matches.first().map(|s| s.to_string())
    }

    fn resolve_c(&self, dir: &str, target: &str) -> Option<String> {
        let t = target.trim();
        if let Some(p) = self.exists(&join(dir, t)) {
            return Some(p);
        }
        for inc in ["include", "src", "src/include", "lib", ""] {
            let base = if inc.is_empty() { t.to_string() } else { format!("{inc}/{t}") };
            if let Some(p) = self.exists(&base) {
                return Some(p);
            }
        }
        let name = t.rsplit('/').next().unwrap_or(t);
        self.unique_basename(name)
    }

    fn resolve_ruby(&self, dir: &str, target: &str) -> Option<String> {
        let t = target.trim().trim_end_matches(".rb");
        if t.starts_with('.') {
            return self.try_exts(&join(dir, t), &["rb"]);
        }
        if let Some(p) = self.try_exts(&join(dir, t), &["rb"]) {
            return Some(p);
        }
        for prefix in ["lib", "app", ""] {
            let base = if prefix.is_empty() { t.to_string() } else { format!("{prefix}/{t}") };
            if let Some(p) = self.try_exts(&base, &["rb"]) {
                return Some(p);
            }
        }
        None
    }

    fn resolve_php(&self, dir: &str, target: &str) -> Option<String> {
        let t = target.trim();
        if t.ends_with(".php") || t.starts_with('.') || t.contains("__DIR__") {
            let cleaned = t.replace("__DIR__", "").replace(" . ", "").replace('"', "").replace('\'', "");
            return self.try_exts(&join(dir, cleaned.trim_start_matches('/')), &["php"]);
        }
        // Namespace: strip the vendor/app prefix and map to src/ (PSR-4 guess).
        let segments: Vec<&str> = t.split('\\').filter(|s| !s.is_empty()).collect();
        if segments.is_empty() {
            return None;
        }
        for skip in 0..segments.len().min(2) {
            let rel = segments[skip..].join("/");
            for prefix in ["src", "app", "lib", ""] {
                let base = if prefix.is_empty() { rel.clone() } else { format!("{prefix}/{rel}") };
                if let Some(p) = self.exists(&format!("{base}.php")) {
                    return Some(p);
                }
            }
        }
        self.unique_basename(&format!("{}.php", segments.last().unwrap()))
    }
}

/// Derive heuristic companion relationships (tests, styles) for a file.
pub fn companions(path: &str, paths: &HashSet<String>) -> Vec<(String, &'static str)> {
    companions_in(path, &StemIndex::new(paths))
}

/// Paths grouped by lowercase file stem (`src/Login.test.tsx` → `login`),
/// so a companion lookup touches only files that share a stem instead of
/// every path in the repository.
pub struct StemIndex<'a> {
    by_stem: std::collections::HashMap<String, Vec<&'a String>>,
}

impl<'a> StemIndex<'a> {
    pub fn new(paths: &'a HashSet<String>) -> Self {
        let mut by_stem: std::collections::HashMap<String, Vec<&'a String>> = std::collections::HashMap::new();
        for p in paths {
            let name = p.rsplit('/').next().unwrap_or(p);
            by_stem.entry(name.split('.').next().unwrap_or(name).to_ascii_lowercase()).or_default().push(p);
        }
        Self { by_stem }
    }

    fn with_stem(&self, stem: &str) -> impl Iterator<Item = &'a String> + '_ {
        self.by_stem.get(stem).into_iter().flatten().copied()
    }
}

/// [`companions`] against a prebuilt [`StemIndex`].
pub fn companions_in(path: &str, index: &StemIndex<'_>) -> Vec<(String, &'static str)> {
    let mut out = Vec::new();
    let name = path.rsplit('/').next().unwrap_or(path);
    let stem = name.split('.').next().unwrap_or(name);
    let lower = stem.to_ascii_lowercase();
    // Test → source: strip test markers from the stem. For `x.test.ts`
    // style names the stem already equals the source stem.
    let mut candidates: Vec<String> = vec![
        lower.clone(),
        lower.trim_start_matches("test_").to_string(),
        lower.trim_end_matches("_test").to_string(),
        lower.trim_end_matches("_spec").to_string(),
        lower.trim_end_matches("tests").to_string(),
        lower.trim_end_matches("test").to_string(),
        lower.trim_end_matches("spec").to_string(),
    ];
    candidates.dedup();
    let is_test_like = name.contains(".test.") || name.contains(".spec.") || name.contains("_test.") || name.contains("_spec.") || name.starts_with("test_") || stem.ends_with("Test") || stem.ends_with("Tests") || stem.ends_with("Spec");
    if is_test_like {
        for cand in candidates.iter().filter(|c| !c.is_empty()) {
            for p in index.with_stem(cand) {
                if p == path {
                    continue;
                }
                let pn = p.rsplit('/').next().unwrap_or(p);
                let p_is_test = pn.contains(".test.") || pn.contains(".spec.") || pn.contains("_test.") || pn.starts_with("test_") || p.contains("/test") || p.contains("/__tests__/") || p.contains("/spec/");
                if !p_is_test {
                    out.push((p.clone(), "test_of"));
                }
            }
        }
    }
    // Style → component.
    if name.ends_with(".css") || name.ends_with(".scss") || name.ends_with(".module.css") || name.ends_with(".less") {
        for p in index.with_stem(&lower) {
            let pn = p.rsplit('/').next().unwrap_or(p);
            if pn.ends_with(".tsx") || pn.ends_with(".jsx") || pn.ends_with(".ts") || pn.ends_with(".js") || pn.ends_with(".vue") || pn.ends_with(".svelte") {
                out.push((p.clone(), "style_of"));
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(paths: &[&str]) -> HashSet<String> {
        paths.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn resolves_js_relative_and_index() {
        let paths = set(&["src/components/LoginButton.tsx", "src/components/Spinner.tsx", "src/hooks/useAuth.ts", "src/utils/index.ts"]);
        let r = Resolver::new(&paths, None);
        assert_eq!(r.resolve("src/components/LoginButton.tsx", "./Spinner", Some("tsx")).as_deref(), Some("src/components/Spinner.tsx"));
        assert_eq!(r.resolve("src/components/LoginButton.tsx", "../hooks/useAuth", Some("tsx")).as_deref(), Some("src/hooks/useAuth.ts"));
        assert_eq!(r.resolve("src/components/LoginButton.tsx", "../utils", Some("tsx")).as_deref(), Some("src/utils/index.ts"));
        assert_eq!(r.resolve("src/components/LoginButton.tsx", "react", Some("tsx")), None);
    }

    #[test]
    fn resolves_python_rust_go_java() {
        let paths = set(&["svc/__init__.py", "svc/pricing.py", "svc/handlers.py", "tests/test_pricing.py", "src/lib.rs", "src/parser.rs", "src/commands/mod.rs", "src/commands/greet.rs", "internal/handlers/handlers.go", "src/main/java/com/example/calc/Parser.java"]);
        let r = Resolver::new(&paths, Some("example.com/goservice".into()));
        assert_eq!(r.resolve("svc/handlers.py", "svc.pricing", Some("python")).as_deref(), Some("svc/pricing.py"));
        assert_eq!(r.resolve("svc/handlers.py", ".pricing", Some("python")).as_deref(), Some("svc/pricing.py"));
        assert_eq!(r.resolve("tests/test_pricing.py", "svc.pricing", Some("python")).as_deref(), Some("svc/pricing.py"));
        assert_eq!(r.resolve("svc/handlers.py", "os", Some("python")), None);
        assert_eq!(r.resolve("src/lib.rs", "crate::parser::parse_args", Some("rust")).as_deref(), Some("src/parser.rs"));
        assert_eq!(r.resolve("src/commands/greet.rs", "super::mod_thing", Some("rust")), None);
        assert_eq!(r.resolve("src/parser.rs", "crate::commands::greet::Greet", Some("rust")).as_deref(), Some("src/commands/greet.rs"));
        assert_eq!(r.resolve("src/lib.rs", "std::collections::HashMap", Some("rust")), None);
        assert_eq!(r.resolve("main.go", "example.com/goservice/internal/handlers", Some("go")).as_deref(), Some("internal/handlers/handlers.go"));
        assert_eq!(r.resolve("main.go", "fmt", Some("go")), None);
        assert_eq!(r.resolve("X.java", "com.example.calc.Parser", Some("java")).as_deref(), Some("src/main/java/com/example/calc/Parser.java"));
    }

    #[test]
    fn resolves_c_ruby_php() {
        let paths = set(&["include/vec.h", "src/vec.c", "lib/textkit/slug.rb", "lib/textkit.rb", "src/Cart.php", "src/Money.php"]);
        let r = Resolver::new(&paths, None);
        assert_eq!(r.resolve("src/vec.c", "vec.h", Some("c")).as_deref(), Some("include/vec.h"));
        assert_eq!(r.resolve("src/vec.c", "stdio.h", Some("c")), None);
        assert_eq!(r.resolve("lib/textkit.rb", "textkit/slug", Some("ruby")).as_deref(), Some("lib/textkit/slug.rb"));
        assert_eq!(r.resolve("test/slug_test.rb", "../lib/textkit", Some("ruby")).as_deref(), Some("lib/textkit.rb"));
        assert_eq!(r.resolve("src/Cart.php", "App\\Money", Some("php")).as_deref(), Some("src/Money.php"));
    }

    #[test]
    fn companions_link_tests_and_styles() {
        let paths = set(&["src/utils/format.ts", "src/__tests__/format.test.ts", "svc/pricing.py", "tests/test_pricing.py", "src/components/Button.tsx", "src/styles/button.css", "src/main/java/Calculator.java", "src/test/java/CalculatorTest.java"]);
        assert_eq!(companions("src/__tests__/format.test.ts", &paths), vec![("src/utils/format.ts".to_string(), "test_of")]);
        assert_eq!(companions("tests/test_pricing.py", &paths), vec![("svc/pricing.py".to_string(), "test_of")]);
        assert_eq!(companions("src/styles/button.css", &paths), vec![("src/components/Button.tsx".to_string(), "style_of")]);
        assert_eq!(companions("src/test/java/CalculatorTest.java", &paths), vec![("src/main/java/Calculator.java".to_string(), "test_of")]);
        assert!(companions("src/utils/format.ts", &paths).is_empty());
    }
}
