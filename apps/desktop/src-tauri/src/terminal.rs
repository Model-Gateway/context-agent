//! The Inspector Terminal tab's shells (`TerminalSession.swift`): one
//! pseudo-terminal per tab (portable-pty: a Unix pty, ConPTY on Windows)
//! running the user's shell in the project root. Output goes to xterm.js as
//! `terminal:data` events (UTF-8 text, decoded across chunk boundaries) and
//! the shell's end as `terminal:exit`; the frontend writes keystrokes and
//! resizes with `terminal_write` / `terminal_resize`. `terminal_close` hangs
//! the session up and kills the shell, like closing a SwiftTerm tab.
//!
//! The shell is the login shell on macOS and Linux (`$SHELL`, else the
//! password database, started as `-zsh` the way `LocalProcessTerminalView`
//! is started with `execName: "-zsh"`), and PowerShell on Windows (`pwsh.exe`
//! when it is on `PATH`, else Windows PowerShell). The environment is the
//! app's with `TERM=xterm-256color`, `COLORTERM=truecolor` and
//! `TERM_PROGRAM=ContextAgent`, as in `TerminalSession.init`. A project on
//! a remote host gets `ssh -t <host> 'cd <root>; exec $SHELL -l'` instead
//! (`TerminalWorkspace(directory:host:)`), on the same pty.

use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::backend::launch::{ssh_shell_arguments, BackendLaunch, SshHost};
use crate::backend::{debug, lock};

/// Emitted for output: `{ id, data }` with `data` a UTF-8 string.
pub const DATA_EVENT: &str = "terminal:data";
/// Emitted once when the shell has exited: `{ id, code }` (`code` is null
/// when the exit status could not be read).
pub const EXIT_EVENT: &str = "terminal:exit";

/// The size a session starts at when the view has not measured itself yet.
const DEFAULT_COLS: u16 = 80;
const DEFAULT_ROWS: u16 = 24;
const READ_CHUNK: usize = 64 * 1024;
/// A read at least this long means the program is printing in bulk: wait a
/// moment and send what follows in the same event instead of one event per
/// pty buffer (macOS hands out ~1 KiB at a time).
const COALESCE_THRESHOLD: usize = 1024;
const COALESCE_WINDOW: Duration = Duration::from_millis(6);
const MAX_BATCH: usize = 512 * 1024;
/// How long the exit watcher waits for the last output after the shell left.
const DRAIN_TIMEOUT: Duration = Duration::from_millis(300);
/// How long it waits again after hanging up (ConPTY only closes its output
/// pipe when the pseudo console goes).
const HANGUP_DRAIN_TIMEOUT: Duration = Duration::from_secs(1);

/// The open sessions, keyed by the id `terminal_open` returned (Tauri
/// managed state; `TerminalWorkspace.sessions` holds the frontend side).
#[derive(Default)]
pub struct Terminals {
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    next_id: AtomicU64,
}

impl Terminals {
    fn get(&self, id: &str) -> Option<Arc<Session>> {
        lock(&self.sessions).get(id).cloned()
    }

    fn insert(&self, id: String, session: Arc<Session>) {
        lock(&self.sessions).insert(id, session);
    }

    fn remove(&self, id: &str) -> Option<Arc<Session>> {
        lock(&self.sessions).remove(id)
    }

    /// Hangs up every session (the app is going away).
    pub fn close_all(&self) {
        let sessions: Vec<Arc<Session>> = lock(&self.sessions).drain().map(|(_, session)| session).collect();
        for session in sessions {
            session.terminate();
        }
    }
}

/// One pty and its shell. The reader, the output pump and the exit watcher
/// run on their own threads; this holds what the commands need.
struct Session {
    writer: Mutex<Option<Box<dyn Write + Send>>>,
    master: Mutex<Option<Box<dyn MasterPty + Send>>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
}

impl Session {
    /// Closes our end of the pty: the shell gets SIGHUP on Unix, and ConPTY
    /// ends the console's processes on Windows.
    fn hang_up(&self) {
        lock(&self.writer).take();
        lock(&self.master).take();
    }

    /// `TerminalSession.terminate`: signal the shell and hang up.
    fn terminate(&self) {
        if let Err(error) = lock(&self.killer).kill() {
            debug(|| format!("terminal: could not signal the shell: {error}"));
        }
        self.hang_up();
    }
}

/// `terminal_open`'s result.
#[derive(Debug, Clone, Serialize)]
pub struct TerminalOpened {
    pub id: String,
    /// The shell's name for the tab's fallback title ("zsh", "pwsh").
    pub shell: String,
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
struct DataPayload {
    id: String,
    data: String,
}

#[derive(Debug, Clone, Serialize)]
struct ExitPayload {
    id: String,
    code: Option<i64>,
}

// MARK: - Shell

/// The login shell and its name (`TerminalSession.shellPath`).
#[cfg(not(windows))]
fn shell_command() -> (CommandBuilder, String) {
    // `new_default_prog` runs `$SHELL` (else the password database's shell)
    // with argv[0] = "-<name>", i.e. as a login shell.
    let command = CommandBuilder::new_default_prog();
    let shell = command.get_shell();
    let name = Path::new(&shell)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "sh".to_string());
    (command, name)
}

/// PowerShell 7 when it is on `PATH`, else Windows PowerShell.
#[cfg(windows)]
fn shell_command() -> (CommandBuilder, String) {
    let program = find_on_path("pwsh.exe")
        .or_else(|| find_on_path("powershell.exe"))
        .unwrap_or_else(|| PathBuf::from("powershell.exe"));
    let name = program
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "powershell".to_string());
    let mut command = CommandBuilder::new(&program);
    command.arg("-NoLogo");
    (command, name)
}

/// The first `name` in the directories of `PATH`.
#[cfg_attr(not(windows), allow(dead_code))]
fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
}

/// The folder the shell starts in: `cwd`, or the home folder when none is given.
fn start_directory(cwd: &str) -> Result<PathBuf, String> {
    let trimmed = cwd.trim();
    if trimmed.is_empty() {
        return dirs::home_dir().ok_or_else(|| "There is no home folder to start the shell in.".to_string());
    }
    let path = PathBuf::from(trimmed);
    if path.is_dir() {
        Ok(path)
    } else {
        Err(format!("{trimmed} is not a folder."))
    }
}

fn pty_size(cols: Option<u16>, rows: Option<u16>) -> PtySize {
    PtySize {
        rows: rows.unwrap_or(DEFAULT_ROWS).clamp(1, 1000),
        cols: cols.unwrap_or(DEFAULT_COLS).clamp(1, 1000),
        pixel_width: 0,
        pixel_height: 0,
    }
}

// MARK: - Output

/// Decodes a byte stream as UTF-8 across chunk boundaries: an incomplete
/// sequence at the end of a chunk waits for the next one, invalid bytes
/// become U+FFFD.
#[derive(Default)]
struct Utf8Stream {
    pending: Vec<u8>,
}

impl Utf8Stream {
    fn decode(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let mut out = String::with_capacity(self.pending.len());
        let mut start = 0;
        let mut keep_from = self.pending.len();
        while start < self.pending.len() {
            match std::str::from_utf8(&self.pending[start..]) {
                Ok(text) => {
                    out.push_str(text);
                    break;
                }
                Err(error) => {
                    let valid = start + error.valid_up_to();
                    out.push_str(std::str::from_utf8(&self.pending[start..valid]).unwrap_or_default());
                    match error.error_len() {
                        Some(length) => {
                            out.push('\u{FFFD}');
                            start = valid + length;
                        }
                        None => {
                            keep_from = valid;
                            break;
                        }
                    }
                }
            }
        }
        if keep_from < self.pending.len() {
            self.pending.drain(..keep_from);
        } else {
            self.pending.clear();
        }
        out
    }

    /// What is left at the end of the stream.
    fn finish(&mut self) -> String {
        let rest = String::from_utf8_lossy(&self.pending).into_owned();
        self.pending.clear();
        rest
    }
}

/// Reads the pty until it closes and hands the chunks to the pump.
fn spawn_reader(id: &str, mut reader: Box<dyn Read + Send>, chunks: mpsc::Sender<Vec<u8>>) -> std::io::Result<()> {
    thread::Builder::new().name(format!("terminal-reader-{id}")).spawn(move || {
        let mut buffer = vec![0u8; READ_CHUNK];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    if chunks.send(buffer[..count].to_vec()).is_err() {
                        break;
                    }
                }
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    })?;
    Ok(())
}

/// Turns chunks into `terminal:data` events (coalescing bulk output), then
/// says it is done once the reader has stopped.
fn spawn_pump(app: AppHandle, id: String, chunks: mpsc::Receiver<Vec<u8>>, done: mpsc::Sender<()>) -> std::io::Result<()> {
    thread::Builder::new().name(format!("terminal-output-{id}")).spawn(move || {
        let emit = |data: String| {
            if data.is_empty() {
                return;
            }
            if let Err(error) = app.emit(DATA_EVENT, DataPayload { id: id.clone(), data }) {
                debug(|| format!("terminal: could not emit output: {error}"));
            }
        };
        let mut decoder = Utf8Stream::default();
        while let Ok(first) = chunks.recv() {
            let mut batch = first;
            if batch.len() >= COALESCE_THRESHOLD {
                let deadline = Instant::now() + COALESCE_WINDOW;
                while batch.len() < MAX_BATCH {
                    let now = Instant::now();
                    if now >= deadline {
                        break;
                    }
                    match chunks.recv_timeout(deadline - now) {
                        Ok(more) => batch.extend_from_slice(&more),
                        Err(_) => break,
                    }
                }
            } else {
                while batch.len() < MAX_BATCH {
                    match chunks.try_recv() {
                        Ok(more) => batch.extend_from_slice(&more),
                        Err(_) => break,
                    }
                }
            }
            emit(decoder.decode(&batch));
        }
        emit(decoder.finish());
        let _ = done.send(());
    })?;
    Ok(())
}

// MARK: - Commands

/// `terminal_open({ cwd, cols?, rows?, host? }) -> { id, shell, pid }`: starts
/// the shell in `cwd` on a new pty (`TerminalSession.init`); with `host`, an
/// `ssh -t` shell in that folder on the host (`cwd` is then a remote path).
#[tauri::command]
pub async fn terminal_open(
    app: AppHandle,
    terminals: State<'_, Terminals>,
    cwd: String,
    cols: Option<u16>,
    rows: Option<u16>,
    host: Option<SshHost>,
) -> Result<TerminalOpened, String> {
    let number = terminals.next_id.fetch_add(1, Ordering::Relaxed) + 1;
    let id = format!("term-{number}");
    let program = match host {
        Some(host) => ShellProgram::Ssh { host, directory: cwd.trim().to_owned() },
        None => ShellProgram::Local(start_directory(&cwd)?),
    };
    let size = pty_size(cols, rows);
    let app_for_open = app.clone();
    let id_for_open = id.clone();
    let (session, opened) = tauri::async_runtime::spawn_blocking(move || open_session(app_for_open, id_for_open, program, size))
        .await
        .map_err(|error| format!("Could not start the shell: {error}"))??;
    terminals.insert(id, session);
    Ok(opened)
}

/// What a tab runs: the user's shell in a local folder, or ssh to a host.
enum ShellProgram {
    Local(PathBuf),
    Ssh { host: SshHost, directory: String },
}

/// `ssh -t … -- <address> 'cd <root>; exec $SHELL -l'` on the pty: the
/// user's shell on the host, with a tty so prompts and programs behave as
/// in Terminal. The login shell's environment is kept (SSH_AUTH_SOCK).
fn ssh_command(host: &SshHost, directory: &str) -> (CommandBuilder, String) {
    let mut command = CommandBuilder::new(BackendLaunch::ssh_binary());
    for argument in ssh_shell_arguments(host, directory) {
        command.arg(argument);
    }
    (command, "ssh".to_owned())
}

fn open_session(app: AppHandle, id: String, program: ShellProgram, size: PtySize) -> Result<(Arc<Session>, TerminalOpened), String> {
    let (mut command, shell) = match &program {
        ShellProgram::Local(_) => shell_command(),
        ShellProgram::Ssh { host, directory } => ssh_command(host, directory),
    };
    let directory: PathBuf = match &program {
        ShellProgram::Local(directory) => directory.clone(),
        // ssh itself runs at home; the `cd` happens on the host.
        ShellProgram::Ssh { .. } => dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")),
    };
    let directory = directory.as_path();
    command.cwd(directory);
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    command.env("TERM_PROGRAM", "ContextAgent");
    #[cfg(not(windows))]
    {
        if command.get_env("LANG").is_none() {
            command.env("LANG", "en_US.UTF-8");
        }
        command.env("PWD", directory);
    }

    let pair = native_pty_system()
        .openpty(size)
        .map_err(|error| format!("Could not open a terminal: {error}"))?;
    let mut child = pair
        .slave
        .spawn_command(command)
        .map_err(|error| format!("Could not start {shell}: {error}"))?;
    // Only the shell keeps the slave side open, so the reader sees the end
    // of the stream when it exits.
    drop(pair.slave);
    let pid = child.process_id();
    let killer = child.clone_killer();
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|error| format!("Could not read the terminal: {error}"))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|error| format!("Could not write to the terminal: {error}"))?;

    let session = Arc::new(Session {
        writer: Mutex::new(Some(writer)),
        master: Mutex::new(Some(pair.master)),
        killer: Mutex::new(killer),
    });

    let (chunk_tx, chunk_rx) = mpsc::channel::<Vec<u8>>();
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let started = spawn_pump(app.clone(), id.clone(), chunk_rx, done_tx).and_then(|()| spawn_reader(&id, reader, chunk_tx));
    if let Err(error) = started {
        session.terminate();
        return Err(format!("Could not start the terminal: {error}"));
    }

    let watcher_session = session.clone();
    let watcher_id = id.clone();
    let watcher = thread::Builder::new().name(format!("terminal-exit-{id}")).spawn(move || {
        let code = match child.wait() {
            Ok(status) => Some(i64::from(status.exit_code())),
            Err(error) => {
                debug(|| format!("terminal: could not wait for the shell: {error}"));
                None
            }
        };
        // Let the last output through before the tab goes.
        if done_rx.recv_timeout(DRAIN_TIMEOUT).is_err() {
            watcher_session.hang_up();
            let _ = done_rx.recv_timeout(HANGUP_DRAIN_TIMEOUT);
        }
        watcher_session.hang_up();
        app.state::<Terminals>().remove(&watcher_id);
        if let Err(error) = app.emit(EXIT_EVENT, ExitPayload { id: watcher_id, code }) {
            debug(|| format!("terminal: could not emit the exit: {error}"));
        }
    });
    if let Err(error) = watcher {
        session.terminate();
        return Err(format!("Could not start the terminal: {error}"));
    }

    Ok((session, TerminalOpened { id, shell, pid }))
}

/// `terminal_write({ id, data })`: keystrokes and pastes from xterm.js.
#[tauri::command]
pub async fn terminal_write(terminals: State<'_, Terminals>, id: String, data: String) -> Result<(), String> {
    let session = terminals.get(&id).ok_or_else(|| "The terminal has closed.".to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut writer = lock(&session.writer);
        let writer = writer.as_mut().ok_or_else(|| "The terminal has closed.".to_string())?;
        writer
            .write_all(data.as_bytes())
            .and_then(|()| writer.flush())
            .map_err(|error| format!("Could not write to the terminal: {error}"))
    })
    .await
    .map_err(|error| format!("Could not write to the terminal: {error}"))?
}

/// `terminal_resize({ id, cols, rows })`: the view's size in cells.
#[tauri::command]
pub async fn terminal_resize(terminals: State<'_, Terminals>, id: String, cols: u16, rows: u16) -> Result<(), String> {
    let Some(session) = terminals.get(&id) else {
        return Ok(());
    };
    let master = lock(&session.master);
    match master.as_ref() {
        Some(master) => master
            .resize(pty_size(Some(cols), Some(rows)))
            .map_err(|error| format!("Could not resize the terminal: {error}")),
        None => Ok(()),
    }
}

/// `terminal_close({ id })`: hangs the session up and kills the shell
/// (`TerminalWorkspace.close`). `terminal:exit` follows once it has gone.
#[tauri::command]
pub async fn terminal_close(terminals: State<'_, Terminals>, id: String) -> Result<(), String> {
    let Some(session) = terminals.get(&id) else {
        return Ok(());
    };
    tauri::async_runtime::spawn_blocking(move || session.terminate())
        .await
        .map_err(|error| format!("Could not close the terminal: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_split_sequences() {
        let mut stream = Utf8Stream::default();
        let bytes = "héllo ✳".as_bytes();
        let (first, second) = bytes.split_at(2); // inside "é"
        assert_eq!(stream.decode(first), "h");
        assert_eq!(stream.decode(second), "éllo ✳");
        assert_eq!(stream.finish(), "");
    }

    #[test]
    fn replaces_invalid_bytes() {
        let mut stream = Utf8Stream::default();
        assert_eq!(stream.decode(&[b'a', 0xff, b'b']), "a\u{FFFD}b");
        assert_eq!(stream.decode(&[0xe2, 0x9c]), "");
        assert_eq!(stream.finish(), "\u{FFFD}");
    }

    #[test]
    fn clamps_sizes() {
        let size = pty_size(Some(0), None);
        assert_eq!((size.cols, size.rows), (1, DEFAULT_ROWS));
    }
}
