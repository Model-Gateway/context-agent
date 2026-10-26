// Port of `BackendClient` (Backend/BackendClient.swift): the typed async API
// over one `ctx serve` child. The child itself lives in the Rust shell
// (`src-tauri/src/backend`, ARCHITECTURE.md "Rust ↔ frontend"): this class
// drives it through the `backend_*` / `rpc_request` commands and receives
// its frames as `backend:notification` / `backend:exited` / `backend:stderr`
// events, delivered to `onNotification` in frame order on the UI thread.

import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { BackendError, BackendFailure, BackendFailureError, errorText } from './errors'
import { debug } from './log'
import { decodeNotification, type BackendNotification } from './notifications'
import {
  normalizeRewindResult,
  normalizeSessionHistory,
  normalizeSessionSummary,
  normalizeTaskSummary,
  normalizeGatewayAccount,
  normalizeGatewayBilling,
  normalizeGatewayContextCredit,
  normalizeGatewayKeySettings,
  normalizeGatewayUsageReport,
  normalizeInitializeResult,
  normalizeMcpCatalogApp,
  normalizeMcpServerStatus,
  jsonObject,
  jsonString,
  type ApprovalDecision,
  type BrowserReply,
  type DoctorReport,
  type FileContent,
  type FileUpload,
  type FileWriteResult,
  type GatewayAccount,
  type GatewayBilling,
  type GatewayCheckout,
  type GatewayContextCredit,
  type GatewayDevicePoll,
  type GatewayDeviceStart,
  type GatewayKeySettings,
  type GatewayPreferenceResult,
  type GatewaySignOutResult,
  type GatewayStatus,
  type GatewayTopUpQuote,
  type GatewayUsageReport,
  type GitBranches,
  type GitCommandOutput,
  type GitCommitMessage,
  type GitCommitResult,
  type GitDiff,
  type GitDiscardResult,
  type GitLogEntry,
  type GitStatus,
  type InitializeResult,
  type JSONObject,
  type JSONValue,
  type LspCompletionResult,
  type LspHover,
  type LspLocation,
  type LspStatus,
  type McpCatalogApp,
  type McpDeviceCode,
  type McpDevicePoll,
  type McpPermissions,
  type McpServerStatus,
  type MemoryEntry,
  type ModelSwitchResult,
  type ModelsList,
  type PriceEntry,
  type ProjectInfo,
  type QuestionAnswer,
  type RewindResult,
  type SessionHistory,
  type SessionHistoryWire,
  type SessionSummary,
  type SpendSummary,
  type StarterSuggestion,
  type SymbolInfo,
  type SymbolsFindMode,
  type TaskRunOptions,
  type TaskSummary,
  type WorktreeEntry,
  type WorktreeMergeResult,
  type WorktreeRemoveResult,
} from './types'

export { decodeNotification, type BackendNotification } from './notifications'

/** `BackendClient.State`: `running` carries the child's pid in `BackendClient.pid`. */
export type BackendState = 'stopped' | 'starting' | 'running' | 'failed'

/** What `start` takes (the `backend_start` command's arguments). */
export interface BackendStartOptions {
  mock: boolean
  /** `--data-dir`; the backend's default when unset. */
  dataDir?: string
  /** The variable the Rust shell fills from the secret store (`model.api_key_env`). */
  apiKeyEnv?: string
  /** `preferences.backendPathOverride` / `--binary`: skip the locator. */
  binaryOverride?: string
}

/** `backend_start`'s result. */
interface BackendStartResult {
  pid: number
  binary: string
  searched: string[]
}

/** `backend:notification`'s payload. */
interface NotificationPayload {
  method: string
  params?: JSONValue
}

/** `backend:exited`'s payload. */
interface ExitedPayload {
  code: number
  tail: string
}

/** `backend:stderr`'s payload. */
interface StderrPayload {
  lines: string[]
}

/** `backend_state`'s result. */
export interface BackendProcessState {
  state: BackendState
  pid?: number | null
  detail?: string | null
}

/// The protocol version this app speaks (`BackendClient.protocolVersion`).
export const PROTOCOL_VERSION = 1

/// `BackendClient.initializeTimeout`, in milliseconds.
export const INITIALIZE_TIMEOUT_MS = 30_000

/// `BackendProcess.stderrRingCapacity`.
const STDERR_RING_CAPACITY = 500

/// How long a fast-failing child gets to report its exit before `start`
/// settles on a transport error (`proc.waitForExit(timeout: 1)`).
const EXIT_GRACE_MS = 1_000

/** `BackendClient`: one instance drives the app's single `ctx serve` child. */
export class BackendClient {
  /** Read it; `start` / `stop` and exits change it (and call `onStateChange`). */
  state: BackendState = 'stopped'
  /// Called after every change of `state` (the Swift class is `@Observable`;
  /// the store mirrors it from here).
  onStateChange?: (state: BackendState) => void
  /** The child's pid while `state === 'running'`. */
  pid: number | null = null
  /** The message of the `failed` state. */
  failureDetail: string | null = null
  /// Delivered in the order the backend sent them. `exited` is sent only
  /// for exits that were not requested by `stop()` and did not happen while
  /// `start()` was still initialising (those are reported through the thrown error).
  onNotification?: (notification: BackendNotification) => void
  /** Last 500 stderr lines of the current (or last) child. */
  stderrLog: string[] = []
  isMock = false
  /** The binary `backend_start` launched. */
  binaryPath: string | null = null
  /** The `initialize` result of the running child. */
  initializeResult: InitializeResult | null = null
  /** Set by `start()` when it throws; cleared on the next successful start. */
  lastStartFailure: BackendFailure | null = null
  /** True from `stop()` until the next `start()`. */
  private stopRequested = false

  private listeners: UnlistenFn[] | null = null
  private listening: Promise<void> | null = null
  private initialized = false
  private processAlive = false
  /** An exit that arrived while `start()` was still initialising. */
  private exitDuringStart: ExitedPayload | null = null
  private exitWaiters: Array<(exit: ExitedPayload) => void> = []

  get isRunning(): boolean {
    return this.state === 'running'
  }

  // MARK: - Lifecycle

  /// Spawns `ctx serve [--mock] [--data-dir DIR]` through `backend_start`,
  /// sends `initialize` and checks the protocol version. Throws
  /// `BackendFailureError` (its `failure` is also `lastStartFailure`): an
  /// incompatible binary is `failure.kind === 'incompatible'`.
  async start(opts: BackendStartOptions): Promise<InitializeResult> {
    if (this.processAlive) await this.stop()
    this.stopRequested = false
    this.initialized = false
    this.exitDuringStart = null
    this.failureDetail = null
    this.isMock = opts.mock
    this.binaryPath = opts.binaryOverride ?? null
    this.initializeResult = null
    this.stderrLog = []
    this.setState('starting')

    // The listeners must be in place before the child can write a frame.
    try {
      await this.ensureListeners()
    } catch (error) {
      return this.failStart(BackendFailure.transport(`Could not listen for backend events: ${errorText(error)}`))
    }

    let launched: BackendStartResult
    try {
      launched = await invoke<BackendStartResult>('backend_start', startArguments(opts))
    } catch (error) {
      const failure = BackendFailure.fromInvoke(error)
      return this.failStart(failure)
    }
    this.processAlive = true
    this.pid = launched.pid
    this.binaryPath = launched.binary
    this.setState('running')
    debug(() => `started ${launched.binary} (pid ${launched.pid})`)

    try {
      const result = await withTimeout(INITIALIZE_TIMEOUT_MS, () => this.initialize(), 'initialize')
      if (result.protocol_version !== PROTOCOL_VERSION) {
        const detail = `ctx ${result.ctx_version} speaks desktop protocol ${result.protocol_version}; this app needs protocol ${PROTOCOL_VERSION}.`
        await this.stop()
        return this.failStart(BackendFailure.incompatible(launched.binary, detail), detail)
      }
      this.initialized = true
      this.initializeResult = result
      this.lastStartFailure = null
      return result
    } catch (error) {
      // The protocol mismatch above already stopped the child and recorded itself.
      if (error instanceof BackendFailureError) throw error
      // Give a fast-failing child a moment to be reaped so its stderr is complete.
      const exit = this.exitDuringStart ?? (await this.waitForExit(EXIT_GRACE_MS))
      const tail = exit?.tail ?? this.stderrLog.join('\n')
      await this.stop()
      if (exit && looksIncompatible(tail)) {
        const detail = `${launched.binary} does not support \`ctx serve\` (exit ${exit.code}): ${firstLine(tail)}`
        return this.failStart(BackendFailure.incompatible(launched.binary, detail), detail)
      }
      if (exit) {
        const message = `ctx exited (${exit.code}) before answering initialize` + (tail.length === 0 ? '' : `: ${firstLine(tail)}`)
        return this.failStart(BackendFailure.exited(exit.code, tail), message)
      }
      const backendError = BackendError.from(error)
      let message: string
      switch (backendError.detail.type) {
        case 'transport':
        case 'decoding':
          message = backendError.detail.message
          break
        case 'rpc':
          message = `initialize failed: ${backendError.detail.message}`
          break
        default:
          message = backendError.message
      }
      return this.failStart(BackendFailure.transport(message), message)
    }
  }

  /// `shutdown` → wait ≤ 2 s → kill, done by the Rust shell's `backend_stop`.
  /// Never throws; the `exited` notification is suppressed for this exit.
  async stop(): Promise<void> {
    this.stopRequested = true
    if (!this.processAlive) {
      this.pid = null
      this.setState('stopped')
      return
    }
    try {
      await invoke<void>('backend_stop')
    } catch (error) {
      debug(() => `backend_stop failed: ${errorText(error)}`)
    }
    this.processAlive = false
    this.initialized = false
    this.pid = null
    this.setState('stopped')
  }

  /** `backend_state`: the Rust shell's view of the child. */
  async processState(): Promise<BackendProcessState> {
    return invoke<BackendProcessState>('backend_state')
  }

  /** `backend_candidates`: the locator's search list, for the failure panel. */
  async candidates(): Promise<string[]> {
    try {
      return await invoke<string[]>('backend_candidates')
    } catch (error) {
      debug(() => `backend_candidates failed: ${errorText(error)}`)
      return []
    }
  }

  /** `backend_stderr`: the Rust shell's own copy of the last 500 stderr lines. */
  async fetchStderr(): Promise<string[]> {
    const lines = await invoke<string[]>('backend_stderr')
    this.stderrLog = lines.slice(-STDERR_RING_CAPACITY)
    return this.stderrLog
  }

  // MARK: - Generic RPC

  /// Raw request; the typed methods below wrap it. Throws `BackendError`
  /// (`notRunning` without a child, `rpc` for an error response, `transport`
  /// when the child went away).
  async call(method: string, params?: JSONValue): Promise<JSONValue> {
    if (!this.processAlive) throw BackendError.notRunning()
    try {
      const result = await invoke<JSONValue | undefined>('rpc_request', { method, params: params ?? null })
      return result ?? null
    } catch (error) {
      throw BackendError.from(error)
    }
  }

  /// Typed request (`request<T: Decodable>`): the `result` subtree, checked
  /// to be an object, then run through `normalize` (the Swift `init(from:)`
  /// defaults) when given. `W` is the wire shape `normalize` takes. Throws
  /// `BackendError.decoding` for a result of the wrong shape.
  private async request<T, W = T>(method: string, params?: JSONValue, normalize?: (value: W) => T): Promise<T> {
    const value = await this.call(method, params)
    if (value === null || typeof value !== 'object' || Array.isArray(value)) {
      throw BackendError.decoding(`${method}: expected an object at root`)
    }
    try {
      return normalize ? normalize(value as unknown as W) : (value as unknown as T)
    } catch (error) {
      throw BackendError.decoding(`${method}: ${errorText(error)}`)
    }
  }

  /** A typed request whose result is a `{ <key>: [...] }` envelope. */
  private async requestList<T>(method: string, key: string, params?: JSONValue, map?: (item: T) => T): Promise<T[]> {
    const envelope = await this.request<Record<string, unknown>>(method, params)
    const list = envelope[key]
    if (!Array.isArray(list)) throw BackendError.decoding(`${method}: missing key "${key}" at root`)
    const items = list as T[]
    try {
      return map ? items.map(map) : items
    } catch (error) {
      throw BackendError.decoding(`${method}: ${errorText(error)}`)
    }
  }

  // MARK: - Methods

  async initialize(): Promise<InitializeResult> {
    return this.request('initialize', undefined, normalizeInitializeResult)
  }

  async configGet(): Promise<InitializeResult> {
    return this.request('config/get', undefined, normalizeInitializeResult)
  }

  /** `config/patch`: dotted keys; `null` removes a key. */
  async configPatch(values: JSONObject): Promise<InitializeResult> {
    return this.request('config/patch', { values }, normalizeInitializeResult)
  }

  /** `initGit` asks the backend to `git init` a folder outside any repository first. */
  async projectOpen(path: string, exact = false, initGit = false): Promise<ProjectInfo> {
    return this.request('project/open', object({ path, exact, init_git: initGit }))
  }

  async projectClose(id: string): Promise<void> {
    await this.call('project/close', object({ project_id: id }))
  }

  async projectStatus(id: string): Promise<ProjectInfo> {
    return this.request('project/status', object({ project_id: id }))
  }

  async indexRebuild(id: string): Promise<void> {
    await this.call('index/rebuild', object({ project_id: id }))
  }

  async sessionsList(projectId: string, limit = 100): Promise<SessionSummary[]> {
    return this.requestList('sessions/list', 'sessions', object({ project_id: projectId, limit }), normalizeSessionSummary)
  }

  async sessionCreate(projectId: string, title?: string | null): Promise<SessionSummary> {
    return this.request('session/create', object({ project_id: projectId, title }), normalizeSessionSummary)
  }

  async sessionRename(projectId: string, sessionId: string, title: string): Promise<SessionSummary> {
    return this.request('session/rename', object({ project_id: projectId, session_id: sessionId, title }), normalizeSessionSummary)
  }

  async sessionDelete(projectId: string, sessionId: string): Promise<void> {
    await this.call('session/delete', object({ project_id: projectId, session_id: sessionId }))
  }

  /// Removes user message `messageId` and everything after it; `restoreFiles`
  /// puts the working tree back to the snapshot taken before it.
  async sessionRewind(projectId: string, sessionId: string, messageId: number, restoreFiles: boolean): Promise<RewindResult> {
    return this.request(
      'session/rewind',
      object({ project_id: projectId, session_id: sessionId, message_id: messageId, restore_files: restoreFiles }),
      normalizeRewindResult,
    )
  }

  async sessionHistory(projectId: string, sessionId: string): Promise<SessionHistory> {
    return this.request<SessionHistory, SessionHistoryWire>(
      'session/history',
      object({ project_id: projectId, session_id: sessionId }),
      normalizeSessionHistory,
    )
  }

  async taskRun(
    projectId: string,
    sessionId: string,
    prompt: string,
    options: TaskRunOptions = {},
    images: string[] = [],
    files: FileUpload[] = [],
  ): Promise<void> {
    await this.call(
      'task/run',
      object({
        project_id: projectId,
        session_id: sessionId,
        prompt,
        images: images.length === 0 ? undefined : images,
        files: files.length === 0 ? undefined : files.map((file) => ({ path: file.path, name: file.name })),
        options: encodeOptions(options),
      }),
    )
  }

  /// Hands a message to `sessionId`'s running turn (read at its next step);
  /// `BackendError.isNoTaskRunning` when the run is idle or ending.
  async taskSteer(projectId: string, sessionId: string, prompt: string, images: string[] = [], files: FileUpload[] = []): Promise<void> {
    await this.call(
      'task/steer',
      object({
        project_id: projectId,
        session_id: sessionId,
        prompt,
        images: images.length === 0 ? undefined : images,
        files: files.length === 0 ? undefined : files.map((file) => ({ path: file.path, name: file.name })),
      }),
    )
  }

  /** Cancels `sessionId`'s run; without one, every run of the project. */
  async taskCancel(projectId: string, sessionId?: string | null): Promise<void> {
    await this.call('task/cancel', object({ project_id: projectId, session_id: sessionId }))
  }

  async approvalRespond(projectId: string, sessionId: string, requestId: string, decision: ApprovalDecision): Promise<void> {
    await this.call('approval/respond', object({ project_id: projectId, session_id: sessionId, request_id: requestId, decision }))
  }

  /** Answers a `browser/request` notification. */
  async browserRespond(projectId: string, sessionId: string, requestId: string, reply: BrowserReply): Promise<void> {
    await this.call(
      'browser/respond',
      object({
        project_id: projectId,
        session_id: sessionId,
        request_id: requestId,
        ok: reply.ok,
        content: reply.content ?? '',
        url: reply.url,
        title: reply.title,
        error: reply.error,
      }),
    )
  }

  /// Answers a `question/request`: one answer per question (null: none), and/or
  /// a free-form message; neither tells the agent the user skipped.
  async questionRespond(
    projectId: string,
    sessionId: string,
    requestId: string,
    answers: QuestionAnswer[] | null,
    message: string | null,
  ): Promise<void> {
    const answerValues = answers?.map((answer) => object({ selected: answer.selected, other: answer.other }))
    await this.call(
      'question/respond',
      object({ project_id: projectId, session_id: sessionId, request_id: requestId, answers: answerValues, message }),
    )
  }

  async permissionsSet(projectId: string, profile: string): Promise<ProjectInfo> {
    return this.request('permissions/set', object({ project_id: projectId, profile }))
  }

  async modelsList(projectId?: string | null, refresh = false): Promise<ModelsList> {
    return this.request('models/list', object({ project_id: projectId, refresh }))
  }

  async modelsSwitch(projectId: string, model: string): Promise<ModelSwitchResult> {
    return this.request('models/switch', object({ project_id: projectId, model }))
  }

  /** `suggest/next`: the user's likely next message, or null. */
  async suggestNext(projectId: string, sessionId: string): Promise<string | null> {
    const result = await this.request<{ suggestion?: string | null }>('suggest/next', object({ project_id: projectId, session_id: sessionId }))
    return result.suggestion ?? null
  }

  /** `suggest/starters`: prompts for the empty chat (empty when the model failed). */
  async suggestStarters(projectId: string): Promise<StarterSuggestion[]> {
    return this.requestList('suggest/starters', 'suggestions', object({ project_id: projectId }))
  }

  async gitStatus(projectId: string): Promise<GitStatus> {
    return this.request('git/status', object({ project_id: projectId }))
  }

  async gitDiff(projectId: string, path?: string | null, staged = false): Promise<GitDiff> {
    return this.request('git/diff', object({ project_id: projectId, path, staged }))
  }

  // MARK: Git actions (Changes tab)

  /** Empty `paths` stages every change. */
  async gitStage(projectId: string, paths: string[]): Promise<void> {
    await this.call('git/stage', object({ project_id: projectId, paths }))
  }

  async gitUnstage(projectId: string, paths: string[]): Promise<void> {
    await this.call('git/unstage', object({ project_id: projectId, paths }))
  }

  /** Tracked files go back to the index, untracked ones are deleted. */
  async gitDiscard(projectId: string, paths: string[]): Promise<GitDiscardResult> {
    return this.request('git/discard', object({ project_id: projectId, paths }))
  }

  /** `all` stages everything first; `paths` commits only those files. */
  async gitCommit(projectId: string, message: string, amend = false, all = false, paths: string[] = []): Promise<GitCommitResult> {
    return this.request('git/commit', object({ project_id: projectId, message, amend, all, paths }))
  }

  /** `git reset --soft HEAD~1`; returns the undone commit. */
  async gitUndoCommit(projectId: string): Promise<GitCommitResult> {
    return this.request('git/undo_commit', object({ project_id: projectId }))
  }

  async gitPush(projectId: string, remote: string | null, branch: string | null, setUpstream: boolean, force: boolean): Promise<GitCommandOutput> {
    return this.request('git/push', object({ project_id: projectId, remote, branch, set_upstream: setUpstream, force }))
  }

  async gitPull(projectId: string, rebase: boolean): Promise<GitCommandOutput> {
    return this.request('git/pull', object({ project_id: projectId, rebase }))
  }

  async gitFetch(projectId: string, prune = true): Promise<GitCommandOutput> {
    return this.request('git/fetch', object({ project_id: projectId, prune }))
  }

  async gitBranches(projectId: string): Promise<GitBranches> {
    return this.request('git/branches', object({ project_id: projectId }))
  }

  /** Returns the branch now checked out. */
  async gitCheckout(projectId: string, branch: string, create: boolean): Promise<string | null> {
    const value = await this.call('git/checkout', object({ project_id: projectId, branch, create }))
    return jsonString(jsonObject(value)?.branch)
  }

  async gitLog(projectId: string, limit = 20): Promise<GitLogEntry[]> {
    return this.requestList('git/log', 'commits', object({ project_id: projectId, limit }))
  }

  async gitCommitMessage(projectId: string): Promise<GitCommitMessage> {
    return this.request('git/commit_message', object({ project_id: projectId }))
  }

  // MARK: Worktrees (`worktree/*`, docs/desktop-protocol.md "Worktrees")

  /// Creates a linked worktree of the project's repository, opens it as a
  /// project and returns that project's info.
  async worktreeCreate(projectId: string, name?: string | null, base?: string | null): Promise<ProjectInfo> {
    return this.request('worktree/create', object({ project_id: projectId, name, base }))
  }

  async worktreeList(projectId: string): Promise<WorktreeEntry[]> {
    return this.requestList('worktree/list', 'worktrees', object({ project_id: projectId }))
  }

  /** Closes the worktree's project and removes the checkout (and its branch on request). */
  async worktreeRemove(projectId: string, deleteBranch: boolean, force: boolean): Promise<WorktreeRemoveResult> {
    return this.request('worktree/remove', object({ project_id: projectId, delete_branch: deleteBranch, force }))
  }

  /// Merges the worktree's branch into the parent checkout (`--no-ff`) and,
  /// with `deleteAfter`, removes the worktree and its branch.
  async worktreeMerge(projectId: string, deleteAfter: boolean): Promise<WorktreeMergeResult> {
    return this.request('worktree/merge', object({ project_id: projectId, delete_after: deleteAfter }))
  }

  // MARK: Files and symbols

  async fileRead(projectId: string, path: string, maxBytes?: number | null): Promise<FileContent> {
    return this.request('file/read', object({ project_id: projectId, path, max_bytes: maxBytes }))
  }

  /** `files/search`: repository paths best matching `query`, for `@` completion. */
  async filesSearch(projectId: string, query: string, limit = 8): Promise<string[]> {
    return this.requestList('files/search', 'files', object({ project_id: projectId, query, limit }))
  }

  /// Replaces the file atomically; `expectedHash` (from `file/read`) makes a
  /// file changed meanwhile a `conflict` error instead of an overwrite.
  async fileWrite(projectId: string, path: string, content: string, expectedHash?: string | null): Promise<FileWriteResult> {
    return this.request('file/write', object({ project_id: projectId, path, content, expected_hash: expectedHash }))
  }

  async symbolsFind(projectId: string, query: string, mode: SymbolsFindMode = 'prefix', limit = 50): Promise<SymbolInfo[]> {
    return this.requestList('symbols/find', 'symbols', object({ project_id: projectId, query, mode, limit }))
  }

  // MARK: Language servers (the editor)

  async lspStatus(projectId: string, path: string): Promise<LspStatus> {
    return this.request('lsp/status', object({ project_id: projectId, path }))
  }

  /** `didOpen` with the buffer; starts the language's server on first use. */
  async lspOpen(projectId: string, path: string, text: string): Promise<LspStatus> {
    return this.request('lsp/open', object({ project_id: projectId, path, text }))
  }

  async lspChange(projectId: string, path: string, text: string): Promise<void> {
    await this.request<{ version?: number | null }>('lsp/change', object({ project_id: projectId, path, text }))
  }

  async lspClose(projectId: string, path: string): Promise<void> {
    await this.call('lsp/close', object({ project_id: projectId, path }))
  }

  /** `line` / `character` are 0-based. */
  async lspCompletion(projectId: string, path: string, line: number, character: number): Promise<LspCompletionResult> {
    return this.request('lsp/completion', object({ project_id: projectId, path, line, character }))
  }

  async lspHover(projectId: string, path: string, line: number, character: number): Promise<LspHover> {
    return this.request('lsp/hover', object({ project_id: projectId, path, line, character }))
  }

  async lspDefinition(projectId: string, path: string, line: number, character: number): Promise<LspLocation[]> {
    return this.requestList('lsp/definition', 'locations', object({ project_id: projectId, path, line, character }))
  }

  // MARK: Tasks, memory, doctor

  async tasksList(projectId: string, limit = 50): Promise<TaskSummary[]> {
    return this.requestList('tasks/list', 'tasks', object({ project_id: projectId, limit }), normalizeTaskSummary)
  }

  async memoryList(projectId: string, category?: string | null, limit = 50): Promise<MemoryEntry[]> {
    return this.requestList('memory/list', 'memories', object({ project_id: projectId, category, limit }))
  }

  async doctorRun(projectId?: string | null, probe = false): Promise<DoctorReport> {
    return this.request('doctor/run', object({ project_id: projectId, probe }))
  }

  // MARK: MCP (`mcp/*`, docs/desktop-protocol.md "MCP servers")

  async mcpStatus(): Promise<McpServerStatus[]> {
    return this.requestList('mcp/status', 'servers', {}, normalizeMcpServerStatus)
  }

  /** Reconnects one server (or every active one) and returns the state afterwards. */
  async mcpReconnect(server?: string | null): Promise<McpServerStatus[]> {
    return this.requestList('mcp/reconnect', 'servers', object({ server }), normalizeMcpServerStatus)
  }

  async mcpCatalog(): Promise<McpCatalogApp[]> {
    return this.requestList('mcp/catalog', 'apps', {}, normalizeMcpCatalogApp)
  }

  /** Installs a catalog app with the user's input values; returns the configuration afterwards. */
  async mcpInstall(id: string, inputs: { [key: string]: string }, excludeTools: string[] = []): Promise<InitializeResult> {
    return this.request('mcp/install', { id, inputs, exclude_tools: excludeTools }, normalizeInitializeResult)
  }

  async mcpUninstall(id: string): Promise<InitializeResult> {
    return this.request('mcp/uninstall', { id }, normalizeInitializeResult)
  }

  async mcpOAuthStart(app: string): Promise<McpDeviceCode> {
    return this.request('mcp/oauth/start', { app })
  }

  async mcpOAuthPoll(app: string, deviceCode: string): Promise<McpDevicePoll> {
    return this.request('mcp/oauth/poll', { app, device_code: deviceCode })
  }

  async mcpOAuthAccount(app: string): Promise<string | null> {
    const result = await this.request<{ account?: string | null }>('mcp/oauth/account', { app })
    return result.account ?? null
  }

  async mcpOAuthSignOut(app: string): Promise<void> {
    await this.call('mcp/oauth/signout', { app })
  }

  async mcpPermissions(): Promise<McpPermissions> {
    return this.request('mcp/permissions', {})
  }

  // MARK: ModelGateway (`gateway/*`, docs/desktop-protocol.md "ModelGateway")

  async gatewayDeviceStart(baseURL: string, deviceName?: string | null): Promise<GatewayDeviceStart> {
    return this.request('gateway/device/start', object({ base_url: baseURL, device_name: deviceName }))
  }

  async gatewayDevicePoll(baseURL: string, deviceCode: string): Promise<GatewayDevicePoll> {
    return this.request('gateway/device/poll', object({ base_url: baseURL, device_code: deviceCode }), (poll) => ({
      ...poll,
      account: poll.account == null ? null : normalizeGatewayAccount(poll.account),
    }))
  }

  /** Whether the backend's credential store holds a device key. */
  async gatewayStatus(): Promise<GatewayStatus> {
    return this.request('gateway/status')
  }

  /** The remaining calls authenticate with that stored key. */
  async gatewayAccount(baseURL: string): Promise<GatewayAccount> {
    return this.request('gateway/account', object({ base_url: baseURL }), normalizeGatewayAccount)
  }

  async gatewayUsage(baseURL: string, startTimestamp?: number | null, endTimestamp?: number | null): Promise<GatewayUsageReport> {
    return this.request(
      'gateway/usage',
      object({ base_url: baseURL, start_timestamp: startTimestamp, end_timestamp: endTimestamp }),
      normalizeGatewayUsageReport,
    )
  }

  /** Revokes the key on the gateway and removes it from the store. */
  async gatewaySignOut(baseURL: string): Promise<GatewaySignOutResult> {
    return this.request('gateway/signout', object({ base_url: baseURL }))
  }

  /** Top-up options, plans on sale and the billing preference. */
  async gatewayBilling(baseURL: string): Promise<GatewayBilling> {
    return this.request('gateway/billing', object({ base_url: baseURL }), normalizeGatewayBilling)
  }

  async gatewayBillingQuote(baseURL: string, amount: number): Promise<GatewayTopUpQuote> {
    return this.request('gateway/billing/quote', object({ base_url: baseURL, amount }))
  }

  /** A checkout page for adding `amount` to the balance. */
  async gatewayBillingTopUp(baseURL: string, amount: number): Promise<GatewayCheckout> {
    return this.request('gateway/billing/topup', object({ base_url: baseURL, amount }))
  }

  /** `method` "stripe" returns a checkout page; "balance" buys at once. */
  async gatewayBillingSubscribe(baseURL: string, planId: number, method: string): Promise<GatewayCheckout> {
    return this.request('gateway/billing/subscribe', object({ base_url: baseURL, plan_id: planId, method }))
  }

  async gatewayBillingPreference(baseURL: string, preference: string): Promise<GatewayPreferenceResult> {
    return this.request('gateway/billing/preference', object({ base_url: baseURL, billing_preference: preference }))
  }

  /** Claims the welcome Context credit; the reply is the new summary. */
  async gatewayClaimContextCredit(baseURL: string): Promise<GatewayContextCredit> {
    return this.request('gateway/context_credit/claim', object({ base_url: baseURL }), normalizeGatewayContextCredit)
  }

  /** The device key's price limits and spending cap. */
  async gatewayKeySettings(baseURL: string): Promise<GatewayKeySettings> {
    return this.request('gateway/key', object({ base_url: baseURL }), normalizeGatewayKeySettings)
  }

  /** `values`: any of `price_limits`, `unlimited_quota`, `remain_quota`. */
  async gatewayUpdateKeySettings(baseURL: string, values: JSONObject): Promise<GatewayKeySettings> {
    return this.request('gateway/key/update', { ...values, base_url: baseURL }, normalizeGatewayKeySettings)
  }

  // MARK: Spending (`spend/summary`, `pricing/lookup`)

  async spendSummary(): Promise<SpendSummary> {
    return this.request('spend/summary')
  }

  async pricingLookup(models: string[]): Promise<PriceEntry[]> {
    return this.requestList('pricing/lookup', 'prices', object({ models }))
  }

  /// Asks the child to exit. The exit that follows counts as requested:
  /// `state` becomes `stopped` and no `exited` notification is sent.
  /// Prefer `stop()`, which also escalates when the child does not comply.
  async shutdown(): Promise<void> {
    if (!this.processAlive) throw BackendError.notRunning()
    this.stopRequested = true
    await this.call('shutdown')
  }

  // MARK: - Events from the Rust shell

  /// Subscribes once to the three backend events. Tauri delivers each
  /// listener's events in emission order, so notifications reach
  /// `onNotification` in frame order.
  private ensureListeners(): Promise<void> {
    if (this.listeners) return Promise.resolve()
    if (this.listening) return this.listening
    this.listening = (async () => {
      const unlisten = await Promise.all([
        listen<NotificationPayload>('backend:notification', (event) => {
          const notification = decodeNotification(event.payload.method, event.payload.params)
          if (notification) this.deliver(notification)
        }),
        listen<ExitedPayload>('backend:exited', (event) => this.handleExit(event.payload)),
        listen<StderrPayload>('backend:stderr', (event) => this.appendStderr(event.payload.lines)),
      ])
      this.listeners = unlisten
      this.listening = null
    })()
    return this.listening
  }

  private deliver(notification: BackendNotification): void {
    try {
      this.onNotification?.(notification)
    } catch (error) {
      debug(() => `onNotification threw: ${errorText(error)}`)
    }
  }

  /// `BackendClient.handleExit(of:code:)`: decides whether an `exited`
  /// notification is delivered. A requested stop is silent; an exit before
  /// `initialize` answered is reported through `start()`'s error instead.
  private handleExit(exit: ExitedPayload): void {
    debug(() => `ctx exited (${exit.code})`)
    this.processAlive = false
    this.pid = null
    for (const waiter of this.exitWaiters.splice(0)) waiter(exit)
    if (this.stopRequested) {
      this.setState('stopped')
      return
    }
    if (!this.initialized) {
      this.exitDuringStart = exit
      return
    }
    this.initialized = false
    this.failureDetail = `ctx exited (${exit.code})`
    this.setState('failed')
    this.deliver({ kind: 'exited', code: exit.code, tail: exit.tail })
  }

  /** Resolves with the exit that arrives within `ms`, else `null`. */
  private waitForExit(ms: number): Promise<ExitedPayload | null> {
    if (!this.processAlive) return Promise.resolve(this.exitDuringStart)
    return new Promise((resolve) => {
      const timer = setTimeout(() => {
        this.exitWaiters = this.exitWaiters.filter((waiter) => waiter !== onExit)
        resolve(null)
      }, ms)
      const onExit = (exit: ExitedPayload) => {
        clearTimeout(timer)
        resolve(exit)
      }
      this.exitWaiters.push(onExit)
    })
  }

  private appendStderr(lines: string[]): void {
    this.stderrLog.push(...lines)
    if (this.stderrLog.length > STDERR_RING_CAPACITY) {
      this.stderrLog.splice(0, this.stderrLog.length - STDERR_RING_CAPACITY)
    }
  }

  /// Records a start failure (`lastStartFailure`, `state = .failed(message)`)
  /// and throws it as a `BackendFailureError`.
  private failStart(failure: BackendFailure, message?: string): never {
    const error = new BackendFailureError(failure)
    this.lastStartFailure = failure
    this.processAlive = false
    this.pid = null
    this.failureDetail = message ?? error.message
    this.setState('failed')
    throw error
  }

  /** Sets `state` and tells `onStateChange` (also for a repeated `failed`, whose detail changed). */
  private setState(state: BackendState): void {
    this.state = state
    try {
      this.onStateChange?.(state)
    } catch (error) {
      debug(() => `onStateChange threw: ${errorText(error)}`)
    }
  }
}

// MARK: - Helpers

/// `BackendClient.object(_:)`: drops `undefined` / `null` members, as the
/// Swift `compactMapValues` does, so optional parameters are omitted.
function object(pairs: { [key: string]: JSONValue | undefined }): JSONObject {
  const out: JSONObject = {}
  for (const [key, value] of Object.entries(pairs)) {
    if (value !== undefined && value !== null) out[key] = value
  }
  return out
}

/** `TaskRunOptions` with unset fields omitted (Swift's `encode`). */
function encodeOptions(options: TaskRunOptions): JSONObject {
  return object({
    max_steps: options.max_steps,
    proactive_retrieval: options.proactive_retrieval,
    memory_context: options.memory_context,
    stream: options.stream,
    debug_prompt: options.debug_prompt,
  })
}

/// The `backend_start` arguments. ARCHITECTURE.md names them in snake_case;
/// Tauri's default command binding reads camelCase keys, so both spellings
/// are sent and the extra ones are ignored.
function startArguments(opts: BackendStartOptions): Record<string, unknown> {
  return {
    mock: opts.mock,
    binary_override: opts.binaryOverride ?? null,
    binaryOverride: opts.binaryOverride ?? null,
    data_dir: opts.dataDir ?? null,
    dataDir: opts.dataDir ?? null,
    api_key_env: opts.apiKeyEnv ?? null,
    apiKeyEnv: opts.apiKeyEnv ?? null,
  }
}

/// `BackendClient.looksIncompatible(stderr:)`: a binary too old for `serve`.
export function looksIncompatible(stderr: string): boolean {
  const lowered = stderr.toLowerCase()
  return lowered.includes('unrecognized subcommand') || lowered.includes('unexpected argument')
}

function firstLine(text: string): string {
  return text.split(/\r?\n/).find((line) => line.trim().length > 0) ?? ''
}

/** `BackendClient.withTimeout`: races `operation` against a timer. */
function withTimeout<T>(ms: number, operation: () => Promise<T>, what: string): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => {
      debug(() => `${what} timed out`)
      reject(BackendError.transport(`ctx did not answer within ${Math.round(ms / 1000)} s`))
    }, ms)
    operation().then(
      (value) => {
        clearTimeout(timer)
        resolve(value)
      },
      (error: unknown) => {
        clearTimeout(timer)
        reject(error instanceof Error ? error : new Error(errorText(error)))
      },
    )
  })
}

/** The app's single client (`AppState.backend`). */
export const backend = new BackendClient()
