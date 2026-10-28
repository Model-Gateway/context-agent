# Context for macOS — architecture and build contract

A native SwiftUI desktop app for the Context coding agent. It reuses the
Rust runtime through one new CLI subcommand, `ctx serve`
([docs/desktop-protocol.md](../../docs/desktop-protocol.md), implemented in
`crates/cli/src/serve.rs`): the app launches it as a child process and
renders the same events the TUI consumes. All data (sessions, tasks, index,
memories) stays in `~/.context-agent`, so the TUI, `ctx run` and the app
share history.

## Toolchain and rules

- Xcode 26 / Swift 6.3 toolchain, **Swift language mode 5** (`SWIFT_VERSION = 5.0`,
  `SWIFT_STRICT_CONCURRENCY = minimal`), deployment target **macOS 26.0**.
- SwiftUI first; AppKit where SwiftUI lacks the control (`NSOpenPanel`,
  `NSSavePanel`, `NSPasteboard`, `NSWorkspace`, `NSTextView` for the composer).
- Third-party packages only where the platform has no equivalent (SwiftTerm,
  the tree-sitter stack, Sparkle). No network code in Swift: the backend does
  it. The one exception is Sparkle's updater ("Updates" under Build and run).
- `@Observable` (Observation framework) for state; all state objects are
  `@MainActor`. Backend I/O runs off the main actor and hops back with
  `Task { @MainActor in }` — never `DispatchQueue.main.sync`.
- No force unwraps outside tests; no `fatalError` in shipping paths; errors
  surface per the routing table below.
- Views are small: one primary `struct` per file plus private subviews.
  Styling goes through `Theme` and the modifiers in `Theme/Modifiers.swift`.
- Strings are plain English; no localization tables.
- Look at the screenshots in `docs/images/` before writing any view:
  `new-chat.png` (home / empty chat) and `hero.png` (tool timeline).

### @Observable rules (Swift 5 mode)

1. Never replace an instance stored in a dictionary of observables
   (`ProjectState.chats[id]`): `ProjectState.chat(for:)` is the only way to
   obtain a `ChatState`, and it always returns the same instance.
2. Every scene (`WindowGroup`, `Settings`) and every `#Preview` injects
   `.environment(appState)`; views read it with `@Environment(AppState.self)`.
3. Bindings into class properties use `@Bindable var chat = chat` inside
   `body` (`$chat.draft`).
4. `Preferences` is an `@MainActor @Observable final class` with plain
   `var`s and `didSet { UserDefaults.standard.set(...) }`; property wrappers
   (`@AppStorage`) are not allowed in observable classes. Caches are
   `@ObservationIgnored`.
5. Reads inside `.task {}` / closures register no dependencies; only reads
   during `body` do.
6. `ProjectState: Identifiable` (`id`), `ChatState: Identifiable`
   (`sessionId`) so `ForEach` over them works.
7. `@MainActor` classes are constructed only from the App struct or other
   main-actor code.
8. Mutate `chat.transcript` in place (`chat.transcript.update(assistantAt:)`,
   `chat.transcript.append(...)`); never copy the struct out, mutate and
   assign back (that copies the whole array).

### Scrolling performance rules

AppKit apps scroll tables of reused cells and text views that draw in one
pass; a SwiftUI stack of thousands of small views does not. Inside anything
that scrolls:

1. Monospace blocks (tool output, code fences, approval details) are one
   `LineList` = one `Text`, never one `Text` per line. Long diffs and files
   go to an `NSTextView` (`DiffView`, `CodeEditor`, the full-output sheet).
2. No nested `ScrollView` unless the block really exceeds its cap
   (`ToolOutputView.inlineLineLimit`); a nested scroll view steals the
   wheel from the transcript and adds an `NSScrollView` per card.
3. No per-row `.help()` that repeats what the row shows: tooltips are
   tracking rects AppKit re-registers on every scroll tick.
4. One hover handler per row; rows that are rebuilt by a parent on every
   event (`ToolRow`, `FileRow`) are `Equatable` and applied with
   `.equatable()`.
5. Anything parsed or highlighted from text goes through a `RenderCache`
   (`MarkdownView`, `MarkdownInline`, `LineList`), so a row scrolling back
   into view costs a dictionary lookup.
6. Judge smoothness on a Release build (`-configuration Release`): with
   `-Onone` SwiftUI's layout and attribute graph run several times slower
   than any shipping app the user compares against.
7. Long lists are windowed. With every row in a `VStack`, each streamed
   update re-rendered every row: at 30 runs (~180 items) a 9 s sub-agent
   run kept the main thread busy for over three minutes. The transcript
   and the Inspector's agent view load only a `TailWindow` of their items
   (Components/TailWindow.swift): the newest page, opened at the bottom,
   a page more as the user scrolls up, the far end unloaded past the
   limit and loaded again on the way back. Unloaded items stand as empty
   space at the measured average row height (`TailWindowSpacer`), so the
   scroller stands for the whole list and keeps its size as pages load;
   the scroller dragged into that space loads the items that belong there.
   The windowed rows are a plain `VStack` (exact heights; a `LazyVStack`
   anchors on estimated heights and the view jumps when items load), and
   `ScrollKeeper` holds the distance from the bottom while rows load or
   unload above the visible ones — SwiftUI's
   `defaultScrollAnchor(_:for: .sizeChanges)` is not applied during a live
   scroll. Rows measuring in at other than the average height still
   resize the knob, so `ScrollKeeper` swaps in a `SmoothScroller`
   (Components/SmoothScroller.swift) whose knob glides to its new size
   and place over a third of a second when the document's height changes;
   scrolling and knob drags are not smoothed. Loads and unloads have a gap between their thresholds, or a
   short page loads and unloads in a loop and the scroller pulses. Following the bottom stops as soon as the user
   scrolls (wheel, scroller drag, trackpad: `FollowState`) or jumps to a
   find match, and resumes only at the bottom of a window that reaches the
   newest item, reached by scrolling down or by a gesture that ends there
   — never on the way up (a gesture starting at the bottom spends its
   first frames within reach of it).
8. Rows read only what they draw, and only state that changes when they
   change: a committed row must not depend on anything a running task
   mutates per event (`ChatState.liveAgents`, `live`, `status`). Derive a
   stored, rarely-changing value instead (`ChatState.followedAgentId`).
   Environment values carrying closures are `Equatable` on their data
   (`FileReferenceContext`), or every rebuild of the parent redraws every
   message.
9. Measure before and after: `CTX_PERFBENCH=1` with `--autotest` runs
   `PerfBench` (see "Self-test mode") and writes `perf.json`.

## Directory layout and file ownership

```
crates/cli/src/serve.rs                       # R1  `ctx serve` JSON-RPC server (docs/desktop-protocol.md)
crates/cli/src/main.rs                        # R1  +`mod serve;`, `Command::Serve { mock }`, one match arm (surgical edits only)
crates/cli/Cargo.toml                         # R1  +`toml = { workspace = true }` appended to [dependencies]

apps/macos/
  ContextAgent.xcodeproj/project.pbxproj      # (written, verified)  objectVersion 77, one synced root group, Embed ctx backend phase
  scripts/embed-ctx.sh                        # (written)  cargo build + copy + codesign into Contents/Helpers/ctx
  ContextAgent/                               # synced folder == the app target (every file under it is compiled/bundled)
    App/
      ContextAgentApp.swift                   # S4  @main: signal(SIGPIPE, SIG_IGN); WindowGroup(MainWindow) .windowStyle(.hiddenTitleBar) .defaultSize(1180, 780); AppCommands (no Settings scene: ⌘, opens the profile page)
      AppCommands.swift                       # V1  menu commands / keyboard shortcuts (Commands) — the ONLY owner of ⌘⏎, ⌘⌫, Esc handling, Open Recent
      AppState.swift                          # S3  root @Observable state + all user intents + notification routing
      ProjectState.swift                      # S3  per-project state (sessions, chats, index, git, models, doctor)
      ChatState.swift                         # S3  per-session live state + TranscriptReducer + queue + log lines
      Toasts.swift                            # S3  Toast model + ToastCenter (dedupe 10 s, 5 s auto-dismiss, optional Details action)
      Notifications.swift                     # S3  TaskNotifier: UNUserNotificationCenter wrapper + dock badge
      LaunchOptions.swift                     # S4  parses --mock, --autotest DIR, --project PATH, CTX_DESKTOP_MOCK; exposes LaunchOptions.current
      UITestDriver.swift                      # S4  scripted self-test run + window screenshots (see "Self-test mode")
      GatewayState.swift                      # S3  ModelGateway account: GatewayState (@Observable) + AppState intents (device sign-in, sign-out, refresh, endpoint switch)
      Updater.swift                           #     Sparkle self-updates: Updater (@Observable, AppState.updater), Developer ID gate, relaunch waits for running tasks
    Backend/
      Protocol.swift                          # (written) DTOs, AgentEvent decoding, JSONValue, TaskRunOptions, BackendError
      JSONRPC.swift                           # S1  RPCConnection actor: framing, ids, pending continuations, notification dispatch
      BackendProcess.swift                    # S1  Process + pipes + strict-LF line reader + stderr ring buffer
      BackendClient.swift                     # S1  typed async API (see below) + State + stderrLog
      BackendLocator.swift                    # S1  find the ctx binary; returns (found, searched)
      BackendLaunch.swift                     #     how a child starts: `.local(binary)` or `.ssh(host)` → executable + arguments (`ssh -o BatchMode=yes … -- host 'env PATH=… ctx serve'`), POSIX quoting
      Environment.swift                       # S1  KeychainStore, LoginShellEnvironment (one zsh run, cached), BackendEnvironment.build
    Models/
      Transcript.swift                        # (written) TranscriptItem and friends, Transcript, TranscriptFormat
      RecentProject.swift                     # S3  persisted recents (Application Support JSON) + pinned; a recent carries `hostId`/`hostName` for a folder on a remote host, keyed by `RecentProject.key` (`ctx-ssh://<host>/path`)
      SSHHost.swift                           #     SSHHost (name, address, user, port, identity file, remote ctx path, extra ssh options), SSHHostStore (ssh-hosts.json), RemoteProjectRef (the `ctx-ssh://` key), SSHConfigHosts (~/.ssh/config Host aliases)
      Preferences.swift                       # S3  UserDefaults-backed settings (@Observable)
      ModelCatalog.swift                      # S3  models list cache, recents per base URL, fuzzy ranking, prefix grouping
      ToolMetadata.swift                      # S2  tool name → icon, label, category chip, verbs, path argument
      ComposerAttachment.swift                # V4  composer attachments, prompt references, hover-card preview loading
      AttachmentUpload.swift                  #     attachment → task/run images/files, pasted files, Application Support store
    Theme/
      Theme.swift                             # S2  color/typography/metric tokens (light + dark), Color.adaptive
      Modifiers.swift                         # S2  card, hoverHighlight, kbd, hairline, floatingShadow, selectableText
      Icons.swift                             # S2  every SF Symbol name used in the app + LogoView (✳)
    Components/
      BrailleSpinner.swift                    # S2  ⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ @ 80 ms; reduced-motion static ⠿; InlineDots
      ProgressRing.swift                      #     circular progress (opening / indexing a project); turning quarter arc until the total is known
      Chip.swift                              # S2  rounded label chips (neutral / tinted / badge)
      IconButton.swift                        # S2  icon buttons with hover, tooltip and accessibility label
      SearchField.swift                       # S2  rounded search field with kbd hint
      CopyButton.swift                        # S2  copy-to-pasteboard with "Copied" feedback
      StatusPill.swift                        # S2  completed/blocked/failed/cancelled/running pills (text + colour)
      DisclosureRow.swift                     # S2  chevron row used by timeline + thinking + notices
      EmptyPlaceholder.swift                  # S2  centered icon + title + subtitle (+ optional action)
      HalftoneHeader.swift                    # S2  dotted gradient decoration (accessibilityHidden)
      LineList.swift                          # S2  monospace lines as one cached Text (optional highlighter); caps rendering
    Markdown/
      MarkdownParser.swift                    # S2  block parser: headings, paragraphs, lists, quotes, fences, tables, hr
      MarkdownView.swift                      # S2  renders [MarkdownBlock]; inline via AttributedString(markdown:); links via openURL
      CodeBlockView.swift                     # S2  language label, copy button, horizontal scroll, highlighter
      SyntaxHighlighter.swift                 # S2  regex tokens: strings, comments, keywords, numbers, json keys
    Views/
      MainWindow.swift                        # V1  HStack: SidebarView | DetailView | InspectorView; toasts overlay; openURL handler
      Sidebar/SidebarView.swift               # V1  nav items, Projects section, Recents, account button
      Sidebar/SidebarRows.swift               # V1  NavRow, ProjectRow, WorktreeRow (nested under its parent; merge / remove confirms), SessionRow (hover, badges, context menus, inline rename, delete confirm)
      Sidebar/HostRows.swift                  #     the "Remote" section: HostRow per configured SSH host (status dot / spinner, click → folder picker, Connect / Reconnect / Disconnect / Remove) with its projects, opening rows and pinned remote recents nested under it
      Remote/RemoteFolderPicker.swift         #     the remote folder picker sheet (`AppState.remotePicker`): host picker, path field, parent / home, `fs/list` directory rows with a git glyph, Open
      Remote/RemoteHostsSettingsSection.swift #     Settings → Remote hosts: host rows with state and hint, Add host… (with ~/.ssh/config aliases), the editor sheet with Test connection
      Welcome/WelcomeView.swift               # V1  hero, Open a project…, recents grid, Connect-a-model card, backend failure panel, onDrop
      Palette/CommandPalette.swift            # V1  ⌘K: chats (title + task objectives) + actions, keyboard navigation
      Detail/DetailView.swift                 # V1  TopBar (tabs, search, approvals pill, project chip, approval chip, inspector toggle, settings) + tab content
      Inspector/InspectorView.swift           # V1  right pane: icon tab switcher + Run tab (backend card / failure panel, context stats, counters)
      Inspector/FilesTab.swift                # V3  Files tab: lazily expanded project tree with git status letters; click opens the Editor tab
      Editor/EditorView.swift                 # V3  Editor tab: open-file strip, banners (conflict, truncated), status bar (language, Ln/Col, symbol under the cursor, save)
      Editor/CodeEditor.swift                 # V3  NSTextView (TextKit 1) with line numbers, tree-sitter colouring (SyntaxHighlighter for languages without a grammar), auto-indent, ⌘-click to definition
      Editor/TreeSitterHighlighter.swift      # V3  Neon TreeSitterClient + TextSystemStyler; colours as layout-manager temporary foreground attributes so diagnostics' underlines survive
      Editor/TreeSitterGrammars.swift         # V3  grammar registry (14 grammars, Swift packages pinned by revision in the pbxproj); joins JS/TS query files; resolves injected languages
      Editor/CompletionPanel.swift            # V3  completion popup: language-server items, else index symbols + buffer definitions + keywords + buffer words, in a child NSPanel
      Editor/MediaViews.swift                 # V3  image (fit / 100% over a checkerboard), PDF (PDFKit) and binary-file views of the Editor tab
      Inspector/TerminalTab.swift             # V1  Terminal tab: shell tab strip + embedded SwiftTerm view
      Inspector/AgentsTab.swift               # V3  Agents tab: one tab per sub-agent of the chat + the selected agent's view
      Inspector/BrowserTab.swift              # Browser tab: toolbar (back/forward/reload, address, open externally) + the project's WKWebView; agent status line
      App/BrowserWorkspace.swift              # per-project WKWebView the user browses and the agent drives (`browser/request` → actions → `browser/respond`)
      Chat/ChatView.swift                     # V2  transcript (LazyVStack + live turn), day separators, auto-follow, find bar, action row, composer dock
      Chat/EmptyChatView.swift                # V2  "Where should we begin?" + composer + generated starter suggestions (`suggest/starters`) + index / connect-a-model card
      Chat/TranscriptItemViews.swift          # V2  UserMessageView, AssistantMessageView (hover toolbar), NoticeView
      Chat/ThinkingView.swift                 # V2  collapsible reasoning with spinner, auto-collapse
      Chat/ToolTimelineView.swift             # V3  group header, rows, connectors, expandable card, Stopped/Failed
      Chat/AgentTimelineView.swift            # V3  sub-agents (Ultra effort): header, one row per agent; a row opens the agent in the Inspector Agents tab
      Chat/ToolOutputView.swift               # V3  Arguments | Output segments, format label, copy, caps, "Open full output" sheet
      Chat/ApprovalCardView.swift             # V3  pending approval with Deny / Approve for session / Approve; resolved state
      Chat/OutcomeCardView.swift              # V3  status, summary, file chips with context menu, unresolved, usage footer
      Chat/RewindSheet.swift                  # RewindRequest (edit a sent message / undo a turn) + the keep-or-revert sheet
      Composer/ComposerView.swift             # V4  ComposerTextView (NSTextView), +, permissions chip, effort menu, model chip, send/stop, queue row
      Composer/AttachmentViews.swift          # V4  attachment chips, hover preview card, drop highlight
      Composer/ModelPickerPopover.swift       # V4  searchable list, recents, groups, keyboard nav, custom name, states
      Composer/PermissionsPopover.swift       # V4  permission profiles with icons + descriptions, keyboard nav
      Composer/StatusBarView.swift            # V4  spinner + state + step + elapsed + cancel; tokens/ctx/cost/index
      Changes/ChangesView.swift               # V5  branch/sync row, commit box, Staged Changes + Changes sections with row actions, recent commits, diff pane
      Changes/GitSheets.swift                 # PushSheet (remote, branch, upstream, force-with-lease, output), NewBranchSheet, CommitFileSheet
      App/GitActions.swift                    # AppState extension: stage/unstage/discard, commit (box, file, amend, undo), push/pull/fetch, branches, log, commit message generation
      App/WorktreeActions.swift               # AppState extension: parallel sessions through `worktree/*` (new worktree chat, send the draft in a new worktree, open / remove / merge a worktree)
      App/RemoteHosts.swift                   # RemoteConnection (one `ssh <host> ctx serve` per host) + AppState extension: connect / disconnect / reconnect, open a project on a host, remembered remote projects at launch, host CRUD, Test connection (see "Remote hosts (SSH)")
      Changes/DiffView.swift                  # V5  unified diff in a read-only NSTextView (gutter ruler, full-width +/- tints, hunk collapse)
      Project/ProjectView.swift               # V5  index/git/validation/languages/doctor/memories/tasks cards
      Settings/SettingsView.swift             # V5  ProfileView (the account button's page) tabs: Account, General, Model, BYOK, Spending, Billing, Permissions, Appearance
      Settings/SettingsSections.swift         # V5  the individual forms
      Composer/MentionMenu.swift              # V6  `@` mentions: MentionBridge (NSTextView ↔ SwiftUI: @word under the cursor, keys, insert, tint) + the menu above the composer (Apps from mcp/status, Files from files/search)
      Apps/AppsView.swift                     # V6  Apps: marketplace (search, prompt banner, two-column catalog) + per-app page (status, Connect/Disconnect, tool permission toggles, Computer Use permissions) + connect page (logos, credentials, "What Context can access" checkboxes → exclude_tools, Cancel/Approve)
    Resources/Assets.xcassets/Logos/          # brand marks for catalog apps (logo-github, logo-mcp, logo-upstash, logo-playwright; SVG, template where single-colour)
    Resources/Assets.xcassets/Banners/        # banner-marketplace (warm) and banner-app (teal): the photographic backgrounds of the Apps prompt banners
    Resources/Assets.xcassets/                # (written) AccentColor, empty AppIcon
```

R1 is the Rust agent; S1–S4 are phase-1 Swift agents (foundation); V1–V5
phase-2 agents (views). An agent touches only its files. Shared needs go
through the contracts below; a view agent that needs a helper that does not
exist adds a `private` one in its own file.

**Phase-1 compile gate.** S4 writes `ContextAgentApp.swift` plus compiling
placeholder files `Views/MainWindow.swift`, `App/AppCommands.swift`,
`Views/Settings/SettingsView.swift` (a `Text("…")` body each) so the Xcode
project builds at the end of phase 1; V1/V5 later **replace those three
files wholesale** (the one allowed exception to ownership).

## Design system (from the screenshots)

Light, airy, editorial. White surfaces on an off-white canvas, 1 px hairline
borders, 12–16 px radii, generous whitespace, SF Pro text, muted gray
secondary text, a single blue accent used sparingly (send button, selection,
links). Dark mode mirrors it with graphite surfaces.

Tokens (`Theme.swift`, `static let`, each a `Color` built from light/dark
pairs via `Color.adaptive(light:dark:)`):

| token | light | dark | use |
|---|---|---|---|
| `canvas` | `#FFFFFF` | `#1E1E20` | window / content background (= `surface`) |
| `surface` | `#FFFFFF` | `#1E1E20` | cards, composer, popovers |
| `surfaceRaised` | `#F2F2F4` | `#2B2B2E` | hover, selected rows, chips |
| `sidebar` | `#FFFFFF` | `#1E1E20` | sidebar background (= `surface`); panes and cards are separated by hairlines and shadows, not tints |
| `border` | `#E7E7EA` | `#343438` | hairlines |
| `borderStrong` | `#D8D8DC` | `#45454A` | focused inputs |
| `textPrimary` | `#1A1A1C` | `#F3F3F5` | |
| `textSecondary` | `#6B6F76` | `#A3A6AD` | |
| `textTertiary` | `#9A9EA6` | `#6F7278` | placeholders, timestamps |
| `accent` | `#2F7BF6` | `#5B9BFF` | send, links, selection ring |
| `accentSoft` | `#DCEBFD` | `#1E3A5F` | send button background |
| `success` | `#1A9E57` | `#3DD68C` | |
| `warning` | `#D98806` | `#F5B544` | |
| `danger` | `#D93A3A` | `#FF6B6B` | |
| `codeBackground` | `#F6F6F7` | `#1E1E21` | code blocks, tool output |
| `codeString` | `#1B8A3D` | `#7EE2A8` | strings (green like the screenshot) |
| `codeKeyword` | `#6F42C1` | `#C3A6FF` | |
| `codeNumber` | `#B45309` | `#F0B35E` | |
| `codeComment` | `#8A8F98` | `#7A7F88` | |
| `badgeBackground` / `badgeText` | `#FDE7EC` / `#E0245E` | `#4A1F2B` / `#FF7A9A` | attention badges |

Typography (`Theme.Font`): `title` 22 semibold, `heading` 15 semibold,
`body` 14 regular, `secondary` 13, `caption` 12, `mono` 12.5 monospaced,
`sidebar` 13.5. Metrics (`Theme.Metrics`): `sidebarWidth` 248,
`inspectorWidth` 320, `contentMaxWidth` 760, `radiusSmall` 8, `radius` 12,
`radiusLarge` 16, spacing 4/8/12/16/24/32. `floatingShadow()` = soft
`.black.opacity(0.06)` radius 10 y 4 for composer and popovers.

Logo: the ✳ asterisk glyph (`Icons.LogoView`), 22 pt, `textPrimary`.

Loading: every async wait shows `BrailleSpinner` (never a system
`ProgressView`): sidebar rows while running, model picker,
approval/cancel in flight, settings probe, doctor. With
`accessibilityReduceMotion` the spinner is a static ⠿ labelled "Working".
A project opening and building its index shows `ProgressRing` instead
(recent cards, the empty chat's index card): an accent arc filled to
`index/progress` done/total, a turning quarter arc until the total is
known (still with Reduce Motion).

Accessibility: every `IconButton` has a non-empty label (tooltip +
`accessibilityLabel`); status pills and failed tool rows carry the status
word in text, not only colour; decorative views are `accessibilityHidden`.

All transcript text, code and tool output is selectable
(`.textSelection(.enabled)` via `.selectableText()`).

### Window layout

`MainWindow` is a `VStack(spacing: 0)`: the window-wide `TopBar` (52 pt),
then an `HStack(spacing: 0)` of `SidebarView` (248 pt, `sidebar` bg,
trailing hairline, hidden with ⌃⌘S, `appState.sidebarVisible`, animated
0.2 s) · `DetailView` · `InspectorView` (320 pt default, leading hairline,
`appState.inspectorVisible`, ⌘⇧I; dragging its leading edge resizes it
260–720 pt, never leaving the centre pane under 420 pt, double-click
restores 320; the width persists as `Preferences.inspectorWidth`). The window uses `.hiddenTitleBar`; the
traffic lights sit over the top bar's 90 pt leading inset, moved by
`TrafficLights.align` to x 18–72 (above the sidebar's icon column) and
centred 25 pt from the top (AppKit
centres them in its 32 pt title strip; the move is re-applied after resize,
full screen and focus changes). `NSWindow.setFrameAutosaveName("Main")`.
Files and folders dropped on the Welcome view (`.onDrop(of: [.fileURL])`)
open as projects (files open their parent directory). Other views do not
accept drops to open projects; composer drops remain attachments.

### Screens

**Account** (`Views/Account/AccountView.swift`, `UsageHeatmap.swift`,
`UsageBarChart.swift`;
state in `App/GatewayState.swift`, wire shapes and usage maths in
`Models/ModelGateway.swift`): the optional "Sign in with ModelGateway"
(`https://www.modelgateway.co`, a `new-api` fork; `docs/desktop-protocol.md`
"ModelGateway"). The sidebar's bottom-left button
reads "Sign in with ModelGateway · Optional" when signed out — clicking it
opens the profile page *and*, when it lands on the Account tab, starts the device flow: `gateway/device/start`,
the browser opens `verification_uri_complete`, the page shows the
`XXXX-XXXX` code (Copy, Open again, Cancel) and `gateway/device/poll` runs
every `interval` s until approved / denied / expired. On approval the
gateway creates a *hidden* device key (not listed under the user's API Keys)
which `ctx serve` stores in its credential store (`credentials.toml`, entry
`modelgateway`); the app never sees it. The account summary is remembered in
`Preferences.gatewayAccount`, and (setting "Route model requests through
ModelGateway", on by default) `config/patch` points `model.base_url` at
`<origin>/v1`, `model.credential` at `modelgateway` and
`model.project_header` at `X-ModelGateway-Project` (setting "Attribute usage
to projects", on by default); idle projects switch provider at once, no
restart. The previous `base_url` is kept in
`Preferences.gatewayPreviousEndpoint`. Signed in, the button shows the avatar initial, the name
and "ModelGateway · $X left", and the page shows, top to bottom: the account
header (name, @username · email, group chip, Balance / Spent / Requests /
This device, Refresh, Open dashboard), **Context credit** (from
`gateway/account`'s `context_credit`: credit only this device key's
requests spend, before any plan or the balance; unclaimed, it offers the
$10 welcome credit with a Claim button that `gateway/context_credit/claim`
enables once the account has a card or crypto top-up, plus Add funds…
switching to the Billing tab; claimed, what is left with a bar; hidden once
spent or when the gateway's reply has no `context_credit`), **Plan** (each active
subscription from `gateway/account`'s `subscriptions`: title, price per
period, days left, a quota bar with the next reset, and the billing
preference; none shows "Pay as you go" with View plans… opening
`<origin>/wallet`; hidden when the gateway's reply has no
`subscriptions`), **Usage** (segmented 24h · 7d · 30d · All in the header,
then Total · By model · By project and Cost · Tokens · Requests; 24h / 7d /
30d draw `UsageBarChart`, one bar per hour (24h) or day (7d, 30d) ending
with the current one, hour or date labels underneath, hover shows the
bucket's figures; All draws `UsageHeatmap`, a GitHub-style
53 × 7 grid of 10 pt squares in four `accent` steps, month labels above,
Mon/Wed/Fri at the left, hover shows the day's figures in the caption; by
model / by project list the series biggest first with a share chip, six at
a time, "Unattributed" for rows without a project), **Settings** (the two
toggles) and **Sign out**
(`gateway/signout` revokes the device key and drops the stored credential;
the previous endpoint is restored). `gateway/usage` fetches 371 days of hourly rows;
`UsageBreakdown.byRange` folds them into local hours / days for each
`UsageRange` off the main actor, so switching the period is instant. The gateway
origin is fixed to `https://www.modelgateway.co`. Everything works without
signing in.

**Sidebar**: rows 30 pt, 13.5 pt text, 16 pt SF Symbols in `textSecondary`,
8 pt radius `surfaceRaised` on hover/selection. Items: *Welcome*
(hand.wave), *New chat* (plus), *Projects* (folder.badge.plus → `chooseProject()`),
(Changes and Project are top-bar tabs only). Section header "Projects" (12 pt, `textTertiary`) then one row per
open project (folder icon, name, branch chip; a chevron after them when
worktrees are nested below, folding them away (`Preferences.collapsedProjects`,
also "Hide/Show Worktrees" in the context menu; a selected worktree stays
visible and hidden ones roll their spinner and unseen count into the project
row); spinner when it is running and not selected; a count chip of its unseen chats; close × on hover; context
menu Pin/Unpin, Reveal in Finder, Close…; the × and Close… confirm first,
naming the running task that closing cancels); pinned recents that are not open
appear muted below (click opens). Section "Recents": the selected project's
sessions (title truncated; relative time on hover; spinner while running; a
7 pt dot: `warning` (orange) while `chat.needsHelp` — a pending approval or
`ask_user` question, or an unseen blocked finish — shown even while the run
is going, in place of the spinner; otherwise, when `unseen != nil`, `accent`
(blue) finished and not opened yet, `danger` failed or error; context menu Rename (inline `TextField`, Return commits via
`session/rename`, Esc cancels, double-click also starts it) / Delete
(`confirmationDialog` "Delete “<title>”? Its tasks and history are removed."
destructive; disabled with tooltip while that session runs; deleting the
selected chat calls `newChat()`)). Bottom card "Backend": green dot + "ctx
0.1.0 · connected", the user-level default model labelled "default" and the
selected project's approval mode; click opens Settings; a Retry button and
the failure summary when `connection` is `.failed`.

**Backend failure panel** (Welcome + inspector Backend card, `BackendFailure`): title
"ctx not found" / "ctx is too old" / "Backend stopped", the searched paths
or found path, the stderr tail, buttons *Choose binary…* (`NSOpenPanel` →
`preferences.backendPathOverride`), *Copy build command*
(`cargo build --release -p ctx-cli`), *Retry* (`restartBackend()`). Welcome
stays usable (recents visible; "Open a project" disabled with the reason).

**Top bar** (52 pt, full window width above all three panes, `canvas`,
hairline below): segmented tabs `Chat · Changes · Project` (pill style;
selected = `surface` + hairline + shadow); `SearchField` "Search chats…  ⌘K"
(opens the palette) centred in the window by `CenteredBarLayout`, sliding
aside only when a neighbour would overlap;
right: a `warning` pill "1 approval waiting · “<chat>”" when
`appState.pendingApprovals` is non-empty (click selects that project +
chat), project name + branch chip, approval-mode chip, inspector toggle
(sidebar.right), settings gear (`AppState.openSettings`: the profile page on the last settings tab). The model is picked from
the composer's chip (⌘⇧M and the palette switch to the Chat tab and open
it there). When space runs out the search field narrows, then the project
chip drops its name and goes, then the tabs and approval chip go icon-only;
the field becomes a magnifier icon last.

**Welcome**: hero "Context" + one-line pitch, big "Open a project…"
button, recents grid (name, path, last opened; hover Remove; pinned first;
context menu Pin/Unpin; a folder opened for the first time gets its card
at once, and while a project opens and indexes a `ProgressRing` takes the
folder's place with "Opening…" / "Indexing 42%" in the footer), the
*Connect a model* card when `needsAPIKey`
(below), backend status footer, drop target.

**Connect a model card** (Welcome and EmptyChat when `appState.needsAPIKey`):
base URL + model name (read-only, from `config.model`), a `SecureField`
labelled with `config.model.apiKeyEnv`, *Save* → `KeychainStore.set` then
`restartBackend()` (disabled with "Finishes after the running task" while any
project is running), link "Open Settings → Model".

**Empty chat** (`EmptyChatView`, shown while the draft session has no
transcript): `HalftoneHeader`, then a vertically centered column max 760 pt:
✳, "Where should we begin?" (22 semibold), the composer, three starter cards
(`surface`, hairline, 14 radius, icon + title + subtitle; they are
focusable `Button`s that insert the prompt and focus the composer): *Fix*
"Describe a bug or failing test" → `"Fix the failing test: "`, *Explain*
"Ask how something works" → `"Explain how "`, *Refactor* "Multi-file changes
with validation" → `"Refactor … and run <first validation command, else 'the tests'> to verify"`.
Below: the index card — "Index ready · 310 files · generation 12" (hover:
Rebuild), "Indexing 120/310" with spinner, or "Indexing failed: <error> ·
retrieval falls back to text tools" with *Rebuild*; replaced by the Connect
a model card when `needsAPIKey`.

**Chat** (`ChatView`): `ScrollView { VStack(spacing: 20) { ForEach(items[window]) { TranscriptRow(item:) } ; LiveTurnView(chat) } }.tailWindow(...)`
(committed items in a `TailWindow` of pages of 20, at most 60; a find match
outside it loads a page around it, "Jump to latest" the newest page), `contentMaxWidth`; centred day separators
("Today / Yesterday / Oct 12") when consecutive items differ in calendar day;
auto-follow while streaming unless the user scrolled up ("↓ Jump to latest"
pill); ⌘F find bar (count, ↑↓, highlight); a one-line notice "This chat was
started with <session.model>; new messages use <project.model>" when they
differ (once per selection); an **action row** under the last item when
`status` is `.finished(.failed|.cancelled|.blocked)` or `lastError != nil`:
*Retry* (re-sends `transcript.lastUserText`), *Continue* for blocked (sends
"Continue with the unresolved items"), *Open Settings → Model* when the
error kind is `provider_auth`; composer docked at the bottom in a floating
`surface` card; status bar line above the composer; queue row above the
status bar when `queued` is non-empty.

- **User message**: right-aligned `surfaceRaised` bubble (max 70 % width,
  16 radius, 14 pt), hover timestamp and a pencil `IconButton` (Edit
  message, shown when `turn.isRewindable` and `appState.canRewind(chat)`),
  context menu Copy / Resend / Edit message… / Undo from this message….
  Runtime messages (`isRuntime`) render as a muted notice instead. Editing
  (`chat.editingTurnId == turn.id`) swaps the bubble for a `TextEditor`
  (accent border, same width) with the caption "Sending removes this
  message's reply and everything after it.", *Cancel* and a prominent
  *Send*; Esc cancels (AppCommands' monitor), ⌘⏎ submits
  (`primaryAction` → `submitEdit`).
- **Rewind sheet** (`RewindSheet`, `.sheet(item: $appState.rewindRequest)`
  on `ChatView`): "Edit this message?" / "Undo this turn?" with the quoted
  message, a `surfaceRaised` card saying what goes ("This message, its
  reply and N later messages are removed.", "N files changed since then:"
  + up to 8 mono paths, "+ N more"), a hint line, then *Cancel* (Esc),
  *Keep files* and *Revert files* (default when `turn.hasSnapshot`; without
  a snapshot only *Keep files*, the hint turns `warning`). Both call
  `performRewind` (`session/rewind`, reload history, refresh git, then
  re-send the edit or put the undone text back into the composer).
- **Assistant message**: left-aligned, no bubble, markdown; hover toolbar
  (top-right) with `CopyButton` (raw markdown) and timestamp; context menu
  Copy as Markdown / Copy as plain text; while streaming a Braille spinner
  trails the text; a collapsed **Thinking** row above (`DisclosureRow`
  "Thinking ⠋ 1.2s" → muted 13 pt text) auto-collapses on the first text
  delta. A finished message of a run routed through ModelGateway has a
  **Report** row under it (flag + caption, shown on hover, its height kept
  so nothing moves): a popover with a reason (`ResponseReportReason`) and
  an optional note sends `gateway/report` for the turn's `taskId` + `step`;
  the gateway counts it against the channel that served it. Reported
  messages read *Reported* for the rest of the session.
- **Tool timeline** (reference-tools.png): header "✳ 18 tools, edited 5
  files, ran 2 commands ⌄" with elapsed time at the right
  (`TimelineView(.periodic(by: 1))` while open); rows: 24 pt rounded square
  icon (`surfaceRaised`), label (verb + target: "Reading src/app.rs",
  "Running `cargo test`"), right chip (category); a 1 px connector links the
  icons; a row in flight shows the spinner in its icon slot; failed rows tint
  the icon `danger` and say "failed" in the summary. Clicking a row toggles
  its card (`ToolOutputView`): `codeBackground`, 14 radius, two segments top
  left — *Arguments* (pretty-printed `JSONValue`, JSON highlighter, label
  "JSON"; `apply_patch` edits render `old`/`new` as a mini diff) and *Output*
  (label JSON / stdout / diff / text) — default Output when present,
  Arguments while running; `CopyButton` top right; monospace `LineList`
  capped at 400 lines / 32 KB with footer "+ N more lines · Open full
  output" (sheet with a read-only `NSTextView` and Save…); max 320 pt tall
  then scroll; "+ 12,480 more characters" when the backend truncated. Rows
  and chips whose target is a path get a context menu: Show in Changes,
  Reveal in Finder, Open with Default Editor, Copy Path
  (`appState.reveal(path:in:)`). Last row "Stopped" / "Failed" /
  "Blocked" when the group ended that way; a normally finished group has no
  ending row.
- **Approval card**: `warning` hairline, tool name + risk chip, summary,
  details in a code block (max 240 pt), reason in `textSecondary`; buttons
  right-aligned: *Deny* (plain, ⌘⌫), *Approve for session* (bordered),
  *Approve* (accent, ⌘⏎ when this chat is selected). Esc never answers an
  approval. Resolved state collapses to "Approved · run: cargo test".
  Respond failures render inline on the card.
- **Validation**: no row of its own; a `run_validation` call is an ordinary
  step in the tool timeline.
- **Outcome card**: status pill, summary (markdown), "Modified files" chips
  (click → Editor tab; ⌘-click opens it in the default editor; path
  context menu), "Unresolved" bullets
  in `warning`, footer "12 steps · 6 model calls · 18 tool calls · 42.1k
  tokens · $0.12 · 6m 12s" (cost omitted when nil; hover shows the absolute
  finish time).
- **Agents** (`AgentTimelineView`): the Ultra tier's sub-agents as one card
  per `spawn_agents` call — header "◈ 3 agents, 2 running ⌄" with the
  elapsed time, one row per agent (spinner / check / x icon, name, mode chip
  Explore / Implement, second line with the current tool or verdict, time),
  joined by the same connector as the tool timeline. Clicking a row opens
  the agent in the Inspector Agents tab (`AppState.showAgent`, which sets
  `ChatState.inspectedAgentId`); the inspected row stays highlighted. Searchable like tool groups; exported to markdown as a
  `<details>` block. Agents deployed with `background: true` keep working
  while the lead does; `send_input` to an agent that has reported starts a
  follow-up turn, announced by a new `agent_started` with the same agent
  id, so it shows as a new row whose objective is the follow-up message.
  Agents outlive the lead's turn (only their running turns stop with it),
  so that row can appear in a later turn's card.
  `wait_agents`, `read_agent` and `close_agent` are ordinary tool rows.
- **Notice**: 12.5 pt `textTertiary`; consecutive notices coalesce; noisy
  lines (task boundary, checkpoints) sit behind a disclosure; error notices
  use `danger`.

**Composer**: `surface`, hairline, 16 radius, shadow; `ComposerTextView`
(NSTextView wrapper, below) grows 1→8 lines, placeholder "Ask anything…";
attachment chips above the text (`AttachmentRow`, `Composer/AttachmentViews.swift`):
files and folders dropped anywhere on the card (the card's `.onDrop` wins
over the window's open-as-project drop; the text view's `FileDropTextView`
takes file drags itself instead of pasting paths) or picked from the + menu
become `ComposerAttachment`s (`ChatState.attachments`,
`AppState.pendingAttachments` for the empty state); each chip shows the
Finder icon (image thumbnail for images), the name and ×, click reveals it
in Finder, and a 350 ms hover floats `AttachmentHoverCard` under it through
`HoverCardPanel` (`.below`): name, "PDF · 2.3 MB · 12 PAGES" meta line and
the start of the content (text lines, PDF page text, image, folder entries)
fading out; ⌘V attaches too (`FileDropTextView.paste`): files copied in
Finder, and an image or PDF on the pasteboard with no text (a screenshot,
"Copy Image", pages copied from Preview) saved by `PastedFiles` under
`~/Library/Application Support/ContextAgent/Pasted Images` (chip "Pasted
image" / "Pasted document"; files older than 30 days are pruned). On send
they are appended to the prompt as an "Attached files:" list of
repository-relative (else absolute) paths (pasted files excepted), and
`AttachmentUpload.prompt` adds content the model can read to `task/run`:
images as `images` (PNG/JPEG/GIF/WebP up to 4 MB and 2048 px as they are,
anything else macOS decodes as a downscaled copy), PDFs and Word/RTF/ODT
documents as their extracted text in `files` (named after the original; a
PDF without a text layer as images of its first 5 pages), other UTF-8 text
files outside the project as `files`; folders, in-project text files and
other binaries stay references only. A warning line shows over the chips
when models.dev says the current model takes no images. Queued prompts and
`UserTurn`s keep their `images`/`files` (Retry, Resend, edits and undo
carry them; edits put pasted ones back as chips), and the user bubble
shows image thumbnails;
bottom row: `+` (menu: Attach files… (`NSOpenPanel` → chips), Clear), the **permissions** chip "✋ Ask ⌄"
(`PermissionsPopover`: Ask · Auto · Bypass — the TUI's `/approval` names,
mapped to `approval_required` / `workspace_autonomous` /
`bypass_permissions`, each with an icon and description, sent with
`permissions/set`, current one checked; Bypass is tinted `warning`; the
former Read-only and Sandbox tiers no longer exist), spacer, the
**reasoning effort** menu "Auto / the model's own effort values from
models.dev (`ModelsDevInfo.effortLevels`; Low / Medium / High when it does
not list the model) / Ultra (deploys agents)" (`config/patch` `model.reasoning_effort`, all projects; the chip
shows the agents glyph in `accent` while Ultra is on), model
chip "✳ gpt-4.1 ⌄" (project model), send
button (32 pt circle, `accentSoft` bg, `accent` arrow.up; a stop square
while this chat runs; a clock while the project is busy with another chat).
After a run completes (nothing queued, Settings → General → "Suggest a
next message" on) the app calls `suggest/next`; while the composer is empty
the answer is shown as grey ghost text with a `→` hint, and → inserts it as
real (undoable) text. Typing, sending or a new run clears it.
Return sends; Shift-Return newline; ⌘. cancels. While this chat runs,
Return **steers** the running turn (`task/steer`; `ChatState.steering`, row
"Steering · N" until the `steered` event moves the message into the
transcript) and ⌘Return **queues** the text for after the run
(`ChatState.queued`, row "Queued · N" above the status bar with × per item,
click to edit). A steer the run refuses (it is ending) is queued first; one
the run never read (cancelled, failed) goes to the front of the queue on
`task/finished`. The composer is focused on new chat, after send, after selecting a
session and after any popover closes (`appState.composerFocusToken`).

```swift
struct ComposerTextView: NSViewRepresentable {           // V4, in ComposerView.swift
    @Binding var text: String
    @Binding var measuredHeight: CGFloat
    var focusToken: Int
    var onSend: () -> Void
    // makeNSView: NSTextView.scrollableTextView(); drawsBackground = false; isRichText = false;
    // allowsUndo = true; font 14; textContainerInset (0, 6); no quote/dash substitution;
    // Coordinator: NSTextViewDelegate — textDidChange → text + measure();
    // doCommandBy insertNewline: if hasMarkedText() return false (IME commits);
    //   Shift held (NSApp.currentEvent?.modifierFlags.contains(.shift)) → insertNewlineIgnoringFieldEditor, return true;
    //   else onSend(), return true.
    // updateNSView: sync text when different; when focusToken changed → window?.makeFirstResponder(tv).
    // Parent frames height to min(max(measuredHeight, lineHeight + 12), lineHeight * 8 + 12).
}
```

**Model picker popover** (320 × 400): search field auto-focused, "Recent"
section (per base URL, promoted on successful switch only) then the list
(grouped by prefix before the first `-`/`/` when > 12 entries), current
marked "current", fuzzy match with highlighted substrings, ↑↓ + Return, a
trailing row "Use “<typed>” as the model name" when nothing matches; Braille
spinner while loading; on `models/list.error` the error line + *Refresh*
while still listing current + recents + typed row; choosing a model
closes the picker and switches in the background (the composer chip shows
"Switching…", send disabled); a failed switch, or a model the probe rules
out for agent runs, comes back as a toast; choosing the current model
again re-runs the switch (and so the probe) unless it last passed; each row has the lab's logo and
a models.dev hover card; switching while a task runs is allowed (the
running turn keeps its model, the next turn uses the new one; the server
clones the provider at run start). Return inside the picker/palette never
reaches the composer.

**Status bar**: running → spinner + `state.label` + "step 3" + elapsed +
*Cancel* (⌘.); always: "ctx 12.4k/128k" when `contextStats` exists (click
opens the Inspector Run tab; "compacted" badge), "42.1k tok", "$0.12" (only
with pricing; tooltip on the token segment otherwise), "index gen 12" /
"indexing 120/310"; idle also shows model · approval mode.

**Editor** (⌘4, `Views/Editor/`, state in `App/EditorState.swift`): the
project's open files (`AppState.editors[projectId]`, an `EditorWorkspace`
of `EditorDocument`s) as a chip strip over a `CodeEditor` — a TextKit 1
`NSTextView` with a line-number ruler, line-based colouring from
`SyntaxHighlighter` (re-run for the edited paragraphs only), the file's own
indentation on ⏎ / ⇥, undo, find bar — and a status bar (language, Ln/Col,
the symbol under the cursor from `symbols/find {mode: exact}` with its
signature and `path:line`, click or ⌘-click an identifier to open its
definition; "unsaved" + Save). Files open from the Inspector Files tab
(click), a click on chat file chips, and from "Open in Editor"
in every file context menu (`PathAction.openInEditor`), loaded with
`file/read` (its `hash` is kept). ⌘S / Save → `file/write` with
`expected_hash`; a `conflict` shows a banner with Reload · Overwrite; a
truncated read is read-only. Clean documents re-read themselves when the
project's git status changes (the agent edited the file); edited ones keep
their buffer. Closing an unsaved tab asks Save · Don't Save · Cancel (⌘⇧W
closes the selected one); a tab's context menu has Close · Close
Others · Close Tabs to the Right, asking once about every unsaved file it
would close. Auto save (Settings → General → Editor, or File →
Auto Save; `Preferences.editorAutoSave`) works like VS Code's `afterDelay`:
on, an edited document is saved a second after the last edit, turning it on
saves every edited one, and closing an edited tab saves instead of asking. A
conflicted document is never auto-saved. Images (`png jpg gif webp heic tiff bmp svg ico
icns`) and PDFs open in viewers straight from disk (`MediaViews.swift`:
fit / 100% over a checkerboard with format · pixels · bytes; PDFKit with
page count); a file `file/read` refuses as binary gets a placeholder with
"Open with Default App".

Completion ("type sense"): a borderless child `NSPanel` under the word.
With a language server for the file's language (`lsp/open` on load finds
one from `ctx_lsp::ServerCatalog`; the status bar shows "Rust ·
rust-analyzer"), the buffer is synced with `lsp/change` (200 ms debounce,
flushed before every request) and the popup lists `lsp/completion` items
after one identifier character or a trigger character (`.`, `::`), with the
server's detail and its replace range honoured; the status bar shows the
`lsp/hover` signature of the identifier under the caret, ⌘-click uses
`lsp/definition`, and `lsp/diagnostics` are drawn as dotted underlines
(error / warning / info colours, message as tooltip) with counts in the
status bar. Without a server the popup lists, after two characters, the
index's symbols whose name starts with the prefix (`symbols/find {mode:
prefix}`, same-file first, signature or kind and path), definitions found
in the buffer itself by their keyword (`fn` `def` `class` `struct` `let` …,
so unsaved ones count), the language's keywords and words already in the
buffer. ↑ ↓ move, ⏎ / ⇥ insert, Esc or moving the caret away dismisses.

**Inspector** (320 pt): tab *Run* — the Backend card (connection,
default model, approval mode; Retry / Choose binary… on failure), current/last `ContextStats`
(prompt estimate vs window, repository/memory/conversation tokens,
snippets, memories, compacted), counters (steps, model calls, tool calls),
task state; tab *Files* —
the selected project's tree (`ProjectState.fileTree`), read from disk folder
by folder as it expands (`.git` and `.DS_Store` hidden), changed files tinted
with their status letter and their folders dotted, reloaded on every git
status change; a click opens a file in the default editor, the context menu
has Show in Changes · Open · Reveal in Finder · Copy Path; tab *Terminal* — the selected project's `TerminalWorkspace`
(App/TerminalSession.swift): a horizontally scrolling strip of shell tabs
(title from OSC 0/2, close on hover, "+" at the end of the list) over the
selected `TerminalSession`'s embedded SwiftTerm `LocalProcessTerminalView`
(SwiftPM, pinned below 1.12 because later releases need the Metal toolchain).
Each session is the login shell in the project root; views outlive tab
switches, a shell that exits closes its tab, and closing the project hangs
them all up; tab *Agents* — a strip with one tab per sub-agent of the
selected chat (`ChatState.allAgents`, status icon or spinner + name) over
the selected agent's view: header (status, name, mode chip, current step,
time), brief, its own `ToolTimelineView`, the line it is writing while it
runs, and its report (markdown, unresolved bullets, usage footer). With no
agent picked it follows the latest running, else latest, agent; tab
*Browser* (rightmost) — the selected project's `BrowserWorkspace`
(App/BrowserWorkspace.swift): a toolbar (back, forward, reload / stop,
address field that also searches, open in the default browser) with a thin
progress bar over a long-lived `WKWebView` that keeps its page across tab
switches and goes with the project when it closes. The agent's `browser`
tool (docs/desktop-protocol.md "The browser tool") drives the same page:
`AppState.handle(.browserRequest)` hands the action to the workspace, which
runs it (navigation waits for the load; `snapshot` numbers the visible
interactive elements with `data-ctx-ref`; `click`/`type`/`select`/`scroll`
act on those numbers; `evaluate` runs a function body with
`callAsyncJavaScript`; `screenshot` saves a PNG to the temporary folder;
JavaScript dialogs are dismissed and reported) and answers with
`browser/respond`. **The Inspector never switches to the Browser tab for
the agent**: while it works, the tab button shows an accent dot and the tab
itself a "Agent · Opening example.com…" line (kept 4 s after the action).
Pop-ups open in the same view. Actions of concurrent sessions run one after
another. The header switches tabs with
icon buttons whose selection card slides (`matchedGeometryEffect`); the tab
content slides with it. Toast "Details" open an alert with a Copy button.

**Changes tab** (VS Code-style source control): left pane (300 pt), right
`DiffView` (file header, `@@` lines in `textTertiary`, additions `success`,
deletions `danger`, mono line numbers; `LazyVStack` per line, hunks beyond
the first 20 collapsed with "Show hunk", banner when `truncated`).
The pane, top to bottom:
- header "Changes · N", a spinner with the action in flight
  (`project.gitBusy`), refresh;
- **branch row**: a borderless `Menu` "⎇ main" (local branches with a
  checkmark on the current one, remote-only branches, New Branch…, Refresh
  Branches; `git/branches` / `git/checkout`), the "↑ahead ↓behind" chip when
  the branch has an upstream, then Fetch, Pull (context menu: Pull (Rebase))
  and Push… (`PushSheet`) icon buttons; everything disabled while an action
  runs. When the row is narrow the branch name truncates, down to about ten
  characters, then the chip is dropped, so the buttons always fit;
- **commit box** (`CommitBox`): `TextEditor` bound to `project.commitMessage`
  with the placeholder "Message (⌘⏎ to commit on main)", ✨ Generate
  (`git/commit_message` → the box; a toast says when the message is the
  heuristic one), a prominent *Commit* (enabled with a message and any
  change; nothing staged → confirmation "Nothing is staged. Stage All and
  Commit") and a chevron menu: Commit, Commit All (Stage Everything), Commit
  (Amend Last), Undo Last Commit… (confirmation quoting `head_subject`).
  ⌘⏎ on the Changes tab with a message typed commits (`primaryAction`);
- **sections** `STAGED CHANGES · n` (hover row action −, header Unstage all)
  and `CHANGES · n` (hover row actions ↶ discard and +, header Discard all…
  / Stage all); a file with both kinds of change (`MM`) is listed in both
  and the diff follows the section (`selectChange(path:staged:in:)` sets
  `showStaged`; after an action `reconcileDiffMode` keeps a diff that still
  exists). Row context menu: Stage / Unstage / Discard Changes… / Commit This
  File… (`CommitFileSheet`: its own message, `git/commit` with `paths`),
  then Reveal in Finder / Open with Default Editor / Copy Path. Row counts
  "+12 −3" sit at the right edge and are replaced by the actions on hover.
  Discard always confirms (`confirmationDialog`, destructive);
- `RECENT COMMITS` (last 8 of `git/log`; context menu Copy Hash / Copy
  Subject / Undo Last Commit… on the HEAD row) with a Push… shortcut.
"Working tree clean" is a row inside the list so the controls stay; "Not a
git repository" (`git == nil`) and the status error placeholder remain.
Every action goes through `AppState.gitAction` (GitActions.swift): one
backend request under `project.gitBusy`, `refreshGit` afterwards (also on
failure), errors as toasts with Details, successes as short toasts
("Committed abc1234 · subject", "Pushed main to origin", "Discarded: …").
`PushSheet`: remote `Picker` (or a text field when the repository has no
remote), branch, "Set as upstream" (on when there is none), "Force with
lease" with a warning line, git's output or error inline, Push/Force Push
(⏎); closes on success. Auto-refresh on `file_changed` and `task/finished`.

**Project tab**: 2-column grid of cards: Index (stats, progress before the
first `index/ready`, Rebuild), Git, Validation commands ("None detected —
add [validation] commands to .context-agent.toml"), Languages (bars), Doctor
(Run / Run with probe → sections of label/value with ✓/✗), Memories
("Nothing remembered yet"), Recent tasks ("No tasks yet").

**Settings** (tabs on the profile page, `ProfileView`, next to Account;
there is no Settings window — ⌘,, the palette, the top-bar gear and every
"Open Settings" link call `AppState.openSettings`, which selects the
sidebar's account item and the remembered tab, `Preferences.settingsTab`;
below 720 pt of page width the tab column shows icons only, titles as
tooltips, and the Account tab's usage charts scroll sideways, kept on today):
General — backend binary (detected path,
Choose…, Restart backend), data dir, "Use the context engine" (default on;
off sends `proactive_retrieval: false` with every run), "Developer mode"
(default off; on sends `debug_prompt: true`, and the live `prompt_sent`
event becomes `UserTurn.sentPrompt`: a collapsed "Full prompt" row above the
run's message, `SentPromptView`; not in `session/history`), "Notify when a task
finishes" / "when approval is needed" (default on); Model — base URL, model name, API key env
var, API key (SecureField → Keychain; "Stored in Keychain" / "Not set";
Remove; saving/removing restarts the backend when idle), context window, max
output tokens, reasoning effort, *Probe endpoint* (doctor with probe,
results inline); Permissions — default profile (config/patch) with the five
descriptions, allowed/denied commands; Pricing — three numbers + Clear
("Costs are hidden until pricing is set"); Appearance — System / Light /
Dark.

**Command palette** (⌘K): floating 560 pt panel, search field, results
"Chats" (title + the project's task objectives/summaries from
`ProjectState.tasks`, matched snippet, relative time) and "Actions" (New
chat, Open project…, Switch model…, Rebuild index, Toggle inspector, Run
doctor, Settings…); ↑↓ Return Esc.

## State and data flow

```
BackendProcess ──lines──▶ RPCConnection ──notifications──▶ BackendClient.onNotification
                                                              │ (main actor)
                                     AppState.handle(_:) ─▶ ProjectState ─▶ ChatState.apply(_:)
```

### Declared types

```swift
enum SidebarItem: String { case welcome, chat, changes, project }               // AppState.swift
enum BackendFailure: Equatable { case notFound(searched: [String]), incompatible(found: String, detail: String), exited(code: Int32, tail: String), transport(String) }
enum ConnectionState: Equatable { case connecting, ready(InitializeResult), failed(BackendFailure) }
struct RunProgress: Equatable { var state: ExecutionState; var step: Int; var startedAt: Date }   // ChatState.swift
enum ChatStatus: Equatable { case idle, loadingHistory, running(RunProgress), finished(TaskStatus) }
enum Unseen: Equatable { case finished(TaskStatus), approval, error }
struct LogLine: Identifiable { let id: Int; let date: Date; let text: String; let level: String }
```

### `AppState` (App/AppState.swift) — `@MainActor @Observable final class`

```swift
var connection: ConnectionState
var config: InitializeResult?
var projects: [ProjectState]
var selectedProjectId: String?
var selectedSessionId: String?            // nil → EmptyChatView
var sidebarItem: SidebarItem
var sidebarVisible: Bool; var inspectorVisible: Bool; var inspectorTab: InspectorTab (.run, .files, .terminal, .agents, .browser); editors: [projectId: EditorWorkspace] (App/EditorState.swift); browsers: [projectId: BrowserWorkspace] (App/BrowserWorkspace.swift); var browserDriven: Bool  // the agent is acting in the selected project's browser
var paletteVisible: Bool; var modelPickerVisible: Bool
var composerFocusToken: Int               // bumped by newChat, send, select(session:), popover dismissal
var pendingDraft: [String: String]         // projectId → draft for the empty state
var needsAPIKey: Bool { (config?.model.apiKeySet == false) && config?.mock != true }
var pendingApprovals: [(chat: ChatState, request: ApprovalRequest)]   // across projects
var attentionCount: Int                    // unseen chats + pending approvals → dock badge
let preferences: Preferences; let toasts: ToastCenter; let backend: BackendClient
var selectedProject: ProjectState?; var selectedChat: ChatState?

func startBackend() async                  // locate → environment → spawn → initialize → protocol check → reopen projects
func restartBackend() async                // stop, startBackend, project/open for every open project, sessions/list for the selected one
func chooseProject() async                 // NSOpenPanel (directories only) → openProject(url:)
func openProject(url: URL) async
func closeProject(_ id: String) async
func select(project id: String)
func select(session id: String?) async     // loads history only when transcript.isEmpty && status == .idle; clears unseen
func newChat()                             // no project → sidebarItem = .welcome
func send(_ text: String) async            // creates the session if needed; queues when the project is busy
func retry(chat: ChatState) async
func cancelRunningTask() async
func primaryAction()                       // ⌘⏎: approve the selected chat's pending approval, else send the draft
func denyPendingApproval()                 // ⌘⌫
func respond(approval requestId: String, in chat: ChatState, decision: ApprovalDecision) async
func switchModel(_ name: String) async throws -> ModelSwitchResult
func refreshModels(force: Bool) async
func setApprovalMode(_ profile: String) async   // permissions/set; appends a .notice to the selected chat
func rebuildIndex() async
func renameSession(_ id: String, title: String) async; func deleteSession(_ id: String) async
var rewindRequest: RewindRequest?; var rewindInFlight: Bool      // the keep-or-revert sheet
func canRewind(_ chat: ChatState) -> Bool     // idle project, history loaded, backend ready
func beginEdit(turn: UserTurn, in chat: ChatState); func cancelEdit(in:); func submitEdit(in:)   // inline edit → rewindRequest(.edit)
func undoLastTurn(); func undo(from turn: UserTurn, in chat: ChatState)                         // ⌘⌥Z / context menu → rewindRequest(.undo)
func performRewind(_ request: RewindRequest, restoreFiles: Bool) async   // session/rewind → loadHistory → re-send / refill the draft
func patchConfig(_ values: [String: JSONValue]) async throws
func reveal(path: String, in project: ProjectState, action: PathAction)   // showInChanges, revealInFinder, openWithEditor, copyPath
func handle(_ n: BackendNotification)
```

`handle` rules: `.event` → the chat for (projectId, sessionId) via
`chat(for:)` (created if unknown, e.g. a run started by the TUI); `.taskFinished`
→ `chat.finish(f)`, mark `unseen`, notify, refresh git status, then dequeue
(if any chat of that project has a non-empty `queued`, send the first item
of the finished chat first, else of the earliest-queued chat — never after a
`provider_auth` error); `.indexProgress`/`.indexReady` → project; `.log` →
Inspector Log (+ a single toast when `level == "error"`); `.browserRequest`
→ the project's `BrowserWorkspace.perform` then `browser/respond` (never a
tab switch; a project that is not open answers `ok: false`); `.questionRequest`
→ `chat.pendingQuestion` (the `QuestionCardView` above the composer;
answered with `respond(question:in:answers:message:)` → `question/respond`,
*Skip* sends neither answers nor message, and text sent from the composer
while the question waits becomes its `message` instead of a steer; cleared by
`finish`); `.exited(code, tail)`
→ for every chat in `.running`: `finish(TaskFinished(status: .failed, error: "Backend exited (code N)"))`,
clear `pendingApproval`, close the open tools group with `.stopped`;
`connection = .failed(.exited(...))`; toast "Backend stopped" with Details;
unless the exit followed `stop()`, `restartBackend()` once after 1 s.

Unseen/attention: `ChatState.unseen` is set by `apply`/`finish` when
`appState.selectedChat !== self || !NSApp.isActive`, cleared by
`select(session:)` and on `didBecomeActiveNotification`; the dock badge is
`attentionCount`.

Error routing: user-initiated RPC failures (open project, rebuild,
rename…) → toast 5 s with Details; anything tied to a chat (`task/finished
.error`, model switch failure, approval respond) → inline in that chat /
picker; connection failures → sidebar card + Welcome panel; `log`
notifications → `backendLog` (+ one toast for errors). `ToastCenter` dedupes
identical messages within 10 s.

Notifications (`TaskNotifier`, guarded by `Bundle.main.bundleIdentifier != nil`):
request `.alert .sound .badge` lazily on the first finished task; post
"Task completed/failed/blocked · <chat title>" on `task/finished` and
"Approval needed: <summary>" on `approval_requested` when
`!NSApp.isActive || chat !== selectedChat`; `userInfo = {projectId, sessionId}`;
the delegate selects project + session and activates the app.

### `ProjectState` — `@MainActor @Observable final class`, `Identifiable`

```swift
let id: String; var info: ProjectInfo
var sessions: [SessionSummary]; var sessionsLoading: Bool
private var chats: [String: ChatState]; func chat(for sessionId: String) -> ChatState
var indexProgress: IndexProgress?; var lastIndexReady: IndexReady?
var gitStatus: GitStatus?; var gitError: String?; var selectedChangePath: String?; var diff: GitDiff?; var diffLoading: Bool
var runningSessionId: String?; var isRunning: Bool { runningSessionId != nil }
var models: ModelCatalog; var switchingModel: String?
var doctor: DoctorReport?; var doctorRunning: Bool
var memories: [MemoryEntry]; var tasks: [TaskSummary]
var lastTab: SidebarItem; var unseenCount: Int
```

### `ChatState` — `@MainActor @Observable final class`, `Identifiable` (`sessionId`)

```swift
let sessionId: String; let projectId: String
var summary: SessionSummary
var transcript: Transcript                    // committed items
var live: AssistantTurn?                      // the streaming assistant turn (not yet in transcript)
var liveTools: ToolGroup?                     // the open tool group of the current run
var status: ChatStatus
var usage: TokenUsage?; var costUsd: Double?; var contextStats: ContextStats?
var pendingApproval: ApprovalRequest?
var draft: String; var queued: [String]
var changedPaths: Set<String>
var unseen: Unseen?
var lastError: String?; var lastErrorKind: String?
var log: [LogLine]
var pendingUserItemId: String?               // the optimistic user item appended by send()
var editingTurnId: String?; var editingText: String   // the user item edited in place
func loadHistory(_ h: SessionHistory)
func apply(_ event: AgentEvent)              // TranscriptReducer
func finish(_ f: TaskFinished)
```

Delta coalescing: `model_text_delta` / `model_reasoning_delta` append to a
pending buffer flushed into `live` at most every 33 ms (`Task.sleep`), so
views observing `live` redraw ~30×/s and the committed `transcript` never
changes during streaming. `live` is committed to `transcript` on
`model_response`; `liveTools` on `task_completed` / `finish`.

`TranscriptReducer` rules:

1. `task_started` → if `pendingUserItemId == nil` append `.user(objective)`
   (a run started elsewhere); `status = .running(RunProgress(state: .preparing, step: 0, startedAt: now))`;
   append a log line. Reset `live`, `liveTools`.
2. `state_changed` → update `RunProgress.state`; log line.
3. `model_request_started` → `step`; commit a non-empty `live` to the
   transcript first; `live = AssistantTurn(step:)`.
4. `model_reasoning_delta` → buffer into `live.reasoning`, `isThinking = true`,
   `thinkingStarted` on first delta.
5. `model_text_delta` → buffer into `live.text`; `isThinking = false`,
   `thinkingDuration` set once.
6. `model_response` → flush buffers; `live.text` = final text when present
   (authoritative); `isStreaming = false`; commit `live` to the transcript
   unless it is empty (no text, no reasoning) — then drop it. `live = nil`.
7. `tool_call_started` → `liveTools` created if nil (after committing
   `live`); append `ToolCallItem(status: .running)`; log line "→ name args".
8. `tool_call_finished` → mark the call succeeded/failed, summary,
   duration; log line.
9. `tool_call_output` → attach `ToolOutput` by `callId`.
9a. `agent_started` → commit `live` and `liveTools`, then append an
    `AgentRun` to `liveAgents` (an `AgentGroup`, created on the first
    agent). `agent_progress` → the inner event is applied to that agent's
    own row (`applyAgentEvent`: tool calls / outputs / `file_changed` /
    streamed text as `liveText`; approvals are re-dispatched to the lead
    reducer, whose request id (`approval_call_0@agent-…`) maps back to the
    agent's tool row). `agent_finished` → status, report, files; the group
    is committed as `.agents` once every agent has reported, and closed as
    `.stopped` by `task_completed` / `finish`.
10. `approval_requested` → `pendingApproval = request`; append
    `.approval(pending)` to the transcript (committing `liveTools` first so
    order is preserved, then reopening a fresh group for later calls);
    `unseen = .approval` when not visible; notify.
    `approval_resolved` → mark the item resolved; clear `pendingApproval`.
11. `validation_finished` → log line only (the call is already in `liveTools`).
12. `file_changed` → `liveTools.filesChanged.insert(path)`; `changedPaths.insert(path)`.
13. `checkpoint` / `warning` → `.notice` (coalesce consecutive; "task
    boundary" warnings go to `detailLines`); log line.
14. `usage` → `usage`, `costUsd`. `context_compiled` → `contextStats`; log line.
15. `task_completed` → commit `live` and `liveTools` (terminal `.done` /
    `.stopped` / `.failed` by status); append `.outcome`; `status = .finished(status)`.
16. `finish(f)` with `error` → `.notice(error, isError: true)`;
    `lastError`, `lastErrorKind`; `status = .finished(f.status ?? .failed)`;
    `liveTools` closed with `.failed`. Always clears `pendingApproval`.
    `unseen = .finished(status)` / `.error` when not visible. `f.messageId`
    / `f.snapshot` are stamped onto the newest user item that has no
    `messageId` yet (`UserTurn.messageId`, `hasSnapshot`): that is what
    makes the message editable / undoable (`isRewindable`); history gives
    every user item its id and `snapshot` flag from `messages`.

`loadHistory` rebuilds the transcript from each turn: append the user
message(s) from `messages` (kind `user`, `runtime` flag → `isRuntime`) with
their `created_at`, replay `events` through the reducer with the event date
set from the surrounding records, then fill tool outputs from
`messages[kind == tool_result]` by `call_id` (`is_error` from the message)
and use `outcome` when present. Reasoning is not persisted, so history has
no Thinking rows. The reducer takes an optional `date:` so history items
carry their real timestamps.

### `BackendClient` (Backend/BackendClient.swift) — `@MainActor final class`

```swift
enum State: Equatable { case stopped, starting, running(pid: Int32), failed(String) }
var state: State
var onNotification: ((BackendNotification) -> Void)?   // delivered on the main actor
var stderrLog: [String]                                 // last 500 lines
func start(binary: URL, environment: [String: String], dataDir: URL?, mock: Bool) async throws -> InitializeResult   // throws BackendError; .incompatible detection: exit before initialize with "unrecognized subcommand" in stderr, or protocol_version != 1
func stop() async                                       // shutdown → wait ≤2 s → terminate → 1 s → SIGKILL
func initialize() async throws -> InitializeResult; configGet(); configPatch(values: [String: JSONValue])
func projectOpen(path: String, exact: Bool) -> ProjectInfo; projectClose(id:); projectStatus(id:) -> ProjectInfo; indexRebuild(id:)
func sessionsList(projectId:limit:) -> [SessionSummary]; sessionCreate(projectId:title:) -> SessionSummary; sessionRename(projectId:sessionId:title:) -> SessionSummary; sessionDelete(projectId:sessionId:)
func sessionHistory(projectId:sessionId:) -> SessionHistory
func sessionRewind(projectId:sessionId:messageId:restoreFiles:) -> RewindResult   // projectOpen takes initGit: too
func taskRun(projectId:sessionId:prompt:options: TaskRunOptions); taskCancel(projectId:)
func approvalRespond(projectId:requestId:decision:)
func permissionsSet(projectId:profile:) -> ProjectInfo
func modelsList(projectId:refresh:) -> ModelsList; modelsSwitch(projectId:model:) -> ModelSwitchResult
func gitStatus(projectId:) -> GitStatus; gitDiff(projectId:path:staged:) -> GitDiff; fileRead(projectId:path:maxBytes:) -> FileContent
func tasksList(projectId:limit:) -> [TaskSummary]; memoryList(projectId:category:limit:) -> [MemoryEntry]
func doctorRun(projectId:probe:) -> DoctorReport; shutdown()
```

All methods are `async throws` and throw `BackendError`.

Child-process I/O rules (S1): `signal(SIGPIPE, SIG_IGN)` once at launch
(S4); stdout is read with `FileHandle.readabilityHandler` into a `Data`
buffer split **strictly on 0x0A** (never `AsyncLineSequence`, which also
splits on U+2028/NEL); each complete line is decoded as UTF-8 and handed to
the `RPCConnection` actor with an async hop; an empty `availableData` means
EOF → handler set to nil → `stdoutClosed`; stderr is drained continuously
into a ring buffer; writes go through the actor
(`fileHandleForWriting.write(contentsOf:)`, EPIPE surfaces as `.transport`);
`terminationHandler` awaits `stdoutClosed` first, then fails every pending
continuation with `.transport("ctx exited (N)")` and emits `.exited`.
Frames are decoded with `JSONDecoder` into
`struct Frame: Decodable { id: JSONValue?; method: String?; params: JSONValue?; result: JSONValue?; error: RPCErrorBody? }`
so `JSONValue(any:)` is never on the hot path; typed results are decoded
from the `result` subtree with `JSONDecoder` on re-encoded data.

`BackendLocator.locate(preferences:) -> (found: URL?, searched: [URL])`
tries: the user override, `Bundle.main.bundleURL/Contents/Helpers/ctx`,
`…/Contents/MacOS/ctx`, `$CTX_BINARY`, `~/.cargo/bin/ctx`,
`/opt/homebrew/bin/ctx`, `/usr/local/bin/ctx`, then a repository checkout
(`Cargo.toml` + `target/release/ctx` or `target/debug/ctx`) found by walking
up from the bundle path and from the current directory; the first
`isExecutableFile` wins.

`Environment.swift`: `KeychainStore` (legacy keychain only, service
`dev.contextagent.desktop`, account = env var name; `errSecAuthFailed`,
`errSecUserCanceled`, `errSecInteractionNotAllowed` are treated as "not
available"); `LoginShellEnvironment.snapshot()` runs **once**
`/bin/zsh -ilc 'printf "\0__CTX_ENV__\0"; printenv -0'` with stdin/stderr
`nullDevice`, `TERM=dumb`, `CTX_DESKTOP=1`, 5 s timeout then terminate/kill,
parses NUL-separated `NAME=value` after the marker into a cached dictionary
(fallback: `ProcessInfo.processInfo.environment`); `BackendEnvironment.build`
= process environment + every login-shell variable the process lacks (minus
`PWD`, `OLDPWD`, `SHLVL`, `_`, `TERM`, `CTX_DESKTOP`) + PATH = login-shell
PATH, then `~/.cargo/bin:/opt/homebrew/bin:/usr/local/bin`, then the process
PATH (deduplicated) — a Finder launch only has launchd's PATH, so without
this the agent's commands miss pnpm, node and the like — + the API key
variable from Keychain (ahead of the login shell's) when the process does not
export it.

Ad-hoc signing note: with `CODE_SIGN_IDENTITY = "-"` the hardened runtime is
silently disabled and the Keychain prompts after every rebuild (designated
requirement = cdhash); the login-shell fallback avoids the prompt for
developers. A stable self-signed identity via an optional `Local.xcconfig`
is a follow-up, not v1.

## Component and helper API (S2/S3 implement verbatim; V agents call)

```swift
struct BrailleSpinner: View { init(size: CGFloat = 12, tint: Color = Theme.textSecondary) }
struct ProgressRing: View { init(fraction: Double?, size: CGFloat = 16, lineWidth: CGFloat = 2, tint: Color = Theme.accent) }
struct InlineDots: View { init(tint: Color = Theme.textTertiary) }
struct Chip: View { enum Style { case neutral, tinted(Color), badge }; init(_ text: String, systemImage: String? = nil, style: Style = .neutral) }
struct IconButton: View { init(_ systemImage: String, label: String, isActive: Bool = false, size: CGFloat = 16, action: @escaping () -> Void) }
struct SearchField: View { init(_ placeholder: String, text: Binding<String>, kbdHint: String? = nil, onSubmit: (() -> Void)? = nil) }
struct CopyButton: View { init(text: String, label: String = "Copy") }
struct StatusPill: View { init(_ status: TaskStatus); init(running: Bool); init(text: String, tint: Color) }
struct DisclosureRow<Label: View, Content: View>: View { init(isExpanded: Binding<Bool>, @ViewBuilder label: () -> Label, @ViewBuilder content: () -> Content) }
struct EmptyPlaceholder: View { init(systemImage: String, title: String, subtitle: String? = nil, actionTitle: String? = nil, action: (() -> Void)? = nil) }
struct HalftoneHeader: View { init(height: CGFloat = 120) }
struct LineList: View { init(text: String, maxLines: Int = 400, maxBytes: Int = 32_768, highlight: SyntaxHighlighter.Language? = nil, onOpenFull: (() -> Void)? = nil) }
enum Theme { static let canvas, surface, surfaceRaised, sidebar, border, borderStrong, textPrimary, textSecondary, textTertiary, accent, accentSoft, success, warning, danger, codeBackground, codeString, codeKeyword, codeNumber, codeComment, badgeBackground, badgeText: Color
             enum Font { static let title, heading, body, secondary, caption, mono, sidebar: SwiftUI.Font }
             enum Metrics { static let sidebarWidth: CGFloat = 248; inspectorWidth = 320; contentMaxWidth = 760; radiusSmall = 8; radius = 12; radiusLarge = 16 } }
extension Color { static func adaptive(light: String, dark: String) -> Color }   // hex strings
extension View { func card(radius: CGFloat = Theme.Metrics.radius, padding: CGFloat = 16) -> some View
                 func hoverHighlight(radius: CGFloat = 8) -> some View
                 func kbd() -> some View
                 func hairline(_ edges: Edge.Set) -> some View
                 func floatingShadow() -> some View
                 func selectableText() -> some View }
enum Icons { static let logo = "asterisk"; welcome = "hand.wave"; newChat = "plus"; projects = "folder.badge.plus"; changes = "arrow.triangle.branch"; project = "cube"; inspector = "sidebar.right"; settings = "gearshape"; search = "magnifyingglass"; send = "arrow.up"; stop = "stop.fill"; queued = "clock"; attach = "plus"; tools = "sparkles"; model = "asterisk"; copy = "doc.on.doc"; check = "checkmark.circle"; failed = "xmark.circle"; stopped = "stop.circle"; approval = "hand.raised"; validationPass = "checkmark.seal"; validationFail = "xmark.seal"; chevronDown = "chevron.down"; chevronRight = "chevron.right"; folder = "folder"; branch = "arrow.triangle.branch"; pin = "pin"; close = "xmark"; refresh = "arrow.clockwise"; warning = "exclamationmark.triangle"; info = "info.circle"; thinking = "brain"; file = "doc.text"; terminal = "terminal"; memory = "brain.head.profile"; context = "text.magnifyingglass"
             struct LogoView: View { init(size: CGFloat = 22) } }
enum ToolCategory: String { case file, search, shell, git, memory, validation, context }
struct ToolMetadata { let icon: String; let label: String; let category: ToolCategory; let verb: String /* "Reading" */; let pastVerb: String /* "Read" */
                      static func info(for toolName: String) -> ToolMetadata
                      static func pathArgument(for call: ToolCallItem) -> String?   // path / directory / cwd
                      static func rowLabel(for call: ToolCallItem) -> String }       // "Reading src/app.rs"
struct MarkdownView: View { init(_ markdown: String) }
struct CodeBlockView: View { init(code: String, language: String?) }
struct DiffView: View { init(diff: String, truncated: Bool = false) }
enum SyntaxHighlighter { enum Language { case json, swift, rust, python, javascript, typescript, go, shell, diff, plain; static func detect(_ hint: String?) -> Language }
                         static func highlight(_ line: String, language: Language) -> AttributedString }
struct ToastCenter { func show(_ message: String, kind: Toast.Kind = .info, details: String? = nil) }
```

## Self-test mode (S4: LaunchOptions.swift, UITestDriver.swift)

Launch arguments: `--mock` (start the backend with `ctx serve --mock`; also
`CTX_DESKTOP_MOCK=1`), `--project <path>` (open this folder at launch instead
of the remembered projects), `--autotest <dir>` (run the scripted self-test
and exit), `--binary <path>` (backend override for this launch). Settings →
General also has a "Use mock backend (no API key)" toggle that restarts the
backend. `CTX_DATA_DIR` in the environment is passed through to the child.

The app's own storage is isolated under `--autotest` (`LaunchOptions`,
"Storage"): `Preferences` read and write the
`dev.contextagent.desktop.autotest` defaults suite, emptied at launch, and
recents, hosts and attachment uploads live in `<autotest dir>/support/`
instead of `~/Library/Application Support/ContextAgent`. A run therefore
starts from factory settings and leaves nothing in the user's preferences
(before this, every run's `--project` and the worktree it creates were
remembered as the user's open projects and reopened at their next launch).
`-key value` launch overrides (`-inspectorVisible '<true/>'`,
`-openProjectPaths '(...)'`) still apply; a self-test that needs hosts seeds
`<autotest dir>/support/ssh-hosts.json`. The window frame is not autosaved
in this mode either.

`UITestDriver.run(appState:)` (started from `ContextAgentApp` after the
first window appears when `--autotest` is set): waits for `connection ==
.ready`; opens `--project` (default: the nearest ancestor of the bundle or
cwd containing `Cargo.toml`, else cwd); captures `01-welcome.png`; selects
the project, `newChat()`, captures `02-empty-chat.png`; attaches README.md,
Cargo.toml, docs and crates to the draft, captures `02b-attachments.png`
and renders the first chip's hover card to `02c-attachment-card.png`, then
clears them; sends "Summarize this
repository in three bullet points"; captures `03-running.png` after 1.5 s
and `04-finished.png` 1 s after `task/finished`; opens the model picker
(`modelPickerVisible = true`), captures `05-model-picker.png`, closes it;
renders the project model's models.dev hover card to `05b-model-card.png`
(skipped with a note when models.dev is unreachable);
switches to the Changes tab (`06-changes.png`) and the Project tab
(`07-project.png`); creates a worktree of the project (`worktree/create`,
expects it nested under the parent and selected), captures
`08-worktree.png`, then removes it with its branch (`worktree/remove`) and
expects the parent reselected; clicks the top bar's search field through
`NSWindow.sendEvent` and expects the palette to open (`click-search`); writes `result.json` (`{ok, steps: [...], transcriptItems,
error}`) into the directory and calls `NSApp.terminate` with exit 0 (1 on
failure). Screenshots use
`window.contentView.bitmapImageRepForCachingDisplay(in:)` + `cacheDisplay`
→ PNG (no screen-recording permission needed). Each step logs to stderr.

With `CTX_PERFBENCH=1` the script is shorter (window, backend, project, one
mock run) and ends with `PerfBench` (App/PerfBench.swift): it grows the chat
to each size in `CTX_PERFBENCH_STAGES` (default `0,10,30,60` synthetic runs,
replayed through `ChatState.apply`, half of them with three sub-agents) and at
each size measures a 9 s live run with four streaming sub-agents, 4 s of
composer typing, a scroller drag through the whole transcript and back, the
live run again (every row now built) and four Chat ⇄ Editor switches; at the
end one sub-agent with 150 tool calls opens in the Inspector's Agents tab
(checked to open at the bottom) and is dragged through. With
`CTX_PERFBENCH_VISUAL=1` every drag step also renders the scroll view and
counts the steps after which the content on screen moved by anything other
than the step (each synthetic run's text is unique so rows can be matched).
A probe
posts to the main queue every 8 ms and records the wait; `perf.json` has per
phase p50/p95/p99/max wait, `stalledMs` (wait beyond 16.7 ms) and process CPU.
Run it on a Release build, e.g. `CTX_PERFBENCH=1 CTX_DATA_DIR=<tmp>
"<app>/Contents/MacOS/Context" --mock --autotest <dir> --project <repo>`.

## Keyboard shortcuts (AppCommands.swift, the only owner)

| keys | action |
|---|---|
| ⌘N | New chat |
| ⌘O | Open project… · File → Open Recent submenu (+ Clear Menu) |
| ⌘K | Command palette |
| ⌘⇧M | Model picker |
| ⌘⇧I | Toggle inspector · ⌃⌘S toggle sidebar |
| ⌘4 | Editor tab · ⌘S save the selected document · ⌘⇧W close its tab |
| ⌘1 / ⌘2 / ⌘3 | Chat / Changes / Project tab |
| ⌘. | Cancel running task |
| ⌘⏎ | `primaryAction()` (approve the visible approval, else send) |
| ⌘⌫ | Deny the visible approval |
| ⌘⇧C | Copy transcript as Markdown · File → Export Chat as Markdown… |
| ⌘F | Find in chat |
| ⌘, | Settings (the profile page's last settings tab) |
| ⌘⇧R | Rebuild index |
| ⌘⌥Z | Undo Last Turn… (the keep-or-revert sheet for the newest message the user typed) |

Esc precedence: command palette → model picker / tools popover → clear the
composer selection. Esc never answers an approval.

## Persistence

- Recent projects: `~/Library/Application Support/ContextAgent/recent-projects.json`
  (`[{path, name, lastOpened, pinned}]`, max 30). Projects open at quit are
  reopened at launch.
- Preferences (UserDefaults): backend path override, appearance, context
  toggle per project, last selected project/session, sidebar/inspector
  visibility, last tab and last Changes file per project, recently used
  models per base URL (max 8), notification toggles, `autoInitGit`
  (Settings → General → Git, default on: `project/open` is sent with
  `init_git` so folders without a repository get one, and a toast says so; folders reopened at launch or after a backend restart pass `init_git: false`, so a remembered project whose repository went missing, such as a linked worktree whose `.git` file is gone, is not quietly turned into a new repository).
- Window frame via `setFrameAutosaveName("Main")`.
- API keys: Keychain only.

## Parallel sessions (worktrees)

The backend runs one task per project root, so a second prompt in a busy
project queues ("Waiting for “…”"). To start a task without waiting, the
project gets a linked git worktree (`worktree/create`, docs/desktop-protocol.md
"Worktrees"): a second checkout on its own `ctx/<name>` branch under
`~/.context-agent/worktrees/`, opened as a project of its own and nested
under the parent in the sidebar (`AppState.topLevelProjects` /
`worktrees(of:)`, matched by `ProjectInfo.git.worktree.parent_root`). Entry
points: File → New Chat in a Worktree (⌘⇧N), the project row's context menu,
the palette, and the composer's waiting hint ("Start in a new worktree
instead", ⌘⇧Return) which moves the draft to the new worktree's first chat
and sends it. A worktree row's × (or "Remove Worktree…") asks whether to
keep or delete the branch (`worktree/remove`, force: uncommitted changes in
the checkout are lost; refused while it runs). "Merge into <parent branch>…"
in the same menu lands the worktree's commits in the parent checkout
(`worktree/merge`: `git merge --no-ff` there, a merge commit; "Merge and
Remove Worktree" also takes the checkout and branch away), disabled while
either project runs; the backend refuses an uncommitted worktree and aborts
a conflicting merge, and the toast carries the reason. Afterwards the
parent's git status, commit log and project status are refreshed
(`AppState.mergeWorktree`). Closing the parent closes its
open worktrees too; the parent's context menu reopens them ("Open
Worktree"). Worktrees are remembered in `openProjectPaths` like any project
and never appear in Recents.

## Remote hosts (SSH)

Like Claude Code's and Codex's remote sessions, a project can live on
another machine: the app runs `ctx serve` there over SSH and talks the same
protocol through the SSH session's pipes (docs/desktop-protocol.md "Remote
hosts"). Nothing in the Swift code reaches the network itself; `ssh` is
the child process.

- **One connection per host.** `AppState.remotes[host.id]` is a
  `RemoteConnection` (App/RemoteHosts.swift): an `SSHHost` from
  `SSHHostStore` (Settings → Remote hosts, `ssh-hosts.json` next to the
  recents), its own `BackendClient` started with `BackendLaunch.ssh(host)`
  — `ssh -o BatchMode=yes -o ConnectTimeout=20 -o ServerAliveInterval=15 -T
  [-p port] [-i key] [-l user] <extras> -- <address> 'env PATH="$HOME/.cargo/bin:…:$PATH" ctx serve'`
  (Backend/BackendLaunch.swift; `CTX_SSH_BINARY` replaces `/usr/bin/ssh`
  for tests) — a `state` (disconnected / connecting / ready / failed) and a
  `hint` (what ssh's last words mean: keys, PATH, the address, the host
  key). The local `AppState.backend` is unchanged; `--mock` applies to
  every backend. The connection is made when the first project on the host
  is opened (`AppState.connect`, concurrent callers share one attempt) and
  stays until Disconnect, Remove Host, or ssh exits: then
  `handleRemoteExit` fails the host's running chats, marks the connection
  failed and leaves the rows in place for Reconnect — never an automatic
  retry (a dropped VPN would spawn ssh every second).
- **A remote project is a `ProjectState` with a `backend` and a `host`.**
  Every request about a project goes through `project.backend` (the
  `ProjectState` loads take no backend argument any more; AppState's
  per-project intents use `project.backend`, chat-scoped ones
  `backend(for: projectId)`), readiness is `AppState.isReady(project)`
  and the key check `needsAPIKey(for:)`, so a remote project never looks at
  the local backend's state. Notifications are routed by project id, which
  `openProject(path:host:)` keeps unique across backends (the same path on
  two hosts hashes to one `RepositoryId`: the second open is refused with a
  toast); `.log` lines get a `[host]` prefix, `.mcpStatus` is the local
  backend's only. `makeProject` builds the `ModelCatalog` from the host's
  `initialize` result.
- **Keys.** Preferences and recents key a project by `ProjectState.key` /
  `RecentProject.key`: the path locally, `ctx-ssh://<host uuid>/path` on a
  host (`RemoteProjectRef`). `openProjectPaths` mixes both; the local
  restore skips remote keys and `reopenRememberedRemoteProjects` (started
  by `startBackend`, once per launch) reconnects each host in parallel and
  reopens its folders. `openingProjectPaths` holds keys too (the loading
  rows decode them).
- **No shared filesystem.** The Files tab lists folders through `fs/list`
  (`FileTreeModel(root:remote:)`, async rows), the folder picker is the
  `RemoteFolderPicker` sheet over `fs/list {dirs_only}`, the Editor reads
  and writes through `file/read` / `file/write` as always but shows a
  placeholder for images and PDFs, Finder and "Open with Default Editor"
  actions are hidden (`FileRow.localActions`, `AppState.reveal`), and the
  Terminal tab's shells are `ssh -t <host> 'cd <root>; exec $SHELL -l'`
  (`TerminalWorkspace(directory:host:)`). Attachments: `task/run` and
  `task/steer` get the bytes (`RemoteAttachments.prepare` →
  `InlineImage` / `FileUpload.content`) instead of paths; the transcript
  keeps the local paths for its thumbnails, history shows names.
- **UI.** Welcome: "Open on a remote host…" under the open button (Settings
  → Remote hosts when none is configured). Sidebar: a "Remote" section
  listing every configured host (HostRow) with its projects, worktrees,
  opening rows and pinned recents beneath it; the local Projects section
  shows local projects only. File → Open Project on Remote Host… (⌘⇧O).
  Recents cards carry the host as a chip and open through
  `AppState.openRecent`. The Inspector Run tab's backend card describes the
  selected project's host connection (Reconnect instead of Retry).

## Build and run

```
xcodebuild -project apps/macos/ContextAgent.xcodeproj -scheme ContextAgent -configuration Debug build
```

The project (`objectVersion = 77`, one `PBXFileSystemSynchronizedRootGroup`,
no `PBXBuildFile` entries, `GENERATE_INFOPLIST_FILE = YES`,
`ENABLE_APP_SANDBOX = NO`, `ENABLE_USER_SCRIPT_SANDBOXING = NO`, ad-hoc
signing `-`, `ENABLE_HARDENED_RUNTIME = NO`, bundle id
`dev.contextagent.desktop`, product "Context", category developer-tools)
was validated on Xcode 26.6. The hardened runtime is off for these ad-hoc
builds because its library validation refuses the embedded Sparkle.framework
when nothing carries a Team ID (dyld: "different Team IDs");
`scripts/release.sh` turns it on (`--options runtime`) when it re-signs
everything with the Developer ID. xcodebuild
auto-creates the scheme. The "Embed ctx backend" Run Script phase declares
`$(CODESIGNING_FOLDER_PATH)/Contents/Helpers/ctx` as its output (so CodeSign
re-runs) and runs `scripts/embed-ctx.sh`: `cargo build --release -p ctx-cli`
when cargo is available (`CTX_SKIP_CARGO=1` reuses `target/release/ctx`),
copies the binary into `Contents/Helpers/ctx` and ad-hoc signs it; a missing
binary only prints a warning. Acceptance: `codesign --verify --deep --strict`
on the built app passes; the app launches; `ctx serve --mock` answers
`initialize` from inside the bundle.

### Updates

Releases update themselves with [Sparkle](https://sparkle-project.org)
(`App/Updater.swift`). An update replaces the whole bundle, so the app and
`Contents/Helpers/ctx` always move together and the backend never runs
against an app speaking another protocol version. `apps/macos/Info.plist`
(merged with the generated one) holds the feed, the EdDSA public key and the
defaults: check daily, download in the background, install on quit, verify
the signature before unpacking. The feed is
`https://github.com/Model-Gateway/context-agent/releases/latest/download/appcast.xml`:
`scripts/release.sh` sets `CFBundleVersion` to the release version (what
Sparkle compares), re-signs Sparkle's helpers with the Developer ID, writes
`dist/appcast.xml` with the zip's EdDSA signature (`NOTES=<file.md>` embeds
Markdown release notes) and, with `PUBLISH=1`, creates the GitHub release
with the zip, dmg and appcast. Only Developer ID builds start the updater:
Xcode and local `xcodebuild` builds (ad hoc) and `--autotest` runs show the
version without update controls. "Install and Relaunch" while a task runs
asks whether to restart now (stopping it) or once every task has finished.
`CTX_UPDATE_FEED=<url>` points a release build at another appcast.
