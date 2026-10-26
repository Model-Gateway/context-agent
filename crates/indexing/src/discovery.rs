//! Repository discovery (spec §6.4) and validation tooling detection (§10.5).

use ctx_core::{RepositoryId, Result};
use ctx_languages::{LanguageId, LanguageRegistry};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitInfo {
    pub toplevel: PathBuf,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub dirty_files: Vec<String>,
    pub is_worktree: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildSystem {
    Npm,
    Cargo,
    Python,
    Go,
    Maven,
    Gradle,
    Make,
    CMake,
    Bundler,
    Composer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationKind {
    Test,
    Build,
    Typecheck,
    Lint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationCommand {
    pub command: String,
    pub kind: ValidationKind,
    /// Directory relative to the repository root in which to run.
    pub cwd: String,
    /// Where this command was discovered, e.g. `package.json scripts.test`.
    pub source: String,
    pub system: BuildSystem,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildFile {
    pub path: String,
    pub system: BuildSystem,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RepositoryProfile {
    pub root: PathBuf,
    pub id: RepositoryId,
    pub git: Option<GitInfo>,
    pub languages: BTreeMap<String, usize>,
    pub build_files: Vec<BuildFile>,
    pub validation: Vec<ValidationCommand>,
    pub docs: Vec<String>,
    pub file_count: usize,
    pub total_bytes: u64,
}

impl RepositoryProfile {
    pub fn primary_languages(&self) -> Vec<String> {
        let mut langs: Vec<(String, usize)> = self.languages.iter().filter(|(l, _)| LanguageId::parse(l).map(|id| id.is_code()).unwrap_or(false)).map(|(l, n)| (l.clone(), *n)).collect();
        langs.sort_by(|a, b| b.1.cmp(&a.1));
        langs.into_iter().take(4).map(|(l, _)| l).collect()
    }

    /// Validation commands of a kind, most authoritative first.
    pub fn validation_of(&self, kind: ValidationKind) -> Vec<&ValidationCommand> {
        self.validation.iter().filter(|v| v.kind == kind).collect()
    }
}

pub struct RepositoryDiscovery;

impl RepositoryDiscovery {
    /// Find the repository root for `start`: the git toplevel when inside a
    /// repository, otherwise the directory itself.
    pub fn find_root(start: &Path) -> PathBuf {
        let start = ctx_core::platform::canonicalize(start).unwrap_or_else(|_| start.to_path_buf());
        if let Some(git) = Self::git_info(&start) {
            return git.toplevel;
        }
        start
    }

    pub fn git_info(root: &Path) -> Option<GitInfo> {
        let run = |args: &[&str]| -> Option<String> {
            let out = ctx_core::platform::hide_console_window(&mut Command::new("git")).args(args).current_dir(root).output().ok()?;
            if !out.status.success() {
                return None;
            }
            Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
        };
        let toplevel = run(&["rev-parse", "--show-toplevel"])?;
        let toplevel = PathBuf::from(toplevel);
        let branch = run(&["rev-parse", "--abbrev-ref", "HEAD"]).filter(|b| b != "HEAD");
        let head = run(&["rev-parse", "HEAD"]);
        let dirty_files = run(&["status", "--porcelain"])
            .map(|s| s.lines().filter(|l| l.len() > 3).map(|l| l[3..].trim().to_string()).collect())
            .unwrap_or_default();
        let git_dir = run(&["rev-parse", "--git-dir"]).unwrap_or_default();
        let is_worktree = git_dir.contains("/worktrees/");
        Some(GitInfo { toplevel, branch, head, dirty_files, is_worktree })
    }

    /// Build the profile from an already-walked file list.
    pub fn profile(root: &Path, files: &[crate::walker::WalkedFile], registry: &LanguageRegistry) -> Result<RepositoryProfile> {
        Self::profile_of(root, files.iter().map(|f| (f.relative.as_str(), f.size)), registry)
    }

    /// Build the profile from repository-relative paths and their sizes
    /// (a walk, or the files of a published index).
    pub fn profile_of<'a>(root: &Path, files: impl Iterator<Item = (&'a str, u64)>, registry: &LanguageRegistry) -> Result<RepositoryProfile> {
        let mut languages: BTreeMap<String, usize> = BTreeMap::new();
        let mut build_files = Vec::new();
        let mut docs = Vec::new();
        let mut total_bytes = 0;
        let mut file_count = 0;
        for (relative, size) in files {
            file_count += 1;
            total_bytes += size;
            let lang = registry.language_of(relative).map(|l| l.as_str().to_string()).unwrap_or_else(|| "other".into());
            *languages.entry(lang).or_insert(0) += 1;
            if let Some(system) = build_system_for(relative) {
                build_files.push(BuildFile { path: relative.to_string(), system });
            }
            let lower = relative.to_ascii_lowercase();
            if lower.ends_with(".md") && (lower.starts_with("readme") || lower.contains("/readme") || lower.contains("docs/") || lower.contains("contributing") || lower.contains("architecture")) {
                docs.push(relative.to_string());
            }
        }
        build_files.sort_by_key(|b| b.path.matches('/').count());
        let validation = detect_validation(root, &build_files);
        Ok(RepositoryProfile {
            root: root.to_path_buf(),
            id: RepositoryId::from_root(root),
            git: Self::git_info(root),
            languages,
            build_files,
            validation,
            docs,
            file_count,
            total_bytes,
        })
    }
}

fn build_system_for(rel: &str) -> Option<BuildSystem> {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    Some(match name {
        "package.json" => BuildSystem::Npm,
        "Cargo.toml" => BuildSystem::Cargo,
        "pyproject.toml" | "setup.py" | "setup.cfg" | "pytest.ini" | "tox.ini" | "requirements.txt" => BuildSystem::Python,
        "go.mod" => BuildSystem::Go,
        "pom.xml" => BuildSystem::Maven,
        "build.gradle" | "build.gradle.kts" | "settings.gradle" | "settings.gradle.kts" => BuildSystem::Gradle,
        "Makefile" | "GNUmakefile" | "makefile" => BuildSystem::Make,
        "CMakeLists.txt" => BuildSystem::CMake,
        "Gemfile" | "Rakefile" => BuildSystem::Bundler,
        "composer.json" => BuildSystem::Composer,
        _ => return None,
    })
}

fn dir_of(rel: &str) -> String {
    match rel.rfind('/') {
        Some(i) => rel[..i].to_string(),
        None => ".".to_string(),
    }
}

/// Inspect each build file and derive validation commands. Existing project
/// scripts are preferred over generic guesses (spec §10.5).
pub fn detect_validation(root: &Path, build_files: &[BuildFile]) -> Vec<ValidationCommand> {
    let mut out: Vec<ValidationCommand> = Vec::new();
    let mut seen: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
    fn push_cmd(out: &mut Vec<ValidationCommand>, seen: &mut std::collections::HashSet<(String, String)>, cmd: ValidationCommand) {
        if seen.insert((cmd.command.clone(), cmd.cwd.clone())) {
            out.push(cmd);
        }
    }
    for bf in build_files {
        let cwd = dir_of(&bf.path);
        let abs_dir = root.join(&cwd);
        match bf.system {
            BuildSystem::Npm => {
                let text = std::fs::read_to_string(root.join(&bf.path)).unwrap_or_default();
                let json: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
                let runner = if abs_dir.join("pnpm-lock.yaml").exists() {
                    "pnpm"
                } else if abs_dir.join("yarn.lock").exists() {
                    "yarn"
                } else if abs_dir.join("bun.lockb").exists() || abs_dir.join("bun.lock").exists() {
                    "bun"
                } else {
                    "npm"
                };
                if let Some(scripts) = json.get("scripts").and_then(|s| s.as_object()) {
                    for (name, kind) in [("test", ValidationKind::Test), ("typecheck", ValidationKind::Typecheck), ("type-check", ValidationKind::Typecheck), ("lint", ValidationKind::Lint), ("build", ValidationKind::Build), ("check", ValidationKind::Typecheck)] {
                        if let Some(script) = scripts.get(name).and_then(|s| s.as_str()) {
                            if script.trim().is_empty() || script.contains("no test specified") {
                                continue;
                            }
                            let command = if runner == "npm" { format!("npm run {name}") } else { format!("{runner} run {name}") };
                            push_cmd(&mut out, &mut seen, ValidationCommand { command, kind, cwd: cwd.clone(), source: format!("{} scripts.{name}", bf.path), system: BuildSystem::Npm });
                        }
                    }
                }
                if abs_dir.join("tsconfig.json").exists() && !out.iter().any(|c| c.kind == ValidationKind::Typecheck && c.cwd == cwd) {
                    push_cmd(&mut out, &mut seen, ValidationCommand { command: "npx tsc --noEmit".into(), kind: ValidationKind::Typecheck, cwd: cwd.clone(), source: "tsconfig.json present".into(), system: BuildSystem::Npm });
                }
            }
            BuildSystem::Cargo => {
                let text = std::fs::read_to_string(root.join(&bf.path)).unwrap_or_default();
                let is_workspace = text.contains("[workspace]");
                let suffix = if is_workspace { " --workspace" } else { "" };
                push_cmd(&mut out, &mut seen, ValidationCommand { command: format!("cargo check{suffix}"), kind: ValidationKind::Build, cwd: cwd.clone(), source: bf.path.clone(), system: BuildSystem::Cargo });
                push_cmd(&mut out, &mut seen, ValidationCommand { command: format!("cargo test{suffix}"), kind: ValidationKind::Test, cwd: cwd.clone(), source: bf.path.clone(), system: BuildSystem::Cargo });
            }
            BuildSystem::Python => {
                let name = bf.path.rsplit('/').next().unwrap_or("");
                let text = std::fs::read_to_string(root.join(&bf.path)).unwrap_or_default();
                let has_pytest = text.contains("pytest") || name == "pytest.ini" || abs_dir.join("conftest.py").exists();
                let tests_dir = abs_dir.join("tests").is_dir() || abs_dir.join("test").is_dir();
                if has_pytest {
                    push_cmd(&mut out, &mut seen, ValidationCommand { command: format!("{} -m pytest -q", ctx_core::platform::python_interpreter()), kind: ValidationKind::Test, cwd: cwd.clone(), source: format!("{} mentions pytest", bf.path), system: BuildSystem::Python });
                } else if tests_dir && name == "pyproject.toml" || name == "setup.py" {
                    let dir = if abs_dir.join("tests").is_dir() { "tests" } else { "test" };
                    push_cmd(&mut out, &mut seen, ValidationCommand { command: format!("{} -m unittest discover -s {dir} -v", ctx_core::platform::python_interpreter()), kind: ValidationKind::Test, cwd: cwd.clone(), source: format!("{} with {dir}/ directory", bf.path), system: BuildSystem::Python });
                }
                if text.contains("[tool.ruff]") || text.contains("ruff") {
                    push_cmd(&mut out, &mut seen, ValidationCommand { command: "ruff check .".into(), kind: ValidationKind::Lint, cwd: cwd.clone(), source: bf.path.clone(), system: BuildSystem::Python });
                }
                if text.contains("[tool.mypy]") {
                    push_cmd(&mut out, &mut seen, ValidationCommand { command: "mypy .".into(), kind: ValidationKind::Typecheck, cwd: cwd.clone(), source: bf.path.clone(), system: BuildSystem::Python });
                }
                if text.contains("[tool.pyright]") {
                    push_cmd(&mut out, &mut seen, ValidationCommand { command: "pyright".into(), kind: ValidationKind::Typecheck, cwd: cwd.clone(), source: bf.path.clone(), system: BuildSystem::Python });
                }
            }
            BuildSystem::Go => {
                push_cmd(&mut out, &mut seen, ValidationCommand { command: "go build ./...".into(), kind: ValidationKind::Build, cwd: cwd.clone(), source: bf.path.clone(), system: BuildSystem::Go });
                push_cmd(&mut out, &mut seen, ValidationCommand { command: "go vet ./...".into(), kind: ValidationKind::Lint, cwd: cwd.clone(), source: bf.path.clone(), system: BuildSystem::Go });
                push_cmd(&mut out, &mut seen, ValidationCommand { command: "go test ./...".into(), kind: ValidationKind::Test, cwd: cwd.clone(), source: bf.path.clone(), system: BuildSystem::Go });
            }
            BuildSystem::Maven => {
                let mvn = if abs_dir.join("mvnw").exists() { "./mvnw" } else { "mvn" };
                push_cmd(&mut out, &mut seen, ValidationCommand { command: format!("{mvn} -q test"), kind: ValidationKind::Test, cwd: cwd.clone(), source: bf.path.clone(), system: BuildSystem::Maven });
            }
            BuildSystem::Gradle => {
                if bf.path.contains("settings.gradle") && build_files.iter().any(|b| b.system == BuildSystem::Gradle && b.path.contains("build.gradle") && dir_of(&b.path) == cwd) {
                    continue;
                }
                let gradle = if abs_dir.join("gradlew").exists() { "./gradlew" } else { "gradle" };
                push_cmd(&mut out, &mut seen, ValidationCommand { command: format!("{gradle} test"), kind: ValidationKind::Test, cwd: cwd.clone(), source: bf.path.clone(), system: BuildSystem::Gradle });
            }
            BuildSystem::Make => {
                let text = std::fs::read_to_string(root.join(&bf.path)).unwrap_or_default();
                let has_target = |t: &str| text.lines().any(|l| l.starts_with(&format!("{t}:")) || l.starts_with(&format!("{t} :")));
                if has_target("test") {
                    push_cmd(&mut out, &mut seen, ValidationCommand { command: "make test".into(), kind: ValidationKind::Test, cwd: cwd.clone(), source: format!("{} target test", bf.path), system: BuildSystem::Make });
                }
                if has_target("check") {
                    push_cmd(&mut out, &mut seen, ValidationCommand { command: "make check".into(), kind: ValidationKind::Test, cwd: cwd.clone(), source: format!("{} target check", bf.path), system: BuildSystem::Make });
                }
                if has_target("lint") {
                    push_cmd(&mut out, &mut seen, ValidationCommand { command: "make lint".into(), kind: ValidationKind::Lint, cwd: cwd.clone(), source: format!("{} target lint", bf.path), system: BuildSystem::Make });
                }
                if has_target("build") || has_target("all") {
                    let t = if has_target("build") { "build" } else { "all" };
                    push_cmd(&mut out, &mut seen, ValidationCommand { command: format!("make {t}"), kind: ValidationKind::Build, cwd: cwd.clone(), source: format!("{} target {t}", bf.path), system: BuildSystem::Make });
                }
            }
            BuildSystem::CMake => {
                push_cmd(&mut out, &mut seen, ValidationCommand { command: "cmake -S . -B build && cmake --build build".into(), kind: ValidationKind::Build, cwd: cwd.clone(), source: bf.path.clone(), system: BuildSystem::CMake });
                push_cmd(&mut out, &mut seen, ValidationCommand { command: "ctest --test-dir build --output-on-failure".into(), kind: ValidationKind::Test, cwd: cwd.clone(), source: bf.path.clone(), system: BuildSystem::CMake });
            }
            BuildSystem::Bundler => {
                let name = bf.path.rsplit('/').next().unwrap_or("");
                if abs_dir.join("spec").is_dir() {
                    push_cmd(&mut out, &mut seen, ValidationCommand { command: "bundle exec rspec".into(), kind: ValidationKind::Test, cwd: cwd.clone(), source: format!("{name} with spec/ directory"), system: BuildSystem::Bundler });
                } else if abs_dir.join("test").is_dir() {
                    if name == "Rakefile" {
                        push_cmd(&mut out, &mut seen, ValidationCommand { command: "bundle exec rake test".into(), kind: ValidationKind::Test, cwd: cwd.clone(), source: "Rakefile with test/ directory".into(), system: BuildSystem::Bundler });
                    }
                    push_cmd(&mut out, &mut seen, ValidationCommand {
                        command: "ruby -Ilib -Itest -e 'Dir[\"test/**/*_test.rb\"].each { |f| require File.expand_path(f) }'".into(),
                        kind: ValidationKind::Test,
                        cwd: cwd.clone(),
                        source: format!("{name} with test/ directory (minitest)"),
                        system: BuildSystem::Bundler,
                    });
                }
            }
            BuildSystem::Composer => {
                let text = std::fs::read_to_string(root.join(&bf.path)).unwrap_or_default();
                let json: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
                if json.pointer("/scripts/test").is_some() {
                    push_cmd(&mut out, &mut seen, ValidationCommand { command: "composer test".into(), kind: ValidationKind::Test, cwd: cwd.clone(), source: format!("{} scripts.test", bf.path), system: BuildSystem::Composer });
                } else if abs_dir.join("phpunit.xml").exists() || abs_dir.join("phpunit.xml.dist").exists() {
                    push_cmd(&mut out, &mut seen, ValidationCommand { command: "vendor/bin/phpunit".into(), kind: ValidationKind::Test, cwd: cwd.clone(), source: "phpunit.xml present".into(), system: BuildSystem::Composer });
                }
            }
        }
    }
    // Root-level commands first, then by kind priority.
    out.sort_by_key(|c| (c.cwd.matches('/').count() + usize::from(c.cwd != "."), match c.kind { ValidationKind::Test => 0, ValidationKind::Typecheck => 1, ValidationKind::Build => 2, ValidationKind::Lint => 3 }));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_npm_and_cargo_commands() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"scripts": {"test": "node --test", "lint": "eslint .", "build": ""}}"#).unwrap();
        std::fs::create_dir_all(dir.path().join("tools")).unwrap();
        std::fs::write(dir.path().join("tools/Cargo.toml"), "[package]\nname = \"t\"\n").unwrap();
        std::fs::write(dir.path().join("Makefile"), "all: build\n\ntest:\n\tnode --test\n").unwrap();
        let build_files = vec![
            BuildFile { path: "package.json".into(), system: BuildSystem::Npm },
            BuildFile { path: "tools/Cargo.toml".into(), system: BuildSystem::Cargo },
            BuildFile { path: "Makefile".into(), system: BuildSystem::Make },
        ];
        let cmds = detect_validation(dir.path(), &build_files);
        let texts: Vec<(String, String)> = cmds.iter().map(|c| (c.command.clone(), c.cwd.clone())).collect();
        assert!(texts.contains(&("npm run test".into(), ".".into())));
        assert!(texts.contains(&("npm run lint".into(), ".".into())));
        assert!(!texts.iter().any(|(c, _)| c == "npm run build"), "empty scripts are skipped");
        assert!(texts.contains(&("cargo test".into(), "tools".into())));
        assert!(texts.contains(&("make test".into(), ".".into())));
        assert_eq!(cmds[0].cwd, ".");
    }

    #[test]
    fn git_info_outside_repo_is_none_or_toplevel() {
        let dir = tempfile::tempdir().unwrap();
        // A temp dir is normally not inside a git repository.
        let info = RepositoryDiscovery::git_info(dir.path());
        if let Some(info) = info {
            assert!(dir.path().starts_with(&info.toplevel));
        }
    }
}
