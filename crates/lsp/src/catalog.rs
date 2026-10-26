//! Known language servers and their availability on this machine.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServerSpec {
    pub language: &'static str,
    pub executable: &'static str,
    pub args: &'static [&'static str],
    pub install_hint: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerStatus {
    pub language: String,
    pub executable: String,
    pub installed: bool,
    pub path: Option<String>,
    pub install_hint: String,
}

pub const SERVERS: &[ServerSpec] = &[
    ServerSpec { language: "typescript", executable: "typescript-language-server", args: &["--stdio"], install_hint: "npm install -g typescript-language-server typescript" },
    ServerSpec { language: "javascript", executable: "typescript-language-server", args: &["--stdio"], install_hint: "npm install -g typescript-language-server typescript" },
    ServerSpec { language: "python", executable: "pyright-langserver", args: &["--stdio"], install_hint: "npm install -g pyright" },
    ServerSpec { language: "rust", executable: "rust-analyzer", args: &[], install_hint: "rustup component add rust-analyzer" },
    ServerSpec { language: "go", executable: "gopls", args: &[], install_hint: "go install golang.org/x/tools/gopls@latest" },
    ServerSpec { language: "java", executable: "jdtls", args: &[], install_hint: "install Eclipse JDT Language Server (jdtls)" },
    ServerSpec { language: "c", executable: "clangd", args: &[], install_hint: "install clangd from LLVM" },
    ServerSpec { language: "cpp", executable: "clangd", args: &[], install_hint: "install clangd from LLVM" },
    ServerSpec { language: "ruby", executable: "solargraph", args: &["stdio"], install_hint: "gem install solargraph" },
    ServerSpec { language: "php", executable: "intelephense", args: &["--stdio"], install_hint: "npm install -g intelephense" },
    ServerSpec { language: "markdown", executable: "marksman", args: &["server"], install_hint: "brew install marksman" },
    ServerSpec { language: "html", executable: "vscode-html-language-server", args: &["--stdio"], install_hint: "npm install -g vscode-langservers-extracted" },
    ServerSpec { language: "css", executable: "vscode-css-language-server", args: &["--stdio"], install_hint: "npm install -g vscode-langservers-extracted" },
    ServerSpec { language: "json", executable: "vscode-json-language-server", args: &["--stdio"], install_hint: "npm install -g vscode-langservers-extracted" },
    ServerSpec { language: "yaml", executable: "yaml-language-server", args: &["--stdio"], install_hint: "npm install -g yaml-language-server" },
    ServerSpec { language: "toml", executable: "taplo", args: &["lsp", "stdio"], install_hint: "cargo install taplo-cli --locked --features lsp" },
    ServerSpec { language: "shell", executable: "bash-language-server", args: &["start"], install_hint: "npm install -g bash-language-server" },
];

pub struct ServerCatalog;

impl ServerCatalog {
    pub fn spec_for(language: &str) -> Option<&'static ServerSpec> {
        SERVERS.iter().find(|s| s.language == language)
    }

    /// The full path of `executable` on `PATH` (`PATHEXT` on Windows:
    /// `typescript-language-server` is `typescript-language-server.cmd`).
    pub fn which(executable: &str) -> Option<String> {
        ctx_core::platform::find_executable(executable).map(|p| p.to_string_lossy().to_string())
    }

    pub fn status(language: &str) -> Option<ServerStatus> {
        let spec = Self::spec_for(language)?;
        if let Some((executable, _)) = Self::override_for(language) {
            return Some(ServerStatus { language: language.to_string(), executable: executable.clone(), installed: true, path: Some(executable), install_hint: spec.install_hint.to_string() });
        }
        let path = Self::which(spec.executable);
        Some(ServerStatus { language: language.to_string(), executable: spec.executable.to_string(), installed: path.is_some(), path, install_hint: spec.install_hint.to_string() })
    }

    /// `CTX_LSP_COMMAND_<LANGUAGE>` ("rust-analyzer", "python3 -I fake.py"):
    /// a server of the user's own choice. A value naming an existing file is
    /// the executable as a whole (`C:\Program Files\LLVM\bin\clangd.exe`);
    /// otherwise the first word is the executable and the rest its
    /// arguments, where a double-quoted word may contain spaces.
    pub fn override_for(language: &str) -> Option<(String, Vec<String>)> {
        let value = std::env::var(format!("CTX_LSP_COMMAND_{}", language.to_uppercase())).ok()?;
        let value = value.trim();
        if value.is_empty() {
            return None;
        }
        if std::path::Path::new(value).is_file() {
            return Some((value.to_string(), Vec::new()));
        }
        let mut parts = split_command_line(value).into_iter();
        let executable = parts.next()?;
        Some((executable, parts.collect()))
    }

    /// The command that starts the language's server: the override, else
    /// the catalog's executable when it is installed.
    pub fn command_for(language: &str) -> Option<(String, Vec<String>)> {
        if let Some(command) = Self::override_for(language) {
            return Some(command);
        }
        let spec = Self::spec_for(language)?;
        let path = Self::which(spec.executable)?;
        Some((path, spec.args.iter().map(|a| a.to_string()).collect()))
    }

    pub fn all_statuses() -> Vec<ServerStatus> {
        let mut seen = std::collections::HashSet::new();
        SERVERS.iter().filter(|s| seen.insert(s.language)).filter_map(|s| Self::status(s.language)).collect()
    }
}

/// Splits on whitespace, keeping double-quoted runs together (quotes
/// removed). Backslashes are literal, so Windows paths survive.
fn split_command_line(value: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut has_word = false;
    for c in value.chars() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
                has_word = true;
            }
            c if c.is_whitespace() && !in_quotes => {
                if has_word {
                    words.push(std::mem::take(&mut current));
                    has_word = false;
                }
            }
            c => {
                current.push(c);
                has_word = true;
            }
        }
    }
    if has_word {
        words.push(current);
    }
    words
}

#[cfg(test)]
mod split_tests {
    #[test]
    fn quoted_words_keep_their_spaces_and_backslashes() {
        assert_eq!(super::split_command_line("python3 -I fake.py"), vec!["python3", "-I", "fake.py"]);
        assert_eq!(super::split_command_line(r#""C:\Program Files\LLVM\bin\clangd.exe" --log=error"#), vec![r"C:\Program Files\LLVM\bin\clangd.exe", "--log=error"]);
        assert_eq!(super::split_command_line(r#"node "" x"#), vec!["node", "", "x"]);
    }
}
