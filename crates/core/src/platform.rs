//! Host platform helpers, so the rest of the workspace never has to know
//! whether it runs on Windows, macOS or Linux.
//!
//! - [`home_dir`]: the user's home (`$HOME` on Unix, the profile folder on
//!   Windows, where `HOME` is normally unset).
//! - [`canonicalize`]: `std::fs::canonicalize` without Windows' verbatim
//!   `\\?\C:\…` prefix, which `cmd.exe`, PowerShell, MSVC tools and most
//!   language servers reject as a working directory or path. Identical to
//!   the standard function on Unix, so repository ids do not change there.
//! - [`find_executable`]: `PATH` lookup that honours `PATHEXT` on Windows
//!   (`npx` is `npx.cmd`, `rust-analyzer` is `rust-analyzer.exe`).
//! - [`Shell`] / [`default_shell_program`]: the shell `execute_command`
//!   runs in. Windows prefers Git for Windows' bash, because the command
//!   classifier, the tool descriptions and the model's habits are POSIX;
//!   PowerShell is the fallback.
//! - [`hide_console_window`]: keeps child processes of the desktop app from
//!   flashing console windows on Windows.
//! - [`open_url`], [`host_name`], [`python_interpreter`].

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// `CREATE_NO_WINDOW`: a console child gets no visible console window.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// `CREATE_NEW_PROCESS_GROUP`: the child ignores the parent console's Ctrl+C,
/// like a Unix child in its own process group.
pub const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

/// The user's home directory.
pub fn home_dir() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf()).filter(|p| !p.as_os_str().is_empty())
}

/// `std::fs::canonicalize`, minus the `\\?\` prefix on Windows wherever the
/// path can be written without it (paths longer than `MAX_PATH` or with
/// reserved names keep it).
pub fn canonicalize(path: impl AsRef<Path>) -> std::io::Result<PathBuf> {
    dunce::canonicalize(path)
}

/// Removes a verbatim `\\?\` prefix from an already canonical path (no-op on Unix).
pub fn simplified(path: &Path) -> &Path {
    dunce::simplified(path)
}

/// The extensions an extensionless program name may have on this platform,
/// in lookup order. Empty on Unix.
pub fn executable_extensions() -> Vec<String> {
    if !cfg!(windows) {
        return Vec::new();
    }
    let raw = std::env::var("PATHEXT").ok().filter(|v| !v.trim().is_empty()).unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_string());
    raw.split(';').map(|e| e.trim().to_ascii_lowercase()).filter(|e| e.starts_with('.') && e.len() > 1).collect()
}

/// Finds `name` on this process's `PATH`. See [`find_executable_in`].
pub fn find_executable(name: &str) -> Option<PathBuf> {
    find_executable_in(name, std::env::var_os("PATH").as_deref())
}

/// Finds `name` on `path` (a `PATH`-style list). A name containing a path
/// separator is checked as given. On Windows an extensionless name is tried
/// with every `PATHEXT` extension (`npx` → `npx.cmd`); a name that already
/// has one of those extensions is tried as is. On Unix only files with an
/// execute bit count. `cargo` is also looked for in `~/.cargo/bin`, where
/// rustup installs it without always touching `PATH`.
pub fn find_executable_in(name: &str, path: Option<&OsStr>) -> Option<PathBuf> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let candidate_names = candidate_names(name);
    let as_path = Path::new(name);
    if as_path.components().count() > 1 || as_path.is_absolute() {
        return candidate_names.iter().map(|n| as_path.with_file_name(n)).find(|p| is_executable_file(p));
    }
    let mut dirs: Vec<PathBuf> = path.map(|p| std::env::split_paths(p).collect()).unwrap_or_default();
    if name == "cargo" {
        if let Some(home) = home_dir() {
            dirs.push(home.join(".cargo").join("bin"));
        }
    }
    for dir in dirs {
        if dir.as_os_str().is_empty() {
            continue;
        }
        for candidate in &candidate_names {
            let full = dir.join(candidate);
            if is_executable_file(&full) {
                return Some(full);
            }
        }
    }
    None
}

fn candidate_names(name: &str) -> Vec<String> {
    let extensions = executable_extensions();
    if extensions.is_empty() {
        return vec![name.to_string()];
    }
    let lower = name.to_ascii_lowercase();
    if extensions.iter().any(|e| lower.ends_with(e.as_str())) {
        return vec![name.to_string()];
    }
    extensions.iter().map(|e| format!("{name}{e}")).collect()
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else { return false };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// `path` (a `PATH`-style list) with `extra` directories appended, skipping
/// ones already present and ones that do not exist. Uses the platform's
/// separator (`;` on Windows, `:` elsewhere).
pub fn path_with_extras(path: Option<&OsStr>, extra: &[PathBuf]) -> OsString {
    let mut dirs: Vec<PathBuf> = path.map(|p| std::env::split_paths(p).collect()).unwrap_or_default();
    for dir in extra {
        let present = dirs.iter().any(|d| same_dir(d, dir));
        if !present && dir.is_dir() {
            dirs.push(dir.clone());
        }
    }
    std::env::join_paths(dirs.iter().filter(|d| !d.as_os_str().is_empty())).unwrap_or_else(|_| path.map(OsStr::to_os_string).unwrap_or_default())
}

fn same_dir(a: &Path, b: &Path) -> bool {
    if cfg!(windows) {
        a.to_string_lossy().trim_end_matches(['\\', '/']).eq_ignore_ascii_case(b.to_string_lossy().trim_end_matches(['\\', '/']))
    } else {
        a == b
    }
}

/// Directories where developer tools usually live but which a process
/// started from a GUI may not have on its `PATH` (Homebrew, Cargo, npm, Bun,
/// Deno, Go). Only existing directories matter to [`path_with_extras`].
pub fn tool_directories() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let home = home_dir();
    if cfg!(windows) {
        if let Some(home) = &home {
            dirs.push(home.join(".cargo").join("bin"));
            dirs.push(home.join(".bun").join("bin"));
            dirs.push(home.join(".deno").join("bin"));
            dirs.push(home.join("go").join("bin"));
        }
        if let Some(appdata) = std::env::var_os("APPDATA") {
            dirs.push(PathBuf::from(appdata).join("npm"));
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(PathBuf::from(&local).join("pnpm"));
            dirs.push(PathBuf::from(&local).join("Programs").join("Context"));
        }
        if let Some(pf) = std::env::var_os("ProgramFiles") {
            dirs.push(PathBuf::from(&pf).join("nodejs"));
            dirs.push(PathBuf::from(&pf).join("Git").join("cmd"));
        }
    } else {
        dirs.push(PathBuf::from("/opt/homebrew/bin"));
        dirs.push(PathBuf::from("/usr/local/bin"));
        if let Some(home) = &home {
            for sub in [".cargo/bin", ".local/bin", ".npm-global/bin", ".bun/bin", ".deno/bin", "go/bin"] {
                dirs.push(home.join(sub));
            }
        }
    }
    dirs
}

// ---------------------------------------------------------------------------
// Shell
// ---------------------------------------------------------------------------

/// How a shell takes a command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    /// `sh`, `bash`, `zsh`, Git for Windows' bash…: `<shell> -c <command>`.
    Posix,
    /// `pwsh` / `powershell`: `-NoLogo -NoProfile -NonInteractive -Command <command>`,
    /// with a preamble for UTF-8 output and native exit codes.
    PowerShell,
    /// `cmd.exe`: `/D /S /C "<command>"`, passed verbatim (cmd does not
    /// parse the MSVC quoting std applies to ordinary arguments).
    Cmd,
}

/// A shell program and how to hand it a command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shell {
    pub program: String,
    pub kind: ShellKind,
}

/// The argv that runs one command line in a [`Shell`]. `raw_tail`, when set,
/// must be appended to the Windows command line verbatim (`raw_arg`), after
/// `args`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellInvocation {
    pub args: Vec<String>,
    pub raw_tail: Option<String>,
}

impl Shell {
    /// Classifies `program` by its file name (`pwsh.exe`, `powershell`,
    /// `cmd`, else POSIX).
    pub fn from_program(program: impl Into<String>) -> Self {
        let program = program.into();
        let stem = file_stem_any(&program);
        let kind = match stem.as_str() {
            "pwsh" | "powershell" => ShellKind::PowerShell,
            "cmd" => ShellKind::Cmd,
            _ => ShellKind::Posix,
        };
        Self { program, kind }
    }

    /// The arguments that run `command` (after the program itself).
    pub fn invocation(&self, command: &str) -> ShellInvocation {
        match self.kind {
            ShellKind::Posix => ShellInvocation { args: vec!["-c".into(), command.to_string()], raw_tail: None },
            ShellKind::PowerShell => ShellInvocation {
                args: vec!["-NoLogo".into(), "-NoProfile".into(), "-NonInteractive".into(), "-Command".into(), powershell_script(command)],
                raw_tail: None,
            },
            ShellKind::Cmd => ShellInvocation { args: vec!["/D".into(), "/S".into(), "/C".into()], raw_tail: Some(format!("\"{command}\"")) },
        }
    }

    /// `arg` quoted as one literal word for this shell.
    pub fn quote(&self, arg: &str) -> String {
        match self.kind {
            ShellKind::Posix => format!("'{}'", arg.replace('\'', "'\\''")),
            ShellKind::PowerShell => format!("'{}'", arg.replace('\'', "''")),
            // Windows paths cannot contain `"`; cmd has no escape for it inside quotes.
            ShellKind::Cmd => format!("\"{}\"", arg.replace('"', "")),
        }
    }

    /// A short human name: "bash", "PowerShell", "cmd".
    pub fn display_name(&self) -> String {
        match self.kind {
            ShellKind::PowerShell => "PowerShell".into(),
            ShellKind::Cmd => "cmd.exe".into(),
            ShellKind::Posix => Some(file_stem_any(&self.program)).filter(|s| !s.is_empty()).unwrap_or_else(|| "sh".into()),
        }
    }

    /// One sentence for tool descriptions telling the model which syntax to
    /// write. Empty on Unix, where POSIX `sh` is the unstated default.
    pub fn prompt_note(&self) -> String {
        if !cfg!(windows) {
            return String::new();
        }
        match self.kind {
            ShellKind::Posix => format!(
                " The host is Windows; commands run in {} (Git for Windows), so use POSIX syntax and forward slashes; Windows programs such as cargo, npm and python are on PATH, and /tmp maps to the user's temp folder.",
                self.display_name()
            ),
            ShellKind::PowerShell => " The host is Windows and commands run in PowerShell: use PowerShell syntax (`;` between commands, `$env:NAME` for variables, `Start-Process -NoNewWindow` or `Start-Job` to background a server, `Get-Content -Tail` to read a log). POSIX-only tools such as grep, sed or head may be missing.".into(),
            ShellKind::Cmd => " The host is Windows and commands run in cmd.exe: use cmd syntax (`&&` between commands, `%NAME%` for variables, `start /b` to background a server).".into(),
        }
    }
}

/// The lowercased file name of `program` without its extension, splitting on
/// both `/` and `\\` so a Windows path classifies the same on every host.
fn file_stem_any(program: &str) -> String {
    let name = program.rsplit(['/', '\\']).next().unwrap_or(program).to_ascii_lowercase();
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem.to_string(),
        _ => name,
    }
}

/// The command wrapped so PowerShell writes UTF-8, shows no progress bars
/// (which arrive as CLIXML on a redirected stderr) and exits with the last
/// native program's code, or 1 when the last statement failed.
fn powershell_script(command: &str) -> String {
    format!(
        "$ProgressPreference='SilentlyContinue'; [Console]::OutputEncoding=[System.Text.Encoding]::UTF8; $OutputEncoding=[System.Text.Encoding]::UTF8\n{command}\nif (-not $?) {{ if ($LASTEXITCODE) {{ exit $LASTEXITCODE }} else {{ exit 1 }} }} elseif ($LASTEXITCODE) {{ exit $LASTEXITCODE }}"
    )
}

/// The shell agent commands run in: `CTX_SHELL` when set, else `sh` on
/// Unix; on Windows Git for Windows' bash, another `sh`/`bash` on `PATH`
/// (never WSL's `System32\bash.exe`), then `pwsh`, then Windows PowerShell.
/// Detection runs once per process.
pub fn default_shell_program() -> String {
    if let Ok(shell) = std::env::var("CTX_SHELL") {
        if !shell.trim().is_empty() {
            return resolve_shell_name(shell);
        }
    }
    static DETECTED: OnceLock<String> = OnceLock::new();
    DETECTED.get_or_init(detect_shell).clone()
}

/// `CTX_SHELL` as a program. On Windows a bare `bash` / `sh` becomes the
/// shell detection would pick: `CreateProcess` looks in System32 before
/// `PATH`, so the bare name would start WSL's `bash.exe`. Anything else,
/// and every value on Unix, is used as given.
fn resolve_shell_name(shell: String) -> String {
    #[cfg(windows)]
    {
        let bare = !shell.trim().contains(['/', '\\']);
        if bare && matches!(file_stem_any(shell.trim()).as_str(), "bash" | "sh") {
            if let Some(found) = posix_shell() {
                return found.to_string_lossy().to_string();
            }
        }
    }
    shell
}

/// [`default_shell_program`] as a [`Shell`].
pub fn default_shell() -> Shell {
    Shell::from_program(default_shell_program())
}

#[cfg(not(windows))]
fn detect_shell() -> String {
    "sh".into()
}

#[cfg(windows)]
fn detect_shell() -> String {
    if let Some(posix) = posix_shell() {
        return posix.to_string_lossy().to_string();
    }
    if let Some(pwsh) = find_executable("pwsh") {
        return pwsh.to_string_lossy().to_string();
    }
    let system_root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let powershell = system_root.join("System32").join("WindowsPowerShell").join("v1.0").join("powershell.exe");
    if powershell.is_file() {
        return powershell.to_string_lossy().to_string();
    }
    "powershell.exe".into()
}

/// Git for Windows' bash, else another `sh` / `bash` on `PATH` (MSYS2,
/// Cygwin), never WSL's.
#[cfg(windows)]
fn posix_shell() -> Option<PathBuf> {
    git_bash().or_else(|| ["sh", "bash"].into_iter().find_map(|name| find_executable(name).filter(|p| !is_wsl_launcher(p))))
}

/// Git for Windows' `bin\bash.exe` (the launcher that puts its coreutils on
/// `PATH`), found next to `git.exe` or in the usual install folders.
#[cfg(windows)]
fn git_bash() -> Option<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(git) = find_executable("git") {
        // …\Git\cmd\git.exe, …\Git\bin\git.exe or …\Git\mingw64\bin\git.exe
        let mut dir = git.parent().map(Path::to_path_buf);
        for _ in 0..3 {
            let Some(d) = dir else { break };
            roots.push(d.clone());
            dir = d.parent().map(Path::to_path_buf);
        }
    }
    for var in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
        if let Some(pf) = std::env::var_os(var) {
            roots.push(PathBuf::from(pf).join("Git"));
        }
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        roots.push(PathBuf::from(local).join("Programs").join("Git"));
    }
    roots.into_iter().map(|r| r.join("bin").join("bash.exe")).find(|p| p.is_file() && !is_wsl_launcher(p))
}

/// WSL's `bash.exe` / `wsl.exe` live in System32 or WindowsApps; running
/// commands there would execute them in a Linux VM against `/mnt/c`.
#[cfg(windows)]
fn is_wsl_launcher(path: &Path) -> bool {
    let lower = path.to_string_lossy().to_ascii_lowercase();
    lower.contains("\\windows\\system32\\") || lower.contains("\\windowsapps\\")
}

// ---------------------------------------------------------------------------
// Processes
// ---------------------------------------------------------------------------

/// Marks a command so that, on Windows, it gets no console window (a no-op
/// elsewhere). Use for every helper process (git, language servers, MCP
/// servers, shells): the desktop app runs `ctx` without a console, and each
/// console child would otherwise open its own window.
pub fn hide_console_window(cmd: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Opens `url` in the default browser without waiting for it.
pub fn open_url(url: &str) -> std::io::Result<()> {
    let mut cmd = if cfg!(target_os = "macos") {
        let mut c = std::process::Command::new("open");
        c.arg(url);
        c
    } else if cfg!(windows) {
        // rundll32 takes the URL as one argument; `cmd /C start` would
        // interpret the `&` in query strings.
        let mut c = std::process::Command::new("rundll32");
        c.args(["url.dll,FileProtocolHandler", url]);
        c
    } else {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(url);
        c
    };
    hide_console_window(&mut cmd);
    cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().map(|_| ())
}

/// The computer's user-visible name, when one can be found cheaply.
pub fn host_name() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        if let Ok(out) = std::process::Command::new("scutil").args(["--get", "ComputerName"]).output() {
            let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if out.status.success() && !name.is_empty() {
                return Some(name);
            }
        }
    }
    for var in ["COMPUTERNAME", "HOSTNAME"] {
        if let Ok(name) = std::env::var(var) {
            let name = name.trim().to_string();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    #[cfg(unix)]
    {
        if let Ok(name) = std::fs::read_to_string("/etc/hostname") {
            let name = name.trim().to_string();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

/// What to call this kind of machine in user-facing text.
pub fn machine_noun() -> &'static str {
    if cfg!(target_os = "macos") {
        "Mac"
    } else {
        "computer"
    }
}

/// The Python interpreter to put in generated commands: `python3` on Unix;
/// on Windows the `py` launcher, else `python` (the Store alias `python3`
/// only offers to install Python).
pub fn python_interpreter() -> &'static str {
    static CHOSEN: OnceLock<&'static str> = OnceLock::new();
    CHOSEN.get_or_init(|| {
        if !cfg!(windows) {
            return "python3";
        }
        if find_executable("py").is_some() {
            "py"
        } else if find_executable("python").is_some() {
            "python"
        } else {
            "python3"
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shells_are_classified_by_file_name() {
        assert_eq!(Shell::from_program("sh").kind, ShellKind::Posix);
        assert_eq!(Shell::from_program("/bin/zsh").kind, ShellKind::Posix);
        assert_eq!(Shell::from_program(r"C:\Program Files\Git\bin\bash.exe").kind, ShellKind::Posix);
        assert_eq!(Shell::from_program("pwsh").kind, ShellKind::PowerShell);
        assert_eq!(Shell::from_program(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe").kind, ShellKind::PowerShell);
        assert_eq!(Shell::from_program("cmd.exe").kind, ShellKind::Cmd);
    }

    #[test]
    fn quoting_follows_each_shell() {
        assert_eq!(Shell::from_program("sh").quote("it's a.rs"), "'it'\\''s a.rs'");
        assert_eq!(Shell::from_program("pwsh").quote("it's a.rs"), "'it''s a.rs'");
        assert_eq!(Shell::from_program("cmd").quote("a b.rs"), "\"a b.rs\"");
    }

    #[test]
    fn posix_invocation_is_dash_c() {
        let inv = Shell::from_program("sh").invocation("echo hi && ls");
        assert_eq!(inv.args, vec!["-c".to_string(), "echo hi && ls".to_string()]);
        assert_eq!(inv.raw_tail, None);
    }

    #[test]
    fn cmd_invocation_passes_the_command_verbatim() {
        let inv = Shell::from_program("cmd.exe").invocation(r#"echo "a & b""#);
        assert_eq!(inv.args, vec!["/D", "/S", "/C"]);
        assert_eq!(inv.raw_tail.as_deref(), Some(r#""echo "a & b"""#));
    }

    #[test]
    fn powershell_invocation_keeps_the_command_and_propagates_exit_codes() {
        let inv = Shell::from_program("pwsh").invocation("cargo test");
        assert_eq!(&inv.args[..4], &["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"]);
        let script = &inv.args[4];
        assert!(script.contains("\ncargo test\n"), "{script}");
        assert!(script.contains("exit $LASTEXITCODE"), "{script}");
    }

    #[test]
    fn find_executable_honours_explicit_paths_and_path_lists() {
        let dir = tempfile::tempdir().unwrap();
        let name = if cfg!(windows) { "tool.cmd" } else { "tool" };
        let file = dir.path().join(name);
        std::fs::write(&file, "echo").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let path = std::env::join_paths([dir.path()]).unwrap();
        assert_eq!(find_executable_in("tool", Some(&path)), Some(file.clone()));
        assert_eq!(find_executable_in("missing-tool", Some(&path)), None);
        assert_eq!(find_executable_in(&file.to_string_lossy(), None), Some(file));
    }

    #[cfg(unix)]
    #[test]
    fn find_executable_skips_files_without_an_execute_bit() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("plain"), "data").unwrap();
        let path = std::env::join_paths([dir.path()]).unwrap();
        assert_eq!(find_executable_in("plain", Some(&path)), None);
    }

    #[cfg(windows)]
    #[test]
    fn ctx_shell_bash_is_never_wsl() {
        for name in ["bash", "sh", "bash.exe"] {
            let resolved = resolve_shell_name(name.into());
            assert!(!is_wsl_launcher(Path::new(&resolved)), "{name} → {resolved}");
            if git_bash().is_some() {
                assert!(Path::new(&resolved).is_absolute(), "{name} → {resolved}");
            }
        }
        assert_eq!(resolve_shell_name("pwsh".into()), "pwsh");
        assert_eq!(resolve_shell_name(r"C:\tools\bash.exe".into()), r"C:\tools\bash.exe");
    }

    #[test]
    fn path_with_extras_uses_the_platform_separator_and_dedupes() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let base = std::env::join_paths([a.path()]).unwrap();
        let joined = path_with_extras(Some(&base), &[a.path().to_path_buf(), b.path().to_path_buf(), PathBuf::from("/definitely/not/here")]);
        let parts: Vec<PathBuf> = std::env::split_paths(&joined).collect();
        assert_eq!(parts, vec![a.path().to_path_buf(), b.path().to_path_buf()]);
    }

    #[test]
    fn canonicalize_matches_std_on_unix_and_drops_verbatim_prefixes_on_windows() {
        let dir = tempfile::tempdir().unwrap();
        let ours = canonicalize(dir.path()).unwrap();
        assert!(!ours.to_string_lossy().starts_with(r"\\?\"), "{}", ours.display());
        #[cfg(unix)]
        assert_eq!(ours, std::fs::canonicalize(dir.path()).unwrap());
    }
}
