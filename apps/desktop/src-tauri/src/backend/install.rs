//! "Install ctx on host": the port of `RemoteInstaller.swift`
//! (docs/remote-hosts.md). Puts the static Linux build of this app's ctx
//! version on a host that has none, or an older one, without Rust, a
//! checkout or root there. The archive comes from the GitHub release of the
//! app's version (.github/workflows/release-linux.yml) and is downloaded
//! here with `curl` (it ships with macOS and Windows 10+), so the host needs
//! no internet access; it is unpacked over the host's own ssh settings into
//! `~/.local/bin`, the first directory of [`launch::REMOTE_PATH`].

use std::process::Stdio;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use super::configure_platform;
use super::environment;
use super::launch::{self, BackendLaunch, SshHost};

/// `<base>/v<version>/ctx-<target>.tar.gz`.
pub const RELEASE_BASE: &str = "https://github.com/Model-Gateway/context-agent/releases/download";
/// Where the binary goes on the host (for messages; the script spells it out).
pub const INSTALL_DIRECTORY: &str = "~/.local/bin";

/// Install progress for the host editor's step list, as
/// `ssh:install-progress` events: `phase` is "probe", "download", "upload"
/// or "verify"; `done`/`total` count bytes of the archive (`total` unknown
/// while the download's size is).
pub const PROGRESS_EVENT: &str = "ssh:install-progress";

#[derive(Clone, Serialize)]
pub struct InstallProgress {
    pub host: String,
    pub phase: &'static str,
    pub done: u64,
    pub total: Option<u64>,
}

/// `RemoteInstaller.installScript`: unpacks the archive on stdin next to the
/// old binary, checks that the new one runs there (a `noexec` home fails
/// here, not at the next connect), then renames it over `~/.local/bin/ctx`.
/// Prints the version as its last line. Plain POSIX sh, run with `sh -c`
/// whatever the login shell is.
pub const INSTALL_SCRIPT: &str = r#"set -e
dir="$HOME/.local/bin"
mkdir -p "$dir"
tmp=$(mktemp -d "$dir/.ctx-install.XXXXXX")
trap 'rm -rf "$tmp"' EXIT
tar -xzf - -C "$tmp"
chmod 755 "$tmp/ctx"
"$tmp/ctx" --version
mv -f "$tmp/ctx" "$dir/ctx""#;

/// `uname -sm` → the release target; `None` for a system without a build.
pub fn target(uname: &str) -> Option<&'static str> {
    let words: Vec<String> = uname.split_whitespace().map(str::to_ascii_lowercase).collect();
    if words.len() < 2 || words[0] != "linux" {
        return None;
    }
    match words[1].as_str() {
        "x86_64" | "amd64" => Some("x86_64-unknown-linux-musl"),
        "aarch64" | "arm64" => Some("aarch64-unknown-linux-musl"),
        _ => None,
    }
}

/// The archive for `target`: from the app version's release, or from the
/// directory `$CTX_REMOTE_RELEASE_URL` names (a `file://` URL in tests).
pub fn archive_url(version: &str, target: &str) -> String {
    let name = format!("ctx-{target}.tar.gz");
    match std::env::var("CTX_REMOTE_RELEASE_URL").ok().map(|value| value.trim().to_owned()).filter(|value| !value.is_empty()) {
        Some(base) => format!("{}/{name}", base.trim_end_matches('/')),
        None => format!("{RELEASE_BASE}/v{version}/{name}"),
    }
}

struct Output {
    code: i32,
    stdout: String,
    stderr: String,
}

fn failure_text(output: &Output, doing: &str) -> String {
    let lines: Vec<&str> = output.stderr.lines().collect();
    let tail = lines[lines.len().saturating_sub(6)..].join("\n");
    let head = format!("Could not {doing} (ssh exited with code {}).", output.code);
    if tail.is_empty() {
        head
    } else {
        format!("{head}\n{tail}")
    }
}

/// `ssh <host's options> -- <address> <command>` with `input` on stdin, the
/// same way the app connects ([`launch::ssh_arguments`]).
async fn ssh(host: &SshHost, command: &str, input: Option<&[u8]>) -> Result<Output, String> {
    ssh_with_progress(host, command, input, |_| {}).await
}

/// [`ssh`], calling `written` with the bytes of `input` sent so far.
async fn ssh_with_progress(host: &SshHost, command: &str, input: Option<&[u8]>, written: impl Fn(u64) + Send + 'static) -> Result<Output, String> {
    let binary = BackendLaunch::ssh_binary();
    let environment = environment::build(None).await;
    let mut arguments = launch::ssh_arguments(host);
    arguments.push(command.to_owned());
    let mut child = Command::new(&binary);
    child
        .args(&arguments)
        .env_clear()
        .envs(&environment)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    configure_platform(&mut child);
    let mut child = child.spawn().map_err(|error| format!("Could not run {}: {error}", binary.display()))?;
    // Written while `wait_with_output` drains both streams, so neither side
    // can stall on a full pipe; a host that hangs up early only fails the write.
    let mut stdin = child.stdin.take();
    let data = input.map(<[u8]>::to_vec);
    let writer = tokio::spawn(async move {
        if let (Some(stdin), Some(data)) = (stdin.as_mut(), data) {
            let mut sent = 0u64;
            for chunk in data.chunks(256 * 1024) {
                if stdin.write_all(chunk).await.is_err() {
                    break;
                }
                sent += chunk.len() as u64;
                written(sent);
            }
            let _ = stdin.shutdown().await;
        }
        drop(stdin);
    });
    let output = child.wait_with_output().await.map_err(|error| format!("ssh failed: {error}"))?;
    let _ = writer.await;
    Ok(Output {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// The archive's size from a `HEAD` (after GitHub's redirects), if curl can tell.
async fn content_length(url: &str) -> Option<u64> {
    let mut command = Command::new(if cfg!(windows) { "curl.exe" } else { "curl" });
    command.args(["-sIL", "--max-time", "15", url]).stdin(Stdio::null()).kill_on_drop(true);
    configure_platform(&mut command);
    let output = command.output().await.ok()?;
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split_once(':').filter(|(name, _)| name.trim().eq_ignore_ascii_case("content-length")).and_then(|(_, value)| value.trim().parse::<u64>().ok()))
        .last()
        .filter(|length| *length > 0)
}

/// The archive's bytes, through `curl` (which also reads `file://`);
/// `progress(done, total)` follows the file as it grows.
async fn download(url: &str, version: &str, progress: impl Fn(u64, Option<u64>)) -> Result<Vec<u8>, String> {
    let total = content_length(url).await;
    progress(0, total);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|elapsed| elapsed.as_nanos()).unwrap_or_default();
    let path = std::env::temp_dir().join(format!("ctx-download-{}-{nanos}.tar.gz", std::process::id()));
    let mut command = Command::new(if cfg!(windows) { "curl.exe" } else { "curl" });
    command
        .args(["-sSL", "--retry", "2", "-o"])
        .arg(&path)
        .args(["-w", "%{http_code}", url])
        .stdin(Stdio::null())
        .kill_on_drop(true);
    configure_platform(&mut command);
    let running = command.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();
    let output = match running {
        Ok(child) => {
            let waiting = child.wait_with_output();
            tokio::pin!(waiting);
            loop {
                tokio::select! {
                    output = &mut waiting => break output.map_err(|error| format!("Could not run curl to download {url}: {error}")),
                    _ = tokio::time::sleep(std::time::Duration::from_millis(200)) => {
                        if let Ok(meta) = std::fs::metadata(&path) {
                            progress(meta.len(), total);
                        }
                    }
                }
            }
        }
        Err(error) => Err(format!("Could not run curl to download {url}: {error}")),
    };
    let result = output.and_then(|output| {
        let status = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if !output.status.success() {
            let reason = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            return Err(format!("Could not download {url}: {reason}"));
        }
        match status.as_str() {
            "200" | "000" => std::fs::read(&path).map_err(|error| format!("Could not read the download of {url}: {error}")),
            "404" => Err(format!(
                "The v{version} release has no Linux build of ctx ({}). Update Context, or install ctx on the host yourself.",
                url.rsplit('/').next().unwrap_or(url)
            )),
            other => Err(format!("Downloading {url} failed (HTTP {other}).")),
        }
    });
    let _ = std::fs::remove_file(&path);
    result
}

/// `RemoteInstaller.install`: probes the host, downloads the matching
/// archive and installs it; returns what the new binary printed for
/// `--version` ("ctx 0.1.6").
pub async fn install(host: &SshHost, version: &str, report: impl Fn(InstallProgress) + Clone + Send + Sync + 'static) -> Result<String, String> {
    let id = host.id.clone();
    let step = move |phase: &'static str, done: u64, total: Option<u64>| report(InstallProgress { host: id.clone(), phase, done, total });
    step("probe", 0, None);
    let probe = ssh(host, "uname -sm", None).await?;
    if probe.code != 0 {
        return Err(failure_text(&probe, "ask the host what it runs"));
    }
    let platform = probe.stdout.trim();
    let Some(target) = target(platform) else {
        return Err(format!(
            "There is no ctx build for {}. Automatic install covers Linux on x86_64 and arm64; install ctx there yourself (cargo install --locked --git https://github.com/Model-Gateway/context-agent ctx-cli) or set the host's ctx path.",
            if platform.is_empty() { "this host" } else { platform }
        ));
    };
    let url = archive_url(version, target);
    let archive = download(&url, version, |done, total| step("download", done, total)).await?;
    let size = archive.len() as u64;
    step("upload", 0, Some(size));
    let command = format!("sh -c {}", launch::shell_quote(INSTALL_SCRIPT));
    let uploading = step.clone();
    let result = ssh_with_progress(host, &command, Some(&archive), move |sent| {
        uploading(if sent < size { "upload" } else { "verify" }, sent, Some(size))
    })
    .await?;
    if result.code != 0 {
        return Err(failure_text(&result, &format!("install ctx in {INSTALL_DIRECTORY}")));
    }
    let printed = result.stdout.lines().rev().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default().to_owned();
    Ok(if printed.is_empty() { format!("ctx {version}") } else { printed })
}

/// `ssh_install_ctx({ host })` → the installed version line, or why not.
/// The archive is the one of the running app's version.
#[tauri::command]
pub async fn ssh_install_ctx(app: AppHandle, host: SshHost) -> Result<String, String> {
    if !host.is_valid() {
        return Err("Enter the host's address first.".to_owned());
    }
    let version = app.package_info().version.to_string();
    install(&host, &version, move |progress| {
        let _ = app.emit(PROGRESS_EVENT, progress);
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_from_uname() {
        assert_eq!(target("Linux x86_64\n"), Some("x86_64-unknown-linux-musl"));
        assert_eq!(target("Linux aarch64"), Some("aarch64-unknown-linux-musl"));
        assert_eq!(target("Linux armv7l"), None);
        assert_eq!(target("Darwin arm64"), None);
        assert_eq!(target(""), None);
    }

    #[test]
    fn script_survives_quoting() {
        let quoted = launch::shell_quote(INSTALL_SCRIPT);
        assert!(quoted.starts_with('\'') && quoted.ends_with('\''));
        assert!(quoted.contains(r#"trap '\''rm -rf "$tmp"'\'' EXIT"#));
    }
}
