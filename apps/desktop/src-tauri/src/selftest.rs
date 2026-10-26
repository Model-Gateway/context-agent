//! Launch options: the port of `App/LaunchOptions.swift` (ARCHITECTURE.md
//! "Self-test mode"). Command-line and environment switches read once at
//! launch:
//!
//! - `--mock` or `CTX_DESKTOP_MOCK=1`: start the backend with `ctx serve --mock`.
//! - `--project <path>`: open this folder at launch instead of the remembered projects.
//! - `--autotest <dir>`: run the scripted self-test (`src/selftest.ts`), write
//!   `result.json` into `dir` (`selftest_finish`), then exit.
//! - `--binary <path>`: backend binary override for this launch only.
//!
//! The scripted flow itself lives in the frontend, which reads these through
//! the `launch_options` command. `CTX_DATA_DIR` is not parsed here: the
//! frontend passes it to `backend_start` as `data_dir`.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::backend::locator::expand_tilde;

/// `LaunchOptions`, as `launch_options()` returns it: `{ mock, project, binary, autotest }`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchOptions {
    /// `--mock` / `CTX_DESKTOP_MOCK=1`.
    pub mock: bool,
    /// `--project <path>` (tilde expanded, otherwise as given).
    pub project: Option<String>,
    /// `--binary <path>` (tilde expanded, otherwise as given).
    pub binary: Option<String>,
    /// `--autotest <dir>` (tilde expanded, otherwise as given).
    pub autotest: Option<String>,
}

impl LaunchOptions {
    /// `LaunchOptions.current`: the options of the running process.
    pub fn current() -> &'static LaunchOptions {
        static CURRENT: OnceLock<LaunchOptions> = OnceLock::new();
        CURRENT.get_or_init(|| {
            let arguments = std::env::args_os().map(|arg| arg.to_string_lossy().into_owned());
            let environment: HashMap<String, String> = std::env::vars().collect();
            Self::parse(arguments, &environment)
        })
    }

    /// Parses `arguments` (the first element is the executable and is skipped)
    /// and `environment`. A flag that expects a value is ignored when the value
    /// is missing or looks like another flag; `--flag=value` forms work too and
    /// unknown switches are ignored.
    pub fn parse<I>(arguments: I, environment: &HashMap<String, String>) -> Self
    where
        I: IntoIterator<Item = String>,
    {
        let args: Vec<String> = arguments.into_iter().skip(1).collect();
        let mut options = Self::default();
        let mut index = 0;
        while index < args.len() {
            let arg = args[index].as_str();
            let next = args
                .get(index + 1)
                .filter(|candidate| !candidate.starts_with("--"))
                .map(String::as_str);
            match arg {
                "--mock" => options.mock = true,
                "--project" => {
                    if let Some(value) = next {
                        options.project = Some(expand(value));
                        index += 1;
                    }
                }
                "--autotest" => {
                    if let Some(value) = next {
                        options.autotest = Some(expand(value));
                        index += 1;
                    }
                }
                "--binary" => {
                    if let Some(value) = next {
                        options.binary = Some(expand(value));
                        index += 1;
                    }
                }
                _ => {
                    if let Some(value) = value_of("--project", arg) {
                        options.project = Some(value);
                    } else if let Some(value) = value_of("--autotest", arg) {
                        options.autotest = Some(value);
                    } else if let Some(value) = value_of("--binary", arg) {
                        options.binary = Some(value);
                    }
                }
            }
            index += 1;
        }
        if environment.get("CTX_DESKTOP_MOCK").is_some_and(|value| is_truthy(value)) {
            options.mock = true;
        }
        options
    }

    /// True when `--autotest <dir>` was given.
    pub fn is_autotest(&self) -> bool {
        self.autotest.is_some()
    }
}

fn expand(path: &str) -> String {
    expand_tilde(path).to_string_lossy().into_owned()
}

/// `--flag=value` → `value`.
fn value_of(flag: &str, argument: &str) -> Option<String> {
    let value = argument.strip_prefix(flag)?.strip_prefix('=')?;
    (!value.is_empty()).then(|| expand(value))
}

fn is_truthy(value: &str) -> bool {
    matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

/// `launch_options()` → `{ mock, project, binary, autotest }`.
#[tauri::command]
pub fn launch_options() -> Result<LaunchOptions, String> {
    Ok(LaunchOptions::current().clone())
}

/// `selftest_finish({ dir, result_json, exit_code })`: the end of the
/// `--autotest` run (`src/selftest.ts`, `UITestDriver.finish`). Writes
/// `result_json` to `<dir>/result.json` (creating the directory), then exits
/// the app with `exit_code` (0 passed, 1 failed); quitting stops the backend
/// child through the `RunEvent::Exit` handler in `lib.rs`. Returns the
/// written path (the exit follows on the event loop). When the file cannot
/// be written the app still exits, with code 1.
#[tauri::command(rename_all = "snake_case")]
pub async fn selftest_finish(app: tauri::AppHandle, dir: String, result_json: String, exit_code: i32) -> Result<String, String> {
    let directory = expand_tilde(&dir);
    let path = directory.join("result.json");
    let written = tauri::async_runtime::spawn_blocking({
        let directory = directory.clone();
        let path = path.clone();
        move || -> Result<(), String> {
            std::fs::create_dir_all(&directory).map_err(|error| format!("Could not create {}: {error}", directory.display()))?;
            std::fs::write(&path, result_json.as_bytes()).map_err(|error| format!("Could not write {}: {error}", path.display()))
        }
    })
    .await
    .map_err(|error| format!("Could not write result.json: {error}"))
    .and_then(|result| result);
    match written {
        Ok(()) => {
            eprintln!("[autotest] wrote {} (exit {exit_code})", path.display());
            app.exit(exit_code);
            Ok(path.to_string_lossy().into_owned())
        }
        Err(message) => {
            eprintln!("[autotest] {message}");
            app.exit(1);
            Err(message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str], env: &[(&str, &str)]) -> LaunchOptions {
        let arguments = std::iter::once("app".to_owned()).chain(args.iter().map(|arg| (*arg).to_owned()));
        let environment = env.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
        LaunchOptions::parse(arguments, &environment)
    }

    #[test]
    fn parses_flags_with_values() {
        let options = parse(&["--mock", "--project", "/p", "--autotest", "/a", "--binary", "/b"], &[]);
        assert!(options.mock);
        assert_eq!(options.project.as_deref(), Some("/p"));
        assert_eq!(options.autotest.as_deref(), Some("/a"));
        assert_eq!(options.binary.as_deref(), Some("/b"));
        assert!(options.is_autotest());
    }

    #[test]
    fn ignores_missing_values_and_unknown_switches() {
        let options = parse(&["--project", "--mock", "-NSDocumentRevisionsDebugMode", "YES"], &[]);
        assert!(options.mock);
        assert_eq!(options.project, None);
    }

    #[test]
    fn accepts_equals_forms_and_the_environment() {
        let options = parse(&["--project=/x", "--binary="], &[("CTX_DESKTOP_MOCK", " Yes ")]);
        assert!(options.mock);
        assert_eq!(options.project.as_deref(), Some("/x"));
        assert_eq!(options.binary, None);
        assert!(!parse(&[], &[("CTX_DESKTOP_MOCK", "0")]).mock);
    }

    #[test]
    fn expands_tilde() {
        let options = parse(&["--project", "~/code"], &[]);
        let home = dirs::home_dir().unwrap();
        assert_eq!(options.project.as_deref(), Some(home.join("code").to_string_lossy().as_ref()));
    }
}
