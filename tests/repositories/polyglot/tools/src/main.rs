//! `tools`: the release helper for the polyglot monorepo.
//!
//! The monorepo has no single package manifest, so this crate is the one
//! place the repo-wide version lives. CI and `scripts/build.sh` call
//! `tools version` to stamp builds; everything else in the repo is Python
//! or JavaScript.
//!
//! ```text
//! $ tools            # same as `tools version`
//! polyglot 0.1.0
//! $ tools version --json
//! {"name":"polyglot","version":"0.1.0","tool":"tools 0.1.0"}
//! ```

use std::env;
use std::process::ExitCode;

/// Version of the monorepo as a whole. Bump this when cutting a release.
const REPO_VERSION: &str = "0.1.0";
const REPO_NAME: &str = "polyglot";

/// Render the version banner, optionally as JSON.
fn version_string(json: bool) -> String {
    let tool = format!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
    if json {
        format!(
            "{{\"name\":\"{}\",\"version\":\"{}\",\"tool\":\"{}\"}}",
            REPO_NAME, REPO_VERSION, tool
        )
    } else {
        format!("{} {}", REPO_NAME, REPO_VERSION)
    }
}

fn usage() -> &'static str {
    "usage: tools [version] [--json]\n       tools help"
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut json = false;
    let mut command: Option<&str> = None;

    for arg in &args {
        match arg.as_str() {
            "--json" => json = true,
            "-h" | "--help" | "help" => {
                println!("{}", usage());
                return ExitCode::SUCCESS;
            }
            other if command.is_none() => command = Some(other),
            other => {
                eprintln!("tools: unexpected argument {:?}\n{}", other, usage());
                return ExitCode::FAILURE;
            }
        }
    }

    match command {
        None | Some("version") => {
            println!("{}", version_string(json));
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("tools: unknown command {:?}\n{}", other, usage());
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_banner_names_the_repo() {
        assert_eq!(version_string(false), "polyglot 0.1.0");
    }

    #[test]
    fn json_banner_is_well_formed() {
        let s = version_string(true);
        assert!(s.starts_with("{\"name\":\"polyglot\""));
        assert!(s.contains("\"version\":\"0.1.0\""));
        assert!(s.ends_with("\"tool\":\"tools 0.1.0\"}"));
    }
}
