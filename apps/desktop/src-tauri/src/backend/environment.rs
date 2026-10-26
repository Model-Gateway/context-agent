//! The environment handed to `ctx serve`: the port of `Environment.swift`'s
//! `LoginShellEnvironment` and `BackendEnvironment` (its `KeychainStore` half
//! is `crate::secrets`).
//!
//! An app launched from Finder or the Start menu only gets the session
//! manager's PATH, so without the login shell the agent's commands cannot
//! find pnpm, node, brew tools and the like. On macOS and Linux the login
//! shell's variables are captured once per app run (`$SHELL -ilc`, else
//! `/bin/zsh`, 5 s timeout) and merged under the process environment; PATH
//! becomes the login shell's, then the extra directories, then the process's
//! own, joined with the platform separator. The API key variable comes from
//! the secret store when the process does not export it.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::OnceCell;

use super::locator::expand_tilde;
use super::{debug, terminate_pid};

/// A process environment.
pub type Environment = HashMap<String, String>;

/// Printed before the variables so rc-file chatter is skipped.
pub const LOGIN_SHELL_MARKER: &[u8] = b"\0__CTX_ENV__\0";
/// `printenv -0` where it exists (GNU coreutils); BSD `env -0` on stock macOS.
pub const LOGIN_SHELL_SCRIPT: &str = "printf \"\\0__CTX_ENV__\\0\"; printenv -0 2>/dev/null || env -0";
/// `LoginShellEnvironment.timeout`.
pub const LOGIN_SHELL_TIMEOUT: Duration = Duration::from_secs(5);
/// Login-shell variables that describe that shell run, not the user's setup.
pub const SHELL_LOCAL_VARIABLES: &[&str] = &["PWD", "OLDPWD", "SHLVL", "_", "TERM", "CTX_DESKTOP"];

const PATH_SEPARATOR: &str = if cfg!(windows) { ";" } else { ":" };

static LOGIN_SHELL: OnceCell<Environment> = OnceCell::const_new();

/// `ProcessInfo.processInfo.environment`.
pub fn process_environment() -> Environment {
    std::env::vars().collect()
}

/// `BackendEnvironment.extraPathDirectories`: added to PATH so `ctx` finds
/// cargo and the installed tools even when the login shell could not be
/// captured. `~/.cargo/bin` everywhere; `/opt/homebrew/bin` and
/// `/usr/local/bin` on macOS; `%LOCALAPPDATA%\Programs\Context` on Windows;
/// `/usr/local/bin` on Linux.
pub fn extra_path_directories() -> Vec<PathBuf> {
    let mut directories = Vec::new();
    if let Some(home) = dirs::home_dir() {
        directories.push(home.join(".cargo").join("bin"));
    }
    if cfg!(windows) {
        if let Some(local) = dirs::data_local_dir() {
            directories.push(local.join("Programs").join("Context"));
        }
    } else if cfg!(target_os = "macos") {
        directories.push(PathBuf::from("/opt/homebrew/bin"));
        directories.push(PathBuf::from("/usr/local/bin"));
    } else {
        directories.push(PathBuf::from("/usr/local/bin"));
    }
    directories
}

/// `LoginShellEnvironment.snapshot()`: cached after the first call; falls
/// back to the process environment (always, on Windows).
pub async fn login_shell_snapshot() -> Environment {
    LOGIN_SHELL
        .get_or_init(|| async { capture_login_shell().await.unwrap_or_else(process_environment) })
        .await
        .clone()
}

/// `BackendEnvironment.build(apiKeyEnv:)`: process environment + the login
/// shell's variables and PATH + the API key variable (secret store, else the
/// login shell) when `api_key_env` names one that is not set yet. The secret
/// store beats the login shell; an exported variable beats both.
pub async fn build(api_key_env: Option<&str>) -> Environment {
    let process = process_environment();
    let login = login_shell_snapshot().await;
    let mut environment = merged(&process, &login);

    if let Some(name) = api_key_env.map(str::trim).filter(|name| !name.is_empty()) {
        let exported = process.get(name).is_some_and(|value| !value.is_empty());
        if !exported {
            let account = name.to_owned();
            let stored = tauri::async_runtime::spawn_blocking(move || crate::secrets::get(&account)).await;
            match stored {
                Ok(Ok(Some(key))) if !key.is_empty() => {
                    environment.insert(name.to_owned(), key);
                }
                Ok(Ok(_)) => {}
                Ok(Err(error)) => debug(|| format!("secret store lookup for {name} failed: {error}")),
                Err(error) => debug(|| format!("secret store task failed: {error}")),
            }
        }
    }
    environment
}

/// `BackendEnvironment.merged`: variables the process lacks come from the
/// login shell (minus the shell-local ones); PATH is the login shell's, then
/// the extra directories, then the process's own.
pub fn merged(process: &Environment, login: &Environment) -> Environment {
    let mut environment = process.clone();
    for (name, value) in login {
        if !environment.contains_key(name) && !SHELL_LOCAL_VARIABLES.contains(&name.as_str()) {
            environment.insert(name.clone(), value.clone());
        }
    }
    let key = path_key(process).unwrap_or_else(|| "PATH".to_owned());
    let merged = merged_path(path_value(login).as_deref(), path_value(process).as_deref());
    environment.insert(key, merged);
    environment
}

/// `BackendEnvironment.mergedPath`: login PATH + extras + current PATH,
/// deduplicated, joined with `:` (`;` on Windows).
pub fn merged_path(login: Option<&str>, current: Option<&str>) -> String {
    let split = |value: Option<&str>| -> Vec<String> {
        value
            .map(|value| {
                std::env::split_paths(value)
                    .filter(|dir| !dir.as_os_str().is_empty())
                    .map(|dir| dir.to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    };
    let extra = extra_path_directories()
        .into_iter()
        .map(|dir| dir.to_string_lossy().into_owned());
    let mut seen = HashSet::new();
    let mut merged = Vec::new();
    for dir in split(login).into_iter().chain(extra).chain(split(current)) {
        let key = if cfg!(windows) { dir.to_lowercase() } else { dir.clone() };
        if seen.insert(key) {
            merged.push(dir);
        }
    }
    merged.join(PATH_SEPARATOR)
}

/// The name PATH is stored under (`Path` on Windows, where names are
/// case-insensitive).
pub fn path_key(environment: &Environment) -> Option<String> {
    environment.keys().find(|key| key.eq_ignore_ascii_case("PATH")).cloned()
}

fn path_value(environment: &Environment) -> Option<String> {
    path_key(environment).and_then(|key| environment.get(&key).cloned())
}

/// `LoginShellEnvironment.parse`: `<anything><marker>NAME=value\0NAME=value\0…`.
/// `None` when the marker is absent.
pub fn parse_login_shell_output(data: &[u8]) -> Option<Environment> {
    let marker = memchr::memmem::rfind(data, LOGIN_SHELL_MARKER)?;
    let payload = &data[marker + LOGIN_SHELL_MARKER.len()..];
    let mut result = Environment::new();
    for entry in payload.split(|byte| *byte == 0).filter(|entry| !entry.is_empty()) {
        let text = String::from_utf8_lossy(entry);
        let Some(equals) = text.find('=') else { continue };
        let name = &text[..equals];
        if name.is_empty() {
            continue;
        }
        result.insert(name.to_owned(), text[equals + 1..].to_owned());
    }
    Some(result)
}

/// `$SHELL` when it is an existing file, else `/bin/zsh`, else `/bin/bash`.
fn login_shell_path() -> Option<PathBuf> {
    let from_env = std::env::var("SHELL")
        .ok()
        .map(|shell| shell.trim().to_owned())
        .filter(|shell| !shell.is_empty())
        .map(|shell| expand_tilde(&shell))
        .filter(|shell| shell.is_file());
    from_env.or_else(|| {
        ["/bin/zsh", "/bin/bash"]
            .iter()
            .map(Path::new)
            .find(|shell| shell.is_file())
            .map(Path::to_path_buf)
    })
}

/// `LoginShellEnvironment.capture`: runs the login shell with stdin and
/// stderr discarded, `TERM=dumb`, `CTX_DESKTOP=1`; terminates then kills it
/// after the timeout. `None` on Windows, when the shell does not start or
/// when it prints nothing usable.
async fn capture_login_shell() -> Option<Environment> {
    if cfg!(windows) {
        return None;
    }
    let shell = login_shell_path()?;
    let mut command = Command::new(&shell);
    command
        .args(["-ilc", LOGIN_SHELL_SCRIPT])
        .env("TERM", "dumb")
        .env("CTX_DESKTOP", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            debug(|| format!("login shell {} did not start: {error}", shell.display()));
            return None;
        }
    };
    let mut stdout = child.stdout.take()?;

    let deadline = tokio::time::Instant::now() + LOGIN_SHELL_TIMEOUT;
    let mut collected = Vec::new();
    let mut chunk = vec![0u8; 8192];
    let mut timed_out = false;
    loop {
        match tokio::time::timeout_at(deadline, stdout.read(&mut chunk)).await {
            Ok(Ok(0)) | Ok(Err(_)) => break,
            Ok(Ok(read)) => collected.extend_from_slice(&chunk[..read]),
            Err(_) => {
                timed_out = true;
                break;
            }
        }
    }

    if timed_out {
        debug(|| format!("login shell timed out after {} s; terminating", LOGIN_SHELL_TIMEOUT.as_secs()));
        if !child.id().is_some_and(terminate_pid) {
            let _ = child.start_kill();
        }
        if tokio::time::timeout(Duration::from_secs(1), child.wait()).await.is_err() {
            let _ = child.start_kill();
            let _ = tokio::time::timeout(Duration::from_secs(1), child.wait()).await;
        }
    } else {
        let _ = tokio::time::timeout(Duration::from_secs(1), child.wait()).await;
    }

    match parse_login_shell_output(&collected).filter(|parsed| !parsed.is_empty()) {
        Some(parsed) => {
            debug(|| format!("login shell environment captured: {} variables", parsed.len()));
            Some(parsed)
        }
        None => {
            debug(|| format!("login shell produced no usable environment ({} bytes)", collected.len()));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_variables_after_the_last_marker() {
        let mut data = b"rc chatter\n".to_vec();
        data.extend_from_slice(LOGIN_SHELL_MARKER);
        data.extend_from_slice(b"HOME=/Users/me\0PATH=/a:/b\0WEIRD\0=novalue\0MULTI=a=b\0");
        let parsed = parse_login_shell_output(&data).unwrap();
        assert_eq!(parsed.get("HOME").map(String::as_str), Some("/Users/me"));
        assert_eq!(parsed.get("PATH").map(String::as_str), Some("/a:/b"));
        assert_eq!(parsed.get("MULTI").map(String::as_str), Some("a=b"));
        assert!(!parsed.contains_key("WEIRD"));
        assert!(!parsed.contains_key(""));
        assert_eq!(parse_login_shell_output(b"no marker here"), None);
    }

    #[test]
    fn merged_path_prefers_login_then_extras_then_current() {
        let merged = merged_path(Some("/login/bin:/usr/bin"), Some("/usr/bin:/current/bin"));
        let parts: Vec<&str> = merged.split(PATH_SEPARATOR).collect();
        assert_eq!(parts[0], "/login/bin");
        assert_eq!(parts[1], "/usr/bin");
        assert_eq!(parts.last().copied(), Some("/current/bin"));
        assert_eq!(parts.iter().filter(|part| **part == "/usr/bin").count(), 1);
        let cargo = dirs::home_dir().unwrap().join(".cargo").join("bin");
        assert!(parts.contains(&cargo.to_string_lossy().as_ref()));
    }

    #[test]
    fn merged_skips_shell_local_variables_and_keeps_process_values() {
        let process: Environment = [("PATH", "/p"), ("EDITOR", "vim")]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect();
        let login: Environment = [("PATH", "/l"), ("EDITOR", "nano"), ("PWD", "/x"), ("NPM_TOKEN", "t")]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect();
        let merged = merged(&process, &login);
        assert_eq!(merged["EDITOR"], "vim");
        assert_eq!(merged["NPM_TOKEN"], "t");
        assert!(!merged.contains_key("PWD"));
        assert!(merged["PATH"].starts_with("/l"));
        assert!(merged["PATH"].ends_with("/p"));
    }
}
