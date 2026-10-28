//! How a `ctx serve` child is started: the port of `BackendLaunch.swift` and
//! the `SSHHost` model (apps/macos …/Models/SSHHost.swift). Either the local
//! binary runs `ctx serve`, or `ssh` runs it on a remote host and the same
//! newline-delimited JSON-RPC flows through the SSH session's stdin and
//! stdout (docs/desktop-protocol.md "Remote hosts"). Nothing here reaches
//! the network: `ssh` is the child process, and its authentication, config
//! file and jumps apply as in a terminal.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::locator::expand_tilde;

/// A machine the app can run `ctx serve` on over SSH (Settings → Remote
/// hosts). `address` is what `ssh` is given (a hostname, an IP, or a `Host`
/// alias from `~/.ssh/config`); the optional fields override that file.
/// The field names are the Swift `SSHHost`'s (its JSON is the same shape).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshHost {
    pub id: String,
    /// Shown in the sidebar and the recents; defaults to the address.
    #[serde(default)]
    pub name: String,
    pub address: String,
    #[serde(default)]
    pub user: String,
    /// 0 = ssh's default (or the config file's).
    #[serde(default)]
    pub port: u32,
    /// `-i`: a private key file ("~/.ssh/id_ed25519"); empty = ssh's choice.
    #[serde(default)]
    pub identity_file: String,
    /// The remote `ctx` binary; empty = "ctx" on the remote PATH (plus the
    /// usual install directories, see [`REMOTE_PATH`]).
    #[serde(default)]
    pub ctx_path: String,
    /// Extra `ssh` options, one per entry ("-o", "ProxyJump=bastion").
    #[serde(default)]
    pub extra_arguments: Vec<String>,
}

impl SshHost {
    /// `SSHHost.displayName`.
    pub fn display_name(&self) -> &str {
        let trimmed = self.name.trim();
        if trimmed.is_empty() {
            self.address.trim()
        } else {
            trimmed
        }
    }

    /// `SSHHost.target`: "alice@devbox:2222", for messages.
    pub fn target(&self) -> String {
        let mut text = if self.user.is_empty() {
            self.address.clone()
        } else {
            format!("{}@{}", self.user, self.address)
        };
        if self.port > 0 {
            text.push_str(&format!(":{}", self.port));
        }
        text
    }

    /// `SSHHost.isValid`: something `ssh` can be given.
    pub fn is_valid(&self) -> bool {
        let trimmed = self.address.trim();
        !trimmed.is_empty() && !trimmed.chars().any(char::is_whitespace) && !trimmed.starts_with('-') && self.port <= 65535
    }
}

/// Directories a non-interactive remote shell may lack on its PATH but
/// where `ctx` is usually installed (`BackendLaunch.remotePath`).
pub const REMOTE_PATH: &str = "$HOME/.cargo/bin:$HOME/.local/bin:/usr/local/bin:/opt/homebrew/bin:$PATH";

/// `ssh` options every connection gets (`BackendLaunch.sshOptions`): no
/// password prompts (there is nobody to answer them; keys and the agent
/// work), a bounded connect, keep-alives that notice a dropped link, no
/// forwardings, no tty.
pub const SSH_OPTIONS: &[&str] = &[
    "-o",
    "BatchMode=yes",
    "-o",
    "ConnectTimeout=20",
    "-o",
    "ServerAliveInterval=15",
    "-o",
    "ServerAliveCountMax=4",
    "-o",
    "ClearAllForwardings=yes",
    "-T",
];

/// `BackendLaunch`: the local binary, or `ssh` to a host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendLaunch {
    Local { binary: PathBuf },
    Ssh(SshHost),
}

impl BackendLaunch {
    /// The ssh client: `$CTX_SSH_BINARY` when set (the self-test points it
    /// at a script that runs the local `ctx`, so the transport is exercised
    /// without a server), else OpenSSH's (`ssh.exe` ships with Windows).
    pub fn ssh_binary() -> PathBuf {
        if let Some(override_path) = std::env::var("CTX_SSH_BINARY").ok().map(|value| value.trim().to_owned()).filter(|value| !value.is_empty()) {
            return expand_tilde(&override_path);
        }
        if cfg!(windows) {
            let system32 = std::env::var_os("SystemRoot")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("C:\\Windows"))
                .join("System32")
                .join("OpenSSH")
                .join("ssh.exe");
            if system32.is_file() {
                return system32;
            }
            PathBuf::from("ssh.exe")
        } else {
            PathBuf::from("/usr/bin/ssh")
        }
    }

    /// The program to spawn.
    pub fn executable(&self) -> PathBuf {
        match self {
            BackendLaunch::Local { binary } => binary.clone(),
            BackendLaunch::Ssh(_) => Self::ssh_binary(),
        }
    }

    pub fn host(&self) -> Option<&SshHost> {
        match self {
            BackendLaunch::Ssh(host) => Some(host),
            BackendLaunch::Local { .. } => None,
        }
    }

    /// The child's argument list for `ctx serve <serve_arguments>`.
    pub fn arguments(&self, serve_arguments: &[String]) -> Vec<String> {
        match self {
            BackendLaunch::Local { .. } => serve_arguments.to_vec(),
            BackendLaunch::Ssh(host) => {
                let mut arguments = ssh_arguments(host);
                arguments.push(remote_command(host, serve_arguments));
                arguments
            }
        }
    }

    /// "/path/to/ctx" or "ssh devbox" for messages.
    pub fn label(&self) -> String {
        match self {
            BackendLaunch::Local { binary } => binary.display().to_string(),
            BackendLaunch::Ssh(host) => format!("ssh {}", host.target()),
        }
    }
}

/// Everything before the remote command (`BackendLaunch.sshArguments`): the
/// fixed options, the host's port, key, user and extras, then the address
/// after `--` so an address can never be read as an option.
pub fn ssh_arguments(host: &SshHost) -> Vec<String> {
    let mut arguments: Vec<String> = SSH_OPTIONS.iter().map(|option| (*option).to_owned()).collect();
    if host.port > 0 {
        arguments.push("-p".to_owned());
        arguments.push(host.port.to_string());
    }
    let identity = host.identity_file.trim();
    if !identity.is_empty() {
        arguments.push("-i".to_owned());
        arguments.push(expand_tilde(identity).display().to_string());
    }
    let user = host.user.trim();
    if !user.is_empty() {
        arguments.push("-l".to_owned());
        arguments.push(user.to_owned());
    }
    arguments.extend(host.extra_arguments.iter().map(|argument| argument.trim().to_owned()).filter(|argument| !argument.is_empty()));
    arguments.push("--".to_owned());
    arguments.push(host.address.trim().to_owned());
    arguments
}

/// The arguments of an interactive shell on the host for the Terminal tab
/// (`TerminalWorkspace(directory:host:)`): `ssh -t … -- <address> 'cd
/// <root>; exec $SHELL -l'`. `-T` from the fixed options is dropped and `-t`
/// asks for a tty; `BatchMode` stays, since nobody can type a password.
pub fn ssh_shell_arguments(host: &SshHost, directory: &str) -> Vec<String> {
    let mut arguments: Vec<String> = ssh_arguments(host).into_iter().filter(|argument| argument != "-T").collect();
    // Before `--` and the address.
    let address_index = arguments.len() - 2;
    arguments.insert(address_index, "-t".to_owned());
    arguments.push(format!("cd {}; exec $SHELL -l", shell_quote(directory)));
    arguments
}

/// The command the remote login shell runs (`BackendLaunch.remoteCommand`):
/// `ctx serve …` with the install directories on the PATH. Every piece is
/// quoted for a POSIX shell (`$HOME` / `$PATH` in the PATH prefix are meant
/// to expand there, so that part is not quoted).
pub fn remote_command(host: &SshHost, serve_arguments: &[String]) -> String {
    let binary = host.ctx_path.trim();
    let binary = if binary.is_empty() { "ctx" } else { binary };
    let words: Vec<String> = std::iter::once(binary.to_owned()).chain(serve_arguments.iter().cloned()).map(|word| shell_quote(&word)).collect();
    format!("env PATH=\"{REMOTE_PATH}\" {}", words.join(" "))
}

/// `BackendLaunch.shellQuote`: single-quoted unless the word is plain
/// (letters, digits, `-_./~=:+@%`); a leading `~/` stays outside the quotes
/// so the remote shell expands it.
pub fn shell_quote(word: &str) -> String {
    let plain = !word.is_empty() && word.chars().all(|c| c.is_alphanumeric() || "-_./~=:+@%".contains(c));
    if plain && !word.starts_with('~') {
        return word.to_owned();
    }
    if plain && word.starts_with("~/") {
        return format!("~/{}", shell_quote(&word[2..]));
    }
    format!("'{}'", word.replace('\'', "'\\''"))
}

/// `SSHConfigHosts.aliases`: the `Host` aliases of the user's SSH
/// configuration, offered as suggestions when adding a host (patterns with
/// wildcards are skipped; `Include`d files are read one level deep).
pub fn ssh_config_aliases() -> Vec<String> {
    let Some(home) = dirs::home_dir() else { return Vec::new() };
    let config = home.join(".ssh").join("config");
    let Ok(text) = std::fs::read_to_string(&config) else { return Vec::new() };
    let mut names = Vec::new();
    let mut seen = std::collections::HashSet::new();
    collect_aliases(&text, &mut names, &mut seen, &home.join(".ssh"), 0);
    names
}

fn collect_aliases(text: &str, names: &mut Vec<String>, seen: &mut std::collections::HashSet<String>, base: &std::path::Path, depth: usize) {
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // "Host devbox" or "Host=devbox"; several names on one line.
        let replaced = line.replace('=', " ");
        let parts: Vec<&str> = replaced.split_whitespace().collect();
        let Some(keyword) = parts.first().map(|keyword| keyword.to_ascii_lowercase()) else { continue };
        if keyword == "host" {
            for name in parts.iter().skip(1) {
                if name.contains(['*', '?', '!']) {
                    continue;
                }
                if seen.insert((*name).to_owned()) {
                    names.push((*name).to_owned());
                }
            }
        } else if keyword == "include" && depth == 0 {
            for pattern in parts.iter().skip(1) {
                let expanded = expand_tilde(pattern);
                let file = if expanded.is_absolute() { expanded } else { base.join(expanded) };
                if let Ok(included) = std::fs::read_to_string(&file) {
                    collect_aliases(&included, names, seen, base, depth + 1);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host() -> SshHost {
        SshHost {
            id: "h1".into(),
            name: "Dev box".into(),
            address: "devbox.example".into(),
            user: "alice".into(),
            port: 2222,
            identity_file: "~/.ssh/id_ed25519".into(),
            ctx_path: String::new(),
            extra_arguments: vec!["-o".into(), "ProxyJump=bastion".into()],
        }
    }

    #[test]
    fn shell_quote_leaves_plain_words_and_quotes_the_rest() {
        assert_eq!(shell_quote("ctx"), "ctx");
        assert_eq!(shell_quote("--data-dir"), "--data-dir");
        assert_eq!(shell_quote("/opt/ctx/bin/ctx"), "/opt/ctx/bin/ctx");
        assert_eq!(shell_quote("~/.cargo/bin/ctx"), "~/.cargo/bin/ctx");
        assert_eq!(shell_quote("~alice"), "'~alice'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn remote_command_puts_install_dirs_on_the_path() {
        let command = remote_command(&host(), &["serve".to_owned(), "--mock".to_owned()]);
        assert_eq!(command, format!("env PATH=\"{REMOTE_PATH}\" ctx serve --mock"));
        let mut custom = host();
        custom.ctx_path = "/opt/my tools/ctx".into();
        assert!(remote_command(&custom, &["serve".to_owned()]).ends_with("'/opt/my tools/ctx' serve"));
    }

    #[test]
    fn ssh_arguments_follow_the_swift_order_with_the_address_last() {
        let arguments = ssh_arguments(&host());
        let fixed = SSH_OPTIONS.len();
        assert_eq!(&arguments[..fixed], SSH_OPTIONS);
        assert_eq!(arguments[fixed], "-p");
        assert_eq!(arguments[fixed + 1], "2222");
        assert_eq!(arguments[fixed + 2], "-i");
        assert!(arguments[fixed + 3].ends_with(".ssh/id_ed25519"));
        assert_eq!(arguments[fixed + 4], "-l");
        assert_eq!(arguments[fixed + 5], "alice");
        assert_eq!(arguments[fixed + 6], "-o");
        assert_eq!(arguments[fixed + 7], "ProxyJump=bastion");
        assert_eq!(arguments[arguments.len() - 2], "--");
        assert_eq!(arguments[arguments.len() - 1], "devbox.example");
    }

    #[test]
    fn launch_arguments_end_with_the_remote_command() {
        let launch = BackendLaunch::Ssh(host());
        let arguments = launch.arguments(&["serve".to_owned()]);
        assert_eq!(arguments.last().unwrap(), &format!("env PATH=\"{REMOTE_PATH}\" ctx serve"));
        assert_eq!(arguments[arguments.len() - 2], "devbox.example");
        let local = BackendLaunch::Local { binary: PathBuf::from("/usr/local/bin/ctx") };
        assert_eq!(local.arguments(&["serve".to_owned()]), vec!["serve".to_owned()]);
    }

    #[test]
    fn shell_arguments_ask_for_a_tty_and_change_directory() {
        let arguments = ssh_shell_arguments(&host(), "/home/alice/my proj");
        assert!(!arguments.contains(&"-T".to_owned()));
        assert!(arguments.contains(&"-t".to_owned()));
        assert_eq!(arguments.last().unwrap(), "cd '/home/alice/my proj'; exec $SHELL -l");
        assert_eq!(arguments[arguments.len() - 2], "devbox.example");
        assert_eq!(arguments[arguments.len() - 3], "--");
    }

    #[test]
    fn host_json_matches_the_swift_shape() {
        let json = r#"{"id":"A","name":"","address":"devbox","user":"","port":0,"identityFile":"","ctxPath":"","extraArguments":[]}"#;
        let parsed: SshHost = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.display_name(), "devbox");
        assert_eq!(parsed.target(), "devbox");
        assert!(parsed.is_valid());
        let minimal: SshHost = serde_json::from_str(r#"{"id":"B","address":"-x"}"#).unwrap();
        assert!(!minimal.is_valid());
    }

    #[test]
    fn aliases_skip_patterns_and_follow_includes_one_level() {
        let dir = std::env::temp_dir().join(format!("ctx-ssh-config-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("extra"), "Host extra-one\n  HostName 10.0.0.9\nInclude deeper\n").unwrap();
        std::fs::write(dir.join("deeper"), "Host too-deep\n").unwrap();
        let text = "# comment\nHost devbox staging *.internal\n  User alice\nHost=eq-form\nInclude extra\nHost devbox\n";
        let mut names = Vec::new();
        let mut seen = std::collections::HashSet::new();
        collect_aliases(text, &mut names, &mut seen, &dir, 0);
        assert_eq!(names, vec!["devbox", "staging", "eq-form", "extra-one"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
