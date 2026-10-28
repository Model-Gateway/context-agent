# Context for Windows (Tauri) — architecture and build contract

The cross-platform desktop app for the Context coding agent, built with
Tauri 2. It is a second front end over the same backend as the macOS app
(`apps/macos`): it launches `ctx serve` as a child process and talks
JSON-RPC 2.0 over the child's stdin/stdout
([docs/desktop-protocol.md](../../docs/desktop-protocol.md)). Windows is
the primary target; it also builds and runs on macOS and Linux, which is
how it is developed and tested on this Mac.

**The product rule: the UI and UX match the macOS app as closely as the
platform allows.** [apps/macos/ARCHITECTURE.md](../macos/ARCHITECTURE.md)
("Design system", "Window layout", "Screens", "State and data flow") is the
specification of what every screen shows and does; this document only says
how the same thing is built with web technology and what differs on
Windows. When the two disagree about *what* a screen does, the macOS
document wins. When this document is silent about a screen, read the macOS
document's section for it and the Swift source it names, and port it.

## Toolchain and rules

- Tauri 2.x (`tauri` crate 2, `@tauri-apps/cli` 2 via npm, no global
  install), Rust stable (the workspace's `rust-version`), Node 22, npm.
- Frontend: React 19 + TypeScript 5.9 (strict) + Vite 7. State in
  `zustand` stores. Styling with CSS Modules (`*.module.css`) and the CSS
  custom properties in `src/theme/tokens.css`; no Tailwind, no component
  library. Icons from `lucide-react` through `src/theme/icons.tsx` only.
- `src-tauri` is its own Cargo workspace (`[workspace]` in its
  `Cargo.toml`; the repository root excludes `apps/desktop/src-tauri`). It
  does **not** depend on the `ctx-*` crates: like the macOS app it only
  speaks the protocol. No network code in the app; the backend does it.
- Everything the backend already does (git, files, models, LSP, MCP) is
  asked of the backend. The app only touches the disk for: locating the
  binary, preferences, pasted images, the file tree in the Inspector,
  reading attachments, and the editor's media viewers.
- Strings are plain English; no localization tables.
- Views are small: one exported component per file plus private
  subcomponents. A view that needs a helper that does not exist adds a
  module-private one in its own file; shared things live where this
  document says they live.
- No `any` in exported signatures. No `console.log` in views; the backend
  layer logs when `CTX_DESKTOP_DEBUG` is set (`src/backend/log.ts`).
- `npm run check` (tsc, eslint) and `cargo check` in `src-tauri` must pass
  after every agent's work; `npm run build` must produce `dist/`.

## Directory layout and file ownership

Ownership tags (`F1`–`F6` foundation agents, `V1`–`V9` view agents) say who
writes a file during the port; an agent touches only its files. Shared
needs go through the contracts below.

```
apps/desktop/
  ARCHITECTURE.md                          # this file
  package.json  vite.config.ts  tsconfig.json  index.html  .eslintrc.cjs   # F0 scaffold
  scripts/embed-ctx.sh  scripts/embed-ctx.ps1   # F0  cargo build -p ctx-cli, copy to src-tauri/binaries/ctx-<triple>[.exe]
  src-tauri/
    Cargo.toml  build.rs  tauri.conf.json  capabilities/default.json  icons/   # F0
    binaries/                              # sidecar: ctx-<target-triple>[.exe] (git-ignored)
    src/main.rs  src/lib.rs                # F1  app setup: plugins, state, command registration, window
    src/backend/mod.rs                     # F1  BackendProcess: spawn ctx serve, LF line reader, stderr ring, exit watcher; `Backends`: one child per backend id (local + one per SSH host), events tagged with the id
    src/backend/launch.rs                  # BackendLaunch (local binary | ssh host): the ssh command line, the quoted remote command, SshHost, ~/.ssh/config aliases (see "Remote hosts (SSH)")
    src/backend/rpc.rs                     # F1  request ids, pending map, writer; notifications → Tauri events
    src/backend/locator.rs                 # F1  find ctx (override, sidecar, CTX_BINARY, cargo bin, PATH, repo walk)
    src/backend/environment.rs             # F1  child environment: PATH merge, login shell (macOS/Linux), API key from the secret store
    src/secrets.rs                         # F1  keyring: service "dev.contextagent.desktop", account = env var name
    src/terminal.rs                        # F6  portable-pty sessions (ConPTY on Windows) ↔ xterm.js; `ssh -t` shells for projects on a host
    src/browser.rs                         # F6  child webview per project for the Browser tab + the agent's browser tool
    src/fs.rs                              # F1  list_dir (file tree), read_text_file (attachments), reveal, open_with
    src/selftest.rs                        # F1  --mock / --project / --autotest parsing (LaunchOptions)
  src/
    main.tsx  App.tsx                      # F0 / V1 (App.tsx: MainWindow)
    theme/tokens.css                       # F2  every colour / font / metric token as CSS custom properties (light + dark)
    theme/global.css                       # F2  reset, body font, scrollbars, selection, focus ring
    theme/icons.tsx                        # F2  `Icon` component + the `Icons` catalogue (every name in apps/macos …/Theme/Icons.swift → lucide)
    theme/logo.tsx                         # F2  LogoView: the ✳ mark as inline SVG from Icons.swift's LogoShape polygons
    components/                            # F2  Chip, IconButton, SearchField, CopyButton, StatusPill, DisclosureRow,
                                           #     EmptyPlaceholder, HalftoneHeader, BrailleSpinner, LineList, Skeleton, Kbd,
                                           #     Popover, Menu (context + dropdown), Sheet (modal), Toast, Tooltip, Segmented, Button
    markdown/parser.ts                     # F3  port of MarkdownParser.swift (blocks: headings, paragraphs, lists, quotes, fences, tables, hr)
    markdown/MarkdownView.tsx              # F3  renders blocks; inline (bold, italic, code, links) via markdown/inline.ts
    markdown/CodeBlock.tsx                 # F3  language label, copy, horizontal scroll, highlighter
    markdown/highlighter.ts                # F3  port of SyntaxHighlighter.swift (regex tokens) → spans with token classes
    markdown/fileReferences.ts             # F3  port of FileReferences.swift (path detection in text → clickable chips)
    backend/types.ts                       # F4  every DTO in Protocol.swift as TypeScript types (InitializeResult, ProjectInfo, SessionSummary, …, AgentEvent union, JSONValue)
    backend/client.ts                      # F4  BackendClient: typed methods over `invoke('rpc_request')`; notification subscription
    backend/errors.ts                      # F4  BackendError, BackendFailure
    backend/log.ts                         # F4  debug logging gate
    models/transcript.ts                   # F5  port of Transcript.swift (TranscriptItem, UserTurn, AssistantTurn, ToolGroup, ToolCallItem, AgentGroup, Outcome, Notice, Approval)
    models/reducer.ts                      # F5  TranscriptReducer (the 16 rules in the macOS doc) + delta coalescing at 33 ms
    models/toolMetadata.ts                 # F5  port of ToolMetadata.swift (name → icon, label, category, verbs, path argument)
    models/preferences.ts                  # F5  Preferences store (localStorage-backed, same keys as Preferences.swift where they are plain values; `sshHosts` is Swift's ssh-hosts.json)
    models/sshHost.ts                      # SSHHost, RemoteProjectRef (`ctx-ssh://<host id>/path` keys), the SSHHosts store, ~/.ssh/config aliases
    models/modelCatalog.ts                 # F5  port of ModelCatalog.swift (recents per base URL, fuzzy ranking, prefix grouping)
    models/modelsDev.ts                    # F5  port of ModelsDev.swift (models.dev info fetched by the backend? no: cached JSON in the app; see below)
    models/permissions.ts                  # F5  port of PermissionProfiles.swift
    state/app.ts                           # F5  AppState store: connection, config, projects, selection, sidebar/inspector, toasts, approvals, intents
    state/project.ts                       # F5  ProjectState (per project) + ChatState (per session) classes held in the store
    state/gateway.ts                       # V8  GatewayState (ModelGateway account, device flow)
    state/editor.ts                        # V6  EditorWorkspace / EditorDocument
    state/git.ts                           # V5  git actions (GitActions.swift)
    state/worktrees.ts                     # V5  WorktreeActions.swift
    state/remote.ts                        # RemoteConnection (a host's `ssh … ctx serve` and its state / hint); the intents live in state/app.ts
    state/keys.ts                          # V1  global shortcuts (AppCommands.swift), `mod` = Ctrl on Windows/Linux, ⌘ on macOS
    views/MainWindow.tsx                   # V1  TopBar + Sidebar | Detail | Inspector, toasts, window controls
    views/TopBar.tsx                       # V1
    views/Sidebar/SidebarView.tsx  SidebarRows.tsx  HostRows.tsx   # V1 (HostRows: the "Remote" section, one host row with its projects beneath it)
    views/Remote/RemoteFolderPicker.tsx    # the remote folder picker sheet over `fs/list {dirs_only}`
    views/Welcome/WelcomeView.tsx          # V1  hero, Open a project…, recents, Connect-a-model card, backend failure panel, drop target
    views/Palette/CommandPalette.tsx       # V1
    views/Detail/DetailView.tsx            # V1  tab content switch (Chat / Changes / Project / Editor / Apps / Profile)
    views/Chat/ChatView.tsx  EmptyChatView.tsx  TranscriptItemViews.tsx  ThinkingView.tsx   # V2
    views/Chat/ToolTimelineView.tsx  AgentTimelineView.tsx  ToolOutputView.tsx               # V3
    views/Chat/ApprovalCardView.tsx  OutcomeCardView.tsx  QuestionCardView.tsx  RewindSheet.tsx   # V3
    views/Composer/ComposerView.tsx  AttachmentViews.tsx  ModelPickerPopover.tsx  ModelHoverCard.tsx   # V4
    views/Composer/PermissionsPopover.tsx  StatusBarView.tsx  MentionMenu.tsx                 # V4
    views/Changes/ChangesView.tsx  DiffView.tsx  GitSheets.tsx                                 # V5
    views/Project/ProjectView.tsx          # V5
    views/Editor/EditorView.tsx  CodeEditor.tsx  CompletionPanel.tsx  MediaViews.tsx           # V6  CodeMirror 6
    views/Inspector/InspectorView.tsx  FilesTab.tsx  AgentsTab.tsx  TerminalTab.tsx  BrowserTab.tsx   # V7
    views/Settings/SettingsView.tsx  SettingsSections.tsx  BYOKSettingsSection.tsx  SpendingSettingsSection.tsx  BillingSettingsSection.tsx  RemoteHostsSettingsSection.tsx   # V8
    views/Account/AccountView.tsx  UsageHeatmap.tsx  UsageBarChart.tsx                        # V8
    views/Apps/AppsView.tsx                # V9  marketplace, app page, connect page
```

## Build and run

```
cd apps/desktop
npm install
npm run tauri dev            # Vite + the Rust shell; finds ctx via the locator (a repo checkout's target/release/ctx works)
npm run tauri dev -- -- --mock   # scripted backend, no API key
npm run check                # tsc --noEmit && eslint
npm run build                # dist/ only
npm run tauri build          # installer: NSIS .exe + MSI on Windows, .app/.dmg on macOS
scripts/embed-ctx.sh         # (macOS/Linux) build ctx and copy it to src-tauri/binaries/ for bundling
scripts/embed-ctx.ps1        # (Windows)
scripts/release.ps1          # (Windows) release installers signed with Azure Artifact Signing, in the repo's dist/ (see its header; CI: .github/workflows/release-windows.yml)
```

`tauri.conf.json`: `productName` "Context", identifier
`dev.contextagent.desktop.tauri` (distinct from the macOS app's `dev.contextagent.desktop`, so the two never share LaunchServices, WebKit or app-data state; the credential-store service stays `dev.contextagent.desktop` so both apps share stored keys), `bundle.externalBin: ["binaries/ctx"]` (Tauri
appends the target triple), window `label: "main"`, 1180×780, min
960×600, `decorations: false` on Windows and Linux (custom title bar
region in `TopBar`), `titleBarStyle: "Overlay"` + `hiddenTitle: true` on
macOS so the traffic lights sit over the top bar like the native app.
`app.security.csp` allows `'self'`, inline styles, `data:` and `asset:`
images. Plugins: `dialog`, `opener`, `notification`, `clipboard-manager`,
`process`, `os`, `window-state`.

Launch options (`src-tauri/src/selftest.rs`, mirrored by `LaunchOptions`
in Swift): `--mock` / `CTX_DESKTOP_MOCK=1`, `--project <path>`, `--binary
<path>`, `--autotest <dir>` (runs the scripted flow in `src/selftest.ts`
against the mock backend and writes `result.json` into the directory, then
exits; no screenshots). The frontend reads them with the `launch_options`
command.

## Contracts

### Rust ↔ frontend (Tauri commands and events)

All commands live in `src-tauri/src/lib.rs`'s `generate_handler!`. Names
are snake_case. Every command returns `Result<T, String>` where the error
string is user-presentable.

```
launch_options() -> { mock, project, binary, autotest }
backend_start({ backend?: string, host?: SshHost, binary_override?: string, data_dir?: string, mock: bool, api_key_env?: string }) -> { pid, binary, searched: string[] }
      // locate → environment → spawn; fails with a BackendFailure-shaped error JSON:
      // { kind: "not_found", searched } | { kind: "exited", code, tail } | { kind: "transport", message }
      // `backend` names the child ("local" by default; a host's id for its ssh child); with `host` the child is
      // `ssh … ctx serve` on that host (binary_override and api_key_env do not apply) — see "Remote hosts (SSH)"
backend_stop({ backend? }) -> ()   // "shutdown" request → wait ≤2 s → kill
backend_remove({ backend }) -> ()  // stop + forget a host's entry (Remove Host)
backend_state({ backend? }) -> { state: "stopped"|"starting"|"running"|"failed", pid?, detail? }
backend_stderr({ backend? }) -> string[]       // last 500 lines
rpc_request({ backend?, method: string, params?: JSON }) -> JSON   // resolves with `result`, rejects with { code, message, kind? } as a JSON string
backend_candidates() -> string[]   // the locator's search list, for the failure panel
ssh_config_hosts() -> string[]     // the `Host` aliases of ~/.ssh/config (Include one level deep, patterns skipped)

secret_get({ account }) -> string | null
secret_set({ account, value }) -> ()
secret_remove({ account }) -> ()

fs_list_dir({ path }) -> [{ name, path, is_dir, size }]        // one level, sorted dirs first, hides .git and .DS_Store
fs_read_text({ path, max_bytes }) -> { content, truncated, size }
fs_file_info({ path }) -> { size, is_dir, modified, mime? }
fs_write_temp_image({ bytes_base64, extension }) -> path        // pasted images under <app data>/Pasted Images
fs_pick_project() -> string | null                              // directory picker (plugin-dialog is also fine from TS)
reveal_in_file_manager({ path }) -> ()                          // Explorer / Finder
open_with_default_app({ path }) -> ()
open_url({ url }) -> ()

terminal_open({ cwd, cols?, rows?, host?: SshHost }) -> { id, shell, pid }   // F6; with `host`: `ssh -t <host> 'cd <cwd>; exec $SHELL -l'`
terminal_write({ id, data }) -> ()
terminal_resize({ id, cols, rows }) -> ()
terminal_close({ id }) -> ()
browser_create({ project_id, bounds }) / browser_navigate / browser_back / browser_forward / browser_reload / browser_set_bounds / browser_destroy   // F6
browser_perform({ project_id, action }) -> { ok, content, url, title, error }   // F6: the agent's browser tool actions
```

Events (Rust → frontend, `app.emit`):

```
"backend:notification"  { backend: string, method: string, params: JSON }   // every notification frame, in order, tagged with its backend id
"backend:exited"        { backend: string, code: number, tail: string }
"backend:stderr"        { backend: string, lines: string[] }
"terminal:data"         { id, data }      "terminal:exit" { id, code }
"browser:state"         { project_id, url, title, loading, can_go_back, can_go_forward, progress }
```

The Rust `BackendProcess` mirrors `BackendProcess.swift`: strict LF
framing (`memchr` on new bytes only), UTF-8 decode per line, a 500-line
stderr ring, an exit watcher that emits `backend:exited` and fails every
pending request. The writer is one task behind an `mpsc` channel so lines
never interleave. On Windows the child is spawned with
`CREATE_NO_WINDOW`, and `stop()` cannot send SIGTERM: it sends `shutdown`,
waits two seconds, then kills. `Backends` (managed state) holds one
`Backend` per id: `local`, and one per SSH host; quitting stops them all.

### `BackendClient` (`src/backend/client.ts`)

One instance per backend: `backend` (id `local`) is the local `ctx`; a
remote host's client is `new BackendClient(host.id)` started with `host`
in its options. One set of event listeners routes each shell event to the
client whose id it carries.

```ts
export class BackendClient {
  readonly id: string                                     // 'local', or the host's id
  state: 'stopped' | 'starting' | 'running' | 'failed'
  onNotification?: (n: BackendNotification) => void      // called in frame order on the UI thread
  stderrLog: string[]
  start(opts: { mock: boolean; dataDir?: string; apiKeyEnv?: string; binaryOverride?: string; host?: SSHHost }): Promise<InitializeResult>
  stop(): Promise<void>
  remove(): Promise<void>                                 // stop + backend_remove (a removed host)
  fsList(path: string | null, dirsOnly?: boolean): Promise<FsListing>   // a directory of the backend's machine
  // one method per protocol method, same names as BackendClient.swift:
  initialize(); configGet(); configPatch(values); projectOpen(path, exact, initGit); projectClose(id); projectStatus(id); indexRebuild(id)
  sessionsList(projectId, limit); sessionCreate(projectId, title?); sessionRename(...); sessionDelete(...); sessionHistory(...); sessionRewind(...)
  taskRun(projectId, sessionId, prompt, options); taskSteer(...); taskCancel(projectId, sessionId?)
  approvalRespond(...); questionRespond(...); browserRespond(...); permissionsSet(...)
  modelsList(projectId?); modelsSwitch(projectId, model); suggestNext(...); suggestStarters(...)
  git*(…) for every git/* method; worktree*(…); fileRead; fileWrite; filesSearch; symbolsFind; lsp*(…); tasksList; memoryList; doctorRun
  mcp*(…); spendSummary; pricingLookup; gateway*(…); shutdown()
}
export type BackendNotification =
  | { kind: 'event'; projectId; sessionId; event: AgentEvent }
  | { kind: 'taskFinished'; … } | { kind: 'browserRequest'; … } | { kind: 'questionRequest'; … }
  | { kind: 'lspDiagnostics'; … } | { kind: 'mcpStatus'; … } | { kind: 'indexProgress'; … } | { kind: 'indexReady'; … }
  | { kind: 'projectChanged'; … } | { kind: 'log'; level; message } | { kind: 'exited'; code; tail }
```

`types.ts` is a faithful port of `Protocol.swift`: same type names, same
optionality, snake_case wire fields kept as they are on the wire (no
renaming layer; the Swift app's `CodingKeys` tell you the wire names).
`AgentEvent` is a discriminated union on `event`.

### Stores (`src/state/app.ts`, `src/state/project.ts`)

One `zustand` store `useApp` holding what `AppState.swift` holds (see the
macOS doc's "Declared types" and `AppState` listing): `connection`,
`config`, `projects` (an ordered array of `ProjectState`), the selection,
`sidebarItem`, `sidebarVisible`, `inspectorVisible`, `inspectorTab`,
`inspectorWidth`, `paletteVisible`, `modelPickerVisible`,
`composerFocusToken`, `pendingDraft`, `pendingAttachments`, `rewindRequest`,
`toasts`, and the same intents as methods: `startBackend`,
`restartBackend`, `chooseProject`, `openProject`, `closeProject`,
`selectProject`, `selectSession`, `newChat`, `send`, `retry`,
`cancelRunningTask`, `primaryAction`, `denyPendingApproval`, `respond`,
`switchModel`, `refreshModels`, `setApprovalMode`, `rebuildIndex`,
`renameSession`, `deleteSession`, `beginEdit` / `cancelEdit` /
`submitEdit`, `undoLastTurn`, `performRewind`, `patchConfig`, `reveal`,
`handle(notification)`, `openSettings(tab?)`, `showAgent`.

`ProjectState` and `ChatState` are plain classes (not React state) held in
the store and mutated in place; every mutation ends with
`useApp.getState().touch(chat)` which bumps a per-chat `version` number the
views subscribe to (`useChat(sessionId)` returns `[chat, version]`). The
reducer's delta coalescing flushes `live` at most every 33 ms. The
transcript list is virtualised with a tail window exactly as the macOS
`TailWindow` (pages of 20, at most 60 loaded, newest page first,
`ScrollKeeper` semantics preserved) implemented in
`src/components/TailWindow.tsx` (F2).

Notification routing (`handle`) follows the macOS doc's `handle` rules
verbatim, including `exited` (fail running chats, `connection = failed`,
toast, restart once after 1 s unless we stopped it).

### Preferences (`src/models/preferences.ts`)

`localStorage` under the key prefix `ctx.`, same names as
`Preferences.swift`'s properties (`backendPathOverride`, `settingsTab`,
`inspectorWidth`, `collapsedProjects`, `editorAutoSave`, `suggestNext`,
`notifyOnFinish`, `notifyOnApproval`, `appearance` (system | light |
dark), `developerMode`, `useContextEngine`, `gatewayAccount`,
`gatewayPreviousEndpoint`, `routeThroughGateway`, `attributeProjects`,
`recents` (RecentProject[] with pinned, and `hostId` / `hostName` for a
folder on a host), `modelRecents`, `sshHosts` (the SSH hosts, Swift's
`ssh-hosts.json`)). A `usePref(key)` hook subscribes views.

### Secrets

API keys go to the OS credential store through the `secret_*` commands
(Windows Credential Manager, macOS Keychain with the same service name as
the Swift app so both apps share keys, Secret Service on Linux). The
backend receives the key in its environment under `model.api_key_env`,
built in Rust (`environment.rs`) exactly as `BackendEnvironment.build`:
process environment, then (macOS/Linux only) the login shell's variables
and PATH, then the extra PATH directories (`~/.cargo/bin`, and on Windows
`%USERPROFILE%\.cargo\bin`, `%LOCALAPPDATA%\Context` and `%ProgramFiles%\Context`
where the NSIS and MSI installers put the app), then the
key from the secret store when the variable is not already set.

## Design system

The tokens are the macOS ones, as CSS custom properties on `:root`
(light) and under `:root[data-theme="dark"]` / `@media
(prefers-color-scheme: dark)` when `data-theme` is `system`. Names:
`--canvas --surface --surface-raised --sidebar --border --border-strong
--text-primary --text-secondary --text-tertiary --accent --accent-soft
--ultra --success --warning --danger --code-background --code-string
--code-keyword --code-number --code-comment --code-type --code-function
--badge-background --badge-text`, values from `Theme.swift` (`Hex`).

Typography: `--font-ui: -apple-system, "Segoe UI Variable Text", "Segoe
UI", system-ui, sans-serif`; `--font-mono: ui-monospace, "Cascadia Mono",
"SF Mono", Menlo, Consolas, monospace`. Sizes in px equal to the macOS
points: `--text-title 22/600`, `--text-heading 15/600`, `--text-body 14`,
`--text-secondary 13`, `--text-caption 12`, `--text-mono 12.5`,
`--text-sidebar 13.5`, `--text-mono-small 11.5`. Metrics: `--sidebar-width
248px`, `--inspector-width 320px` (260–720 drag), `--content-max-width
760px`, `--radius-small 8px`, `--radius 12px`, `--radius-large 16px`,
spacing `--space-1 4px` … `--space-6 32px`, `--top-bar-height 52px`.
`--shadow-floating: 0 4px 10px rgba(0,0,0,0.06)`.

Icons: `Icons.swift` names map one-to-one to lucide components in
`theme/icons.tsx` (`Icons.welcome` → `Hand`, `newChat` → `Plus`, `changes`
→ `GitBranch`, `project` → `Box`, `inspector` → `PanelRight`, `settings`
→ `Settings`, `search` → `Search`, `send` → `ArrowUp`, `stop` → `Square`
filled, `approval` → `Hand`, `thinking` → `Brain`, `terminal` →
`Terminal`, `browser` → `Globe`, `apps` → `LayoutGrid`, `mcp` → `Puzzle`,
`agents` → `Network`, …); `<Icon name="…" size={16} />` renders at the
SF-Symbol sizes the Swift views use (16 default, 12 in chips, 14 in
buttons). `starter(kind)` and `fileType(path)` are ported.

Loading: `BrailleSpinner` (⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ at 80 ms, static ⠿ "Working" under
`prefers-reduced-motion`). Never a native progress control. A project
opening and building its index shows `ProgressRing` (an SVG arc filled to
`index/progress` done/total, a turning quarter arc until the total is
known) on the recent cards and the empty chat's index card.

Accessibility: every `IconButton` has `aria-label` and a tooltip; pills
carry the status word in text; decorative elements `aria-hidden`. All
transcript text, code and tool output is selectable.

## Window layout and platform mapping

`MainWindow`: a column — `TopBar` (52 px, `data-tauri-drag-region`, full
width) over a row `SidebarView` (248 px, hairline right, hidden with
`mod+ctrl+S`… see shortcuts) · `DetailView` · `InspectorView` (320 px,
hairline left, drag handle on its left edge 260–720 px, centre pane never
below 420 px, double-click restores 320, width persisted). On Windows and
Linux the top bar's right end carries the window controls (minimize,
maximize/restore, close; 46 × 32 px each, close turns `danger` on hover,
rendered with the `window` plugin's `getCurrentWindow()`); on macOS the top
bar leaves a 90 px leading inset for the traffic lights. The window's
position and size persist through `plugin-window-state`.

Keyboard: `mod` is Ctrl on Windows/Linux and ⌘ on macOS. The macOS doc's
shortcuts map as: ⌘⏎ → `mod+Enter` (primary action), ⌘⌫ → `mod+Backspace`
(deny), ⌘K palette, ⌘, settings, ⌘N new chat, ⌘O open project, ⌘. cancel
(`mod+.`), ⌘F find, ⌘S save, ⌘⇧W close editor tab, ⌘⇧I inspector, ⌃⌘S →
`ctrl+alt+S` sidebar on Windows (`ctrl+cmd+S` on macOS), ⌘⇧M model
picker, ⌘⌥Z undo last turn, ⌘1–4 tabs. The `Kbd` component renders `Ctrl`
/ `⌘` by platform. `state/keys.ts` is the only owner of global key
handling; Esc / Return inside popovers never reach the composer.

Menus: no native menu bar on Windows; every command is reachable from the
command palette and the UI. On macOS Tauri's default menu is kept.

File dialogs: `@tauri-apps/plugin-dialog` (`open({ directory: true })`
for projects, `open({ multiple: true })` for attachments). "Reveal in
Finder" reads "Show in Explorer" on Windows (`Platform.revealLabel`), "Open
with Default Editor" stays. Paths are displayed as the backend sends them
(it already uses the platform's separators).

Fonts in the terminal and editor: `Cascadia Mono`, `Consolas` on
Windows; `SF Mono`, `Menlo` on macOS.

Notifications: `@tauri-apps/plugin-notification` for "Task completed" /
"Approval needed" when the window is not focused or the chat is not
selected; a click focuses the window and selects the chat. The unseen /
attention count goes to the taskbar as an overlay badge (`setBadgeCount`
where supported).

Drag and drop: `getCurrentWebview().onDragDropEvent` provides paths;
dropping on the Welcome view opens a project, dropping on the composer
attaches.

Clipboard: `plugin-clipboard-manager` for copy; paste through the
composer's `paste` event (files from the clipboard are not available in
WebView2; pasted images come as `image/*` blobs and are saved with
`fs_write_temp_image`).

## Screens

Every screen is specified in the macOS document's "Screens" section and
the Swift file named there; port it one-to-one, with these web
substitutions:

- **Composer text view**: a `textarea` that auto-grows 1→8 lines (measure
  with a hidden clone); Enter sends, Shift+Enter newline, IME composition
  (`isComposing`) never sends; `mod+Enter` queues while running; ghost
  suggestion text rendered as an overlay behind the textarea, `→` accepts.
- **Markdown**: `markdown/MarkdownView.tsx` (own parser port, not a
  library) so headings, lists, tables, fences, quotes and inline styles
  look like the macOS app; links open through `open_url`.
- **Diff view**: a virtualised list of lines (one `div` per line, `react-
  virtuoso` is allowed here and in the transcript's tail window) with the
  gutter, full-width `+`/`−` tints, hunk headers, collapse after 20 hunks.
- **Code editor**: CodeMirror 6 (`@codemirror/*`, `@codemirror/lang-*` for
  rust, python, javascript/typescript, go, java, json, markdown, yaml,
  html, css; `@codemirror/legacy-modes` for toml, shell, swift) with line
  numbers, the theme built from the tokens, `mod+S` save, the completion
  panel fed by `lsp/completion` / `symbols/find` through a CodeMirror
  completion source, diagnostics as `@codemirror/lint`, `mod`+click to
  definition.
- **Terminal**: `@xterm/xterm` + `@xterm/addon-fit` over the `terminal_*`
  commands; the shell is `%COMSPEC%`'s PowerShell (`pwsh` when installed,
  else `powershell`) on Windows, the login shell elsewhere.
- **Browser tab**: a Tauri child webview (`browser.rs`) positioned over
  the tab's area and resized with it; the toolbar is HTML. The agent's
  `browser/request` actions run as scripts in the child webview
  (`snapshot` numbers interactive elements with `data-ctx-ref`,
  `click`/`type`/`select`/`scroll` act on them, `evaluate` runs a function
  body, `screenshot` returns `error: "unsupported"` on this platform
  until Tauri exposes capture); the result comes back through an
  initialization script that posts to a `browser_result` command.
- **Usage charts**: inline SVG (`UsageBarChart`, `UsageHeatmap`) drawn
  from the same numbers.
- **Pasted images**: saved under the app data directory (`appDataDir()` /
  `Pasted Images`), pruned after 30 days.
- **Media viewers**: `<img>` from `convertFileSrc(path)`; PDFs in an
  `<iframe>` of the asset URL (WebView2 and WKWebView both render PDFs).

## Remote hosts (SSH)

The port of the macOS feature (apps/macos/ARCHITECTURE.md "Remote hosts
(SSH)", docs/remote-hosts.md): a project can live on another machine; the
app runs `ctx serve` there over SSH and talks the same protocol through the
SSH session's pipes (docs/desktop-protocol.md "Remote hosts"). The server
needs nothing new. What differs from the macOS app is only where the
pieces live:

- **Rust launches ssh.** `src-tauri/src/backend/launch.rs` is
  `BackendLaunch.swift` + `SSHHost.swift`: `BackendLaunch::Ssh(host)` runs
  `ssh -o BatchMode=yes -o ConnectTimeout=20 -o ServerAliveInterval=15 -o
  ServerAliveCountMax=4 -o ClearAllForwardings=yes -T [-p port] [-i key]
  [-l user] <extras> -- <address> 'env PATH="$HOME/.cargo/bin:$HOME/.local/bin:/usr/local/bin:/opt/homebrew/bin:$PATH" ctx serve [--mock]'`
  — the address after `--`, the remote command quoted for a POSIX shell
  (the remote is Unix; `$HOME`/`$PATH` expand there). The client is
  `/usr/bin/ssh`, on Windows `%SystemRoot%\System32\OpenSSH\ssh.exe`
  (`ssh.exe` on PATH as a fallback), or `$CTX_SSH_BINARY` for tests. The
  child gets the login shell's environment (SSH_AUTH_SOCK, the user's PATH)
  and no API key (the host's `ctx` has its own); on Windows it is spawned
  with `CREATE_NO_WINDOW` like every child (`configure_platform`, the same
  flag `ctx_core::platform::hide_console_window` sets; the crate does not
  depend on `ctx-core`). `Backends` keeps one `Backend` per backend id and
  every event (`backend:notification` / `exited` / `stderr`) carries the id,
  so the frontend routes it to the right `BackendClient`; the commands take
  `backend` (default `local`). `ssh_config_hosts` reads the `Host` aliases
  of `~/.ssh/config` for the settings form.
- **One connection per host.** `state/remote.ts` is `RemoteConnection`: the
  `SSHHost` as configured when it connected, its own `BackendClient`
  (`id = host.id`), a `state` (disconnected / connecting / ready / failed)
  and a `hint` (`remoteHint`: what ssh's last words mean — keys, PATH, the
  address, the host key). The intents are in `state/app.ts` ("Remote hosts"
  section): `remoteFor`, `connectHost` (made when the first project on the
  host is opened; concurrent callers share one attempt), `disconnectHost`
  (closes its projects with `forget` false), `reconnectHost`, `removeHost`,
  `saveHost`, `testHost` (a saved host on its own connection, a draft on a
  throwaway client), `chooseRemoteProject`, `listRemoteDirectory`,
  `reopenRememberedRemoteProjects` (at launch, each host in parallel, after
  — and independently of — the local start). `handleRemoteExit` fails the
  host's running chats, marks the connection failed and leaves the rows in
  place for Reconnect; never an automatic retry. `--mock` / Settings →
  General's mock toggle apply to every backend.
- **A remote project is a `ProjectState` with a `backend` and a `host`.**
  Every per-project request goes through `project.backend` (`ProjectState`'s
  loads, `state/git.ts`, `state/editor.ts`, `state/worktrees.ts` and the
  store's intents); chat-scoped ones use `backendFor(projectId)`. Readiness
  is `isProjectReady(state, project)` and the key check
  `needsAPIKeyFor(state, project)` (selectors.ts), so a remote project never
  looks at the local backend. Notifications are routed by project id, which
  `openProject` keeps unique across backends (the same path on two hosts
  hashes to one id: the second open is refused with a toast); `log` lines
  get a `[host]` prefix, `mcp/status` is the local backend's only.
  `makeProject` builds the `ModelCatalog` from the host's `initialize`.
- **Keys.** Preferences and recents key a project by `ProjectState.key` /
  `recentProjectId`: the path locally, `ctx-ssh://<host id>/path` on a host
  (`models/sshHost.ts` `RemoteProjectRef`). `openProjectPaths` mixes both;
  `openingProjectPaths` holds keys too (the loading rows decode them). The
  hosts themselves are the `sshHosts` preference (same JSON shape as the
  Swift `SSHHost`; the Rust `SshHost` reads it as is).
- **No shared filesystem.** The Files tab lists folders through `fs/list`
  (`FileTreeModel(root, onChange, remote)`), the folder picker is the
  `RemoteFolderPicker` sheet over `fs/list {dirs_only}` (`views/Remote/`),
  the Editor reads and writes through `file/read` / `file/write` as always
  but shows a placeholder for images and PDFs, Explorer / Finder and "Open
  with Default Editor" are left out (`FileRow.localActions`, `reveal`), and
  the Terminal tab's shells are `ssh -t <host> 'cd <root>; exec $SHELL -l'`
  (`TerminalWorkspace(directory, host)` → `terminal_open({ host })`).
  Attachments: `task/run` and `task/steer` get the bytes
  (`prepareRemoteAttachments` → `InlineImage` / `FileUpload.content`)
  instead of paths; the transcript keeps the local paths for its thumbnails.
- **UI.** Welcome: "Open on a remote host…" under the open button (Settings
  → Remote hosts when none is configured). Sidebar: a "Remote" section
  (`HostRows.tsx`) listing every configured host with its projects,
  worktrees, opening rows and pinned recents beneath it; the Projects
  section shows local projects only. Palette: "Open project on remote
  host…" (`mod+shift+O`). Recents cards carry the host as a chip and open
  through `openRecent`. Settings → Remote hosts
  (`RemoteHostsSettingsSection.tsx`): the rows with state, hint and Connect /
  Disconnect / Edit… / Remove…, "Add host…" and the `~/.ssh/config` aliases
  menu, the editor sheet with Test connection. The Inspector Run tab's
  Backend card describes the selected project's host connection
  (Reconnect instead of Retry).
- **Testing without a server.** `CTX_SSH_BINARY` points the shell (and
  `scripts/dev-bridge.mjs`, which keeps one child per backend id too) at a
  stand-in ssh: a script that ignores the options and runs its last
  argument with `sh -c`, with the host's ctx path set to a local build
  (`target/debug/ctx`). The browser preview then opens a "remote" project,
  chats, lists files, loses the link when that `ctx serve` exits and
  reconnects from the host row.

## Self-test mode

`--autotest <dir>` with `--mock`: `src/selftest.ts` opens the project
given by `--project` (or the repository root), creates a session, sends
"Summarise this repository", waits for `task/finished`, checks that the
transcript holds the two tool rows and the final text, exercises the
Changes and Project tabs, and writes `result.json` (`{ ok, steps: [{ name,
ok, detail }] }`) before `exit(0)` / `exit(1)`.

## Foundation notes

The names V1–V9 must use (foundation gate); this wins over "Contracts". Props: `src/components/README.md`.

### Backend (`src/backend/*`)

- `import { backend } from '../backend/client'` (singleton, also re-exported by
  `state/app`); views normally use store intents. Methods are BackendClient.swift's
  names (`sessionsList`, `gitStatus`, `gitCommit`, `worktreeCreate`, `fileRead`,
  `fileWrite`, `filesSearch`, `symbolsFind`, `lspOpen`/`lspChange`/`lspClose`/
  `lspCompletion`/`lspHover`/`lspDefinition`, `mcpCatalog`, `mcpOAuthStart`,
  `gatewayStatus`, `gatewayUsage`, `gatewayBilling`, `spendSummary`, …). One order
  differs from Swift: `taskRun(projectId, sessionId, prompt, options = {}, images
  = [], files = [])`; `taskSteer(…, prompt, images, files)` keeps Swift's.
- `types.ts`: Swift type names, snake_case wire fields, `normalize*` for Swift
  defaults; `AgentEvent` is a union on `event` (plus `{ event: 'unknown', name }`).
- `errors.ts`: `BackendError` (`detail.type` rpc | transport | notRunning |
  decoding | binaryNotFound; `isTaskAlreadyRunning`, `isNoTaskRunning`,
  `isProjectNotOpen`, `isConnectionLoss`), `BackendFailure` (`kind` notFound |
  incompatible | exited | transport), `describe(failure, home?)` → `{ title,
  detail, explanation, summary }` (Welcome failure panel), `failureSearchedPaths`,
  `failureStderrTail`, `errorText(e)`. Debug logs: localStorage `ctx.debug`.

### Store (`src/state/app.ts`, `selectors.ts`)

- `useApp(selector)` in views, `useApp.getState().<intent>()` elsewhere. **No
  `toasts` field**: `toasts.push({ message, level?, details? })` from
  `components/toasts`; mount `<ToastHost />` once in MainWindow.
- Intent names beyond the contract: `respond(requestId, chat, decision)`
  (approval), `respondQuestion(requestId, chat, answers | null, message?)`,
  `setPendingDraft(projectId, text)`, `setPendingAttachments(projectId, list)`,
  `setSidebarItem`, `toggleSidebar`, `toggleInspector`, `setInspectorTab`,
  `setInspectorWidth`, `setPaletteVisible`, `toggleModelPicker`, `focusComposer`,
  `setRewindRequest`, `undo(turn, chat)`, `removeQueued(i, chat)`, `editQueued(i,
  chat)`, `canRewind(chat)`, `runInTerminal(cmd)`, `requestSuggestion`,
  `loadStarters`, `modelDisplayName(id)`, `modelInfo(id)`, `modelStat(id,
  project?)`, `runDoctor`, `refreshMcpStatus`, `reconnectMcp`, `loadMcpCatalog`,
  `installMcpApp`, `uninstallMcpApp`, `isAPIKeyStored`, `saveAPIKey`,
  `removeAPIKey`, `chooseBackendBinary`, `copyBuildCommand`, `copyToPasteboard`,
  `selectChange(path, project)`, `setShowStaged`, `setCollapsed(collapsed, project)`.
- `reveal(path, project, action, line?)` with `PathAction` = `'openInEditor' |
  'showInChanges' | 'openWithEditor' | 'revealInFileManager' | 'copyPath'` (Swift
  `revealInFinder`; label `revealLabel` from `components/platform`).
- `connection`: `{ kind: 'connecting' } | { kind: 'ready', result } | { kind:
  'failed', failure }`. Also: `SIDEBAR_ITEMS`, `isProjectTab`, `INSPECTOR_TABS`,
  `inspectorTabTitle`/`Icon`, `makeRewindRequest`, `launchOptions()`,
  `relativePath`, `approvalModeLabel`, `BUILD_COMMAND`, `SETTINGS_TAB_*`.
- Hooks: `useProject(id)`, `useSelectedProject()`, `useChat(sessionId)` or
  `useChat(projectId, sessionId)` → `[chat | null, version]` (never creates),
  `useSelectedChat()`, `useChatValue(chat, select, fallback)`,
  `useProjectValue(project, select, fallback)`, `useSelectedChatRunning()`;
  selectors.ts: `useIsReady`, `useConnectionLabel`, `useNeedsAPIKey`,
  `useAttentionCount`, `useAnyProjectRunning`, `usePendingApprovals`,
  `usePendingDiscountDrop`, `useTopLevelProjects`, `useWorktreeRows(project)` →
  `{ visible, hidden }`, `usePendingProjectPaths`, `useCanRewind(chat)`,
  `useBrowserDriven()` (pure forms drop `use`, e.g. `selectedChat(state)`).
- Rules: never call `project.chat(id)` while rendering (it creates and bumps);
  use `existingChat(id)` / `useChat`. Selectors must return primitives or stable
  references (zustand 5 loops on a fresh object). Mutate `ProjectState` /
  `ChatState` with `update(fn)` or follow with `touch()`.
- Plug-ins: `extendApp({ lspDiagnostics, openInEditor(relativePath, project,
  line), commitFromBox(project), isGatewaySignedIn })` (V6, V6, V5, V8);
  `onProjectClosed(listener)`, `onBackendStarted(listener)` return unsubscribe.
- `project.fileTree`: `FileTreeModel` (`visibleRows`, `toggle(node)`, `select(path)`, `collapseAll()`, `reload()`).

### Models (`src/models/*`)

- Transcript items: union on `kind` (`user | assistant | tools | agents |
  approval | outcome | notice`); computed properties are functions
  (`toolGroupHeadline(g)`, `approvalIsPending(i)`, …); `TranscriptFormat.*`.
  `ChatStatus`, `chatStatusIsRunning`, `chatStatusProgress` are in `reducer.ts`.
- `toolInfo(name)` → `{ icon, label, category, verb, pastVerb }`, `rowLabel(call)`,
  `pathArgument`, `lineChanges`, `mcpParts`, `abbreviate`.
- Preferences: `usePref(key)`, `usePrefState(key)` → `[value, set]`, `getPref`,
  `setPref`; renamed keys `suggestNext`, `useContextEngine`, `routeThroughGateway`,
  `attributeProjects`, `modelRecents`, `recents`. V1 calls `applyAppearance()`
  at boot and on change.
- `project.models` (`ModelCatalog`: `sections(query, displayName?)`, `ranked`,
  `offersCustomName`, `matchSegments`); `modelsDev.info(id, baseURL)`,
  `modelsDev.logo(labId)` (URL; `onError` → `markLogoFailed`),
  `useModelsDevVersion()`, `ModelNames.pretty(id)`; `permissionProfileOptions`,
  `permissionProfileIcon`; attachments `appendingAttachments(paths, list)`,
  `savePastedImage(blob)`, `loadAttachmentPreview(a)`, `attachmentName`.

### Markdown (`src/markdown/*`)

- `<MarkdownView markdown files? onOpenURL? onRunCommand? />` (pass
  `onRunCommand` only with a selected project), `<CodeBlock code language? onRun? />`.
- `FileReferenceContext = { root, changedPaths(), isFile?(absolutePath),
  perform(ref: { path, line?, column? }, action) }`, built in ChatView as in
  ChatView.swift: `perform: (ref, action) => useApp.getState().reveal(ref.path,
  project, action, ref.line)`. `invalidateFileReferences()` after tree changes.
- `lineHighlighter(lang)` goes into `LineList`'s `highlight`; also
  `detectLanguage`, `highlightLine`, `renderSpans`, `languageDisplayName`, `keywordsFor`.

### Components, theme, terminal, browser

- Font sizes: `--font-size-{title,heading,body,secondary,caption,mono,sidebar,mono-small}`;
  `--text-secondary` is the colour. `IconButton` requires `label`. `Menu` items:
  `{ label, onSelect, … } | { type: 'separator' } | { type: 'header', label }`.
  `confirm(opts)` / `showAlert(opts)` return promises, no host needed.
- `TailWindow` defaults `page={20} limit={60}` (AgentsTab passes 30 / 90);
  ChatView uses its handle (`jumpToLatest`, `loadAround`, `stopFollowing`,
  `followIfNeeded`) and `onAtBottomChange`, not its own FollowState.
- Terminal (V7): `terminals(project)` → `TerminalWorkspace`,
  `useTerminalWorkspace(ws)`, `useTerminalSession(s)`; xterm lives on
  `session.view`; wire `session.onData`→`term.write`, `term.onData`→`session.write`,
  `term.onResize`→`session.resize`, `term.onTitleChange`→`session.titleChanged`.
- Browser (V7): `browser(project)` → `BrowserWorkspace`, `useBrowserWorkspace(ws)`,
  `const stop = ws.track(element)` in an effect; address bar via
  `snapshot.addressText`, `setAddressText`, `navigate(text)`; call `hide()` before
  a popover covers the native page. No `browser_result` command: results come
  back by `eval_with_callback` polling; `browser:state` adds `load_error`, `visible`.
- Shell: snake_case argument keys; `rpc_request` rejects with `{ code, message,
  kind? }` (shell-local: code -1, kind `transport | not_running | decoding`);
  `terminal_open` → `{ id, shell, pid }`. Gate config: CSP allows
  `https://models.dev`; capability `opener:allow-open-path`; Node globals for
  `scripts/*.mjs` in eslint.

## View phase: ownership and component contract

This supersedes the `V1`–`V9` tags in "Directory layout" (and the plug-in
owner tags in "Foundation notes": `openInEditor` / `lspDiagnostics` → Editor,
`commitFromBox` → Changes, `isGatewaySignedIn` → Account). Each view agent
writes only its files; a view another agent owns is imported by the path and
export name below, with exactly these props.

| Agent | Swift sources (apps/macos/ContextAgent/…) | Files (src/…) | Exports other agents use |
|---|---|---|---|
| Shell | Views/MainWindow, Views/Detail/DetailView (TopBar + tab switch), Views/Sidebar/*, Views/Welcome/*, App/AppCommands, App/ContextAgentApp | `App.tsx`, `views/MainWindow.tsx`, `views/TopBar.tsx`, `views/WindowControls.tsx`, `views/Detail/DetailView.tsx`, `views/Sidebar/*`, `views/Welcome/*`, `state/keys.ts` | `BackendFailurePanel` (views/Welcome/BackendFailurePanel.tsx, `{ failure: BackendFailure; compact?: boolean }`), `ConnectModelCard` (views/Welcome/ConnectModelCard.tsx, no props) |
| Chat | Views/Chat/ChatView, EmptyChatView, TranscriptItemViews, ThinkingView, SentPromptView | `views/Chat/ChatView.tsx` (`ChatTabView`, `ChatView`), `EmptyChatView.tsx`, `TranscriptItemViews.tsx`, `ThinkingView.tsx`, `SentPromptView.tsx`, `FindBar.tsx`, `DaySeparator.tsx` | `ChatTabView` (no props: the Chat tab) |
| Cards | Views/Chat/ToolTimelineView, AgentTimelineView, ToolOutputView, ApprovalCardView, OutcomeCardView, QuestionCardView, RewindSheet | `views/Chat/ToolTimelineView.tsx` (`{ group: ToolGroup; showsHeader?: boolean; chat?: ChatState }`), `AgentTimelineView.tsx` (`{ group: AgentGroup; chat: ChatState }`), `ToolOutputView.tsx` (`{ call: ToolCallItem }`), `ApprovalCardView.tsx` (`{ item: ApprovalItem; chat: ChatState }`), `OutcomeCardView.tsx` (`{ item: OutcomeItem; chat: ChatState }`), `QuestionCardView.tsx` (`{ chat: ChatState }`), `RewindSheet.tsx` (`RewindSheet`, no props: reads `rewindRequest`), `FullOutputSheet.tsx` | all of these |
| Composer | Views/Composer/* | `views/Composer/ComposerView.tsx` (`ComposerView`: `{ chat: ChatState } \| { draftFor: ProjectState }`), `StatusBarView.tsx` (`{ chat: ChatState }`), `QueueRow.tsx` (`{ chat: ChatState }`), `AttachmentViews.tsx`, `ModelPickerPopover.tsx`, `ModelHoverCard.tsx` (`ModelHoverCard`, `ModelLogo`, `LabLogo`), `PermissionsPopover.tsx`, `EffortPopover.tsx`, `MentionMenu.tsx` | `ComposerView`, `StatusBarView`, `QueueRow`, `ModelLogo` |
| Changes | Views/Changes/*, App/GitActions, App/WorktreeActions | `views/Changes/ChangesView.tsx` (`ChangesView`, no props), `DiffView.tsx`, `GitSheets.tsx`, `state/git.ts`, `state/worktrees.ts` | `ChangesView`; `state/git.ts` intents (`gitAction`, `stage`, `commit`, …); `state/worktrees.ts` (`newWorktreeChat`, `sendInNewWorktree`, `removeWorktree`, `mergeWorktree`) |
| Project | Views/Project/ProjectView, Views/Projects/ProjectsView, Views/Palette/CommandPalette | `views/Project/ProjectView.tsx` (`ProjectView`), `views/Projects/ProjectsView.tsx` (`ProjectsView`), `views/Palette/CommandPalette.tsx` (`CommandPalette`, no props: reads `paletteVisible`) | those three |
| Editor | Views/Editor/*, App/EditorState, Models/EditorApp | `views/Editor/*` (`EditorView`, no props), `state/editor.ts` | `EditorView`; `state/editor.ts` (`editors(project)`, `openInEditor`) |
| Inspector | Views/Inspector/* | `views/Inspector/*` (`InspectorView`, no props) | `InspectorView` |
| Settings | Views/Settings/* | `views/Settings/*` (`ProfileView`, no props: the account item's page with the Account tab and the settings tabs) | `ProfileView` |
| Account | Views/Account/*, App/GatewayState, Models/ModelGateway | `views/Account/*` (`AccountView`, no props), `state/gateway.ts`, `models/modelGateway.ts` | `AccountView`; `state/gateway.ts` (`useGateway`, `startDeviceSignIn`, `signOut`, `refreshAccount`) |
| Apps | Views/Apps/AppsView, Resources/Assets.xcassets/Logos, Banners | `views/Apps/*` (`AppsView`, no props), `assets/logos/*`, `assets/banners/*` | `AppsView`, `appLogo(id)` |

Everything visible follows the macOS screen descriptions in
[apps/macos/ARCHITECTURE.md](../macos/ARCHITECTURE.md) "Screens"; the README
screenshots in `docs/images/*.png` (welcome, new-chat, running, changes,
project, editor, hero) show the target look at 2× scale.

## View notes

View gate result: `npm run check`, `npm run build` and `cargo check` (src-tauri) pass.
The app boots to MainWindow, and DetailView reaches every `SidebarItem`.

**Seams the gate fixed** (everything else was already consistent with the contract table):
- `components/platform.ts` `formatShortcut`: on Windows and Linux it printed "Shift+Ctrl+N"; it now prints "Ctrl+Shift+N", as its doc comment says (`WIN_ORDER` puts `mod` first).
- CSS module classes that did not exist rendered `class="… undefined"`: `styles.above` in `components/Tooltip.tsx` and `styles.lineRow` in `views/Changes/DiffView.tsx`. Both references were removed. No other `styles.x` lacks a rule, and no `var(--x)` is undefined.
- Seams the view agents fixed themselves before the gate, now verified:
  - the U+2028 regex in `state/keys.ts`;
  - the missing `views/Editor/EditorView`;
  - the duplicate gateway store in Settings (removed; Settings, Account and the sidebar all use `state/gateway.ts`);
  - `MentionMenu` now imports `views/Apps/AppIcon`.

**How boot was verified** (repeat this after large changes; no server is needed):
- Build `dist/`, open it in headless Chrome through Playwright, and serve the files through request interception (`context.route('http://ctx.test/**')`).
- An init script installs `@tauri-apps/api/mocks` (`mockIPC` with `shouldMockEvents`, `mockWindows('main')`) and `__TAURI_OS_PLUGIN_INTERNALS__` (pick the platform there).
- An exposed Node function answers the shell commands, using the table from `scripts/dev-bridge.mjs`, against `target/debug/ctx serve --mock --data-dir <tmp>` over stdio. Notifications are replayed with `emit('backend:notification', …)`.
- What was walked:
  - Welcome, Open a project, then the empty chat (starters).
  - Chat: send a prompt, then the tool rows, an output card, the path context menu → Open in Editor, mod+F find, mod+⌥Z RewindSheet, and an Ultra run with agents.
  - The Changes diff, Project, the Editor with a file, all five Inspector tabs, the palette and the model picker.
  - Projects, Apps, then Account and its six settings tabs, plus the failed-backend Welcome/Inspector/Settings.
  - mod+1–4, mod+, and mod+⇧I.
  - Platforms: Windows, macOS, Linux, plus dark mode.
- Also checked: a development build (`NODE_ENV=development vite build --mode development`), which runs StrictMode double effects; and a dynamic `import()` of every emitted chunk.
- Result: no page errors, no console errors and no React warnings; only models.dev is blocked. (Quirk: CodeMirror reads the *host* platform, so on a Mac host with `platform=windows`, Ctrl+K in the editor is Emacs kill-line.)

**Exports outside the contract table that are now load-bearing** (keep the names, or update the importers):
- `views/Chat/AgentTimelineView` (`AgentStatusIcon`, `AgentDetailLine`, `AgentElapsed`, `agent*` helpers) → Inspector AgentsTab.
- `views/Changes/GitSheets` (`PushSheet`, `CommitChangesSheet`) → Inspector ChangesCard.
- `views/Changes/ChangesView` `ChangesStatus` → FilesTab.
- `views/Apps/AppIcon` → Composer MentionMenu.
- `views/Welcome/ConnectModelCard` → EmptyChatView.
- `views/Composer/PermissionsPopover` → TopBar.
- `views/Editor/SaveChangesDialog` is imported by `state/editor.ts`.
- `state/keys.ts` `onFindInChat` → ChatView.
- `state/editor.ts` (`useUnsavedCount`, `saveSelectedDocument`, `closeSelectedDocument`, `setEditorAutoSave`) → TopBar, keys and Settings.
- `state/worktrees.ts` `sendInNewWorktree` → Composer.
- `state/git.ts` `pull` → ChangesCard.

**Duplicates to unify next** (they don't conflict at build time, but they look or behave differently):
- Middle truncation. Canvas-measured: `components/MiddleTruncate`, the Inspector's `MiddleText` and Account's private copy. Approximate (head ellipsis plus a fixed tail of 14–18 characters): Composer `MiddleText` (ModelHoverCard), Changes (`GitSheets` *exports another `MiddleTruncate`*), Project/Projects, Apps, Settings `PathValue`. Move them all onto `components/MiddleTruncate`.
- Small NSSwitch replicas: Settings `Toggle`, Apps `Switch`, Account's private toggle.
- `InlineMessage`: Settings and a private Apps copy.
- `RecentCard`: Welcome and a private copy in ProjectsView.
- EffortLevel helpers: copied into SettingsSections from Composer/EffortPopover.
- Tooltip-on-disabled-button wrappers in Project, Projects, Settings and Account. Fix this once in `components/Tooltip` instead.

**Next phase must know:**
- Outside `src`:
  - Add `@lezer/highlight` to package.json; the Editor imports it directly.
  - Main-window `trafficLightPosition` (x 18, centred in the 52 px bar) in `tauri.macos.conf.json`.
  - Minimum size 960×600 (Swift) vs 900×600.
  - File-open and single-instance hooks for folders dropped on the Dock icon or passed as arguments.
  - `src/selftest.ts` for `--autotest`.
  - A shell command to write a file at a chosen path (FullOutputSheet Save…, Export Chat as Markdown).
  - A `tccutil` / permission-prompt command for Computer Use.
- The main chunk is about 1.6 MB minified (Vite warns). Only the CodeMirror language packs are split. `React.lazy` for Editor, Settings, Account and Apps is an option.
- Confirmation dialogs everywhere are foundation `Sheet`s with right-aligned buttons, not macOS stacked alerts. Exit animations (list rows, popovers, sheets, page transitions) are mostly missing. These are the largest remaining visual differences.

### Shell additions after the view phase

- `fs_save_text({ title, default_name, content, extension? })` → saved path or
  `null` (native save dialog, then writes the text; the only command that writes
  to a user-chosen path). Use it for Export Chat as Markdown… and the full-output
  sheet's Save…. The dev bridge saves into `<data dir>/Saved` without a dialog.
- Browser preview for visual checks: `npm run web` (dev bridge on
  `BRIDGE_PORT`, default 1422, plus Vite on `VITE_PORT`, default 1420) and open
  `http://localhost:1420/?platform=windows` (or `macos`); the folder picker
  opens `BRIDGE_PROJECT` (default: a fresh throwaway demo repo, never this repository), file picks take
  `?files=a;b`. `node scripts/snap.mjs steps.json out-dir --url …` drives it in
  headless Chrome and writes screenshots (see the script's header for the step
  language).

## QA notes

Gate result: `npm run check`, `npm run build` and `cargo check` (src-tauri) pass. The smoke flow passes in light and dark, with Windows chrome, at 1180×780 and 960×600, with no console errors, page errors or failed requests: Welcome → Open a project… → empty chat → send → two tool rows, the reply and the Completed outcome → Changes → Project → Editor (via the Files tab) → the Inspector's Run tab → Settings → Apps → the command palette. The macOS chrome (`?platform=macos`) was checked up to Project. The View notes' "Duplicates to unify next" are done, and so are tooltips on disabled controls. Of "Next phase must know", only the file-open and single-instance hook is still open.

What still differs from the macOS app:
- **All screens**
  - Glyphs are lucide icons in place of SF Symbols. They are sized to match, but their strokes are lighter.
  - The UI font is Segoe UI on Windows, but the metrics were tuned for SF. The composer's 17 px line height may look tight.
  - Menus have no submenus, so ProjectRow's "Open Worktree" is a header plus one item per worktree. Menus only animate in, not out.
  - Confirmations are Sheets under the top bar with right-aligned buttons, not stacked NSAlerts.
  - SwiftUI springs, `contentTransition(.numericText)` and height changes are approximated with CSS and the Web Animations API.
- **Window and shell**
  - **Windows**: the caption buttons are drawn by the app, so hovering Maximize does not open the Windows 11 snap-layouts flyout. That needs `WM_NCHITTEST` → `HTMAXBUTTON` in Rust.
  - **macOS**: double-clicking the top bar always zooms the window, because `AppleActionOnDoubleClick` is not read.
  - **Both**: nothing handles a folder passed on the command line or dropped on the Dock or taskbar icon, and nothing keeps the app to a single instance.
  - Windows has no menu bar, so the palette has actions that Swift's does not: Export chat as Markdown…, Go to Projects, Toggle sidebar and Clear recent projects.
- **Chat**
  - A file name can stay plain text for about 50 ms while its folder is being listed.
  - Absolute paths outside the project never become chips.
  - User image thumbnails load the full image.
  - Day separators do not refresh at midnight; Swift's do not either.
- **Cards**: the height and digit-roll animations approximate SwiftUI's. Find in the full-output sheet uses simple case folding.
- **Composer**
  - "+" picks files only, because the Tauri dialog cannot mix files and folders. Folders can still be dropped.
  - Files copied in Explorer or Finder cannot be pasted; only image and PDF blobs can.
  - Hover cards are clamped inside the window, and popovers have no arrow.
  - Alt+Esc is not mapped, because Windows reserves it.
  - Hover cards for PDF and Office files show only the meta line.
- **Changes**
  - The backend returns `refs/remotes/origin/HEAD` as a branch called `origin` (crates/cli/src/serve/gitops.rs `git_branches`). The view filters it out.
  - DiffView has no find bar, and browser find only reaches the rows that are rendered.
  - Diffs over 64 KB are parsed on the main thread.
  - The Push sheet's remote picker is a native `<select>`.
- **Editor**: CodeMirror takes its keymap from the host platform, so on a Mac with `?platform=windows`, Ctrl+K is kill-line. Otherwise the editor matches editor.png.
- **Welcome, Projects, Project, Inspector, Settings, Account, Apps**: this pass found no differences beyond the ones above.
- **Not covered by the browser preview**: Tauri file drop, the real window controls and traffic lights, the PTY terminal, the Browser tab's child webview, notifications and the taskbar badge.
