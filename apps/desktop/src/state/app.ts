// Port of apps/macos/ContextAgent/App/AppState.swift and of the `AppState`
// extension in App/ProjectState.swift: backend lifecycle, open projects,
// selection, every user intent the views call, and the routing of backend
// notifications into `ProjectState` / `ChatState` (apps/macos/ARCHITECTURE.md
// "AppState" and "handle rules"; apps/desktop/ARCHITECTURE.md "Stores").
//
// One zustand store, `useApp`, holds the plain fields and the intents. The
// projects and chats are classes mutated in place: every mutation ends with
// `touch(target)` (or the object's own `touch()`), which bumps the chat's /
// project's `version` and the store's `version`, so subscribers re-render.
// `useProject`, `useChat`, `useSelectedProject` and `useSelectedChat`
// subscribe a view to one object; state/selectors.ts holds the derived
// values (`selectedChat`, `needsAPIKey`, `pendingApprovals`, …).
//
// Git actions, worktrees, the ModelGateway account and the editor are other
// modules (state/git.ts, worktrees.ts, gateway.ts, editor.ts). They build on
// the intents here (`register`, `startLoads`, `refreshGit`, `patchConfig`,
// `restartBackend`, …) and plug into the flows that need them through
// `extendApp` / `onProjectClosed` / `onBackendStarted`.
//
// Projects on remote hosts (App/RemoteHosts.swift, docs/desktop-protocol.md
// "Remote hosts"): one `ssh <host> ctx serve` connection per configured
// host (`RemoteConnection`, state/remote.ts), started when the first
// project on it is opened and kept until the host is disconnected. A remote
// project is an ordinary `ProjectState` whose `backend` is that connection
// and whose `host` names it; every per-project request goes to
// `project.backend`, readiness and the key check are per backend, and the
// notifications of each backend are routed by project id.

import { create } from 'zustand'
import { invoke } from '@tauri-apps/api/core'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { openPath } from '@tauri-apps/plugin-opener'
import { backend, BackendClient } from '../backend/client'
import { BackendError, errorText, failureOf, type BackendFailure } from '../backend/errors'
import { debug } from '../backend/log'
import type { BackendNotification } from '../backend/notifications'
import {
  isDiscountDrop,
  isEarlyCancel,
  jsonInt,
  jsonObject,
  jsonString,
  parseRFC3339,
  restoreReportTotal,
  runningSessionIds,
  type ApprovalDecision,
  type BrowserRequest,
  type DoctorReport,
  type FileUpload,
  type FsListing,
  type GitInfo,
  type InitializeResult,
  type InlineImage,
  type JSONObject,
  type JSONValue,
  type LspDiagnostics,
  type McpCatalogApp,
  type McpServerStatus,
  type ModelStat,
  type ModelSwitchResult,
  type ProjectInfo,
  type QuestionAnswer,
  type QuestionRequest,
  type TaskRunOptions,
  type TaskStatus,
} from '../backend/types'
import { copyText } from '../components/clipboard'
import { toasts, type ToastLevel } from '../components/toasts'
import type { PathAction } from '../markdown/fileReferences'
import { pastedAttachments, prepareAttachmentPrompt, prepareRemoteAttachments, type ComposerAttachment } from '../models/attachments'
import { ModelCatalog } from '../models/modelCatalog'
import { modelsDev, type ModelsDevInfo } from '../models/modelsDev'
import { approvalModeLabel as permissionLabel } from '../models/permissions'
import {
  forgetOpenProject,
  getPref,
  lastTabFor,
  rememberOpenProject,
  setLastChangesPath,
  setLastSelectedSession,
  setLastTab,
  setPermissionProfile,
  setPref,
  setProjectCollapsed,
} from '../models/preferences'
import { lastPathComponent, RecentProjects, standardizePath, type RecentProject } from '../models/recentProject'
import { chatStatusIsRunning, type LogLine } from '../models/reducer'
import { decodeRemoteProject, hostDisplayName, hostIsValid, hostTarget, isRemoteProjectKey, normalizeHost, projectKey, sameHost, SSHHosts, type SSHHost } from '../models/sshHost'
import { makeNoticeItem, makeUserTurn, outgoingPrompt, userTurnIsRewindable, type OutgoingPrompt, type UserTurn } from '../models/transcript'
import type { IconName } from '../theme/icons'
import { BrowserWorkspace } from './browser'
import { ChatState } from './chat'
import { isAppActive, notifier, onAppBecameActive } from './notifications'
import { ProjectState, starterFallback } from './project'
import { isCtxProblem, RemoteConnection, remoteFailureSummary, remoteHint, remoteProblem, type RemotePickerRequest } from './remote'
import {
  anyProjectRunning,
  apiKeyEnvName,
  attentionCount,
  canRewind as canRewindIn,
  isProjectReady,
  isReady,
  needsAPIKeyFor,
  projectById,
  projectsOn,
  selectedChat,
  selectedProject,
  worktreesOf,
} from './selectors'
import { TerminalWorkspace } from './terminal'

export type { PathAction } from '../markdown/fileReferences'

// MARK: - Declared types

/// `SidebarItem` (AppState.swift): the detail pane's screen. Chat · Changes ·
/// Project · Editor are the tabs remembered per project; Welcome, Projects,
/// Apps and Account are app-wide screens.
export type SidebarItem = 'welcome' | 'projects' | 'apps' | 'chat' | 'changes' | 'project' | 'editor' | 'account'

/** `SidebarItem.allCases`. */
export const SIDEBAR_ITEMS: readonly SidebarItem[] = ['welcome', 'projects', 'apps', 'chat', 'changes', 'project', 'editor', 'account']

/** `SidebarItem.isProjectTab`. */
export function isProjectTab(item: SidebarItem): boolean {
  return item === 'chat' || item === 'changes' || item === 'project' || item === 'editor'
}

/** `SidebarItem(rawValue:)`. */
export function parseSidebarItem(raw: string | null | undefined): SidebarItem | null {
  return raw != null && (SIDEBAR_ITEMS as readonly string[]).includes(raw) ? (raw as SidebarItem) : null
}

/** `InspectorTab` (AppState.swift). */
export type InspectorTab = 'run' | 'files' | 'terminal' | 'agents' | 'browser'

/** `InspectorTab.allCases`. */
export const INSPECTOR_TABS: readonly InspectorTab[] = ['run', 'files', 'terminal', 'agents', 'browser']

/** `InspectorTab.title`. */
export function inspectorTabTitle(tab: InspectorTab): string {
  switch (tab) {
    case 'run':
      return 'Run'
    case 'files':
      return 'Files'
    case 'terminal':
      return 'Terminal'
    case 'agents':
      return 'Agents'
    case 'browser':
      return 'Browser'
  }
}

/** `InspectorTab.systemImage`. */
export function inspectorTabIcon(tab: InspectorTab): IconName {
  switch (tab) {
    case 'run':
      return 'chart'
    case 'files':
      return 'folder'
    case 'terminal':
      return 'terminal'
    case 'agents':
      return 'agents'
    case 'browser':
      return 'browser'
  }
}

/** `ConnectionState` (AppState.swift). */
export type ConnectionState =
  | { kind: 'connecting' }
  | { kind: 'ready'; result: InitializeResult }
  | { kind: 'failed'; failure: BackendFailure }

/** `RewindRequest.Mode` (Views/Chat/RewindSheet.swift). */
export type RewindMode =
  /** Re-send `newText` in place of the message. */
  | { kind: 'edit'; newText: string }
  /** Remove the turn; the message goes back into the composer. */
  | { kind: 'undo' }

/// `RewindRequest` (Views/Chat/RewindSheet.swift): the edit / undo waiting
/// on the keep-or-revert sheet.
export interface RewindRequest {
  id: string
  chat: ChatState
  turn: UserTurn
  mode: RewindMode
  /** Messages the user typed after this one (they go too). */
  laterMessages: number
  /** Files changed from this message on, as the transcript recorded them. */
  changedFiles: string[]
}

let rewindRequestCounter = 0

/** `RewindRequest(chat:turn:at:mode:)`. */
export function makeRewindRequest(chat: ChatState, turn: UserTurn, index: number, mode: RewindMode): RewindRequest {
  const scope = chat.transcript.rewindScope(index)
  rewindRequestCounter += 1
  return { id: `rewind-${rewindRequestCounter}`, chat, turn, mode, laterMessages: scope.laterMessages, changedFiles: scope.changedFiles }
}

/** `LaunchOptions` as the `launch_options` command returns it (src-tauri/src/selftest.rs). */
export interface LaunchOptions {
  mock: boolean
  project: string | null
  binary: string | null
  autotest: string | null
}

/** `SettingsTab` raw values the store itself uses (Views/Settings/SettingsView.swift). */
export const SETTINGS_TAB_ACCOUNT = 'account'
export const SETTINGS_TAB_GENERAL = 'general'
export const SETTINGS_TAB_REMOTE_HOSTS = 'remoteHosts'

/** `BackendLocator.buildCommand`. */
export const BUILD_COMMAND = 'cargo build --release -p ctx-cli'

// MARK: - Store shape

/** `AppState`'s stored properties. */
export interface AppFields {
  connection: ConnectionState
  config: InitializeResult | null
  projects: ProjectState[]
  selectedProjectId: string | null
  /** Null → EmptyChatView. */
  selectedSessionId: string | null
  sidebarItem: SidebarItem
  sidebarVisible: boolean
  inspectorVisible: boolean
  inspectorWidth: number
  inspectorTab: InspectorTab
  paletteVisible: boolean
  /** The model picker, anchored to the composer's model chip. */
  modelPickerVisible: boolean
  /** Bumped by newChat, send, select(session) and popover dismissal. */
  composerFocusToken: number
  /** projectId → draft typed in the empty state. */
  pendingDraft: Record<string, string>
  /** projectId → attachments of the empty-state draft. */
  pendingAttachments: Record<string, ComposerAttachment[]>
  /** `log` notifications (errors also raise a toast). */
  backendLog: LogLine[]
  /** Doctor results without a project (Settings → Probe endpoint). */
  doctorReport: DoctorReport | null
  doctorRunning: boolean
  doctorError: string | null
  /** `mcp/status`: every configured MCP server's live state (kept current by the notification). */
  mcpServers: McpServerStatus[]
  /** `mcp/catalog`: the curated apps (Apps tab); fetched once per backend. */
  mcpCatalog: McpCatalogApp[]
  /** True while `startBackend` / `restartBackend` run. */
  backendStarting: boolean
  /// Folders whose `project/open` has not returned yet: the remembered
  /// projects from the moment the app launches (before the backend is up),
  /// then any folder being opened. The sidebar shows them as loading rows
  /// and their recent cards say "Opening…".
  openingProjectPaths: string[]
  /// The remembered chat `selectProject` is still waiting for the chat list
  /// to find; the Chat tab shows the transcript skeleton meanwhile instead of
  /// flashing the empty state.
  restoringSessionId: string | null
  /** The edit / undo waiting on the keep-or-revert sheet (`RewindSheet`). */
  rewindRequest: RewindRequest | null
  rewindInFlight: boolean
  /** The remote folder picker sheet (`RemoteFolderPicker`), when shown. */
  remotePicker: RemotePickerRequest | null
  /** Bumped by every change of the store or of an object in it. */
  version: number
}

/** `testHost`'s answer: what the probe found, or why it failed (with the hint). */
export type HostTestResult = { ok: true; summary: string } | { ok: false; message: string; needsCtx: boolean }

/** `installCtx`'s answer: the installed version line, or why not. */
export type CtxInstallResult = { ok: true; installed: string } | { ok: false; message: string }

/** `openProject`'s options. */
export interface OpenProjectOptions {
  select?: boolean
  restoreSession?: boolean
  /** The SSH host the folder is on; null (the default) opens a local folder. */
  host?: SSHHost | null
}

/** `AppState`'s intents. */
export interface AppActions {
  // Observation
  /** Marks a chat, a project or (no argument) the store itself changed. */
  touch: (target?: ChatState | ProjectState | null) => void

  // Simple setters (the Swift properties' `didSet`s)
  setSidebarItem: (item: SidebarItem) => void
  setSidebarVisible: (visible: boolean) => void
  toggleSidebar: () => void
  setInspectorVisible: (visible: boolean) => void
  toggleInspector: () => void
  setInspectorWidth: (width: number) => void
  setInspectorTab: (tab: InspectorTab) => void
  setPaletteVisible: (visible: boolean) => void
  setModelPickerVisible: (visible: boolean) => void
  /** `composerFocusToken += 1`. */
  focusComposer: () => void
  setPendingDraft: (projectId: string, text: string) => void
  setPendingAttachments: (projectId: string, attachments: ComposerAttachment[]) => void
  setRewindRequest: (request: RewindRequest | null) => void
  setCollapsed: (collapsed: boolean, project: ProjectState) => void

  // Workspaces
  runInTerminal: (command: string) => void
  showAgent: (agentId: string, chat: ChatState) => void
  browser: (project: ProjectState) => BrowserWorkspace
  terminals: (project: ProjectState) => TerminalWorkspace

  // Backend lifecycle
  claimBackendStart: () => boolean
  startBackend: () => Promise<void>
  restartBackend: () => Promise<void>

  // Projects
  chooseProject: (newProject?: boolean) => Promise<void>
  openProject: (path: string, options?: OpenProjectOptions) => Promise<ProjectState | null>
  /** Opens a remembered project, local or remote. */
  openRecent: (recent: RecentProject, options?: Omit<OpenProjectOptions, 'host'>) => Promise<ProjectState | null>
  register: (info: ProjectInfo, host?: SSHHost | null) => ProjectState
  startLoads: (project: ProjectState) => void
  /** `forget` false keeps the project in the remembered-open list (a host being disconnected). */
  closeProject: (id: string, forget?: boolean) => Promise<void>
  openSettings: (tab?: string | null) => void
  selectProject: (id: string, restoreSession?: boolean) => void

  // Remote hosts (App/RemoteHosts.swift)
  /** The host's connection object, created (disconnected) on first use. */
  remoteFor: (host: SSHHost) => RemoteConnection
  /** The connection of the project's host, if any. */
  remoteOf: (project: ProjectState) => RemoteConnection | null
  /** The backend a project id belongs to (the local one for unknown ids). */
  backendFor: (projectId: string | null | undefined) => BackendClient
  /** Starts `ssh <host> ctx serve` unless it is already up; null (with a toast unless `quietly`) when it cannot be reached. */
  connectHost: (host: SSHHost, quietly?: boolean) => Promise<RemoteConnection | null>
  /** Closes the host's projects (kept remembered) and stops its ssh. */
  disconnectHost: (host: SSHHost) => Promise<void>
  /** Reconnects after a failure or a dropped link and reopens the host's projects. */
  reconnectHost: (host: SSHHost) => Promise<void>
  /** Disconnects, forgets the host's recents and remembered folders, removes it from Settings. */
  removeHost: (host: SSHHost) => Promise<void>
  /** Adds or updates a host (Settings → Remote hosts); applies at the next connect. */
  saveHost: (host: SSHHost) => SSHHost
  /** Test connection: connects exactly as the app will and reports what answered. */
  testHost: (host: SSHHost) => Promise<HostTestResult>
  /** Install ctx on host: copies this version's Linux build to ~/.local/bin over ssh, then reconnects a saved host. */
  installCtx: (host: SSHHost) => Promise<CtxInstallResult>
  /** Welcome, the sidebar, mod+shift+O: the remote folder picker (Settings → Remote hosts first when no host is configured). */
  chooseRemoteProject: (host?: SSHHost | null) => void
  setRemotePicker: (request: RemotePickerRequest | null) => void
  /** `fs/list {dirs_only}` on the host (connecting first), for the picker. */
  listRemoteDirectory: (path: string | null, host: SSHHost) => Promise<FsListing>

  // Sessions
  selectSession: (id: string | null) => Promise<void>
  newChat: () => void
  requestSuggestion: (chat: ChatState, projectId: string) => void
  loadStarters: (project: ProjectState) => void
  send: (prompt: string | OutgoingPrompt, options?: { queue?: boolean }) => Promise<void>
  retry: (chat: ChatState) => Promise<void>
  cancelRunningTask: () => Promise<void>
  removeQueued: (index: number, chat: ChatState) => void
  editQueued: (index: number, chat: ChatState) => void
  renameSession: (id: string, title: string) => Promise<void>
  deleteSession: (id: string) => Promise<void>

  // Rewind
  canRewind: (chat: ChatState) => boolean
  beginEdit: (turn: UserTurn, chat: ChatState) => void
  cancelEdit: (chat: ChatState) => void
  submitEdit: (chat: ChatState) => void
  undoLastTurn: () => void
  undo: (turn: UserTurn, chat: ChatState) => void
  performRewind: (request: RewindRequest, restoreFiles: boolean) => Promise<void>

  // Approvals and questions
  primaryAction: () => void
  denyPendingApproval: () => void
  /** `respond(approval:in:decision:)`. */
  respond: (requestId: string, chat: ChatState, decision: ApprovalDecision) => Promise<void>
  /** `respond(question:in:answers:message:)`: `answers` and `message` null skip the questions. */
  respondQuestion: (requestId: string, chat: ChatState, answers: QuestionAnswer[] | null, message?: string | null) => Promise<void>

  // Models and permissions
  switchModel: (name: string) => Promise<ModelSwitchResult>
  toggleModelPicker: () => void
  modelDisplayName: (id: string) => string
  modelInfo: (id: string) => ModelsDevInfo | null
  modelStat: (id: string, project?: ProjectState | null) => ModelStat | null
  refreshModels: (force: boolean) => Promise<void>
  prefetchModels: (project: ProjectState) => Promise<void>
  restorePermissionProfile: (project: ProjectState) => Promise<void>
  setApprovalMode: (profile: string) => Promise<void>
  rebuildIndex: () => Promise<void>
  runDoctor: (probe: boolean) => Promise<void>

  // Config, MCP, API key, binary
  patchConfig: (values: JSONObject) => Promise<void>
  refreshMcpStatus: () => Promise<void>
  reconnectMcp: (server?: string | null) => Promise<void>
  loadMcpCatalog: () => Promise<void>
  installMcpApp: (id: string, inputs: { [key: string]: string }, excludeTools?: string[]) => Promise<void>
  uninstallMcpApp: (id: string) => Promise<void>
  isAPIKeyStored: () => Promise<boolean>
  saveAPIKey: (key: string) => Promise<void>
  removeAPIKey: () => Promise<void>
  chooseBackendBinary: () => Promise<void>
  copyBuildCommand: () => void
  copyToPasteboard: (text: string) => void

  // Git / paths
  refreshGit: (project: ProjectState) => Promise<void>
  loadProjectExtras: (project: ProjectState) => Promise<void>
  selectChange: (path: string | null, project: ProjectState) => Promise<void>
  setShowStaged: (staged: boolean, project: ProjectState) => Promise<void>
  /** `line` (1-based) comes from a file chip's `:42` / `#L42` suffix and is used by `openInEditor`. */
  reveal: (path: string, project: ProjectState, action: PathAction, line?: number | null) => void
  openInEditor: (path: string) => void
  scheduleGitRefresh: (project: ProjectState) => void

  // Notifications
  /** Routes one notification of the local backend (`remote` null) or of a host's connection. */
  handle: (notification: BackendNotification, remote?: RemoteConnection | null) => void
  clearBackendLog: () => void
  refreshAttention: () => void
}

/** The `useApp` store: `AppState`'s fields and intents. */
export type AppStore = AppFields & AppActions

// MARK: - Extension points (other modules' parts of AppState)

/// What state/editor.ts, state/git.ts and state/gateway.ts plug into the
/// flows this module owns. Each is optional; without it the flow skips that part.
export interface AppExtensions {
  /** state/editor.ts: `lsp/diagnostics` → the open document (`editors[projectId]?.document(for:)`). */
  lspDiagnostics: (diagnostics: LspDiagnostics) => void
  /** state/editor.ts: `openInEditor(path:line:in:)`, the Editor tab (`PathAction.openInEditor`). */
  openInEditor: (relativePath: string, project: ProjectState, line: number | null) => void
  /** state/git.ts: `commitFromBox(in:)`, ⌘⏎ on the Changes tab with a commit message typed. */
  commitFromBox: (project: ProjectState) => Promise<void>
  /** state/gateway.ts: `gateway.isSignedIn` (model stats are shown only while signed in). */
  isGatewaySignedIn: () => boolean
}

const extensions: Partial<AppExtensions> = {}
const projectClosedListeners = new Set<(projectId: string) => void>()
const backendStartedListeners = new Set<() => Promise<void> | void>()

/** Plugs other modules' parts into the store's flows (later registrations replace earlier ones). */
export function extendApp(extension: Partial<AppExtensions>): void {
  Object.assign(extensions, extension)
}

/// `closeProject` dropped the project (`editors.removeValue(forKey:)`, …);
/// `listener` runs with its id. Returns the unsubscribe function.
export function onProjectClosed(listener: (projectId: string) => void): () => void {
  projectClosedListeners.add(listener)
  return () => {
    projectClosedListeners.delete(listener)
  }
}

/// The backend started (or restarted) and the projects were reopened;
/// `listener` runs without holding the start up (`retireLegacyGatewayKey`,
/// `restoreGatewaySession`). Returns the unsubscribe function.
export function onBackendStarted(listener: () => Promise<void> | void): () => void {
  backendStartedListeners.add(listener)
  return () => {
    backendStartedListeners.delete(listener)
  }
}

// MARK: - Module state (`@ObservationIgnored` in Swift)

/** `maxAutoRestarts`. */
const MAX_AUTO_RESTARTS = 2
/** `autoRestartResetAfter`, in seconds. */
const AUTO_RESTART_RESET_AFTER = 300
/** `maxBufferedNotifications`. */
const MAX_BUFFERED_NOTIFICATIONS = 64
/** The debounce of `scheduleGitRefresh`. */
const GIT_REFRESH_DELAY_MS = 800
/** How long a remembered ("approve for session") request may stay before it is announced. */
const APPROVAL_NOTIFY_DELAY_MS = 150
/** `suggest/starters` at most this often per project. */
const STARTERS_MAX_AGE_MS = 600_000

const gitRefreshTimers = new Map<string, ReturnType<typeof setTimeout>>()
/** projectId → the Inspector Terminal tab's shells, kept across tab switches. */
const terminalWorkspaces = new Map<string, TerminalWorkspace>()
/** projectId → the Inspector Browser tab's page. */
const browserWorkspaces = new Map<string, BrowserWorkspace>()
let lastSuccessfulStart: number | null = null
let autoRestartPending = false
/** Consecutive automatic restarts; reset after a long healthy run. */
let autoRestartCount = 0
let backendStartClaimed = false
let backendLogNextId = 0
/// Notifications for a project whose `project/open` has not returned yet (a
/// small repository's `index/ready` beats the response frame), replayed once
/// the `ProjectState` exists.
const pendingProjectNotifications = new Map<string, BackendNotification[]>()
/// The remembered projects (keys: paths, or `ctx-ssh://…` on a host) in
/// sidebar order while launch reopens them in parallel, so rows land in
/// place however the opens finish.
let restoreOrder: string[] = []
let launchOptionsPromise: Promise<LaunchOptions> | null = null
/** host id → its `ssh … ctx serve` connection (`AppState.remotes`). */
const remoteConnections = new Map<string, RemoteConnection>()
/** `reopenRememberedRemoteProjects` runs once per launch. */
let remoteRestoreStarted = false
let remotePickerCounter = 0

/** `remotes[hostId]` without creating one. */
export function remoteConnection(hostId: string | null | undefined): RemoteConnection | null {
  if (hostId == null) return null
  return remoteConnections.get(hostId) ?? null
}

// MARK: - Store plumbing

let batchDepth = 0
let batchDirty = false

/** Bumps the store's version (deferred to the end of a `batch`). */
function bumpStore(): void {
  if (batchDepth > 0) {
    batchDirty = true
    return
  }
  useApp.setState((state) => ({ version: state.version + 1 }))
}

/// Runs `body` with the store notifications it causes coalesced into one
/// (the touches of one backend notification, say). Synchronous only.
export function batch<T>(body: () => T): T {
  batchDepth += 1
  try {
    return body()
  } finally {
    batchDepth -= 1
    if (batchDepth === 0 && batchDirty) {
      batchDirty = false
      bumpStore()
    }
  }
}

/** Sets plain fields (and bumps the version). */
function patch(fields: Partial<AppFields>): void {
  useApp.setState((state) => ({ ...fields, version: state.version + 1 }))
}

function get(): AppStore {
  return useApp.getState()
}

/** `touch(chat?)`: marks a chat, a project or the store changed. */
export function touch(target?: ChatState | ProjectState | null): void {
  if (target instanceof ChatState || target instanceof ProjectState) target.touch()
  else bumpStore()
}

/** `ToastCenter.show(_:kind:details:)`. */
function toast(message: string, level: ToastLevel, details?: string): void {
  toasts.push({ message, level, details })
}

// MARK: - Launch options and paths

/** The process's launch options (`launch_options`), read once. */
export function launchOptions(): Promise<LaunchOptions> {
  if (!launchOptionsPromise) {
    launchOptionsPromise = (async () => {
      try {
        const raw = await invoke<Partial<LaunchOptions>>('launch_options')
        return { mock: raw.mock === true, project: raw.project ?? null, binary: raw.binary ?? null, autotest: raw.autotest ?? null }
      } catch {
        return { mock: false, project: null, binary: null, autotest: null }
      }
    })()
  }
  return launchOptionsPromise
}

/** `fs_file_info`'s result. */
interface FileInfo {
  size: number
  is_dir: boolean
}

async function fileInfo(path: string): Promise<FileInfo | null> {
  try {
    return await invoke<FileInfo>('fs_file_info', { path })
  } catch {
    return null
  }
}

/** The folder holding `path` (either separator). */
function parentPath(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, '')
  const slash = Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\'))
  if (slash < 0) return trimmed
  if (slash === 0) return trimmed.slice(0, 1)
  const parent = trimmed.slice(0, slash)
  return /^[A-Za-z]:$/.test(parent) ? parent + trimmed.charAt(slash) : parent
}

/// `AppState.projectFolder(_:)`: the folder `openProject` opens for `path`:
/// the path itself, or the folder holding it when it is a file.
export async function projectFolder(path: string): Promise<string> {
  const target = standardizePath(path)
  const info = await fileInfo(target)
  if (info && !info.is_dir) return parentPath(target)
  return target
}

/** Whether `path` is absolute on this platform's terms (POSIX, drive letter or UNC). */
function isAbsolutePath(path: string): boolean {
  return path.startsWith('/') || /^[A-Za-z]:[\\/]/.test(path) || path.startsWith('\\\\')
}

/** `(root as NSString).appendingPathComponent(path)` with the root's separator. */
function joinPath(root: string, path: string): string {
  const separator = root.includes('\\') && !root.includes('/') ? '\\' : '/'
  const base = root.replace(/[\\/]+$/, '')
  const rest = path.replace(/^[\\/]+/, '')
  return base + separator + (separator === '\\' ? rest.replace(/\//g, '\\') : rest)
}

/// `AppState.relativePath(_:root:)`: `absolute` relative to `root` when it is
/// inside it, else unchanged.
export function relativePath(absolute: string, root: string): string {
  const separator = root.includes('\\') && !root.includes('/') ? '\\' : '/'
  const normalizedRoot = root.endsWith('/') || root.endsWith('\\') ? root : root + separator
  if (absolute.startsWith(normalizedRoot)) return absolute.slice(normalizedRoot.length).replace(/\\/g, '/')
  return absolute
}

/** `AppState.approvalModeLabel(_:)`: "Ask", "Auto", "Bypass". */
export function approvalModeLabel(profile: string): string {
  return permissionLabel(profile)
}

/// `restoreRank(of:)`: a remembered project's position at launch; later
/// opens sort last.
export function restoreRank(root: string): number {
  const index = restoreOrder.indexOf(root)
  return index < 0 ? Number.MAX_SAFE_INTEGER : index
}

/// `browsers[projectId]` without creating one (`browserDriven` reads it);
/// null when the project's Browser tab never opened.
export function existingBrowser(projectId: string | null | undefined): BrowserWorkspace | null {
  if (projectId == null) return null
  return browserWorkspaces.get(projectId) ?? null
}

/** `terminals[projectId]` without creating one. */
export function existingTerminals(projectId: string | null | undefined): TerminalWorkspace | null {
  if (projectId == null) return null
  return terminalWorkspaces.get(projectId) ?? null
}

/** `mockRequested`: `--mock` / `CTX_DESKTOP_MOCK=1` (parsed by the shell) or Settings → General. */
function mockRequested(launch: LaunchOptions): boolean {
  return launch.mock || getPref('useMockBackend')
}

async function secretIsSet(account: string): Promise<boolean> {
  try {
    const value = await invoke<string | null>('secret_get', { account })
    return value !== null && value.length > 0
  } catch {
    return false
  }
}

// MARK: - Setters

function setSidebarItem(item: SidebarItem): void {
  patch({ sidebarItem: item })
  // `sidebarItem.didSet`: the project tab is remembered per project.
  if (!isProjectTab(item)) return
  const project = selectedProject(get())
  if (!project) return
  if (project.lastTab !== item) {
    project.lastTab = item
    project.touch()
  }
  setLastTab(project.id, item)
}

function setSidebarVisible(visible: boolean): void {
  patch({ sidebarVisible: visible })
  setPref('sidebarVisible', visible)
}

function setInspectorVisible(visible: boolean): void {
  patch({ inspectorVisible: visible })
  setPref('inspectorVisible', visible)
}

function setInspectorWidth(width: number): void {
  patch({ inspectorWidth: width })
  setPref('inspectorWidth', width)
}

function focusComposer(): void {
  patch({ composerFocusToken: get().composerFocusToken + 1 })
}

function setPendingDraft(projectId: string, text: string): void {
  const next = { ...get().pendingDraft }
  if (text.length === 0) delete next[projectId]
  else next[projectId] = text
  patch({ pendingDraft: next })
}

function setPendingAttachments(projectId: string, attachments: ComposerAttachment[]): void {
  const next = { ...get().pendingAttachments }
  if (attachments.length === 0) delete next[projectId]
  else next[projectId] = attachments
  patch({ pendingAttachments: next })
}

function clearPending(projectId: string): void {
  const state = get()
  if (!(projectId in state.pendingDraft) && !(projectId in state.pendingAttachments)) return
  const drafts = { ...state.pendingDraft }
  const attachments = { ...state.pendingAttachments }
  delete drafts[projectId]
  delete attachments[projectId]
  patch({ pendingDraft: drafts, pendingAttachments: attachments })
}

function setCollapsed(collapsed: boolean, project: ProjectState): void {
  setProjectCollapsed(project.root, collapsed)
  bumpStore()
}

// MARK: - Workspaces

/** The project's browser page, created (blank) on first use. */
function browser(project: ProjectState): BrowserWorkspace {
  const existing = browserWorkspaces.get(project.id)
  if (existing) return existing
  const workspace = new BrowserWorkspace(project.id)
  browserWorkspaces.set(project.id, workspace)
  // A view may ask for it while rendering: tell the store afterwards.
  queueMicrotask(bumpStore)
  return workspace
}

/** The project's terminal tabs, created (with no shell yet) on first use. */
function terminals(project: ProjectState): TerminalWorkspace {
  const existing = terminalWorkspaces.get(project.id)
  if (existing) return existing
  const workspace = new TerminalWorkspace(project.root, project.host)
  terminalWorkspaces.set(project.id, workspace)
  queueMicrotask(bumpStore)
  return workspace
}

/// Runs `command` in a new terminal tab of the selected project and shows
/// the Inspector Terminal tab (the play button on shell code blocks).
function runInTerminal(command: string): void {
  const project = selectedProject(get())
  if (!project) return
  terminals(project).newSession(command)
  setInspectorVisible(true)
  patch({ inspectorTab: 'terminal' })
}

/// Shows a sub-agent of `chat` in the Inspector Agents tab (clicking an
/// agent row in the transcript).
function showAgent(agentId: string, chat: ChatState): void {
  chat.setInspectedAgent(agentId)
  setInspectorVisible(true)
  patch({ inspectorTab: 'agents' })
}

// MARK: - Backend lifecycle

/// True the first time it is called: the launch-time `startBackend()` runs
/// once per window load, however often the root component mounts.
function claimBackendStart(): boolean {
  if (backendStartClaimed) return false
  backendStartClaimed = true
  return true
}

/// Locate → environment → spawn → initialize → protocol check → reopen
/// projects (remembered ones on first start; in place on a restart).
async function startBackend(): Promise<void> {
  if (get().backendStarting) return
  patch({ backendStarting: true, connection: { kind: 'connecting' } })
  try {
    await startBackendSteps()
  } finally {
    patch({ backendStarting: false })
  }
}

async function startBackendSteps(): Promise<void> {
  const launch = await launchOptions()
  if (get().projects.length === 0) await listRememberedProjects(launch)

  // `--binary` for this launch, else Settings → General's override (the
  // shell's locator tries it first, then the usual places).
  const binaryOverride = launch.binary ?? getPref('backendPathOverride') ?? undefined
  const mock = mockRequested(launch)
  let apiKeyEnv = get().config?.model.api_key_env ?? 'OPENAI_API_KEY'

  let result: InitializeResult
  try {
    result = await backend.start({ mock, apiKeyEnv: mock ? undefined : apiKeyEnv, binaryOverride })
    // The configured key variable is only known after initialize: retry once with the right one.
    if (!result.mock && !result.model.api_key_set && result.model.api_key_env !== apiKeyEnv) {
      apiKeyEnv = result.model.api_key_env
      if (await secretIsSet(apiKeyEnv)) {
        result = await backend.start({ mock, apiKeyEnv, binaryOverride })
      }
    }
  } catch (error) {
    patch({ connection: { kind: 'failed', failure: backend.lastStartFailure ?? failureOf(error) } })
    abandonRestore()
    // The hosts do not depend on the local ctx: their projects still come back.
    void reopenRememberedRemoteProjects()
    return
  }
  patch({ config: result, connection: { kind: 'ready', result } })
  lastSuccessfulStart = Date.now()

  // A new backend may see a different key or endpoint: refetch models.
  for (const project of get().projects) {
    if (project.isRemote) continue
    project.models.baseURL = result.model.base_url
    project.models.invalidate()
    project.touch()
  }
  if (get().projects.some((project) => !project.isRemote)) {
    await reopenProjectsInPlace()
  } else {
    await reopenRememberedProjects(launch)
  }
  // The remembered projects on hosts, each host in parallel, without
  // holding the start up (once per launch).
  void reopenRememberedRemoteProjects()
  // A remembered ModelGateway account: refresh its summary and usage through
  // the new backend, without holding the start up.
  for (const listener of [...backendStartedListeners]) {
    void Promise.resolve()
      .then(listener)
      .catch((error: unknown) => debug(() => `backend-started listener failed: ${errorText(error)}`))
  }
}

/// Stop, start, `project/open` for every open project, `sessions/list` for
/// the selected one.
async function restartBackend(): Promise<void> {
  if (get().backendStarting) return
  // The old child still emits task/finished while it shuts down: with the
  // connection already marked as restarting, `dequeue` keeps queued prompts
  // instead of sending them into a process that stopped reading.
  patch({ connection: { kind: 'connecting' } })
  batch(() => {
    for (const project of get().projects) {
      if (project.isRemote) continue
      for (const chat of project.allChats) {
        if (!chat.isRunning) continue
        chat.finish(
          { project_id: project.id, session_id: chat.sessionId, task_id: null, status: 'cancelled', error: 'Backend restarted', error_kind: 'backend_restarted' },
          'stopped',
        )
      }
      project.runningSessionIds = new Set()
      project.switchingModel = null
      project.touch()
    }
  })
  refreshAttention()
  await backend.stop()
  await startBackend()
}

/// Lists the projects launch will reopen (`--project`, else the remembered
/// open folders that still exist) as opening, before the backend is up, and
/// shows the Chat tab's skeleton instead of Welcome while they load.
async function listRememberedProjects(launch: LaunchOptions): Promise<void> {
  const keys: string[] = []
  if (launch.project !== null) {
    keys.push(await projectFolder(launch.project))
  } else {
    const remembered = getPref('openProjectPaths')
    const infos = await Promise.all(remembered.map((key) => (isRemoteProjectKey(key) ? Promise.resolve(null) : fileInfo(key))))
    remembered.forEach((key, index) => {
      if (isRemoteProjectKey(key)) {
        // A folder on a host is listed under its host; one whose host is
        // gone from Settings is forgotten.
        const ref = decodeRemoteProject(key)
        if (ref && SSHHosts.host(ref.hostId)) keys.push(key)
        else forgetOpenProject(key)
      } else if (infos[index]?.is_dir === true) keys.push(standardizePath(key))
      else forgetOpenProject(key)
    })
  }
  restoreOrder = keys
  patch({ openingProjectPaths: keys })
  if (keys.length > 0 && get().sidebarItem === 'welcome') setSidebarItem('chat')
}

/// The local backend did not start: none of the local folders listed by
/// `listRememberedProjects` is coming (the hosts' folders still are).
function abandonRestore(): void {
  const local = restoreOrder.filter((key) => !isRemoteProjectKey(key))
  if (local.length === 0) return
  const order = new Set(local)
  patch({ openingProjectPaths: get().openingProjectPaths.filter((path) => !order.has(path)) })
  restoreOrder = restoreOrder.filter((key) => isRemoteProjectKey(key))
  const state = get()
  if (state.projects.length === 0 && state.sidebarItem === 'chat' && state.openingProjectPaths.length === 0) setSidebarItem('welcome')
}

/// Opens the remembered projects in parallel (`ctx serve` handles each
/// request on its own task) and selects the last selected one as soon as it
/// is open rather than after the slowest of them.
async function reopenRememberedProjects(launch: LaunchOptions): Promise<void> {
  const paths = restoreOrder.filter((key) => !isRemoteProjectKey(key))
  try {
    const first = paths[0]
    if (launch.project !== null && first !== undefined) {
      await openProject(first, { select: true, restoreSession: false })
      return
    }
    const last = getPref('lastSelectedProjectId')
    await Promise.all(
      paths.map(async (path) => {
        const project = await openProject(path, { select: false, restoreSession: false })
        if (project && project.id === last && get().selectedProjectId === null) selectRestoredProject(project.id)
      }),
    )
    if (get().selectedProjectId === null) {
      const project = get().projects[0]
      if (project) selectRestoredProject(project.id)
      else if (get().sidebarItem === 'chat' && !restoreOrder.some((key) => isRemoteProjectKey(key))) setSidebarItem('welcome')
    }
  } finally {
    // The hosts' folders keep their ranks until their own reopen is done.
    restoreOrder = restoreOrder.filter((key) => isRemoteProjectKey(key))
  }
}

/// Selects a project reopened at launch without pulling the user off
/// Welcome, Projects or Account if they went there while it loaded.
function selectRestoredProject(id: string): void {
  const item = get().sidebarItem
  selectProject(id)
  if (!isProjectTab(item)) setSidebarItem(item)
}

async function reopenProjectsInPlace(): Promise<void> {
  for (const project of get().projects) {
    if (project.isRemote) continue
    try {
      const info = await backend.projectOpen(project.root, false, getPref('autoInitGit'))
      project.info = info
      project.runningSessionIds = runningSessionIds(info)
      project.indexProgress = null
      project.lastIndexReady = null
      project.touch()
      await restorePermissionProfile(project)
      void prefetchModels(project)
    } catch (error) {
      const message = errorText(error)
      toast(`Could not reopen ${project.name}: ${message}`, 'error', message)
    }
  }
  const project = selectedProject(get())
  if (project && !project.isRemote) {
    await project.loadSessions()
    void (async () => {
      await project.refreshGit()
      await project.loadExtras()
    })()
  }
}

// MARK: - Projects

/// The directory picker (`NSOpenPanel`, directories only) → `openProject`.
/// `newProject` lets the panel create the folder (Projects → New project).
async function chooseProject(newProject = false): Promise<void> {
  let picked: string | string[] | null = null
  try {
    picked = await openDialog({
      directory: true,
      multiple: false,
      canCreateDirectories: newProject,
      title: newProject ? 'Choose or create a folder for the new project' : 'Choose a repository folder',
    })
  } catch {
    try {
      picked = await invoke<string | null>('fs_pick_project')
    } catch {
      picked = null
    }
  }
  const path = Array.isArray(picked) ? picked[0] : picked
  if (!path) return
  await openProject(path)
}

/// `openProject(path:host:select:restoreSession:)`: `project/open` for the
/// folder (`projectFolder` locally; as given on a host, whose connection is
/// made first), registered, remembered, selected (a new chat unless
/// `restoreSession`) and loaded. Returns the project, null on failure.
async function openProject(path: string, options: OpenProjectOptions = {}): Promise<ProjectState | null> {
  const select = options.select ?? true
  const restoreSession = options.restoreSession ?? false
  const host = options.host ?? null
  const target = host === null ? await projectFolder(path) : path
  const key = projectKey(target, host?.id ?? null)
  {
    const state = get()
    if (!state.openingProjectPaths.includes(key) && !state.projects.some((project) => project.key === key)) {
      patch({ openingProjectPaths: [...state.openingProjectPaths, key] })
    }
  }
  const removeOpening = () => {
    const opening = get().openingProjectPaths
    if (opening.includes(key)) patch({ openingProjectPaths: opening.filter((entry) => entry !== key) })
  }
  try {
    let client: BackendClient
    if (host !== null) {
      const connection = await connectHost(host)
      if (!connection) return null
      client = connection.client
    } else {
      if (!isReady(get())) {
        toast('The backend is not running; open a project once it is connected.', 'warning')
        return null
      }
      client = backend
    }
    try {
      const info = await client.projectOpen(target, false, getPref('autoInitGit'))
      // One repository id per app: the same path on two hosts hashes to the
      // same id and would mix their events.
      const clash = projectById(get(), info.project_id)
      if (clash && (clash.host?.id ?? null) !== (host?.id ?? null)) {
        const where = clash.host === null ? 'locally' : `on ${hostDisplayName(clash.host)}`
        toast(`${info.name} is already open ${where} at the same path; close it first.`, 'warning')
        return null
      }
      const project = register(info, host)
      // The real row replaces the loading one now, even when the backend
      // resolved the folder to a different repository root.
      removeOpening()
      // A worktree is listed under its parent, not among the recents.
      if (!project.isWorktree) RecentProjects.touch(info.root, info.name, host)
      if (info.git_initialized === true) toast(`Initialized a Git repository in ${info.name} so turns can be undone.`, 'info')
      await restorePermissionProfile(project)
      if (select) {
        selectProject(project.id, restoreSession)
        if (!restoreSession) newChat()
      }
      startLoads(project)
      return project
    } catch (error) {
      const message = errorText(error)
      const location = host === null ? '' : ` on ${hostDisplayName(host)}`
      toast(`Could not open ${lastPathComponent(target)}${location}: ${message}`, 'error', message)
      return null
    }
  } finally {
    removeOpening()
  }
}

/** `openRecent(_:select:restoreSession:)`: opens a remembered project, local or remote. */
async function openRecent(recent: RecentProject, options: Omit<OpenProjectOptions, 'host'> = {}): Promise<ProjectState | null> {
  if (recent.hostId !== null) {
    const host = SSHHosts.host(recent.hostId)
    if (!host) {
      toast(`The host of ${recent.name} is no longer configured; add it again in Settings → Remote hosts.`, 'warning')
      return null
    }
    return openProject(recent.path, { ...options, host })
  }
  return openProject(recent.path, options)
}

/// Adopts a `project/open` (or `worktree/create`) result: reuses the open
/// `ProjectState` for that id, else creates it (on `host`'s connection when
/// given), replays the notifications that beat the response, and remembers
/// it for the next launch.
function register(info: ProjectInfo, host: SSHHost | null = null): ProjectState {
  let project = projectById(get(), info.project_id)
  if (project) {
    project.info = info
    project.touch()
  } else {
    project = makeProject(info, host)
    // A project reopened at launch goes to its remembered position.
    const rank = restoreRank(project.key)
    const list = [...get().projects]
    const index = list.findIndex((candidate) => restoreRank(candidate.key) > rank)
    list.splice(index < 0 ? list.length : index, 0, project)
    patch({ projects: list })
    replayBufferedNotifications(project.id)
  }
  rememberOpenProject(project.key)
  return project
}

/// The background loads every freshly opened project starts. The chat list
/// does not wait for Git: `ctx serve` answers both at once.
function startLoads(project: ProjectState): void {
  void prefetchModels(project)
  void project.loadSessions()
  void (async () => {
    await project.refreshGit()
    await project.loadExtras()
  })()
}

async function closeProject(id: string, forget = true): Promise<void> {
  const project = projectById(get(), id)
  if (!project) return
  // Its open worktrees go with it (their checkouts and chats are kept).
  for (const child of worktreesOf(get(), project)) await closeProject(child.id, forget)
  const timer = gitRefreshTimers.get(id)
  if (timer !== undefined) clearTimeout(timer)
  gitRefreshTimers.delete(id)
  terminalWorkspaces.get(id)?.terminateAll()
  terminalWorkspaces.delete(id)
  const page = browserWorkspaces.get(id)
  browserWorkspaces.delete(id)
  if (page) void page.tearDown()
  for (const listener of [...projectClosedListeners]) {
    try {
      listener(id)
    } catch (error) {
      debug(() => `project-closed listener threw: ${errorText(error)}`)
    }
  }
  if (project.backend.isReady) {
    try {
      await project.backend.projectClose(id)
    } catch (error) {
      const message = errorText(error)
      toast(`Could not close ${project.name}: ${message}`, 'error', message)
    }
  }
  project.dispose()
  patch({ projects: get().projects.filter((candidate) => candidate.id !== id) })
  if (forget) forgetOpenProject(project.key)
  if (get().selectedProjectId === id) {
    patch({ selectedProjectId: null, selectedSessionId: null })
    const next = get().projects[0]
    if (next) {
      selectProject(next.id)
    } else {
      setPref('lastSelectedProjectId', null)
      setSidebarItem('welcome')
    }
  }
  refreshAttention()
}

/// Shows the profile page (the sidebar's account button) on `tab`; with no
/// tab, on the last settings tab, or General when the last one was Account.
/// The settings shortcut and every "Open Settings" link land here.
function openSettings(tab?: string | null): void {
  if (tab != null) setPref('settingsTab', tab)
  else if (getPref('settingsTab') === SETTINGS_TAB_ACCOUNT) setPref('settingsTab', SETTINGS_TAB_GENERAL)
  setSidebarItem('account')
}

/// `select(project:restoreSession:)`: selects the project, its remembered
/// tab and (with `restoreSession`) its remembered chat, waiting for the chat
/// list when it is still loading.
function selectProject(id: string, restoreSession = true): void {
  const project = projectById(get(), id)
  if (!project) return
  const last = restoreSession ? (getPref('lastSelectedSession')[id] ?? null) : null
  // With the chat list already loaded, the remembered chat is selected in
  // this same update: clearing it first and restoring it later flashed
  // "New chat" and the empty state in between.
  const ready = last !== null && project.sessionsLoad === null ? (project.session(last)?.id ?? null) : null
  if (get().selectedProjectId !== id) {
    patch({ selectedProjectId: id, selectedSessionId: ready, restoringSessionId: null })
    setPref('lastSelectedProjectId', id)
  }
  setSidebarItem(isProjectTab(project.lastTab) ? project.lastTab : 'chat')
  focusComposer()
  refreshAttention()
  if (last === null) return
  if (ready !== null && get().selectedSessionId === ready) {
    // Already showing; this loads its history and clears its unseen mark.
    void selectSession(ready)
    return
  }
  patch({ restoringSessionId: last })
  void (async () => {
    try {
      // Wait for a `sessions/list` already in flight (openProject starts one)
      // instead of skipping it: the remembered session is only known once
      // the list has arrived.
      if (project.sessionsLoad) await project.sessionsLoad
      else if (project.sessions.length === 0) await project.loadSessions()
      const state = get()
      if (state.selectedProjectId !== id || state.selectedSessionId !== null || state.restoringSessionId !== last || project.session(last) === null) return
      await selectSession(last)
    } finally {
      // Cleared by newChat or picking another chat or project meanwhile.
      if (get().restoringSessionId === last) patch({ restoringSessionId: null })
    }
  })()
}

// MARK: - Sessions

/** `select(session:)`: loads history only when the transcript is empty and the chat idle; clears unseen. */
async function selectSession(id: string | null): Promise<void> {
  const project = selectedProject(get())
  if (!project) {
    patch({ selectedSessionId: null })
    return
  }
  patch({ selectedSessionId: id, restoringSessionId: null })
  setSidebarItem('chat')
  focusComposer()
  setLastSelectedSession(project.id, id)
  if (id === null) {
    refreshAttention()
    return
  }
  const chat = project.chat(id)
  if (chat.unseen !== null) chat.update((c) => (c.unseen = null))
  notifier.clearDelivered(id)
  refreshAttention()

  if (chat.transcript.isEmpty && chat.status.kind === 'idle') {
    chat.update((c) => (c.status = { kind: 'loadingHistory' }))
    try {
      const history = await project.backend.sessionHistory(project.id, id)
      if (statusKind(chat) === 'loadingHistory') {
        chat.loadHistory(history)
      } else {
        // A run started meanwhile (Return was pressed, or a queued prompt
        // was dequeued): keep the new turn and put the earlier conversation
        // in front of it.
        chat.prependHistory(history)
      }
    } catch (error) {
      chat.update((c) => {
        c.historyError = errorText(error)
        if (c.status.kind === 'loadingHistory') c.status = { kind: 'idle' }
      })
    }
    if (project.isRunningSession(id) && !chat.isRunning) {
      const info = project.info
      const running = info.running_tasks?.find((task) => task.session_id === id) ?? (info.running_task?.session_id === id ? info.running_task : null)
      const startedAt = (running ? parseRFC3339(running.started_at) : null) ?? new Date()
      chat.update((c) => (c.status = { kind: 'running', progress: { state: 'INVESTIGATING', step: 0, startedAt } }))
    }
  }
}

/** Empty state for the selected project; Welcome when none is open. */
function newChat(): void {
  const project = selectedProject(get())
  if (!project) {
    setSidebarItem('welcome')
    return
  }
  patch({ selectedSessionId: null, restoringSessionId: null })
  setLastSelectedSession(project.id, null)
  setSidebarItem('chat')
  focusComposer()
}

/// `suggest/next` for a chat whose run just completed; the answer is kept
/// only if the composer is still empty and the chat idle.
function requestSuggestion(chat: ChatState, projectId: string): void {
  chat.setSuggestion(null)
  const project = projectById(get(), projectId)
  if (!getPref('suggestNext') || !project || !isProjectReady(get(), project)) return
  const sessionId = chat.sessionId
  void (async () => {
    let text: string | null
    try {
      text = await project.backend.suggestNext(projectId, sessionId)
    } catch {
      return
    }
    if (text === null) return
    if (chat.draft.length > 0 || chat.isRunning || chat.queued.length > 0) return
    chat.setSuggestion(text)
  })()
}

/// `suggest/starters` for the empty chat, at most every ten minutes per
/// project. Fire-and-forget: nothing waits on it, so opening and sending
/// chats are never held up; a failed or empty answer falls back to
/// `starterFallback`.
function loadStarters(project: ProjectState): void {
  if (project.startersLoading) return
  const state = get()
  if (!isProjectReady(state, project) || needsAPIKeyFor(state, project)) {
    if (project.starters === null) project.update((p) => (p.starters = starterFallback(p.info)))
    return
  }
  const loaded = project.startersLoadedAt
  if (loaded !== null && Date.now() - loaded.getTime() < STARTERS_MAX_AGE_MS) return
  project.update((p) => (p.startersLoading = true))
  const projectId = project.id
  void (async () => {
    let generated: Awaited<ReturnType<typeof backend.suggestStarters>> = []
    try {
      generated = await project.backend.suggestStarters(projectId)
    } catch {
      generated = []
    }
    project.update((p) => {
      p.starters = generated.length === 0 ? starterFallback(p.info) : generated
      p.startersLoadedAt = new Date()
      p.startersLoading = false
    })
  })()
}

/// Sends a prompt with its attachments (`prepareAttachmentPrompt`); one with
/// attachments but no text gets a short stand-in text. Creates the session
/// when needed. While the chat runs, Return steers the running turn and
/// mod+Return (`queue`) waits for it to end.
async function send(input: string | OutgoingPrompt, options: { queue?: boolean } = {}): Promise<void> {
  const queue = options.queue ?? false
  const source = typeof input === 'string' ? outgoingPrompt(input) : input
  const prompt: OutgoingPrompt = { text: source.text.trim(), images: [...source.images], files: [...source.files] }
  if (prompt.text.length === 0 && prompt.images.length === 0 && prompt.files.length === 0) return
  if (prompt.text.length === 0) {
    prompt.text = prompt.files.length === 0 && prompt.images.length === 1 ? 'See the attached image.' : 'See the attached files.'
  }
  selectedChat(get())?.setSuggestion(null)
  const project = selectedProject(get())
  if (!project) {
    setSidebarItem('welcome')
    return
  }
  if (!isProjectReady(get(), project)) {
    toast(project.isRemote ? `${project.hostLabel ?? 'The host'} is not connected.` : 'The backend is not running.', 'error')
    return
  }
  let chat: ChatState
  const sessionId = get().selectedSessionId
  if (sessionId !== null) {
    chat = project.chat(sessionId)
  } else {
    const created = await createSession(project)
    if (!created) return
    chat = created
  }
  chat.update((c) => {
    c.draft = ''
    c.attachments = []
  })
  clearPending(project.id)
  // Other sessions of the project may be running; only this chat's own run
  // makes the prompt wait.
  const question = chat.pendingQuestion
  if (project.isRunningSession(chat.sessionId) && !queue && question !== null && prompt.images.length === 0 && prompt.files.length === 0) {
    // The run waits on the agent's questions, so it would never read a
    // steered message: the text answers them instead.
    await respondQuestion(question.request_id, chat, null, prompt.text)
  } else if (project.isRunningSession(chat.sessionId)) {
    if (queue) chat.enqueue(prompt)
    else await steer(prompt, chat, project)
  } else {
    await run(prompt, chat, project)
  }
  focusComposer()
}

/// `task/steer`: the prompt waits above the composer until the `steered`
/// event shows the model read it. A run that is already ending refuses it;
/// it then runs as the next turn.
async function steer(prompt: OutgoingPrompt, chat: ChatState, project: ProjectState): Promise<void> {
  chat.update((c) => (c.steering = [...c.steering, prompt]))
  try {
    const { images, files } = await attachmentsFor(project, prompt)
    await project.backend.taskSteer(project.id, chat.sessionId, prompt.text, images, files)
  } catch (error) {
    // `task/finished` may have moved it to the queue meanwhile.
    const index = chat.steering.indexOf(prompt)
    if (index < 0) return
    chat.update((c) => (c.steering = c.steering.filter((_, i) => i !== index)))
    if (error instanceof BackendError && error.isNoTaskRunning) {
      if (project.isRunningSession(chat.sessionId)) chat.requeueFirst(prompt)
      else await run(prompt, chat, project)
    } else {
      void moveToComposer(prompt, chat)
      const message = errorText(error)
      toast(`Could not send the message to the running task: ${message}`, 'error', message)
    }
  }
}

/** Re-sends the chat's last user message. */
async function retry(chat: ChatState): Promise<void> {
  const prompt = chat.transcript.lastUserPrompt
  const project = projectById(get(), chat.projectId)
  if (!prompt || !project) return
  if (get().selectedProjectId !== project.id) selectProject(project.id, false)
  if (get().selectedSessionId !== chat.sessionId) await selectSession(chat.sessionId)
  if (project.isRunningSession(chat.sessionId)) chat.enqueue(prompt)
  else await run(prompt, chat, project)
}

/** Stops the selected chat's run; other sessions of the project keep going. */
async function cancelRunningTask(): Promise<void> {
  const project = selectedProject(get())
  const sessionId = get().selectedSessionId
  if (!project || sessionId === null || !project.isRunningSession(sessionId)) return
  const chat = project.chat(sessionId)
  chat.update((c) => (c.cancelRequested = true))
  try {
    await project.backend.taskCancel(project.id, sessionId)
  } catch (error) {
    chat.update((c) => (c.cancelRequested = false))
    if (error instanceof BackendError && error.isNoTaskRunning) {
      // The backend has no run for this session (a stale running_tasks
      // entry): no task/finished will ever come, so settle the chat here.
      if (chat.isRunning) {
        chat.finish({ project_id: project.id, session_id: chat.sessionId, task_id: null, status: 'cancelled', error: null, error_kind: null }, 'stopped')
      }
      project.removeRunning(sessionId)
      project.touch()
      refreshAttention()
    } else {
      const message = errorText(error)
      toast(`Could not cancel the task: ${message}`, 'error', message)
    }
  }
}

function removeQueued(index: number, chat: ChatState): void {
  chat.removeQueued(index)
}

/** Queue row click: moves the item back into the composer. */
function editQueued(index: number, chat: ChatState): void {
  const prompt = chat.queued[index]
  if (prompt === undefined) return
  chat.removeQueued(index)
  void moveToComposer(prompt, chat)
  focusComposer()
}

/// The text goes into the draft and pasted images and documents back onto
/// it as chips (attached files are still referenced in the text).
async function moveToComposer(prompt: OutgoingPrompt, chat: ChatState): Promise<void> {
  chat.update((c) => (c.draft = c.draft.length === 0 ? prompt.text : c.draft + '\n' + prompt.text))
  let pasted: ComposerAttachment[]
  try {
    pasted = await pastedAttachments(prompt)
  } catch {
    pasted = []
  }
  if (pasted.length === 0) return
  const merged = [...chat.attachments]
  for (const attachment of pasted) {
    if (!merged.some((entry) => entry.id === attachment.id)) merged.push(attachment)
  }
  chat.setAttachments(merged)
}

async function createSession(project: ProjectState): Promise<ChatState | null> {
  try {
    const summary = await project.backend.sessionCreate(project.id, null)
    project.upsertSession(summary)
    const chat = project.chat(summary.id)
    if (get().selectedProjectId === project.id) {
      patch({ selectedSessionId: summary.id })
      setLastSelectedSession(project.id, summary.id)
      setSidebarItem('chat')
    }
    return chat
  } catch (error) {
    const message = errorText(error)
    toast(`Could not create a chat: ${message}`, 'error', message)
    return null
  }
}

/** Appends the optimistic user item and calls `task/run`. */
async function run(prompt: OutgoingPrompt, chat: ChatState, project: ProjectState): Promise<void> {
  const item = makeUserTurn({ text: prompt.text, images: prompt.images, files: prompt.files })
  chat.update((c) => {
    c.transcript.append(item)
    c.pendingUserItemId = item.id
    c.lastError = null
    c.lastErrorKind = null
    c.historyError = null
    c.unseen = null
    c.status = { kind: 'running', progress: { state: 'PREPARING', step: 0, startedAt: new Date() } }
  })
  project.addRunning(chat.sessionId)
  project.touchSession(chat.sessionId, prompt.text)
  refreshAttention()

  const options: TaskRunOptions = {
    proactive_retrieval: getPref('useContextEngine') ? null : false,
    debug_prompt: getPref('developerMode') ? true : null,
  }
  try {
    const { images, files } = await attachmentsFor(project, prompt)
    await project.backend.taskRun(project.id, chat.sessionId, prompt.text, options, images, files)
  } catch (error) {
    const backendError = error instanceof BackendError ? error : null
    if (backendError?.isTaskAlreadyRunning) {
      // The session is still running (a stale view): queue instead of failing.
      removeUserItem(item.id, chat)
      chat.update((c) => (c.status = { kind: 'idle' }))
      project.removeRunning(chat.sessionId)
      chat.requeueFirst(prompt)
      await project.refreshStatus()
    } else if (backendError?.isConnectionLoss) {
      // The backend went away between the dequeue and the request (a
      // restart in progress): keep the prompt for the next run instead of
      // losing it behind an error notice.
      removeUserItem(item.id, chat)
      project.removeRunning(chat.sessionId)
      chat.update((c) => {
        c.status = { kind: 'idle' }
        c.transcript.append(makeNoticeItem({ lines: ['Backend connection lost; the message stays queued.'] }))
      })
      chat.requeueFirst(prompt)
      chat.addLog(`queued after a connection loss: ${ChatState.clip(prompt.text, 60)}`, new Date(), 'warn')
    } else {
      const message = errorText(error)
      project.removeRunning(chat.sessionId)
      chat.update((c) => {
        c.transcript.append(makeNoticeItem({ lines: [message], isError: true }))
        c.lastError = message
        if (backendError?.kind != null) c.lastErrorKind = backendError.kind
        c.pendingUserItemId = null
        c.status = { kind: 'idle' }
      })
      chat.addLog(`error: ${message}`, new Date(), 'error')
    }
    project.touch()
    refreshAttention()
  }
}

/// What `task/run` / `task/steer` get for the prompt's attachments: the
/// local paths for the local backend; for a project on a host, the images'
/// bytes and the text files' content (`RemoteAttachments.prepare`), since
/// the backend there cannot open the app's files. The transcript keeps the
/// local paths for its thumbnails.
async function attachmentsFor(project: ProjectState, prompt: OutgoingPrompt): Promise<{ images: Array<string | InlineImage>; files: FileUpload[] }> {
  if (!project.isRemote) return { images: prompt.images, files: prompt.files }
  const prepared = await prepareRemoteAttachments(prompt.images, prompt.files)
  if (prepared.skipped.length > 0) {
    toast(`${prepared.skipped.length} attachment${prepared.skipped.length === 1 ? ' was' : 's were'} not sent: ${prepared.skipped.map(lastPathComponent).join(', ')}`, 'warning')
  }
  return { images: prepared.inlineImages, files: prepared.files }
}

/** `chat.status`'s case, read afresh (TypeScript keeps an earlier narrowing across awaits). */
function statusKind(chat: ChatState): ChatState['status']['kind'] {
  return chat.status.kind
}

function removeUserItem(id: string, chat: ChatState): void {
  chat.update((c) => {
    if (c.transcript.last?.id === id) c.transcript.removeLast()
    c.pendingUserItemId = null
  })
}

async function renameSession(id: string, title: string): Promise<void> {
  const project = selectedProject(get())
  if (!project) return
  try {
    const summary = await project.backend.sessionRename(project.id, id, title.trim())
    project.upsertSession(summary)
  } catch (error) {
    const message = errorText(error)
    toast(`Could not rename the chat: ${message}`, 'error', message)
  }
}

async function deleteSession(id: string): Promise<void> {
  const project = selectedProject(get())
  if (!project) return
  try {
    await project.backend.sessionDelete(project.id, id)
    project.removeSession(id)
    if (get().selectedSessionId === id) newChat()
    refreshAttention()
  } catch (error) {
    const message = errorText(error)
    toast(`Could not delete the chat: ${message}`, 'error', message)
  }
}

// MARK: - Rewind (edit a sent message · undo a turn)

/// Whether the chat can be rewound right now: its session idle and its
/// history loaded (other sessions may run; a rewind restores only this
/// session's files).
function canRewind(chat: ChatState): boolean {
  return canRewindIn(get(), chat)
}

/** Hover pencil / context menu: the bubble becomes an editor. */
function beginEdit(turn: UserTurn, chat: ChatState): void {
  if (!userTurnIsRewindable(turn)) {
    toast('This message cannot be edited yet.', 'info')
    return
  }
  if (!canRewind(chat)) {
    toast('Wait for the running task to finish before editing a message.', 'info')
    return
  }
  chat.update((c) => {
    c.editingTurnId = turn.id
    c.editingText = turn.text
  })
}

function cancelEdit(chat: ChatState): void {
  chat.update((c) => {
    c.editingTurnId = null
    c.editingText = ''
  })
  focusComposer()
}

/** Send of an inline edit: asks keep-or-revert, then rewinds and re-sends. */
function submitEdit(chat: ChatState): void {
  const id = chat.editingTurnId
  if (id === null) return
  const index = chat.transcript.locateUser(id)
  if (index === null) return
  const turn = chat.transcript.at(index)
  if (!turn || turn.kind !== 'user') return
  const text = chat.editingText.trim()
  if (text.length === 0) return
  if (!canRewind(chat)) {
    toast('Wait for the running task to finish before sending the edit.', 'info')
    return
  }
  patch({ rewindRequest: makeRewindRequest(chat, turn, index, { kind: 'edit', newText: text }) })
}

/// Chat → Undo Last Turn (mod+alt+Z): the newest message the user typed
/// goes, with everything after it, and comes back into the composer.
function undoLastTurn(): void {
  const chat = selectedChat(get())
  if (!chat) {
    toast('Select a chat to undo its last turn.', 'info')
    return
  }
  const turn = chat.transcript.lastRewindableUser
  if (!turn) {
    toast('Nothing to undo in this chat.', 'info')
    return
  }
  undo(turn, chat)
}

/** Context menu "Undo from this message…". */
function undo(turn: UserTurn, chat: ChatState): void {
  if (!userTurnIsRewindable(turn)) {
    toast('This message cannot be undone yet.', 'info')
    return
  }
  if (!canRewind(chat)) {
    toast('Wait for the running task to finish before undoing.', 'info')
    return
  }
  const index = chat.transcript.locateUser(turn.id)
  if (index === null) return
  if (chat.editingTurnId !== null) cancelEdit(chat)
  patch({ rewindRequest: makeRewindRequest(chat, turn, index, { kind: 'undo' }) })
}

/// The sheet's Keep files / Revert files: `session/rewind`, reload the chat
/// from storage, then re-send (edit) or refill the composer (undo).
async function performRewind(request: RewindRequest, restoreFiles: boolean): Promise<void> {
  const chat = request.chat
  const messageId = request.turn.messageId
  const project = projectById(get(), chat.projectId)
  if (messageId === null || !project) return
  if (!canRewind(chat)) {
    toast('Wait for the running task to finish before rewinding.', 'info')
    return
  }
  patch({ rewindInFlight: true })
  try {
    let result: Awaited<ReturnType<typeof backend.sessionRewind>>
    try {
      result = await project.backend.sessionRewind(project.id, chat.sessionId, messageId, restoreFiles)
    } catch (error) {
      const message = errorText(error)
      toast(`Could not rewind the chat: ${message}`, 'error', message)
      return
    }
    patch({ rewindRequest: null })
    chat.update((c) => {
      c.editingTurnId = null
      c.editingText = ''
    })
    project.upsertSession(result.session)
    // Reload rather than trim locally: a turn cut in the middle is rebuilt
    // exactly as storage now has it.
    chat.update((c) => (c.status = { kind: 'loadingHistory' }))
    try {
      const history = await project.backend.sessionHistory(project.id, chat.sessionId)
      chat.loadHistory(history)
    } catch (error) {
      chat.update((c) => {
        c.status = { kind: 'idle' }
        c.historyError = errorText(error)
      })
    }
    const files = result.files ?? null
    const skipped = files?.skipped ?? []
    chat.addLog(
      `rewound to message ${messageId}; removed ${result.removed_tasks} task(s)` +
        (files
          ? `; restored ${files.restored}, deleted ${files.deleted} file(s)` + (skipped.length === 0 ? '' : `; kept ${skipped.join(', ')}, changed since`)
          : ''),
    )
    void (async () => {
      await project.refreshGit()
      await project.loadExtras()
    })()
    if (files) {
      const total = restoreReportTotal(files)
      const reverted = total === 0 ? 'Files were already as before that message.' : `Reverted ${total} file${total === 1 ? '' : 's'}.`
      if (skipped.length === 0) {
        toast(reverted, 'success')
      } else {
        // Changed again since by another session or by hand: kept.
        const kept = `Kept ${skipped.length} file${skipped.length === 1 ? '' : 's'} changed since: ${skipped.slice(0, 3).join(', ')}${skipped.length > 3 ? '…' : ''}.`
        toast(total === 0 ? kept : `${reverted} ${kept}`, 'info', skipped.join('\n'))
      }
    }
    switch (request.mode.kind) {
      case 'edit': {
        // The edited text goes with the original message's attachments.
        const prompt = outgoingPrompt(request.mode.newText, request.turn.images, request.turn.files)
        if (project.isRunningSession(chat.sessionId)) chat.enqueue(prompt)
        else await run(prompt, chat, project)
        break
      }
      case 'undo':
        void moveToComposer(outgoingPrompt(request.turn.text, request.turn.images, request.turn.files), chat)
        if (!files) toast('Undid the last turn; the message is back in the composer.', 'success')
        break
    }
    focusComposer()
  } finally {
    patch({ rewindInFlight: false })
  }
}

// MARK: - Approvals

/// mod+Return: approve the selected chat's pending approval, else submit the
/// inline edit, else commit from the Changes tab's box, else send the draft
/// (queued while the chat runs).
function primaryAction(): void {
  const state = get()
  const chat = selectedChat(state)
  const request = chat?.pendingApproval ?? null
  if (chat && request && !chat.approvalInFlight) {
    void respond(request.id, chat, 'approve')
    return
  }
  if (chat && chat.editingTurnId !== null) {
    submitEdit(chat)
    return
  }
  // Changes tab with a commit message typed: mod+Return commits.
  const project = selectedProject(state)
  if (state.sidebarItem === 'changes' && project && project.isGitRepository && project.commitMessage.trim().length > 0 && extensions.commitFromBox) {
    void extensions.commitFromBox(project)
    return
  }
  // mod+Return sends; while the chat runs it queues the prompt for after the run.
  const projectId = state.selectedProjectId ?? ''
  const draft = chat?.draft ?? state.pendingDraft[projectId] ?? ''
  const attachments = chat?.attachments ?? state.pendingAttachments[projectId] ?? []
  if ((draft.trim().length === 0 && attachments.length === 0) || !project) return
  const root = project.root
  void (async () => {
    const prompt = await prepareAttachmentPrompt(draft, attachments, root)
    await send(prompt, { queue: true })
  })()
}

/** mod+Backspace. */
function denyPendingApproval(): void {
  const chat = selectedChat(get())
  const request = chat?.pendingApproval ?? null
  if (!chat || !request || chat.approvalInFlight) return
  void respond(request.id, chat, 'deny')
}

async function respond(requestId: string, chat: ChatState, decision: ApprovalDecision): Promise<void> {
  chat.update((c) => {
    c.approvalInFlight = true
    c.approvalError = null
  })
  try {
    // "Always continue" on a lower discount: stop asking from now on
    // (Settings → Spending), then continue this run.
    const pending = chat.pendingApproval
    if (decision === 'approve_for_session' && pending && pending.id === requestId && isDiscountDrop(pending)) {
      try {
        await patchConfig({ 'spend.confirm_discount_drop': false })
      } catch (error) {
        toast(`Could not turn off the lower-discount question: ${errorText(error)}`, 'error')
      }
    }
    try {
      await backendFor(chat.projectId).approvalRespond(chat.projectId, chat.sessionId, requestId, decision)
    } catch (error) {
      chat.update((c) => (c.approvalError = errorText(error)))
    }
  } finally {
    chat.update((c) => (c.approvalInFlight = false))
  }
}

// MARK: - Questions

/// Answers the chat's pending `ask_user` questions (`answers` null and
/// `message` null: the user skipped them).
async function respondQuestion(requestId: string, chat: ChatState, answers: QuestionAnswer[] | null, message: string | null = null): Promise<void> {
  if (chat.questionInFlight) return
  chat.update((c) => {
    c.questionInFlight = true
    c.questionError = null
  })
  try {
    await backendFor(chat.projectId).questionRespond(chat.projectId, chat.sessionId, requestId, answers, message)
    if (chat.pendingQuestion?.request_id === requestId) chat.update((c) => (c.pendingQuestion = null))
  } catch (error) {
    chat.update((c) => (c.questionError = errorText(error)))
  } finally {
    chat.update((c) => (c.questionInFlight = false))
  }
  refreshAttention()
}

// MARK: - Models, permissions, index, doctor

/// `models/switch` for the selected project; failures surface inline in the
/// picker (`models.switchError`) and are rethrown.
async function switchModel(name: string): Promise<ModelSwitchResult> {
  const model = name.trim()
  const project = selectedProject(get())
  if (!project || model.length === 0) throw BackendError.notRunning()
  project.update((p) => {
    p.switchingModel = model
    p.models.switchError = null
  })
  try {
    const result = await project.backend.modelsSwitch(project.id, model)
    project.info = { ...project.info, model: result.model }
    project.models.current = result.model
    project.models.lastSwitch = result
    if (!project.models.models.includes(result.model)) project.models.models = [result.model, ...project.models.models]
    project.models.promote(result.model)
    return result
  } catch (error) {
    project.models.switchError = errorText(error)
    throw error
  } finally {
    project.update((p) => (p.switchingModel = null))
  }
}

/// mod+shift+M and the palette: the picker hangs off the composer, so opening
/// it switches to the Chat tab first.
function toggleModelPicker(): void {
  if (get().modelPickerVisible) {
    patch({ modelPickerVisible: false })
    return
  }
  if (!selectedProject(get())) {
    toast('Open a project to choose its model.', 'info')
    return
  }
  if (get().sidebarItem !== 'chat') setSidebarItem('chat')
  patch({ modelPickerVisible: true })
}

/** "GPT-4.1" for "gpt-4.1": models.dev's name for the configured endpoint, else `ModelNames.pretty`. */
function modelDisplayName(id: string): string {
  return modelsDev.displayName(id, get().config?.model.base_url ?? '')
}

function modelInfo(id: string): ModelsDevInfo | null {
  return modelsDev.info(id, get().config?.model.base_url ?? '')
}

/// ModelGateway's savings and 24 h error rate for a model of `project`'s
/// endpoint (the selected project's by default); null while signed out.
function modelStat(id: string, project?: ProjectState | null): ModelStat | null {
  if (!(extensions.isGatewaySignedIn?.() ?? false)) return null
  return (project ?? selectedProject(get()))?.models.stats[id] ?? null
}

/// The picker's fetch: a no-op unless the list is missing or stale
/// (`ModelCatalog.needsLoad`) or `force` (Refresh).
async function refreshModels(force: boolean): Promise<void> {
  const project = selectedProject(get())
  if (!project) return
  if (force) {
    if (project.models.loading && project.models.models.length > 0) return
    void modelsDev.load(true)
    await project.refreshModels(true)
  } else {
    await prefetchModels(project)
  }
}

/// Fetches the model list in the background when a project opens or its
/// endpoint changes, so the picker opens on a ready list.
async function prefetchModels(project: ProjectState): Promise<void> {
  // models.dev rides along (cached, refetched once a day).
  void modelsDev.load()
  if (!isProjectReady(get(), project) || !project.models.needsLoad || project.models.loading) return
  await project.refreshModels(false)
}

/// Re-applies the mode last chosen for this repository (the backend starts
/// every opened project on the configured default). A profile the backend
/// rejects is forgotten.
async function restorePermissionProfile(project: ProjectState): Promise<void> {
  const saved = getPref('permissionProfiles')[project.root]
  if (saved === undefined || saved === project.info.permission_profile) return
  try {
    const info = await project.backend.permissionsSet(project.id, saved)
    project.update((p) => (p.info = info))
  } catch {
    const next = { ...getPref('permissionProfiles') }
    delete next[project.root]
    setPref('permissionProfiles', next)
  }
}

/** `permissions/set`; appends a notice to the selected chat. */
async function setApprovalMode(profile: string): Promise<void> {
  const project = selectedProject(get())
  if (!project) return
  try {
    const info = await project.backend.permissionsSet(project.id, profile)
    project.update((p) => (p.info = info))
    setPermissionProfile(project.root, info.permission_profile)
    selectedChat(get())?.update((c) => c.transcript.append(makeNoticeItem({ lines: [`Approval mode set to ${approvalModeLabel(info.permission_profile)}`] })))
  } catch (error) {
    const message = errorText(error)
    toast(`Could not change the approval mode: ${message}`, 'error', message)
  }
}

async function rebuildIndex(): Promise<void> {
  const project = selectedProject(get())
  if (!project) return
  try {
    await project.backend.indexRebuild(project.id)
    project.update((p) => {
      p.indexProgress = { project_id: p.id, done: 0, total: 0 }
      p.lastIndexReady = null
      p.info = { ...p.info, indexing: true }
    })
  } catch (error) {
    const message = errorText(error)
    toast(`Could not rebuild the index: ${message}`, 'error', message)
  }
}

/** `doctor/run` for the selected project (results inline in its Doctor card), else without a project (Settings). */
async function runDoctor(probe: boolean): Promise<void> {
  const project = selectedProject(get())
  if (project) {
    await project.runDoctor(probe)
    return
  }
  patch({ doctorRunning: true })
  try {
    const report = await backend.doctorRun(null, probe)
    patch({ doctorReport: report, doctorError: null })
  } catch (error) {
    patch({ doctorError: errorText(error) })
  } finally {
    patch({ doctorRunning: false })
  }
}

// MARK: - Config

/// `config/patch`; the result replaces `config`. Errors propagate to the
/// caller (Settings shows them inline).
async function patchConfig(values: JSONObject): Promise<void> {
  const previous = get().config?.model ?? null
  const result = await backend.configPatch(values)
  patch({ config: result, connection: { kind: 'ready', result } })
  const endpointChanged = previous?.base_url !== result.model.base_url || previous?.api_key_env !== result.model.api_key_env
  for (const project of get().projects) {
    if (project.isRemote) continue
    project.models.baseURL = result.model.base_url
    if (endpointChanged) {
      project.models.invalidate()
      void prefetchModels(project)
    }
    project.touch()
    if (!project.isRunning) void project.refreshStatus()
  }
}

// MARK: - MCP servers (Settings)

/** Fetches every MCP server's state; the notification keeps it current afterwards. */
async function refreshMcpStatus(): Promise<void> {
  if (!isReady(get())) return
  try {
    patch({ mcpServers: await backend.mcpStatus() })
  } catch {
    // Older backends: no MCP.
  }
}

/** Reconnects one server (or all) and waits for the outcome. */
async function reconnectMcp(server: string | null = null): Promise<void> {
  patch({ mcpServers: await backend.mcpReconnect(server) })
}

/** Fetches the app catalog (once; it is static for a backend). */
async function loadMcpCatalog(): Promise<void> {
  if (!isReady(get()) || get().mcpCatalog.length > 0) return
  try {
    patch({ mcpCatalog: await backend.mcpCatalog() })
  } catch {
    // Keep the empty catalog; the Apps tab shows its empty state.
  }
}

/** Installs a catalog app; the backend connects it in the background. */
async function installMcpApp(id: string, inputs: { [key: string]: string }, excludeTools: string[] = []): Promise<void> {
  const result = await backend.mcpInstall(id, inputs, excludeTools)
  patch({ config: result, connection: { kind: 'ready', result } })
  await refreshMcpStatus()
}

async function uninstallMcpApp(id: string): Promise<void> {
  const result = await backend.mcpUninstall(id)
  patch({ config: result, connection: { kind: 'ready', result } })
  await refreshMcpStatus()
}

// MARK: - API key and binary (Connect-a-model card, Settings)

/** Whether the credential store holds the configured key variable. */
async function isAPIKeyStored(): Promise<boolean> {
  return secretIsSet(apiKeyEnvName(get()))
}

/** Stores the key in the credential store and restarts the backend when idle. */
async function saveAPIKey(key: string): Promise<void> {
  const value = key.trim()
  if (value.length === 0) return
  await invoke<void>('secret_set', { account: apiKeyEnvName(get()), value })
  if (!anyProjectRunning(get())) await restartBackend()
}

async function removeAPIKey(): Promise<void> {
  await invoke<void>('secret_remove', { account: apiKeyEnvName(get()) })
  if (!anyProjectRunning(get())) await restartBackend()
}

/** "Choose binary…": file picker → `backendPathOverride` → restart. */
async function chooseBackendBinary(): Promise<void> {
  let picked: string | string[] | null
  try {
    picked = await openDialog({ directory: false, multiple: false, title: 'Choose the ctx binary' })
  } catch {
    return
  }
  const path = Array.isArray(picked) ? picked[0] : picked
  if (!path) return
  setPref('backendPathOverride', path)
  await restartBackend()
}

function copyToPasteboard(text: string): void {
  void copyText(text)
}

function copyBuildCommand(): void {
  copyToPasteboard(BUILD_COMMAND)
}

// MARK: - Git / paths

async function refreshGit(project: ProjectState): Promise<void> {
  await project.refreshGit()
}

async function loadProjectExtras(project: ProjectState): Promise<void> {
  await project.loadExtras()
}

/** Changes tab selection → loads the diff and remembers the path. */
async function selectChange(path: string | null, project: ProjectState): Promise<void> {
  project.update((p) => (p.selectedChangePath = path))
  setLastChangesPath(project.id, path)
  await project.loadDiff()
}

async function setShowStaged(staged: boolean, project: ProjectState): Promise<void> {
  project.update((p) => (p.showStaged = staged))
  await project.loadDiff()
}

/** The context-menu actions on a repository path (`PathAction`). */
function reveal(path: string, project: ProjectState, action: PathAction, line: number | null = null): void {
  const absolute = isAbsolutePath(path) ? path : joinPath(project.root, path)
  switch (action) {
    case 'showInChanges': {
      const relative = relativePath(absolute, project.root)
      selectProject(project.id, false)
      setSidebarItem('changes')
      void (async () => {
        if (project.gitStatus === null) await project.refreshGit()
        await selectChange(relative, project)
      })()
      return
    }
    case 'revealInFileManager':
      if (project.isRemote) {
        toast(`${lastPathComponent(absolute)} is on ${project.hostLabel ?? 'a remote host'}; the file manager cannot show it.`, 'info')
        return
      }
      void invoke<void>('reveal_in_file_manager', { path: absolute }).catch((error: unknown) => {
        toast(`Could not show ${lastPathComponent(absolute)}: ${errorText(error)}`, 'error')
      })
      return
    case 'openWithEditor':
      if (project.isRemote) {
        toast(`${lastPathComponent(absolute)} is on ${project.hostLabel ?? 'a remote host'}; open it in the Editor tab instead.`, 'info')
        return
      }
      openInEditor(absolute)
      return
    case 'openInEditor': {
      const relative = relativePath(absolute, project.root)
      if (extensions.openInEditor) extensions.openInEditor(relative, project, line ?? null)
      else openInEditor(absolute)
      return
    }
    case 'copyPath':
      copyToPasteboard(absolute)
      return
  }
}

/// Opens `path` in Settings → General "Default editor", or with the app the
/// system associates with the file when none is chosen.
function openInEditor(path: string): void {
  const editor = getPref('defaultEditorPath')
  const fallback = () =>
    invoke<void>('open_with_default_app', { path }).catch((error: unknown) => {
      toast(`Could not open ${lastPathComponent(path)}: ${errorText(error)}`, 'error')
    })
  if (editor !== null) {
    void openPath(path, editor).catch(() => fallback())
  } else {
    void fallback()
  }
}

/** Debounced `git/status` after `file_changed` / `project/changed`. */
function scheduleGitRefresh(project: ProjectState): void {
  const existing = gitRefreshTimers.get(project.id)
  if (existing !== undefined) clearTimeout(existing)
  gitRefreshTimers.set(
    project.id,
    setTimeout(() => {
      gitRefreshTimers.delete(project.id)
      if (!projectById(get(), project.id)) return
      void project.refreshGit()
    }, GIT_REFRESH_DELAY_MS),
  )
}

// MARK: - Notifications from the backend

/// `handle(_:remote:)`: routes one notification of the local backend
/// (`remote` null) or of a host's connection (the macOS doc's "handle
/// rules"). Project ids are unique across backends (`openProject` refuses a
/// clash), so project-scoped notifications route by id alone.
function handle(notification: BackendNotification, remote: RemoteConnection | null = null): void {
  batch(() => handleNotification(notification, remote))
}

function handleNotification(notification: BackendNotification, remote: RemoteConnection | null): void {
  switch (notification.kind) {
    case 'event': {
      const project = projectById(get(), notification.projectId)
      if (!project) return
      const chat = project.chat(notification.sessionId)
      if (!project.isRunningSession(notification.sessionId)) {
        project.addRunning(notification.sessionId)
        project.touch()
      }
      chat.apply(notification.event)
      const event = notification.event
      if (event.event === 'approval_requested') {
        if (getPref('notifyOnApproval') && (!isAppActive() || chat !== selectedChat(get()))) {
          // A remembered ("approve for session") request is resolved by the
          // very next event: give that pair a moment before announcing a
          // request nobody needs to answer.
          const request = event.request
          const projectId = project.id
          setTimeout(() => {
            if (chat.pendingApproval?.id !== request.id) return
            notifier.notifyApprovalNeeded(request.summary, chat.title, projectId, chat.sessionId)
          }, APPROVAL_NOTIFY_DELAY_MS)
        }
      } else if (event.event === 'file_changed') {
        scheduleGitRefresh(project)
      }
      refreshAttention()
      return
    }

    case 'taskFinished': {
      const finished = notification.finished
      const project = projectById(get(), finished.project_id)
      if (!project) return
      const chat = project.chat(finished.session_id)
      chat.finish(finished)
      project.removeRunning(finished.session_id)
      project.touch()
      if (getPref('notifyOnFinish') && (!isAppActive() || chat !== selectedChat(get()))) {
        // A cancel before the runtime started is a cancel, not a failure.
        const early = isEarlyCancel(finished)
        const status: TaskStatus | null = early ? 'cancelled' : (finished.status ?? null)
        notifier.notifyTaskFinished(status, early ? null : (finished.error ?? null), chat.title, project.id, chat.sessionId)
      }
      refreshAttention()
      const timer = gitRefreshTimers.get(project.id)
      if (timer !== undefined) clearTimeout(timer)
      gitRefreshTimers.delete(project.id)
      void (async () => {
        await project.refreshGit()
        await project.loadSessions()
        await project.refreshStatus()
        await project.loadExtras()
      })()
      if (finished.error_kind !== 'provider_auth') dequeue(project, chat)
      if (finished.status === 'completed' && chat.queued.length === 0) requestSuggestion(chat, project.id)
      return
    }

    case 'indexProgress': {
      const progress = notification.progress
      const project = projectById(get(), progress.project_id)
      if (!project) {
        buffer(notification, progress.project_id)
        return
      }
      project.update((p) => {
        p.indexProgress = progress
        p.info = { ...p.info, indexing: true }
      })
      return
    }

    case 'indexReady': {
      const ready = notification.ready
      const project = projectById(get(), ready.project_id)
      if (!project) {
        buffer(notification, ready.project_id)
        return
      }
      project.update((p) => {
        p.indexProgress = null
        p.lastIndexReady = ready
        p.info = { ...p.info, indexing: false }
      })
      void project.refreshStatus()
      return
    }

    case 'log': {
      // A host's lines carry its name; the local backend's are as they are.
      const message = remote ? `[${remote.displayName}] ${notification.message}` : notification.message
      appendBackendLog(notification.level, message)
      if (notification.level.toLowerCase() === 'error') toast(message, 'error', message)
      return
    }

    case 'lspDiagnostics':
      extensions.lspDiagnostics?.(notification.diagnostics)
      return

    case 'browserRequest':
      handleBrowserRequest(notification.request, remote?.client ?? backend)
      return

    case 'questionRequest':
      handleQuestionRequest(notification.request, remote?.client ?? backend)
      return

    case 'mcpStatus':
      // Settings → Apps describe the local backend's servers only.
      if (remote === null) patch({ mcpServers: notification.servers })
      return

    case 'projectChanged': {
      const value = jsonObject(notification.params)
      const id = value ? jsonString(value.project_id) : null
      const project = id !== null ? projectById(get(), id) : null
      if (!value || !project) return
      const git = decodeGitInfo(value.git)
      if (git) project.update((p) => (p.info = { ...p.info, git }))
      scheduleGitRefresh(project)
      return
    }

    case 'exited':
      if (remote) handleRemoteExit(remote, notification.code, notification.tail)
      else handleExit(notification.code, notification.tail)
      return
  }
}

/** `GitInfo` of a `project/changed` notification, or null when absent or malformed. */
function decodeGitInfo(raw: JSONValue | undefined): GitInfo | null {
  const object = jsonObject(raw)
  if (!object) return null
  const dirty = jsonInt(object.dirty_files)
  if (dirty === null) return null
  const worktree = jsonObject(object.worktree)
  const parentRoot = worktree ? jsonString(worktree.parent_root) : null
  return {
    branch: jsonString(object.branch),
    head: jsonString(object.head),
    dirty_files: dirty,
    worktree: worktree && parentRoot !== null ? { parent_root: parentRoot, managed: worktree.managed === true } : null,
  }
}

/// The `browser` tool wants something done in the project's Browser tab:
/// the page acts and the reply goes back with `browser/respond`. The
/// Inspector is left on whatever tab the user has open.
function handleBrowserRequest(request: BrowserRequest, client: BackendClient): void {
  const project = projectById(get(), request.project_id)
  if (!project) {
    void client
      .browserRespond(request.project_id, request.session_id, request.request_id, { ok: false, error: 'the project is not open in the app' })
      .catch(() => undefined)
    return
  }
  const workspace = browser(project)
  void (async () => {
    const reply = await workspace.perform(request.action)
    try {
      await client.browserRespond(request.project_id, request.session_id, request.request_id, reply)
    } catch (error) {
      debug(() => `browser/respond failed: ${errorText(error)}`)
    }
  })()
}

/// The `ask_user` tool put questions to the user: the chat shows them above
/// its composer (and gets the orange dot) until they are answered.
function handleQuestionRequest(request: QuestionRequest, client: BackendClient): void {
  const project = projectById(get(), request.project_id)
  if (!project) {
    void client.questionRespond(request.project_id, request.session_id, request.request_id, null, null).catch(() => undefined)
    return
  }
  const chat = project.chat(request.session_id)
  chat.update((c) => {
    c.pendingQuestion = request
    c.questionError = null
    c.questionInFlight = false
  })
  if (getPref('notifyOnApproval') && (!isAppActive() || chat !== selectedChat(get()))) {
    const first = request.questions[0]?.question ?? 'The agent has a question'
    notifier.notifyQuestionAsked(first, chat.title, project.id, chat.sessionId)
  }
  refreshAttention()
}

/// `exited(code, tail)`: every running chat fails ("Backend exited (code
/// N)", open tool groups stopped), the connection fails, a toast with the
/// stderr tail; one automatic restart after 1 s per healthy run.
function handleExit(code: number, tail: string): void {
  for (const project of get().projects) {
    if (project.isRemote) continue
    for (const chat of project.allChats) {
      if (!chat.isRunning) continue
      chat.finish(
        { project_id: project.id, session_id: chat.sessionId, task_id: null, status: 'failed', error: `Backend exited (code ${code})`, error_kind: 'backend_exited' },
        'stopped',
      )
    }
    project.runningSessionIds = new Set()
    project.switchingModel = null
    project.indexProgress = null
    project.touch()
  }
  patch({ connection: { kind: 'failed', failure: { kind: 'exited', code, tail } } })
  appendBackendLog('error', `Backend exited (code ${code})`)
  toast(`Backend stopped (code ${code})`, 'error', tail.length === 0 ? `ctx exited with code ${code}` : tail)
  refreshAttention()

  // One automatic restart per healthy run (not after an explicit stop — the
  // client does not report those — and not in a crash loop). A child that
  // keeps dying after the 5 s mark (during the first index of a large
  // repository, say) gets at most `MAX_AUTO_RESTARTS` consecutive retries;
  // then the panel's Retry.
  const healthyFor = lastSuccessfulStart === null ? 0 : (Date.now() - lastSuccessfulStart) / 1000
  if (healthyFor > AUTO_RESTART_RESET_AFTER) autoRestartCount = 0
  if (autoRestartPending || healthyFor <= 5 || autoRestartCount >= MAX_AUTO_RESTARTS) return
  autoRestartCount += 1
  autoRestartPending = true
  setTimeout(() => {
    autoRestartPending = false
    const connection = get().connection
    if (connection.kind === 'failed' && connection.failure.kind === 'exited') void restartBackend()
  }, 1000)
}

/// After `task/finished`: every chat of the project with queued prompts
/// whose session is idle sends its next one, the finished chat first.
function dequeue(project: ProjectState, chat: ChatState): void {
  if (!isProjectReady(get(), project)) return
  const started = new Set<string>()
  for (const target of [chat, ...project.queuedChats]) {
    if (target.queued.length === 0 || project.isRunningSession(target.sessionId) || started.has(target.sessionId)) continue
    started.add(target.sessionId)
    const prompt = target.dequeueFirst()
    if (!prompt) continue
    void run(prompt, target, project)
  }
}

function buffer(notification: BackendNotification, projectId: string): void {
  const list = [...(pendingProjectNotifications.get(projectId) ?? []), notification]
  if (list.length > MAX_BUFFERED_NOTIFICATIONS) list.splice(0, list.length - MAX_BUFFERED_NOTIFICATIONS)
  pendingProjectNotifications.set(projectId, list)
}

function replayBufferedNotifications(projectId: string): void {
  const list = pendingProjectNotifications.get(projectId)
  if (!list) return
  pendingProjectNotifications.delete(projectId)
  for (const notification of list) handle(notification)
}

function appendBackendLog(level: string, message: string): void {
  backendLogNextId += 1
  const next = [...get().backendLog, { id: backendLogNextId, date: new Date(), text: message, level }]
  if (next.length > ChatState.maxLogLines) next.splice(0, next.length - ChatState.maxLogLines)
  patch({ backendLog: next })
}

function clearBackendLog(): void {
  patch({ backendLog: [] })
}

// MARK: - Attention

/** `didBecomeActiveNotification`: the selected chat is seen now. */
function appDidBecomeActive(): void {
  const chat = selectedChat(get())
  if (chat) {
    if (chat.unseen !== null) chat.update((c) => (c.unseen = null))
    notifier.clearDelivered(chat.sessionId)
  }
  refreshAttention()
}

/** The badge follows `attentionCount`. */
function refreshAttention(): void {
  notifier.updateBadge(attentionCount(get()))
}

// MARK: - Helpers

function makeProject(info: ProjectInfo, host: SSHHost | null): ProjectState {
  // The catalog follows the backend the project is on: the host's endpoint
  // for a remote project (its `initialize` result).
  const connection = host === null ? null : remoteConnections.get(host.id)
  const baseURL = connection?.result?.model.base_url ?? (host === null ? (get().config?.model.base_url ?? '') : '')
  const catalog = new ModelCatalog(baseURL)
  catalog.current = info.model
  const project = new ProjectState(info, catalog, connection?.client ?? backend, host)
  project.lastTab = parseSidebarItem(lastTabFor(info.project_id)) ?? 'chat'
  project.selectedChangePath = getPref('lastChangesPath')[info.project_id] ?? null
  project.onChatCreated = (chat) => {
    chat.isVisible = () => selectedChat(get()) === chat && isAppActive()
  }
  return project
}

// MARK: - Remote hosts (App/RemoteHosts.swift)

/** The host's connection object, created (disconnected) on first use; a newer host record replaces the old. */
function remoteFor(host: SSHHost): RemoteConnection {
  const existing = remoteConnections.get(host.id)
  if (existing) {
    if (!sameHost(existing.host, host)) existing.update((c) => (c.host = host))
    return existing
  }
  const connection = new RemoteConnection(host)
  connection.client.onNotification = (notification) => get().handle(notification, connection)
  remoteConnections.set(host.id, connection)
  bumpStore()
  return connection
}

function remoteOf(project: ProjectState): RemoteConnection | null {
  return project.host === null ? null : (remoteConnections.get(project.host.id) ?? null)
}

/** `backend(for:)`: the backend a project id belongs to; the local one when the project is not open. */
function backendFor(projectId: string | null | undefined): BackendClient {
  return projectById(get(), projectId)?.backend ?? backend
}

/// `connect(_:quietly:)`: starts `ssh <host> ctx serve` unless it is already
/// up; null (with a toast) when it cannot be reached. Concurrent callers
/// share one attempt.
async function connectHost(host: SSHHost, quietly = false): Promise<RemoteConnection | null> {
  const connection = remoteFor(host)
  if (connection.isReady && connection.client.isReady) return connection
  const inFlight = connection.connecting
  if (inFlight) return (await inFlight) ? connection : null
  const attempt = startRemote(connection)
  connection.connecting = attempt
  const ok = await attempt
  connection.connecting = null
  if (!ok && !quietly) {
    const failure = connection.failure
    if (failure) {
      toast(`Could not connect to ${connection.displayName}: ${remoteFailureSummary(failure)}`, 'error', [describeDetail(failure), connection.hint].filter((part): part is string => part !== null && part.length > 0).join('\n\n'))
    }
  }
  return ok ? connection : null
}

function describeDetail(failure: BackendFailure): string {
  switch (failure.kind) {
    case 'exited':
      return failure.tail
    case 'transport':
      return failure.message
    case 'incompatible':
      return failure.detail
    case 'notFound':
      return failure.searched.join('\n')
  }
}

async function startRemote(connection: RemoteConnection): Promise<boolean> {
  connection.update((c) => {
    c.state = { kind: 'connecting' }
    c.hint = null
    c.needsCtx = false
  })
  const launch = await launchOptions()
  // The login shell's environment carries SSH_AUTH_SOCK and the user's PATH
  // (a Homebrew ssh, say); no key is passed along. `--mock` applies to every backend.
  try {
    const result = await connection.client.start({ mock: mockRequested(launch), host: connection.host })
    connection.update((c) => (c.state = { kind: 'ready', result }))
    const who = result.host ? `${result.host.user}@${result.host.hostname}` : connection.host.address
    appendRemoteLog(connection, `connected: ctx ${result.ctx_version} on ${who}`)
    return true
  } catch (error) {
    const failure = connection.client.lastStartFailure ?? failureOf(error)
    connection.update((c) => {
      c.state = { kind: 'failed', failure }
      c.hint = remoteHint(failure, c.client.stderrLog, c.host)
      c.needsCtx = isCtxProblem(remoteProblem(failure, c.client.stderrLog))
    })
    appendRemoteLog(connection, `connection failed: ${remoteFailureSummary(failure)}`, 'error')
    return false
  }
}

/// `disconnect(_:)`: closes the host's projects (kept remembered) and stops
/// its ssh. Running tasks on the host are cancelled by the server as its
/// stdin closes.
async function disconnectHost(host: SSHHost): Promise<void> {
  const connection = remoteConnections.get(host.id)
  if (!connection) return
  for (const project of projectsOn(get(), host.id)) await closeProject(project.id, false)
  await connection.client.stop()
  connection.update((c) => {
    c.state = { kind: 'disconnected' }
    c.hint = null
    c.needsCtx = false
  })
}

/// `reconnect(_:)`: reconnects a host after a failure or a dropped link and
/// reopens the projects the app still lists for it.
async function reconnectHost(host: SSHHost): Promise<void> {
  const remembered = projectsOn(get(), host.id).map((project) => project.root)
  const connection = remoteConnections.get(host.id)
  if (connection && connection.client.isRunning) await connection.client.stop()
  if (!(await connectHost(host))) return
  for (const root of remembered) await openProject(root, { host, select: false, restoreSession: true })
  if (remembered.length === 0) {
    for (const key of getPref('openProjectPaths')) {
      const ref = decodeRemoteProject(key)
      if (ref && ref.hostId === host.id) await openProject(ref.path, { host, select: false, restoreSession: true })
    }
  }
}

/// `handleRemoteExit(_:code:tail:)`: the link to a host went away (ssh
/// exited): its projects' runs are over, the rows stay with the host marked
/// failed, Reconnect in the sidebar brings them back. No automatic retry: a
/// dropped VPN would otherwise spawn ssh every second.
function handleRemoteExit(connection: RemoteConnection, code: number, tail: string): void {
  const name = connection.displayName
  for (const project of projectsOn(get(), connection.id)) {
    for (const chat of project.allChats) {
      if (!chat.isRunning) continue
      chat.finish(
        { project_id: project.id, session_id: chat.sessionId, task_id: null, status: 'failed', error: `Connection to ${name} lost (ssh exited with code ${code})`, error_kind: 'backend_exited' },
        'stopped',
      )
    }
    project.runningSessionIds = new Set()
    project.switchingModel = null
    project.indexProgress = null
    project.touch()
  }
  const failure: BackendFailure = { kind: 'exited', code, tail }
  connection.update((c) => {
    c.state = { kind: 'failed', failure }
    c.hint = remoteHint(failure, c.client.stderrLog, c.host)
    c.needsCtx = isCtxProblem(remoteProblem(failure, c.client.stderrLog))
  })
  appendRemoteLog(connection, `ssh exited (code ${code})`, 'error')
  toast(`Lost the connection to ${name} (ssh exited with code ${code}).`, 'error', tail.length === 0 ? undefined : tail)
  refreshAttention()
}

function appendRemoteLog(connection: RemoteConnection, message: string, level = 'info'): void {
  handle({ kind: 'log', level, message }, connection)
}

/// `chooseRemoteProject(host:)`: the remote folder picker (Settings → Remote
/// hosts first when no host is configured).
function chooseRemoteProject(host: SSHHost | null = null): void {
  const hosts = SSHHosts.items()
  if (hosts.length === 0 && host === null) {
    openSettings(SETTINGS_TAB_REMOTE_HOSTS)
    toast('Add an SSH host first; then projects on it can be opened.', 'info')
    return
  }
  remotePickerCounter += 1
  patch({ remotePicker: { id: remotePickerCounter, host: host ?? (hosts.length === 1 ? (hosts[0] ?? null) : null) } })
}

/** `listRemoteDirectory(_:on:)`: `fs/list` on the host (connecting first), for the picker. */
async function listRemoteDirectory(path: string | null, host: SSHHost): Promise<FsListing> {
  const connection = await connectHost(host, true)
  if (!connection) {
    const failure = remoteConnections.get(host.id)?.failure ?? null
    throw BackendError.transport(failure ? remoteFailureSummary(failure) : `${hostDisplayName(host)} is not connected`)
  }
  return connection.client.fsList(path, true)
}

/// `reopenRememberedRemoteProjects()`: at launch, every remembered open
/// project on a host, each host in parallel, without blocking anything. A
/// host that cannot be reached keeps its rows out of the sidebar and its
/// state in the Remote section; the folders stay remembered.
async function reopenRememberedRemoteProjects(): Promise<void> {
  if (remoteRestoreStarted) return
  remoteRestoreStarted = true
  const byHost = new Map<string, string[]>()
  for (const key of getPref('openProjectPaths')) {
    const ref = decodeRemoteProject(key)
    if (!ref) continue
    if (!SSHHosts.host(ref.hostId)) {
      forgetOpenProject(key)
      continue
    }
    byHost.set(ref.hostId, [...(byHost.get(ref.hostId) ?? []), ref.path])
  }
  const last = getPref('lastSelectedProjectId')
  try {
    await Promise.all(
      [...byHost.entries()].map(async ([hostId, paths]) => {
        const host = SSHHosts.host(hostId)
        if (!host) return
        const connection = await connectHost(host, true)
        if (!connection) {
          // Its loading rows go; the host row says why.
          const keys = new Set(paths.map((path) => projectKey(path, hostId)))
          patch({ openingProjectPaths: get().openingProjectPaths.filter((key) => !keys.has(key)) })
          return
        }
        for (const path of paths) {
          const project = await openProject(path, { host, select: false, restoreSession: false })
          if (project && project.id === last && get().selectedProjectId === null) selectRestoredProject(project.id)
        }
      }),
    )
  } finally {
    restoreOrder = []
    if (byHost.size > 0 && get().selectedProjectId === null) {
      const project = get().projects[0]
      if (project) selectRestoredProject(project.id)
      else if (get().sidebarItem === 'chat') setSidebarItem('welcome')
    }
  }
}

/** `saveHost(_:)`: adds or updates a host; a connected host keeps its connection until it connects next. */
function saveHost(host: SSHHost): SSHHost {
  const normalized = SSHHosts.upsert(host)
  remoteConnections.get(normalized.id)?.update((c) => (c.host = normalized))
  RecentProjects.renameHost(normalized.id, hostDisplayName(normalized))
  bumpStore()
  return normalized
}

/// `removeHost(_:)`: disconnects it, closes its projects, forgets its
/// recents and remembered-open folders, drops it from Settings.
async function removeHost(host: SSHHost): Promise<void> {
  await disconnectHost(host)
  const connection = remoteConnections.get(host.id)
  remoteConnections.delete(host.id)
  if (connection) await connection.client.remove()
  SSHHosts.remove(host.id)
  for (const key of getPref('openProjectPaths')) {
    if (decodeRemoteProject(key)?.hostId === host.id) forgetOpenProject(key)
  }
  RecentProjects.forgetHost(host.id)
  bumpStore()
}

/// `testHost(_:)`: `ssh <host> ctx serve` + `initialize` for the settings
/// form: a saved host is tested on its own connection; an unsaved draft on
/// a throwaway client that is stopped afterwards.
async function testHost(host: SSHHost): Promise<HostTestResult> {
  const probe = normalizeHost(host)
  if (!hostIsValid(probe)) return { ok: false, message: "Enter the host's address first.", needsCtx: false }
  const saved = SSHHosts.host(host.id)
  if (saved && sameHost(saved, probe)) {
    const connection = await connectHost(probe, true)
    if (connection) return { ok: true, summary: connection.statusText }
    const failed = remoteConnections.get(host.id)
    const failure = failed?.failure ?? null
    return {
      ok: false,
      message: [failure ? remoteFailureSummary(failure) : 'Could not connect.', failed?.hint ?? null].filter((part): part is string => part !== null).join('\n'),
      needsCtx: failed?.needsCtx ?? false,
    }
  }
  const client = new BackendClient(`probe:${probe.id}:${Date.now()}`)
  const launch = await launchOptions()
  try {
    const result = await client.start({ mock: mockRequested(launch), host: probe })
    const who = result.host ? `${result.host.user}@${result.host.hostname} · ${result.host.os}` : hostTarget(probe)
    await client.remove()
    return { ok: true, summary: `ctx ${result.ctx_version} · ${who}` }
  } catch (error) {
    const failure = client.lastStartFailure ?? failureOf(error)
    const hint = remoteHint(failure, client.stderrLog, probe)
    const needsCtx = isCtxProblem(remoteProblem(failure, client.stderrLog))
    await client.remove()
    return { ok: false, message: [remoteFailureSummary(failure), hint].filter((part): part is string => part !== null).join('\n'), needsCtx }
  }
}

/// `installCtx(on:)`: Install (or update) ctx on the host through the
/// shell's `ssh_install_ctx` (src-tauri/src/backend/install.rs), then
/// reconnect a saved host so its projects come back on the new build. A
/// host whose ctx path points elsewhere gets the path cleared, so the
/// installed copy is the one that runs. An unsaved draft (the editor's Test
/// connection) is only installed to.
async function installCtx(host: SSHHost): Promise<CtxInstallResult> {
  const connection = SSHHosts.host(host.id) ? remoteFor(host) : null
  connection?.update((c) => (c.installing = true))
  try {
    const installed = await invoke<string>('ssh_install_ctx', { host: normalizeHost(host) })
    if (connection) {
      appendRemoteLog(connection, `installed ${installed} in ~/.local/bin`)
      const saved = SSHHosts.host(host.id)
      if (saved && saved.ctxPath.trim().length > 0) saveHost({ ...saved, ctxPath: '' })
      if (connection.isReady || connection.failure !== null) {
        connection.update((c) => (c.installing = false))
        await reconnectHost(SSHHosts.host(host.id) ?? host)
      }
    }
    return { ok: true, installed }
  } catch (error) {
    const message = errorText(error)
    if (connection) appendRemoteLog(connection, `installing ctx failed: ${message}`, 'error')
    return { ok: false, message }
  } finally {
    connection?.update((c) => (c.installing = false))
  }
}

// MARK: - The store

const actions: AppActions = {
  touch,
  setSidebarItem,
  setSidebarVisible,
  toggleSidebar: () => setSidebarVisible(!get().sidebarVisible),
  setInspectorVisible,
  toggleInspector: () => setInspectorVisible(!get().inspectorVisible),
  setInspectorWidth,
  setInspectorTab: (tab) => patch({ inspectorTab: tab }),
  setPaletteVisible: (visible) => patch({ paletteVisible: visible }),
  setModelPickerVisible: (visible) => patch({ modelPickerVisible: visible }),
  focusComposer,
  setPendingDraft,
  setPendingAttachments,
  setRewindRequest: (request) => patch({ rewindRequest: request }),
  setCollapsed,
  runInTerminal,
  showAgent,
  browser,
  terminals,
  claimBackendStart,
  startBackend,
  restartBackend,
  chooseProject,
  openProject,
  openRecent,
  register,
  startLoads,
  closeProject,
  openSettings,
  selectProject,
  remoteFor,
  remoteOf,
  backendFor,
  connectHost,
  disconnectHost,
  reconnectHost,
  removeHost,
  saveHost,
  testHost,
  installCtx,
  chooseRemoteProject,
  setRemotePicker: (request) => patch({ remotePicker: request }),
  listRemoteDirectory,
  selectSession,
  newChat,
  requestSuggestion,
  loadStarters,
  send,
  retry,
  cancelRunningTask,
  removeQueued,
  editQueued,
  renameSession,
  deleteSession,
  canRewind,
  beginEdit,
  cancelEdit,
  submitEdit,
  undoLastTurn,
  undo,
  performRewind,
  primaryAction,
  denyPendingApproval,
  respond,
  respondQuestion,
  switchModel,
  toggleModelPicker,
  modelDisplayName,
  modelInfo,
  modelStat,
  refreshModels,
  prefetchModels,
  restorePermissionProfile,
  setApprovalMode,
  rebuildIndex,
  runDoctor,
  patchConfig,
  refreshMcpStatus,
  reconnectMcp,
  loadMcpCatalog,
  installMcpApp,
  uninstallMcpApp,
  isAPIKeyStored,
  saveAPIKey,
  removeAPIKey,
  chooseBackendBinary,
  copyBuildCommand,
  copyToPasteboard,
  refreshGit,
  loadProjectExtras,
  selectChange,
  setShowStaged,
  reveal,
  openInEditor,
  scheduleGitRefresh,
  handle,
  clearBackendLog,
  refreshAttention,
}

function initialFields(): AppFields {
  return {
    connection: { kind: 'connecting' },
    config: null,
    projects: [],
    selectedProjectId: null,
    selectedSessionId: null,
    sidebarItem: 'welcome',
    sidebarVisible: getPref('sidebarVisible'),
    inspectorVisible: getPref('inspectorVisible'),
    inspectorWidth: getPref('inspectorWidth'),
    inspectorTab: 'run',
    paletteVisible: false,
    modelPickerVisible: false,
    composerFocusToken: 0,
    pendingDraft: {},
    pendingAttachments: {},
    backendLog: [],
    doctorReport: null,
    doctorRunning: false,
    doctorError: null,
    mcpServers: [],
    mcpCatalog: [],
    backendStarting: false,
    openingProjectPaths: [],
    restoringSessionId: null,
    rewindRequest: null,
    rewindInFlight: false,
    remotePicker: null,
    version: 0,
  }
}

/// `AppState`: the app's one store. `useApp(selector)` subscribes a view;
/// `useApp.getState()` reads it (and calls its intents) outside React.
export const useApp = create<AppStore>()(() => ({ ...initialFields(), ...actions }))

// MARK: - Wiring (`AppState.init`)

ChatState.changeSink = (chat) => {
  const project = projectById(get(), chat.projectId)
  if (project) project.version += 1
  bumpStore()
}
ProjectState.changeSink = () => bumpStore()
RemoteConnection.changeSink = () => bumpStore()

backend.onNotification = (notification) => get().handle(notification, null)

notifier.onOpen = (projectId, sessionId) => {
  get().selectProject(projectId)
  void get().selectSession(sessionId)
}

onAppBecameActive(appDidBecomeActive)

// MARK: - Hooks

/** The project with `id`, re-rendering when it (or one of its chats) changes. */
export function useProject(id: string | null | undefined): ProjectState | null {
  const project = useApp((state) => projectById(state, id))
  useApp(() => project?.version ?? -1)
  return project
}

/** The selected project, re-rendering when it changes. */
export function useSelectedProject(): ProjectState | null {
  const project = useApp(selectedProject)
  useApp(() => project?.version ?? -1)
  return project
}

/// The chat of (`projectId`, `sessionId`) — or, with one argument, of
/// `sessionId` in the selected project — and its version: `[chat, version]`.
/// The chat is never created here (`selectSession` does that); `version`
/// changes whenever the chat does, so it is the memo key for its transcript.
export function useChat(projectOrSessionId: string | null | undefined, sessionId?: string | null): [ChatState | null, number] {
  const chat = useApp((state) => {
    if (sessionId === undefined) {
      const project = selectedProject(state)
      return project && projectOrSessionId != null ? project.existingChat(projectOrSessionId) : null
    }
    const project = projectById(state, projectOrSessionId)
    return project && sessionId !== null ? project.existingChat(sessionId) : null
  })
  const version = useApp(() => chat?.version ?? -1)
  return [chat, version]
}

/** The selected chat (never created here), re-rendering when it changes. */
export function useSelectedChat(): ChatState | null {
  const chat = useApp(selectedChat)
  useApp(() => chat?.version ?? -1)
  return chat
}

/// One value of `chat`, re-rendering only when it changes (the draft, the
/// status kind, the queue): `select` must return a primitive or a reference
/// the chat keeps (arrays it replaces on change, like `queued`).
export function useChatValue<T>(chat: ChatState | null, select: (chat: ChatState) => T, fallback: T): T {
  return useApp(() => (chat ? select(chat) : fallback))
}

/** `useChatValue` for a project. */
export function useProjectValue<T>(project: ProjectState | null, select: (project: ProjectState) => T, fallback: T): T {
  return useApp(() => (project ? select(project) : fallback))
}

/** `chatStatusIsRunning` of the selected chat (status bar, composer). */
export function useSelectedChatRunning(): boolean {
  return useApp((state) => {
    const chat = selectedChat(state)
    return chat ? chatStatusIsRunning(chat.status) : false
  })
}

/** The app's backend client (`AppState.backend`). */
export { backend } from '../backend/client'
/** The derived values, re-exported for callers that import only the store. */
export { selectedChat, selectedProject, needsAPIKey, pendingApprovals, attentionCount } from './selectors'
