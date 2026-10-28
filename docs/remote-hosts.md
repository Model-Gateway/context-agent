# Remote hosts (SSH)

The macOS app can work on a project that lives on another machine, the way
Claude Code's desktop app and Codex open sessions over SSH: Context runs
`ctx serve` on the host and talks to it through the SSH session, so the
files, the index, the chats, the git operations and the terminal are all
the host's. The app on your Mac is only the window.

## What the host needs

- **`ctx` installed.** Build it there (`cargo install --path crates/cli`
  from a checkout, or copy a release binary to `~/.cargo/bin`) so it is on
  the PATH of a non-interactive shell; the app also looks in
  `~/.cargo/bin`, `~/.local/bin`, `/usr/local/bin` and `/opt/homebrew/bin`.
  Any other place: set the **ctx path** on the host in Settings.
- **A model configured on the host.** The remote `ctx` reads its own
  `~/.context-agent/config.toml` and environment: put the API key there
  (`ctx` → Settings on the host, or `OPENAI_API_KEY` in the login
  environment of non-interactive shells, e.g. `~/.bashrc` on Ubuntu or
  `~/.zshenv`), or sign in to ModelGateway with the CLI on the host. The
  app never copies your local key over.
- **Key-based SSH.** The app runs `ssh` with `BatchMode=yes`, so a password
  cannot be typed: use a key in `ssh-agent` or an identity file. Everything
  in `~/.ssh/config` applies (`Host` aliases, `User`, `Port`,
  `IdentityFile`, `ProxyJump`). Connect once from Terminal (`ssh host`) to
  accept the host key.

## Adding a host

Settings (⌘,) → **Remote hosts** → **Add host…** (or pick an alias under
**From ~/.ssh/config**). Give it an address, optionally a name, user, port,
identity file, the remote `ctx` path and extra `ssh` options, then **Test
connection**: the app connects exactly as it will later and reports the
`ctx` version and user it found, or explains what ssh said (a refused
key, an unresolvable address, `ctx` not found).

## Opening a project on a host

- Welcome → **Open on a remote host…**, or File → **Open Project on Remote
  Host…** (⌘⇧O), or click the host's row in the sidebar's **Remote**
  section. The picker browses the host's folders (`~` for home, type a
  path, repositories carry a branch glyph); **Open** opens the folder as a
  project.
- The project appears under its host in the sidebar with the usual chats,
  Changes, Project and Inspector tabs. Remote projects are remembered and
  reopened at the next launch (each host connects in parallel); their
  recents show the host as a chip.
- The host row's menu has **Reconnect** (after a dropped link), **Disconnect**
  (closes its projects, keeps them remembered) and **Remove Host…**.

## What differs from a local project

- The Terminal tab opens `ssh -t` shells in the project's folder on the
  host.
- Images and PDFs of the project are not shown in the Editor (text files
  are edited through the backend as usual, with language servers that run
  on the host).
- Finder actions (Reveal in Finder, Open with Default Editor) are not
  offered; Copy Path copies the remote path.
- Attachments you add to a message are uploaded with it; the chat history
  lists them by name.
- Settings → Model, BYOK, Spending, Permissions and Apps configure the
  local backend. The composer's model and approval-mode pickers act on
  the project's own backend, so a remote project can run a different model.
- If the link drops (ssh exits), running tasks on that host end as failed
  and the host shows as failed in the sidebar; Reconnect brings the
  projects back. The app does not retry on its own.

## Under the hood

`ssh -o BatchMode=yes -o ConnectTimeout=20 -o ServerAliveInterval=15 -T
[-p port] [-i key] [-l user] <options> -- <address> 'env
PATH="$HOME/.cargo/bin:$HOME/.local/bin:/usr/local/bin:/opt/homebrew/bin:$PATH" ctx serve'`,
with the desktop protocol's newline-delimited JSON-RPC on the session's
stdin and stdout ([docs/desktop-protocol.md](desktop-protocol.md), "Remote
hosts"). The server is an ordinary `ctx serve`; two methods exist for far
clients: `initialize.host` (who and where the server is) and `fs/list`
(a directory of the host, for the picker and the Files tab), and
`task/run` accepts images and files inline. `CTX_SSH_BINARY` points the app
at another `ssh` (a script, in tests).
