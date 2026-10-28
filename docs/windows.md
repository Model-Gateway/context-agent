# Context on Windows

`ctx` and the desktop app run natively on Windows 10 (1809 or later) and
Windows 11, on x86-64 and ARM64. WSL is not needed. If you already work
inside WSL, the Linux build of `ctx` runs there unchanged, but it then sees
your projects through `/mnt/c` and runs Linux tools against them.

## Install

### Desktop app

Download `Context_<version>_x64-setup.exe` (per-user installer) or the
`.msi` from [Releases](../../../releases). Both bundle `ctx.exe`, so nothing
else is needed. The installer adds the WebView2 runtime when it is missing
(it ships with Windows 11 and current Windows 10).

### CLI from source

You need the Rust toolchain for MSVC (`rustup default stable-msvc`) and the
"Desktop development with C++" workload of the Visual Studio Build Tools,
which provides the linker and the C compiler the tree-sitter grammars and
SQLite need.

```powershell
git clone <this repository> context; cd context
cargo build --release              # builds target\release\ctx.exe
cargo install --path crates/cli    # optional: puts ctx.exe in %USERPROFILE%\.cargo\bin
```

### Desktop app from source

Additionally Node.js 22 and npm.

```powershell
cd apps\desktop
npm install
powershell -ExecutionPolicy Bypass -File scripts\embed-ctx.ps1   # builds ctx and copies it next to the app
npm run tauri build -- --config src-tauri/tauri.bundle.conf.json  # NSIS installer + MSI in src-tauri\target\release\bundle
npm run tauri dev                                                 # or: run it from source
```

See [apps/desktop/ARCHITECTURE.md](../apps/desktop/ARCHITECTURE.md) for how
the app is built.

## Where things live

| What | Where |
|---|---|
| Data directory (sessions, tasks, index, memories, `config.toml`, `credentials.toml`) | `%USERPROFILE%\.context-agent`, or `CTX_DATA_DIR` |
| The desktop app and the `ctx.exe` it ships | `%LOCALAPPDATA%\Context` (setup `.exe`, per user) or `%ProgramFiles%\Context` (`.msi`) |
| The desktop app's API keys | Windows Credential Manager, service `dev.contextagent.desktop` |
| The desktop app's pasted images | `%APPDATA%\dev.contextagent.desktop.tauri\Pasted Images` |

The terminal UI, `ctx run` and the desktop app share the data directory, as
on macOS.

## The shell the agent's commands run in

`execute_command`, `run_validation` and the git tools run one command line
in a shell. On Windows `ctx` picks, in order:

1. `CTX_SHELL`, when set (`bash`, `pwsh`, `powershell`, `cmd`, or a full path).
2. Git for Windows' `bin\bash.exe`, found next to `git.exe` or under
   `Program Files\Git`. **Recommended**: the command classifier that decides
   what needs approval, the tool descriptions and the models' own habits are
   POSIX, and Git Bash puts the usual tools (`grep`, `sed`, `head`, `find`)
   on `PATH` while Windows programs (`cargo`, `npm`, `python`, `dotnet`)
   keep working.
3. Another `sh.exe` / `bash.exe` on `PATH` (MSYS2, Cygwin). WSL's
   `System32\bash.exe` is never used.
4. PowerShell 7 (`pwsh.exe`), then Windows PowerShell. The agent is told it
   is in PowerShell; commands it does not recognise ask for approval under
   the Ask profile.

The model is told which shell it has. Commands run without a console
window, in their own process group, inside a Job Object, so a timeout or
cancel stops everything a command started.

With a POSIX shell, `execute_command` also takes `shell: "powershell"`
(PowerShell 7 when installed, else Windows PowerShell) for Windows
administration such as disk space, services or the registry. The script
runs in PowerShell directly rather than inside `powershell -Command '…'`
from bash, whose quoting rewrites `$` and `'` before PowerShell sees them.
The classifier counts PowerShell's `Remove-Item` and cmd's `del` / `rd`
(and their aliases) as destructive in either shell.

## Terminal UI

Use Windows Terminal (the default on Windows 11) or the VS Code terminal for
the full-screen session: they render the spinner and status glyphs, colours
and mouse scrolling. In the legacy console host (`conhost`) `ctx` switches
to glyphs every console font has; set `CTX_ASCII=0` to force the Unicode set
or `CTX_ASCII=1` to force the fallback anywhere. AltGr characters (`@ { } [
] \ | ~`) and Ctrl shortcuts work regardless of Caps Lock.

## Remote hosts

Projects on another machine work as on macOS ([remote-hosts.md](remote-hosts.md)):
Settings → **Remote hosts** → **Add host…**, then **Open on a remote host…**
on Welcome, the host's row in the sidebar's Remote section, or Ctrl+Shift+O.
The app runs Windows' own OpenSSH client (`C:\Windows\System32\OpenSSH\ssh.exe`,
part of Windows 10 1809 and later; `ssh.exe` on `PATH` otherwise) with
`BatchMode=yes`, so sign-in is key-based: load the key into the
`ssh-agent` service (`Start-Service ssh-agent; ssh-add`), set an identity
file for the host, or put the host in `%USERPROFILE%\.ssh\config`, whose
`Host` aliases the Add host form offers. Connect once from a terminal
(`ssh host`) to accept the host key. The remote machine is Unix and needs
`ctx` installed and a model configured there; the Terminal tab opens
`ssh -t` shells on it. `CTX_SSH_BINARY` names another ssh client (a Git for
Windows `ssh.exe`, or a test shim).

## Differences from macOS

- **Computer Use** is macOS only; it is not in the app catalog on Windows.
- **Python projects**: detected test commands use the `py` launcher (else
  `python`), not `python3`, which on Windows is only the Microsoft Store
  installer alias.
- **Line endings**: `apply_patch` edits CRLF files (Git for Windows'
  default `core.autocrlf=true`) in place and keeps their line endings.
- **Projects directly under a drive** (`D:\work`) get git snapshots for
  rewind like any other folder; the drive root and system folders
  (`C:\Users`, `C:\Windows`, `C:\Program Files`) never do.
- **Language servers and MCP servers** installed through npm (`.cmd`
  shims) are found through `PATHEXT`.
