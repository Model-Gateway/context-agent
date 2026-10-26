//! Finds the `ctx` binary: the port of `BackendLocator.swift`. The first
//! executable file on the list wins, and the whole list is reported back
//! (`searched`) for the "ctx not found" panel.
//!
//! Search order — macOS, as the Swift code: the user override, the bundle
//! (`Contents/Helpers/ctx`, `Contents/MacOS/ctx`, where Tauri puts the
//! sidecar), `$CTX_BINARY`, `~/.cargo/bin`, `/opt/homebrew/bin`,
//! `/usr/local/bin`, then a repository checkout (`Cargo.toml` next to
//! `target/release/ctx` or `target/debug/ctx`) found by walking up from the
//! executable's directory and from the current directory.
//! Windows: the override, `ctx.exe` next to the app executable, `CTX_BINARY`,
//! `%USERPROFILE%\.cargo\bin`, `%LOCALAPPDATA%\Programs\Context`, every PATH
//! entry honouring `PATHEXT`, then the repository walk. Linux: the override,
//! next to the executable, `CTX_BINARY`, `~/.cargo/bin`, `/usr/local/bin`,
//! every PATH entry, then the repository walk.

use std::env;
use std::fs::Metadata;
use std::path::{Path, PathBuf};

/// The binary's base name.
pub const BINARY_NAME: &str = "ctx";
/// What the failure panel's *Copy build command* copies.
pub const BUILD_COMMAND: &str = "cargo build --release -p ctx-cli";
/// Ancestors visited at most by the repository walk.
const WALK_LIMIT: usize = 64;

/// `locate`'s result: the first executable candidate and every path tried.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Located {
    pub found: Option<PathBuf>,
    pub searched: Vec<PathBuf>,
}

/// `BackendLocator.locate(preferences:)`: `override_path` is the user's
/// backend path override (or `--binary`), tried first.
pub fn locate(override_path: Option<&str>) -> Located {
    let mut searched = Vec::new();
    for candidate in candidates(override_path) {
        searched.push(candidate.clone());
        if is_executable(&candidate) {
            return Located {
                found: Some(candidate),
                searched,
            };
        }
    }
    Located { found: None, searched }
}

/// Every path that would be tried, in order, without touching the disk
/// (except for the repository walk, which needs `Cargo.toml` to exist).
pub fn candidates(override_path: Option<&str>) -> Vec<PathBuf> {
    let name = binary_file_name();
    let mut list: Vec<PathBuf> = Vec::new();

    if let Some(path) = override_path.map(str::trim).filter(|path| !path.is_empty()) {
        list.push(expand_tilde(path));
    }

    // The sidecar sits next to the app executable (`Contents/MacOS` inside a bundle).
    let exe_dir = env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf));
    if let Some(dir) = &exe_dir {
        if cfg!(target_os = "macos") {
            if let Some(bundle) = bundle_root(dir) {
                list.push(bundle.join("Contents").join("Helpers").join(&name));
            }
        }
        list.push(dir.join(&name));
    }

    if let Some(path) = env::var("CTX_BINARY").ok().map(|value| value.trim().to_owned()).filter(|value| !value.is_empty()) {
        list.push(expand_tilde(&path));
    }

    if let Some(home) = dirs::home_dir() {
        list.push(home.join(".cargo").join("bin").join(&name));
    }
    if cfg!(target_os = "macos") {
        list.push(PathBuf::from("/opt/homebrew/bin").join(&name));
        list.push(PathBuf::from("/usr/local/bin").join(&name));
    } else if cfg!(windows) {
        if let Some(local) = dirs::data_local_dir() {
            list.push(local.join("Programs").join("Context").join(&name));
        }
        list.extend(path_candidates());
    } else {
        list.push(PathBuf::from("/usr/local/bin").join(&name));
        list.extend(path_candidates());
    }

    if let Some(dir) = &exe_dir {
        list.extend(repository_candidates(dir));
    }
    if let Ok(cwd) = env::current_dir() {
        list.extend(repository_candidates(&cwd));
    }

    dedupe(list)
}

/// Walks up from `start`; every ancestor holding a `Cargo.toml` contributes
/// `target/release/ctx` then `target/debug/ctx`.
pub fn repository_candidates(start: &Path) -> Vec<PathBuf> {
    let name = binary_file_name();
    let mut results = Vec::new();
    let mut dir = start.to_path_buf();
    for _ in 0..WALK_LIMIT {
        if dir.join("Cargo.toml").is_file() {
            results.push(dir.join("target").join("release").join(&name));
            results.push(dir.join("target").join("debug").join(&name));
        }
        let parent = dir.parent().map(Path::to_path_buf);
        match parent {
            Some(parent) if parent != dir => dir = parent,
            _ => break,
        }
    }
    results
}

/// `isExecutable`: an existing regular file (through symlinks) the user may
/// run; on Windows any regular file with the binary's name counts.
pub fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    metadata.is_file() && mode_allows_execution(&metadata)
}

/// `(path as NSString).expandingTildeInPath`.
pub fn expand_tilde(path: &str) -> PathBuf {
    if path == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(path));
    }
    let rest = path
        .strip_prefix("~/")
        .or_else(|| if cfg!(windows) { path.strip_prefix("~\\") } else { None });
    match (rest, dirs::home_dir()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(path),
    }
}

#[cfg(unix)]
fn mode_allows_execution(metadata: &Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn mode_allows_execution(_metadata: &Metadata) -> bool {
    true
}

fn binary_file_name() -> String {
    if cfg!(windows) {
        format!("{BINARY_NAME}.exe")
    } else {
        BINARY_NAME.to_owned()
    }
}

/// `Context.app` for an executable directory `Context.app/Contents/MacOS`.
fn bundle_root(exe_dir: &Path) -> Option<PathBuf> {
    if exe_dir.file_name()? != "MacOS" {
        return None;
    }
    let contents = exe_dir.parent()?;
    if contents.file_name()? != "Contents" {
        return None;
    }
    contents.parent().map(Path::to_path_buf)
}

/// `ctx` in every PATH entry; on Windows one name per `PATHEXT` extension.
fn path_candidates() -> Vec<PathBuf> {
    let Some(path) = env::var_os("PATH") else {
        return Vec::new();
    };
    let names = if cfg!(windows) { pathext_names() } else { vec![BINARY_NAME.to_owned()] };
    let mut list = Vec::new();
    for dir in env::split_paths(&path).filter(|dir| !dir.as_os_str().is_empty()) {
        for name in &names {
            list.push(dir.join(name));
        }
    }
    list
}

/// `ctx<ext>` for every extension in `PATHEXT` (default `.COM;.EXE;.BAT;.CMD`).
fn pathext_names() -> Vec<String> {
    let pathext = env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_owned());
    let mut names: Vec<String> = pathext
        .split(';')
        .map(str::trim)
        .filter(|ext| !ext.is_empty())
        .map(|ext| format!("{BINARY_NAME}{}", ext.to_ascii_lowercase()))
        .collect();
    if names.is_empty() {
        names.push(binary_file_name());
    }
    names
}

/// Keeps the first occurrence of every path (case-insensitively on Windows).
fn dedupe(list: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = std::collections::HashSet::new();
    list.into_iter()
        .filter(|path| {
            let key = path.to_string_lossy().into_owned();
            let key = if cfg!(windows) { key.to_lowercase() } else { key };
            seen.insert(key)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_comes_first_and_is_tilde_expanded() {
        let list = candidates(Some("  ~/bin/ctx  "));
        let home = dirs::home_dir().unwrap();
        assert_eq!(list[0], home.join("bin").join("ctx"));
    }

    #[test]
    fn repository_walk_finds_this_checkout() {
        let here = Path::new(env!("CARGO_MANIFEST_DIR"));
        let found = repository_candidates(here);
        assert!(found.iter().any(|path| path.ends_with(Path::new("target").join("release").join(binary_file_name()))));
        // apps/desktop/src-tauri → the repository root is three levels up.
        let repo_root = here.ancestors().nth(3).unwrap();
        assert!(found.iter().any(|path| path.starts_with(repo_root.join("target"))));
    }

    #[test]
    fn bundle_root_needs_contents_macos() {
        assert_eq!(
            bundle_root(Path::new("/Applications/Context.app/Contents/MacOS")),
            Some(PathBuf::from("/Applications/Context.app"))
        );
        assert_eq!(bundle_root(Path::new("/tmp/target/debug")), None);
    }

    #[test]
    fn candidates_are_unique() {
        let list = candidates(None);
        let mut seen = std::collections::HashSet::new();
        assert!(list.iter().all(|path| seen.insert(path.clone())));
    }
}
